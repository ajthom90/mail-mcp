# mail-mcp Design Spec

**Date:** 2026-05-01
**Status:** Design — pending implementation plan

## Summary

`mail-mcp` is a locally-running, multi-account email service that exposes a Model Context Protocol (MCP) server so AI assistants (Claude Desktop, Claude Code, third-party MCP clients) can read, triage, and compose email through the user's existing mail accounts. It supports Gmail (OAuth), Microsoft 365 (OAuth), and generic IMAP/SMTP, runs as a long-lived per-user daemon with native tray apps on macOS, Windows, and Linux, and is built around an explicit, customizable permissions model with a "convert to draft" default for outgoing mail.

It is *not* a mail client UI. It is a programmable backend for mail with a small native shell for status and configuration. It is intended to run alongside the user's existing mail client.

## Goals

- **Local-first.** Nothing leaves the machine except calls to mail providers and OAuth endpoints. No telemetry by default.
- **Three providers.** Gmail (Google API + OAuth), Microsoft 365 (Graph API + OAuth), generic IMAP/SMTP.
- **Three platforms.** macOS, Windows, Linux. Native UI on each.
- **Maximum MCP client compatibility.** Both HTTP/SSE and stdio transports.
- **Multi-account.** Multiple accounts of any provider type connected simultaneously, addressed by `account_id` in MCP tool calls.
- **Customizable safety.** Per-account, per-category permissions. Sensible defaults set in a first-run wizard. "Convert to draft" as the default for outgoing mail so the AI can compose freely while the human controls the send.
- **Polished distribution.** Signed/notarized on macOS via the user's Apple Developer account. Unsigned on Windows and Linux for now.
- **Vertical-slice delivery.** Ship a working Mac+Gmail v0.1 first; add providers and platforms in subsequent releases.

## Non-Goals (v1)

- Mail client UI for human reading/triage. (Possible post-v1.)
- Persistent local mail index / offline cache. (Possible post-v1; v1 is on-demand fetch + small in-memory LRU.)
- Admin tool surface (create/delete labels, manage filters, manage signatures). (Possible post-v1.)
- Windows code signing. (Until a cert is acquired.)
- Auto-update on Windows and Linux. (Sparkle on macOS only in v1.0.)
- Telemetry, crash reporting, analytics.

## Architecture Overview

Three independent processes:

```
┌──────────────────────────────────────────────────────────────────┐
│  mail-mcp-daemon (Rust, long-running, autostart at login)        │
│                                                                  │
│  ┌──────────────────┐  ┌──────────────────┐  ┌─────────────┐     │
│  │ MCP Server       │  │ Provider Layer   │  │ IPC Server  │     │
│  │ (HTTP/SSE +      │  │ (Gmail, M365,    │  │ (UDS / NPipe│     │
│  │  socket bridge)  │  │  IMAP/SMTP)      │  │  + JSON-RPC)│     │
│  └────────▲─────────┘  └─────────▲────────┘  └──────▲──────┘     │
│           │                      │                  │            │
│           └────── account/permissions/secrets ──────┘            │
└───────────┬──────────────────────┬──────────────────┬────────────┘
            │                      │                  │
            │ HTTP/SSE             │ OS keychain      │ local socket
            │                      │                  │
   ┌────────▼─────────┐    ┌───────▼────────┐  ┌──────▼────────────┐
   │ MCP clients      │    │ OS keychain    │  │ Tray app          │
   │ (Claude Desktop, │    │ (refresh tokens│  │ (SwiftUI/WinUI/   │
   │  Claude Code,…)  │    │  per account)  │  │  GTK4)            │
   └────────▲─────────┘    └────────────────┘  └───────────────────┘
            │
            │ stdio
   ┌────────┴─────────┐
   │ mail-mcp-stdio   │
   │ (tiny shim, fwds │
   │  stdio↔socket)   │
   └──────────────────┘
```

### Components

- **`mail-mcp-daemon`** — Rust binary, long-running, single source of truth. Hosts the MCP server, manages accounts, holds in-memory access tokens, talks to mail providers, owns all persistent state.
- **Tray apps** — Native per platform (SwiftUI / WinUI 3 / GTK 4). View and controller over the daemon. Hold no state of their own.
- **`mail-mcp-stdio`** — Tiny Rust binary (~200 LOC) that proxies stdio MCP frames to/from the daemon's local socket, for MCP clients that only support stdio transport.
- **`mail-mcp-admin`** — Rust CLI for headless / power-user account management. Useful for users who want to skip the tray entirely (e.g., running on a NAS).

