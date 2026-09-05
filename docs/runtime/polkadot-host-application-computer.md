---
title: "PolkaVM application computer on the Polkadot Host"
type: architecture-decision
status: proposed-prototype
---

# PolkaVM application computer on the Polkadot Host

## Context

We want the Polkadot Host to run portable applications compiled for PolkaVM.
The goal is broader than smart-contract-style execution: a PolkaVM application
should be able to access granted files, use a terminal, make network
connections, render UI, launch child applications, and use Polkadot-native
services such as signing, identity, Statement Store, and chain calls.

The Polkadot Host is the platform boundary. macOS, iOS, Linux, Android, browser,
and other shells are implementations of that Host. Applications must not
depend directly on the underlying operating system.

The desired long-term environment includes shells and common Unix utilities,
Vim, terminal Emacs, SSH, Git, terminal multiplexing, graphical applications,
browser integration, tiling workspaces, and nested workspaces whose child
applications remain independently sandboxed.

WASI is useful architectural precedent because it separates portable guest code
from operating-system capabilities and models external resources explicitly.
Hyprland is useful interaction-model precedent, but its Linux and Wayland
assumptions must not become part of the PolkaVM platform.

## Decision

Define a small, modular, versioned Polkadot Host ABI for PolkaVM applications.
The ABI is capability-oriented and operating-system-neutral. POSIX compatibility
is a library above this ABI, not the platform contract.

```text
                    Polkadot Host
                         |
                  Polkadot Host ABI
                         |
         +---------------+----------------+
         |               |                |
      POSIX shim      native Rust SDK   other SDKs
         |               |
    Vim / Emacs /     native PVM
    SSH / Git           applications
```

The initial capability groups are:

- `host.core`
- `host.io`
- `host.fs`
- `host.process`
- `host.net`
- `host.tty`

Graphics and nested composition follow terminal applications:

- `host.display`
- `host.input`
- `host.workspace`

Polkadot-specific services remain separate:

- `host.polkadot`
- `host.identity`
- `host.crypto`
- `host.statement`

This keeps general-purpose computing capabilities independent from
chain-specific functionality.

## Relationship to application ABI v1

This is a separate execution contract, not an extension of the cooperative
`init()` / `update()` PolkaVM application ABI v1. Long-lived entrypoints,
blocking or pollable I/O, mutable files, process exit status, terminals, and
child VM supervision have materially different lifecycle semantics.

The prototype interface is named `polkadot-host-computer/0.1`. A manifest must
eventually select it explicitly. Existing applications requesting PolkaVM
application ABI v1 retain their current behavior.

## ABI principles

### Explicit capabilities

Applications receive only resources granted by the Host. There is no ambient
filesystem, terminal, network, signing, or process authority.

Example editor grant:

```yaml
filesystem:
  ~/documents: read-write
terminal: true
network: none
polkadot-signing: none
```

Example SSH grant:

```yaml
filesystem:
  ~/.ssh: read-only
network: outbound-tcp
terminal: true
random: true
```

A manifest requests capabilities; the Host decides what is granted.

### Opaque handles

Host resources use ABI-owned handles, never operating-system file descriptors.
A handle identifies both a resource and its granted operations.

```text
Handle(12) -> file
Handle(13) -> TCP stream
Handle(14) -> terminal
Handle(15) -> child process
Handle(16) -> display surface
```

Generic operations may work across suitable handle types:

```text
read(handle, buffer)
write(handle, buffer)
close(handle)
poll(handles)
```

Handles are scoped to one application instance and become invalid when closed
or when the owning instance exits.

### No `fork()`

`fork()` is not a primitive. Process operations are explicit:

```text
process.spawn()
process.wait()
process.terminate()
pipe.create()
```

A POSIX layer may map `posix_spawn` and emulate compatible behavior. It must not
weaken Host ownership of child instances.

### Host-owned isolation

Every child remains a Host-managed sandbox, including children visually
contained by a workspace application.

```text
workspace.pvm
    +-- requests host.spawn(vim.pvm)
    +-- requests host.spawn(shell.pvm)
    +-- receives child surface handles
```

The workspace owns layout, focus policy, shortcuts, pane arrangement, and
launch requests. The Host owns VM instances, permissions, limits, lifecycle,
surfaces, and input authority.

