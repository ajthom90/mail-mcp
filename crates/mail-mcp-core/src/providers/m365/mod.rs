//! Microsoft 365 / Outlook provider via Microsoft Graph API.
//!
//! Auth flows through the shared `mail-mcp-core::oauth` (PKCE + loopback). The
//! HTTP client is `mail-mcp-core::providers::gmail::AuthClient` — same shape,
//! same token-refresh logic, different `ProviderConfig` endpoints.
//!
//! Tasks 3-8 of the v0.2 plan fill in each submodule. This skeleton compiles
//! against an empty trait impl so the workspace stays green during the
//! incremental build-out.

mod compose;
mod folders;
mod messages;
mod parse;
mod provider;
mod triage;

pub use provider::M365Provider;
