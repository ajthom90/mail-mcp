# mail-mcp threat model

This document describes the threats mail-mcp defends against, the threats it explicitly does NOT defend against, and the design choices that follow from each. It backs the public security policy in [SECURITY.md](../SECURITY.md).

Last reviewed: 2026-05-03 (v0.1c). Re-reviewed at every minor version.

---

## Trust boundary

mail-mcp runs as a per-user, local-only process. Its trust boundary draws around the OS user account: anything the user can do, the daemon can do, and we treat the user's process / disk / network access as inside the boundary.

What's outside the boundary:

- Other OS users on the same machine.
- The MCP host process (Claude Desktop, Claude Code, etc.). The MCP host is treated as semi-trusted: we run the tools it asks for, but we gate sensitive operations behind per-account / per-category approval.
- Network attackers who can intercept or replay traffic to providers.
- Provider-side attackers (e.g. a compromised Gmail account that injects malicious content into messages).

The user themselves is in-scope as the authenticated principal. We do not defend against an attacker who has already obtained code execution as the same OS user — at that point our keychain access, our config files, and our IPC socket are all reachable.

---

## Assets

| Asset | Location | Sensitivity |
|---|---|---|
| OAuth refresh tokens | OS keychain (Keychain / Credential Manager / libsecret) | high |
| Account list, permissions, settings | `${XDG_CONFIG_HOME}/mail-mcp/`, `state.db` | medium |
| Cached message metadata | `MessageCache` (in-memory, bounded LRU) | low–medium |
| MCP endpoint bearer token | `${XDG_RUNTIME_DIR}/mail-mcp/endpoint.json` (0600) | high |
| IPC socket / named pipe | `${XDG_RUNTIME_DIR}/mail-mcp/ipc.sock` (0600) / `\\.\pipe\mail-mcp-{user}` | high |
| Pending approval queue | in-memory only | low |
| Audit log (v0.5+) | `${XDG_DATA_HOME}/mail-mcp/rules.log` | low |
| Daemon logs | `${XDG_STATE_HOME}/mail-mcp/logs/` | medium (may contain redacted message metadata) |

---

## Threat: token theft via filesystem

**Attacker:** local cross-user attacker with read access to the user's home directory.

**Defense:** OAuth tokens are stored in the OS keychain, not in cleartext on disk. The keychain enforces per-user access control:

- macOS Keychain — entries are owned by the user; access requires interactive authorization or successful login session.
- Windows Credential Manager — per-user credential vault.
- Linux libsecret — defaults to a user-session-scoped collection unlocked at login.

Config files (`accounts.toml`, `permissions.toml`, etc.) reference accounts by ID only and contain no token material. `state.db` is sqlite with read-restricted permissions but contains only message-cache metadata, not credentials.

**Residual risk:** if the attacker has the same OS user (e.g. malware running as the user, or a compromised shell), they can request keychain entries the same way the daemon does. We treat that as out-of-scope per the trust boundary above.

---

## Threat: token theft via memory dump

**Attacker:** local OS-user attacker who can read the daemon's process memory.

**Defense:** none meaningful — process memory is in scope of the same OS user, which is inside the trust boundary. We do not pin secrets, zero them after use, or use protected memory primitives. Building those defenses without a hardened OS substrate (TPM, enclave) is theater.

**Residual risk:** accepted.

---

## Threat: IPC eavesdropping or hijacking

**Attacker:** local cross-user attacker on the same machine.

**Defense:**

- **Unix:** the IPC socket is created with `0600` mode under `${XDG_RUNTIME_DIR}/mail-mcp/`, which is itself per-user (`/run/user/<uid>/`). Other OS users cannot connect.
- **Windows:** the named pipe is created with a SECURITY_DESCRIPTOR that grants access only to the current user SID. (Implementation in `crates/mail-mcp-core/src/ipc/transport_windows.rs`.)
- The MCP HTTP endpoint binds `127.0.0.1` only and protects each request with a freshly-generated bearer token written to `endpoint.json` (0600). The token is unguessable (cryptographically random 256-bit) and rotates each daemon start.

**Residual risk:** a same-user process can still read `endpoint.json` and connect — but per the trust boundary that's accepted.

---

## Threat: model-driven destructive actions

**Attacker:** the MCP host or a compromised model session asks the daemon to send mail / delete events / wipe a label without the user's intent.

**Defense:** every MCP tool that produces a side effect is gated by the per-account, per-category permission map. Defaults (as of v0.2):

| Category | Default | Rationale |
|---|---|---|
| `read` | `allow` | Reading is reversible by re-fetching; the model needs context. |
| `modify` | `confirm` | Label changes (including INBOX moves) prompt by default. |
| `draft` | `allow` | Drafts are private to the account, not user-visible until sent. |
| `trash` | `confirm` | Reversible from Trash for 30 days but explicit confirmation prevents surprises. |
| `send` | `confirm` | Irreversible. Default-prompts so a model cannot send unsupervised. |
| `calendar.write` (v0.4+) | `confirm` | Visible to attendees; default-prompts. |
| `calendar.delete` (v0.4+) | `confirm` | |

