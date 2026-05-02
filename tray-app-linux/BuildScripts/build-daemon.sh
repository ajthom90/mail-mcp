#!/usr/bin/env bash
# Builds the mail-mcp-daemon Rust binary and copies it to the meson custom_target
# output path. Honors host architecture by default; cross-compile is selected
# by setting CARGO_TARGET=<triple> in the environment (CI sets this for the
# aarch64 cross job).
set -euo pipefail
out="$1"

# Walk up to the cargo workspace root: tray-app-linux/BuildScripts → tray-app-linux → repo root.
cd "$(dirname "$0")/../.."

target_flag=()
if [[ -n "${CARGO_TARGET:-}" ]]; then
  rustup target add "$CARGO_TARGET" >/dev/null
  target_flag=(--target "$CARGO_TARGET")
  built="target/$CARGO_TARGET/release/mail-mcp-daemon"
else
  built="target/release/mail-mcp-daemon"
fi

cargo build --release -p mail-mcp-daemon "${target_flag[@]}"
install -m 0755 "$built" "$out"