### Bounded data and stable errors

Every ABI operation has explicit byte, handle, and queue bounds. Guest pointers
are valid only during a call and are checked before access. Errors are stable ABI
values rather than host-language strings or operating-system errno values.

## Initial host interfaces

The prototype targets these logical operations. Exact binary signatures and
record encodings are specified by conformance fixtures before being advertised
as stable.

### `host.core`

```text
args()
environment()
clock_monotonic()
clock_wall()
random()
exit()
log()
```

The first executable slice uses these versioned imports:

```text
polkadot_host_0_1_core_args(pointer: u32, capacity: u32) -> i32
polkadot_host_0_1_core_environment(pointer: u32, capacity: u32) -> i32
polkadot_host_0_1_core_yield() -> ()
polkadot_host_0_1_core_clock_monotonic(destination: u32) -> i32
polkadot_host_0_1_core_clock_wall(destination: u32) -> i32
polkadot_host_0_1_core_random(destination: u32, length: u32) -> i32
polkadot_host_0_1_core_exit(status: i32) -> never
```

Arguments are encoded as a little-endian `u32` count followed by one
little-endian `u32` byte length and that many UTF-8 bytes for each argument.
Environment records use the same count, followed by a length-prefixed key and
length-prefixed value for each entry. Keys are non-empty, unique, and contain
neither `=` nor NUL. Arguments and values contain no NUL.

The read operations return bytes written when capacity is sufficient. Otherwise
they return the required capacity as a negative `i32` and write nothing.
Invalid guest memory fails the execution. Each encoded record is limited to
64 KiB and 1,024 entries.

Clock calls write one little-endian `u64` nanosecond value: monotonic time since
the process was created, or wall time since the Unix epoch. The pointer-based
record avoids target-specific 64-bit return-register conventions on 32-bit
guests. `core_random` fills exactly the requested bytes from the Host CSPRNG;
zero-length requests are invalid and one call is limited to 4 KiB.

### Stream operations

The first prototype deliberately has no generic `host.io` interface. Terminal
and filesystem capabilities own their operations until pipes and sockets prove
that a shared stream contract has useful common semantics.

### `host.fs`

```text
polkadot_host_0_1_fs_open(path_pointer, path_length, flags) -> handle | error
polkadot_host_0_1_fs_read(handle, destination, capacity) -> bytes | error
polkadot_host_0_1_fs_write(handle, source, length) -> bytes | error
polkadot_host_0_1_fs_seek(handle, offset, whence) -> position | error
polkadot_host_0_1_fs_truncate(handle, length) -> status
polkadot_host_0_1_fs_stat(path_pointer, path_length, record_pointer) -> status
polkadot_host_0_1_fs_sync(handle) -> status
polkadot_host_0_1_fs_close(handle) -> status
polkadot_host_0_1_fs_remove(path_pointer, path_length) -> status
```

The first implementation uses a virtual filesystem with a Host-provided
persistent `/home` mount. Removing an open file returns `DENIED`; a successful
removal is reported separately from modified file contents so persistence
adapters can delete the backing entry.

### `host.process`

```text
spawn(package, argv, environment, handles)
wait(process)
terminate(process)
```

Package resolution and authorization are Host capabilities. A guest cannot load
an arbitrary host path. The first executable slice implements a combined
foreground operation while the parent stays suspended in the hostcall:

```text
polkadot_host_0_1_process_run(package_ptr, package_len, args_ptr, args_len)
    -> child exit status | error
polkadot_host_0_1_fs_list(destination, capacity) -> record bytes | error
```

The supervisor owns the process stack (max depth 4), grants the foreground
process the terminal, and rebases the shared `/home` store when a child exits.

Piped background children implement pipes and `:!` filters without `fork`:

```text
polkadot_host_0_1_process_spawn(pkg_ptr, pkg_len, args_ptr, args_len) -> pid | error
polkadot_host_0_1_pipe_write(pid, src, len)  -> written | error
polkadot_host_0_1_pipe_read(pid, dst, cap)   -> bytes | 0 on EOF | WOULD_BLOCK
polkadot_host_0_1_pipe_close(pid)            -> 0 | error
polkadot_host_0_1_process_wait(pid)          -> exit status | WOULD_BLOCK
```

