# SSH client → freestanding riscv32 PolkaVM guest — feasibility assessment

Assessment artifacts: `/tmp/ssh-assessment/` (`dropbear/`, `wolfssh/`, `libssh2/` shallow clones;
`build/` hand-written `config.h` + `localoptions.h` + generated `default_options_guard.h`;
`stubs/` 29 header shims, 14.7 KB; `obj/` 258 objects). No repository files were modified.
Method mirrors `docs/runtime/vim-tiny-compatibility.md`: same clang 22.1.8
`--target=riscv32-unknown-unknown -march=rv32emc -mabi=ilp32e -std=gnu99 -fno-builtin
-ffreestanding -nostdinc -fno-stack-protector -fPIC -Os -w`, same
`-I pvm-posix/include -I doom/c_src/include` chain with shims first, ground truth via
`rust-lld -e main --error-limit=0` + llvm-nm over all objects.

## (a) Candidate comparison and pick

| | Dropbear `dbclient` | wolfSSH client | libssh2 minimal client |
|---|---|---|---|
| Revision assessed | `59870ad4` (2026-08-31) | `557f3df4` (2026-09-02) | `4884fc61` (2026-09-03) |
| Crypto dependency | **bundled** libtommath + libtomcrypt (in-tree, same freestanding flags) | external **wolfSSL** (separate clone, `user_settings.h` engineering, own RNG/clock porting layer) | external backend required (OpenSSL/wolfSSL/mbedTLS/libgcrypt) — none bundled |
| Ships a client program | yes — `cli-main.c` is a complete interactive ssh with pty, password/pubkey auth, known_hosts | example only (`examples/client/client.c`, POSIX sockets + termios, itself a port target) | no — library; you write the client |
| Compile-time feature pruning | excellent (`localoptions.h`; whole files compile to empty) | good (`WOLFSSH_*`/wolfSSL defines) | moderate |
| First-order compile result here | **258/258 objects compile; 0 source edits** | fatal at include #1: `wolfssh/ssh.h:34: fatal error: 'wolfssl/options.h' file not found` | fatal at include #1: `src/crypto.h:48: error: "no cryptography backend selected"` |
| Transport abstraction | owns fd + select loop (needs shim, analyzed in (d)) | pluggable I/O callbacks (`WOLFSSH_USER_IO`) — nice, but doesn't offset the wolfSSL port | send/recv overridable via `libssh2_session_callback_set` |

**Pick: Dropbear dbclient.** It is the only candidate that is self-contained, and it went from
first contact to a 100 % compile sweep in three stub iterations. wolfSSH and libssh2 both fail on
their first include with an external-crypto-library dependency (verbatim errors above), i.e. each
implies porting a second, larger library before the SSH code is even reachable. Dropbear's select
loop is the one structural mismatch, and it is adaptable (see (d)).

Configuration used (full files in `/tmp/ssh-assessment/build/`): client-only,
`curve25519-sha256` + `diffie-hellman-group14-sha256` kex, `ssh-ed25519` + `rsa-sha2-256`
hostkeys, `aes128/256-ctr` + `chacha20-poly1305` ciphers, `hmac-sha2-256`, password + pubkey
auth; X11/TCP-forward/agent/netcat/proxycmd/sntrup761/mlkem768/ECDSA/DSS/zlib all compiled out.
`-DDROPBEAR_CLIENT` (as the upstream Makefile does), `NON_INETD_MODE 1` +
`DROPBEAR_SVR_PUBKEY_AUTH 1` kept only because `sysoptions.h` `#error`s otherwise; no server
object is compiled. Interop note: this algo set matches modern OpenSSH server defaults.

## (b) Compile-verified gaps

First-contact errors (empty config.h, verbatim):

