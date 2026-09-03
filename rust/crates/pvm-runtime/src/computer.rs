/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use crate::corevm::{Interruption, Vm};
use anyhow::{anyhow, bail, Context, Result};
use polkavm::ProgramBlob;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

/// Version of the experimental Polkadot Host application-computer contract.
pub const COMPUTER_ABI_VERSION: (u16, u16) = (0, 1);

/// Maximum encoded argument or environment record accepted at launch.
pub const MAX_COMPUTER_CONTEXT_BYTES: usize = 64 * 1024;

/// Maximum number of arguments or environment entries accepted at launch.
pub const MAX_COMPUTER_CONTEXT_ENTRIES: usize = 1_024;

/// Terminal handle granted to every computer guest.
pub const COMPUTER_TTY_HANDLE: u32 = 1;
/// Raw (non-canonical) terminal input mode flag.
pub const TTY_MODE_RAW: u32 = 1;
/// Terminal echo mode flag.
pub const TTY_MODE_ECHO: u32 = 2;
/// Open flag granting read access.
pub const FS_OPEN_READ: u32 = 1;
/// Open flag granting write access.
pub const FS_OPEN_WRITE: u32 = 2;
/// Open flag creating a missing file when writable.
pub const FS_OPEN_CREATE: u32 = 4;
/// Open flag truncating an existing writable file.
pub const FS_OPEN_TRUNCATE: u32 = 8;

/// Maximum bytes queued toward the guest terminal.
pub const MAX_TTY_INPUT_BYTES: usize = 64 * 1024;
/// Maximum guest terminal output retained per run.
pub const MAX_TTY_OUTPUT_BYTES: usize = 1024 * 1024;
/// Maximum files in the mounted computer filesystem.
pub const MAX_COMPUTER_FILES: usize = 64;
/// Maximum size of one mounted file.
pub const MAX_COMPUTER_FILE_BYTES: usize = 1024 * 1024;
/// Maximum simultaneously open computer file handles.
pub const MAX_OPEN_COMPUTER_FILES: usize = 16;
/// Maximum accepted file path length in bytes.
pub const MAX_COMPUTER_PATH_BYTES: usize = 200;

pub(crate) const STATUS_WOULD_BLOCK: i32 = -1;
pub(crate) const STATUS_BAD_HANDLE: i32 = -2;
pub(crate) const STATUS_INVALID: i32 = -3;
pub(crate) const STATUS_NOT_FOUND: i32 = -4;
pub(crate) const STATUS_DENIED: i32 = -5;
pub(crate) const STATUS_LIMIT: i32 = -6;

/// Launch context exposed through `polkadot-host-computer/0.1/core`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComputerContext {
    pub(crate) arguments: Vec<String>,
    pub(crate) environment: Vec<(String, String)>,
    pub(crate) encoded_arguments: Vec<u8>,
    pub(crate) encoded_environment: Vec<u8>,
}

impl ComputerContext {
    /// Validates and encodes an application-computer launch context.
    pub fn new(arguments: Vec<String>, environment: Vec<(String, String)>) -> Result<Self> {
        if arguments.len() > MAX_COMPUTER_CONTEXT_ENTRIES {
            bail!("computer argument count exceeds the host limit");
        }
        if environment.len() > MAX_COMPUTER_CONTEXT_ENTRIES {
            bail!("computer environment count exceeds the host limit");
        }

        for argument in &arguments {
            if argument.as_bytes().contains(&0) {
                bail!("computer arguments must not contain NUL bytes");
            }
        }

        let mut keys = BTreeSet::new();
        for (key, value) in &environment {
            if key.is_empty() || key.contains('=') || key.as_bytes().contains(&0) {
                bail!(
                    "computer environment keys must be non-empty and contain neither '=' nor NUL"
                );
            }
            if value.as_bytes().contains(&0) {
                bail!("computer environment values must not contain NUL bytes");
            }
            if !keys.insert(key.as_str()) {
                bail!("computer environment contains duplicate key {key:?}");
            }
        }

        let encoded_arguments = encode_arguments(&arguments)?;
        let encoded_environment = encode_environment(&environment)?;
        Ok(Self {
            arguments,
            environment,
            encoded_arguments,
            encoded_environment,
        })
    }

