# `polkadot-host-computer/0.1` — capability compatibility matrix

Each application column records which versioned host operations it requires to
function. "✓" means the app exercises this API in its normal execution path.
A bullet (•) means an optional path (e.g. a subcommand). An empty cell means
the API is neither required nor called.

## host.core

| Operation | shell | kilo-tty | vim-tty | lynx-tty | coreutils | workspace |
|---|---|---|---|---|---|---|
| `core_args` | ✓ | | | ✓ | ✓ | |
| `core_environment` | | | | | • (env) | |
| `core_clock_monotonic` | | | | ✓ | • (sleep) | |
| `core_clock_wall` | | | | ✓ | | |
| `core_random` | | | | ✓ | | |
| `core_exit` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `core_yield` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |

## host.fs

| Operation | shell | kilo-tty | vim-tty | lynx-tty | coreutils | workspace |
|---|---|---|---|---|---|---|
| `fs_open` | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `fs_read` | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `fs_write` | ✓ | ✓ | ✓ | ✓ | • (tee) | |
| `fs_seek` | | | | | • (head/tail) | |
| `fs_stat` | | | | | • (stat) | |
| `fs_truncate` | | | | ✓ | | |
| `fs_sync` | | | | ✓ | | |
| `fs_close` | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `fs_remove` | | | | ✓ | • (rm/mv) | |
| `fs_list` | ✓ | | | | | |

## host.tty

| Operation | shell | kilo-tty | vim-tty | lynx-tty | coreutils | workspace |
|---|---|---|---|---|---|---|
| `tty_current` | ✓ | ✓ | ✓ | ✓ | | ✓ |
| `tty_read` | ✓ | ✓ | ✓ | ✓ | • (tee) | ✓ |
| `tty_write` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `tty_get_size` | | | ✓ | ✓ | | ✓ |
| `tty_set_mode` | ✓ | | ✓ | ✓ | | ✓ |

## host.process

| Operation | shell | kilo-tty | vim-tty | lynx-tty | coreutils | workspace |
|---|---|---|---|---|---|---|
| `process_run` | ✓ | | | | | |
| `process_spawn` | • (pipelines) | | | | | |
| `process_wait` | • (pipelines) | | | | | |
| `pipe_read` | • (pipelines) | | | | | |
| `pipe_write` | • (pipelines) | | | | | |
| `pipe_close` | • (pipelines) | | | | | |

## host.net

| Operation | shell | kilo-tty | vim-tty | lynx-tty | coreutils | workspace |
|---|---|---|---|---|---|---|
| `net_tcp_connect` | | | | ✓ | | |
| `net_read` | | | | ✓ | | |
| `net_write` | | | | ✓ | | |
| `net_close` | | | | ✓ | | |

## host.workspace

| Operation | shell | kilo-tty | vim-tty | lynx-tty | coreutils | workspace |
|---|---|---|---|---|---|---|
| `workspace_spawn` | | | | | | ✓ |
| `workspace_send_input` | | | | | | ✓ |
| `workspace_read` | | | | | | ✓ |
| `workspace_resize` | | | | | | ✓ |
| `workspace_wait` | | | | | | ✓ |
| `workspace_close` | | | | | | ✓ |

## Capability coverage summary

| Capability group | Apps exercising it | Coverage |
|---|---|---|
| `host.core` (7 ops) | shell, lynx, coreutils, workspace | 6 / 7 (`core_random` exercised only by Lynx; `core_args` by 3 apps, `core_environment` by 1, `core_clock_monotonic` by 2) |
| `host.fs` (10 ops) | shell, kilo, vim, lynx, coreutils | 10 / 10 (every op exercised by at least one app; `fs_remove` by Lynx + coreutils rm/mv; `fs_seek` by coreutils head/tail) |
| `host.tty` (5 ops) | shell, kilo, vim, lynx, coreutils, workspace | 5 / 5 |
| `host.process` (6 ops) | shell, workspace (indirectly) | 6 / 6 (`process_run` shell foreground spawn; `process_spawn` shell pipelines; workspace children are process.run under the Host) |
| `host.net` (4 ops) | lynx | 4 / 4 (single application; TLS/HTTP in guest) |
| `host.workspace` (6 ops) | workspace | 6 / 6 |
| **Total: 38 ops** | 7 apps | 37 / 38 exercised (`core_random` exercised by Lynx only) |

## POSIX compatibility notes

The pvm-posix shim maps common POSIX functions to host operations. The
compatibility matrix for individual ports (Vim, Lynx, SSH assessment) lives in
per-application assessments. Noted gaps:

- `fork()` → `process.spawn` + `process.wait` (cooperative scheduling, no
  address-space cloning; child cannot outlive the parent hostcall).
- `exec()` → Not supported. The Host launches packages by name; the guest
  cannot replace its own program image.
- `select`/`poll` → `core_yield` + WOULD_BLOCK retry pattern; no descriptor
  multiplexing across tty/pipe/socket handles yet.
- `mmap` → Not supported. Files are read/written whole; Vim `:edit` works
  through the shim's buffered read path.
- `signals` → Partial. Ctrl-C becomes `terminate_foreground` at the Host
  boundary. No guest-to-guest signal delivery. `SIGPIPE` from closed pipe
  streams maps to `pipe_write` returning error, not a signal.
- `pthread` → Not supported. The PolkaVM guest is single-threaded.
- `gettimeofday` → `core_clock_wall`.
- `getpid`/`getppid` → Not applicable (no ambient process identity).