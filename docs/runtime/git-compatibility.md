# Git → freestanding riscv32 PolkaVM guest: compatibility assessment

## Result and evidence boundary

**Compilation is demonstrated; a usable Git port is not. Git has not been run in PVM.**
The retained objects still fail to link, and the current filesystem contract does not
supply the directory, locking, publication, and change-detection semantics needed for
reliable local repository mutation. Those are correctness blockers, not merely performance
improvements to defer until after a toy port.

Scope: assess local `init`, `add`, `commit -m`, `status`, `log`, and `diff`; no remotes or
networking, and no Git implementation in this change. Inspection used the existing
`/tmp/git-assessment/` sources, header shims, objects, and host-side toy repository.
No compilation or guest execution was performed for this document correction. The complete
link check was rerun separately by the integration owner; its failure is reported below.

### Retained compilation experiment

- Git **v2.55.0**, commit `e9019fcafe0040228b8631c30f97ae1adb61bcdc`;
  zlib **v1.3.1**, commit `51b7f2abdade71cd9bb0e7a373ef2610ec6f9daf`.
  The clone HEADs and Git generated `version-def.h`/zlib `zlib.h` agree.
- Recorded toolchain invocation: clang 22.1.8,
  `--target=riscv32-unknown-unknown -march=rv32emc -mabi=ilp32e -std=gnu99
  -fno-builtin -ffreestanding -nostdinc -fno-stack-protector -fPIC -Os -w`,
  with assessment shims before `pvm-posix/include` and `doom/c_src/include`.
  Inspection of saved `git.c.o` confirms ELF32 RISC-V, RVC/RVE, and clang 22.1.8.
  `-w` suppresses warnings: object generation is not proof of ABI or runtime correctness.
- Recorded configuration disables curl, OpenSSL, pthreads, iconv, gettext, mmap, system
  regex, Unix sockets, IPv6, and nanosecond stat fields, and selects bundled compatibility
  functions. Generated command/config/hook/version headers came from Git's host-side
  generators. The earlier experiment reported no Git/zlib source edits; this correction
  did not rebuild it or independently establish every historical invocation.
- **40 header shims, 20,811 bytes** remain. Initial diagnostics included missing `locale.h`,
  `regex.h`, `utime.h`, `sys/statvfs.h`, `memory.h`, stat fields, `PRIuMAX`, `PTRDIFF_MAX`,
  `readlink`, `iscntrl`, `struct itimerval`, `POLLNVAL`, and `NSIG`. Additional declarations
  made these translation units compile; they did not implement the corresponding calls.
- Notably, the assessment's `sys/stat.h` replaces Doom's three-field `struct stat` with a
  larger, differently ordered layout. The implementation and all callers must use one
  consistent ABI; linking an existing shim object does not prove that compatibility.

## Object, symbol, and size accounting

Inspection of **441 retained objects** gives this non-overlapping breakdown:

| Group | Objects |
|---|---:|
| Git `LIB_OBJS` matching the Makefile entries, including xdiff, refs/reftable and four `compat/` library units | 283 |
| Git builtin translation units | 130 |
| Git dispatcher (`git.c.o`) | 1 |
| Hash implementations: block SHA-1, block SHA-256, SHA1DC × 3 | 5 |
| Additional compat units: strcasestr, memmem, strlcpy, qsort_s, setenv, unsetenv, strtoumax, strtoimax, mmap | 9 |
| Bundled regex | 1 |
| **Git subtotal** | **429** |
| zlib core (no `gz*` stdio family) | 10 |
| Existing `pvm_posix.o` and `libc_shim.o` | 2 |
| **Total** | **441** |

The saved set contains both SHA-1 alternatives; this does not prescribe shipping both.
The dispatcher references **142 distinct undefined `cmd_*` symbols**, not 130 commands:
translation units can implement several commands. Selecting the five builtin objects for
`add`, `init`, `commit`/`status`, `log`, and `diff` leaves **304 Git objects** and **131
unresolved `cmd_*` symbols** in that reduced object set. All 429 Git objects resolve those
command symbols. A local-only build therefore needs an intentional dispatcher/reachability
cut, not just omission of unwanted builtin objects.

The integration owner's complete `rust-lld -e main --error-limit=0` check over all 441
objects failed with **130 undefined symbols and zero duplicate-symbol errors**. Independent
`llvm-nm --format=posix` set subtraction over those same objects agrees:

- **26** compiler support symbols (floating-point helpers and 64-bit division/modulo).
  A final toolchain runtime must supply them; that final link has not been demonstrated.
- **14** host-wrapper imports (`pvm_*_wrapper` and `host_log_wrapper`). They need actual
  guest wrapper integration, not success stubs.
- **90** remaining libc/POSIX/platform gaps, partitioned exactly once below.

