#!/usr/bin/env bash
# Notarizes a signed .app via notarytool, then staples.
# Required env: APPLE_API_KEY_PATH, APPLE_API_KEY_ID, APPLE_API_ISSUER_ID

set -euo pipefail

APP_PATH="${1:?APP_PATH required}"

# notarytool requires a zip, not raw .app.
ZIP_PATH="$(mktemp -d)/notarize.zip"
ditto -c -k --keepParent "$APP_PATH" "$ZIP_PATH"

xcrun notarytool submit "$ZIP_PATH" \
    --key "$APPLE_API_KEY_PATH" \
    --key-id "$APPLE_API_KEY_ID" \
    --issuer "$APPLE_API_ISSUER_ID" \
    --wait

xcrun stapler staple "$APP_PATH"
echo "Notarized + stapled: $APP_PATH"
