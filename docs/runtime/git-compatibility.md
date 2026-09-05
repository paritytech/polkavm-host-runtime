# Git (local-only) → freestanding riscv32 PolkaVM guest — compatibility assessment

Assessment artifacts: `/tmp/git-assessment/` (`git/` shallow clone, `zlib/` shallow clone,
`stubs/` 40 header shims 20.3 KB, `obj/` 441 objects, `toyrepo/` file-budget experiment).
No repository files were modified. Scope is **local plumbing only** — `init`/`add`/`commit`/
`status`/`log`/`diff`, no remotes, no network. Method mirrors
`docs/runtime/vim-tiny-compatibility.md` and `ssh-client-compatibility.md`: same clang 22.1.8
`--target=riscv32-unknown-unknown -march=rv32emc -mabi=ilp32e -std=gnu99 -fno-builtin
-ffreestanding -nostdinc -fno-stack-protector -fPIC -Os -w`, same
`-I pvm-posix/include -I doom/c_src/include` chain with shims first (29 of the 40 stubs are the
ssh assessment's, reused verbatim), ground truth via `rust-lld -e main --error-limit=0` +
llvm-nm over all objects.

## (a) Revision and feature level

- Git `e9019fcafe0040228b8631c30f97ae1adb61bcdc` = tag **v2.55.0** (github.com/git/git, shallow
  clone 2026-09-05). zlib **v1.3.1** (madler/zlib) — git's one hard external library dependency.
- Build configuration (command-line defines, mirroring the Makefile knobs):
  `NO_CURL NO_OPENSSL NO_PTHREADS NO_ICONV NO_GETTEXT NO_MMAP NO_REGEX NO_UNIX_SOCKETS NO_IPV6
  NO_STRCASESTR NO_MEMMEM NO_STRLCPY NO_QSORT_S NO_SETENV NO_UNSETENV NO_MKDTEMP NO_STRTOUMAX
  NO_INET_NTOP NO_INET_PTON NO_NSEC NO_GETPAGESIZE` plus the path/pager/template `-D` strings the
  Makefile normally injects. Generated headers (`command-list.h`, `hook-list.h`, `config-list.h`,
  `version-def.h`) produced with git's own `tools/generate-*.sh` — host-side codegen, no source
  edits.
- Hash: **both** SHA-1 implementations compile — `block-sha1/sha1.o` (`SHA1_BLK`) and the
  upstream-default collision-detecting `sha1dc/{sha1,ubc_check}.o + sha1dc_git.o` (`SHA1_DC`);
  SHA-256 via bundled `sha256/block/sha256.o` (`SHA256_BLK`). No OpenSSL anywhere.
- One structural note up front: git is a single binary whose `git.c` command table references
  **all 130 builtins** (link-verified: with only the six local builtins compiled, exactly 130
  `cmd_*` symbols are undefined; with all builtins compiled, zero). A port either links
  everything (~2 MiB text, measured below) or trims the `commands[]` table — a
  BusyBox-style decision, not a compile problem.

## (b) Missing headers

First-contact errors (verbatim, in discovery order — every one header/declaration-level):

```
compat/posix.h:115:10: fatal error: 'locale.h' file not found
compat/posix.h:125:10: fatal error: 'regex.h' file not found        (NO_REGEX also needs -Icompat/regex)
compat/posix.h:126:10: fatal error: 'utime.h' file not found
compat/posix.h:150:10: fatal error: 'sys/statvfs.h' file not found
git-compat-util.h:325:10: error: no member named 'st_uid' in 'struct stat'
git-compat-util.h:638:27: error: expected ')'                        (PRIuMAX missing)
strbuf.c:592:9:   error: call to undeclared function 'readlink'
xdiff/xprepare.c:293:11: error: use of undeclared identifier 'PTRDIFF_MAX'
sha1dc/sha1.c:10:10: fatal error: 'memory.h' file not found
compat/regex/regcomp.c:3584:27: error: call to undeclared function 'iscntrl'
progress.c:75:19: error: variable has incomplete type 'struct itimerval'
parallel-checkout.c:628:31: error: use of undeclared identifier 'POLLNVAL'
run-command.c:853:23: error: use of undeclared identifier 'NSIG'
```

### New headers (absent from all include dirs) — 5 beyond the reused ssh/vim stubs

| Header | Needed by | Difficulty |
|---|---|---|
| `utime.h` | compat/posix.h unconditional; `utime()` used by commit-graph, object-file, packfile, rerere freshening | trivial types; runtime no-op |
| `sys/utsname.h` | compat/posix.h; `uname()` only feeds bugreport/ident fallbacks | trivial |
| `sys/statvfs.h` | compat/posix.h; only `diagnose.c` calls `statvfs` | trivial fail-stub |
| `libgen.h` | compat/posix.h (`basename`/`dirname`) | trivial (~15 lines) |
| `memory.h` | `sha1dc/sha1.c` | one line (`#include <string.h>`) |

### Existing headers needing augmentation (shadow + `#include_next`, pvm-posix's own pattern) — 12

| Header | Missing pieces | Difficulty |
|---|---|---|
| `sys/stat.h` | doom's 3-field `struct stat` is unusable: git's index stat-cache reads `st_dev/st_ino/st_uid/st_gid/st_nlink/st_atime/st_ctime`. Full replacement struct (no `include_next`), all `S_IF*`/`S_IS*`/permission bits, lstat/fstat/chmod/mkdir/umask/mkfifo protos | trivial header; **semantics are the real story, see (e)** |
| `inttypes.h` | `PRIuMAX/PRIdMAX/PRIxMAX/SCNuMAX` (+ strtoimax/strtoumax protos) — `PRIuMAX` appears in `die()` calls in git-compat-util.h itself, so nothing compiles without it | trivial |
| `stdint.h` | `PTRDIFF_MAX`, `SIZE_MAX` | trivial |
| `ctype.h` | iscntrl/isgraph/ispunct/isblank/isascii | trivial |
| `stdio.h` | vprintf, putc, setbuf, fscanf, freopen, rewind, fgetpos/fsetpos, tmpfile, FILENAME_MAX | trivial header; pvm-posix FILE layer already exists (Vim work item 3, since landed) |
| `stdlib.h` | bsearch, strtod, strtol/strtoul/strtoll/strtoull, mkstemp, mkdtemp, labs/llabs, system | trivial |
| `time.h` | gmtime/gmtime_r/mktime/ctime/asctime/difftime | trivial header; **implementation is a real work item, see (c)** |
| `signal.h` | sigfillset, SA_RESTART, NSIG | trivial (handlers stay inert) |
| `unistd.h` | ~20 protos: readlink/symlink/rmdir/lseek/pread/sysconf/tcgetpgrp/getpgid/getppid/execve/execl/execlp/sleep/truncate + `_SC_OPEN_MAX`/`_SC_PAGESIZE`/`R_OK…F_OK` | trivial |
| `poll.h` | POLLNVAL, POLLPRI | trivial |
| `sys/time.h` | `struct itimerval`, setitimer/getitimer (progress.c display timer) | trivial no-ops |
| `netdb.h` | hstrerror, h_errno, `struct servent`/getservbyname (connect.c — dead code locally, must still parse) | trivial |

## (c) Compile + link results

- **Git: 429/429 objects compile cleanly, zero source edits**: 283 `LIB_OBJS` (incl. all 7
  xdiff, all reftable, all refs backends), **all 130 builtins**, `git.o`, 5 hash objects
  (block-sha1, sha256-block, sha1dc×3), 9 `COMPAT_OBJS` (strcasestr, memmem, strlcpy, qsort_s,
  setenv, unsetenv, strtoumax, strtoimax, mmap), `compat/regex/regex.o`
  (`-DGAWK -DNO_MBSUPPORT`).
- **zlib: 10/10 core objects compile** (adler32, crc32, deflate, inflate, inffast, inftrees,
  trees, zutil, compress, uncompr) against the same stub chain — zlib's default allocator lands
  on the shim malloc; the `gz*` stdio family is not compiled and not referenced by git.
- Iteration profile: first contact fatal (`locale.h`) → wave 1 (5 new stubs + stat/inttypes/
  stdint/unistd/stdio shadows): **285/304** of the local-slice sweep → wave 2 (ctype, memory.h,
  poll, itimerval, stdlib/time/netdb decls): **301** → three declaration-level stragglers
  (h_errno, SA_RESTART/_SC_OPEN_MAX/NSIG, getservbyname/tcgetpgrp, execve) → **304/304**; full
  builtin sweep then failed on exactly one symbol (`execlp`, builtin/help.c) → **429/429**.
  **No file hit a structural incompatibility** — no pointer-size or varargs issue on ilp32e in
  either sweep (`timestamp_t` is `uintmax_t` = 64-bit, fine; 32-bit `off_t` is irrelevant under
  a 1 MiB file cap).
- Link (`rust-lld -e main --error-limit=0`, all 441 objects incl. current `pvm_posix.o` +
  `libc_shim.o`): **130 undefined symbols, 0 duplicates** = 26 compiler-rt builtins (`__*df3`,
  `__*sf3`, 64-bit div/mod — Rust `compiler_builtins` provides these at the final PolkaVM link,
  as in doom/vim/dropbear) + 14 expected `pvm_*_wrapper`/`host_log_wrapper` imports + **90
  genuine libc/POSIX gaps**.
- Size: total text ≈ **2,088 KiB** for all 429 objects; the six-command local slice (305
  objects) is ≈ 1,441 KiB (biggest: sequencer 56K, diff 54K, compat/regex 30K, apply 29K,
  revision 29K). Roughly 2.2× the Vim port; well within PolkaVM blob practice. zlib adds 28 KiB.

### Link-verified gap table (90 symbols, referencing objects from llvm-nm)

| Category | Symbols (referenced by) | Verdict |
|---|---|---|
| fs metadata + dirs (23) | `lstat`(25 objs) `fstat`(22) `opendir/readdir/closedir`(21: dir, refs/files-backend, odb/source-loose, commit-graph…) `unlink`(30) `rename`(10: lockfile-committed via refs, object-file, index) `mkdir`(9) `link`(2: object-file, midx-write) `rmdir symlink readlink chmod umask utime mkstemp statvfs fsync getcwd chdir basename dirname` | **needs the pvm-posix fs/fd expansion** — the one real guest work item, fully analysed in (e). `fsync`→`fs_sync`, `unlink`→`fs_remove`, `lseek/pread`→`fs_seek` already have host ops |
| fd I/O (2) | `lseek` (csum-file, pack-write, reftable/stack, odb/streaming), `pread` (wrapper.c) | **have-in-ABI**: `fs_seek` exists since the FILE-layer work; 20-line glue |
| time (6) | `gettimeofday`(10: date, builtin/log, blame…), `gmtime_r localtime_r mktime strftime setitimer` | **real work item ~200 lines**: commit timestamps are load-bearing. `core_clock_wall` exists; needs civil-time conversion (gmtime/mktime pair) + the strftime subset pretty.c uses. setitimer = no-op (progress display) |
| process (13) | `fork pipe waitpid execv execve execvp execl execlp setsid alarm getpgid tcgetpgrp` (run-command; editor/pager/hooks paths) | **trivial fail-stubs** for the slice: hooks skipped when `access()` says ENOENT, pager disabled via `GIT_PAGER=`, editor avoided via `commit -m`. Later, real spawn maps onto `process_spawn` (capability exists) |
| identity (10) | `getuid geteuid getpid getppid getpwnam getpwuid gethostname uname getpass environ getrlimit sysconf` | **trivial stubs** (uid 0, pid 1, static passwd with `pw_dir=$HOME`, `environ=NULL`); ident.c satisfied by `GIT_AUTHOR_*`/`GIT_COMMITTER_*` env or `/home/.gitconfig` |
| net — dead code locally (11) | `socket connect setsockopt shutdown htons ntohs inet_ntoa gethostbyname getservbyname hstrerror h_errno` (connect.c, daemon paths) | **trivial fail-stubs**; never executed by the six local commands |
| signals (6) | `sigaction sigemptyset sigaddset sigfillset sigprocmask` + `setitimer` above | **inert no-ops** (same species as Vim/dropbear) |
| str/ctype/stdlib (16) | `bsearch strtod strtol… labs strcspn strpbrk strspn setlocale iscntrl isgraph ispunct putenv vprintf setbuf setvbuf fscanf` | **trivial** (~150 lines; strcoll not needed — git sorts bytewise) |
| trace2 (1) | `trace2_collect_process_info` | 5-line empty impl (upstream provides per-platform versions in compat/) |

## (d) File-budget experiment (real git, host-side)

`git init` + 2 commits on a 2-file worktree, empty template dir, measured with the real v2.55.0:

```
16 files total: 7 loose objects, 7 .git metadata files
(HEAD, config, index, COMMIT_EDITMSG, logs/HEAD, logs/refs/heads/main,
refs/heads/main), 2 worktree files; longest path 70 bytes
```

So: **~8 metadata files fixed cost, then 3–4 new loose objects per commit** (blob + tree +
commit; unchanged blobs reuse). Against `MAX_COMPUTER_FILES = 64` a toy repo supports roughly
**12–15 commits** before `fs_open` returns LIMIT. Every file is far below the 1 MiB cap, and
`/home/repo/...` paths stay far below the 200-byte path cap. Disabling reflogs
(`core.logAllRefUpdates=false`) saves 2 files.

## (e) Platform-semantic collisions with `polkadot-host-computer/0.1`

The host fs is a flat map of `/home/...` keys: slash-containing names are legal opaque keys
(`validate_computer_path` rejects only `.`/`..` segments, trailing `/`, NUL, >200 bytes) —
there are no directories, no rename, no mtime, `fs_stat` returns size only, `fs_list` returns
every mounted path in one record. Collision by collision:

1. **`.git` object-directory trees vs flat namespace — emulation suffices.**
   `.git/objects/ab/cdef…` is simply a key; `mkdir` (9 referencing objects) becomes a
   success-no-op; `opendir/readdir` becomes a prefix filter over `fs_list` with synthesized
   intermediate-directory dirents; `stat("<prefix>")` reports `S_IFDIR` when any key extends
   `<prefix>/` (a strict generalisation of pvm-posix's existing `is_virtual_home`). At ≤64
   files the O(all-files) listing per readdir is irrelevant.

2. **Atomic `rename()` for lockfiles and objects — emulation acceptable, fs 0.2 op wanted.**
   Every git mutation commits through lockfile.c (`index.lock` → `rename`), and loose objects
   finalize via `link`+`unlink` *or* `rename` (`core.createObject`; compile-time
   `-DOBJECT_CREATION_MODE=OBJECT_CREATION_USES_RENAMES` removes the `link` dependency
   entirely, environment.c:67). The host has no rename: the shim does read → write → remove.
   Non-atomic, but the computer is a **single cooperative process** — no concurrent observer
   exists (piped children run only while the parent is suspended), so the only exposure is a
   crash mid-copy, the same window the ADR already accepts for cancelled processes'
   partial writes. Likewise `O_CREAT|O_EXCL` (lockfile.c:118, tempfile.c) has no host
   exclusive-create bit; a stat-then-create emulation is race-free under cooperative
   scheduling. Verdict: **guest emulation is correct for the slice; a real `fs_rename` is the
   single most valuable fs 0.2 op** (a BTreeMap key move host-side) — it restores the
   crash-consistency story and reftable compaction semantics.

3. **mtime and the racy-index heuristic — emulation suffices, with one mandatory trick.**
   `fs_stat` has no mtime. Returning `st_mtime = 0` is **silently wrong**: read-cache.c's
   `is_racy_stat` is gated on a non-zero index timestamp (`istate->timestamp.sec && …`,
   read-cache.c:355), so zero-mtime entries are never racy *and* `ie_match_stat` compares
   equal zeros — a same-size content edit becomes invisible to `status`/`add`. The correct
   emulation is `st_mtime = clock_wall(now)` on every `stat`: every entry then differs from
   its recorded stat data, forcing a content re-hash on each refresh — semantically exact
   (detection by hashing, the racy-git fallback), costing O(worktree bytes) per `status`,
   bounded by the 64 × 1 MiB = 64 MiB fs ceiling. `core.checkStat=minimal` keeps the rest of
   the fake stat fields (dev/ino/uid/gid) out of the comparison. fs 0.2 mtime makes this
   efficient, not more correct.

4. **64-file / 1 MiB caps — the binding constraint, host-side quota question.**
   Measured budget above: the slice fits with ~40 files of headroom; real use does not
   (a 100-commit history alone needs ~300 loose objects; packing is the cure but `gc`/`repack`
   writes multi-MiB packs that break the 1 MiB cap long before that). `pvm-posix` must map
   `LIMIT` (−6) to `ENOSPC` (today's `open()` maps every failure to `EACCES`/`ENOENT`) so git
   dies with a truthful message. Verdict: no new *op* required — the caps are host constants;
   fs 0.2 should make them per-grant quotas.

5. **readdir over `fs_list` — emulation suffices.** Covered by 1; the only host directory
   op is exactly the one git needs. A `prefix` argument on `fs_list` is a nice-to-have for
   larger quotas, not a slice prerequisite.

### Proposed minimal fs 0.2 op set

1. `polkadot_host_0_1_fs_rename(old_ptr, old_len, new_ptr, new_len) -> status` — atomic key
   move, POSIX overwrite semantics, `DENIED` while either path is open. Unblocks honest
   lockfiles, loose-object finalize, reftable compaction; also what SQLite (delivery-sequence
   priority 1 among "remaining") needs for atomic replacement.
2. `FS_OPEN_EXCLUSIVE` flag bit on the existing `fs_open` (with CREATE: fail `EXISTS` when the
   key is present) — one host branch; makes `O_EXCL` real instead of emulated.
3. `fs_stat` record v2: `{size: u32, mtime_ns: u64}`, host-stamped on write — turns the
   every-status full re-hash into the normal racy-index fast path.
4. Per-grant quotas (file count, per-file bytes) replacing the global 64 / 1 MiB constants;
   `LIMIT` remains the error. Not an op — a grant field.

Explicitly **not** needed: mkdir/rmdir (no directories to make), symlink/link
(`core.symlinks=false` is automatic when the probe fails; `core.createObject=rename`),
chmod/chown (mode bits are synthetic).

## (f) Bottom line + recommended vertical slice

**Feasible with zero host-ABI changes for the toy slice.** 429/429 git objects and 10/10 zlib
objects compile against the established freestanding toolchain with 40 stub headers and no
source edits; all 90 link-level gaps are guest-side, and the four fs-semantics collisions all
have correct (if slower or less crash-hardened) emulations under the computer's cooperative
single-process model. The 64-file cap — not any missing call — is what separates the toy repo
from a useful one.

**Vertical slice — `git init`/`add`/`commit -m`/`log` (+`status`, `diff`) on a toy repo in
`/home/repo`:**

1. pvm-posix fs/fd expansion (the analogue of Vim's FILE layer, ~400–500 lines): fd-path
   tracking for fstat/lstat, `lseek`/`pread` over `fs_seek`, `unlink`→`fs_remove`,
   rename-as-copy, mkdir no-op, dirent emulation + directory-stat synthesis over `fs_list`,
   virtual cwd (`chdir`/`getcwd` — setup.c walks up from cwd to find `.git`), `O_EXCL` and
   `O_APPEND` emulation (reflog appends), `mkstemp` over `core_random`, stat mtime = wall
   clock (see (e).3), `LIMIT`→`ENOSPC`.
2. Time layer (~200 lines): `gettimeofday` over `core_clock_wall` (wrapper already in
   pvm_posix.h), gmtime_r/localtime_r/mktime + the strftime subset pretty.c emits.
3. ~45 trivial stubs (~250 lines, vi-tty style): identity, signals, process/net fail-stubs,
   str/ctype/stdlib, `environ`, `trace2_collect_process_info`.
4. Scaffold `apps/git-tty` mirroring `apps/vim-tty` (build.rs two-pass cc over git + zlib +
   the 40 stubs from `/tmp/git-assessment/stubs`, Rust wrapper crate); link all builtins
   first (2 MiB text), trim the command table later if blob size matters. Launch defaults:
   `HOME=/home`, `GIT_PAGER=`, `GIT_CONFIG_NOSYSTEM=1`, `GIT_AUTHOR_*`/`GIT_COMMITTER_*`,
   `-DOBJECT_CREATION_MODE=OBJECT_CREATION_USES_RENAMES`, `core.logAllRefUpdates=false`,
   `gc.auto=0`.
5. Smoke: `git init`, `add`, `commit -m`, `log`, `status`, `diff` in the term host; relaunch
   and `git log` again to prove `/home` persistence of the object store.
6. After the slice: fs 0.2 (rename, EXCLUSIVE, mtime, quotas) in that order — rename first;
   then packfile-based housekeeping becomes thinkable once per-file quota rises.

Risks (second-order, not compiler-visible):
- **Allocator**: git churns allocations far harder than Vim (mem-pools help, but free is a
  no-op in both shims). Budget the arena ≥64 MiB or give pvm-posix a free-list before `log`
  on non-trivial histories.
- libc_shim `vsnprintf` must honor `%llu`/`%llx` — `PRIuMAX` formats are everywhere in git's
  message paths (Vim flagged adjacent corners; verify once at smoke time).
- sha1dc on rv32e under `-Os` is slow per object; acceptable at toy scale, switch to
  `SHA1_BLK` (also compiled) if hashing dominates.
- `fscanf` is referenced (builtin worktree/gc paths) — the FILE layer lacks it; a fail-stub is
  safe for the slice but must exist.
- The six local commands still initialize trace2/config machinery that reads
  `/etc/gitconfig` — ENOENT paths, verified present in the compiled objects, must stay cheap
  in the fs shim (no yield loops on missing paths).

## (g) Estimated scope

| Item | Size | Kind |
|---|---|---|
| Guest: pvm-posix fs/fd expansion (dirent-over-fs_list, rename-as-copy, cwd, mtime trick) | 400–500 lines C | pvm-posix, reusable by SQLite/BusyBox ports |
| Guest: time layer (civil time + strftime subset) | ~200 lines | pvm-posix, shared |
| Guest: trivial stubs | ~250 lines | glue (vi-tty style) |
| Port scaffold: `apps/git-tty` (build.rs over git+zlib, 40 stub headers, launch env) | config-only | mirror vim-tty |
| Host (fs 0.2, post-slice): `fs_rename` + `FS_OPEN_EXCLUSIVE` + stat mtime + grant quotas | ~120 lines Rust + conformance fixtures | new capability, versioned |
| Compiled footprint | ~2.1 MiB text (+28 KiB zlib) + arenas | measured |

The slice is **3–5 focused days**, dominated by the fs/fd expansion and its dirent semantics,
not by git — which, on the evidence of 429 cleanly compiled objects, is already a well-behaved
freestanding citizen.
