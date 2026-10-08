---
title: "PolkaVM application runtime ABI v1"
type: runtime-contract
status: draft
---

# PolkaVM application runtime ABI v1

## Scope

This document defines the application-visible boundary selected by an App v2
manifest with:

```json
{
  "runtime": {
    "kind": "polkavm",
    "abiVersion": 1
  }
}
```

It covers cooperative PolkaVM applications with `init` and `update` exports,
the Host imports available to those applications, guest-memory rules, common
resource bounds, and failure behavior.

Graphics command payloads are defined by the separately versioned Framebuffer,
[Tri2D](tri2d-v1.md), WebGPU Raster, and WebGPU profile contracts. The `_pvm_start` CoreVM
compatibility path is outside this ABI and must be specified separately before
it is advertised as a portable Product runtime.

## Conformance language

The key words MUST, MUST NOT, REQUIRED, SHOULD, SHOULD NOT, and MAY are to be
interpreted as described in RFC 2119.

A Host advertises PolkaVM application ABI v1 only when its observable behavior
conforms to this document and the conformance fixtures associated with it.

## Program model

The executable is a valid PolkaVM program selected by the App manifest's
archive-relative `runtime.entrypoint`.

The program MUST export:

```text
init() -> ()
update() -> ()
```

The Host instantiates a fresh program, calls `init` exactly once, and calls
`update` zero or more times while the App is running. Calls are serialized; the
Host MUST NOT enter the same program concurrently.

The Host selects and enforces a nonzero gas budget for each call. It MAY
execute that budget as smaller internal quanta, returning to its scheduler and
resuming the same call between quanta. This preserves the program counter,
registers, memory, and remaining call budget; it is not a transparent restart.
Exhausting the complete call budget, a trap, invalid guest-memory access, or a
Host-call budget failure fails the current execution. ABI v1 does not restart
a failed program transparently.

The Host owns scheduling and presentation. Returning from `update` yields
control to the Host; it does not imply that a frame was presented.

## Byte order and guest memory

All integer and sample encodings defined by this ABI are little-endian.
Pointers are `u32` offsets into guest memory. A pointer is valid only for the
duration of the Host call receiving it. The Host MUST copy or consume the
referenced bytes before returning and MUST NOT retain guest pointers.

A Host MUST bounds-check every guest read and write. Integer overflow while
computing a range is an invalid guest-memory access. A failed memory access
fails the current execution unless an individual Host call explicitly defines
a returned status for that condition.

## Capability gating

The App manifest selects exactly one graphics profile and may enable audio.
A Host call made outside its declared graphics or audio capability MUST fail
with that call's unavailable or invalid-state result. The Host MUST NOT
silently reinterpret a submission as another graphics profile. Application
input and file input are part of the base ABI and are never enabled by a
manifest capability.

## File-type hint

An App manifest MAY carry a top-level `fileTypes` list naming the files the App
opens:

```json
{
  "fileTypes": [
    {
      "label": "SNES cartridge image",
      "extensions": [".sfc", ".smc"],
      "handler": "snes-rom"
    }
  ]
}
```

Each entry has a `label`, at least one of `extensions` or `mimeTypes`, and an
optional `handler`, each following the rules for the matching §File input
descriptor field; `handler` names the `id` the App registers for those files.
The list holds at most 16 entries. A Host MAY use it to suggest the App for a
file before the App runs. The list is not a capability and grants nothing: a
Host delivers a file only to a runtime registration.

## Host imports

### Cooperative update scheduling

```text
host_update_after(delay_ms: u32) -> ()
```

Importing `host_update_after` opts a cooperative application guest into
demand-driven updates. The Host performs the first `update` after `init`
automatically. Before each later update, the Host clears the previous request.
Calls made during that Host update select the smallest requested delay.

The CoreVM compatibility path recognizes the same import and applies equivalent
behavior to the initial `_pvm_start` slice and each later resume. This is Host
compatibility behavior, not part of the portable CoreVM contract.

`delay_ms == 0` requests another update as soon as the Host can schedule it.
`delay_ms == u32::MAX` requests no timer; the Host waits until input, a
Host-frame response, a GPU event, or another external event is queued for the
guest. Every such event MUST wake an opted-in guest promptly.

A guest that does not import this call retains Host-defined continuous
scheduling for compatibility. Scheduling does not weaken per-update gas or
Host-call budgets.

A Host may hard-pause execution. It MUST release held input before pausing,
discard queued gameplay actions and audio, and prevent new gameplay presses
from accumulating. Releases and viewport state may remain pending until the
first resumed update. Hard-paused execution does not process updates or
external-event wakes. Execution-scoped monotonic time excludes the pause; wall
time does not. Resume MUST NOT replay missed update ticks or buffered audio.

The browser endpoint distinguishes this hard pause (`pause` / `pause-state`,
with boolean `paused`) from presentation inactivity (`background` /
`background-state`, with boolean `backgrounded`). A background request MAY carry
a nonnegative safe-integer `seq`, echoed by its acknowledgment before resumed
presentation. Both states are retained before and during startup, allowing
initialization but withholding ordinary updates. Hard pause takes precedence.
Overlapping inactive intervals freeze elapsed update time once, not once per
reason, and both states discard gameplay input, motion, and audio.

Background mode is **not simulation suspension**. Host-frame responses wake
bounded service updates for legacy as well as demand-driven guests, without
periodic background timers or honoring guest update-delay requests. Responses
remain ordered in the existing bounded queue; rejection due to queue pressure
is retryable and also wakes service work. A coalesced burst allows up to 32
service updates, with up to 32 additional translated cooperative continuation
slices per response wake; exhausted work waits for another external response
or foreground resume rather than spinning indefinitely. Guests must poll their
responses to make progress. Service updates may read real wall time, change
guest state, submit saves, or perform external side effects. Hosts MUST NOT
stop subscriptions, coalesce responses, or discard protocol/GPU work merely
because presentation is inactive.

The browser runtime retains the latest complete framebuffer for foreground
resume, including idle guests. Tri2D retained-resource transitions MUST still
be applied atomically and in order while inactive; a Host may hold only the
latest completed offscreen presentation, not only the latest Tri2D byte stream.
The same distinction applies to WebGPU command execution versus visible surface
presentation; already submitted GPU work may complete at the transition.
Hosts suppress clipboard/navigation interactions, defer pointer-capture
acquisition, cancel new mediated-input prompts (including file pickers) while
inactive, and retain current cursor/IME state for resume. Stopping MUST clear
retained presentation so queued callbacks cannot replay stale output.

