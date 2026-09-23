# PolkaVM Host Runtime

Host-neutral PolkaVM application runtime for native and browser Hosts.

The repository owns one implementation of the App Manifest v2 PolkaVM execution contract across native Rust, browser WebAssembly, framebuffer, Tri2D, WebGPU Raster, and expanded WebGPU presentation. Runtime limits, GPU records, browser workers, and distributable assets are built and reviewed together.

## Layout

- `rust/crates/polkavm-host-runtime`: execution, hostcalls, lifecycle, bounds, and native/wasm backends.
- `rust/crates/polkavm-gpu-wire`: bounded Tri2D and WebGPU wire protocol.
- `rust/crates/polkavm-motion-wire`: bounded motion-sample wire protocol.
- `rust/crates/polkavm-ui-wire`: bounded cursor, clipboard, navigation, and IME output protocol.
- `rust/crates/polkavm-host-runtime-assets`: source-identified browser assets exposed as static Rust data.
- `rust/crates/polkavm-assets-export`: exports those assets for Android, iOS, and browser packaging.
- `js/packages/polkavm-browser-runtime`: source-built `@parity/polkavm-browser-runtime` package.
- `docs/runtime/polkavm-app-abi-v1.md`: application ABI contract.
- `docs/runtime/tri2d-v1.md`: Tri2D frame, command, retained-resource, and limit contract.

## Host boundary

Hosts integrate through the `truapi-polkavm-host` bridge in [`paritytech/host-rust-core`](https://github.com/paritytech/host-rust-core). The bridge pins one immutable release of this repository and exposes the supported Rust API plus browser asset identity. Host applications do not pin this repository independently.

Native UniFFI hosts mediate opaque TrUAPI frames through
`take_host_frame_request()` and `send_host_frame_response()`; the runtime keeps
the guest-facing `host_frame_send` / `host_frame_poll` queues bounded.
Native callers terminate execution explicitly with `NativePolkaVmRuntime.stop()`.
Guest execution and host-transport failures also stop the runtime; subsequent
input, output, GPU, audio, and host-frame operations return
`NativePolkaVmError::Stopped`, while `is_exited()` reports `true`. Consumers
must regenerate their UniFFI bindings when updating to this API surface.

Large browser guests use bounded groups of Wasm functions instead of one
function per basic block, avoiding browser function-count limits while
preserving gas accounting and hostcall resumption. Compilation first uses one
module to keep calls local. If the browser exhausts native compilation capacity,
the runtime retries with bounded code modules sharing guest memory, registers,
and dispatch state before falling back to the interpreter. Cached compiled
programs include the root and every code module; instantiation creates fresh
guest state.

Application hosts may pause through `ApplicationRuntime::set_paused(bool)`.
Updates do not execute while paused, execution-scoped monotonic clocks freeze,
and resume excludes paused wall time. Wall-clock imports remain real time.
Release held controls before pausing and suspend/clear the host audio device;
the runtime discards pending gameplay actions and audio while retaining input
releases and viewport state for the next update.

The browser endpoint accepts `{ type: "pause", paused: boolean }` and acknowledges
every valid request with `{ type: "pause-state", paused: boolean }`. This is a hard
pause: no updates or external-event wakes execute. Pause is retained before and
during asynchronous startup: initialization completes, but updates wait for resume.

For menu/visibility inactivity without interrupting host-response delivery, use
`{ type: "background", backgrounded: boolean, seq?: number }`, acknowledged with
`{ type: "background-state", backgrounded: boolean, seq?: number }`. An optional
nonnegative safe-integer sequence is echoed before any resumed framebuffer.
Hosts combine menu and visibility reasons before sending the effective state.
Background mode freezes elapsed update time and discards gameplay input, motion,
and audio, but services host responses with coalesced bounded update work for
both legacy and demand-driven guests. Queue-full retries also wake servicing;
there are no periodic background frames. Hard pause takes precedence, and
overlapping inactive reasons exclude their combined duration exactly once.
This is **not simulation suspension**: service updates execute guest code;
wall-clock reads, host requests, saves, and other external side effects remain
possible. Subscriptions are not interrupted or their responses coalesced.

The runtime retains only the latest complete framebuffer while inactive and
delivers it on resume, even if the guest is idle. Tri2D streams are not standalone
snapshots: they include retained texture mutations. Hosts must apply every stream
in order offscreen, retaining only the latest completed presentation for resume.
GPU batches and protocol events likewise remain ordered and lossless; Hosts hide
surface presentation rather than discard commands. Hosts also suppress inactive
clipboard/navigation actions while retaining current cursor/IME state.
Resume does not replay missed ticks or buffered audio, and stopping clears held
presentation and cannot be reversed by queued work or asynchronous compilation.
The worker and Wasm runtime must be rebuilt together: the interpreter uses
`polkavm_browser_pause_input` to enforce the same input boundary as translation.

Browser and native render passes accept registered texture views as offscreen
color attachments; zero still selects the surface. Offscreen passes preserve
the surface and retain generation and resource-handle validation. These changes
remain within application runtime ABI 1, as required by ADR 0001.


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
cargo run -p polkavm-assets-export -- --output ./out/polkavm-host-runtime
```

## Releases

A release is identified by one source commit and records:

- Rust workspace version.
- `@parity/polkavm-browser-runtime` version.
- native and wasm PolkaVM revisions.
- SHA-256 digest of every browser artifact.

Release tags use `v<version>`. Moving branch references are not release inputs.

## License

MPL-2.0. See `LICENSE`.