### Communication paths

- **MCP clients ↔ daemon:** HTTP/SSE on `127.0.0.1:<port>` for HTTP-capable clients, or stdio via the shim binary. Bearer-token gated. Endpoint info written to `endpoint.json` for client discovery.
- **Tray apps ↔ daemon:** JSON-RPC over a Unix domain socket (macOS/Linux) or named pipe (Windows). Method calls plus pub/sub for daemon → tray events. User-account-scoped permissions on the socket/pipe.
- **Daemon ↔ providers:** Gmail REST API, Microsoft Graph API, IMAP/SMTP via `async-imap` and `lettre`.
- **Daemon ↔ keychain:** Cross-platform via the `keyring` Rust crate. Refresh tokens and IMAP/SMTP passwords live here. Access tokens are kept only in memory.

### Key invariants

- The daemon never accepts non-loopback connections. IPC sockets and MCP HTTP both bind to local-only addresses with user-only permissions.
- Refresh tokens never touch disk in plaintext.
- The MCP HTTP bearer token is regenerated on every daemon start.
- Tray apps are dumb. All state lives in the daemon. UI updates flow from `state` push events.

## Core Daemon Design

### Module structure

| Module | Purpose |
|---|---|
| `mcp::server` | HTTP/SSE endpoint, tool registry, dispatch. Validates bearer token, calls `policy::check`, then provider |
| `policy` | Permissions matrix + approval flow. `check(account, op)` returns `Allowed` / `RequiresApproval` / `ConvertToDraft` / `Block` |
| `approvals` | Pending approval queue, broadcast channel for tray subscribers, timeout-driven auto-deny |
| `accounts` | Account metadata store (id, label, provider kind, settings, permissions) — persisted in SQLite |
| `secrets` | Wrapper around `keyring`. Refresh tokens stored under service `mail-mcp.<account_id>` |
| `oauth` | Localhost loopback listener + PKCE code exchange + token refresh. Endpoint configs for Google and Microsoft |
| `providers::{gmail, m365, imap}` | Each implements the `MailProvider` trait. Gmail uses Gmail REST API. M365 uses Graph API. IMAP uses `async-imap` for read/triage and `lettre` for SMTP |
| `cache` | In-memory LRU keyed by `(account_id, message_id)` → parsed message; bounded size, 5-min TTL |
| `ipc` | JSON-RPC server over UDS / named pipe; pub/sub for state events |
| `logging` | Structured `tracing` logs with sensitive-data redaction; daily-rotated files |

### Provider abstraction

A single trait abstracts over all three backends. Each method takes an explicit `account_id`-resolved provider instance.

```rust
#[async_trait]
trait MailProvider: Send + Sync {
    // Read
    async fn search(&self, q: &SearchQuery) -> Result<Vec<ThreadSummary>>;
    async fn get_thread(&self, id: &ThreadId) -> Result<Thread>;
    async fn get_message(&self, id: &MessageId) -> Result<Message>;
    async fn list_folders(&self) -> Result<Vec<Folder>>;
    async fn list_labels(&self) -> Result<Vec<Label>>;
    async fn list_drafts(&self) -> Result<Vec<DraftSummary>>;

    // Triage (reversible)
    async fn mark_read(&self, ids: &[MessageId], read: bool) -> Result<()>;
    async fn star(&self, ids: &[MessageId], starred: bool) -> Result<()>;
    async fn label(&self, ids: &[MessageId], label: &LabelId, on: bool) -> Result<()>;
    async fn move_to(&self, ids: &[MessageId], folder: &FolderId) -> Result<()>;
    async fn archive(&self, ids: &[MessageId]) -> Result<()>;

    // Triage (semi-reversible)
    async fn trash(&self, ids: &[MessageId]) -> Result<()>;
    async fn untrash(&self, ids: &[MessageId]) -> Result<()>;

    // Compose
    async fn create_draft(&self, d: &DraftInput) -> Result<DraftId>;
    async fn update_draft(&self, id: &DraftId, d: &DraftInput) -> Result<()>;
    async fn send_message(&self, m: &OutgoingMessage) -> Result<MessageId>;
    async fn send_draft(&self, id: &DraftId) -> Result<MessageId>;
}
```

### Unified vocabulary across providers