### Framebuffer presentation

```text
host_present_frame(
  pointer: u32,
  width: u32,
  height: u32,
  stride: u32
) -> u32
```

The call submits one complete packed framebuffer. `stride` MUST equal
`width * 4`. The selected graphics profile MUST be `framebuffer`.

Return values:

```text
0  accepted
1  invalid dimensions, stride, or byte length
3  framebuffer profile unavailable for this execution
```

The Framebuffer profile contract defines pixel order, dimensions, and
presentation semantics.

### Tri2D presentation

```text
host_tri2d_submit(pointer: u32, length: u32) -> u32
```

The call submits one complete Tri2D command stream. The selected graphics
profile MUST be `tri2d`. ABI v1 accepts at most one Tri2D submission during one
`init` or `update` call.

Return values:

```text
0  accepted
1  malformed or out-of-bounds command stream
2  a Tri2D stream was already submitted during this call
3  Tri2D profile unavailable for this execution
```

The [Tri2D profile contract](tri2d-v1.md) defines the command stream and
retained-resource semantics.

### WebGPU capabilities

```text
host_gpu_capabilities(pointer: u32, capacity: u32) -> i32
```

The selected graphics profile MUST be `webgpu-raster` or `webgpu`. The Host
writes the current WebGPU capability record when the supplied capacity is
sufficient. `webgpu-raster` exposes only raster commands. `webgpu` exposes the
same resource table plus compute pipeline and dispatch commands.

Return values:

```text
> 0  capability-record bytes written
  0  capabilities are not ready
< 0  required capacity, represented as the negated byte count, or a stable
     GPU error defined by the selected WebGPU contract
```

#### Layered and volume textures

Wire version 1 has an additive texture extension, available in both WebGPU
profiles. Capability key `22` (`RasterFeatures`) bit `0` advertises 2D arrays,
cube/cube-array views, and 3D color sampling. Keys `23` and `24` report
`MaxTextureDimension3d` and `MaxTextureArrayLayers`, each capped at 256.
Guests MUST check this feature bit and the advertised limits before using the
extension. An absent feature entry does not grant support.

`CreateTexture` (opcode 3) retains its compact 24-byte 2D payload. Its layout is:

```text
offset  type   meaning
0       u32    resource handle
4       u32    width
8       u32    height
12      u16    mip-level count
14      u16    sample count (1)
16      u16    texture format
18      u8     dimension: 1 = 2D, 2 = 3D
19      u8     flags: bit 0 = explicit depth or array-layer count
20      u32    texture usage
24      u32    depth or array-layer count, present only when flag bit 0 is set
```

The payload is exactly 28 bytes with the flag, otherwise exactly 24 bytes.
Unknown flags are invalid. A 3D texture requires the flag. For 2D textures the
extra count is the number of array layers; without it the count is one.
All dimensions and counts are nonzero. The 4096 limit for 2D width/height is
unchanged; 3D width, height and depth obey key 23. Mip counts cannot exceed
the dimensions. Depth formats and render-attachment usage are not supported
for 3D textures by this extension.

`CreateTextureView` (opcode 23) keeps its 20-byte payload. View dimension at
offset 10, and the texture-binding layout's view-dimension field, use the
same values: `1` = 2D, `2` = 2D array, `3` = cube, `4` = cube array,
`5` = 3D. Its existing base-array-layer/count fields at offsets 16/18 select
layers of a 2D texture. A 2D view selects one layer; a cube selects six;
a cube array selects a nonzero multiple of six. Cube views require a square
base texture, even if a nonsquare texture's last mip is square. A 3D view
requires a 3D source and both array fields zero. Views retain their source
format, and mip/layer ranges must fit. Texture bindings must match the view
dimension, sample type and texture-binding usage.

`WriteTexture` (opcode 4) already carries origin Z and copy depth/layer count.
Array-layer counts stay constant across mips; 3D depth shrinks with each mip.
Upload strides must cover every row/image, including intermediate padding;
the byte range may end at the last texel of the final row. R8 copies use one
byte per texel, color RGBA/BGRA copies four. Depth uploads are not supported.
The 16 MiB per-tick inline-upload budget is unchanged.

The native backend also charges the backend's aligned staging row span,
including gaps between array layers or volume slices, against that budget.
Short final rows are padded before the backend copy; a small inline payload
does not authorize an unbounded staging allocation.

Texture quota accounting conservatively reserves four bytes per texel,
including R8, across every layer and mip. The 256 MiB live texture limit
and per-batch allocation budget, and the 512-texture limit, apply to the
expanded dimensions. A selected array layer or cube face may be rendered
through a single-mip 2D view with render-attachment usage; cube/array/3D views
are not themselves render attachments.

#### Stencil and depth bias

Wire version 1 has a second additive raster extension, available in both
WebGPU profiles. `RasterFeatures` (key `22`) bit `1` (value `2`) advertises
it. Guests MUST check the bit before using any of the following; an absent
bit does not grant support.

- Texture format `8` is `Depth24PlusStencil8`. It follows every depth-format
  rule: no uploads, no 3D textures, four quota bytes per texel. A render
  attachment view of it uses aspect `1` (all); a sampled view uses aspect `2`
  (depth only) and binds as depth or unfilterable float.
- `BeginRenderPass` (opcode 12) keeps its 36-byte payload: color view,
  depth view, surface generation, flags, clear RGBA and clear depth. Flags
  `16` (stencil load), `32` (stencil store) and `64` (stencil clear value)
  join color load `1`, color store `2`, depth load `4` and depth store `8`.
  Flag `64` appends a `u32` stencil clear value at offset 36 (payload 40
  bytes), at most 255. Without flag `16` the stencil aspect is cleared to
  that value, or to 0 without flag `64`; flags `16` and `64` together are
  invalid. Stencil flags require a depth attachment with a stencil aspect.
  Other flag bits are invalid.
- `CreateRenderPipeline` (opcode 10) flag `2` appends a 24-byte trailer
  after the color targets. Other flag bits besides depth write `1` are
  invalid.

  ```text
  offset  type   meaning
  0       4 u8   front face: compare, fail op, depth-fail op, pass op
  4       4 u8   back face: compare, fail op, depth-fail op, pass op
  8       u8     stencil read mask
  9       u8     stencil write mask
  10      u16    zero
  12      i32    constant depth bias
  16      f32    depth bias slope scale
  20      f32    depth bias clamp
  ```

  Compare functions use the depth-compare values. Stencil operations are
  `1` keep, `2` zero, `3` replace, `4` invert, `5` increment-clamp,
  `6` decrement-clamp, `7` increment-wrap, `8` decrement-wrap. The trailer
  requires a depth format. A face other than compare always with keep
  operations requires a stencil format. Point and line topologies require
  all three bias values to be zero. Without the trailer the pipeline keeps
  the WebGPU defaults: stencil always/keep, masks 0xFF, no bias.
