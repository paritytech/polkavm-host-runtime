#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FIXTURES="$ROOT/rust/crates/pvm-runtime/tests/fixtures"
TOOLCHAIN="${PVM_GUEST_TOOLCHAIN:-nightly-2025-10-09}"
GUESTS=(
  "polkavm-app-v1/truapi-roundtrip pvm_truapi_roundtrip truapi-roundtrip.polkavm"
  "polkadot-host-computer-0.1/core-context pvm_computer_core_context computer-core-context.polkavm"
  "polkadot-host-computer-0.1/tty-fs-roundtrip pvm_computer_tty_fs_roundtrip computer-tty-fs-roundtrip.polkavm"
)

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

for guest in "${GUESTS[@]}"; do
  read -r directory artifact fixture <<<"$guest"
  cargo +"$TOOLCHAIN" build \
    -Z build-std=core \
    --locked \
    --manifest-path "$ROOT/conformance/runtime/$directory/Cargo.toml" \
    --target-dir "$TARGET_DIR" \
    --target "$TARGET_JSON" \
    --release

  polkatool link \
    "$TARGET_DIR/$TARGET_NAME/release/$artifact.elf" \
    -o "$TARGET_DIR/$fixture"

  cmp "$FIXTURES/$fixture" "$TARGET_DIR/$fixture"
  printf 'Verified %s\n' "$FIXTURES/$fixture"
done
