# mail-mcp

A Model Context Protocol (MCP) server for mail, with a macOS menu-bar tray app
for local dogfooding.

## Quick start (macOS, dogfood / Phase A)

Prerequisites: Xcode 15+, Homebrew, a Google Cloud OAuth Desktop client_id.

~~~bash
git clone https://github.com/ajthom90/mail-mcp.git
cd mail-mcp/tray-app
brew bundle install
xcodegen generate
cp Config-Default.xcconfig Config-Local.xcconfig
# Edit Config-Local.xcconfig — set MAIL_MCP_GOOGLE_CLIENT_ID
open MailMCP.xcodeproj
# Run from Xcode (Cmd+R)
~~~

The first launch shows a wizard. Sign in to Gmail; the menu-bar icon appears.
Add the snippet from the wizard's last step to your Claude Desktop config.

## Install (end users)

Download the latest signed DMG from the [releases page](https://github.com/ajthom90/mail-mcp/releases), open it, and drag MailMCP into Applications. First launch:

1. Right-click MailMCP.app → Open (Apple's first-launch security check).
2. Follow the wizard to connect a Gmail account.
3. Add the snippet from the wizard's last step to your Claude Desktop config.

Updates are delivered via Sparkle when you choose "Check for Updates…" from the menu, or automatically in the background.