- `SetStencilReference` (opcode 30) carries one `u32` reference, at most
  255, and is valid only inside a render pass. As in WebGPU, every pass
  starts with reference 0.

#### Blend constant

Wire version 1 has a third additive raster extension, available in both
WebGPU profiles. `RasterFeatures` (key `22`) bit `2` (value `4`) advertises
it; an absent bit does not grant support.

`SetBlendConstant` (opcode 31) carries the blend constant as four `f32`
values, red, green, blue and alpha, in a 16-byte payload. Every value MUST be
finite. The command is valid only inside a render pass and applies to draws
recorded after it in that pass. As in WebGPU, every pass starts with constant
`0, 0, 0, 0`. Blend factors `12` (constant) and `13` (one minus constant) read
this value; without the extension they always see zero.

#### Occlusion queries

Wire version 1 has a fourth additive raster extension, available in both
WebGPU profiles. `RasterFeatures` (key `22`) bit `3` (value `8`) advertises
it; an absent bit does not grant support.

- `BeginRenderPass` (opcode 12) flag `128` declares occlusion queries for the
  pass. It appends a `u32` query count and a guest-chosen `u32` token after
  the payload's other fields, including any stencil clear value (payload 44
  bytes, or 48 with flag `64`). The count is nonzero, and the counts of all
  passes in one batch total at most 4,096. Payloads without the flag keep
  their existing layout.
- `BeginOcclusionQuery` (opcode 32) carries one `u32` query index, below the
  pass's count. `EndOcclusionQuery` (opcode 33) has an empty payload. Both are
  valid only inside a render pass that declared queries. As in WebGPU, queries
  do not nest, each index begins at most once per pass, an end needs an open
  query, and the pass MUST NOT end while a query is open. A violation rejects
  the batch.

After the batch completes, the Host resolves each declaring pass and reads its
results back without delaying the batch or later submissions. It delivers one
event `9` (occlusion results) per declaring pass, after that batch's
`submission complete` event, in submission order. The event header carries
the batch sequence. Its payload is:

```text
offset  type      meaning
0       u32       token from BeginRenderPass
4       u32       query count N
8       N x u64   samples that passed the depth and stencil tests, by index
```

Zero means no sample passed; an index the pass never began reports zero.
Guests SHOULD treat any nonzero value as visible, because backends may report
a conservative count rather than the exact number of samples. Results of a
batch whose device is lost or reset, or of a stopped execution, are not
delivered. Native Hosts receive outstanding results from
`NativeGpuRenderer::poll_events` as well as from later `execute` calls.

The browser backend retains at most 16,384 unresolved queries across 64
readback passes. Admission includes readbacks still awaiting asynchronous
mapping, not only submitted batches; exceeding either bound rejects the
batch before GPU mutation. Completion or teardown releases the reservation.
Reset, device loss and stop destroy pending readback buffers, and results
from a retired device cannot delay or overwrite a replacement device's work.

### WebGPU submission

```text
host_gpu_submit(pointer: u32, length: u32) -> i32
```

The call submits one complete WebGPU batch. Acceptance means that the batch
passed synchronous Host validation and was queued; it does not imply shader
compilation or GPU completion. A `webgpu-raster` Host MUST reject compute
commands. A `webgpu` Host accepts both raster and compute commands.

Return values are defined by the selected WebGPU contract. ABI v1 reserves:

```text
 0  accepted
 1  bounded backpressure; the guest may retry
-1  invalid guest range
-2  malformed batch
-3  quota exceeded
-4  invalid or stale resource handle
-5  invalid lifecycle or profile state
-6  stopped execution
```

### WebGPU events

```text
host_gpu_receive(pointer: u32, capacity: u32) -> i32
```

The call reads the oldest queued WebGPU event. Occlusion results (event `9`)
are described with the occlusion-query extension.

```text
> 0  event bytes written
  0  no event is available
< 0  required capacity, represented as the negated byte count, or a stable
     GPU error defined by the selected WebGPU contract
```

#### Surface resize

`SurfaceChanged` (event `6`) publishes new capabilities without invalidating
guest-owned resources or abandoning previously submitted work. The surface
generation in `BeginRenderPass` applies only to color view `0`, the Host's
default surface. Explicit color/depth views retain their own handle lifetimes
and attachment validation across a resize; their pass does not depend on the
default surface's generation.

A default-surface pass with an obsolete generation is rejected with
`BatchRejected` error code `4` and the offending command index. The browser
validates the whole batch before mutation; native execution may already have
applied commands preceding the rejection. Guests MUST NOT blindly replay or
discard resource-bearing batches on this error. A guest can isolate its final
resource-free screen blit in a separate batch, retire that submission on a
stale-surface rejection at command `0`, and present the next frame using fresh
dimensions. Keep the generation and dimensions from the same snapshot; do not
retag an old viewport or depth attachment with a new generation.

Queued batches deliver terminal completion/rejection events in submission order.
Offscreen and resource-only batches still complete, but do not count as
presented frames. A discarded obsolete blit does not count as a presentation.

#### Device loss and restoration

A browser or driver may take the GPU device away at any time — a driver reset,
a backgrounded tab, memory pressure — and the App did nothing wrong when it
happens. The Host reports the transition through two events:

```text
7  device lost
8  device restored
```

On `device lost` every resource handle the guest holds is dead and every
in-flight submission is abandoned; the Host will not accept batches until the
device comes back. A Host that can rebuild the device MUST then publish fresh
capabilities through `host_gpu_capabilities` and emit `device restored`; the
guest re-reads capabilities, re-creates its resources with the handles it
wants, and resumes drawing. The surface generation and device generation in the
capabilities record both identify the new device, so a guest that caches either
can detect the change.

A guest MUST NOT treat `device lost` as fatal on its own. A Host that cannot
rebuild the device emits no `device restored` and terminates the application
through its ordinary lifecycle, which is the Host's decision to make, not a
trap the guest raises. A Host MUST bound its rebuild attempts so a permanently
broken adapter cannot loop.

### Host-frame transport

Every ABI v1 application receives a bounded transport for opaque request and
response frames. The runtime does not define frame encoding or service
semantics.

```text
host_frame_send(pointer: u32, length: u32) -> u32
```