```
src/sysoptions.h:350:3: error: "DROPBEAR_SVR_PASSWORD_AUTH requires `crypt()'."
src/sysoptions.h:374:3: error: "DROPBEAR_SVR_DROP_PRIVS requires setresgid()."
src/includes.h:33:10: fatal error: 'sys/param.h' file not found
```

then, after options/config: `fake-rfc2553.h:57: redefinition of 'sockaddr_storage'` (fixed by
`HAVE_STRUCT_SOCKADDR_STORAGE` etc. in config.h), `loginrec.h:143: unknown type name
'suseconds_t'`, `dbutil.c:571: use of undeclared identifier 'O_NOFOLLOW'`, `netio.c:380:
use of undeclared identifier 'UIO_MAXIOV'`, `cli-auth.c:363: call to undeclared function
'getpass'`, `cli-kex.c:251: call to undeclared function 'getc'`,
`common-chansession.c:31: use of undeclared identifier 'SIGBUS'` — all header-level; **no file
hit a structural incompatibility** (no pointer-size or varargs issues on ilp32e).

### New headers (absent from both include dirs) — 18 stubs

| Header | Needed by | Difficulty |
|---|---|---|
| `sys/socket.h`, `netinet/in.h`, `netinet/ip.h`, `netinet/tcp.h`, `arpa/inet.h`, `netdb.h`, `sys/un.h`, `sys/uio.h` | includes.h → every file; netio.c, fake-rfc2553 | trivial types/protos; **runtime lands on the new net shim** (see (c)/(d)) |
| `sys/select.h` | session_loop, netio | trivial `fd_set` macros; select() impl is the real work |
| `sys/param.h`, `sys/wait.h`, `sys/resource.h`, `pwd.h`, `grp.h`, `syslog.h`, `dirent.h`, `setjmp.h` | includes.h unconditional | trivial; setjmp/longjmp only *referenced* under DROPBEAR_FUZZ, declarations suffice (link-verified: not undefined) |

### Existing headers needing augmentation (shadow + `#include_next`, pvm-posix's own pattern) — 11

| Header | Missing pieces | Difficulty |
|---|---|---|
| `termios.h` | speed_t, IGNPAR/ISTRIP/INLCR/IGNCR/IXON/IXOFF/IXANY, ISIG/ECHOE/ECHOK/ECHONL/IEXTEN, OPOST partner flags, VEOF…VLNEXT, TCSADRAIN, cf* protos | trivial — `cli_tty_setup()` clears exactly `ECHO\|ICANON` (+ICRNL), which is precisely what pvm `tcsetattr` maps to `PVM_TTY_MODE_RAW` / `stdin_translate_cr` |
| `time.h` | `struct timespec`, `clockid_t`, `CLOCK_MONOTONIC`, `clock_gettime` proto | trivial |
| `sys/time.h` | `struct timezone`, `gettimeofday`, pulls sys/select.h | trivial |
| `sys/types.h` | `suseconds_t`, `u_char`/`u_int`… | trivial |
| `stdio.h` | fputs/fputc/getc/ungetc/ferror/setvbuf/fdopen/fileno/BUFSIZ | trivial header; **implementation is the same FILE-layer work item the Vim assessment identified** |
| `stdlib.h` | strtoll/strtoull, putenv/setenv/unsetenv protos | trivial |
| `string.h` | strpbrk/strspn/strcspn/strtok protos | trivial |
| `unistd.h` | ~25 protos (getpid/getuid/fork/pipe/dup2/execv/fcntl/getpass/…) | trivial |
| `fcntl.h` | `O_NONBLOCK`, `O_NOFOLLOW`, F_GETFL/F_SETFL | trivial |
| `signal.h` | sig_atomic_t, sigset_t, struct sigaction + protos, SIGBUS/SIGCHLD/… | trivial (handlers become inert no-ops) |
| `sys/stat.h`, `errno.h` | group/other mode bits, fstat/lstat/chmod/umask; ECONN*/EINPROGRESS/ENOTSOCK/EWOULDBLOCK | trivial |

### Compile + link results

- **Dropbear: 63/63 client sources compile cleanly** (26 COMMONOBJS + 24 CLISVROBJS incl.
  kex-x25519/kex-dh + 13 CLIOBJS), **155/155 libtommath**, **38/38 libtomcrypt** objects
  (aes, ctr, sha1/256/512, hmac, chacha, poly1305, base64, hash_memory, descriptor plumbing,
  ltm_desc). Zero source edits. Iteration profile: first contact fatal → wave 1 (options +
  18 stubs + 11 shadows): 45/59 → +`-DDROPBEAR_CLIENT`, O_NOFOLLOW/uio/gethostbyaddr/getgroups/
  SIGBUS: 56/59 → getpass/getc/ENOTSOCK: 59/59.