A spawned child has no terminal: its tty handle is a pipe pair owned by the
parent. Scheduling is cooperative — the child executes only while its parent
is suspended inside a pipe or wait hostcall, so a `write -> close -> read to
EOF -> wait` sequence always terminates. Pids are pid-scoped pipe handles for
now; generic transferable handles remain an open question. Background
children cannot spawn (max 4 live per computer).

### `host.net`

```text
resolve(hostname)
tcp_connect(address)
tcp_listen(address)
tcp_accept(listener)
udp_bind(address)
```

Networking is capability-gated and denied by default. The first implemented
slice is outbound TCP only; resolution happens inside connect:

```text
polkadot_host_0_1_net_tcp_connect(address_ptr, address_len) -> handle | error
polkadot_host_0_1_net_read(handle, dst, cap)  -> bytes | 0 on EOF | WOULD_BLOCK
polkadot_host_0_1_net_write(handle, src, len) -> written | WOULD_BLOCK | error
polkadot_host_0_1_net_close(handle)           -> 0 | error
```

The Host grants the capability per computer (`set_network_enabled`); at most
4 sockets, nonblocking, 64 KiB per transfer. Listen, accept, and UDP remain
unimplemented. Browser Hosts inject a byte-stream provider and remain DENIED
when none is configured. The reference `WebSocketTcpProvider` opens a
Host-selected relay URL, sends one JSON request
`{"version":1,"address":"host:port"}`, waits for `{"type":"connected"}`, then
exchanges raw TCP bytes as binary frames. TLS, HTTP, and SSH stay in the guest.
The relay retains at most 1 MiB or 1,024 incoming chunks per socket and permits
at most 1 MiB of queued outgoing bytes. Receive overflow closes the stream and
reports an I/O error rather than silently truncating it; activity wakes the
yielded guest to retry. Late relay events cannot reopen a closed stream.

### `host.tty`

```text
polkadot_host_0_1_tty_current() -> handle
polkadot_host_0_1_tty_read(handle, destination, capacity) -> bytes | error
polkadot_host_0_1_tty_write(handle, source, length) -> bytes | error
polkadot_host_0_1_tty_get_size(handle, record_pointer) -> status
polkadot_host_0_1_tty_set_mode(handle, flags) -> status
```

The POSIX layer maps stdin, stdout, termios, and required terminal ioctls onto
these operations.

The primary terminal is a granted opaque handle. Reads are nonblocking:
`WOULD_BLOCK` causes the guest to call `core_yield`; the Host resumes it when
input arrives. Size records contain little-endian `u32` columns and rows.
Prototype mode flags are raw input, echo, and signal generation.

Prototype status values are `-1` for `WOULD_BLOCK`, `-2` for a bad handle,
`-3` for an invalid argument, `-4` for a missing path, `-5` for denied access,
and `-6` for a resource limit. Invalid guest-memory ranges fail execution.

## Nested workspace model

A workspace application owns a layout tree and requests independently sandboxed
children from the Host:

```text
SplitHorizontal
+-- vim.pvm
+-- SplitVertical
    +-- shell.pvm
    +-- ssh.pvm
```

### Draft `host.workspace` surface ABI (unimplemented)

Design draft; binary signatures follow the conventions above and must be
pinned by conformance fixtures before any Host advertises them.

A surface is an opaque handle owned by the Host, like every other resource.
Two surface kinds exist initially, and both deliberately avoid a fixed bitmap
geometry:

- `text`: a column/row cell grid. The Host renders cells natively, so
  terminals stay DPI-independent on every platform (this removes the current
  640x400 presentation ceiling rather than generalizing it).
- `frame`: an RGBA framebuffer presented whole. This is the phase-7 graphics
  contract and is not required for the tiling milestone.

```text
polkadot_host_0_1_workspace_spawn(package, argv, environment)
    -> (child_handle, surface_handle) | error
polkadot_host_0_1_workspace_run(child_handle) -> status
polkadot_host_0_1_workspace_send_input(child_handle, record_pointer, length)
    -> written | WOULD_BLOCK | error
polkadot_host_0_1_workspace_resize(child_handle, columns, rows) -> status
polkadot_host_0_1_workspace_surface_read(surface_handle, pointer, capacity)
    -> length | WOULD_BLOCK | error
polkadot_host_0_1_workspace_close(child_handle) -> status
```