The call copies one complete request frame into the Host's FIFO request queue.
It returns:

```text
0  accepted
1  empty or larger than the frame limit
2  request queue count or byte limit reached
```

```text
host_frame_poll(pointer: u32, capacity: u32) -> i32
```

The call reads the oldest complete response frame. A successful read removes
that response from the queue.

```text
> 0  response bytes written
  0  no response is available
< 0  required capacity, represented as the negated byte count; the response
     remains queued
```

Request and response queues are independent. ABI v1 allows frames up to
1 MiB, at most 32 queued frames, and at most 4 MiB of queued frame bytes in
each direction. The Host MUST reject an empty or over-limit response before it
becomes visible to the guest.

The host-frame transport is part of the base application ABI and does not
require a manifest capability. Product identity, execution kind, permissions,
and service availability remain Host policy.

### Input

```text
host_poll_input(pointer: u32, capacity: u32) -> u32
```

The Host writes as many complete eight-byte input records as fit in `capacity`
and returns the number of bytes written. It never writes a partial record.
Zero means that no event was available or that the capacity was smaller than
one record.

The legacy fixed record layout is:

```text
offset  type  field
0       u8    event type
1       u8    code
2       u16   x
4       u16   y
6       u16   zero
```

ABI v1 event types are:

```text
1   key down
2   key up
3   pointer button down
4   pointer button up
5   pointer position
6   pointer delta
7   surface metrics
8   committed UTF-8 text chunk
9   IME preedit UTF-8 chunk
10  IME commit UTF-8 chunk
11  IME enabled
12  IME disabled or cancelled
13  focus (`code` is 0 or 1)
14  wheel delta (`x` and `y` are signed i16)
15  pointer capture (`code` is 0 or 1)
16  safe-area inset pair (`code` is 0 or 1)
17  virtual-keyboard occlusion inset pair (`code` is 0 or 1)
18  touch start
19  touch move
20  touch end
21  touch cancel
```

Pointer movement and buttons, physical key transitions, committed text, IME,
focus, wheel, and surface metrics are baseline application input. An App does
not declare them in its manifest. A Host with no source for an optional input
simply emits no records for it, and that absence is not a launch failure.
Pointer capture is Host policy and is never selected by the manifest. The guest
arms capture through the pointer-capture hostcall below, and the Host decides
when an activation is eligible.

Touch records use `code` as a Host-assigned contact ID and `x`/`y` as the
physical-pixel position. An ID MUST remain stable from start through end or
cancel and MUST NOT be reused while active. Hosts MAY omit touch input when
unavailable. Touch contacts are independent of the compatibility pointer
stream; a Host MUST NOT synthesize pointer records for non-primary contacts.

Safe-area and virtual-keyboard occlusion values are unsigned physical pixels
measured inward from the current render-surface edges. Each complete update is
an atomic pair:

```text
code  x       y
0     left    right
1     top     bottom
```

Bytes 6–7 are zero. The record with `code` 0 MUST precede the record with
`code` 1, and the Host MUST queue both records together whenever one source
changes and after surface metrics change: the runtime rejects a lone record and
delivers a queued pair in one poll unless the guest's buffer is smaller than
two records. `left` plus `right` MUST NOT exceed the surface width and `top`
plus `bottom` MUST NOT exceed the surface height, so a guest can subtract them
without producing an inverted rectangle. A Host that cannot observe a source
emits no records for it; the guest treats an absent source as four zero insets.
A Host that has emitted a non-zero pair MUST emit a zero pair once that source
stops occluding the surface — a dismissed keyboard or a rotation that removes a
cutout — because the guest keeps the last pair it received until then.
Virtual-keyboard insets describe the edge-connected occlusion while a
Host-owned text-input agent is active. A floating keyboard that touches no
surface edge is not representable and does not reduce the rectangular content
area. Safe-area and keyboard values remain separate so a guest can diagnose
each source and combine them without double-counting overlap.

Text and IME records use `code` bits 0–2 as a payload length from zero through
six, bit 6 for the first chunk, and bit 7 for the last chunk. Bytes 2–7 contain
the chunk and zero padding. A complete text event is at most 4 KiB. The Host
MUST queue all chunks of one event atomically; the guest MUST reject malformed
flag sequences or invalid UTF-8.

### Mediated input

Large or permissioned inputs use a request lifecycle instead of the eight-byte
event queue. The guest registers a Host-owned input kind and the exact media
type it accepts, then explicitly triggers that registration:

```text
host_input_register(
  kind_pointer: u32,
  kind_length: u32,
  media_type_pointer: u32,
  media_type_length: u32,
  max_bytes: u32
) -> i32
host_input_trigger(handle: u32) -> u32
host_input_status(handle: u32) -> u32
host_input_read(handle: u32, pointer: u32, capacity: u32) -> i32
host_input_cancel(handle: u32) -> u32
```

`kind` and `media_type` are lowercase ASCII tokens using letters, digits,
`-`, `.`, `_`, or `+`; each starts and ends with a letter or digit. Kinds are
at most 32 bytes and media types at most 64 bytes. A media type is a
kind-defined token, such as the UR type for `camera-ur`; it is not a MIME type. `max_bytes` is in
`1..=1048576`. One execution may hold at most eight registrations, counting
file registrations from `host_file_register`. Repeating
an identical registration returns the existing positive handle. Registration
otherwise returns:

```text
-1  malformed token, size, or guest range
-2  kind unavailable for this execution
-3  registration quota exhausted
```

`host_input_trigger` returns 0 when accepted, 1 for an unknown handle, and 2
while any registration is already active or undrained Host commands exhaust
admission capacity. Hosts reserve capacity for cancellation and teardown.
Returning 2 does not release an existing selection; the guest may retry after
the Host drains commands. Acceptance means only that the Host will present its
own consent and capture UI. The guest does not receive raw device frames and
cannot bypass Host permission policy.

`host_input_status` returns:

```text
0  invalid handle
1  registered and idle
2  active
3  decoded result ready
4  cancelled
5  permission denied
6  capture or decode failed
```

When status is 3, `host_input_read` writes the complete decoded value. A
successful read returns its positive byte length and resets the registration
to status 1. Capacity smaller than the result returns the negated required
length without consuming it. Other states and unknown handles return zero.
The Host MUST reject empty results and results larger than the registration's
bound before they become visible to the guest.

`host_input_cancel` returns 0 and tells the Host to stop capture for an active
request, returns 0 and discards the result of a handle in status 3, 1 for an
unknown handle, or 2 for any other state. Either success resets the
registration to status 1. Runtime teardown cancels every active request and
releases every device stream.