- **Folder** — Exclusive container, one per message. Maps to: IMAP mailbox, M365 folder, Gmail system labels (`INBOX`, `SENT`, `TRASH`).
- **Label** — Additive tag, zero-to-many per message. Maps to: Gmail user labels, M365 categories, IMAP keywords (`$Foo`).
- Each MCP tool description documents per-provider semantics so the AI knows what `move_to` does on Gmail (re-label) vs IMAP (mailbox copy/move).

### Tool surface (v1)

Read + triage + compose. Admin (label creation, filter management, signatures) is post-v1.

| Tool | Op category |
|---|---|
| `search`, `get_thread`, `get_message`, `list_folders`, `list_labels`, `list_drafts`, `list_accounts` | `read` |
| `mark_read`, `star`, `label`, `unlabel`, `move_to`, `archive` | `modify` |
| `trash`, `untrash` | `trash` |
| `create_draft`, `update_draft` | `draft` |
| `send_message`, `send_draft` (also covers reply/forward via params) | `send` |

### Tool dispatch flow

1. MCP client calls a tool with `account_id` + payload.
2. `mcp::server` resolves the account, validates bearer token.
3. `policy::check(account, op_category)` returns one of:
   - `Allowed` → execute.
   - `RequiresApproval` → enqueue in `approvals`, broadcast to tray, wait for user decision (5-min timeout → auto-reject).
   - `ConvertToDraft` (only for `send` category) → rewrite to `create_draft`, return a response telling the AI the operation was downgraded.
   - `Block` → return MCP error.
4. On `Allowed`, the relevant `providers::*` method runs, and the result is returned through MCP.

## Tray App Design

Three independent native apps, each ~1500-3000 LOC, all speaking the same JSON-RPC protocol to the daemon. They live in the repo as separate sub-projects with their own build pipelines.

### Shared responsibilities

1. Tray/menu-bar icon with status indication (connected accounts count, "syncing", error states).
2. Drop-down menu (status summary, "Open Settings", "Pause MCP", "Quit").
3. Native settings/preferences window.
4. Native approval dialogs.
5. Daemon process management on first launch (spawn the daemon if not already running).
6. Subscribe to daemon state events and reflect them in UI.

### Per-platform stack

| Concern | macOS | Windows | Linux |
|---|---|---|---|
| Language / framework | Swift / SwiftUI + AppKit's `NSStatusItem` | C# / WinUI 3 (Windows App SDK) + `NotifyIcon` | C / GTK 4 + `AyatanaAppIndicator` (libadwaita for settings) |
| Settings pane | SwiftUI `Settings` scene (sidebar layout) | WinUI 3 `NavigationView`-based settings window | GTK 4 `AdwPreferencesWindow` |
| Approval dialog | `NSAlert` modal with sender/subject/preview | WinUI `ContentDialog` | `AdwMessageDialog` |
| Browser launch (OAuth) | `NSWorkspace.open` | `Launcher.LaunchUriAsync` | `gtk_show_uri` |
| Autostart registration | `SMAppService` (LaunchAgents) | Windows `Startup` registry / Login Items API | systemd user unit + autostart `.desktop` fallback |

### Settings pane layout (consistent across platforms)

```
┌─ Settings ──────────────────────────────────────────────┐
│ [Accounts] [Permissions] [General] [About]              │
└─────────────────────────────────────────────────────────┘
```

- **Accounts** — list of connected accounts with reconnect / remove actions, plus "Add Gmail / M365 / IMAP" buttons.
- **Permissions** — per-account matrix (categories × policies). See "Permissions UX" below.
- **General** — autostart toggle, MCP endpoint info, "Pause MCP" toggle, log location.
- **About** — daemon version, uptime, status, last 50 redacted log lines, copy-to-clipboard for support.

### Daemon spawning

Tray apps coordinate with the daemon at launch:
1. Try to connect to the IPC socket.
2. If missing, spawn the daemon binary (which lives next to or under the tray app bundle).
3. The daemon writes a PID file (`daemon.pid`); flock prevents two tray instances from double-spawning.

If autostart is enabled, the daemon is already running and the tray simply connects.

### Headless mode

Power users can run the daemon standalone (no tray app) and manage accounts via the `mail-mcp-admin` CLI. Useful for servers / NAS deployments.

## First-Run Wizard, OAuth, and Permissions UX

### First-run wizard

Runs from the tray app on first launch (when daemon reports no accounts and no `onboarding_complete` flag). Native modal/window per platform, identical step structure:

1. **Welcome** — explanation of what mail-mcp does and how it stays local.
2. **Add first account** — pick Gmail / M365 / IMAP (or skip).
3. **OAuth or IMAP form** — opens system browser for OAuth providers; native form for IMAP.
4. **Set permissions** — per-category dropdowns with explainer text. Defaults:
   - Read & search: **Allow always**
   - Modify (label, archive, mark read): **Allow always**
   - Move to trash: **Always confirm**
   - Create drafts: **Allow always**
   - Send / reply / forward: **Convert to draft**
5. **Autostart** — Yes (recommended) / No. User explicitly chooses; no silent registration.
6. **Configure your AI client** — show MCP server config snippet for stdio shim, plus optional HTTP endpoint info; "Copy to clipboard" and "Open Claude Desktop config" actions.

### OAuth flow

```
1. User picks provider in wizard (or "Add account" in settings)
2. Tray sends `accounts.add_oauth { provider }` to daemon
3. Daemon:
   a. Generates PKCE code_verifier + state nonce
   b. Binds a localhost listener on a random ephemeral port (loopback only)
   c. Constructs authorization URL with redirect_uri=http://127.0.0.1:<port>/callback
   d. Returns the URL to the tray
4. Tray opens it in the system default browser
5. Provider redirects to localhost callback after auth
6. Daemon's listener catches /callback?code=...&state=...
   - Verifies state matches (CSRF)
   - Closes listener
   - Exchanges code for access + refresh tokens via PKCE
   - Stores refresh token in OS keychain
   - Stores account row in SQLite (id, label, provider, email, scopes, created_at)
7. Daemon notifies tray "account.added" via IPC pubsub
8. Tray closes "Waiting for browser..." overlay, advances wizard
```

Embedded webviews are not used for OAuth (Google blocks them; Microsoft is hostile to them).

### OAuth scope sets

| Provider | Scopes |
|---|---|
| Gmail | `gmail.modify`, `gmail.compose`, `gmail.send`, plus `email`, `profile` |
| Microsoft 365 | `Mail.ReadWrite`, `Mail.Send`, `User.Read` |

Admin scopes are deliberately not requested in v1.

### IMAP/SMTP flow

- Native form: server hostname, port, username, password (or app password), TLS settings.
- Test connection before saving.
- Auto-detect server config from email domain via Mozilla's autoconfig database when possible.
- Password stored in OS keychain under service `mail-mcp.<account_id>`.

### Permissions semantics

| Policy | Behavior |
|---|---|
| **Allow always** | Tool call executes immediately; no UI |
| **Always confirm** | Tool call blocks; native dialog shows action details; user clicks Approve / Reject; decision applied to that single call |
| **Per-session trust** | First call in a daemon-process-lifetime is treated as Always confirm; if approved, subsequent calls in the same session execute immediately. Resets on daemon restart |
| **Convert to draft** | (Send category only.) Silently rewrites `send_message` / `send_draft` calls to `create_draft`. AI receives a `draft_created` response with a `note` explaining the downgrade so it can tell its user |
| **Block** | Returns MCP error: `"This action is not permitted by user policy. Open mail-mcp Settings to allow."` |

Permissions are settable per account. The settings pane offers a "trust this account's MCP calls for the current session" quick toggle that effectively switches all policies for the account to Allow until daemon restart.

## Storage, Secrets, Observability

### On-disk layout