Invariants carried over from the terminal-computer supervisor:

- The Host owns every child VM, its gas budget, its capability grants, and its
  fault containment. `workspace_run` returns the same status codes as
  `process_run`; a faulted child is reported and reaped, never resurrected.
- The workspace guest sees only surface output records and child exit status.
  It never reads child memory, files, or capability state.
- Input is routed, not shared: the Host delivers input records exclusively to
  the workspace, which forwards bytes to at most one focused child per call.
  Host-authority cancellation (the Ctrl-] equivalent) always targets the
  workspace itself and cannot be intercepted by it.
- `workspace_resize` obeys the existing 1..=1000 column/row clamp, and surface
  reads are bounded by the same queue limits as terminal output.

A workspace application therefore composes existing contracts: it is an
ordinary computer guest whose extra capability is holding child and surface
handles. Layout, focus, and keybindings are guest policy.

A first tiling workspace may implement bindings such as:

```text
SUPER+Enter             launch terminal
SUPER+H/J/K/L           focus
SUPER+Shift+H/J/K/L     move
SUPER+1..9              workspace
SUPER+Q                 close
```

Actual Hyprland is out of scope. It is a Wayland compositor, while this ABI must
map to native Linux, macOS, iOS, Android, and browser surfaces.

## Delivery sequence

### Terminal computer

Build a Host terminal running `shell.pvm`, with `ls.pvm`, `cat.pvm`, and a tiny
editor. The slice succeeds when arguments and environment work, files persist,
pipes work, terminal resize is observable, cancellation works, and a second
child can be launched through the same process abstraction.

Do not begin with Bash.

### Real editor

Port a small vi first. Then use Vim as an integration test for terminal,
filesystem, clocks, polling, environment, metadata, cancellation, and libc
compatibility.

Success means editing a persistent file, exiting, and reopening the same bytes.

Status: neatvi (ISC, vendored unmodified in the application repository as
`apps/vi-tty`) runs against pvm-posix with the LSP client stubbed out; its
`:%!` filters and `:make` map onto the piped `process_spawn` capability. The
layer grew `poll`, `getchar`, `memchr`, `getenv`, file-descriptor reads, and
ICRNL emulation along the way.

Vim (feature-tiny, patch 9.2.1036) **runs**: vendored unmodified as
`apps/vim-tty`, built from the assessment's config.h and stub headers, with
~30 trivial glue stubs and the pvm-posix FILE layer. The phase-2 success
criterion holds end to end — launch, edit a persistent file, `:wq`, relaunch,
see the changes — both standalone and as a sandboxed child of the shell.
`:!` filters remain stubbed pending a `mch_call_shell` mapping onto the pipe
capability. Full matrix: [vim-tiny-compatibility.md](vim-tiny-compatibility.md).

SSH is assessed: Dropbear's dbclient plus its bundled crypto stack
compiles cleanly against the same toolchain; the single missing host
capability is entropy (`core_random`). Matrix and vertical-slice plan:
[ssh-client-compatibility.md](ssh-client-compatibility.md).

Presentation: the `computer-serve` Host mode (application repository: Epoca)
renders the supervisor's ANSI stream host-side through the shared terminal
emulator and speaks the framebuffer-app wire protocol, so browser surfaces
can present the computer without new message types.

Browser Host: `js/packages/pvm-browser-runtime/src/pvm-computer.js` implements
the same contract in JavaScript over the wasm-translated guest - context,
tty/fs devices, supervisor, pipes, spawn gating, and network denial - and runs
the identical `.polkavm` conformance fixtures in the browser test suite.
Translated shell and Vim guests run unmodified under it, so the computer is a
client-side web product, not a native-only CLI.

### Priority 1: Lynx