ABI v1 defines the `camera-ur` kind. Its media type is the expected UR type.
The Host owns camera access, QR recognition, UR fountain reconstruction, and
type filtering; only the reconstructed UR CBOR bytes cross into guest memory.
Apps discover `camera-ur` support through `host_input_register`. An unavailable
kind returns `-2` from registration; it does not make the executable
structurally incompatible.

#### File input

```text
host_file_register(pointer: u32, length: u32) -> i32
host_file_info(handle: u32, pointer: u32, capacity: u32) -> i32
host_file_read(handle: u32, offset: u32, pointer: u32, length: u32) -> i32
host_file_cache_reset(handle: u32, size: u32) -> i32
host_file_cache_write(handle: u32, offset: u32, pointer: u32, length: u32) -> i32
host_file_cache_commit(handle: u32) -> i32
host_file_cache_read(handle: u32, offset: u32, pointer: u32, length: u32) -> i32
```

`host_file_register` registers one file handler described by a UTF-8 JSON
object of at most 4 KiB:

```json
{
  "id": "snes-rom",
  "label": "SNES cartridge image",
  "extensions": [".sfc", ".smc", ".swc", ".fig"],
  "maxBytes": 16777216,
  "delivery": "relaunch",
  "mountPath": "game/cartridge.sfc"
}
```

- `id` is 1 to 64 bytes of lowercase letters, digits, and `-`, starting and
  ending with a letter or digit, and is unique within the execution.
- `label` is 1 to 80 UTF-8 bytes shown in Host UI.
- `extensions` holds at most 16 unique values of `.` followed by 1 to 16
  lowercase letters or digits.
- `mimeTypes` holds at most 16 unique lowercase `type/subtype` values of at
  most 127 bytes, each part using letters, digits, and `!#$&^_.+-`. Wildcards
  and parameters are invalid.
- At least one of `extensions` or `mimeTypes` is non-empty.
- `delivery` is `inline`, `relaunch`, or `stream`.
- `maxBytes` is an integer in `1..=8388608` for `inline`,
  `1..=134217728` for `relaunch`, and `1..=4294967295` for `stream`.
- `mountPath` is present only for `relaunch`. It is an archive-relative UTF-8
  path of at most 1,024 bytes with no empty, `.`, or `..` segment, no leading
  `/`, no `\`, and no control character. It differs from `runtime.entrypoint`
  and from every other relaunch registration of the execution.

The descriptor MUST NOT contain unknown fields or duplicate keys, and every
number is an integer. Rejecting unknown fields means a guest cannot probe for a
field a Host does not implement; a new field requires a new descriptor shape.

The call returns a positive handle shared with mediated input, so
`host_input_trigger`, `host_input_status`, `host_input_read`, and
`host_input_cancel` apply unchanged. Registering a descriptor equal after
parsing to an existing one returns the existing handle; `extensions` and
`mimeTypes` compare as ordered lists. Registration otherwise
returns:

```text
-1  malformed descriptor, guest range, or an existing id with a different
    descriptor