| Path (macOS) | Path (Windows) | Path (Linux) | Contents |
|---|---|---|---|
| `~/Library/Application Support/mail-mcp/` | `%LOCALAPPDATA%\mail-mcp\` | `${XDG_DATA_HOME:-~/.local/share}/mail-mcp/` | Application data root |
| └ `state.db` | └ `state.db` | └ `state.db` | SQLite — accounts, permissions, app state |
| └ `endpoint.json` | └ `endpoint.json` | └ `endpoint.json` | MCP endpoint URL + bearer token (mode 0600) |
| `~/Library/Logs/mail-mcp/` | `%LOCALAPPDATA%\mail-mcp\logs\` | `${XDG_STATE_HOME:-~/.local/state}/mail-mcp/logs/` | Daily-rotated daemon logs |
| `~/Library/Caches/mail-mcp/` | `%LOCALAPPDATA%\mail-mcp\cache\` | `${XDG_CACHE_HOME:-~/.cache}/mail-mcp/` | Future bodies/attachments cache (empty in v1) |

IPC socket / named pipe location:
- macOS: `${TMPDIR}mail-mcp-<uid>/ipc.sock` (`TMPDIR` is per-user on macOS), perms 0700 on dir, 0600 on socket
- Linux: `${XDG_RUNTIME_DIR:-/run/user/<uid>}/mail-mcp/ipc.sock`, perms 0700 on dir, 0600 on socket; if neither exists, fall back to `${TMPDIR:-/tmp}/mail-mcp-<uid>/ipc.sock`
- Windows: `\\.\pipe\mail-mcp-<sid>` (per-session), default DACL restricting to current user

The daemon unlinks any stale socket on startup before binding.

### `state.db` schema

```sql
CREATE TABLE accounts (
  id              TEXT PRIMARY KEY,         -- ULID
  label           TEXT NOT NULL,
  provider        TEXT NOT NULL,            -- 'gmail' | 'm365' | 'imap'
  email           TEXT NOT NULL,
  config_json     TEXT NOT NULL,            -- provider-specific (IMAP server, port, etc.)
  scopes          TEXT,
  created_at      INTEGER NOT NULL,
  last_validated  INTEGER
);

CREATE TABLE permissions (
  account_id  TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  category    TEXT NOT NULL,                -- 'read'|'modify'|'trash'|'draft'|'send'
  policy      TEXT NOT NULL,                -- 'allow'|'confirm'|'session'|'draftify'|'block'
  PRIMARY KEY (account_id, category)
);

CREATE TABLE app_state (
  key   TEXT PRIMARY KEY,
  value TEXT
);
-- e.g., onboarding_complete, autostart_enabled, schema_version
```

Migrations are embedded in the binary and applied on daemon startup.

### Secrets

- OAuth refresh tokens, IMAP/SMTP passwords — OS keychain via the `keyring` crate.
- Service name format: `mail-mcp.<account_id>`; keychain account names: `refresh_token`, `imap_password`, `smtp_password`.
- Access tokens kept only in memory; refreshed on demand from the refresh token.
- MCP HTTP bearer token in `endpoint.json` is regenerated on every daemon start.

### Observability

- Structured logs via `tracing`, JSON to file plus human format to stderr if interactive. Daily rotation, 14-day retention.
- A custom redaction layer scrubs email bodies, recipient addresses, full message IDs, OAuth codes/tokens before formatting. Operations are logged as e.g. `"send on account_id ABCD…"` rather than recipient/subject details.
- Tray "About" pane shows daemon version, uptime, account statuses, last 50 redacted log lines, copy-to-clipboard for support.
- No telemetry. No outbound calls except to providers and OAuth endpoints.

### Failure modes the daemon handles gracefully

- **Token refresh fails** (e.g., user revoked access in Google) → mark account `needs_reauth`, surface to tray with a Reconnect prompt, return MCP errors with actionable text.
- **IMAP server unreachable** → exponential backoff, surface status, MCP calls return "transient error".
- **Provider rate-limited** → respect `Retry-After`, retry, MCP call returns either after retry or as transient error.
- **Keychain access denied** → can't make calls; surface as `needs_reauth`; do not crash.

## Repo Layout, Build, Distribution

### Repo layout

```
mail-mcp/
├─ Cargo.toml                          # workspace root
├─ crates/
│  ├─ mail-mcp-core/                   # library: providers, policy, accounts, secrets, oauth, cache
│  ├─ mail-mcp-daemon/                 # binary: long-running daemon (uses core)
│  ├─ mail-mcp-stdio/                  # binary: stdio↔socket shim
│  └─ mail-mcp-admin/                  # binary: CLI for headless / power-user mgmt
├─ apps/
│  ├─ mac/                             # SwiftUI app (Xcode project)
│  ├─ win/                             # WinUI 3 app (.NET 8 / C#)
│  └─ linux/                           # GTK 4 app (Meson, C with libadwaita)
├─ proto/
│  └─ ipc-schema.json                  # JSON-RPC method definitions, source of truth
├─ scripts/
│  ├─ build-mac.sh                     # builds daemon + signs/notarizes app bundle
│  ├─ build-win.ps1                    # builds daemon + packages MSIX
│  └─ build-linux.sh                   # builds daemon + AppImage / .deb / .rpm
├─ docs/
│  ├─ superpowers/specs/               # design docs
│  ├─ user/                            # user-facing docs
│  └─ dev/                             # contributor docs
├─ .github/workflows/
│  ├─ ci.yml
│  └─ release.yml
└─ README.md
```

### Crate dependencies

```
mail-mcp-core ←─ mail-mcp-daemon
              ←─ mail-mcp-admin
