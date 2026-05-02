use crate::error::{Error, Result};
use crate::oauth::{self, OAuthTokens, ProviderConfig};
use std::sync::Arc;
use tokio::sync::Mutex;

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn cfg(token_url: String) -> ProviderConfig {
        ProviderConfig {
            auth_url: "https://example/auth".into(),
            token_url,
            client_id: "test-client-id".into(),
            default_scopes: vec![],
        }
    }

    fn fresh_tokens() -> OAuthTokens {
        OAuthTokens {
            access_token: "AT-1".into(),
            refresh_token: Some("RT-1".into()),
            expires_at: chrono::Utc::now() + chrono::Duration::seconds(600),
            scope: None,
        }
    }

    fn expired_tokens() -> OAuthTokens {
        OAuthTokens {
            access_token: "AT-OLD".into(),
            refresh_token: Some("RT-1".into()),
            expires_at: chrono::Utc::now() - chrono::Duration::seconds(60),
            scope: None,
        }
    }

    #[tokio::test]
    async fn uses_existing_token_if_unexpired() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/probe"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
            .mount(&server)
            .await;

        let cfg = cfg(format!("{}/token", server.uri()));
        let client = AuthClient::new(reqwest::Client::new(), cfg, fresh_tokens());

        let resp: serde_json::Value = client
            .get(&format!("{}/probe", server.uri()))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(resp, serde_json::json!({"ok": true}));
        assert_eq!(client.access_token().await, "AT-1");
    }

    #[tokio::test]
    async fn refreshes_when_token_expired() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "AT-NEW",
                "expires_in": 3600,
                "token_type": "Bearer"
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/probe"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
            .mount(&server)
            .await;

        let cfg = cfg(format!("{}/token", server.uri()));
        let client = AuthClient::new(reqwest::Client::new(), cfg, expired_tokens());

        let resp = client
            .get(&format!("{}/probe", server.uri()))
            .await
            .unwrap();
        assert!(resp.status().is_success());
        assert_eq!(client.access_token().await, "AT-NEW");
    }

    #[tokio::test]
    async fn rotation_callback_fires_when_refresh_token_changes() {
        // Issue #2: when Google rotates the refresh token during a refresh,
        // the new value must be surfaced to the daemon so it can be persisted
        // to the keychain. Otherwise on next startup we'd reload the stale
        // token from disk and get an irrecoverable auth failure.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "AT-NEW",
                "refresh_token": "RT-2",
                "expires_in": 3600,
                "token_type": "Bearer"
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/probe"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
            .mount(&server)
            .await;

        let captured: Arc<std::sync::Mutex<Option<String>>> = Arc::new(std::sync::Mutex::new(None));
        let captured_for_cb = captured.clone();
        let cb: RefreshRotationCallback = Arc::new(move |new_rt: &str| {
            *captured_for_cb.lock().unwrap() = Some(new_rt.to_string());
        });

        let cfg = cfg(format!("{}/token", server.uri()));
        let client = AuthClient::with_rotation_callback(
            reqwest::Client::new(),
            cfg,
            expired_tokens(),
            Some(cb),
        );

        let _ = client
            .get(&format!("{}/probe", server.uri()))
            .await
            .unwrap();
        // The rotation callback fired with the new refresh token.
        assert_eq!(
            captured.lock().unwrap().as_deref(),
            Some("RT-2"),
            "rotation callback should have fired with the new refresh token"
        );
    }

    #[tokio::test]
    async fn rotation_callback_does_not_fire_when_token_unchanged() {
        // Google's typical behavior: returns the SAME refresh_token on each
        // refresh. The callback should NOT fire — there's nothing to persist.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "AT-NEW",
                "refresh_token": "RT-1",          // same as before
                "expires_in": 3600,
                "token_type": "Bearer"
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/probe"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
            .mount(&server)
            .await;

        let fired = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let fired_for_cb = fired.clone();
        let cb: RefreshRotationCallback = Arc::new(move |_| {
            fired_for_cb.store(true, std::sync::atomic::Ordering::Relaxed);
        });

        let cfg = cfg(format!("{}/token", server.uri()));
        let client = AuthClient::with_rotation_callback(
            reqwest::Client::new(),
            cfg,
            expired_tokens(),
            Some(cb),
        );
        let _ = client
            .get(&format!("{}/probe", server.uri()))
            .await
            .unwrap();
        assert!(
            !fired.load(std::sync::atomic::Ordering::Relaxed),
            "rotation callback should NOT fire when refresh_token is unchanged"
        );
    }
}

