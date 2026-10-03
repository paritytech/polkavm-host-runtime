# PolkaVM Host Runtime 0.3.2-rc.1

Release candidate for downstream integration testing, not a stable-release
recommendation. Changes below are relative to `v0.3.1`.

## Changes

- Extract `polkavm-wasm-compiler` with explicit translation limits. Native
  production runtime builds do not depend on the Wasm translator.
- Add typed browser `startSession` lifecycle and file-input routing APIs, plus
  runnable native and browser framebuffer examples. Direct runtime embedding
  does not require TrUAPI; the bridge remains optional.
- Add the bounded mediated-input runtime API and remove obsolete manifest input
  negotiation. Device availability is negotiated at runtime.
- Validate `capabilities.fileInput` handlers consistently across native and
  browser hosts, including the 128 MiB per-file ceiling and consent-before-read
  delivery contract.
- Harden native GPU descriptor validation, backend error handling, retained
  resource accounting, quotas, and texture-upload padding. These boundaries do
  not isolate hosts from graphics-driver faults.
- Bound native and browser computer supervision to one 8,192-resumption budget
  across foreground, background, and workspace children per host turn. Preserve
  queued input, ordered output, and package resolution across budget yields.
- Preserve CoreVM vectored-I/O partial progress when later descriptors or
  buffers fault, and support browser startup from a precompiled runtime module.
- Ship MPL and third-party notices with both browser distribution formats, and
  enforce clean tagged source, aligned versions, immutable engine pins, and
  reproduced embedded assets in the release-manifest gate.

## Compatibility

- All seven Rust crates and the browser package use `0.3.2-rc.1`.
- Application manifest v2, guest application ABI 1, and browser runtime ABI 2
  remain distinct contracts. Ship the worker, JavaScript, and Wasm from the same
  artifact set; do not mix them with files from `v0.3.1`.
- `capabilities.deviceInput` and removed manifest input-negotiation declarations
  are not accepted. Hosts must use the runtime availability/input APIs.
- Native engine and standalone compiler pin PolkaVM revision
  `642fa95a6f1df85612bdbd0a7e4353a2aa4dc9b5`. This maintenance revision retains the
  existing `Latest64` blob encoding; the upstream engine version `0.37.0` alone
  does not establish guest compatibility.
- The pinned engine has passed Linux and actual Apple Silicon qualification,
  including hardened-runtime JIT execution. Conformance guests use
  `polkatool 0.31.0` and `nightly-2025-10-09`; other guest toolchains and production
  host integrations require their own compatibility validation.

## Candidate artifacts

The release workflow prepares:

- `parity-polkavm-browser-runtime-0.3.2-rc.1.tgz`: installable browser package,
  including typed APIs, Wasm, worker code, `SOURCE`, checksums, and notices.
- `polkavm-host-runtime-assets-0.3.2-rc.1.tar.gz`: the identical flat browser asset
  set for native/mobile host packaging. Despite its name, this is not a native
  engine binary, Android AAR, or application installer.
- `runtime-release.json`: exact tagged source revision, crate/package versions,
  engine revisions, and hashes of the individual browser artifacts.

Publication is through GitHub release assets, not this repository's npm registry
or crates.io. Rust source consumers pin the full source revision recorded by the
release manifest. An approved RC publication must be marked prerelease and must
not replace the stable release as GitHub's latest release.

## Redistribution and remaining qualification

The runtime remains MPL-2.0. Preserve the bundled license texts, notices, and
access to corresponding covered source when redistributing it.

The conservative native dependency inventory still records unresolved upstream
notice evidence for `block 0.1.6` and a later license clarification for
`malloc_buf 0.0.6`. Neither is on the browser-default dependency path. These
browser/source artifacts do not resolve the obligations of a separately built
native host; review the documented limitations before distributing native
binaries that include those dependencies. See
[THIRD_PARTY_NOTICES.md](https://github.com/paritytech/polkavm-host-runtime/blob/v0.3.2-rc.1/THIRD_PARTY_NOTICES.md#upstream-evidence-limitations).

The candidate does not update or deploy a downstream host. Qualify the packaged
runtime in an actual consumer before stable promotion; source and example smoke
checks are not a substitute for that integration gate.
