# Security Policy

## Reporting a vulnerability

If you discover a security issue in mail-mcp, please report it privately so we can fix it before public disclosure.

**Where to report:**
- GitHub: open a [private security advisory](https://github.com/ajthom90/mail-mcp/security/advisories/new)
- Or email the maintainer directly (see GitHub profile)

Please do **not** open a public issue.

## What to include

- A description of the vulnerability and its impact
- Steps to reproduce, or a proof-of-concept
- Affected versions or commit SHAs
- Your name / handle for credit (optional)

## Response timeline

- Initial acknowledgement within 72 hours
- Triage and severity assessment within 7 days
- A fix or mitigation plan within 30 days for high-severity issues; longer for low-severity

We coordinate disclosure on a best-effort basis. For high-severity issues, the patched release will land before public details, with credit (unless you prefer otherwise).

## Scope

In scope:
- The `mail-mcp-daemon`, `mail-mcp-stdio`, and `mail-mcp-admin` binaries
- The macOS, Windows, and Linux tray apps
- The IPC protocol (UDS / named pipes / JSON-RPC framing)
- OAuth flows (PKCE, token storage, redirect URI handling)
- The MCP tool surface and per-account / per-category permission gate

Out of scope:
- Third-party dependencies (report upstream; we'll bump our pin once a patched version exists)
- Issues that require pre-existing local code execution as the same user (the daemon explicitly trusts the local user — that's the design)
- DoS via local resource exhaustion (the daemon is per-user, not multi-tenant)

## Security model summary

mail-mcp is a *local-only* MCP bridge. The daemon binds IPC to a per-user UDS or named pipe with `0600` permissions. OAuth tokens live in the OS keychain (macOS Keychain / Windows Credential Manager / libsecret on Linux). The daemon never accepts inbound network connections except the MCP HTTP endpoint on `127.0.0.1` (configurable, off by default for stdio mode).

Approval gates default to `ask` for all `send`, `trash`, and `draftify` categories. The user explicitly approves each action (or sets a per-account policy of `allow` / `block`). The MCP host (Claude Desktop, etc.) cannot bypass this gate.

For full threat-model details (assets, threats, defenses, residual risks), see [`docs/threat-model.md`](docs/threat-model.md).