Lynx is the first remaining product milestone. It deliberately precedes SSH:
both need generic DNS, TCP, poll, cryptographic random, and clocks, while Lynx
also exercises the existing terminal and virtual filesystem through an
immediately visible application. The Host MUST NOT implement HTTP, TLS, or
Lynx-specific calls. Lynx owns HTTP and TLS over a Host-provided opaque TCP
stream. Native Hosts connect directly; browser Hosts use a generic
WebSocket/WebTransport-to-TCP connector. The connector is not browser or SSH
support and can serve any byte-stream protocol.

Success means the same `lynx.polkavm` binary:

1. launches standalone and through open DotNS spawn from the computer shell;
2. opens an HTTPS page, follows a link, and navigates back;
3. downloads a file into the Host-provided virtual `/home`;
4. persists bookmarks, configuration, and downloads across relaunch;
5. runs unchanged on native and browser Hosts.

Shell escapes, external mail/news commands, printer commands, and protocols
other than HTTP/HTTPS remain disabled in the first port.

### Priority 2: Doom as a workspace child

After Lynx, implement the first mixed-surface workspace rather than another
terminal program. A `workspace.polkavm` root launches independently sandboxed
children whose existing contracts remain unchanged:

```text
workspace.polkavm
+-- shell.polkavm  -> text surface
+-- kilo.polkavm   -> text surface
+-- doom.polkavm   -> frame surface
```

The Host owns every VM, surface, capability grant, resource bound, and input
route. The workspace owns only layout, focus, resize requests, and launch/close
requests. Success means Doom renders and receives input beside terminal
children, a child fault does not terminate the workspace, and no child can
read another child's memory or widen its capabilities.

### Remaining priority

After Lynx and the Doom workspace child, proceed in this order:

1. SQLite for random-access durability, atomic replacement, and locking;
2. Lua for useful scripting over existing Host capabilities;
3. BusyBox/Toybox applets (not a speculative full `fork`-based shell);
4. SSH, reusing Lynx's DNS/TCP/random/clock substrate with all SSH protocol and
   cryptography remaining in the guest;
5. tmux for multiple TTY handles, polling, and retained sessions;
6. Git for the combined filesystem/process/network integration test;
7. terminal Emacs as a late libc/terminal/process compatibility stress test;
8. Servo as a long-term full-platform stress test, not a prerequisite.

### Targeted POSIX compatibility

Implement only APIs demanded by selected applications:

```text
POSIX read()       -> host.io.read()
POSIX write()      -> host.io.write()
POSIX open()       -> host.fs.open()
POSIX stat()       -> host.fs.stat()
POSIX poll()       -> host.io.poll()
POSIX posix_spawn  -> host.process.spawn()
termios            -> host.tty
```

Maintain an explicit compatibility matrix. `fork` remains unsupported.

The first shared implementation is `pvm-posix` (C, in the application
repository): an allocator with consistent malloc/realloc headers plus
`read`/`write`/`open`/`ftruncate`/`close`/`ioctl(TIOCGWINSZ)`/termios/
`fopen`/`getline` mapped onto the computer capabilities. Ports include their
unmodified upstream source after redirecting `exit`, `malloc`, `realloc`,
and `free` to the shim.

### Host-authority cancellation

The Host may terminate the foreground process at any time. A terminated
child is discarded and its parent resumes from `process_run` with status
130; terminating the root ends the computer. Guests cannot veto or observe
the difference from a voluntary child exit.

### SSH

Port an SSH client using TTY, TCP, DNS, random, clock, and filesystem
capabilities. Success means connecting to a real SSH server and presenting its
remote shell in a Host terminal.

### Nested tiling workspace

Build `workspace.pvm`, launch three independent child applications, tile their
surfaces, route input, and rearrange them interactively.

### Larger Unix programs

Use Git, terminal Emacs, and BusyBox-style utilities to discover required
compatibility. Add a base ABI primitive only when the missing operation is
generally useful; otherwise implement it in the POSIX layer.

### Graphics

Start with a small Host-owned surface contract:

```text
surface_create(width, height)
surface_resize()
surface_present(buffer)
input_next_event()
```

Do not make Wayland the base ABI. Hosts map surfaces to their native graphics
systems. Browser integration starts with a Host webview, then a PVM browser
shell controlling that engine; a self-contained browser engine is a later
stress test.

## Package model

