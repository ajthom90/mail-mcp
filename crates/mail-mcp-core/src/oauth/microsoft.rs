//! Microsoft Graph OAuth 2.0 endpoints + Mail scope set.

use super::ProviderConfig;

/// Build a `ProviderConfig` for Microsoft Graph with the given OAuth client_id.
///
/// `/common` (vs `/{tenant}`) accepts both consumer accounts (outlook.com,
/// hotmail.com, live.com) and AAD work/school accounts. The user picks during
/// the consent prompt.
///
/// `offline_access` is required for the `refresh_token`. Microsoft rotates
/// refresh tokens on every refresh — the `AuthClient`'s
/// `RefreshRotationCallback` (issue #2 fix) handles persistence transparently
/// on every refresh, where Google rarely hits that path.
pub fn config(client_id: impl Into<String>) -> ProviderConfig {
    ProviderConfig {
        auth_url: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize".into(),
        token_url: "https://login.microsoftonline.com/common/oauth2/v2.0/token".into(),
        client_id: client_id.into(),
        default_scopes: vec![
            "offline_access".into(),
            "https://graph.microsoft.com/Mail.ReadWrite".into(),
            "https://graph.microsoft.com/Mail.Send".into(),
            "https://graph.microsoft.com/User.Read".into(),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn microsoft_config_has_expected_endpoints() {
        let cfg = config("test-client-id");
        assert_eq!(
            cfg.auth_url,
            "https://login.microsoftonline.com/common/oauth2/v2.0/authorize"
        );
        assert_eq!(
            cfg.token_url,
            "https://login.microsoftonline.com/common/oauth2/v2.0/token"
        );
        assert_eq!(cfg.client_id, "test-client-id");
        assert!(cfg.default_scopes.iter().any(|s| s == "offline_access"));
        assert!(cfg
            .default_scopes
            .iter()
            .any(|s| s.contains("Mail.ReadWrite")));
        assert!(cfg.default_scopes.iter().any(|s| s.contains("Mail.Send")));
        assert!(cfg.default_scopes.iter().any(|s| s.contains("User.Read")));
    }
}