| Category | Count | Exact unresolved symbols |
|---|---:|---|
| Filesystem namespace/metadata | 22 | `basename chdir chmod closedir dirname fstat fsync getcwd link lstat mkdir mkstemp opendir readdir readlink rename rmdir statvfs symlink umask unlink utime` |
| File descriptors | 5 | `dup dup2 fcntl lseek pread` |
| Time | 6 | `gettimeofday gmtime_r localtime_r mktime strftime setitimer` |
| Processes | 11 | `alarm execl execlp execve execvp fork getpgid pipe setsid tcgetpgrp waitpid` |
| Identity/environment/limits | 12 | `environ geteuid gethostname getpass getpid getppid getpwnam getpwuid getrlimit getuid sysconf uname` |
| Networking | 11 | `connect gethostbyname getservbyname h_errno hstrerror htons inet_ntoa ntohs setsockopt shutdown socket` |
| Signals | 5 | `sigaction sigaddset sigemptyset sigfillset sigprocmask` |
| Library/stdio | 17 | `bsearch fscanf iscntrl isgraph ispunct labs putenv setbuf setlocale setvbuf strcspn strpbrk strspn strtod strtoll strtoull vprintf` |
| Trace2 platform hook | 1 | `trace2_collect_process_info` |
| **Total** | **90** | |

These are gaps in the retained all-builtin object set, not 90 established requirements of
an exercised six-command port. Conversely, already-defined functions may still have
inadequate semantics. For example, current POSIX `open()` maps create/truncate but not
exclusive-create or append, and maps host failures other than NOT_FOUND to `EACCES`.

`llvm-size -A` reports **2,138,356 bytes (2,088.2 KiB) of `.text` sections for Git**, plus
**28,440 bytes (27.8 KiB) for zlib**. The 304-object local selection has 1,471,074 bytes
(1,436.6 KiB) of `.text`. These are sums of input code sections, **not linked PVM blob
sizes or memory requirements**. Default `llvm-size` includes other read-only sections in
its `text` column and gives larger totals (2,711,063 Git bytes; 39,678 zlib bytes).
No final dead stripping, interpreter cost, gas consumption, or usable heap budget was measured.

## Filesystem and scheduling blockers

Host evidence: [`computer.rs`](../../rust/crates/pvm-runtime/src/computer.rs), especially
`ComputerDevices::{fs_open,fs_stat,fs_sync,fs_remove,fs_list_record}`,
`validate_computer_path`, and `ComputerSupervisor::{spawn_workspace_child,
drive_workspace_child,drive_background,mount_file}`. The browser counterpart is
[`pvm-computer.js`](../../js/packages/pvm-browser-runtime/src/pvm-computer.js).

The Rust host stores file bytes under flat `/home/...` path keys. `fs_stat` supplies size
only; there is no directory record, modification timestamp, rename operation, or
exclusive-create flag. Paths are capped at 200 bytes and reject NUL, trailing `/`, and
`.`/`..` segments. Current constants allow 64 files, 1 MiB per file, and 16 open handles.
`fs_seek` and `fs_truncate` exist. `fs_remove` refuses paths open in that device;
`fs_sync` merely checks that the handle exists, so it is not a demonstrated durable flush.

### 1. Empty directories must survive discovery and relaunch