The user can promote categories to `allow` per account (e.g. an automation account may grant `send=allow`), or demote to `block` to disable entirely.

The `draftify` policy (only meaningful on `send`) converts a send into a draft creation and emits a notification. This lets a model "compose and ask" without ever crossing the send threshold.

Approvals time out after 5 minutes (default), at which point the action is rejected automatically. The user sees pending approvals in the tray menu.

**Residual risk:** a user who repeatedly clicks "Approve" without reading prompts loses the safety. UX should keep prompt content terse and unambiguous (recipient + subject for sends, message count for trash, event title + start for calendar).

---

## Threat: MCP host compromise

**Attacker:** the local MCP host process is itself compromised and tries to abuse the daemon.

**Defense:** the daemon treats the MCP host as semi-trusted — it must hold the bearer token to connect, and tools are gated as above. Sensitive admin operations (`accounts.add_oauth`, `queries.add`, `rules.add`) are NOT exposed via the MCP tool surface; they require the local-only privileged IPC channel which the tray and admin CLI use, and which has no authentication beyond the per-user socket permissions.

So a compromised MCP host can:
- Call any allow-listed MCP tool with the user's per-account permission gate enforced.
- NOT add/remove accounts, NOT install rules, NOT change permissions.

**Residual risk:** the MCP host can chain enough `allow`-gated tools to do real harm if the user has set permissive defaults. Mitigation: the default policies are conservative.

---

## Threat: rule-driven autonomous actions (v0.5+)

**Attacker:** a model session installs a malicious automation rule that sends data to an attacker-controlled address whenever new mail arrives.

**Defense:** rule installation requires the local-only privileged IPC channel — the MCP host has no API to install rules. Rules can only be created by the user via the tray UI or admin CLI. Once installed, rule actions go through the same approval gate as direct tool calls, so even a compromised rule cannot bypass `send=confirm`.

**Residual risk:** if the user has set `send=allow` on an account AND a malicious rule is installed (which requires user-level access to the config file), exfiltration is possible. Both prerequisites are user-action-gated.

---

## Threat: provider-side content injection

**Attacker:** a malicious sender sends a message designed to manipulate the model into taking unauthorized actions ("system prompt injection").

**Defense:** the daemon does not interpret message content. Tools return mail as opaque text blobs to the host; the model is responsible for treating them as untrusted. We do recommend (in user docs) that hosts:
- Use approval gates for any side effect.
- Apply the model's own prompt-injection defenses on text returned from `read_message` / `search`.

**Residual risk:** prompt injection is a research-level open problem; we cannot fully defend against it in the daemon. Approval gates provide the last-line backstop because the user sees the actual recipient / content of any send before it goes out.

---

## Threat: OAuth redirect manipulation

**Attacker:** a local attacker hijacks the OAuth redirect to capture an authorization code.

**Defense:** the daemon implements OAuth 2.0 PKCE (RFC 7636). The redirect URI is `http://127.0.0.1:<random-port>/callback` bound only on loopback. PKCE means the captured code cannot be exchanged for tokens without the per-flow code verifier, which lives only in the daemon's memory. Even a same-user attacker capturing the code from network traffic cannot redeem it without first compromising the daemon process.

**Residual risk:** a same-user attacker that compromises the daemon can complete the OAuth flow as the user. Already accepted under the trust boundary.

---

## Threat: dependency-supply-chain compromise

**Attacker:** a malicious release of a transitive dependency.

**Defense:**
- `Cargo.lock` is committed; reproducible builds with `cargo build --release --locked`.
- `cargo audit` runs in CI and fails on known vulnerabilities. Exception list (if any) lives in `audit.toml`.
- `cargo deny` enforces an allow-list of license types and bans known-bad crates. (Planned for v1.0.)
- We pin major versions for all direct dependencies; minor / patch float.

**Residual risk:** a previously-clean dependency goes bad between audits. Detection is reactive (we update when an advisory lands).

---

## Threat: physical access / lost device

**Attacker:** physical access to the user's unlocked machine.

**Defense:** none beyond OS-level. The daemon doesn't add a layer of authentication on top of the user's session.

**Residual risk:** accepted. The expectation is that users lock their machines.

---

## Out of scope (explicitly)

- **Multi-user / multi-tenant operation.** mail-mcp is per-user-per-machine. There is no "admin" user separate from the daemon-running user.
- **Sandbox escape** (e.g. a malicious provider response that exploits json-glib). We rely on memory-safe Rust + json-glib's documented soundness. No additional sandboxing.
- **Side-channel resistance.** No defense against power / timing / cache-side-channel attacks.
- **Hardware token requirement for sends.** A user can choose to install a YubiKey-backed PGP setup; we don't gate sends on it.
- **End-to-end encryption** (PGP, S/MIME). Out of scope for v1.0; possibly v1.x.

---

## Reporting

See [SECURITY.md](../SECURITY.md) for the disclosure process.
