# Git on the PolkaVM application computer

## Result and evidence boundary

The bounded local Git port builds, links without unresolved-symbol suppression,
and runs in PolkaVM. Its application source belongs to
`polkavm-app-kit/apps/git-tty`; this repository owns the capability runtime.

Verified on 2026-09-05:

- Native PolkaVM compiler backend: `init`, `add`, `commit -m`, `status`, `log`,
  and `diff`, with filesystem bytes and metadata restored into a fresh supervisor
  for every invocation.
- A same-size `one\n` → `two\n` working-tree edit was detected, diffed, staged,
  and committed. Both commits remained readable after relaunch.
- An existing `index.lock` caused a nonzero exit without changing the index.
  Exhausting the file quota also failed without changing the index; the previous
  commits remained readable.
- Dotli's actual local browser terminal ran the same six commands, including
  quoted commit messages and the same-size edit. Reloading the application
  restored both commits and a clean working tree from IndexedDB.
- Shared filesystem conformance ran through native and translated browser guests:
  exclusive creation across live participants, persistent directories and metadata,
  atomic replacement, and cancellation before publication.

These are small-repository results, not a claim of complete Git/POSIX compatibility,
large-repository scalability, or crash-durable storage. No application was deployed
as part of this work.

## Source and build

- Git **v2.55.0**, commit `e9019fcafe0040228b8631c30f97ae1adb61bcdc`.
- zlib **v1.3.1**, commit `51b7f2abdade71cd9bb0e7a373ef2610ec6f9daf`.
- Freestanding RV32E C with Rust host-import wrappers, PolkaVM 0.31.0,
  and the app kit's pinned Rust toolchain.
- Explicit source selection and a six-command dispatcher; missing link symbols
  fail the build. Bundled regex, SHA-1 collision detection, zlib, integer/time
  conversion, formatting, and a controlled mutable environment are retained.
- GPL-2.0 Git and zlib notices accompany the binary. The package includes the
  complete modified upstream trees, adapters, shared POSIX/libc sources, build
  scripts, Cargo.lock, and vendored Cargo dependencies as corresponding source.
  Archive ordering, ownership, and timestamps are normalized.

From the app kit:

```sh
APP=git-tty npm run build
APP=git-tty npm run verify
```

The Host must provide the updated shared-filesystem contract and register or resolve
this package as `git`. Its manifest requests only `core`, `fs`, and `tty`.

## Deliberate command boundary

| Surface | Contract |
| --- | --- |
| Commands | `init`, `add`, `commit`, `status`, `log`, `diff` |
| Global options | `-C`, `-c`, `--no-pager`/`-P`; unsupported globals reject |
| Commit messages | `-m` required; interactive message editing unavailable |
| Identity | Caller author/committer environment or Git configuration; synthetic guest OS identity, not host credentials |
| Time/locale | UTC/C; existing signed 32-bit `time_t` range |
| Hooks | Deliberately disabled |
| External execution | Rejected, including filters and textconv; no shell fallback |
| Maintenance | Automatic gc/maintenance, background helpers and threading disabled |
| Presentation | Pager disabled; terminal output supports POSIX `OPOST`/`ONLCR` |
| Signing | Disabled by default; no signing provider in this port |
| Networking | No remotes, sockets, or network capability |
| Other Git commands | Not part of the dispatcher |

This is not an unrestricted copy of the host's Git executable. Configuration
requiring unsupported helpers does not make those helpers available.

## Filesystem foundation

All processes in a supervisor tree share one authoritative `/home` namespace.
Handles and seek positions remain process-local; there are no copied views merged
at child exit. The contract is specified in
[the application-computer ABI](polkadot-host-application-computer.md).

Git uses:

- Atomic exclusive creation for lock files; an existing lock is never truncated.
- Atomic rename/replacement for publication, with failure preflight.
- Persistent directory entries, inode identity, and monotonic nanosecond mutation
  timestamps, including same-size changes and clock rollback.
- Descriptor-backed seek/read/write/append, metadata and directory enumeration.
- Shared POSIX stdio and guest-local descriptor duplication above opaque host handles.

Host cancellation/fault/exit releases that process's open handles without undoing
completed writes. Cancellation before rename preserves the old destination, but
may leave a candidate or Git lock file; this is not automatic stale-lock recovery.
Git fatal/normal exits run the port's registered cleanup handlers.

Dotli checkpoints file bytes and namespace metadata in one IndexedDB record.
Version-1 byte-only saves migrate to version 2; malformed existing saves fail
without silently replacing the filesystem. `fs_sync` does not claim an OS fsync,
transaction across multiple Git files, or survival of abrupt host/storage failure.

## Bounds

- 64 files and 256 directories in the computer filesystem; 1 MiB per file.
- 16 open host file handles per Git invocation.
- A fresh bounded 16 MiB bump-allocation arena per invocation; no general `free`
  reclamation. This is a material workload limit, not a measured large-repository
  memory budget.
- Fixed regular-file/directory modes, 0644/0755; no symlinks or hardlinks.
- No complete POSIX signal/job-control, process-execution, or permission model.
- Floating formatting does not support long double, hexadecimal float, or precision
  beyond 18; unsupported formatting reports failure.

## Earlier assessment

The pre-port assessment recorded compilation of 429 Git objects and 10 zlib objects,
not a working application. Its complete link still had 90 libc/POSIX gaps after
compiler helpers and host wrappers were excluded, and the old filesystem lacked
Git's correctness requirements. That historical assessment is retained in commit
`807a164`; it must not be mistaken for the current build or execution result.
The filesystem work and executed bounded port above supersede its proposed next slice.
