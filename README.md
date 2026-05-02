# mail-mcp

A locally-running, multi-account email service that exposes a [Model Context
Protocol](https://modelcontextprotocol.io) server so AI assistants (Claude Desktop,
Claude Code, third-party MCP clients) can read, triage, and compose email through
your existing accounts. Supports Gmail today; Microsoft 365 (Graph API) and IMAP/SMTP
in v0.2.

> [!NOTE]
> mail-mcp is in active development. v0.1a (headless backend) and v0.1b (macOS tray app)
> have shipped. v0.1c (Windows tray app) is scaffolded; v0.1d (Linux tray app) is in design.

## Architecture

```
┌─ AI client (Claude Desktop, Claude Code, …) ──────────┐
│   speaks MCP over HTTP/SSE or stdio                   │
└────────────┬──────────────────────────────────────────┘
             │
┌────────────▼──────────────────────────────────────────┐
│  mail-mcp-daemon  (Rust, long-running per-user)       │
│  - MCP HTTP server (axum)                             │
│  - Tool dispatch + permission enforcement              │
│  - Provider trait → Gmail / M365 / IMAP                │
│  - SQLite for accounts/permissions; OS keychain for    │
│    refresh tokens                                      │
└────────────┬──────────────────────────────────────────┘
             │ JSON-RPC over UDS (Unix) / Named Pipe (Windows)
┌────────────▼──────────────────────────────────────────┐
│  Tray app  (one per platform — macOS / Win / Linux)   │
│  - Status menu, settings, first-run wizard             │
│  - Approval dialogs for Send / Trash                   │
│  - Holds zero state; just a view of the daemon         │
└───────────────────────────────────────────────────────┘
```

Headless / NAS users skip the tray entirely and drive the daemon via
[`mail-mcp-admin`](crates/mail-mcp-admin/) — a CLI that speaks the same JSON-RPC.

## Status

| Milestone | What | Status |
|---|---|---|
| v0.1a | Rust core: daemon, IPC, OAuth, Gmail provider, MCP server, stdio shim, admin CLI | ✅ shipped |
| v0.1b | macOS tray app (SwiftUI + AppKit) | ✅ shipped (unsigned DMG available; signing setup pending) |
| v0.1c | Windows tray app (.NET 8 + WinUI 3 + WinForms NotifyIcon) | 🚧 scaffold landed; execution in progress |
| v0.1d | Linux tray app (C + GTK4 + libadwaita + AyatanaAppIndicator) | 📐 designed |
| v0.2.0 | Microsoft 365 (Graph API) provider | 📐 designed; OAuth config landed |
| v0.2.1 | Generic IMAP/SMTP provider | 📐 designed |

Designs and plans live under [`docs/superpowers/`](docs/superpowers/).

## Quick start (macOS dev build)

Prerequisites: Xcode 15+, Homebrew, Rust 1.88+ (via rustup), a Google Cloud
OAuth Desktop client_id.

```bash
git clone https://github.com/ajthom90/mail-mcp.git
cd mail-mcp/tray-app
brew bundle install
xcodegen generate
cp Config-Default.xcconfig Config-Local.xcconfig
# Edit Config-Local.xcconfig — set MAIL_MCP_GOOGLE_CLIENT_ID
open MailMCP.xcodeproj
# Cmd+R to build + run
```

The first launch shows a wizard. Sign in to Gmail; the menu-bar icon appears.
Add the snippet from the wizard's last step to your Claude Desktop config.

## Quick start (Windows dev build, when v0.1c lands)

Prerequisites: Visual Studio 2022 17.8+ with .NET 8 SDK + Windows App SDK
workload, Rust 1.88+, a Google Cloud OAuth Desktop client_id.

```powershell
cd mail-mcp/tray-app-win
dotnet restore
dotnet build MailMCP.sln -c Debug -p:Platform=x64
dotnet run --project MailMCP/MailMCP.csproj -c Debug -p:Platform=x64
```

## Headless / NAS use

You don't need a tray app. The daemon and admin CLI are usable on their own:

```bash
cargo run --release -p mail-mcp-daemon -- \
  --google-client-id $YOUR_CLIENT_ID \
  --root /var/lib/mail-mcp
# in another shell:
cargo run --release -p mail-mcp-admin -- \
  --root /var/lib/mail-mcp accounts add-gmail
cargo run --release -p mail-mcp-admin -- \
  --root /var/lib/mail-mcp accounts list
# Configure Claude Desktop to point at target/release/mail-mcp-stdio.
```

## Install (end users)

Pre-release unsigned DMGs are published on the [releases page](https://github.com/ajthom90/mail-mcp/releases).
First launch: right-click MailMCP.app → Open (Apple's first-launch security check).

Signed builds with auto-updates via Sparkle will land once the maintainer
finishes Apple Developer ID setup.

## Cross-platform support

The daemon is verified to compile for:

- macOS aarch64 + x86_64 (Universal2 in Release)
- Linux x86_64 + aarch64 + riscv64gc
- Windows x86_64 + aarch64 (with named-pipe IPC; landed in PR #11)

CI runs the Rust test suite on macOS + Linux hosts, plus `cargo check` for every
cross-target on every PR.

## License

Dual-licensed under [MIT](LICENSE-MIT) and [Apache-2.0](LICENSE-APACHE), at your
option.