mail-mcp-stdio (no dep on core; just a thin proxy)
```

### Code-sharing across languages

- `proto/ipc-schema.json` is the source of truth for the IPC API.
- Build-time codegen produces:
  - Rust types in `mail-mcp-core::ipc::types`
  - Swift types via a small Swift script
  - C# types via T4 templates or a custom MSBuild task
  - C structs/parsers for the GTK app
- Keeps the protocol DRY across four languages without anyone hand-writing structs that drift.

### Build / signing / packaging

| Platform | Output | Signing |
|---|---|---|
| macOS | `MailMCP.app` (universal binary, x86_64+arm64); daemon embedded in `Contents/Resources/`; tray app is the entry point | Developer ID Application cert + notarization via `notarytool` |
| Windows | `MailMCP.msix`; self-contained `.exe` daemon in package | Unsigned (SmartScreen warning) |
| Linux | `.AppImage` (universal); also `.deb` and `.rpm` | Unsigned |

`mail-mcp-stdio` and `mail-mcp-admin` symlinked to a known PATH location (`/usr/local/bin` on macOS/Linux; an installer-managed dir on Windows) so MCP clients can find the shim.

### Distribution channels

- **macOS:** GitHub Releases (DMG); Homebrew Cask (`brew install --cask mail-mcp`)
- **Windows:** GitHub Releases (MSIX + portable zip); winget manifest later
- **Linux:** GitHub Releases (AppImage, .deb, .rpm); Flatpak later

### Auto-update

- macOS: Sparkle 2 (v1.0)
- Windows: deferred (re-install for now)
- Linux: deferred (distros prefer not to self-update)

### CI

- GitHub Actions matrix: macOS (Apple silicon) + Windows + Ubuntu.
- Per-job: `cargo test` for the workspace + native build for that platform's tray app + smoke tests against fake IMAP/SMTP/OAuth servers.
- Release workflow on tag push: build all 3 artifacts, sign Mac, upload to GitHub Releases.

## Roadmap

| Milestone | Scope |
|---|---|
| **v0.1 — Mac + Gmail vertical slice** | Rust core + daemon + stdio shim + admin CLI; Gmail provider only; macOS tray with full settings + first-run wizard; OAuth + keychain; permissions including "Convert to draft"; live MCP via HTTP and stdio |
| **v0.2 — Microsoft 365** | Add M365 provider (Graph API + OAuth); both providers usable on Mac |
| **v0.3 — IMAP/SMTP** | Add generic IMAP/SMTP provider with autoconfig; password storage in keychain; account form UI in Mac tray |
| **v0.4 — Windows tray** | WinUI 3 app, settings, wizard, autostart, MSIX packaging |
| **v0.5 — Linux tray** | GTK 4 app, settings, wizard, systemd autostart, AppImage/deb/rpm |
| **v1.0** | Polish, docs, Homebrew Cask, Sparkle auto-update on Mac, bug bash |
| **post-v1** | Admin tool surface; persistent local index; basic mail UI in tray; opt-in telemetry; crash reporting |

## Open Questions / Future Work

- **IMAP IDLE / push.** Not needed for the v1 cache model (on-demand fetch). May be valuable when persistent local index lands post-v1.
- **Multi-user scenarios.** v1 is single-user-per-machine. Per-user-data layout already supports multi-user without changes.
- **Reply / forward semantics on IMAP.** IMAP itself has no thread concept; thread reconstruction is via headers (`In-Reply-To`, `References`). The IMAP provider will reconstruct threads heuristically; documented in the tool description.
- **Send-as / aliases.** Gmail and M365 support send-as identities; the IMAP/SMTP path uses whatever the user configured. Surfacing alias selection in the MCP `send` tool is a v1 nice-to-have.
- **Attachments in v1.** Read: yes (return as base64 or as a fetchable URL referencing a daemon-local resource). Send: yes (MCP tool accepts attachment payloads). Detail to be finalized in the implementation plan.
- **Draft deletion / cleanup.** When `Convert to draft` is the policy, the user accumulates drafts. We should consider a UI in the tray for batch-reviewing AI-generated drafts. Post-v1.
