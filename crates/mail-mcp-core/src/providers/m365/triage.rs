// Wired up by Task 8 (provider.rs MailProvider impl).
#![allow(dead_code)]

//! mark_read / star / label / move_to / archive / trash / untrash for Graph.
//!
//! Graph PATCHes `/me/messages/{id}` for in-place mutations (read flag,
//! starred flag, categories) and POSTs `/me/messages/{id}/move` for folder
//! transitions. There's no batch API on the free tier; we loop the
//! per-id calls. The `$batch` endpoint is a v0.2.x optimization.

use crate::error::Result;
use crate::providers::gmail::AuthClient;
use crate::types::{FolderId, LabelId, MessageId};
use serde_json::{json, Value};

// Well-known folder IDs (per folders.rs::list_folders) used by the move
// helpers. Folder ids surfaced to callers ARE the wellKnownName strings,
// so move_to_impl can pass them straight to Graph's destinationId.
const ARCHIVE_FOLDER: &str = "archive";
const TRASH_FOLDER: &str = "deleteditems";
const INBOX_FOLDER: &str = "inbox";

async fn patch_one(client: &AuthClient, base: &str, id: &str, body: Value) -> Result<()> {
    let url = format!("{base}/me/messages/{id}");
    client.patch_json(&url, &body).await?.error_for_status()?;
    Ok(())
}

async fn move_one(client: &AuthClient, base: &str, id: &str, dest: &str) -> Result<()> {
    let url = format!("{base}/me/messages/{id}/move");
    let body = json!({ "destinationId": dest });
    client.post_json(&url, &body).await?.error_for_status()?;
    Ok(())
}

pub async fn mark_read_impl(
    client: &AuthClient,
    base: &str,
    ids: &[MessageId],
    read: bool,
) -> Result<()> {
    for id in ids {
        patch_one(client, base, id.as_str(), json!({ "isRead": read })).await?;
    }
    Ok(())
}

pub async fn star_impl(
    client: &AuthClient,
    base: &str,
    ids: &[MessageId],
    starred: bool,
) -> Result<()> {
    let status = if starred { "flagged" } else { "notFlagged" };
    for id in ids {
        patch_one(
            client,
            base,
            id.as_str(),
            json!({ "flag": { "flagStatus": status } }),
        )
        .await?;
    }
    Ok(())
}

pub async fn label_impl(
    client: &AuthClient,
    base: &str,
    ids: &[MessageId],
    label: &LabelId,
    on: bool,
) -> Result<()> {
    // Graph's PATCH /me/messages/{id} { categories: [...] } REPLACES the
    // category list. To toggle one without losing the others, GET the
    // current list, mutate, PATCH back.
    for id in ids {
        let url = format!("{base}/me/messages/{}?$select=categories", id.as_str());
        let resp: Value = client.get(&url).await?.error_for_status()?.json().await?;
        let existing: Vec<String> = resp
            .get("categories")
            .and_then(|c| c.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let target = label.as_str().to_string();
        let mut next: Vec<String> = existing.into_iter().filter(|c| c != &target).collect();
        if on {
            next.push(target);
        }
        patch_one(client, base, id.as_str(), json!({ "categories": next })).await?;
    }
    Ok(())
}

pub async fn move_to_impl(
    client: &AuthClient,
    base: &str,
    ids: &[MessageId],
    folder: &FolderId,
) -> Result<()> {
    for id in ids {
        move_one(client, base, id.as_str(), folder.as_str()).await?;
    }
    Ok(())
}

pub async fn archive_impl(client: &AuthClient, base: &str, ids: &[MessageId]) -> Result<()> {
    for id in ids {
        move_one(client, base, id.as_str(), ARCHIVE_FOLDER).await?;
    }
    Ok(())
}

pub async fn trash_impl(client: &AuthClient, base: &str, ids: &[MessageId]) -> Result<()> {
    for id in ids {
        move_one(client, base, id.as_str(), TRASH_FOLDER).await?;
    }
    Ok(())
}

pub async fn untrash_impl(client: &AuthClient, base: &str, ids: &[MessageId]) -> Result<()> {
    // Graph doesn't have a "restore from trash to original folder" API.
    // Closest analogue: move back to Inbox. The user can re-organize from
    // there; preserving the original parent would need us to remember it.
    for id in ids {
        move_one(client, base, id.as_str(), INBOX_FOLDER).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_partial_json, method, path};
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
    async fn mark_read_patches_is_read_true() {
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(path("/v1.0/me/messages/m1"))
            .and(body_partial_json(json!({ "isRead": true })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&server)
            .await;
        let c = auth(&server);
        mark_read_impl(
            &c,
            &format!("{}/v1.0", server.uri()),
            &[MessageId::from("m1")],
            true,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn star_patches_flag_status() {
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(path("/v1.0/me/messages/m1"))
            .and(body_partial_json(json!({ "flag": { "flagStatus": "flagged" } })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&server)
            .await;
        let c = auth(&server);
        star_impl(
            &c,
            &format!("{}/v1.0", server.uri()),
            &[MessageId::from("m1")],
            true,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn archive_posts_destination_archive() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1.0/me/messages/m1/move"))
            .and(body_partial_json(json!({ "destinationId": "archive" })))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({})))
            .mount(&server)
            .await;
        let c = auth(&server);
        archive_impl(
            &c,
            &format!("{}/v1.0", server.uri()),
            &[MessageId::from("m1")],
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn trash_moves_to_deleteditems() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1.0/me/messages/m1/move"))
            .and(body_partial_json(json!({ "destinationId": "deleteditems" })))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({})))
            .mount(&server)
            .await;
        let c = auth(&server);
        trash_impl(
            &c,
            &format!("{}/v1.0", server.uri()),
            &[MessageId::from("m1")],
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn label_merges_existing_categories() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1.0/me/messages/m1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "categories": ["VIP", "Followups"]
            })))
            .mount(&server)
            .await;
        Mock::given(method("PATCH"))
            .and(path("/v1.0/me/messages/m1"))
            .and(body_partial_json(json!({
                "categories": ["VIP", "Followups", "Project-X"]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&server)
            .await;
        let c = auth(&server);
        label_impl(
            &c,
            &format!("{}/v1.0", server.uri()),
            &[MessageId::from("m1")],
            &LabelId::from("Project-X"),
            true,
        )
        .await
        .unwrap();
    }
}
