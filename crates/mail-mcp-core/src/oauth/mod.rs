//! OAuth 2.0 + PKCE orchestration.
//!
//! For each provider we hold a `ProviderConfig` containing endpoint URLs and the
//! client_id used for the public-client PKCE flow (no client secret required, since we
//! cannot keep one secret on user machines).

use crate::error::Result;
use serde::{Deserialize, Serialize};

pub mod google;
pub mod loopback;

/// Static configuration for an OAuth provider that this daemon knows how to talk to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub auth_url: String,
    pub token_url: String,
    pub client_id: String,
    pub default_scopes: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn google_config_has_expected_endpoints() {
        let cfg = google::config("test-client-id");
        assert_eq!(cfg.auth_url, "https://accounts.google.com/o/oauth2/v2/auth");
        assert_eq!(cfg.token_url, "https://oauth2.googleapis.com/token");
        assert_eq!(cfg.client_id, "test-client-id");
        assert!(cfg.default_scopes.iter().any(|s| s.contains("gmail.modify")));
        assert!(cfg.default_scopes.iter().any(|s| s.contains("gmail.send")));
        assert!(cfg.default_scopes.iter().any(|s| s.contains("gmail.compose")));
    }

    #[test]
    fn pkce_pair_has_proper_lengths() {
        let pair = PkcePair::generate();
        // verifier should be 43–128 unreserved chars
        assert!(pair.verifier.len() >= 43 && pair.verifier.len() <= 128);
        // challenge is base64url(SHA-256(verifier)), 43 chars no padding
        assert_eq!(pair.challenge.len(), 43);
    }

    #[test]
    fn pkce_challenge_matches_verifier() {
        let pair = PkcePair::generate();
        let recomputed = PkcePair::compute_challenge(&pair.verifier);
        assert_eq!(recomputed, pair.challenge);
    }

    #[test]
    fn state_nonce_is_unique() {
        let a = state_nonce();
        let b = state_nonce();
        assert_ne!(a, b);
        assert!(a.len() >= 32);
    }
}

/// PKCE code verifier + the derived challenge. Per RFC 7636 §4.1.
#[derive(Debug, Clone)]
pub struct PkcePair {
    pub verifier: String,
    pub challenge: String,
}

impl PkcePair {
    pub fn generate() -> Self {
        use rand::distributions::Alphanumeric;
        use rand::Rng;
        let verifier: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(64)
            .map(char::from)
            .collect();
        let challenge = Self::compute_challenge(&verifier);
        Self { verifier, challenge }
    }

    pub fn compute_challenge(verifier: &str) -> String {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine;
        use sha2::{Digest, Sha256};
        let digest = Sha256::digest(verifier.as_bytes());
        URL_SAFE_NO_PAD.encode(digest)
    }
}

/// Generate a random state value used for CSRF protection on the OAuth callback.
pub fn state_nonce() -> String {
    use rand::distributions::Alphanumeric;
    use rand::Rng;
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(40)
        .map(char::from)
        .collect()
}

/// Tokens returned by the token endpoint after a successful code exchange or refresh.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub scope: Option<String>,
}

/// Convenience: try a refresh, returning the new tokens. Caller is responsible for storing
/// any new refresh token (some providers rotate; Google generally does not).
pub async fn refresh(
    client: &reqwest::Client,
    cfg: &ProviderConfig,
    refresh_token: &str,
) -> Result<OAuthTokens> {
    #[derive(Deserialize)]
    struct Resp {
        access_token: String,
        #[serde(default)]
        refresh_token: Option<String>,
        expires_in: i64,
        #[serde(default)]
        scope: Option<String>,
    }
    let params = [
        ("client_id", cfg.client_id.as_str()),
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
    ];
    let resp = client
        .post(&cfg.token_url)
        .form(&params)
        .send()
        .await?
        .error_for_status()?
        .json::<Resp>()
        .await?;
    let expires_at = chrono::Utc::now() + chrono::Duration::seconds(resp.expires_in - 30);
    Ok(OAuthTokens {
        access_token: resp.access_token,
        refresh_token: resp.refresh_token,
        expires_at,
        scope: resp.scope,
    })
}

