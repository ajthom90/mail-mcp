use super::client::AuthClient;
use crate::error::Result;
use crate::types::MessageId;

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn auth(server: &MockServer) -> AuthClient {
        AuthClient::new(
            reqwest::Client::new(),
            crate::oauth::ProviderConfig {
                auth_url: "x".into(),
                token_url: format!("{}/token", server.uri()),
                client_id: "c".into(),
                default_scopes: vec![],
            },
            crate::oauth::OAuthTokens {
                access_token: "AT".into(),
                refresh_token: Some("RT".into()),
                expires_at: chrono::Utc::now() + chrono::Duration::seconds(600),
                scope: None,
            },
        )
    }

    #[tokio::test]
    async fn trash_calls_trash_endpoint() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/gmail/v1/users/me/messages/m1/trash"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&server)
            .await;
        let c = auth(&server);
        let base = format!("{}/gmail/v1", server.uri());
        trash_impl(&c, &base, &[MessageId::from("m1")])
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn untrash_calls_untrash_endpoint() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/gmail/v1/users/me/messages/m1/untrash"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&server)
            .await;
        let c = auth(&server);
        let base = format!("{}/gmail/v1", server.uri());
        untrash_impl(&c, &base, &[MessageId::from("m1")])
            .await
            .unwrap();
    }
}

pub async fn trash_impl(client: &AuthClient, base: &str, ids: &[MessageId]) -> Result<()> {
    for id in ids {
        let url = format!("{base}/users/me/messages/{}/trash", id.as_str());
        client
            .post_json(&url, &serde_json::json!({}))
            .await?
            .error_for_status()?;
    }
    Ok(())
}

pub async fn untrash_impl(client: &AuthClient, base: &str, ids: &[MessageId]) -> Result<()> {
    for id in ids {
        let url = format!("{base}/users/me/messages/{}/untrash", id.as_str());
        client
            .post_json(&url, &serde_json::json!({}))
            .await?
            .error_for_status()?;
    }
    Ok(())
}
