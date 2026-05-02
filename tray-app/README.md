# MailMCP — macOS tray app

This directory contains the macOS menu-bar app for the mail-mcp daemon.

## Quick start (developers)

```bash
brew bundle install                     # xcodegen, create-dmg
xcodegen generate                       # produces MailMCP.xcodeproj
cp Config-Default.xcconfig Config-Local.xcconfig
# Edit Config-Local.xcconfig — set MAIL_MCP_GOOGLE_CLIENT_ID to your dev client_id
open MailMCP.xcodeproj                  # build + run from Xcode
```

The build phase script invokes `cargo build` on the workspace's Rust crates and copies `mail-mcp-daemon` into the app bundle. Debug builds use the host architecture; Release builds produce universal2 (arm64 + x86_64).

## Layout

See `docs/superpowers/specs/2026-05-01-v0.1b-macos-tray-design.md` in the workspace root for the full design.
