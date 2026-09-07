---
title: "ADR 0002: This repository does not publish to npm"
type: decision-record
status: accepted
---

# ADR 0002: This repository does not publish to npm

- Date: 2026-09-07

## Context

The browser runtime is consumed by Hosts as a packaged tarball. Two channels
were possible:

- publishing `@parity/polkavm-browser-runtime` to the npm registry from this
  repository's release workflow, through `paritytech/npm_publish_automation`; or
- shipping the tarball and the asset bundle as immutable GitHub release
  artifacts, and letting `paritytech/useragent-kit` republish the distribution
  it curates for Hosts.

A release workflow briefly took the first path. It failed closed — the
automation's allowlist has no entry for this repository — and because the
publish step ran *before* `gh release create`, the v0.2.0 tag produced no
release artifacts at all. Nothing reached the registry.

## Decision

**Releases from this repository publish GitHub release artifacts only. The npm
distribution is published from `paritytech/useragent-kit`, not from here.**

The release workflow MUST NOT dispatch `npm_publish_automation`, require an
`NPM_PUBLISH_AUTOMATION_TOKEN`, or otherwise run `npm publish`. Do not add an
allowlist entry for this repository to `paritytech/npm_publish_automation`.

## Consequences

- `release.yml` packs `parity-polkavm-browser-runtime-<version>.tgz` and
  `polkavm-host-runtime-assets-<version>.tar.gz`, writes the release manifest,
  and attaches all three to the tagged GitHub release. That tarball is the
  artifact downstream Hosts vendor, recorded by upstream revision in their own
  lockfiles.
- A Host that wants the package from a registry gets it through the
  useragent-kit distribution, which owns that publication and its versioning.
- Changing the distribution channel is an organisational decision, not a
  workflow edit: supersede this ADR first.
