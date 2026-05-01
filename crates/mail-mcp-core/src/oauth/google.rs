//! Google OAuth 2.0 endpoints + Gmail scope set.

use super::ProviderConfig;

/// Build a ProviderConfig for Google with the given OAuth client_id.
///
/// The client_id is configured per-installation (set via env / config file at daemon
/// start, baked into the build via build.rs in releases). Scopes match v0.1a:
/// gmail.modify, gmail.compose, gmail.send + email/profile for display.
pub fn config(client_id: impl Into<String>) -> ProviderConfig {
    ProviderConfig {
        auth_url: "https://accounts.google.com/o/oauth2/v2/auth".into(),
        token_url: "https://oauth2.googleapis.com/token".into(),
        client_id: client_id.into(),
        default_scopes: vec![
            "https://www.googleapis.com/auth/gmail.modify".into(),
            "https://www.googleapis.com/auth/gmail.compose".into(),
            "https://www.googleapis.com/auth/gmail.send".into(),
            "openid".into(),
            "email".into(),
            "profile".into(),
        ],
    }
}