-2  file input unavailable for this execution
-3  registration quota exhausted
-4  delivery mode unavailable for this execution
```

A file reaches the guest only through Host UI: a picker opened by
`host_input_trigger`, a Host menu entry, or a file the user brings to the App,
such as by dropping it on the surface, opening it from a share sheet, or
choosing the App in an open-with chooser. The user's selection is the consent.
The Host activates only an idle registration, one in status 1, 4, 5, or 6
while no other registration is active. When a file matches several
registrations, the user chooses among them; the Host never picks silently. A
file that matches no registration is refused in Host UI and does not change any
status. The Host rejects an empty matched file or one above `maxBytes` and reports
status 6 without exposing any bytes. A dismissed picker reports status 4. Every status
change is an external event and wakes a guest that imports
`host_update_after`. While the execution is paused or backgrounded, the change
is retained and observed by the next executed update; it does not start one.

Extension and MIME-type matching selects a registration; it does not validate
the contents. The guest MUST treat the bytes as untrusted input.

While status is 3, `host_file_info` writes a UTF-8 JSON object describing the
selected file:

```json
{ "name": "Example Game.sfc", "mimeType": "", "size": 1048576 }
```

`name` is the base name the Host received, with any path removed and control
characters replaced; it is 1 to 1,024 bytes, and a file whose name is empty
after sanitizing is rejected. `mimeType` is the lowercase `type/subtype` the
Host resolved, without parameters, or empty when it has none. The call returns the written byte length, the negated required length
when `capacity` is too small, zero when no file is selected, or `-1` for an
unknown handle or invalid guest range.

For `inline` delivery, the selected file becomes a status 3 result read with
`host_input_read`. The read counts against the per-update Host-call byte
budget.

For `stream` delivery, the Host retains a disk-backed source rather than copying
the file into guest memory or the asset archive. `host_file_read` synchronously
copies at most 65,536 bytes from the selected file, starting at `offset`, into
writable guest memory at `pointer`. `length` MUST be in `1..=65536`. The result
is the number of bytes copied, limited by the remaining file length; at EOF it
is zero. The call returns:

```text
-1  unknown handle, non-stream handler, or no selected source
-2  invalid length or offset beyond the selected file size
-3  destination is not a writable guest range
-4  source I/O failure or short read
```

The Host validates the destination before performing I/O and charges the
existing Host-call and byte budgets. Invalid requests leave the selection
unchanged. Successful reads retain status 3 and metadata; `host_input_read`
returns zero without acknowledging a stream. `host_input_cancel` releases the
source, clears metadata, and returns the registration to status 1. Opening a
new picker also releases the old source. An I/O failure releases the source,
clears metadata, and reports status 6. Stopping the execution releases all
selected sources. A refused replacement MUST NOT release the existing source.

Native Hosts can supply `LocalFileSource` around an already user-selected file.
Browser Hosts advertise stream support only from workers with `FileReaderSync`,
retain the selected `File`/`Blob`, and read bounded slices there. File paths,
private source tokens, and arbitrary filesystem access are not guest APIs.

A Host MAY attach a private, disk-backed working cache to a selected stream.
This stores derived bytes without changing the original selection, its metadata,
or the asset archive. It is session scratch, not a persistent save or arbitrary
filesystem API. No guest-chosen path or manifest capability is involved.

- `host_file_cache_reset` reserves `size` bytes, truncates previous contents,
  and starts an unsealed cache with write cursor zero. Size MUST be nonzero;
  aggregate reservations across the execution MUST NOT exceed 536,870,912 bytes.
  Invalid size or quota requests preserve the previous cache.
- `host_file_cache_write` copies `1..=65536` guest bytes to the exact current
  write cursor, advancing it by the returned byte count. Sparse, out-of-order,
  over-length, and sealed writes are rejected.
- `host_file_cache_commit` requires every declared byte to have been written.
  It flushes and seals the cache. Reset and commit return zero on success.
- `host_file_cache_read` reads only sealed caches, with the same bounded reads,
  EOF clamping, guest-memory validation, and budget charging as `host_file_read`.

Cache calls return `-1` for an unavailable cache (including unsealed reads),
`-2` for invalid state, range, size, or quota, `-3` for invalid guest memory,
and `-4` for backend I/O or short transfers. Backend failure drops the derived
cache without changing the Ready original source. Its reservation remains
charged until the backend has released the storage; asynchronous deletion MUST
NOT allow overlapping retired and live caches to exceed the aggregate quota.
Guest memory is validated before disk I/O. Cancel, picker reopen, replacement
after release, and execution stop (including a fatal guest trap or exhausted
gas budget) close both source and cache; refused candidates MUST NOT disturb
the existing selection.

Native `send_file_stream` accepts an optional `Box<dyn FileCache>`;
`LocalFileCache::new` creates private scratch in a Host-selected directory,
immediately unlinked on Unix and removed on drop elsewhere. Browser start
messages opt in with `fileCache: true`, requiring streamed-file support.
Workers use OPFS synchronous access handles by default; trusted embedders may
supply `createPolkaVmRuntime(endpoint, { createFileCache })`. The asynchronous,
zero-argument factory returns a backend implementing `size`, `reset`, `write`,
`read`, `flush`, and `close`. Guest calls remain synchronous. Cache opening is
bounded and completed before Ready delivery; stale candidates are closed.
Normal browser termination waits for queued close/deletion operations and
reports failures with an error and `terminated.cleanupFailed: true`. A hard
worker or browser-process kill can leave origin-private scratch behind.

For `relaunch` delivery, the Host stops the execution and starts a fresh
execution of the same App. A Host MUST obtain confirmation before stopping an
execution the user is interacting with. In the fresh execution,
`host_asset_read` at `mountPath` returns the selected file in place of any
archive asset at that path. The fresh execution starts with no registrations
and registers its handlers again during `init`. A handler registered with the
same `id`, `delivery`, and `mountPath` and a `maxBytes` no smaller than the
file reports status 3, and `host_file_info` describes the mounted file until the
guest acknowledges it; any other re-registration stays at status 1 while the
file remains mounted. For a relaunch handle, `host_input_read` writes nothing,
returns zero, and acknowledges the file, as does `host_input_cancel`; both
reset the registration to status 1. Whether a later launch
reuses the selection is Host policy; a Host SHOULD offer to reopen the last
file.

The Host retains the most recent non-empty set of registrations until a later
execution of the App registers a handler, so it can offer to choose another
file after an execution fails, including one that fails during `init`.

When a file is opened with an App that is not running, the Host launches the
App holding the file and delivers it to the first registration that matches it.
A `relaunch` match causes one immediate relaunch with the file mounted.

ABI v1 delivers one file per selection. Multi-file inputs, such as a disc image
with its cue sheet, are outside this ABI.

Mobile platform pickers filter by system type rather than extension. A Host MAY
present an unfiltered picker and match the extension afterwards, and Apps
SHOULD list `mimeTypes` wherever the format has a registered type.

### Pointer capture

```text
host_pointer_capture(request: u32) -> i32
```

The hostcall is part of the base ABI and MUST always resolve. `request` is 1 to
arm capture for the next eligible primary activation and 0 to release capture
and disarm it. A Host without capture support returns `-1` for every value; a
Host with capture support returns `-2` for every other value.

The call returns the resulting policy state:

```text
 0  released: capture is neither armed nor active
 1  armed: capture starts at the next eligible primary activation
 2  active: the Host currently captures the pointer