- Link (`rust-lld`, all 258 objects + pvm_posix.o + libc_shim.o): **59 undefined symbols,
  0 duplicates** = 5 compiler builtins (`__adddf3 __divdf3 __muldf3 __floatunsidf __udivdi3`,
  provided by Rust `compiler_builtins` as in doom/vi-tty) + 13 expected `pvm_*_wrapper`/
  `host_log_wrapper` Rust-side imports + **41 genuine libc/POSIX gaps**.
- Size: total text across all 258 objects ≈ **128 KiB** (biggest: aes 8K, curve25519 7K,
  common-channel 4K) — comfortably below the Vim port (~925 KiB).

### Link-verified gap table (41 symbols, with referencing objects)

| Category | Symbols (referenced by) | Verdict |
|---|---|---|
| sockets/net (17) | `socket connect bind listen setsockopt getsockopt getpeername getsockname shutdown htons ntohs inet_ntoa getaddrinfo gai_strerror gethostbyaddr writev` (netio, common-channel, packet, fake-rfc2553), `select` (common-session, dbutil) | **needs new guest shim over existing net ABI** (`pvm_net.c`, see (d)). No *host* change needed for connect/read/write/close — `net_tcp_connect/read/write/close` already exist, nonblocking, resolution-in-connect. `bind/listen/gethostbyaddr` = fail-stubs (client never listens with fwd disabled); `writev` = loop over `net_write` (or undefine `HAVE_WRITEV` — packet.c has a plain-write fallback, compile-verified) |
| entropy (0 at link!) | `dbrandom.c` seeds via `open("/dev/urandom") + read` — compiles/links against pvm `open()`, but the fs ABI only mounts `/home`, so it **fails at runtime** and `seedrandom()` calls `dropbear_exit` | **BLOCKER → needs new host capability** `core_random` (see (c)). Exactly **32 bytes once per process** (`INIT_SEED_SIZE`), stirred into a SHA-256 hashpool; every consumer (kex cookie 16 B, curve25519 secret 32 B, per-packet padding, libtommath prime search via `dropbear_rand_source`) draws from `genrandom()`; reseed only after 2^30 draws. Fallback mixing uses getpid/gettimeofday/clock — never as sole source |
| clock (2) | `clock_gettime` (dbutil `gettime_wrapper` → monotonic_now: kex timeouts, keepalive, auth timeout), `gettimeofday` (dbrandom mixing, loginrec) | **have-in-ABI, needs 10-line glue**: `pvm_time_ms` import already exists in the runtime (doom uses `host_time_ms_wrapper`); implement both over it. Monotonic-vs-wall distinction irrelevant (only deltas used) |
| process/identity (12) | `fork` (compat daemon(), dbutil spawn_command — both dead with proxycmd/askpass off), `pipe` (common-session **signal_pipe**, dbutil), `dup dup2 setsid execv` (cli-session stdin/stdout copies, compat), `getpid getuid getgroups getpwnam getpwuid` (dbrandom seed mixing, cli-runopts/cli-kex `~` expansion), `getpass` (cli-auth password prompt) | **trivial stubs**: uid 0/pid 1, `getpwuid` → static passwd with `pw_dir=getenv("HOME")`, `dup(fd)`→fd for 0-2, fork/execv fail-stubs. `pipe` must return two pseudo-fds whose `read` yields EAGAIN (signal-pipe is only ever select()ed and drained; signals never fire). `getpass` = ~20 lines over tty raw mode + read |
| fs metadata (7) | `fstat fileno fsync` (compat/dbutil), `mkdir unlink link` (cli-kex `~/.ssh` creation; gensignkey — dead for client), `chdir` (compat daemon — dead) | **trivial fail-stubs**; consequence: known_hosts saving degrades unless `/home/.ssh` pre-exists (see risk list) |
| rlimit (2) | `getrlimit setrlimit` (dbutil `disallow_core`) | **trivial no-op stubs** |

Runtime-only stdio gaps (link-clean but semantically broken today): dbclient needs
`fopen("a+"/"r") fseek fwrite getc fprintf(stderr)` for known_hosts + the host-key trust prompt,
and libc_shim's `fprintf` routes to `host_log`, not the tty. Same small FILE-layer work item as
the Vim assessment (its plan already covers r/w modes); `fprintf(stderr/stdout)` must be
redirected to `write(2,…)`, and `fopen("/dev/tty","r")` mapped to stdin.

## (c) REQUIRED new host ABI capabilities