    /// Returns launch arguments in guest-visible order.
    pub fn arguments(&self) -> &[String] {
        &self.arguments
    }

    /// Returns launch environment entries in guest-visible order.
    pub fn environment(&self) -> &[(String, String)] {
        &self.environment
    }
}

impl Default for ComputerContext {
    fn default() -> Self {
        Self::new(Vec::new(), Vec::new()).expect("an empty computer context is valid")
    }
}

/// Observable state after running an application-computer guest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComputerStatus {
    /// The guest yielded control and may be resumed.
    Yielded,
    /// The guest exited with the supplied application status.
    Exited(i32),
    /// The guest requested a child process; the supervisor must resolve it.
    SpawnRequested,
}

struct OpenComputerFile {
    path: String,
    position: usize,
    readable: bool,
    writable: bool,
}

/// Terminal and filesystem devices granted to one computer guest.
pub(crate) struct ComputerDevices {
    tty_input: VecDeque<u8>,
    tty_output: Vec<u8>,
    tty_columns: u32,
    tty_rows: u32,
    tty_mode: u32,
    files: BTreeMap<String, Vec<u8>>,
    modified: BTreeSet<String>,
    open_files: BTreeMap<u32, OpenComputerFile>,
    next_handle: u32,
}

impl ComputerDevices {
    pub(crate) fn new() -> Self {
        Self {
            tty_input: VecDeque::new(),
            tty_output: Vec::new(),
            tty_columns: 80,
            tty_rows: 24,
            tty_mode: TTY_MODE_ECHO,
            files: BTreeMap::new(),
            modified: BTreeSet::new(),
            open_files: BTreeMap::new(),
            next_handle: 16,
        }
    }

    fn push_terminal_input(&mut self, bytes: &[u8]) -> Result<()> {
        if self.tty_input.len().saturating_add(bytes.len()) > MAX_TTY_INPUT_BYTES {
            bail!("terminal input queue limit exceeded");
        }
        self.tty_input.extend(bytes.iter().copied());
        Ok(())
    }

    fn take_terminal_output(&mut self) -> Option<Vec<u8>> {
        if self.tty_output.is_empty() {
            return None;
        }
        Some(core::mem::take(&mut self.tty_output))
    }

    fn mount_file(&mut self, path: &str, bytes: Vec<u8>) -> Result<()> {
        if validate_computer_path(path).is_none() {
            bail!("invalid computer file path {path:?}");
        }
        if bytes.len() > MAX_COMPUTER_FILE_BYTES {
            bail!("mounted file {path:?} exceeds {MAX_COMPUTER_FILE_BYTES} bytes");
        }
        if !self.files.contains_key(path) && self.files.len() == MAX_COMPUTER_FILES {
            bail!("computer filesystem file limit exceeded");
        }
        self.files.insert(path.to_owned(), bytes);
        Ok(())
    }

    fn take_modified_files(&mut self) -> Vec<(String, Vec<u8>)> {
        let modified = core::mem::take(&mut self.modified);
        modified
            .into_iter()
            .filter_map(|path| {
                let bytes = self.files.get(&path)?.clone();
                Some((path, bytes))
            })
            .collect()
    }

    pub(crate) fn has_terminal_input(&self) -> bool {
        !self.tty_input.is_empty()
    }

    pub(crate) fn terminal_mode(&self) -> u32 {
        self.tty_mode
    }

    pub(crate) fn terminal_size(&self) -> (u32, u32) {
        (self.tty_columns, self.tty_rows)
    }

    pub(crate) fn tty_read_into(&mut self, handle: u32, buffer: &mut [u8]) -> i32 {
        if handle != COMPUTER_TTY_HANDLE {
            return STATUS_BAD_HANDLE;
        }
        if buffer.is_empty() {
            return STATUS_INVALID;
        }
        if self.tty_input.is_empty() {
            return STATUS_WOULD_BLOCK;
        }
        let mut written = 0usize;
        while written < buffer.len() {
            let Some(byte) = self.tty_input.pop_front() else {
                break;
            };
            buffer[written] = byte;
            written += 1;
        }
        written as i32
    }

