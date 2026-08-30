#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GUEST="$ROOT/conformance/runtime/polkavm-app-v1/truapi-roundtrip"
FIXTURE="$ROOT/rust/crates/pvm-runtime/tests/fixtures/truapi-roundtrip.polkavm"
TOOLCHAIN="${PVM_GUEST_TOOLCHAIN:-nightly-2025-10-09}"

for tool in cargo polkatool rustup; do
  command -v "$tool" >/dev/null 2>&1 || {
    printf 'missing required tool: %s\n' "$tool" >&2
    exit 1
  }
done

rustup component add rust-src --toolchain "$TOOLCHAIN" >/dev/null
RUSTC="$(rustup which --toolchain "$TOOLCHAIN" rustc)"
TARGET_JSON="$(RUSTC="$RUSTC" polkatool get-target-json-path --bitness 32)"
TARGET_NAME="$(basename "$TARGET_JSON" .json)"
TARGET_DIR="$(mktemp -d)"
trap 'rm -rf "$TARGET_DIR"' EXIT

cargo +"$TOOLCHAIN" build \
  -Z build-std=core \
  --locked \
  --manifest-path "$GUEST/Cargo.toml" \
  --target-dir "$TARGET_DIR" \
  --target "$TARGET_JSON" \
  --release

polkatool link \
  "$TARGET_DIR/$TARGET_NAME/release/pvm_truapi_roundtrip.elf" \
  -o "$TARGET_DIR/truapi-roundtrip.polkavm"

cmp "$FIXTURE" "$TARGET_DIR/truapi-roundtrip.polkavm"
printf 'Verified %s\n' "$FIXTURE"