-1  unsupported: this Host has no pointer-capture policy
-2  invalid request
```

Arming is a request, never a guarantee: the Host owns activation eligibility,
platform permission, and the escape affordance. A Host MUST emit a pointer
capture record for every transition it makes, including capture the user ended,
so a guest that arms capture learns when capture actually started and stopped.
A Host that stops supporting capture MUST report the release first, so `-1` is
never returned while the guest still believes capture is active, and it MUST
discard the arming request it has not served yet.

A request answered with `-1` is not remembered. A Host MAY gain capture support
after the guest initialised, so a guest that still wants capture MUST re-issue
the request on a later update rather than arming once during `init`.

While capture is active the Host delivers pointer delta records; the guest
releases capture whenever it shows a cursor-driven surface such as a menu.

This hostcall is provisional. It is implemented by the reference runtime and
the reference Hosts so that first-party applications can replace Host-guessed
capture, and it is a candidate for the RFC that standardises this ABI. Until
that RFC is accepted, the import name, request values, return codes, and record
type MAY change with this draft, and no third-party Host is expected to
advertise it as a stable contract.

### UI semantics

```text
host_ui_semantics_submit(pointer: u32, length: u32) -> u32
```

The guest may submit one complete UTF-8 JSON semantic tree per `init` or
`update` call. The tree is presentation output, not an instruction to invoke
guest functions. Hosts use its roles, labels, values, actions, focus, and
surface-relative bounds for accessibility and UI automation, then deliver
actual pointer, keyboard, text, or IME records for every interaction.

The version 1 object contains `version`, monotonic `generation`, and `nodes`.
Each node contains a nonzero numeric `id`, nullable `parent`, `role`, `name`,
`value`, `[x0,y0,x1,y1]` bounds, `actions`, `disabled`, and `focused`. Version 1
allows at most 1,024 nodes, 1 KiB per name or value, and 256 KiB for the whole
tree. It requires exactly one root, unique IDs, existing parents, finite ordered
bounds, and no unknown object fields.

Return values:

```text
0  accepted
1  malformed, out-of-bounds, or over-limit tree
2  a tree was already submitted during this call
```

### UI platform output

```text
host_ui_output_submit(pointer: u32, length: u32) -> u32
```

The guest may submit one complete `PUI1` stream per `init` or `update` call.
The stream combines persistent integration state (cursor and text-editor
geometry) with ordered ephemeral commands. It is part of the base ABI and does
not require a manifest capability. Acceptance means that the stream was
validated and queued; it does not imply that platform policy allowed every
command to complete.

The fixed 48-byte little-endian header is:

```text
offset  type    field
0       [u8;4]  magic "PUI1"
4       u16     version = 1
6       u16     header bytes = 48
8       u32     total stream bytes
12      u16     command count
14      u8      cursor icon
15      u8      flags
16      f32     text editor x0
20      f32     text editor y0
24      f32     text editor x1
28      f32     text editor y1
32      f32     primary cursor x0
36      f32     primary cursor y0
40      f32     primary cursor x1
44      f32     primary cursor y1
```

Header flag bit 0 means the pointer is over mutable text. Bit 1 means the two
rectangles contain active IME geometry. Coordinates are surface-relative
logical UI points and both rectangles MUST be finite and ordered. When bit 1 is
clear, bytes 16–47 MUST be zero. Unknown flag bits are invalid.

Cursor values are:

```text
0 default        1 none             2 context-menu     3 help
4 pointing-hand  5 progress         6 wait             7 cell
8 crosshair      9 text            10 vertical-text   11 alias
12 copy         13 move            14 no-drop         15 not-allowed
16 grab         17 grabbing        18 all-scroll      19 resize-horizontal
20 resize-ne-sw 21 resize-nw-se    22 resize-vertical 23 resize-east
24 resize-se    25 resize-south    26 resize-sw       27 resize-west
28 resize-nw    29 resize-north    30 resize-ne       31 resize-column
32 resize-row   33 zoom-in         34 zoom-out
```

Each command immediately follows the previous payload:

```text
offset  type  field
0       u8    opcode
1       u8    flags
2       u16   zero
4       u32   payload bytes
8       [...] payload
```

Version 1 commands are:

```text
opcode  flags  payload
1       0      clipboard text as UTF-8
2       bit 0  non-empty URL as UTF-8; bit 0 requests a new surface
3       0      width u32, height u32, then row-major unpremultiplied sRGBA bytes
```


Commands are processed in stream order. URL bytes are untrusted input: the Host
MUST apply its navigation scheme, origin, permission, and user-gesture policy,
and a new surface MUST NOT retain a privileged opener. Clipboard access remains
subject to platform policy. Image dimensions are non-zero, at most 2048 on
either axis, at most 1,048,576 total pixels, and followed by exactly
`width * height * 4` bytes. Unknown opcodes are rejected rather than ignored.

A stream is at most 4.25 MiB and contains at most 64 commands. Clipboard text is
at most 64 KiB, one clipboard image is at most 4 MiB, and a URL is at most
8 KiB. All reserved bytes and unsupported flags MUST be zero. The encoded total
must end exactly after the declared command sequence.

Return values:

```text
0  accepted
1  malformed, out-of-bounds, or over-limit stream
2  a stream was already submitted during this call
```

### Motion

```text
host_motion_read(pointer: u32, capacity: u32) -> i32
```

The hostcall is part of the base ABI and MUST always resolve. A Host without a
motion source returns an explicit status instead of leaving the import
unresolved. One successful read consumes the latest sample; later reads return
zero until a newer sample arrives.

```text
 48  one complete MotionSample v1 record written
  0  no newer sample
 -1  motion unavailable
 -2  motion permission denied
 -3  invalid guest output range
 -4  output capacity is smaller than 48 bytes
