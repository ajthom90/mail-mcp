// Wired up by Task 8 (provider.rs MailProvider impl).
#![allow(dead_code)]

//! Compose / send for Microsoft Graph.
//!
//! Graph's draft model: POST /me/messages creates a draft (body is a Graph
//! Message resource); PATCH /me/messages/{id} updates one;
//! POST /me/messages/{id}/send dispatches it. The /send endpoint returns
//! 202 with no body and Graph generates a NEW message id in Sent Items
//! asynchronously — so the MessageId we return for send_* is the draft id
//! (what the user identifies as "the thing that got sent"). When the
//! daemon later wants the Sent Items copy, it can search by subject /
//! recipients.

use crate::error::Result;
use crate::providers::gmail::AuthClient;
use crate::providers::types::{DraftInput, DraftSummary, OutgoingMessage};
use crate::types::{DraftId, MessageId};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
struct CreatedMessage {
    id: String,
}

#[derive(Deserialize)]
struct DraftListResponse {
    value: Vec<DraftListItem>,
}

#[derive(Deserialize)]
struct DraftListItem {
    id: String,
    #[serde(default)]
    subject: Option<String>,
    #[serde(rename = "bodyPreview", default)]
    body_preview: Option<String>,
    #[serde(rename = "createdDateTime", default)]
    created_date_time: Option<String>,
    #[serde(rename = "isDraft", default)]
    is_draft: bool,
}

fn message_body(input: &DraftInput) -> Value {
    let (content_type, content) = match (&input.body_html, &input.body_text) {
        (Some(html), _) => ("html", html.clone()),
        (None, Some(text)) => ("text", text.clone()),
        (None, None) => ("text", String::new()),
    };
    json!({
        "subject": input.subject,
        "body": { "contentType": content_type, "content": content },
        "toRecipients": recipients(&input.to),
        "ccRecipients": recipients(&input.cc),
        "bccRecipients": recipients(&input.bcc),
    })
}

fn recipients(addrs: &[crate::providers::types::EmailAddress]) -> Value {
    addrs
        .iter()
        .map(|a| {
            let mut email = json!({ "address": a.email });
            if let Some(name) = &a.name {
                email["name"] = json!(name);
            }
            json!({ "emailAddress": email })
        })
        .collect::<Vec<_>>()
        .into()
}

pub async fn create_draft_impl(
    client: &AuthClient,
    base: &str,
    input: &DraftInput,
) -> Result<DraftId> {
    let url = format!("{base}/me/messages");
    let resp: CreatedMessage = client
        .post_json(&url, &message_body(input))
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(DraftId::from(resp.id))
}

pub async fn update_draft_impl(
    client: &AuthClient,
    base: &str,
    id: &DraftId,
    input: &DraftInput,
) -> Result<()> {
    let url = format!("{base}/me/messages/{}", id.as_str());
    client
        .patch_json(&url, &message_body(input))
        .await?
        .error_for_status()?;
    Ok(())
}

pub async fn list_drafts_impl(client: &AuthClient, base: &str) -> Result<Vec<DraftSummary>> {
    let url =
        format!("{base}/me/mailFolders/Drafts/messages?$top=50&$orderby=createdDateTime desc");
    let resp: DraftListResponse = client.get(&url).await?.error_for_status()?.json().await?;
    let mut out = Vec::with_capacity(resp.value.len());
    for d in resp.value {
        if !d.is_draft {
            continue;
        }
        let date = d
            .created_date_time
            .as_deref()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(chrono::Utc::now);
        out.push(DraftSummary {
            id: DraftId::from(d.id.clone()),
            // Graph drafts don't have a separate "message id"; reuse the
            // same value. Once sent, the Sent Items copy gets its own id.
            message_id: MessageId::from(d.id),
            subject: d.subject.unwrap_or_default(),
            snippet: d.body_preview.unwrap_or_default(),
            date,
        });
    }
    Ok(out)
}

pub async fn send_draft_impl(client: &AuthClient, base: &str, id: &DraftId) -> Result<MessageId> {
    let url = format!("{base}/me/messages/{}/send", id.as_str());
    // /send returns 202 No Content; no body to deserialize.
    client
        .post_json(&url, &json!({}))
        .await?
        .error_for_status()?;
    // Graph generates a new id in Sent Items asynchronously; the daemon
    // can locate the sent copy via search if needed.
    Ok(MessageId::from(id.as_str().to_string()))
}