1. **Entropy (hard blocker, security-critical):**
   `polkadot_host_0_1_core_random(destination_ptr, capacity) -> bytes_written | error`
   — fills guest memory with host-OS CSPRNG bytes (getrandom(2)/SecRandomCopyBytes/
   crypto.getRandomValues). Cap transfers at 64 KiB like other calls; dbclient needs one
   32-byte read at startup. The ADR already lists `random()` under `host.core`;
   `corevm.rs` confirms no implementation exists today (no match for `random`). Wire into
   the guest as `getrandom()` + a `/dev/urandom` special-case in `open()`, then define
   `HAVE_GETRANDOM`. Seeding from `time_ms` instead is **not** an acceptable fallback for SSH.

2. **Readiness over handle sets (strongly recommended, not slice-blocking):**
   `polkadot_host_0_1_poll(records_ptr, record_count, timeout_ms) -> ready_count | 0 on timeout`
   where each 8-byte record is `{u32 handle_class:2|handle:30, u32 events_in/events_out}`
   (classes: TTY, NET, PIPE; events POLLIN/POLLOUT). Without it the guest select() shim
   busy-probes after every `core_yield` wake (correct — the runtime resumes the guest on any
   pending event, cf. tcp-roundtrip's `WOULD_BLOCK → core_yield` loop on sockets and pvm_posix's
   identical tty loop — but each wake probes all fds, and **pure-timeout wakes don't exist**:
   `core_yield` returns `Interruption::Yield` with no deadline, so keepalives (`-K`), idle
   timeout (`-I`) and time-based rekey checks only run when traffic arrives). A timeout-bearing
   poll (or minimally `core_yield_timeout(ms)`) fixes timer correctness and removes the probe loop.

   Everything else the client needs is already in the ABI: outbound TCP with in-host resolution
   (so no DNS capability needed — `getaddrinfo` shim just carries the `host:port` string through
   to `net_tcp_connect`), tty raw mode, `/home` fs, environment, `time_ms`.

## (d) Event-loop adaptation plan

Dropbear's `session_loop` (common-session.c:168-283) selects on: `signal_pipe[0]` (readfd),
channel fds = stdin 0 / stdout 1 / stderr 2 (via `setchannelfds`), pending-connect fds (writefd,
netio.c `EINPROGRESS` + `getsockopt(SO_ERROR)` pattern), and the session socket (readfd gated on
`writequeue_has_space`, writefd when the writequeue is non-empty). Every read/write path
tolerates EAGAIN (packet.c:90,119,190,240; common-channel.c:428,492) — compile-verified with our
headers — so a guest-side shim works:

1. **fd table in `pvm_net.c`:** virtual fds map to `{TTY0/1/2, FS handle, NET handle, PIPE-stub}`
   so `read/write/close/fcntl/select` can route to the right hostcall family (fs and net handles
   come from different host namespaces and may collide numerically).
2. **connect:** `getaddrinfo(host, port)` returns one fake `addrinfo` carrying `"host:port"`;
   `socket()` allocates a table slot; `connect()` calls `net_tcp_connect` (host resolves +
   connects synchronously) and returns 0 — netio's nonblocking-connect dance degrades gracefully:
   the fd is immediately "writable", `getsockopt(SO_ERROR)` shim returns 0, `connect_try_next`
   error paths map from the connect status code. `fcntl(F_SETFL, O_NONBLOCK)` = no-op (host net
   is already nonblocking; tty read shim already approximates VMIN/VTIME).