    pub(crate) fn tty_write(&mut self, handle: u32, bytes: &[u8]) -> i32 {
        if handle != COMPUTER_TTY_HANDLE {
            return STATUS_BAD_HANDLE;
        }
        let available = MAX_TTY_OUTPUT_BYTES.saturating_sub(self.tty_output.len());
        let written = bytes.len().min(available);
        self.tty_output.extend_from_slice(&bytes[..written]);
        written as i32
    }

    pub(crate) fn tty_set_mode(&mut self, handle: u32, flags: u32) -> i32 {
        if handle != COMPUTER_TTY_HANDLE {
            return STATUS_BAD_HANDLE;
        }
        if flags & !(TTY_MODE_RAW | TTY_MODE_ECHO) != 0 {
            return STATUS_INVALID;
        }
        self.tty_mode = flags;
        0
    }

    pub(crate) fn fs_open(&mut self, path: &str, flags: u32) -> i32 {
        let Some(path) = validate_computer_path(path) else {
            return STATUS_INVALID;
        };
        let readable = flags & FS_OPEN_READ != 0;
        let writable = flags & FS_OPEN_WRITE != 0;
        if !readable && !writable {
            return STATUS_INVALID;
        }
        if self.open_files.len() == MAX_OPEN_COMPUTER_FILES {
            return STATUS_LIMIT;
        }
        if !self.files.contains_key(path) {
            if !(writable && flags & FS_OPEN_CREATE != 0) {
                return STATUS_NOT_FOUND;
            }
            if self.files.len() == MAX_COMPUTER_FILES {
                return STATUS_LIMIT;
            }
            self.files.insert(path.to_owned(), Vec::new());
            self.modified.insert(path.to_owned());
        } else if writable && flags & FS_OPEN_TRUNCATE != 0 {
            self.files.insert(path.to_owned(), Vec::new());
            self.modified.insert(path.to_owned());
        }
        let handle = self.next_handle;
        let Some(next) = handle.checked_add(1) else {
            return STATUS_LIMIT;
        };
        self.next_handle = next;
        self.open_files.insert(
            handle,
            OpenComputerFile {
                path: path.to_owned(),
                position: 0,
                readable,
                writable,
            },
        );
        handle as i32
    }

    pub(crate) fn fs_read(&mut self, handle: u32, buffer: &mut [u8]) -> i32 {
        let Some(open) = self.open_files.get_mut(&handle) else {
            return STATUS_BAD_HANDLE;
        };
        if !open.readable {
            return STATUS_DENIED;
        }
        let Some(file) = self.files.get(&open.path) else {
            return STATUS_NOT_FOUND;
        };
        let start = open.position.min(file.len());
        let length = buffer.len().min(file.len() - start);
        buffer[..length].copy_from_slice(&file[start..start + length]);
        open.position = start + length;
        length as i32
    }

    pub(crate) fn fs_write(&mut self, handle: u32, bytes: &[u8]) -> i32 {
        let Some(open) = self.open_files.get_mut(&handle) else {
            return STATUS_BAD_HANDLE;
        };
        if !open.writable {
            return STATUS_DENIED;
        }
        let Some(file) = self.files.get_mut(&open.path) else {
            return STATUS_NOT_FOUND;
        };
        let end = open.position.saturating_add(bytes.len());
        if end > MAX_COMPUTER_FILE_BYTES {
            return STATUS_LIMIT;
        }
        if file.len() < end {
            file.resize(end, 0);
        }
        file[open.position..end].copy_from_slice(bytes);
        open.position = end;
        self.modified.insert(open.path.clone());
        bytes.len() as i32
    }

    pub(crate) fn fs_seek(&mut self, handle: u32, offset: i32, whence: u32) -> i32 {
        let Some(open) = self.open_files.get_mut(&handle) else {
            return STATUS_BAD_HANDLE;
        };
        let Some(file) = self.files.get(&open.path) else {
            return STATUS_NOT_FOUND;
        };
        let base = match whence {
            0 => 0i64,
            1 => open.position as i64,
            2 => file.len() as i64,
            _ => return STATUS_INVALID,
        };
        let position = base + i64::from(offset);
        if !(0..=MAX_COMPUTER_FILE_BYTES as i64).contains(&position) {
            return STATUS_INVALID;
        }
        open.position = position as usize;
        position as i32
    }