pub async fn send_message_impl(
    client: &AuthClient,
    base: &str,
    m: &OutgoingMessage,
) -> Result<MessageId> {
    // Two-step: create the draft, then send it. /me/sendMail also exists
    // as a one-shot but doesn't return the underlying id, which our
    // MailProvider trait wants. The two-step path keeps the IDs consistent
    // with the create_draft + send_draft flow.
    let input = DraftInput {
        to: m.to.clone(),
        cc: m.cc.clone(),
        bcc: m.bcc.clone(),
        subject: m.subject.clone(),
        body_text: m.body_text.clone(),
        body_html: m.body_html.clone(),
        in_reply_to: m.in_reply_to.clone(),
        thread_id: m.thread_id.clone(),
    };
    let draft_id = create_draft_impl(client, base, &input).await?;
    send_draft_impl(client, base, &draft_id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::types::EmailAddress;
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

    fn draft() -> DraftInput {
        DraftInput {
            to: vec![EmailAddress {
                email: "alice@example.com".into(),
                name: Some("Alice".into()),
            }],
            cc: vec![],
            bcc: vec![],
            subject: "Hello".into(),
            body_text: Some("Plain text".into()),
            body_html: None,
            in_reply_to: None,
            thread_id: None,
        }
    }

    #[tokio::test]
    async fn create_draft_returns_id_from_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1.0/me/messages"))
            .and(body_partial_json(json!({
                "subject": "Hello",
                "body": { "contentType": "text", "content": "Plain text" }
            })))
            .respond_with(
                ResponseTemplate::new(201).set_body_json(json!({ "id": "AAMkAD-newdraft" })),
            )
            .mount(&server)
            .await;
        let c = auth(&server);
        let id = create_draft_impl(&c, &format!("{}/v1.0", server.uri()), &draft())
            .await
            .unwrap();
        assert_eq!(id.as_str(), "AAMkAD-newdraft");
    }

    #[tokio::test]
    async fn update_draft_patches_message() {
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(path("/v1.0/me/messages/AAMkAD-d1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&server)
            .await;
        let c = auth(&server);
        update_draft_impl(
            &c,
            &format!("{}/v1.0", server.uri()),
            &DraftId::from("AAMkAD-d1"),
            &draft(),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn list_drafts_returns_summaries() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1.0/me/mailFolders/Drafts/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "value": [
                    {
                        "id": "d1",
                        "subject": "Reply to Bob",
                        "bodyPreview": "Sounds good",
                        "createdDateTime": "2026-05-01T12:00:00Z",
                        "isDraft": true,
                    },
                    {
                        "id": "x1",
                        "subject": "Not a draft",
                        "bodyPreview": "...",
                        "createdDateTime": "2026-05-01T12:00:00Z",
                        "isDraft": false,
                    }
                ]
            })))
            .mount(&server)
            .await;
        let c = auth(&server);
        let drafts = list_drafts_impl(&c, &format!("{}/v1.0", server.uri()))
            .await
            .unwrap();
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].subject, "Reply to Bob");
    }

    #[tokio::test]
    async fn send_draft_posts_to_send_endpoint() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1.0/me/messages/AAMkAD-d1/send"))
            .respond_with(ResponseTemplate::new(202))
            .mount(&server)
            .await;
        let c = auth(&server);
        let id = send_draft_impl(
            &c,
            &format!("{}/v1.0", server.uri()),
            &DraftId::from("AAMkAD-d1"),
        )
        .await
        .unwrap();
        assert_eq!(id.as_str(), "AAMkAD-d1");
    }

    #[tokio::test]
    async fn send_message_creates_draft_then_sends() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1.0/me/messages"))
            .respond_with(
                ResponseTemplate::new(201).set_body_json(json!({ "id": "AAMkAD-newdraft" })),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1.0/me/messages/AAMkAD-newdraft/send"))
            .respond_with(ResponseTemplate::new(202))
            .mount(&server)
            .await;
        let c = auth(&server);
        let m = OutgoingMessage {
            to: vec![EmailAddress {
                email: "bob@example.com".into(),
                name: None,
            }],
            cc: vec![],
            bcc: vec![],
            subject: "Hi".into(),
            body_text: Some("Body".into()),
            body_html: None,
            in_reply_to: None,
            thread_id: None,
        };
        let id = send_message_impl(&c, &format!("{}/v1.0", server.uri()), &m)
            .await
            .unwrap();
        assert_eq!(id.as_str(), "AAMkAD-newdraft");
    }
}
