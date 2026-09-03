# Vim (tiny) → freestanding riscv32 PolkaVM guest — compatibility assessment

Assessment artifacts: `/tmp/vim-assessment/` (`vim/` shallow clone, `stubs/` header shims,
`obj/` 131 objects, `vim/src/auto/{config.h,osdef.h,pathdef.c}` hand-written). No repository
files were modified. Method mirrors the working `apps/vi-tty` neatvi port (same clang flags,
same `-I pvm-posix/include -I doom/c_src/include` header chain, shims first in include path).

## (a) Revision and feature level

- Vim `f1b454912996d6417acd0d738de0eb7b60902c56` — "patch 9.2.1036" (github.com/vim/vim, shallow clone 2026-09-02).
- Compiled with `-DHAVE_CONFIG_H -DFEAT_TINY` and a hand-written 35-line `auto/config.h`
  (full text in Appendix A), empty `auto/osdef.h`, trivial `auto/pathdef.c`.
- FEAT_TINY verified effective: `eval.o`, `syntax.o`, `spell.o`, `terminal.o`, `vim9execute.o`
  all have 0 text bytes; total Vim text = **~925 KiB** (biggest: regexp.o 120K, option.o 40K,
  ex_docmd.o 39K). Vim-side bss is tiny (~20 KiB); the 41 MiB bss total is the shims' own
  arenas (libc_shim 32 MiB heap, pvm_posix 8 MiB).
- Toolchain: `clang 22.1.8 --target=riscv32-unknown-unknown -march=rv32emc -mabi=ilp32e
  -std=gnu99 -fno-builtin -ffreestanding -nostdinc -fno-stack-protector -fPIC -Os -w`.

## (b) Missing headers

Two classes: headers that do not exist anywhere in the freestanding set (new stubs), and
existing pvm/doom headers that lack types/macros Vim needs (augmented via `#include_next`
shadow headers, exactly the pattern pvm-posix already uses internally).

### New headers (absent from both include dirs)

