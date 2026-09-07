---
title: "ADR 0001: The application runtime ABI stays at version 1"
type: decision-record
status: accepted
---

# ADR 0001: The application runtime ABI stays at version 1

- Date: 2026-09-07

## Context

`docs/runtime/polkavm-app-abi-v1.md` is the only application contract this
repository publishes. It defines the guest boundary an App v2 manifest selects:

```json
{
  "runtime": { "kind": "polkavm", "abiVersion": 1 }
}
```

Two other numbers are easily mistaken for it:

- `$v` in an App manifest versions the **manifest document**. A `$v: 2`
  manifest still selects runtime ABI 1; the two move independently.
- `abi.runtime` in a redistribution's `SOURCE.json` — notably
  `@useragent-kit/polkavm-runtime`, which records `2` — is that
  distribution's **packaging metadata**, not the guest contract.

The confusion has already cost real downtime. Consuming Hosts flipped their
manifest gate between 1 and 2 three times in one day; each flip refused every
application built against the other number, because `polkavm-app-kit` emits 1
(`scripts/prepare-app.mjs`, `tests/manifest-v2.test.mjs`) while some products
had been republished as 2 to match a strict-2 Host.

An open PR proposes renaming the repository's surfaces and versioning a future
breaking contract as App ABI v2.

## Decision

**The application runtime ABI is version 1.** Hosts accept
`runtime.abiVersion === 1`; applications declare 1.

ABI v1 has never been publicly released, so no shipped application depends on
its current shape. Breaking changes — including the renames and the neutral
`host_frame_*` transport — are spent **inside v1** rather than on a version
number nothing has shipped against. A version bump buys compatibility for
consumers that do not exist yet, and costs a coordinated republish of every
product plus a Host that must straddle both numbers during the transition.

Reserving new input record types, graphics opcodes, or hostcalls does not bump
the ABI; those are additive within v1, which is how types 16 and 17
(safe-area and virtual-keyboard insets) landed.

## Consequences

- `docs/runtime/polkavm-app-abi-v1.md` remains the single normative contract.
  Do not introduce a `polkavm-app-abi-v2.md` without superseding this ADR.
- A future v2 needs, in order: the published v2 contract document, a Host that
  accepts 1 and 2 during the transition, every product republished, and this
  ADR superseded. Changing a Host's gate on its own is not a migration.
- Redistributions may number their packaging however they like, but a Host MUST
  NOT derive an App manifest's `runtime.abiVersion` from a distribution's
  `abi.runtime` field.

## References

- `docs/runtime/polkavm-app-abi-v1.md` — §Scope shows the manifest that selects
  this ABI.
- `paritytech/dotli-community` ADR 0001 records the consuming Host's side of
  this decision.
