/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use crate::file_input::{
    self, FileDelivery, FileDescriptor, FileInfo, FileInputDelivery, FileInputRequest,
    FileInputSupport, FileRegistration, FileRelaunch, FileSelection, FileStreamSelection,
    FILE_READ_INVALID_HANDLE, FILE_READ_INVALID_RANGE, FILE_REGISTER_DELIVERY_UNAVAILABLE,
    MAX_FILE_READ_BYTES,
};
use anyhow::{anyhow, bail, Result};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub const MAX_MEDIATED_INPUT_KIND_BYTES: usize = 32;
pub const MAX_MEDIATED_INPUT_MEDIA_TYPE_BYTES: usize = 64;
pub const MAX_MEDIATED_INPUT_BYTES: usize = 1024 * 1024;
pub const MAX_MEDIATED_INPUT_REGISTRATIONS: usize = 8;

pub const MEDIATED_INPUT_REGISTER_INVALID: i32 = -1;
pub const MEDIATED_INPUT_REGISTER_UNAVAILABLE: i32 = -2;
pub const MEDIATED_INPUT_REGISTER_QUOTA_EXCEEDED: i32 = -3;

pub const MEDIATED_INPUT_TRIGGER_ACCEPTED: u32 = 0;
pub const MEDIATED_INPUT_TRIGGER_INVALID_HANDLE: u32 = 1;
pub const MEDIATED_INPUT_TRIGGER_BUSY: u32 = 2;

pub const MEDIATED_INPUT_CANCEL_ACCEPTED: u32 = 0;
pub const MEDIATED_INPUT_CANCEL_INVALID_HANDLE: u32 = 1;
pub const MEDIATED_INPUT_CANCEL_NOT_ACTIVE: u32 = 2;

#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MediatedInputStatus {
    Invalid = 0,
    Registered = 1,
    Active = 2,
    Ready = 3,
    Cancelled = 4,
    PermissionDenied = 5,
    Failed = 6,
}

impl TryFrom<u32> for MediatedInputStatus {
    type Error = anyhow::Error;