    pub(crate) fn fs_truncate(&mut self, handle: u32, length: u32) -> i32 {
        let length = length as usize;
        if length > MAX_COMPUTER_FILE_BYTES {
            return STATUS_LIMIT;
        }
        let Some(open) = self.open_files.get_mut(&handle) else {
            return STATUS_BAD_HANDLE;
        };
        if !open.writable {
            return STATUS_DENIED;
        }
        let Some(file) = self.files.get_mut(&open.path) else {
            return STATUS_NOT_FOUND;
        };
        file.resize(length, 0);
        self.modified.insert(open.path.clone());
        0
    }

    pub(crate) fn fs_stat(&self, path: &str) -> Option<u32> {
        let path = validate_computer_path(path)?;
        self.files.get(path).map(|file| file.len() as u32)
    }

    pub(crate) fn fs_sync(&mut self, handle: u32) -> i32 {
        if self.open_files.contains_key(&handle) {
            0
        } else {
            STATUS_BAD_HANDLE
        }
    }

    pub(crate) fn fs_close(&mut self, handle: u32) -> i32 {
        if self.open_files.remove(&handle).is_some() {
            0
        } else {
            STATUS_BAD_HANDLE
        }
    }

    /// Encodes the mounted file paths as a length-delimited record.
    pub(crate) fn fs_list_record(&self) -> Vec<u8> {
        let mut record = Vec::with_capacity(64);
        record.extend_from_slice(&(self.files.len() as u32).to_le_bytes());
        for path in self.files.keys() {
            record.extend_from_slice(&(path.len() as u32).to_le_bytes());
            record.extend_from_slice(path.as_bytes());
        }
        record
    }
}

fn validate_computer_path(path: &str) -> Option<&str> {
    if path.len() > MAX_COMPUTER_PATH_BYTES
        || !path.starts_with("/home/")
        || path.ends_with('/')
        || path.bytes().any(|byte| byte == 0)
        || path
            .split('/')
            .any(|segment| segment == "." || segment == "..")
    {
        return None;
    }
    Some(path)
}

/// Experimental host-neutral runtime for `polkadot-host-computer/0.1` guests.
pub struct ComputerRuntime {
    vm: Vm,
    max_gas_per_run: u64,
    exit_status: Option<i32>,
    pending_spawn: Option<(String, Vec<String>)>,
}

impl ComputerRuntime {
    /// Creates a runtime using the preferred backend for this platform.
    pub fn new(program: &[u8], context: ComputerContext, max_gas_per_run: u64) -> Result<Self> {
        Self::new_with_backend(
            program,
            context,
            max_gas_per_run,
            crate::preferred_backend(),
        )
    }

    /// Creates a runtime using an explicitly selected PolkaVM backend.
    pub fn new_with_backend(
        program: &[u8],
        context: ComputerContext,
        max_gas_per_run: u64,
        backend: polkavm::BackendKind,
    ) -> Result<Self> {
        crate::validate_launch_inputs(program, &HashMap::new(), max_gas_per_run)?;
        let blob = ProgramBlob::parse(program.into()).context("parse PolkaVM computer program")?;
        crate::validate_blob(&blob)?;
        if !blob.exports().any(|export| export.symbol() == "_pvm_start") {
            bail!("computer guests must export '_pvm_start'");
        }

        let mut vm = Vm::from_blob(blob, backend).context("create PolkaVM computer guest")?;
        vm.setup(context).map_err(|error| anyhow!(error))?;
        Ok(Self {
            vm,
            max_gas_per_run,
            exit_status: None,
            pending_spawn: None,
        })
    }

