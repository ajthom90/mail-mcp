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

if [ "${CONFIGURATION}" = "Release" ]; then
    rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null
    cargo build --release --target aarch64-apple-darwin -p "${DAEMON_NAME}"
    cargo build --release --target x86_64-apple-darwin  -p "${DAEMON_NAME}"
    mkdir -p "$(dirname "${DEST}")"
    lipo -create \
        "target/aarch64-apple-darwin/release/${DAEMON_NAME}" \
        "target/x86_64-apple-darwin/release/${DAEMON_NAME}" \
        -output "${DEST}"
else
    cargo build -p "${DAEMON_NAME}"
    mkdir -p "$(dirname "${DEST}")"
    cp "target/debug/${DAEMON_NAME}" "${DEST}"
fi

# Make sure it's executable. cargo already does this, but cp loses mode if the
# DEST already existed with a different mode.
chmod +x "${DEST}"

echo "Bundled ${DAEMON_NAME} → ${DEST}"
