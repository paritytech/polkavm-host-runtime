> [!WARNING]
> This open source code is provided for research, experimentation, and developer education only. This code has not been audited, is actively experimental, and may contain bugs, vulnerabilities, or incomplete features. Use at your own risk and obtain legal advice as appropriate - DYOR.

Parity doesn’t deploy the code but may update it based on community feedback.

If you experience problems with any product or service that was built on or deployed from this code, you should contact the third party who deployed the code in its amended form, not Parity.

# PolkaVM Host Runtime

Experimental, host-neutral PolkaVM application runtime code developed and published by Parity for native and browser Hosts.

The repository owns one implementation of the App Manifest v2 PolkaVM execution contract across native Rust, browser WebAssembly, framebuffer, Tri2D, WebGPU Raster, and expanded WebGPU presentation. Runtime limits, GPU records, browser workers, and distributable assets are built and reviewed together.

## Layout

- `rust/crates/polkavm-host-runtime`: execution, hostcalls, lifecycle, bounds, and native/wasm backends.
- `rust/crates/polkavm-wasm-compiler`: host-independent bytecode-to-Wasm translation with explicit caller-supplied limits.
- `rust/crates/polkavm-gpu-wire`: bounded Tri2D and WebGPU wire protocol.
- `rust/crates/polkavm-motion-wire`: bounded motion-sample wire protocol.
- `rust/crates/polkavm-ui-wire`: bounded cursor, clipboard, navigation, and IME output protocol.
- `rust/crates/polkavm-host-runtime-assets`: source-identified browser assets exposed as static Rust data.
- `rust/crates/polkavm-assets-export`: exports those assets for Android, iOS, and browser packaging.
- `js/packages/polkavm-browser-runtime`: source-built `@parity/polkavm-browser-runtime` package.
- `docs/runtime/polkavm-app-abi-v1.md`: application ABI contract.
- `docs/runtime/tri2d-v1.md`: Tri2D frame, command, retained-resource, and limit contract.

## Host boundary