pub async fn exchange_code(
    client: &reqwest::Client,
    cfg: &ProviderConfig,
    verifier: &str,
    code: &str,
    redirect_uri: &str,
) -> Result<OAuthTokens> {
    #[derive(Deserialize)]
    struct Resp {
        access_token: String,
        #[serde(default)]
        refresh_token: Option<String>,
        expires_in: i64,
        #[serde(default)]
        scope: Option<String>,
    }
    let params = [
        ("client_id", cfg.client_id.as_str()),
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("code_verifier", verifier),
    ];
    let resp = client
        .post(&cfg.token_url)
        .form(&params)
        .send()
        .await?
        .error_for_status()?
        .json::<Resp>()
        .await?;
    let expires_at = chrono::Utc::now() + chrono::Duration::seconds(resp.expires_in - 30);
    Ok(OAuthTokens {
        access_token: resp.access_token,
        refresh_token: resp.refresh_token,
        expires_at,
        scope: resp.scope,
    })
}

/// High-level orchestration: build the auth URL the caller should open in the browser,
/// then await the callback and exchange the code for tokens.
///
/// The caller is responsible for actually launching the browser (the daemon does this
/// via the IPC layer telling the tray app to call `NSWorkspace.open` etc.).
pub struct AuthChallenge {
    pub auth_url: String,
    pub redirect_uri: String,
    pub state: String,
    pub verifier: String,
    pub listener: loopback::LoopbackListener,
}

pub async fn begin_authorization(
    cfg: &ProviderConfig,
    extra_scopes: Option<&[String]>,
) -> Result<AuthChallenge> {
    let pair = PkcePair::generate();
    let state = state_nonce();
    let listener = loopback::LoopbackListener::bind(&state).await?;
    let redirect_uri = listener.redirect_uri();
    let scopes: Vec<&str> = extra_scopes
        .map(|s| s.iter().map(|s| s.as_str()).collect())
        .unwrap_or_else(|| cfg.default_scopes.iter().map(|s| s.as_str()).collect());
    let auth_url = format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method=S256&access_type=offline&prompt=consent",
        cfg.auth_url,
        urlencode(&cfg.client_id),
        urlencode(&redirect_uri),
        urlencode(&scopes.join(" ")),
        urlencode(&state),
        urlencode(&pair.challenge),
    );
    Ok(AuthChallenge {
        auth_url,
        redirect_uri,
        state,
        verifier: pair.verifier,
        listener,
    })
}

fn urlencode(s: &str) -> String {
    use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
    utf8_percent_encode(s, NON_ALPHANUMERIC).to_string()
}

pub async fn complete_authorization(
    client: &reqwest::Client,
    cfg: &ProviderConfig,
    challenge: AuthChallenge,
    timeout: std::time::Duration,
) -> Result<OAuthTokens> {
    let captured = challenge.listener.await_callback(timeout).await?;
    exchange_code(client, cfg, &challenge.verifier, &captured.code, &challenge.redirect_uri).await
}

#[cfg(test)]
mod exchange_tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn exchange_code_parses_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "at-1",
                "refresh_token": "rt-1",
                "expires_in": 3600,
                "scope": "https://www.googleapis.com/auth/gmail.modify",
                "token_type": "Bearer"
            })))
            .mount(&server)
            .await;
        let cfg = ProviderConfig {
            auth_url: "https://example".into(),
            token_url: format!("{}/token", server.uri()),
            client_id: "test-id".into(),
            default_scopes: vec![],
        };
        let client = reqwest::Client::new();
        let pair = PkcePair::generate();
        let tokens = exchange_code(
            &client,
            &cfg,
            &pair.verifier,
            "auth-code-1",
            "http://127.0.0.1:1234/callback",
        ).await.unwrap();
        assert_eq!(tokens.access_token, "at-1");
        assert_eq!(tokens.refresh_token.as_deref(), Some("rt-1"));
    }
}
