use super::client::AuthClient;
use crate::error::Result;
use crate::types::{FolderId, LabelId, MessageId};
use serde::Serialize;

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_json, method, path};
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
    async fn mark_read_removes_unread_label() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/gmail/v1/users/me/messages/m1/modify"))
            .and(body_json(serde_json::json!({"removeLabelIds":["UNREAD"]})))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&server)
            .await;
        let c = auth(&server);
        let base = format!("{}/gmail/v1", server.uri());
        mark_read_impl(&c, &base, &[MessageId::from("m1")], true)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn star_adds_starred_label() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/gmail/v1/users/me/messages/m1/modify"))
            .and(body_json(serde_json::json!({"addLabelIds":["STARRED"]})))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&server)
            .await;
        let c = auth(&server);
        let base = format!("{}/gmail/v1", server.uri());
        star_impl(&c, &base, &[MessageId::from("m1")], true)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn archive_removes_inbox_label() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/gmail/v1/users/me/messages/m1/modify"))
            .and(body_json(serde_json::json!({"removeLabelIds":["INBOX"]})))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&server)
            .await;
        let c = auth(&server);
        let base = format!("{}/gmail/v1", server.uri());
        archive_impl(&c, &base, &[MessageId::from("m1")])
            .await
            .unwrap();
    }
}

#[derive(Default, Serialize)]
struct ModifyRequest {
    #[serde(rename = "addLabelIds", skip_serializing_if = "Vec::is_empty")]
    add: Vec<String>,
    #[serde(rename = "removeLabelIds", skip_serializing_if = "Vec::is_empty")]
    remove: Vec<String>,
}

async fn modify_one(
    client: &AuthClient,
    base: &str,
    id: &MessageId,
    req: &ModifyRequest,
) -> Result<()> {
    let url = format!("{base}/users/me/messages/{}/modify", id.as_str());
    client.post_json(&url, req).await?.error_for_status()?;
    Ok(())
}

async fn modify_all(
    client: &AuthClient,
    base: &str,
    ids: &[MessageId],
    req: ModifyRequest,
) -> Result<()> {
    for id in ids {
        modify_one(client, base, id, &req).await?;
    }
    Ok(())
}

pub async fn mark_read_impl(
    client: &AuthClient,
    base: &str,
    ids: &[MessageId],
    read: bool,
) -> Result<()> {
    let req = if read {
        ModifyRequest {
            remove: vec!["UNREAD".into()],
            ..Default::default()
        }
    } else {
        ModifyRequest {
            add: vec!["UNREAD".into()],
            ..Default::default()
        }
    };
    modify_all(client, base, ids, req).await
}

pub async fn star_impl(
    client: &AuthClient,
    base: &str,
    ids: &[MessageId],
    starred: bool,
) -> Result<()> {
    let req = if starred {
        ModifyRequest {
            add: vec!["STARRED".into()],
            ..Default::default()
        }
    } else {
        ModifyRequest {
            remove: vec!["STARRED".into()],
            ..Default::default()
        }
    };
    modify_all(client, base, ids, req).await
}

pub async fn label_impl(
    client: &AuthClient,
    base: &str,
    ids: &[MessageId],
    label: &LabelId,
    on: bool,
) -> Result<()> {
    let req = if on {
        ModifyRequest {
            add: vec![label.as_str().into()],
            ..Default::default()
        }
    } else {
        ModifyRequest {
            remove: vec![label.as_str().into()],
            ..Default::default()
        }
    };
    modify_all(client, base, ids, req).await
}

pub async fn move_to_impl(
    client: &AuthClient,
    base: &str,
    ids: &[MessageId],
    folder: &FolderId,
) -> Result<()> {
    // Gmail "move" semantics = remove all the system folder labels we know about, add the target.
    let mut remove: Vec<String> = vec![];
    for sys in &["INBOX", "SPAM", "TRASH"] {
        if folder.as_str() != *sys {
            remove.push((*sys).into());
        }
    }
    let req = ModifyRequest {
        add: vec![folder.as_str().into()],
        remove,
    };
    modify_all(client, base, ids, req).await
}

pub async fn archive_impl(client: &AuthClient, base: &str, ids: &[MessageId]) -> Result<()> {
    let req = ModifyRequest {
        remove: vec!["INBOX".into()],
        ..Default::default()
    };
    modify_all(client, base, ids, req).await
}