/// Auth-aware HTTP client for a single account. Wraps a `reqwest::Client` and the
/// most-recent `OAuthTokens`. Refreshes the access token automatically before expiry
/// and on 401 responses (see `send`'s retry path). When the provider rotates the
/// refresh token during a refresh, fires `on_rotation` so the daemon can persist
/// the new token to the keychain.
#[derive(Clone)]
pub struct AuthClient {
    http: reqwest::Client,
    cfg: ProviderConfig,
    tokens: Arc<Mutex<OAuthTokens>>,
    on_rotation: Option<RefreshRotationCallback>,
}

/// Callback invoked synchronously by `AuthClient::ensure_fresh` whenever the
/// provider rotates the refresh token (i.e., the new refresh_token differs
/// from the one we used to perform the refresh). The daemon hands in a closure
/// that persists the new token to the OS keychain. The callback receives the
/// new refresh_token bytes — it must not block for long, since it runs while
/// the AuthClient holds its tokens lock.
pub type RefreshRotationCallback = Arc<dyn Fn(&str) + Send + Sync>;

impl AuthClient {
    pub fn new(http: reqwest::Client, cfg: ProviderConfig, tokens: OAuthTokens) -> Self {
        Self::with_rotation_callback(http, cfg, tokens, None)
    }

    /// Same as `new`, but registers `on_rotation` to fire whenever the provider
    /// hands back a different refresh_token during a refresh — closes the v0.1a
    /// gap where rotated tokens were updated in memory but never persisted.
    pub fn with_rotation_callback(
        http: reqwest::Client,
        cfg: ProviderConfig,
        tokens: OAuthTokens,
        on_rotation: Option<RefreshRotationCallback>,
    ) -> Self {
        Self {
            http,
            cfg,
            tokens: Arc::new(Mutex::new(tokens)),
            on_rotation,
        }
    }

    pub async fn access_token(&self) -> String {
        self.tokens.lock().await.access_token.clone()
    }

    async fn ensure_fresh(&self) -> Result<()> {
        let needs_refresh = {
            let g = self.tokens.lock().await;
            g.expires_at <= chrono::Utc::now()
        };
        if !needs_refresh {
            return Ok(());
        }
        let old_refresh_token = {
            let g = self.tokens.lock().await;
            g.refresh_token
                .clone()
                .ok_or_else(|| Error::OAuth("no refresh token available".into()))?
        };
        let new = oauth::refresh(&self.http, &self.cfg, &old_refresh_token).await?;
        let rotated = new.refresh_token.as_deref().map(str::to_owned);
        {
            let mut g = self.tokens.lock().await;
            g.access_token = new.access_token;
            g.expires_at = new.expires_at;
            if let Some(rt) = &rotated {
                g.refresh_token = Some(rt.clone());
            }
        }
        // If the provider rotated the refresh token (Google does this rarely,
        // but the protocol allows it any time), notify the registered
        // persistence callback. Compare against the token we used for THIS
        // refresh, not the construction-time original — that way every
        // rotation gets a callback, even after several rotations.
        if let Some(new_rt) = rotated {
            if new_rt != old_refresh_token {
                if let Some(cb) = &self.on_rotation {
                    cb(&new_rt);
                }
            }
        }
        Ok(())
    }

    pub async fn get(&self, url: &str) -> Result<reqwest::Response> {
        self.send(self.http.get(url)).await
    }

    pub async fn post_json<B: serde::Serialize>(
        &self,
        url: &str,
        body: &B,
    ) -> Result<reqwest::Response> {
        self.send(self.http.post(url).json(body)).await
    }

    pub async fn put_json<B: serde::Serialize>(
        &self,
        url: &str,
        body: &B,
    ) -> Result<reqwest::Response> {
        self.send(self.http.put(url).json(body)).await
    }

    pub async fn delete(&self, url: &str) -> Result<reqwest::Response> {
        self.send(self.http.delete(url)).await
    }

    async fn send(&self, req: reqwest::RequestBuilder) -> Result<reqwest::Response> {
        self.ensure_fresh().await?;
        let token = self.access_token().await;
        let resp = req
            .try_clone()
            .ok_or_else(|| Error::Internal("non-clonable request".into()))?
            .bearer_auth(&token)
            .send()
            .await?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            // Force a refresh, retry once.
            {
                let mut g = self.tokens.lock().await;
                g.expires_at = chrono::Utc::now() - chrono::Duration::seconds(1);
            }
            self.ensure_fresh().await?;
            let token = self.access_token().await;
            return Ok(req.bearer_auth(&token).send().await?);
        }
        Ok(resp)
    }
}