Native Hosts can consume `polkavm-host-runtime` directly. TrUAPI-based native
Hosts may instead use the optional `truapi-polkavm-host` composition crate in
[`paritytech/host-rust-core`](https://github.com/paritytech/host-rust-core), which
owns its runtime pin and TrUAPI routing. Do not add a second independent runtime
version underneath that bridge. The base TrUAPI implementation is PolkaVM-free.

Browser Hosts consume `@parity/polkavm-browser-runtime` directly; browser workers
and assets come from this repository, not host-rust-core. In either integration,
the Host owns product verification, identity, permissions, transport, persistent
storage, presentation, and device/UI resource cleanup. The runtime does not
grant permissions or interpret opaque TrUAPI requests on the Host's behalf.

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
GPU batches and protocol events likewise remain ordered and lossless; Hosts
suppress new surface presentation rather than discard commands. Already
submitted GPU work may complete at the transition. Hosts also suppress inactive
clipboard/navigation actions, defer pointer-capture acquisition, and cancel
new mediated-input prompts while retaining current cursor/IME state.
Resume does not replay missed ticks or buffered audio, and stopping clears held
presentation and cannot be reversed by queued work or asynchronous compilation.
The worker and Wasm runtime must be rebuilt together:
`polkavm_browser_pause_input` enforces the same input boundary as translation,
and `polkavm_browser_pending_host_frame_responses` reports actual queued work
when entering background mode instead of assuming foreground updates polled it.

Browser and native render passes accept registered texture views as offscreen
color attachments; zero still selects the surface. Offscreen passes preserve
the surface and retain generation and resource-handle validation. These changes
remain within application runtime ABI 1, as required by ADR 0001.

Native GPU descriptors are checked before backend creation, and validation,
out-of-memory, and internal backend errors are captured at the runtime boundary.
Resource quotas include objects retained transitively by views, bindings, and
pipelines, plus resources referenced by an in-flight batch. A rejected batch is
not transactional: earlier successful commands and their charges remain.
These checks do not isolate the Host from native graphics-driver faults.

The computer supervisor shares one bounded resumption budget across foreground,
background, and nested workspace processes per Host turn. Budget exhaustion
yields to the Host without discarding output or child state.

## Direct native embedding

The [native framebuffer example](rust/crates/polkavm-host-runtime/examples/native_framebuffer.rs)
uses only public APIs, selects the interpreter explicitly, disables audio and
external services, captures a real frame, and stops/releases the VM before
writing its output:

```bash
cargo run -p polkavm-host-runtime --example native_framebuffer -- \
  rust/crates/polkavm-host-runtime/tests/fixtures/framebuffer-test.polkavm \
  /tmp/framebuffer.ppm
```

It accepts an asset-free guest path and output path, not a product identifier.
A production launcher must first verify the manifest and archive, choose the
presentation profile, supply bounded assets, and mediate requested services.
`native-gpu` enables the native renderer; `ffi` enables UniFFI bindings. Neither
feature is required for this headless example.

## Typed browser sessions

Import `startSession` and its types from `@parity/polkavm-browser-runtime`.
Provide runtime bytes or a compiled `WebAssembly.Module`, guest bytes, an asset
array, the selected `graphicsProfile`, and an ordered `onOutput` callback.
The session owns one Worker; the Host owns all presentation and permissions.

- Await `session.ready` before `session.send(input)`. Startup failure rejects it.
- `RuntimeInput` and `RuntimeOutput` cover input, pause/background, graphics,
  audio, host frames, mediated input, and compiler-cache messages.
- Frame pixels are RGBA bytes; audio samples are signed-16-bit bytes. GPU and
  Tri2D outputs require their corresponding Host renderers.
- Binary inputs are cloned rather than transferred; callers retain ownership.
- `await session.stop()` is idempotent and terminal. It requests cancellation,
  then forcibly releases an unresponsive Worker after one second of Host scheduling.
- `session.terminal` always resolves after Worker/listener/timer cleanup. Release
  outstanding Host prompts, audio, graphics, and other resources on every terminal
  outcome, including startup/callback failure. Stopping before ready rejects
  `ready` with `AbortError`; queued startup work cannot resurrect the session.

Run the [browser framebuffer example](js/packages/polkavm-browser-runtime/examples/framebuffer.html)
after `npm run build`, serving the repository root over HTTP:

```bash
python3 -m http.server 8765 --bind 127.0.0.1
```

Open `http://127.0.0.1:8765/js/packages/polkavm-browser-runtime/examples/framebuffer.html`.
It loads the real fixture, renders its pixels, and exposes pause, background,
and stop controls. Deployments must serve the Worker and Wasm URLs allowed by
their CSP; do not mix files from different runtime revisions.

The `./file-input-router` subpath routes selected-file metadata to validated
`capabilities.fileInput` handlers. Bytes are read only after Host consent and
delivered as bounded launch assets. The
[file-input prototype](js/packages/polkavm-browser-runtime/prototype/file-input.html)
demonstrates routing and consent; it simulates delivery, not an emulator.

## Standalone compiler boundary

`polkavm-wasm-compiler` exposes `translate(program, limits)` and
`translate_partitioned(program, limits)`. Its `Limits` require maximum program
bytes, read-write data, stack, and heap sizes. It contains no Host policy,
application manifest parsing, Worker lifecycle, graphics renderer, or native
engine dependency in production. The native runtime does not pull in the
translator: it is a `wasm32` production dependency and a native test dependency.

Both outputs retain the existing guest register/memory/gas/resumption contract.
Partitioned output embeds `epoca.pvm.code-part` modules: instantiate the root,
then every part importing the root under `pvm`, before execution. The browser
runtime handles this ordering and caches the complete set. Browser Wasm tail
calls are required by the compiled backend; unsupported compilation falls back
to the interpreter. Compiler tests use the pinned native engine only as an oracle.

## Compatibility and installation

Version numbers describe different boundaries; they are not interchangeable:

| Boundary | Current contract |
| --- | --- |
| Application manifest | `$v: 2`, `kind: "app"` |
| Guest application imports | `runtime.abiVersion: 1` |
| Presentation records | Graphics ABI 1, with the selected profile and required limits |
| Browser bootstrap exports | Browser runtime ABI 2; ship Wasm and JS from the same build |
| Engine and blob encoding | Exact revision `c160c13c3c29bf3219ce1404ec95976000235a94` |
| Conformance guest toolchain | `polkatool 0.31.0`, `nightly-2025-10-09` |

For [PolkaVM App Kit](https://github.com/paritytech/polkavm-app-kit), match the
guest's linker/blob format, imported host functions, graphics profile, and limits
to this runtime. The conformance fixtures exercise the toolchain above; this is
not a promise that every App Kit application or moving branch is compatible.
Older `capabilities.deviceInput` declarations are rejected; current device
availability is negotiated by the runtime, not restored through that obsolete
manifest field. Rebuild guests, compiler caches, and Host assets as one reviewed
compatibility change when the engine's blob encoding changes. PolkaVM `0.37.0`
alone is insufficient to identify that encoding.

Rust crates are consumed from a reviewed full Git revision or local workspace
paths; this repository does not publish them to crates.io. Direct consumers pin
the same revision for runtime, wire crates, compiler, and asset crate. Bridge
consumers use their bridge's pin instead.

Browser releases are immutable GitHub release tarballs, not npm publications
from this repository ([ADR 0002](docs/adr/0002-runtime-distribution-channel.md)).
Download the reviewed release's `parity-polkavm-browser-runtime-<version>.tgz`,
check its runtime release manifest/artifact hashes, and install that local tarball
with npm. Registry distribution is owned separately by `paritytech/useragent-kit`;
its versioning must not be assumed to match this repository.

For local development, `npm ci && npm run build && npm run pack:browser`
creates the tarball in the repository root. Install that file in the consuming
project, or serve the complete `dist/` directory. Native/mobile packaging can use
`polkavm-assets-export`; it exports the same checked-in files, checksums, and
license notices. A runtime source update requires regenerating those embedded
files before the exporter represents the new build.


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

Development versions are currently Rust `0.3.1` and browser
`0.3.2-background.0`; this checkout is not a tagged release. The release generator
deliberately refuses inconsistent versions, development-only `SOURCE` records,
dirty source, stale embedded files, or mismatched compiler/engine provenance.
See [CONTRIBUTING.md](CONTRIBUTING.md) for preparation and integrity checks.

## Security

See [SECURITY.md](SECURITY.md) and the
[Parity security policy](https://github.com/paritytech/.github/blob/main/SECURITY.md).
Report unpatched vulnerabilities privately.

## License

MPL-2.0. See [LICENSE](LICENSE). Third-party source exceptions retain their
original licenses; see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) and
[the generated license bundle](licenses/THIRD_PARTY_LICENSES.txt) for exact
notices, provenance, evidence limitations, and redistribution obligations.
