---
title: "ADR 0003: Applications register file handlers at runtime"
type: decision-record
status: accepted
---

# ADR 0003: Applications register file handlers at runtime

- Date: 2026-09-29

## Context

Applications need user files in two shapes:

- a file that becomes the starting state of a fresh execution, such as a
  cartridge image for the `nes`, `gameboy`, and `supafaust` guests in
  `paritytech/polkavm-app-kit`; and
- a file opened by an application that is already running, such as a document
  an editor imports.

Those guests declare an App v2 `capabilities.fileInput` block listing handler
IDs, labels, extensions, a size bound, and a mount path. Nothing in this
repository defines that block, and no released `bulletin-deploy` accepts it, so
the kit publishes through a local patch of the CLI. A manifest declaration can
also describe only the first shape: a running application cannot use it to ask
for a file.

The ABI already has the pieces a runtime design needs. Mediated input gives the
guest a registration, a Host-owned consent and capture step, and a bounded read.
Assets give a fresh execution an immutable file at an archive-relative path.
Motion and the base input records establish that permissioned Host services are
discovered through runtime calls rather than negotiated in the manifest.

`paritytech/polkavm-app-kit` ADR 0001 keeps a manifest capability for a Host
service that is separately authorized. File input is authorized per use, by the
user's own selection in Host UI, so a launch-time declaration adds no
authorization and does not qualify.

## Decision

**File input is a runtime service of ABI v1, built on mediated input.** The
guest registers each handler with `host_file_register`, and the returned handle
uses the existing `host_input_trigger`, `host_input_status`, `host_input_read`,
and `host_input_cancel` lifecycle. The contract lives in
`docs/runtime/polkavm-app-abi-v1.md` under §File input and §File-type hint.

- **A handler selects one delivery.** `inline` delivers the selected bytes to
  the running execution through `host_input_read`, up to 8 MiB. `relaunch`
  stops the execution and starts a fresh one in which the file replaces the
  asset at the handler's mount path, up to 128 MiB. `stream` retains a selected
  disk-backed source up to 4 GiB minus one byte and exposes synchronous random
  reads of at most 64 KiB through `host_file_read`.
- **The guest learns what it received.** `host_file_info` returns the file's
  base name, resolved MIME type, and size for all deliveries, so an editor can
  title and save a document and an emulator can key save data by cartridge.
- **The Host owns selection and consent.** A file reaches the guest only
  through Host UI: a picker the guest triggered, a Host menu entry, a drop, a
  share sheet, or an open-with chooser. The user's selection is the consent.
  When several handlers match, the user chooses. A relaunch that would stop an
  execution in use needs confirmation.
- **The Host keeps the most recent registrations.** They survive until a later
  execution registers a handler, so the Host can offer another file even when
  the relaunched execution fails during `init`.
- **Opening a file before the application runs is defined.** The Host launches
  the application holding the file and delivers it to the first matching
  registration, relaunching once for a relaunch handler.
- **The manifest carries only an advisory hint.** A top-level `fileTypes` list,
  with an optional `handler` naming the runtime `id`, lets a Host suggest an
  application for a file. It is not a capability and grants nothing.
- **ABI v1 absorbs the change.** Per ADR 0001, v1 has no released consumers.
  Hosts implement the new calls and applications republish without
  `capabilities.fileInput`.

## Consequences

- Cartridge guests register one `relaunch` handler during `init`, read their
  cartridge with `host_asset_read`, drop `capabilities.fileInput`, and may list
  `fileTypes`. A packaged default cartridge at the mount path keeps working
  until the user selects a file.
- Publishing tools validate `fileTypes` instead of `capabilities.fileInput`.
  Handler bounds are checked by the Host at registration rather than at
  publication.
- A Host that cannot relaunch, such as a headless one, returns `-4` for
  `relaunch` registrations and can still serve `inline` ones.
- Inline reads count against the per-update Host-call byte budget, which the
  8 MiB bound keeps within reach of a single update.
- Stream delivery keeps large map and disc files outside guest memory. Native
  Hosts read the selected file directly; browser workers use bounded
  `FileReaderSync` slices. A Host without that facility returns `-4` for stream
  registrations rather than buffering the whole file.
- A selected stream can have a Host-private working cache for derived bytes,
  such as locally decompressed maps. Sequential writes, explicit sealing,
  bounded reads, and a 512 MiB aggregate reservation keep it separate from
  arbitrary filesystem access. The original source and metadata stay unchanged.
  Native Hosts supply `FileCache`; browser Hosts opt into OPFS or a trusted
  cache factory. Cancel, picker reopen, and normal stop release the scratch
  cache. Browser termination waits for deletion and reports cleanup failures;
  hard worker/process termination may leave origin-private scratch. This is
  session-local preparation, not cross-launch persistence or redistribution.
- One file per selection. Multi-file inputs and files above 4 GiB minus one
  byte need a later decision.
- Picker and chooser UI, confirmation wording, remembering and clearing the
  last file, error wording, and store presentation of `fileTypes` are Host
  policy.

## References

- `docs/runtime/polkavm-app-abi-v1.md` — §File-type hint, §Mediated input,
  §File input, §Assets, and §Failure and shutdown.
- ADR 0001 — ABI v1 absorbs pre-release breaking changes.
- `paritytech/polkavm-app-kit` ADR 0001 — application input is baseline ABI and
  is not negotiated in the manifest.
