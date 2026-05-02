# mail-mcp — Claude Code conventions

This file captures decisions, gotchas, and conventions that aren't obvious from reading the code. The structure of the repo, the names of types, and the general architecture are best learned by reading specs in `docs/superpowers/specs/` and the source — don't duplicate them here.

## Workspace shape

- Rust workspace: `crates/mail-mcp-{core,daemon,stdio,admin}`. Pinned via `rust-toolchain.toml` to **1.88** + `rustfmt` + `clippy`. Do not bump without coordinating with CI.
- Tray apps live OUTSIDE the cargo workspace:
  - `tray-app/` — macOS (Swift, xcodegen, Sparkle)
  - `tray-app-win/` — Windows (.NET 8 + WinUI 3 + WinForms hybrid + WiX)
  - `tray-app-linux/` — Linux (C, GTK4, libadwaita, Meson) — design + plan only as of 2026-05-02
- License is **dual MIT / Apache-2.0** (see `LICENSE-MIT`, `LICENSE-APACHE`). Add the SPDX line `// SPDX-License-Identifier: MIT OR Apache-2.0` to new Rust source files.

## Branch & PR convention

- Branches: `vX.Y<letter>-<topic>` (e.g. `v0.1a-headless-backend`, `v0.1b-macos-tray`, `v0.1c-scaffolding`, `v0.1d-plan`). Letter is the milestone, not a sub-version — `v0.1a` is the headless backend, `v0.1b` is macOS tray, etc.
- PRs always merge into `main`. Squash merge.
- Specs go in `docs/superpowers/specs/YYYY-MM-DD-<topic>-design.md`.
- Plans go in `docs/superpowers/plans/YYYY-MM-DD-<topic>.md`.

## Conventions worth knowing

### IPC (subscribe race)

The IPC client must fire the `subscribe` RPC and wait for the ack BEFORE registering the local listener for that event type. Registering first races against the daemon and silently drops events that fire between `register` and `ack`. PR #6 fixed this in the macOS Swift client; v0.1c (Windows) and v0.1d (Linux) clients reproduce the fix.

### IPC framing

JSON-RPC 2.0, newline-delimited (one frame per `\n`). UTF-8 only. Both sides scan for `\n` to delimit frames. Do not buffer to size; scan to delimiter.

### OAuth client_id

Google + Microsoft client IDs are NOT checked in. They are baked into release binaries at build time (`MAIL_MCP_GOOGLE_CLIENT_ID`, `MAIL_MCP_MICROSOFT_CLIENT_ID` env vars), and end users / dev builds can override with the same env var at runtime. Never commit a real client ID.

### Localhost OAuth callback

The headless daemon listens on a random localhost port for the OAuth redirect (PR before merge of v0.1a). PKCE is non-negotiable; do not add a confidential-client flow.

### Daemon spawn

The tray apps locate `mail-mcp-daemon` next to their own binary first (so a bundle install just works), then fall back to PATH. Then they wait up to 5 seconds for `endpoint.json` (see `mail-mcp-core::paths`) to appear before connecting IPC. Do not extend the timeout — if the daemon hasn't written endpoint.json in 5s, something is wrong and we should surface an error.

### Approvals

Approvals are gated PER ACCOUNT, PER CATEGORY (`read`/`list`/`search`/`draft`/`send`/`labels`/`trash`). Policies: `allow`/`ask`/`block`/`draftify`. `Draftify` is only sensible for `send`. The tray UI must disable that radio for non-send rows.

## Gotchas

### Windows: WinUI 3 + WinForms = MC6000

`<UseWinUI>true</UseWinUI>` + `<UseWindowsForms>true</UseWindowsForms>` causes the WPF XAML compiler (`Microsoft.WinFX.targets`) to fire on `App.xaml` and fail with `MC6000: Project file must include the .NET Framework assembly 'PresentationCore, PresentationFramework'` because we don't reference WPF.

Fix: `tray-app-win/MailMCP/Directory.Build.targets` defines empty `<Target Name="MarkupCompilePass1"/>` etc. to override the SDK-imported WPF targets. **Don't move these stubs into the csproj body** — they get clobbered there because csproj-body targets are imported BEFORE the SDK targets. `Directory.Build.targets` is auto-imported AFTER the SDK targets, so overrides actually win.

### Linux daemon CI

`libdbus-1-dev` is required on Linux runners for the `keyring` crate to build (see commit `eeaf642`). The CI workflow installs it; if you spin up a new runner image, replicate that.

### Cross-compile checks

CI cross-compiles to `aarch64-pc-windows-msvc`, `aarch64-unknown-linux-gnu`, `riscv64gc-unknown-linux-gnu`, and `x86_64-apple-darwin` from the host runners. The `--all-features` flag pulls in keychain-tests which only build on macOS — CI uses default features instead (commit `dd6fc90`).

### .NET TreatWarningsAsErrors

`tray-app-win/MailMCP.csproj` has `TreatWarningsAsErrors=true`. Auto-generated XAML `InitializeComponent` partials trigger CS8002, CA1416, CS0067 — `<NoWarn>` covers those. Do NOT widen `<NoWarn>` to suppress real warnings.

### NuGet floating versions

`Microsoft.WindowsAppSDK`, `Microsoft.Windows.SDK.BuildTools`, and `CommunityToolkit.Mvvm` use floating `.*` patch versions (e.g. `1.5.*`). Reason: `TreatWarningsAsErrors` flags `NU1603` ("resolved a higher version than requested") when an exact build-number-suffixed version is requested but the published one is one digit different. Float to avoid spurious restore failures.

## Testing

- Rust: `cargo test --workspace`. Some integration tests use real keychains and run only on macOS — gated by feature flags.
- Windows tray: `dotnet test tray-app-win/MailMCP.Tests/MailMCP.Tests.csproj`. Mock named-pipe server (`MockIpcServer.cs`) covers IPC client tests.
- Linux tray: `meson test -C tray-app-linux/build`. CMocka-based.

## Style

- Default to no comments. Add a comment only when the WHY is non-obvious (a hidden constraint, a workaround for a specific bug, a counter-intuitive invariant). Don't explain what well-named code already does, and don't reference current tasks/PRs/callers — those rot.
- Don't add error handling, fallbacks, or validation for scenarios that can't happen. Trust internal-code guarantees. Validate at system boundaries (user input, external APIs).
- Don't introduce abstractions or feature flags beyond what the task requires. Three similar lines is better than a premature abstraction.
- Prefer editing existing files over creating new ones. Don't create README/.md/docs files unless the user explicitly asks.

## Where to look

- High-level architecture: `docs/superpowers/specs/2026-05-01-mail-mcp-design.md`
- Per-milestone designs: `docs/superpowers/specs/2026-05-0?-v0.1?-*.md`
- Implementation plans: `docs/superpowers/plans/`
- IPC protocol authoritative source: `crates/mail-mcp-core/src/ipc/`
- MCP tool definitions: `crates/mail-mcp-core/src/mcp/`