    fn try_from(value: u32) -> Result<Self> {
        match value {
            1 => Ok(Self::Registered),
            2 => Ok(Self::Active),
            3 => Ok(Self::Ready),
            4 => Ok(Self::Cancelled),
            5 => Ok(Self::PermissionDenied),
            6 => Ok(Self::Failed),
            _ => Err(anyhow!("invalid mediated-input status {value}")),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MediatedInputRequest {
    pub handle: u32,
    pub kind: String,
    pub media_type: String,
    pub max_bytes: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MediatedInputCommand {
    Request(MediatedInputRequest),
    FileRequest(FileInputRequest),
    Cancel { handle: u32 },
}

#[derive(Debug)]
enum Source {
    Device { kind: String, media_type: String },
    File(FileDescriptor),
}

#[derive(Debug)]
struct Registration {
    source: Source,
    max_bytes: usize,
    status: MediatedInputStatus,
    result: Option<Vec<u8>>,
    file: Option<FileInfo>,
}

impl Registration {
    fn file_descriptor(&self) -> Option<&FileDescriptor> {
        match &self.source {
            Source::File(descriptor) => Some(descriptor),
            Source::Device { .. } => None,
        }
    }

    fn is_stream(&self) -> bool {
        self.file_descriptor()
            .is_some_and(|descriptor| descriptor.delivery == FileDelivery::Stream)
    }

    fn clear_result(&mut self) {
        self.result = None;
        self.file = None;
    }
}

/// A relaunch file mounted before `init`, waiting for its handler to return.
#[derive(Debug)]
struct MountedFile {
    id: String,
    mount_path: String,
    info: FileInfo,
}

#[derive(Debug, Default)]
pub(crate) struct MediatedInputState {
    supported_kinds: BTreeSet<String>,
    file_support: Option<FileInputSupport>,
    mounted_file: Option<MountedFile>,
    registrations: BTreeMap<u32, Registration>,
    commands: VecDeque<MediatedInputCommand>,
    file_registrations_changed: bool,
    next_handle: u32,
}

impl MediatedInputState {
    pub(crate) fn set_supported_kinds(&mut self, kinds: &[String]) -> Result<()> {
        if kinds.len() > MAX_MEDIATED_INPUT_REGISTRATIONS {
            bail!("too many mediated-input kinds");
        }
        if self
            .registrations
            .values()
            .any(|registration| registration.status == MediatedInputStatus::Active)
        {
            bail!("cannot change mediated-input kinds while a request is active");
        }
        let mut supported = BTreeSet::new();
        for kind in kinds {
            if !valid_token(kind, MAX_MEDIATED_INPUT_KIND_BYTES) {
                bail!("invalid mediated-input kind {kind}");
            }
            if !supported.insert(kind.clone()) {
                bail!("duplicate mediated-input kind {kind}");
            }
        }
        self.supported_kinds = supported;
        self.registrations
            .retain(|_, registration| match &registration.source {
                Source::Device { kind, .. } => self.supported_kinds.contains(kind),
                Source::File(_) => true,
            });
        Ok(())
    }

    /// Selects the deliveries later file registrations may use.
    pub(crate) fn set_file_support(&mut self, support: FileInputSupport) -> Result<()> {
        let enabled = support.inline || support.relaunch || support.stream;
        if enabled && !file_input::valid_mount_path(&support.entrypoint) {
            bail!("invalid file-input entrypoint {}", support.entrypoint);
        }
        self.file_support = enabled.then_some(support);
        Ok(())
    }

    /// Records the relaunch file the fresh execution starts with.
    pub(crate) fn set_mounted_file(&mut self, relaunch: &FileRelaunch) {
        self.mounted_file = Some(MountedFile {
            id: relaunch.id.clone(),
            mount_path: relaunch.mount_path.clone(),
            info: relaunch.info(),
        });
    }

    pub(crate) fn register(&mut self, kind: String, media_type: String, max_bytes: usize) -> i32 {
        if !valid_token(&kind, MAX_MEDIATED_INPUT_KIND_BYTES)
            || !valid_token(&media_type, MAX_MEDIATED_INPUT_MEDIA_TYPE_BYTES)
            || !(1..=MAX_MEDIATED_INPUT_BYTES).contains(&max_bytes)
        {
            return MEDIATED_INPUT_REGISTER_INVALID;
        }
        if !self.supported_kinds.contains(&kind) {
            return MEDIATED_INPUT_REGISTER_UNAVAILABLE;
        }
        if let Some((&handle, _)) = self.registrations.iter().find(|(_, registration)| {
            matches!(
                &registration.source,
                Source::Device { kind: existing_kind, media_type: existing_media_type }
                    if *existing_kind == kind && *existing_media_type == media_type
            ) && registration.max_bytes == max_bytes
        }) {
            return handle as i32;
        }
        self.insert(Source::Device { kind, media_type }, max_bytes)
    }

    pub(crate) fn register_file(&mut self, bytes: &[u8]) -> i32 {
        let Some(descriptor) = file_input::parse_descriptor(bytes) else {
            return MEDIATED_INPUT_REGISTER_INVALID;
        };
        let Some(support) = &self.file_support else {
            return MEDIATED_INPUT_REGISTER_UNAVAILABLE;
        };
        let supported = match descriptor.delivery {
            FileDelivery::Inline => support.inline,
            FileDelivery::Relaunch => support.relaunch,
            FileDelivery::Stream => support.stream,
        };
        if !supported {
            return FILE_REGISTER_DELIVERY_UNAVAILABLE;
        }
        if descriptor.mount_path.as_deref() == Some(support.entrypoint.as_str()) {
            return MEDIATED_INPUT_REGISTER_INVALID;
        }
        for (&handle, registration) in &self.registrations {
            let Some(existing) = registration.file_descriptor() else {
                continue;
            };
            if *existing == descriptor {
                return handle as i32;
            }
            if existing.id == descriptor.id
                || (descriptor.mount_path.is_some() && existing.mount_path == descriptor.mount_path)
            {
                return MEDIATED_INPUT_REGISTER_INVALID;
            }
        }
        let max_bytes = descriptor.max_bytes as usize;
        let mounted = self
            .mounted_file
            .take_if(|mounted| mounted.id == descriptor.id)
            .filter(|mounted| {
                descriptor.mount_path.as_ref() == Some(&mounted.mount_path)
                    && mounted.info.size <= max_bytes as u64
            });
        let handle = self.insert(Source::File(descriptor), max_bytes);
        if handle > 0 {
            self.file_registrations_changed = true;
            if let Some(mounted) = mounted {
                let registration = self
                    .registrations
                    .get_mut(&(handle as u32))
                    .expect("file registration was just inserted");
                registration.status = MediatedInputStatus::Ready;
                registration.file = Some(mounted.info);
            }
        }
        handle
    }

    fn insert(&mut self, source: Source, max_bytes: usize) -> i32 {
        if self.registrations.len() == MAX_MEDIATED_INPUT_REGISTRATIONS {
            return MEDIATED_INPUT_REGISTER_QUOTA_EXCEEDED;
        }
        let Some(handle) = self.allocate_handle() else {
            return MEDIATED_INPUT_REGISTER_QUOTA_EXCEEDED;
        };
        self.registrations.insert(
            handle,
            Registration {
                source,
                max_bytes,
                status: MediatedInputStatus::Registered,
                result: None,
                file: None,
            },
        );
        handle as i32
    }

    /// Every file registration of the execution, in handle order.
    pub(crate) fn file_registrations(&self) -> Vec<FileRegistration> {
        self.registrations
            .iter()
            .filter_map(|(&handle, registration)| {
                Some(FileRegistration {
                    handle,
                    descriptor: registration.file_descriptor()?.clone(),
                    status: registration.status,
                })
            })
            .collect()
    }

    /// Takes the file registrations when they changed since the last take.
    pub(crate) fn take_file_registrations(&mut self) -> Option<Vec<FileRegistration>> {
        std::mem::take(&mut self.file_registrations_changed).then(|| self.file_registrations())
    }

    /// The `host_file_info` value of a selected file; `Err` for a handle that
    /// is not a file registration.
    pub(crate) fn file_info(&self, handle: u32) -> Result<Option<Vec<u8>>, ()> {
        let registration = self.registrations.get(&handle).ok_or(())?;
        registration.file_descriptor().ok_or(())?;
        Ok(registration
            .file
            .as_ref()
            .filter(|_| registration.status == MediatedInputStatus::Ready)
            .map(FileInfo::encode))
    }

    /// Delivers a file the user selected, either for the guest's active
    /// request or onto an idle registration from Host UI.
    pub(crate) fn deliver_file(
        &mut self,
        handle: u32,
        selection: FileSelection,
    ) -> Result<FileInputDelivery> {
        let name = file_input::sanitize_name(&selection.name)?;
        file_input::validate_selected_mime_type(&selection.mime_type)?;
        let active = self.active_handle();
        let registration = self
            .registrations
            .get_mut(&handle)
            .ok_or_else(|| anyhow!("unknown mediated-input handle {handle}"))?;
        let Some(descriptor) = registration.file_descriptor() else {
            bail!("mediated-input handle {handle} is not a file registration");
        };
        if descriptor.delivery == FileDelivery::Stream {
            bail!("stream selections are delivered as metadata, not whole-file bytes");
        }
        let accepted = match active {
            Some(active) => active == handle,
            None => registration.status != MediatedInputStatus::Ready,
        };
        if !accepted {
            return Ok(FileInputDelivery::Refused);
        }
        if selection.bytes.is_empty() || selection.bytes.len() > registration.max_bytes {
            registration.clear_result();
            registration.status = MediatedInputStatus::Failed;
            return Ok(FileInputDelivery::Rejected);
        }
        let info = FileInfo {
            name,
            mime_type: selection.mime_type,
            size: selection.bytes.len() as u64,
        };
        match descriptor.delivery {
            FileDelivery::Inline => {
                registration.result = Some(selection.bytes);
                registration.file = Some(info);
                registration.status = MediatedInputStatus::Ready;
                Ok(FileInputDelivery::Ready)
            }
            FileDelivery::Relaunch => {
                let relaunch = FileRelaunch {
                    id: descriptor.id.clone(),
                    mount_path: descriptor
                        .mount_path
                        .clone()
                        .expect("relaunch descriptors carry a mount path"),
                    name: info.name,
                    mime_type: info.mime_type,
                    bytes: selection.bytes,
                };
                registration.clear_result();
                registration.status = MediatedInputStatus::Registered;
                Ok(FileInputDelivery::Relaunch(relaunch))
            }
            FileDelivery::Stream => unreachable!("stream delivery was rejected above"),
        }
    }

    pub(crate) fn deliver_stream(
        &mut self,
        handle: u32,
        selection: FileStreamSelection,
    ) -> Result<FileInputDelivery> {
        let name = file_input::sanitize_name(&selection.name)?;
        file_input::validate_selected_mime_type(&selection.mime_type)?;
        let active = self.active_handle();
        let registration = self
            .registrations
            .get_mut(&handle)
            .ok_or_else(|| anyhow!("unknown mediated-input handle {handle}"))?;
        if !registration.is_stream() {
            bail!("mediated-input handle {handle} is not a stream registration");
        }
        let accepted = match active {
            Some(active) => active == handle,
            None => registration.status != MediatedInputStatus::Ready,
        };
        if !accepted {
            return Ok(FileInputDelivery::Refused);
        }
        if selection.size == 0 || selection.size > registration.max_bytes as u64 {
            registration.clear_result();
            registration.status = MediatedInputStatus::Failed;
            return Ok(FileInputDelivery::Rejected);
        }
        registration.result = None;
        registration.file = Some(FileInfo {
            name,
            mime_type: selection.mime_type,
            size: selection.size,
        });
        registration.status = MediatedInputStatus::Ready;
        Ok(FileInputDelivery::Ready)
    }

    pub(crate) fn file_read_length(
        &self,
        handle: u32,
        offset: u32,
        length: u32,
    ) -> Result<usize, i32> {
        let registration = self
            .registrations
            .get(&handle)
            .ok_or(FILE_READ_INVALID_HANDLE)?;
        if !registration.is_stream() || registration.status != MediatedInputStatus::Ready {
            return Err(FILE_READ_INVALID_HANDLE);
        }
        let info = registration.file.as_ref().ok_or(FILE_READ_INVALID_HANDLE)?;
        if !(1..=MAX_FILE_READ_BYTES).contains(&length) || u64::from(offset) > info.size {
            return Err(FILE_READ_INVALID_RANGE);
        }
        Ok(u64::from(length).min(info.size - u64::from(offset)) as usize)
    }

    pub(crate) fn fail_file_read(&mut self, handle: u32) {
        let registration = self
            .registrations
            .get_mut(&handle)
            .expect("selected file registration");
        registration.clear_result();
        registration.status = MediatedInputStatus::Failed;
        self.commands
            .push_back(MediatedInputCommand::Cancel { handle });
    }

    fn active_handle(&self) -> Option<u32> {
        self.registrations
            .iter()
            .find(|(_, registration)| registration.status == MediatedInputStatus::Active)
            .map(|(&handle, _)| handle)
    }

    pub(crate) fn trigger(&mut self, handle: u32) -> u32 {
        let selected_stream = self
            .registrations
            .get(&handle)
            .is_some_and(|registration| registration.is_stream() && registration.file.is_some());
        if self.active_handle().is_some() {
            return MEDIATED_INPUT_TRIGGER_BUSY;
        }
        let Some(registration) = self.registrations.get_mut(&handle) else {
            return MEDIATED_INPUT_TRIGGER_INVALID_HANDLE;
        };
        if selected_stream {
            self.commands
                .push_back(MediatedInputCommand::Cancel { handle });
        }
        registration.status = MediatedInputStatus::Active;
        registration.clear_result();
        self.commands.push_back(match &registration.source {
            Source::Device { kind, media_type } => {
                MediatedInputCommand::Request(MediatedInputRequest {
                    handle,
                    kind: kind.clone(),
                    media_type: media_type.clone(),
                    max_bytes: registration.max_bytes as u32,
                })
            }
            Source::File(descriptor) => MediatedInputCommand::FileRequest(FileInputRequest {
                handle,
                descriptor: descriptor.clone(),
            }),
        });
        MEDIATED_INPUT_TRIGGER_ACCEPTED
    }

    pub(crate) fn status(&self, handle: u32) -> MediatedInputStatus {
        self.registrations
            .get(&handle)
            .map_or(MediatedInputStatus::Invalid, |registration| {
                registration.status
            })
    }

    pub(crate) fn result(&self, handle: u32) -> Option<&[u8]> {
        self.registrations.get(&handle)?.result.as_deref()
    }

    pub(crate) fn consume_result(&mut self, handle: u32) {
        if let Some(registration) = self.registrations.get_mut(&handle) {
            registration.clear_result();
            registration.status = MediatedInputStatus::Registered;
        }
    }

    /// A read of a mounted relaunch file returns no bytes, since the file is
    /// an asset, and acknowledges the selection.
    pub(crate) fn acknowledge_mounted_file(&mut self, handle: u32) {
        if self.registrations.get(&handle).is_some_and(|registration| {
            registration.status == MediatedInputStatus::Ready
                && registration.result.is_none()
                && registration.file.is_some()
                && !registration.is_stream()
        }) {
            self.consume_result(handle);
        }
    }

    /// Stops an active request or discards a ready result; either leaves the
    /// registration idle.
    pub(crate) fn cancel(&mut self, handle: u32) -> u32 {
        let Some(registration) = self.registrations.get_mut(&handle) else {
            return MEDIATED_INPUT_CANCEL_INVALID_HANDLE;
        };
        if registration.is_stream()
            && matches!(
                registration.status,
                MediatedInputStatus::Ready | MediatedInputStatus::Active
            )
        {
            registration.clear_result();
            registration.status = MediatedInputStatus::Registered;
            self.commands
                .push_back(MediatedInputCommand::Cancel { handle });
            return MEDIATED_INPUT_CANCEL_ACCEPTED;
        }
        if registration.status == MediatedInputStatus::Ready {
            registration.clear_result();
            registration.status = MediatedInputStatus::Registered;
            return MEDIATED_INPUT_CANCEL_ACCEPTED;
        }
        if registration.status != MediatedInputStatus::Active {
            return MEDIATED_INPUT_CANCEL_NOT_ACTIVE;
        }
        registration.status = MediatedInputStatus::Registered;
        registration.clear_result();
        self.commands
            .push_back(MediatedInputCommand::Cancel { handle });
        MEDIATED_INPUT_CANCEL_ACCEPTED
    }

    pub(crate) fn take_command(&mut self) -> Option<MediatedInputCommand> {
        self.commands.pop_front()
    }

    /// Clears stream metadata and emits release notifications on teardown.
    pub(crate) fn close_streams(&mut self) {
        for (&handle, registration) in &mut self.registrations {
            if registration.is_stream()
                && (registration.file.is_some()
                    || registration.status == MediatedInputStatus::Active)
            {
                registration.clear_result();
                registration.status = MediatedInputStatus::Registered;
                self.commands
                    .push_back(MediatedInputCommand::Cancel { handle });
            }
        }
    }
    pub(crate) fn complete(
        &mut self,
        handle: u32,
        status: MediatedInputStatus,
        bytes: Vec<u8>,
    ) -> Result<()> {
        let registration = self
            .registrations
            .get_mut(&handle)
            .ok_or_else(|| anyhow!("unknown mediated-input handle {handle}"))?;
        if registration.status != MediatedInputStatus::Active {
            bail!("mediated-input handle {handle} is not active");
        }
        match status {
            MediatedInputStatus::Ready if registration.file_descriptor().is_some() => {
                bail!("file results are delivered with their name and MIME type");
            }
            MediatedInputStatus::Ready => {
                if bytes.is_empty() || bytes.len() > registration.max_bytes {
                    bail!("mediated-input result exceeds the registered bound");
                }
                registration.result = Some(bytes);
            }
            MediatedInputStatus::Cancelled
            | MediatedInputStatus::PermissionDenied
            | MediatedInputStatus::Failed => {
                if !bytes.is_empty() {
                    bail!("mediated-input failure carries unexpected bytes");
                }
                registration.clear_result();
            }
            MediatedInputStatus::Invalid
            | MediatedInputStatus::Registered
            | MediatedInputStatus::Active => {
                bail!("invalid terminal mediated-input status")
            }
        }
        registration.status = status;
        Ok(())
    }

    fn allocate_handle(&mut self) -> Option<u32> {
        for _ in 0..=MAX_MEDIATED_INPUT_REGISTRATIONS {
            self.next_handle = self.next_handle.wrapping_add(1).max(1);
            if !self.registrations.contains_key(&self.next_handle)
                && self.next_handle <= i32::MAX as u32
            {
                return Some(self.next_handle);
            }
        }
        None
    }
}

fn valid_token(value: &str, max_bytes: usize) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= max_bytes
        && bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.iter().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'-' | b'.' | b'_' | b'+')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_only_support_obeys_shared_registration_quota() {
        let descriptor = |id: usize| {
            format!(
                r#"{{"id":"stream-{id}","label":"Stream","extensions":[".bin"],"delivery":"stream","maxBytes":4294967295}}"#
            )
        };
        let mut state = MediatedInputState::default();
        assert_eq!(
            state.register_file(descriptor(0).as_bytes()),
            MEDIATED_INPUT_REGISTER_UNAVAILABLE
        );
        state
            .set_file_support(FileInputSupport {
                stream: true,
                entrypoint: "app.polkavm".into(),
                ..FileInputSupport::default()
            })
            .unwrap();
        for id in 0..MAX_MEDIATED_INPUT_REGISTRATIONS {
            assert_eq!(
                state.register_file(descriptor(id).as_bytes()),
                id as i32 + 1
            );
        }
        assert_eq!(state.register_file(descriptor(0).as_bytes()), 1);
        assert_eq!(
            state.register_file(descriptor(MAX_MEDIATED_INPUT_REGISTRATIONS).as_bytes()),
            MEDIATED_INPUT_REGISTER_QUOTA_EXCEEDED
        );
        assert_eq!(
            state.register_file(
                br#"{"id":"inline","label":"Inline","extensions":[".bin"],"delivery":"inline","maxBytes":1}"#
            ),
            FILE_REGISTER_DELIVERY_UNAVAILABLE
        );
    }

    #[test]
    fn registration_trigger_completion_and_read_are_bounded() {
        let mut state = MediatedInputState::default();
        state
            .set_supported_kinds(&["camera-ur".to_owned()])
            .unwrap();
        let handle = state.register(
            "camera-ur".to_owned(),
            "x-zklock-authorization".to_owned(),
            4,
        );
        assert!(handle > 0);
        assert_eq!(
            state.trigger(handle as u32),
            MEDIATED_INPUT_TRIGGER_ACCEPTED
        );
        assert_eq!(
            state.take_command(),
            Some(MediatedInputCommand::Request(MediatedInputRequest {
                handle: handle as u32,
                kind: "camera-ur".to_owned(),
                media_type: "x-zklock-authorization".to_owned(),
                max_bytes: 4,
            }))
        );
        assert!(state
            .complete(
                handle as u32,
                MediatedInputStatus::Ready,
                vec![1, 2, 3, 4, 5]
            )
            .is_err());
        state
            .complete(handle as u32, MediatedInputStatus::Ready, vec![1, 2, 3])
            .unwrap();
        assert_eq!(state.result(handle as u32), Some([1, 2, 3].as_slice()));
        state.consume_result(handle as u32);
        assert_eq!(state.result(handle as u32), None);
        assert_eq!(state.status(handle as u32), MediatedInputStatus::Registered);
    }

    #[test]
    fn cancellation_is_forwarded_and_late_results_are_rejected() {
        let mut state = MediatedInputState::default();
        state
            .set_supported_kinds(&["camera-ur".to_owned()])
            .unwrap();
        let handle = state.register("camera-ur".into(), "bytes".into(), 16) as u32;
        assert_eq!(state.trigger(handle), MEDIATED_INPUT_TRIGGER_ACCEPTED);
        state.take_command();
        assert_eq!(state.cancel(handle), MEDIATED_INPUT_CANCEL_ACCEPTED);
        assert_eq!(
            state.take_command(),
            Some(MediatedInputCommand::Cancel { handle })
        );
        assert_eq!(state.status(handle), MediatedInputStatus::Registered);
        assert!(state
            .complete(handle, MediatedInputStatus::Ready, vec![1])
            .is_err());
    }

    #[test]
    fn file_registrations_outlive_kind_changes_and_need_file_results() {
        let mut state = MediatedInputState::default();
        state
            .set_supported_kinds(&["camera-ur".to_owned()])
            .unwrap();
        state
            .set_file_support(FileInputSupport {
                inline: true,
                relaunch: false,
                stream: false,
                entrypoint: "app.polkavm".into(),
            })
            .unwrap();
        let file = state.register_file(
            br#"{"id":"doc","label":"Doc","extensions":[".txt"],"delivery":"inline","maxBytes":4}"#,
        ) as u32;
        let camera = state.register("camera-ur".into(), "bytes".into(), 4) as u32;
        state.set_supported_kinds(&[]).unwrap();
        assert_eq!(state.status(camera), MediatedInputStatus::Invalid);
        assert_eq!(state.status(file), MediatedInputStatus::Registered);
        assert_eq!(state.trigger(file), MEDIATED_INPUT_TRIGGER_ACCEPTED);
        assert!(matches!(
            state.take_command(),
            Some(MediatedInputCommand::FileRequest(FileInputRequest { handle, .. })) if handle == file
        ));
        assert!(state
            .complete(file, MediatedInputStatus::Ready, vec![1])
            .is_err());
        state
            .complete(file, MediatedInputStatus::PermissionDenied, Vec::new())
            .unwrap();
        assert_eq!(state.file_info(file), Ok(None));
        assert_eq!(state.file_info(camera), Err(()));
    }

    #[test]
    fn cancelling_a_ready_result_discards_it_without_a_host_command() {
        let mut state = MediatedInputState::default();
        state
            .set_supported_kinds(&["camera-ur".to_owned()])
            .unwrap();
        let handle = state.register("camera-ur".into(), "bytes".into(), 16) as u32;
        assert_eq!(state.cancel(handle), MEDIATED_INPUT_CANCEL_NOT_ACTIVE);
        state.trigger(handle);
        state.take_command();
        state
            .complete(handle, MediatedInputStatus::Ready, vec![1])
            .unwrap();
        assert_eq!(state.cancel(handle), MEDIATED_INPUT_CANCEL_ACCEPTED);
        assert_eq!(state.take_command(), None);
        assert_eq!(state.status(handle), MediatedInputStatus::Registered);
        assert_eq!(state.result(handle), None);
        assert_eq!(state.cancel(handle), MEDIATED_INPUT_CANCEL_NOT_ACTIVE);
    }
}
