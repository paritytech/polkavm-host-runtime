# Contributing

## Pull requests

Runtime changes must preserve the reviewed host boundary:

- Keep native and browser bounds and failure behavior aligned.
- Treat `pvm-gpu-wire` changes as compatibility changes.
- Generate browser artifacts from source; do not edit generated assets.
- Add behavioral coverage for observable runtime changes.
- Pin every git dependency to an immutable full commit.

Run the affected checks locally. Shared Rust, GPU wire, lockfile, artifact, and release changes require the complete matrix.

Native GPU descriptor and quota checks run in the regular test suite. On a
machine with a WebGPU adapter, also run the opt-in backend regressions:

```bash
cargo test --locked -p pvm-runtime --features native-gpu native_gpu::tests::gpu_ -- --ignored --test-threads=1
```

These exercise backend rejection, recovery, retained-resource quotas, and
texture readback. The default suite skips them; passing it alone does not verify
driver-backed GPU behavior or establish isolation from driver faults.

## Commit messages

Use concise conventional subjects such as `fix(runtime): reject stale GPU sequences` or `build(browser): reproduce release assets`.

## Releases

Release manifests and artifacts must be produced from a clean tagged source commit. Artifact digests are part of the release contract.

`npm run manifest` refuses dirty or untagged source. The `v<version>` tag must
identify HEAD, and the workspace, Rust crates, browser package, and npm lockfile
must agree on the version. `SOURCE` must name that tag and the PolkaVM revisions
actually pinned by Cargo. Every generated browser artifact must match both its
checksum and its checked-in embedded copy, with no extra or missing files.

Generate and review the browser assets and update their embedded Rust digests
before preparing the tagged source commit. The manifest command verifies
artifacts; it does not rebuild or repair them. Its output belongs in the ignored
`artifacts/` directory, not among the tagged source inputs.

Release-integrity regressions run in isolated temporary Git repositories via
`node --test scripts/release-manifest.test.mjs` and are included in `npm test`.
