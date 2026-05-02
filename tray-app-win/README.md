# MailMCP — Windows tray app

This directory contains the Windows system-tray app for the mail-mcp daemon.

## Quick start (developers)

```powershell
# Prerequisites:
#   - Visual Studio 2022 17.8+ with .NET 8 SDK and the Windows App SDK workload
#   - Rust 1.88+ via rustup (the build will invoke cargo for the daemon)
#   - Optional: WinAppDriver for UI tests

# From the repo root:
cd tray-app-win
dotnet restore
dotnet build MailMCP.sln -c Debug -p:Platform=x64
dotnet run --project MailMCP/MailMCP.csproj -c Debug -p:Platform=x64
```

The first build takes a while because the cargo→exe build phase compiles
`mail-mcp-daemon` for `x86_64-pc-windows-msvc`.

## Layout

See `docs/superpowers/specs/2026-05-02-v0.1c-windows-tray-design.md` and
`docs/superpowers/plans/2026-05-02-v0.1c-windows-tray.md` in the workspace root
for the full design.
