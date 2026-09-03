---
title: "Declaring required Polkadot Host interfaces"
type: rfc-draft
status: draft — input for the TruAPI RFC process
---

# Declaring required Polkadot Host interfaces

Draft for a TruAPI RFC. Nothing here is stable until the RFC lands; the
experimental hosts implement it as a prototype.

## Motivation

A PolkaVM program blob does not self-describe which host contract it
expects: cartridge guests (application ABI v1) and computer guests
(`polkadot-host` interfaces) are both PolkaVM bytecode exporting
`_pvm_start`. Hosts must decide the execution model and the authority
grant *before* running anything, and users must be able to see what an
app requests.

Two rejected alternatives frame the design:

- **A new manifest/runtime kind** (e.g. `polkavm-computer`) duplicates
  what the capability request already says, and forces consistency
  validation between the kind and the capability list.
- **A new package container** for program-plus-metadata is unnecessary:
  the `.polkavm` container already supports optional custom sections
  that all existing parsers skip (`polkavm-common` `program.rs`: any
  section id with the high bit set is length-prefixed and skippable;
  ids 128–131 are reserved for debug data and the metadata hash).

Therefore: one small declaration record, carried in two places.

## The declaration record

A canonical UTF-8 JSON object:

```json
{
  "requires": [
    "polkadot-host/0.1/core",
    "polkadot-host/0.1/fs",
    "polkadot-host/0.1/tty",
    "polkadot-host/0.1/process"
  ]
}
```

- `requires` is a non-empty list of unique interface ids.
- Interface ids follow the ADR namespace `polkadot-host/<version>/<name>`.
  The contract version rides on the id; there is no separate ABI version
  field anywhere in the declaration.
- `polkadot-host/0.1/core` is mandatory: every conforming guest needs
  arguments, environment, and exit.
- Future extensions attach per-interface parameters as sibling keys
  (e.g. filesystem path grants); the RFC must reserve that shape.

## Carrier 1: the executable manifest

App Manifest v2 apps declare the record under `capabilities.host`:

```json
{
  "$v": 2,
  "kind": "app",
  "runtime": { "kind": "polkavm", "entrypoint": "shell.polkavm" },
  "capabilities": {
    "host": { "requires": ["polkadot-host/0.1/core", "…"] },
    "packages": [{ "name": "vim", "path": "vim.polkavm" }]
  }
}
```

- `runtime.kind` stays `polkavm`; the presence of `capabilities.host`
  selects the host-interface execution model.
- `runtime.abiVersion` MUST be absent (versions live in interface ids).
- Graphics/device-input/audio capability blocks MUST be absent; those
  belong to the cooperative application ABI. When a graphics interface
  exists (`polkadot-host/<v>/display`), it will be an interface id.
- This carrier is the pre-fetch trust anchor: it is published as the
  DotNS executable record, byte-verified against the packaged
  `manifest.json`, and evaluable before any content is fetched.

## Carrier 2: a `.polkavm` custom section

The identical record embedded in the program blob as an optional custom
section (id to be assigned by the RFC from the skippable `0x80..0xFF`
space, payload = the canonical JSON bytes).

Why a second carrier:

- **Child packages have no manifest.** A computer package such as
  `vim.polkavm` is spawned by a guest, not resolved through DotNS. The
  supervisor reads the child's declared interfaces from its blob and
  clamps grants to the intersection with the parent's authority.
- **Tooling and provenance.** `polkatool`-style tools can print what a
  blob requires; conformance CI can assert fixtures declare exactly the
  interfaces they exercise.
- **Defense in depth.** For top-level apps carrying both, hosts MUST
  refuse on mismatch between manifest and blob declarations.

Compatibility: existing loaders, the wasm translator, and gas metering
ignore unknown optional sections, so stamped blobs run unchanged on
hosts that predate this RFC.

## Host rules

1. **Fail closed.** A host MUST refuse to launch a program requiring an
   interface it cannot or will not provide, before executing anything.
2. **Grants clamp, declarations do not grant.** The record is a request;
   the host decides the actual grant and MAY grant less (e.g. deny
   network) where the interface semantics allow partial refusal.
3. **Unknown ids are errors, not warnings.** Forward compatibility comes
   from versioned ids, not from ignoring requests.
4. **Children clamp to parents.** A spawned child's effective grant is
   at most its own declaration intersected with its parent's grant.

## Open questions for the RFC

- Section id assignment and a registry for interface namespaces.
- Canonical JSON rules (key order, whitespace) so byte-compares work.
- Per-interface parameter shapes (filesystem scopes, network targets).
- Whether the metadata-hash section (131) should commit to the
  declaration bytes.
