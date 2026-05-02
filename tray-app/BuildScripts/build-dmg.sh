#!/usr/bin/env bash
# Builds a release DMG containing MailMCP.app.
# Requires: create-dmg (Homebrew), a signed+notarized .app at $APP_PATH.
#
# Usage: build-dmg.sh <APP_PATH> <DMG_OUT_PATH> <VERSION>

set -euo pipefail

APP_PATH="${1:?APP_PATH required}"
DMG_OUT="${2:?DMG_OUT_PATH required}"
VERSION="${3:?VERSION required}"

create-dmg \
    --volname "MailMCP $VERSION" \
    --window-pos 200 120 \
    --window-size 600 400 \
    --icon-size 128 \
    --icon "MailMCP.app" 175 200 \
    --hide-extension "MailMCP.app" \
    --app-drop-link 425 200 \
    "$DMG_OUT" \
    "$(dirname "$APP_PATH")"

echo "DMG built: $DMG_OUT"