| Header | Needed by | Stub difficulty |
|---|---|---|
| `float.h` | `macros.h:288` → every one of the 129 files | **trivial** (6 constants: DBL_MAX/DBL_MIN/DBL_EPSILON/DBL_DIG…) |
| `dirent.h` | `os_unix.h` (with HAVE_DIRENT_H) → fileio.c, filepath.c, os_unix.c | **trivial** types (`DIR`, `struct dirent{d_name}`); runtime must fail gracefully (no dir-listing capability in the pvm ABI) |
| `sys/wait.h` | os_unix.c (waitpid/WNOHANG/WIFEXITED…) | **trivial** (macros + 2 protos) |
| `termcap.h` | term.c, termlib.c (with HAVE_TERMCAP_H) | **trivial** — prototypes for Vim's *own* `src/termlib.c` plus `extern UP/BC/PC/ospeed` |
| `locale.h` | vim.h (with HAVE_LOCALE_H; needed so misc1.c's `get_cmd_output` guard fires — filepath.c references it unconditionally) | **trivial** (LC_* constants + `setlocale`) |
| `sgtty.h` | termlib.c:17 (unconditional include) | **trivial** — empty file suffices |

### Existing headers needing augmentation (shadow + `#include_next`)

| Header | Missing pieces | Difficulty |
|---|---|---|
| `signal.h` | `sig_atomic_t` (globals.h uses it → every file), SIGUSR1/SIGTSTP/SIGCONT/… numbers | trivial |
| `sys/time.h` | only `struct timeval` exists; add `struct timezone`, `gettimeofday` proto | trivial |
| `sys/stat.h` | doom's `struct stat` lacks `st_uid/st_gid/st_dev/st_ino/st_rdev/st_atime/st_ctime`, S_IFLNK/S_ISLNK, S_IR* group/other bits; add lstat/fstat/chmod/umask protos | trivial header; **medium semantics** (shim `stat()` always fails → file-changed checks, swap-file naming, backup logic degrade) |
| `termios.h` | pvm one lacks `ECHOE` (required to select Vim's NEW_TTY_SYSTEM path!), `TCSANOW`, `VERASE`, `VINTR`, IXANY/ONLCR/CSIZE/PARENB/NOFLSH | trivial |
| `unistd.h` | ~25 prototypes (dup, getuid/getgid/getpid, unlink, chdir, getcwd, fork, execvp, pipe, fsync, umask-adjacent…) | trivial |
| `stdio.h` | FILE-function protos: fputs/fputc/putc/getc/fgets/fread/fwrite/fseek/ftell/rewind/rename/fdopen/… | trivial header; implementation is the real work (see (c)) |
| `stdlib.h` | strtod, bsearch, labs, `int putenv(const char*)` (matching Vim's own misc2.c definition) | trivial |
| `string.h` | strcoll, strtok, strpbrk/strspn protos | trivial |
| `ctype.h` | iscntrl, isgraph, ispunct | trivial |
| `math.h` | INFINITY/NAN, ceil, floor(present)/log10, isnan, isinf | trivial |
| `time.h` | `struct tm`, localtime/mktime/ctime/strftime protos (declared to satisfy time.c; none end up referenced at link in tiny) | trivial |

Representative first-contact errors, before stubbing (verbatim):

```
macros.h:288:10: fatal error: 'float.h' file not found
structs.h:2753:20: error: field has incomplete type 'struct timeval'
proto/os_unix.pro:96:10: error: unknown type name 'sig_atomic_t'
os_unix.c:3877:26: error: variable has incomplete type 'struct sgttyb'   (ECHOE missing → old-tty path chosen)
os_unix.c:4667:36: error: use of undeclared identifier 'WNOHANG'
fileio.c:5095:5:  error: use of undeclared identifier 'DIR'
main.c:3610:44:   error: no member named 'st_uid' in 'struct stat'
fuzzy.c:138:23:   error: use of undeclared identifier 'INFINITY'
ex_cmds.c:303:9:  error: call to undeclared function 'strcoll'
os_unix.c:2634:1: error: static declaration of 'strerror' follows non-static declaration  (fixed by HAVE_STRERROR)
```

Two link-level wrinkles found the same way (fixed purely in config.h/stub headers, no source edits):
- `duplicate symbol: qsort` (misc2.c compiles its own unless `HAVE_QSORT`) and
  `duplicate symbol: UP/BC/PC/ospeed` (term.c vs termlib.c; fixed with `HAVE_UP_BC_PC`/`HAVE_OSPEED` + externs in termcap.h).
- `term_set_winsize`/`get_cmd_output` initially undefined: term.c only emits the former under
  `HAVE_TGETENT`; misc1.c only emits the latter under `FEAT_EVAL || HAVE_LOCALE_H`. Defining
  `HAVE_TGETENT` + compiling Vim's own `termlib.c` (its bundled mini-termcap; falls back to
  Vim's builtin xterm termcap when /etc/termcap is absent) and `HAVE_LOCALE_H` resolves both.

## (c) Missing libc functions (link-verified)

Ground truth: `rust-lld -e main --error-limit=0` over all 131 objects (129 Vim + pvm_posix.o +
libc_shim.o) reports **exactly 73 undefined symbols**, cross-checked with llvm-nm set arithmetic
(identical list). Of these, 14 are the `pvm_*_wrapper`/`host_log_wrapper` Rust runtime imports
(expected — provided by the app-kit Rust wrapper crate, same as vi-tty) and 13 are compiler
builtins. That leaves **46 genuine libc gaps** (45 functions + `environ`).

| Category | Symbols (referenced by) | Verdict |
|---|---|---|
| compiler-rt builtins | `__adddf3 __muldf3 __divdf3 __eqdf2 __nedf2 __ltdf2 __gtdf2 __gedf2 __fixdfsi __fixunsdfsi __floatunsidf __divdi3 __moddi3` | **have-in-shim** — supplied by Rust `compiler_builtins` at the final PolkaVM link (doom already links f64 code this way). Verify rv32e coverage once at link time. |
| stdio (FILE) | `fgets fread fseek ftell rewind getc putc fputc fputs` (fileio, scriptfile, tag, getchar `:mkexrc`, help, map, message, option `:mkvimrc`, session, misc1, os_unix, termlib) | **needs new capability in pvm-posix**: a real (if small) FILE layer. Current `fopen` is single-file, read-only, whole-file-in-memory. Feasible without ABI changes: back FILE with a memory buffer over `pvm_fs_open/read/write/truncate` (biggest single work item, ~200–300 lines). |
| fd I/O | `lseek` (fileio, memfile, memline), `dup` (fileio, main, ui, os_unix) | `lseek`: pvm fs ABI has **no seek** — but with `'noswapfile'` memfile keeps `mf_fd < 0` and never seeks; **trivial stub** (return -1) is safe. `dup`: **trivial stub** (return fd for 0-2, else -1). |
| termios/tty | *(none undefined)* — tcgetattr/tcsetattr/ioctl(TIOCGWINSZ)/isatty/poll all present | **have-in-shim**. Vim's RealWaitForChar uses `poll()` on stdin (pvm supports exactly that). Note SIGWINCH never fires → resize only detected via existing TIOCGWINSZ path. |
| process | `fork execvp pipe waitpid` (os_unix mch_call_shell) | **trivial fail-stubs** for a terminal-only Vim (`:!`/filters return error). Later, *medium*: rewire `mch_call_shell` onto `pvm_process_spawn/pipe_*/process_wait` (capability exists!). Not a blocker. |
| signals | *(none undefined)* — signal/raise/kill stubbed in shims | **have-in-shim** (as inert no-ops; deadly-signal handling simply absent). |
| time | *(none undefined)* — `time`/`clock` in libc_shim; localtime/strftime declared but unreferenced in tiny | **have-in-shim** (timestamps are constant 0 → 'timestamps' checks inert, matches existing `stat()` behavior). |
| pwd/uid | `getuid getgid getpid gethostname environ` (+ `getpwuid` never referenced: HAVE_PWD_H off) | **trivial stubs** (uid/gid 0, pid 1, fixed hostname, `char **environ = 0`). |
| fs metadata | `fstat chmod umask mkdir rmdir unlink link` (bufwrite, memline, fileio, ex_cmds, misc1, os_unix); `opendir readdir closedir` (fileio, filepath) | **trivial fail-stubs**, *provided* the port sets `nobackup nowritebackup noswapfile viminfo=` so failure paths are never load-bearing. `unlink`/`mkdir` as real features would need new host fs capabilities. |
| memory | *(none undefined)* — malloc/realloc/free/calloc in shims | **have-in-shim** (bump allocator, free is no-op — long editing sessions will grow monotonically; acceptable first-order, same trade-off vi-tty accepted). |
| setjmp | *(none undefined)* — avoided by leaving HAVE_SETJMP_H undefined | **have-in-shim** (n/a). |
| select/poll | *(none undefined)* — HAVE_POLL → pvm `poll()` | **have-in-shim**. |
| str/locale | `strcoll strtod strtok bsearch labs setlocale` | **trivial stubs** (strcoll→strcmp, strtod→atof wrapper, setlocale→"C"). |
| ctype | `iscntrl isgraph ispunct` | **trivial** (3 one-liners). |
| math | `ceil floor pow log10 isnan isinf` (fuzzy.c, linematch.c, strings.c) | **trivial** soft-float implementations (~40 lines); pow/log10 only need integer-ish accuracy here. |

## (d) Compile results

- Final: **129/129 files compile cleanly** (Vim's full `BASIC_SRC` list of 128 files including
  `auto/pathdef.c`, plus `termlib.c`) — zero warnings suppressed beyond `-w`, zero source edits.
- Iteration profile: first attempt died instantly (`float.h` missing). After config.h + first
  stub wave: 8/12 of the targeted core files OK (failures: ex_cmds.c, fileio.c, option.c,
  os_unix.c). Full sweep: 116/128 → 125/128 (stdio/math/ctype/time decls) → 128/128 (HAVE_MATH_H).
- Every failure across all iterations was header/declaration-level; **no file hit a
  structural/source-level incompatibility** (no varargs ABI issues, no pointer-size assumptions
  tripped on ilp32e at compile time).

## (e) Bottom line

**No true blocker exists for a terminal-only tiny Vim on PolkaVM.**

Must be added to pvm-posix (or a vim-specific glue file, vi-tty style):
1. ~30 trivial stubs/one-liners: ctype trio, math six, strcoll/strtod/strtok/bsearch/labs/
   setlocale, uid/gid/pid/hostname/environ, dup, and fail-stubs for lseek/fstat/chmod/umask/
   mkdir/rmdir/unlink/link/opendir/readdir/closedir/fork/execvp/pipe/waitpid. (~150 lines total.)
2. The one real work item: a small FILE layer (fopen r/w modes, fgets/fread/fwrite/fseek/ftell/
   rewind/getc/putc/fputs/fputc/fclose) buffering whole files in memory over the existing
   `pvm_fs_*` ABI. Without it `:w` still works (fileio.c uses fd write) but viminfo/tags/
   `:source`/`:mksession` paths reference these symbols, so they must at least exist.
3. 17 stub/shadow headers (~7 KB total, already written in `/tmp/vim-assessment/stubs`).

Must be compiled out / configured away via config.h + option defaults:
- Everything already off in FEAT_TINY (eval, syntax, spell, GUI, terminal, X11…).
- Keep undefined: HAVE_SELECT, HAVE_SETJMP_H, HAVE_PWD_H, HAVE_SIGACTION, HAVE_GETTIMEOFDAY,
  HAVE_PUTENV, TERMINFO, HAVE_ICONV, HAVE_LANGINFO — all successfully avoided.
- Define (the working set, Appendix A): UNIX, sizes, HAVE_POLL(+_H), HAVE_TERMIOS_H,
  HAVE_SYS_{TIME,IOCTL,WAIT}_H, HAVE_DIRENT_H, HAVE_GETCWD, HAVE_STRERROR, HAVE_MATH_H,
  HAVE_TGETENT + HAVE_TERMCAP_H (with bundled termlib.c), HAVE_LOCALE_H, HAVE_QSORT,
  HAVE_OSPEED/HAVE_UP_BC_PC (+_EXTERN variants).
- Runtime defaults in glue: `:set noswapfile nobackup nowritebackup viminfo= shell=` and
  `-u NONE -i NONE`-equivalent startup, terminal forced to builtin `xterm` termcap.

Risks (second-order, not seen by the compiler):
- libc_shim's `vsnprintf` lacks `%f/%e/%g` and `%S/%ld` corner semantics Vim's message code
  leans on less in tiny, but Vim mostly uses its own vim_snprintf — low risk.
- libc_shim `qsort` bails on elements > 256 bytes (fine for tiny's sort users) — now bypassed
  anyway since HAVE_QSORT selects... note: HAVE_QSORT makes Vim *use* libc qsort; if a >256-byte
  element ever appears, drop HAVE_QSORT to use Vim's own qsort in misc2.c instead (zero cost).
- Bump allocator never frees; Vim churns allocations far more than neatvi. Budget the arena
  (≥32 MiB) or add a free-list.
- `mch_early_init`/startup paths call `getcwd` — stub must return "/" not NULL to avoid an
  early FAIL.

## (f) Recommended next steps

1. Copy the working scaffold: new `apps/vim-tty/` mirroring `apps/vi-tty` (`build.rs` two-pass
   cc build, `package.sh`, Rust lib.rs wrapper). Vendor Vim at `f1b45491` (or pin a release tag)
   with `src/` only; check in `auto/config.h`, empty `auto/osdef.h`, `auto/pathdef.c`, and the
   17 stub headers from `/tmp/vim-assessment/stubs` under `vendor-shims/`.
2. Write `c_src/vim_glue.c`: the ~30 trivial stubs + `environ` + startup defaults
   (`main → vim_upstream_main` with argv `["vim","-u","NONE","-i","NONE","-X"]`, TERM=xterm),
   with `-Dmalloc=pvm_posix_malloc` etc. as in vi-tty's build.rs.
3. Implement the FILE layer in pvm-posix (new `pvm_stdio.c`), reusing its existing single-file
   fopen as the read path; add write-mode flush-on-fclose via `pvm_fs_write/truncate`.
4. Link through the Rust wrapper (drop `--unresolved-symbols=ignore-all` that vi-tty uses, so
   regressions surface); confirm compiler_builtins resolves the 13 `__*df*` intrinsics on rv32e.
5. Smoke test in the term host: startup screen, `i`/`ESC`, `:w`/`:e` roundtrip through pvm_fs,
   window resize via TIOCGWINSZ, `:q`.
6. Second wave (optional): map `mch_call_shell` onto `pvm_process_spawn`/`pvm_pipe_*` to light
   up `:!` and filters — the host capability already exists.

## Appendix A — working `auto/config.h`

```c
#define UNIX 1
#define VIM_SIZEOF_INT 4
#define VIM_SIZEOF_LONG 4
#define SIZEOF_OFF_T 4
#define SIZEOF_TIME_T 4
#define USEMEMMOVE 1
#define RETSIGTYPE void
#define SIGRETURN return
#define HAVE_ERRNO_H 1
#define HAVE_STRING_H 1
#define HAVE_STDLIB_H 1
#define HAVE_STDINT_H 1
#define HAVE_UNISTD_H 1
#define HAVE_FCNTL_H 1
#define HAVE_POLL_H 1
#define HAVE_POLL 1
#define HAVE_TERMIOS_H 1
#define HAVE_SYS_TIME_H 1
#define HAVE_SYS_IOCTL_H 1
#define HAVE_DIRENT_H 1
#define HAVE_GETCWD 1
#define HAVE_SYS_WAIT_H 1
#define HAVE_STRERROR 1
#define HAVE_MATH_H 1
#define HAVE_TGETENT 1
#define HAVE_LOCALE_H 1
#define HAVE_TERMCAP_H 1
#define HAVE_QSORT 1
#define HAVE_OSPEED 1
#define OSPEED_EXTERN 1
#define HAVE_UP_BC_PC 1
#define UP_BC_PC_EXTERN 1
```

## Appendix B — full 73-symbol undefined list (rust-lld, --error-limit=0)

Runtime imports (Rust wrapper provides): pvm_tty_{read,write,get_size,set_mode}_wrapper,
pvm_fs_{open,read,write,truncate,close}_wrapper, pvm_core_{yield,exit,environment}_wrapper,
host_log_wrapper.
Builtins: __adddf3 __divdf3 __divdi3 __eqdf2 __fixdfsi __fixunsdfsi __floatunsidf __gedf2
__gtdf2 __ltdf2 __moddi3 __muldf3 __nedf2.
Libc gaps: bsearch ceil chdir chmod closedir dup environ execvp fgets floor fork fputc fputs
fread fseek fstat ftell getc getcwd getgid gethostname getpid getuid iscntrl isgraph isinf
isnan ispunct labs link log10 lseek mkdir opendir pipe pow putc readdir rewind rmdir setlocale
strcoll strtod strtok umask unlink waitpid.