    /// Runs until the guest yields, exits, or fails.
    pub fn run(&mut self) -> Result<ComputerStatus> {
        if let Some(status) = self.exit_status {
            return Ok(ComputerStatus::Exited(status));
        }

        self.vm.set_gas(self.max_gas_per_run);
        match self.vm.run().map_err(|error| anyhow!(error))? {
            Interruption::Exit(status) => {
                self.exit_status = Some(status);
                Ok(ComputerStatus::Exited(status))
            }
            Interruption::Yield => Ok(ComputerStatus::Yielded),
            Interruption::ProcessRun { package, arguments } => {
                self.pending_spawn = Some((package, arguments));
                Ok(ComputerStatus::SpawnRequested)
            }
            Interruption::SetPalette { .. }
            | Interruption::Display { .. }
            | Interruption::AudioInit { .. }
            | Interruption::AudioFrame { .. } => {
                bail!("computer guest requested an application-presentation operation")
            }
        }
    }

    /// Returns the selected execution backend.
    pub fn backend(&self) -> polkavm::BackendKind {
        self.vm.backend()
    }

    /// Takes the pending spawn request after `SpawnRequested`.
    pub fn take_spawn_request(&mut self) -> Option<(String, Vec<String>)> {
        self.pending_spawn.take()
    }

    /// Completes a pending spawn with the child's exit status or an error.
    pub fn resolve_spawn(&mut self, result: i32) {
        self.vm.resolve_process_run(result);
    }

    /// Queues keyboard bytes toward the guest terminal.
    pub fn send_terminal_input(&mut self, bytes: &[u8]) -> Result<()> {
        self.vm.computer.push_terminal_input(bytes)
    }

    /// Drains ANSI output produced by the guest terminal.
    pub fn take_terminal_output(&mut self) -> Option<Vec<u8>> {
        self.vm.computer.take_terminal_output()
    }

    /// Returns whether undelivered terminal input remains queued.
    pub fn has_terminal_input(&self) -> bool {
        self.vm.computer.has_terminal_input()
    }

    /// Sets the terminal dimensions observed by the guest.
    pub fn set_terminal_size(&mut self, columns: u32, rows: u32) -> Result<()> {
        if columns == 0 || rows == 0 || columns > 1_000 || rows > 1_000 {
            bail!("invalid terminal size {columns}x{rows}");
        }
        self.vm.computer.tty_columns = columns;
        self.vm.computer.tty_rows = rows;
        Ok(())
    }

    /// Returns the current guest terminal mode flags.
    pub fn terminal_mode(&self) -> u32 {
        self.vm.computer.terminal_mode()
    }

    /// Mounts one file into the guest `/home` filesystem.
    pub fn mount_file(&mut self, path: &str, bytes: Vec<u8>) -> Result<()> {
        self.vm.computer.mount_file(path, bytes)
    }

    /// Drains files modified by the guest since the previous call.
    pub fn take_modified_files(&mut self) -> Vec<(String, Vec<u8>)> {
        self.vm.computer.take_modified_files()
    }

    /// Returns the recorded exit status, when the guest has exited.
    pub fn exit_status(&self) -> Option<i32> {
        self.exit_status
    }
}

/// Maximum depth of the foreground process stack.
pub const MAX_COMPUTER_PROCESSES: usize = 4;

/// Supervises a stack of computer processes sharing one terminal and `/home`.
///
/// The Host owns every child VM: guests request packages by name through
/// `process_run`, and only packages registered by the Host can be launched.
/// The foreground process (top of the stack) owns terminal input; a parent
/// stays suspended inside its `process_run` hostcall until the child exits.
pub struct ComputerSupervisor {
    packages: BTreeMap<String, Vec<u8>>,
    stack: Vec<ComputerRuntime>,
    files: BTreeMap<String, Vec<u8>>,
    modified: BTreeMap<String, Vec<u8>>,
    backend: polkavm::BackendKind,
    max_gas_per_run: u64,
    columns: u32,
    rows: u32,
}

impl ComputerSupervisor {
    /// Creates a supervisor whose root process runs `program`.
    pub fn new(program: &[u8], context: ComputerContext, max_gas_per_run: u64) -> Result<Self> {
        Self::new_with_backend(
            program,
            context,
            max_gas_per_run,
            crate::preferred_backend(),
        )
    }

