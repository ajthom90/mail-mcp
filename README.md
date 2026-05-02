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
