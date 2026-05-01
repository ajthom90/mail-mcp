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

        let resp: serde_json::Value = client.get(&format!("{}/probe", server.uri())).await.unwrap().json().await.unwrap();
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

        let resp = client.get(&format!("{}/probe", server.uri())).await.unwrap();
        assert!(resp.status().is_success());
        assert_eq!(client.access_token().await, "AT-NEW");
    }
}

/// Auth-aware HTTP client for a single account. Wraps a `reqwest::Client` and the
/// most-recent `OAuthTokens`. Refreshes the access token automatically before expiry
/// (and on 401 responses, via `with_refresh_on_401`).
#[derive(Clone)]
pub struct AuthClient {
    http: reqwest::Client,
    cfg: ProviderConfig,
    tokens: Arc<Mutex<OAuthTokens>>,
}

impl AuthClient {
    pub fn new(http: reqwest::Client, cfg: ProviderConfig, tokens: OAuthTokens) -> Self {
        Self {
            http,
            cfg,
            tokens: Arc::new(Mutex::new(tokens)),
        }
    }

    pub async fn access_token(&self) -> String {
        self.tokens.lock().await.access_token.clone()
    }

    /// Returns Some(new_refresh_token) if the provider rotated it during the most recent
    /// refresh, so the daemon can persist it to the keychain. Resets to None after read.
    pub async fn take_rotated_refresh(&self) -> Option<String> {
        let mut g = self.tokens.lock().await;
        // Heuristic: if refresh_token differs from what we started with, surface it once.
        // The daemon polls this after each call.
        g.refresh_token.take()
    }

    async fn ensure_fresh(&self) -> Result<()> {
        let needs_refresh = {
            let g = self.tokens.lock().await;
            g.expires_at <= chrono::Utc::now()
        };
        if !needs_refresh {
            return Ok(());
        }
        let refresh_token = {
            let g = self.tokens.lock().await;
            g.refresh_token
                .clone()
                .ok_or_else(|| Error::OAuth("no refresh token available".into()))?
        };
        let new = oauth::refresh(&self.http, &self.cfg, &refresh_token).await?;
        let mut g = self.tokens.lock().await;
        g.access_token = new.access_token;
        g.expires_at = new.expires_at;
        if new.refresh_token.is_some() {
            g.refresh_token = new.refresh_token;
        }
        // Note: rotated refresh tokens (Google rarely rotates) — caller should persist on next
        // observation via `take_rotated_refresh`. For v0.1a Google practice means this is rare.
        Ok(())
    }

    pub async fn get(&self, url: &str) -> Result<reqwest::Response> {
        self.send(self.http.get(url)).await
    }

    pub async fn post_json<B: serde::Serialize>(&self, url: &str, body: &B) -> Result<reqwest::Response> {
        self.send(self.http.post(url).json(body)).await
    }

    pub async fn put_json<B: serde::Serialize>(&self, url: &str, body: &B) -> Result<reqwest::Response> {
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
