# Contributing

## Pull requests

Runtime changes must preserve the reviewed host boundary:

- Keep native and browser bounds and failure behavior aligned.
- Treat `polkavm-gpu-wire` changes as compatibility changes.
- Generate browser artifacts from source; do not edit generated assets.
- Add behavioral coverage for observable runtime changes.
- Pin every git dependency to an immutable full commit.

Run the affected checks locally. Shared Rust, GPU wire, lockfile, artifact, and release changes require the complete matrix.

Native GPU descriptor and quota checks run in the regular suite. On a machine
with a WebGPU adapter, also run the opt-in backend regressions:

```bash
cargo test --locked -p polkavm-host-runtime --features native-gpu native_gpu::tests::gpu_ -- --ignored --test-threads=1
```

These exercise backend rejection/recovery, retained-resource quotas, texture
readback, and offscreen attachments. Passing the default suite alone does not
verify driver-backed behavior or establish isolation from driver faults.

When locked dependencies or toolchains change, regenerate attribution with
`node scripts/generate-third-party-notices.mjs`; `npm run verify:notices`
checks deterministic output. The pinned Rust toolchain includes the source and
documentation components needed to preserve standard-library license texts.

## Commit messages

Use concise conventional subjects such as `fix(runtime): reject stale GPU sequences` or `build(browser): reproduce release assets`.

## Releases

Release manifests and artifacts must be produced from a clean tagged source commit. Artifact digests are part of the release contract.

`npm run manifest` refuses dirty or untagged source. The `v<version>` tag must
identify HEAD, and workspace, crate, browser package, and npm lockfile versions
must agree. `SOURCE` must name that tag and the exact PolkaVM revisions pinned
by both the runtime and compiler. Every generated artifact must match its
checksum and embedded copy, with no extra files or symlinks.

Generate and review assets and embedded Rust digests before preparing the tagged
source commit. The manifest command only verifies them; it does not repair
release inputs. Output belongs in ignored `artifacts/`, not the tagged source.
The current `0.3.2-background.0` browser development version and `0.3.1` Rust
versions are intentionally not a releasable version set.

Release-integrity regressions use isolated temporary Git repositories and run
through `node --test scripts/release-manifest.test.mjs`, included in `npm test`.

## Engine maintenance

`.github/workflows/engine-qualification.yml` pins a separate candidate; changing
that pin does not update the runtime. It checks native Linux and ARM64 macOS,
requires the compiler-specific tests to exist, forces the native JIT for a real
guest, and also executes an ad-hoc signed hardened macOS host with `allow-jit`.
The Linux runner enables userfaultfd and unprivileged namespaces only inside
its ephemeral VM; do not weaken a developer workstation's kernel policy merely
to obtain a green result.

Engine regression fixtures must use the existing `test_module_config()` helper
when they require native execution: the default 4 KiB module page size is
incompatible with Apple Silicon's 16 KiB host pages. Preserve the semantic
assertions rather than skipping those backends. Native JIT examples must also
configure a 16 KiB module page size on macOS, as both runtime execution paths do.

Keep guest bytecode encoding compatible during engine refreshes. Upstream
`e68f2a60f4e6cbc9ea7b10e5cccbafc531c2f8f0` removed the non-legacy code-length
field without changing the ISA version: existing `Latest64` blobs can parse
successfully as different instructions. The maintenance branch retains that
field and covers an existing blob with a decoder regression. A parse-success
check or unchanged instruction count alone is not a compatibility test.

Only after Linux, actual Apple Silicon, and native/browser guest parity pass,
update runtime and standalone compiler Cargo pins together with `SOURCE` and
the compatibility documentation. Regenerate the lockfile, attribution, browser
distribution, embedded assets, and Rust asset digests. Verify both byte and
precompiled-Module browser startup, compiler/interpreter execution, and package
consumption before publishing the runtime branch. Never promote a candidate
solely because it cross-compiles for macOS or because its crate version matches.