Prefix filtering over `fs_list` can synthesize **nonempty** intermediate directories.
It cannot preserve empty ones. Git's
[`setup.c:is_git_directory`](https://github.com/git/git/blob/e9019fcafe0040228b8631c30f97ae1adb61bcdc/setup.c#L415)
requires accessible `objects` and `refs` directories as well as a valid `HEAD`.
A newly initialized repository needs these before the first object/ref file exists.
Consequently, `mkdir()` returning success without recording anything is not a valid
implementation. Directory existence must have a persistent representation (host directories
or a specified guest representation), consistent across `mkdir/rmdir`, `access/stat`,
`opendir/readdir`, cwd resolution, and subsequent guest launches. Representation files
would also consume quota and must not leak into normal directory enumeration.

### 2. Lock acquisition and publication need real atomicity

Git's
[`tempfile.c:create_tempfile_mode`](https://github.com/git/git/blob/e9019fcafe0040228b8631c30f97ae1adb61bcdc/tempfile.c#L142)
uses `O_CREAT|O_EXCL`; `rename_tempfile` closes and renames the temporary file to publish it.
[`object-file.c:finalize_object_file_flags`](https://github.com/git/git/blob/e9019fcafe0040228b8631c30f97ae1adb61bcdc/object-file.c#L408)
uses hardlink/unlink or rename. Selecting rename mode changes that publication path; it
does not make a copy/remove implementation atomic or necessarily remove all `link` references.

The host is **not a single-process filesystem**. It retains live workspace children and
piped background processes. They run cooperatively, start with copied file views, and
merge modifications/removals into supervisor state when driven, including writes made
before a child fault. Cooperative scheduling does not provide a repository-wide critical
section or coherent POSIX locking across these views. No two-writer race was executed in
this assessment, but the source provides no global exclusion guarantee to justify calling
stat-then-create race-free. Copy/write/remove can expose incomplete replacement or lose
updates and cannot preserve the old destination on every failed/cancelled publication.

A usable mutation path needs atomic exclusive creation and atomic replacement **against
one authoritative namespace across participating processes**, plus defined merge/conflict,
open-handle, error, cancellation, and persistence behavior. Adding a key move only to an
individual process's copied map would not establish those cross-process guarantees.
Restricting an experiment to an explicitly isolated writer could reduce its concurrency
scope, but would not make copy/remove atomic or establish general Git compatibility.

### 3. Wall-clock-per-stat is not a proven index strategy

Git's
[`statinfo.c:match_stat_data`](https://github.com/git/git/blob/e9019fcafe0040228b8631c30f97ae1adb61bcdc/statinfo.c#L64)
compares cached mtime/size and selected metadata. In
[`read-cache.c:is_racy_stat`](https://github.com/git/git/blob/e9019fcafe0040228b8631c30f97ae1adb61bcdc/read-cache.c#L355),
additional content checking depends on a nonzero index timestamp and the cached entry's
mtime being at least that timestamp. The index timestamp itself comes from `fstat` when
loading the index (`do_read_index`).

Constant-zero stat timestamps can miss same-size edits. Replacing zero with the current
wall clock on every `stat`/`fstat` is **not an unconditional rehash mechanism**: calls in one
clock tick can compare equal (the experiment defines `NO_NSEC`), the wall clock can move
backward, and a newly sampled index timestamp is not its actual write time. Equal sampled
values can trigger Git's racy fallback in some cases, but do not prove every refresh path
checks content. `core.checkStat=minimal` still compares mtime and size; it does not force
hashing.

The port needs coherent file metadata compatible with Git's racy-index rules, or an explicit,
reviewed Git-side strategy that guarantees content validation where required without trusting
fabricated stat equality. Same-size rewrites within one tick, after index reload/relaunch,
and during clock rollback are necessary acceptance cases, not completed experiments here.

### 4. Quotas constrain repositories, not a fixed number of commits

The earlier host-Git experiment recorded two commits on two worktree files with empty
templates: **16 files** (7 loose objects, 7 Git metadata files, 2 worktree files), longest
relative path 70 bytes. This is historical experiment evidence, not a PVM result.
The retained `toyrepo` has since been packed: current inspection finds **15 files**, including
`packed-refs`, a commit-graph, and a 415-byte pack with `.idx`/`.rev` companions; its longest
relative path is 68 bytes. The original loose snapshot is no longer directly inspectable.

Both tiny states fit the file-count and per-file caps in isolation. Neither supports a
universal “12–15 commits” limit: object reuse, changed blobs, directory trees, refs/reflogs,
other mounted files, temporary lock/object files, and directory representation all affect
headroom. Packing can help but is not proven to run in PVM; packs can also exceed 1 MiB.
Quota failures must preserve repository integrity and map truthfully to POSIX errors such
as `ENOSPC`, including temporary-file creation and partial multi-call operations.

## Other runtime requirements

Local commands do not imply subprocess-free execution. `builtin/commit.c` invokes hooks
and automatic maintenance; `diff.c` supports external diff/textconv, and attributes can
select filters. A deliberately bounded command mode must disable or explicitly reject
unsupported hooks, signing, filters, pagers, editors, external helpers, and maintenance.
`commit -m` and an empty pager setting alone are not a sufficient restriction. A host
package-spawn capability is not a drop-in implementation of POSIX `fork/exec`.

Real time conversion/formatting and author/committer environment handling remain necessary.
Networking can be excluded from a deliberately cut local build, but unexecuted paths are
not thereby proven unreachable. Unsupported calls must fail honestly rather than report
success for missing behavior. Signal/timer cleanup, fd positioning/append, permission and
symlink policy, formatting, and allocator behavior also require an explicit contract.
The current Doom libc shim uses a bump allocator with no-op `free`; no Git workload heap
requirement or safe fixed arena size has been measured.

## Next bounded slice

**Resolve the filesystem contract before scaffolding a Git application.** The next slice
should specify and exercise one small repository-storage scenario on the actual host:

1. Persist an empty directory tree and rediscover it after guest relaunch.
2. Acquire one exclusive lock with two live workspace/process participants: exactly one
   wins, and the loser cannot truncate or replace it through a stale copied view.
3. Replace an existing file atomically; inject failure/cancellation before publication and
   ensure readers see only the old or complete new contents. State separately what survives
   a host restart; the current `fs_sync` is not evidence of crash durability.
4. Establish the metadata/content-validation strategy with same-size, same-tick edits and
   index reload; include quota exhaustion without corrupting the previous state.

Only after those guarantees are demonstrated should a subsequent Git-specific slice close
the required runtime gaps, link a deliberately bounded dispatcher, and run
`init → add → commit -m → log/status/diff → relaunch → log` with controlled configuration.
That sequence is a proposed acceptance target, **not a completed port or execution claim**.
The retained compilation artifacts make this investigation concrete, but do not establish
that zero host-ABI changes suffice, that all gaps are guest-only, or any implementation
schedule/line-count estimate.