3. **select():** generalize the existing `poll()` probe-and-pushback pattern (pvm_posix.c:203-238
   — readiness proven by consuming bytes into a pushback buffer that `read()` drains first):
   - readfd stdin → `tty_read` probe into the existing 64-byte pushback;
   - readfd socket → `net_read` probe into a per-handle pushback (≤64 KiB);
   - readfd signal_pipe → never ready (signals don't exist);
   - writefd socket/stdout → report ready optimistically; actual `net_write`/`tty_write`
     WOULD_BLOCK maps to EAGAIN which every caller tolerates (evidence above);
   - nothing ready → check `time_ms` deadline (select timeout), `core_yield`, re-probe.
     The host resumes the guest on tty *or* net events (both loops are the documented ABI
     pattern), so this is exactly one probe round per event.
4. **Blocking phases** (`read_session_identification`'s per-byte ident read, initial kex) sit on
   the same select shim unchanged.
5. **Channel plumbing:** `dup(0/1/2)` returns the same fd; `cli_tty_setup`'s flag clears map to
   `PVM_TTY_MODE_RAW` via the existing `tcsetattr`; window-size = existing `TIOCGWINSZ`
   (no SIGWINCH → resize picked up only when dropbear re-queries; same limitation Vim accepted).

Known correctness gap (until (c).2 lands): keepalive/idle/rekey timers fire only on traffic.

## (e) Bottom line + recommended vertical slice

**Feasible. One hard host-ABI blocker: entropy.** Everything else is guest-side shim work of the
same species (and smaller text size) than the accepted Vim plan: 41 link-verified libc gaps of
which ~24 are one-line stubs, plus two real work items — `pvm_net.c` (fd table + getaddrinfo +
select ≈ 250–350 lines, the analogue of Vim's FILE layer) and the shared FILE layer itself
(needed for known_hosts; Vim's plan item 3 covers it). No structural/ilp32e incompatibility
surfaced in 258 compiled objects; dropbear's loop is EAGAIN-clean throughout.

**Vertical slice 1 — "connect + banner + kex":** dbclient `-y -y` (skip known_hosts writes) to a
real server through `net_tcp_connect`; success = version exchange, KEXINIT negotiation
(curve25519-sha256 / ed25519 / aes128-ctr / hmac-sha2-256), NEWKEYS, then clean
`dropbear_exit`. Requires: `core_random` hostcall, `pvm_net.c`, clock glue, trivial stubs.
Explicitly excludes FILE layer (known_hosts), auth, and pty session.
**Slice 2:** password auth (`getpass` over raw tty) + interactive shell channel — first real
remote shell in the Host terminal; ADR's "SSH computer" milestone.
**Slice 3:** known_hosts persistence via the FILE layer on `/home`, then pubkey auth
(`~/.ssh/id_dropbear` read path), then keepalives once timed poll exists.

Risks (second-order, not compiler-visible):
- **rv32e register pressure + soft-float bignum performance**: RSA-2048 verify and DH group14 on
  a 16-register ISA under `-Os` may take seconds per connect; curve25519/ed25519 path is the
  cheap one — prefer it in the default algo list (already first in common-algo with this config).
- libtomcrypt compiled without a detected endianness falls back to portable byte access —
  correct, modestly slower; can pin `ENDIAN_LITTLE` later.
- libc_shim bump allocator never frees; dbclient churns per-packet buffers. Budget the arena
  (session traffic is bounded by window sizes; 8–32 MiB fine) — same trade-off as vi-tty/Vim.
- `mkdir` fail-stub means known_hosts persists only if `/home/.ssh` already exists (host fs has
  no mkdir capability; the fs_open CREATE flag's behavior on nested paths is untested).
- stderr today routes to `host_log`; must route to tty or the host-key prompt is invisible.
- `sysoptions.h` forces `DROPBEAR_SVR_PUBKEY_AUTH` on even client-only — harmless (no server
  objects linked) but worth a one-line upstream-tracking note in the port's localoptions.h.

## (f) Estimated scope

| Item | Size | Kind |
|---|---|---|
| Host: `core_random` hostcall (native + wasm-DENIED parity, conformance test) | ~60 lines Rust + test | new capability |
| Host (later): `poll`/timed-yield hostcall | ~150 lines Rust + test | new capability |
| Guest: `pvm_net.c` (fd table, getaddrinfo/connect/read/write/close/select/getsockopt/shutdown/writev) | 250–350 lines C | new shim, reusable by any net port |
| Guest: clock glue (clock_gettime/gettimeofday over `pvm_time_ms`) + getrandom | ~30 lines | glue |
| Guest: ~24 trivial stubs (pwd/uid/rlimit/fork/pipe/dup/fs-metadata/getpass) | ~150 lines | glue (vi-tty style) |
| Guest: FILE layer (shared with Vim port plan item 3) | 200–300 lines | pvm-posix, shared |
| Port scaffold: `apps/dbclient/` (build.rs two-pass cc, localoptions/config/29 stub headers from `/tmp/ssh-assessment/stubs`, Rust wrapper) | config-only | mirror vi-tty/doom |
| Compiled footprint | ~128 KiB text + arenas | measured |

Slice 1 is roughly **2–4 focused days** once `core_random` exists; the hostcall itself is the
smallest piece. Total to an interactive remote shell (slice 2): about a week, dominated by
select-shim edge cases, not by Dropbear.