Every launchable program is a normal DotNS application with its own App
Manifest, content-addressed archive, entrypoint, and requested Host contract.
`process_spawn("vim", ...)` is open resolution, not a parent-manifest
allowlist: the Host resolves `vim`, verifies the child's own signed executable
record against the fetched archive, checks that it speaks a supported contract,
and creates an independently sandboxed VM. The child's effective grant cannot
exceed the parent's.

An application MAY carry name-to-CID pins for deterministic/offline operation.
Pins are lockfile data, never authorization: an unpinned published app remains
launchable when Host policy permits registry access, and a pinned app receives
no additional authority.

The manifest never names a real host resource. Filesystem operations address
only namespaces the Host chooses to mount into the process (`/home` in 0.1);
the backing store is unobservable Host policy. The same rule applies to
terminal, socket, process, and surface handles.

A `.polkavm` optional custom section MAY mirror contract metadata for tooling
or archive-local fixtures, but DotNS executable records remain the pre-fetch
trust anchor for published applications.

Applications declare a Host contract before fetch; the blob's exact import
table is checked before execution. A machine-readable IDL should generate Rust
and C bindings plus Host stubs once the prototype signatures settle.

## Repository ownership

`pvm-host-runtime` owns the ABI definitions, VM integration, capability and
handle semantics, reference backends, and conformance fixtures.

`polkavm-app-kit` owns packaged demonstration applications such as the shell,
utilities, and editor.

`host-rust-core` pins a reviewed `pvm-host-runtime` release and exposes it to
concrete Polkadot Hosts. It does not carry a second runtime implementation.

## First development spike

Implement one vertical slice:

```text
Epoca terminal surface
    +-- Kilo process-style PolkaVM guest
        +-- granted terminal handle
        +-- writable /home/hello.c
```

Required interfaces:

```text
core.args
core.environment
core.yield
core.exit
tty.current
tty.read
tty.write
tty.get_size
tty.set_mode
fs.open
fs.read
fs.write
fs.seek
fs.truncate
fs.stat
fs.sync
fs.close
```

The experiment succeeds when Epoca launches upstream Kilo, keyboard bytes pass
through the terminal handle, Kilo emits ANSI bytes to the Host terminal, a file
under `/home` can be edited and saved, and a fresh runtime instance observes
the saved bytes. Shells, pipes, child processes, and workspace composition are
explicitly deferred until this direct editor slice works.

## Consequences

- The base platform stays portable and capability-oriented.
- Existing Unix programs require a deliberate compatibility layer.
- Child processes require a Host-side supervisor rather than recursive ambient
  VM creation.
- Terminal semantics and polling must be specified precisely before Vim or SSH
  can be meaningful tests.
- Native and browser implementations must run the same conformance fixtures.
- This prototype introduces a second explicit execution contract rather than
  overloading application ABI v1.

## Deferred decisions

- IDL syntax and generator ownership.
- Exact handle representation and stale-handle detection.
- Poll record format and cancellation semantics.
- Persistent mount transaction and synchronization behavior.
- Threading, shared memory, `mmap`, and signal compatibility.
- Surface protocol and text/IME input.
- Package identity, discovery, and child capability attenuation.

## Known limitations (reviewed, deliberately deferred)

From the pre-publication security review of the experimental runtime:

- Host-side work is not metered by guest gas: every process transition clones
  the shared `/home` store (up to 64 MiB), and one pipe hostcall can drive a
  background child through up to 1,024 full gas slices. Needs copy-on-write
  file sharing and a shared drive budget.
- A granted network capability has no destination policy: loopback,
  link-local, and private ranges are reachable, and resolution/connect block
  the runtime thread (up to 5 s). Needs a Host-supplied address policy and
  async connect before any untrusted deployment.
- `fs_list`/`core_args` "required size" returns overlap the status-code
  space for records under 7 bytes.
- Cancelling a process persists its partial writes; whether cancellation
  should roll back dirty buffers is an open design call.
- Out-of-range pointer registers terminate the guest rather than returning
  a status; safe, but a child fault surfaces as status 139 rather than a
  distinguishable error.

## References

- WASI: <https://wasi.dev/>
- WASI capability-oriented security: <https://github.com/WebAssembly/WASI/blob/main/docs/Capabilities.md>
- Hyprland project documentation: <https://wiki.hypr.land/>