```

MotionSample v1 is a fixed 48-byte little-endian record:

```text
offset  type    field
0       [u8;4] magic "PMO1"
4       u16    version = 1
6       u16    flags
8       u32    byte length = 48
12      u32    nonzero sequence
16      f64    monotonic timestamp, milliseconds
24      f32    acceleration including gravity X, m/s²
28      f32    acceleration including gravity Y, m/s²
32      f32    acceleration including gravity Z, m/s²
36      f32    rotation rate alpha around Z, degrees/second
40      f32    rotation rate beta around X, degrees/second
44      f32    rotation rate gamma around Y, degrees/second
```

Flags are:

```text
bit 0  acceleration fields are valid
bit 1  rotation fields are valid
bit 2  rotation is emulated from pointer movement
```

All numeric fields MUST be finite. Pointer emulation sets alpha and all
acceleration fields to zero, fills beta and gamma, and sets bits 1 and 2.

Importing `host_motion_read` declares runtime intent to use motion, not a
manifest requirement. A Host that provides physical motion MUST authorize it
only while the execution is in the foreground and MUST stop physical sensor
acquisition when the execution loses the foreground, closes, or loses
authorization. The application MUST handle `-1` and `-2`.

### Exact binary32 arithmetic

```text
host_f32_add(a: u32, b: u32) -> u32
host_f32_mul(a: u32, b: u32) -> u32
```

These optional imports interpret both arguments and the result as raw IEEE-754
binary32 bits, not integer values or pointers. Addition and multiplication MUST
round to nearest, ties to even, with gradual underflow (no flushing subnormals
to zero), signed zeros, and signed infinities. Each operation rounds once to
binary32; it MUST NOT be fused with another operation.

NaN propagation is explicit and matches the guest soft-float implementation,
independently of the Host's native floating-point NaN conventions:

- If either argument is a NaN, select the first NaN in argument order `a`, `b`.
- Addition returns `(selected & 0x7fffffff) | 0x00400000`: preserve the payload,
  quiet signaling NaNs, and clear the sign.
- Multiplication returns `selected | 0x00400000`: preserve the payload and
  sign, and quiet signaling NaNs.
- With no NaN operand, opposite-signed infinities added together or zero
  multiplied by infinity return the positive canonical NaN `0x7fc00000`,
  regardless of operand order or signs.

Subtraction uses `host_f32_add(a, b ^ 0x80000000)`; there is no separate
subtraction import.

These are pure, fixed-cost arithmetic operations: they access no guest memory,
allocate no per-operation storage, and perform no IO. Their guest instructions
remain gas-metered, including import calls. Hosts MUST NOT charge them against
Host IO call-count or byte budgets; this exemption does not apply to any other
Host import.

Existing guests that do not import these symbols are unchanged. Guests that
import them require an updated runtime providing this exact contract; an
unsupported Host MUST reject the import rather than silently substitute an
approximation or a fallback implementation.

The browser Wasm translator lowers these imports directly to binary32
arithmetic without a JavaScript hostcall. It checks the result for NaN and
repairs exceptional results from the original integer operand bits; non-NaN
results need no operand checks. This lowering preserves the register and gas
contract above, including when execution resumes after gas exhaustion.
When the arithmetic continuation is the adjacent block in the same generated
function, execution falls through without re-entering the group dispatcher.
The translator still records the continuation PC and retains its gas checks.
Continuations across function/module boundaries retain tail-call dispatch,
and ordinary Host imports still yield to the Host.

### Time

```text
host_time_ms() -> u64
host_sleep_ms(duration_ms: u32) -> ()
```

Applications may also use the versioned `host.core` clock and entropy operations:

```text
polkadot_host_0_1_core_clock_monotonic(destination: u32) -> i32
polkadot_host_0_1_core_clock_wall(destination: u32) -> i32
polkadot_host_0_1_core_random(destination: u32, length: u32) -> i32
```

The clock operations write a little-endian `u64` nanosecond value and return
zero. The monotonic clock is scoped to the execution; the wall clock is Unix
time. `core_random` fills exactly the requested bytes, at most 4 KiB, from the
Host CSPRNG. It returns `-3` for an empty request, `-5` when secure entropy is
unavailable, and `-6` above the per-call limit. A failed entropy request does
not write the guest destination. Invalid writable guest ranges fail the
execution. These imports do not opt the application into the separate
application-computer lifecycle.

`host_time_ms` returns a monotonic millisecond clock scoped to the execution.
It is not wall-clock time.

`host_sleep_ms` yields or advances runtime time by no more than the remaining
sleep allowance for the current call. A Host MAY return earlier than the
requested duration.

### Random

```text
host_random_fill(destination: u32, length: u32) -> u32
```

The Host fills the requested guest range from a CSPRNG. Random bytes are
independent for every execution and MUST NOT be derived from `host_time_ms`.

```text
0  accepted
1  zero length, over the per-call limit, or execution pool exhausted
```

### Audio

```text
host_audio_submit(pointer: u32, sample_count: u32) -> u32
```

Samples are interleaved signed 16-bit little-endian PCM, stereo, at 48,000 Hz.
`sample_count` counts individual channel samples and MUST therefore be even.

Return values:

```text
0  accepted
1  invalid sample count or audio queue limit reached
3  audio capability unavailable for this execution
```

### Assets

```text
host_asset_read(
  name_pointer: u32,
  name_length: u32,
  offset: u32,
  destination: u32,
  capacity: u32
) -> u32
```

The asset name is UTF-8 and relative to the application archive. The
Host writes at most `capacity` bytes starting at `offset` and returns the
number written.

Zero means the name was invalid, the asset was absent, or the offset was at or
past the end of the asset. Assets are immutable for the lifetime of one
execution. Assets come from the verified archive, except that a file delivered
through a relaunch registration replaces the asset at that registration's mount
path. That file is untrusted user input, and it counts toward the asset bounds
below.

### Save data

```text
host_save_submit(pointer: u32, length: u32) -> u32
```

The call submits one opaque save-data value for Host persistence. A later
successful submission replaces the pending value.

```text
0  accepted
1  empty or over the size limit
```

Storage lifetime, synchronization, and user controls are Host policy outside
this ABI.

### Logging

```text
host_log(pointer: u32, length: u32) -> ()
```

The Host copies at most the v1 log-byte limit and decodes the bytes as lossy
UTF-8 for diagnostics. Logs are not application storage and MUST NOT affect
application behavior.

## ABI v1 resource bounds

The initial v1 implementation applies the following ceilings:

```text
program bytes                         64 MiB
read-write data                       64 MiB
stack                                 16 MiB
heap                                  128 MiB
asset files                           2,048
one asset                             128 MiB
all assets                            256 MiB
one asset read                        16 MiB
Host IO call bytes per init/update    32 MiB
Host IO calls during init             131,072
Host IO calls during update           65,536
sleep during init                     100 ms
sleep during update                   50 ms
audio samples per submission          96,000
queued audio                           2 seconds
queued input events                   4,096
save data                             1 MiB
random bytes per call                 4 KiB
random bytes per execution            64 KiB
one log                               4 KiB
queued logs                           64
queued GPU batches                    4
queued GPU events                     256
GPU submissions per init/update       8
GPU inline uploads per init/update    16 MiB
host frame                            1 MiB
queued host frames per direction     32
queued host-frame bytes per direction 4 MiB
```

Profile contracts define their additional bounds. Conforming Hosts MUST NOT
accept values above these ceilings. Before this draft becomes stable, the Host
SDK maintainers must decide which values are also minimum capacities that every
conforming Host must provide.

## Failure and shutdown

A successful `init` does not guarantee that later updates will succeed. The
Host stops the execution on an unhandled guest trap, exhaustion of the complete
call gas budget, invalid memory access, unrecoverable profile error, or Host
transport failure. An internal execution quantum ending is not gas exhaustion
at this contract boundary.

The Host may stop an execution when its App surface closes, the Product is
replaced, the user selects a file for a relaunch registration, or platform
lifecycle policy requires termination. ABI v1 does not
promise transparent restoration of guest memory or graphics resources after a
stop.

Device loss and recoverable WebGPU errors are delivered according to the
selected WebGPU event contract. They do not permit stale resource handles to
be reused.

## Version compatibility

A Host that does not implement PolkaVM application ABI v1 MUST NOT launch an
App requesting it. A program compiled for ABI v1 imports only the symbols and
uses only the behavior defined by this document and its selected capability
contracts.

Changes to an import signature, lifecycle requirement, record layout, or
observable status meaning require a new ABI version unless explicitly defined
as a backward-compatible extension.

## Conformance

The normative fixture set contains reproducible PolkaVM guests and expected
results covering:

- required exports and initialization;
- repeated updates;
- guest traps and gas exhaustion;
- invalid guest-memory ranges;
- input record delivery;
- monotonic time;
- asset reads;
- audio submission and gating;
- save submission;
- bounded logging;
- host-frame request/response round trips and queue bounds;
- file registration bounds, file info, inline delivery, and relaunch delivery
  through assets;
- graphics-profile enforcement;
- demand-driven update deadlines, idle suspension, and external-event wakes.

Native and browser implementations MUST run the same fixture inputs. Full
sample applications are integration evidence rather than normative fixtures.

## Open questions before stabilization

- Which resource values are required minimum capacities across all Hosts?
- What stable registry defines key and pointer-button codes?
- Is `host_sleep_ms` necessary in the stable cooperative ABI, or should the
  Host own all scheduling without a guest sleep operation?
- Should save persistence be a capability declared separately from the base
  ABI?
- How is the CoreVM compatibility path named and versioned independently from
  this cooperative ABI?
- Does guest-armed pointer capture belong in the base ABI as `host_pointer_capture`,
  or in a versioned input extension negotiated separately? The call ships
  provisionally and needs the standardisation RFC before Hosts advertise it.