    /// Creates a supervisor using an explicitly selected PolkaVM backend.
    pub fn new_with_backend(
        program: &[u8],
        context: ComputerContext,
        max_gas_per_run: u64,
        backend: polkavm::BackendKind,
    ) -> Result<Self> {
        let root = ComputerRuntime::new_with_backend(program, context, max_gas_per_run, backend)?;
        Ok(Self {
            packages: BTreeMap::new(),
            stack: vec![root],
            files: BTreeMap::new(),
            modified: BTreeMap::new(),
            backend,
            max_gas_per_run,
            columns: 80,
            rows: 24,
        })
    }

    /// Registers a launchable package under a Host-authorized name.
    pub fn register_package(&mut self, name: &str, program: Vec<u8>) -> Result<()> {
        if name.is_empty()
            || name.len() > 64
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            bail!("invalid package name {name:?}");
        }
        self.packages.insert(name.to_owned(), program);
        Ok(())
    }

    /// Mounts one persistent file into the shared `/home` store.
    pub fn mount_file(&mut self, path: &str, bytes: Vec<u8>) -> Result<()> {
        self.foreground().mount_file(path, bytes.clone())?;
        self.files.insert(path.to_owned(), bytes);
        Ok(())
    }

    /// Sets the terminal size observed by every process.
    pub fn set_terminal_size(&mut self, columns: u32, rows: u32) -> Result<()> {
        for process in &mut self.stack {
            process.set_terminal_size(columns, rows)?;
        }
        self.columns = columns;
        self.rows = rows;
        Ok(())
    }

    /// Queues keyboard bytes toward the foreground process.
    pub fn send_terminal_input(&mut self, bytes: &[u8]) -> Result<()> {
        self.foreground().send_terminal_input(bytes)
    }

    /// Drains ANSI output produced by the foreground process.
    pub fn take_terminal_output(&mut self) -> Option<Vec<u8>> {
        self.foreground().take_terminal_output()
    }

    /// Returns whether undelivered terminal input remains queued.
    pub fn has_terminal_input(&self) -> bool {
        self.stack
            .last()
            .is_some_and(ComputerRuntime::has_terminal_input)
    }

    /// Drains files modified by any process since the previous call.
    pub fn take_modified_files(&mut self) -> Vec<(String, Vec<u8>)> {
        core::mem::take(&mut self.modified).into_iter().collect()
    }

    /// Runs the foreground process until the system yields or the root exits.
    pub fn run(&mut self) -> Result<ComputerStatus> {
        loop {
            let status = self.foreground().run()?;
            self.collect_modified();
            match status {
                ComputerStatus::Yielded => return Ok(ComputerStatus::Yielded),
                ComputerStatus::SpawnRequested => {
                    let request = self.foreground().take_spawn_request();
                    let Some((package, arguments)) = request else {
                        bail!("spawn status without a pending request");
                    };
                    match self.spawn_child(&package, arguments) {
                        Ok(child) => self.stack.push(child),
                        Err(status) => self.foreground().resolve_spawn(status),
                    }
                }
                ComputerStatus::Exited(code) => {
                    // The exited root stays resident so terminal accessors
                    // remain valid; rerunning it reports the same status.
                    if self.stack.len() == 1 {
                        return Ok(ComputerStatus::Exited(code));
                    }
                    self.stack.pop();
                    let parent = self.foreground();
                    parent.resolve_spawn(code.clamp(-128, 255));
                    // The child may have changed shared files while the
                    // parent held stale copies; refresh the parent's view.
                    let files = self.files.clone();
                    for (path, bytes) in files {
                        self.foreground().mount_file(&path, bytes)?;
                    }
                }
            }
        }
    }

    /// Host-authority cancellation of the foreground process.
    ///
    /// A child is discarded and its parent resumes with status 130
    /// (interrupted). Terminating the root ends the whole computer.
    pub fn terminate_foreground(&mut self) -> Result<ComputerStatus> {
        self.collect_modified();
        if self.stack.len() == 1 {
            return Ok(ComputerStatus::Exited(130));
        }
        self.stack.pop();
        self.foreground().resolve_spawn(130);
        let files = self.files.clone();
        for (path, bytes) in files {
            self.foreground().mount_file(&path, bytes)?;
        }
        Ok(ComputerStatus::Yielded)
    }

    fn foreground(&mut self) -> &mut ComputerRuntime {
        self.stack
            .last_mut()
            .expect("supervisor stack is never empty")
    }

    fn collect_modified(&mut self) {
        let changed = self.foreground().take_modified_files();
        for (path, bytes) in changed {
            self.files.insert(path.clone(), bytes.clone());
            self.modified.insert(path, bytes);
        }
    }

    fn spawn_child(
        &mut self,
        package: &str,
        arguments: Vec<String>,
    ) -> Result<ComputerRuntime, i32> {
        if self.stack.len() >= MAX_COMPUTER_PROCESSES {
            return Err(STATUS_LIMIT);
        }
        let Some(program) = self.packages.get(package) else {
            return Err(STATUS_NOT_FOUND);
        };
        let mut argv = Vec::with_capacity(arguments.len() + 1);
        argv.push(package.to_owned());
        argv.extend(arguments);
        let context = ComputerContext::new(argv, Vec::new()).map_err(|_| STATUS_INVALID)?;
        let mut child =
            ComputerRuntime::new_with_backend(program, context, self.max_gas_per_run, self.backend)
                .map_err(|_| STATUS_INVALID)?;
        child
            .set_terminal_size(self.columns, self.rows)
            .map_err(|_| STATUS_INVALID)?;
        for (path, bytes) in &self.files {
            child
                .mount_file(path, bytes.clone())
                .map_err(|_| STATUS_LIMIT)?;
        }
        Ok(child)
    }
}

