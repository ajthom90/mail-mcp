#!/usr/bin/env bash
# Builds mail-mcp-daemon and copies it into the app bundle's Contents/MacOS.
# Invoked by Xcode as a pre-build phase.
#
# Debug → host arch only, debug profile (fast iteration).
# Release → universal2 (arm64 + x86_64), release profile, lipo-d together.

set -euo pipefail

WORKSPACE_ROOT="${SRCROOT}/.."
DAEMON_NAME="mail-mcp-daemon"
DEST="${BUILT_PRODUCTS_DIR}/${EXECUTABLE_FOLDER_PATH}/${DAEMON_NAME}"

# Ensure cargo is on PATH (Xcode shells inherit a stripped PATH).
export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin"

cd "${WORKSPACE_ROOT}"

# Resolve cargo. Setups vary: the standard rustup install puts shims in
# ~/.cargo/bin, but Homebrew's rustup leaves no shim. `rustup which cargo`
# resolves to the active toolchain (respects rust-toolchain.toml in cwd).
if command -v cargo >/dev/null 2>&1; then
    CARGO=cargo
    RUSTUP=rustup
elif command -v rustup >/dev/null 2>&1; then
    CARGO="$(rustup which cargo)"
    RUSTUP=rustup
    # cargo invokes `rustc` from PATH; make sure the right toolchain bin dir is on it.
    export PATH="$(dirname "$CARGO"):$PATH"
else
    echo "error: neither cargo nor rustup found on PATH" >&2
    echo "       PATH=$PATH" >&2
    exit 1
fi

if [ "${CONFIGURATION}" = "Release" ]; then
    $RUSTUP target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null
    $CARGO build --release --target aarch64-apple-darwin -p "${DAEMON_NAME}"
    $CARGO build --release --target x86_64-apple-darwin  -p "${DAEMON_NAME}"
    mkdir -p "$(dirname "${DEST}")"
    lipo -create \
        "target/aarch64-apple-darwin/release/${DAEMON_NAME}" \
        "target/x86_64-apple-darwin/release/${DAEMON_NAME}" \
        -output "${DEST}"
else
    $CARGO build -p "${DAEMON_NAME}"
    mkdir -p "$(dirname "${DEST}")"
    cp "target/debug/${DAEMON_NAME}" "${DEST}"
fi

# Make sure it's executable. cargo already does this, but cp loses mode if the
# DEST already existed with a different mode.
chmod +x "${DEST}"

echo "Bundled ${DAEMON_NAME} → ${DEST}"
