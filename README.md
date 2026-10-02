> [!WARNING]
> This open source code is provided for research, experimentation, and developer education only. This code has not been audited, is actively experimental, and may contain bugs, vulnerabilities, or incomplete features. Use at your own risk and obtain legal advice as appropriate - DYOR.

Parity doesn’t deploy the code but may update it based on community feedback.

If you experience problems with any product or service that was built on or deployed from this code, you should contact the third party who deployed the code in its amended form, not Parity.

# PVM Host Runtime

Experimental, host-neutral PolkaVM application runtime code developed and published by Parity for native and browser Hosts.

The repository owns one implementation of the App Manifest v2 PolkaVM execution contract across native Rust, browser WebAssembly, framebuffer, Tri2D, WebGPU Raster, and expanded WebGPU presentation. Runtime limits, GPU records, browser workers, and distributable assets are built and reviewed together.

## Layout

- `rust/crates/pvm-runtime`: execution, hostcalls, lifecycle, bounds, and native/wasm backends.
- `rust/crates/pvm-gpu-wire`: bounded Tri2D and WebGPU wire protocol.
- `rust/crates/pvm-motion-wire`: bounded motion-sample wire protocol.
- `rust/crates/pvm-ui-wire`: bounded cursor, clipboard, navigation, and IME output protocol.
- `rust/crates/pvm-runtime-assets`: source-identified browser assets exposed as static Rust data.
- `rust/crates/pvm-assets-export`: exports those assets for Android, iOS, and browser packaging.
- `js/packages/pvm-browser-runtime`: source-built `@parity/pvm-browser-runtime` package.
- `js/packages/pvm-browser-runtime/prototype/file-input.html`: drag, picker, routing, and consent prototype.
- `docs/runtime/polkavm-app-abi-v1.md`: application ABI contract.
- `docs/runtime/tri2d-v1.md`: Tri2D frame, command, retained-resource, and limit contract.

## Host boundary

Hosts integrate through the `truapi-pvm-host` bridge in [`paritytech/host-rust-core`](https://github.com/paritytech/host-rust-core). The bridge pins one immutable release of this repository and exposes the supported Rust API plus browser asset identity. Host applications do not pin this repository independently.

The browser endpoint accepts runtime WASM bytes or a precompiled
`WebAssembly.Module` in its `start` message. `stop` is terminal, including while
runtime instantiation or guest compilation is pending; late completions do not
restart the guest or emit further startup output.

`ComputerSupervisor::run()` shares a maximum of 8,192 guest resumptions across
foreground and piped background work. It returns `ComputerStatus::Yielded` when
that host-turn budget is exhausted, preserving completed requests and guest
continuations for the next call. Hosts must schedule yielded work rather than
assume each call runs until a guest explicitly yields.

`NativeGpuRenderer` rejects invalid guest descriptors and backend validation
errors through `BatchRejected` events. Resource counts, aggregate buffer/texture
bytes, and per-batch work are bounded. Resources retained by dependent objects
remain charged after their guest handles are destroyed. A rejected batch is not
transactional: successful earlier commands remain applied and charged.

## Build and test

```bash
cargo +nightly fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
npm ci
npm test
```

Build and export browser assets:

```bash
npm run build
cargo run -p pvm-assets-export -- --output ./out/pvm-runtime
```

## File-input prototype

Serve the repository root, then open
`/js/packages/pvm-browser-runtime/prototype/file-input.html`. The prototype
registers Supafaust's SNES cartridge handler and demonstrates drag-and-drop and
the Host's **Add file** consent flow. Delivery is simulated: the page reads an
approved file and displays the proposed mount path, but does not launch an
emulator or validate a cartridge.

## Releases

A release is identified by one source commit and records:

- Rust workspace version.
- `@parity/pvm-browser-runtime` version.
- native and wasm PolkaVM revisions.
- SHA-256 digest of every browser artifact.

Release tags use `v<version>`. Moving branch references are not release inputs.

## Security

This is experimental library code, not a production-hardened runtime. It has not
received an independent security audit. This repository does not establish the
security of a downstream host or its configuration. Downstream users are
responsible for independently reviewing the code, dependencies, generated
artifacts, and their own integration before use.

For vulnerabilities in this code, follow the
[Parity security policy](https://github.com/paritytech/.github/blob/main/SECURITY.md).
Do not disclose unpatched vulnerabilities in public issues. Problems specific to
a third-party product or service should be reported to its operator.

## License

MPL-2.0. See `LICENSE`.