fn encode_arguments(arguments: &[String]) -> Result<Vec<u8>> {
    let mut output = encoded_record(arguments.len())?;
    for argument in arguments {
        push_bytes(&mut output, argument.as_bytes())?;
    }
    Ok(output)
}

fn encode_environment(environment: &[(String, String)]) -> Result<Vec<u8>> {
    let mut output = encoded_record(environment.len())?;
    for (key, value) in environment {
        push_bytes(&mut output, key.as_bytes())?;
        push_bytes(&mut output, value.as_bytes())?;
    }
    Ok(output)
}

fn encoded_record(count: usize) -> Result<Vec<u8>> {
    let count = u32::try_from(count).context("computer context entry count overflow")?;
    Ok(count.to_le_bytes().to_vec())
}

fn push_bytes(output: &mut Vec<u8>, bytes: &[u8]) -> Result<()> {
    let length = u32::try_from(bytes.len()).context("computer context field length overflow")?;
    let required = output
        .len()
        .checked_add(4)
        .and_then(|length| length.checked_add(bytes.len()))
        .ok_or_else(|| anyhow!("computer context length overflow"))?;
    if required > MAX_COMPUTER_CONTEXT_BYTES {
        bail!("encoded computer context exceeds the host limit");
    }
    output.extend_from_slice(&length.to_le_bytes());
    output.extend_from_slice(bytes);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_encoding_is_length_delimited_and_ordered() {
        let context = ComputerContext::new(
            vec!["shell.polkavm".into(), "--login".into()],
            vec![
                ("HOME".into(), "/home".into()),
                ("TERM".into(), "pvm-tty".into()),
            ],
        )
        .unwrap();

        assert_eq!(
            context.encoded_arguments,
            b"\x02\0\0\0\x0d\0\0\0shell.polkavm\x07\0\0\0--login"
        );
        assert_eq!(
            context.encoded_environment,
            b"\x02\0\0\0\x04\0\0\0HOME\x05\0\0\0/home\x04\0\0\0TERM\x07\0\0\0pvm-tty"
        );
    }

    #[test]
    fn context_rejects_ambiguous_environment() {
        assert!(ComputerContext::new(Vec::new(), vec![("".into(), "value".into())]).is_err());
        assert!(ComputerContext::new(Vec::new(), vec![("A=B".into(), "value".into())]).is_err());
        assert!(ComputerContext::new(
            Vec::new(),
            vec![("HOME".into(), "one".into()), ("HOME".into(), "two".into())]
        )
        .is_err());
    }

    #[test]
    fn terminal_reads_block_until_input_arrives() {
        let mut devices = ComputerDevices::new();
        let mut buffer = [0u8; 4];
        assert_eq!(
            devices.tty_read_into(COMPUTER_TTY_HANDLE, &mut buffer),
            STATUS_WOULD_BLOCK
        );
        devices.push_terminal_input(b"hi").unwrap();
        assert_eq!(devices.tty_read_into(COMPUTER_TTY_HANDLE, &mut buffer), 2);
        assert_eq!(&buffer[..2], b"hi");
        assert_eq!(devices.tty_read_into(2, &mut buffer), STATUS_BAD_HANDLE);
    }

    #[test]
    fn terminal_output_is_drained_by_the_host() {
        let mut devices = ComputerDevices::new();
        assert_eq!(devices.tty_write(COMPUTER_TTY_HANDLE, b"\x1b[2J"), 4);
        assert_eq!(
            devices.take_terminal_output().as_deref(),
            Some(b"\x1b[2J".as_slice())
        );
        assert!(devices.take_terminal_output().is_none());
    }

    #[test]
    fn files_create_write_seek_read_and_track_modification() {
        let mut devices = ComputerDevices::new();
        assert_eq!(
            devices.fs_open("/home/hello.c", FS_OPEN_READ),
            STATUS_NOT_FOUND
        );
        let handle = devices.fs_open(
            "/home/hello.c",
            FS_OPEN_READ | FS_OPEN_WRITE | FS_OPEN_CREATE,
        );
        assert!(handle > 0);
        let handle = handle as u32;
        assert_eq!(devices.fs_write(handle, b"hello world"), 11);
        assert_eq!(devices.fs_truncate(handle, 5), 0);
        assert_eq!(devices.fs_seek(handle, 0, 0), 0);
        let mut buffer = [0u8; 16];
        assert_eq!(devices.fs_read(handle, &mut buffer), 5);
        assert_eq!(&buffer[..5], b"hello");
        assert_eq!(devices.fs_stat("/home/hello.c"), Some(5));
        assert_eq!(devices.fs_sync(handle), 0);
        assert_eq!(devices.fs_close(handle), 0);
        assert_eq!(devices.fs_close(handle), STATUS_BAD_HANDLE);

        let modified = devices.take_modified_files();
        assert_eq!(
            modified,
            vec![("/home/hello.c".to_owned(), b"hello".to_vec())]
        );
        assert!(devices.take_modified_files().is_empty());
    }

    #[test]
    fn file_paths_are_confined_to_home() {
        let mut devices = ComputerDevices::new();
        for path in ["/etc/passwd", "/home/", "/home/../etc", "home/x", ""] {
            assert_eq!(
                devices.fs_open(path, FS_OPEN_READ | FS_OPEN_WRITE | FS_OPEN_CREATE),
                STATUS_INVALID,
                "path {path:?} must be rejected"
            );
        }
    }

    #[test]
    fn mounted_files_are_readable_without_modification_tracking() {
        let mut devices = ComputerDevices::new();
        devices
            .mount_file("/home/hello.c", b"seed".to_vec())
            .unwrap();
        assert!(devices.take_modified_files().is_empty());
        let handle = devices.fs_open("/home/hello.c", FS_OPEN_READ) as u32;
        let mut buffer = [0u8; 8];
        assert_eq!(devices.fs_read(handle, &mut buffer), 4);
        assert_eq!(&buffer[..4], b"seed");
        assert_eq!(devices.fs_write(handle, b"x"), STATUS_DENIED);
    }

    #[test]
    fn listing_record_is_length_delimited_and_sorted() {
        let mut devices = ComputerDevices::new();
        devices.mount_file("/home/b.txt", b"b".to_vec()).unwrap();
        devices.mount_file("/home/a.txt", b"a".to_vec()).unwrap();
        assert_eq!(
            devices.fs_list_record(),
            b"\x02\0\0\0\x0b\0\0\0/home/a.txt\x0b\0\0\0/home/b.txt"
        );
    }
}
