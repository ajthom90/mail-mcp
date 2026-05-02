use super::client::AuthClient;
use crate::error::{Error, Result};
use crate::providers::types::{DraftInput, DraftSummary, OutgoingMessage};
use crate::types::{DraftId, MessageId};
use base64::Engine;
use serde::{Deserialize, Serialize};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::types::EmailAddress;
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

    fn input() -> DraftInput {
        DraftInput {
            to: vec![EmailAddress {
                email: "alice@example.com".into(),
                name: None,
            }],
            cc: vec![],
            bcc: vec![],
            subject: "Hello".into(),
            body_text: Some("hi".into()),
            body_html: None,
            in_reply_to: None,
            thread_id: None,
        }
    }

    #[test]
    fn build_mime_round_trips_subject_and_body() {
        let mime = build_mime(&input(), Some("me@example.com")).unwrap();
        let s = std::str::from_utf8(&mime).unwrap();
        assert!(s.contains("Subject: Hello"));
        assert!(s.contains("alice@example.com"));
        assert!(s.contains("hi"));
    }

    #[tokio::test]
    async fn create_draft_posts_raw_message() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/gmail/v1/users/me/drafts"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "d1",
                "message": {"id": "m1", "threadId": "t1"}
            })))
            .mount(&server)
            .await;
        let c = auth(&server);
        let base = format!("{}/gmail/v1", server.uri());
        let id = create_draft_impl(&c, &base, &input(), Some("me@example.com"))
            .await
            .unwrap();
        assert_eq!(id.as_str(), "d1");
    }

    #[tokio::test]
    async fn send_message_posts_send_endpoint() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/gmail/v1/users/me/messages/send"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "m1", "threadId": "t1"
            })))
            .mount(&server)
            .await;
        let c = auth(&server);
        let base = format!("{}/gmail/v1", server.uri());
        let m = OutgoingMessage {
            to: vec![EmailAddress {
                email: "alice@example.com".into(),
                name: None,
            }],
            cc: vec![],
            bcc: vec![],
            subject: "Hi".into(),
            body_text: Some("body".into()),
            body_html: None,
            in_reply_to: None,
            thread_id: None,
        };
        let id = send_message_impl(&c, &base, &m, Some("me@example.com"))
            .await
            .unwrap();
        assert_eq!(id.as_str(), "m1");
    }

    #[tokio::test]
    async fn list_drafts_populates_subject_and_snippet() {
        let server = MockServer::start().await;
        // The list endpoint returns stubs.
        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/drafts"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "drafts": [
                    {"id":"d-1","message":{"id":"m-1","threadId":"t-1"}},
                    {"id":"d-2","message":{"id":"m-2","threadId":"t-2"}},
                ]
            })))
            .mount(&server)
            .await;
        // Each per-draft fetch returns full metadata.
        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/drafts/d-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "d-1",
                "message": {
                    "id": "m-1",
                    "snippet": "preview one",
                    "internalDate": "1714579200000",
                    "payload": {
                        "headers": [
                            {"name": "Subject", "value": "First draft"},
                            {"name": "From", "value": "me@example.com"},
                        ]
                    }
                }
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/drafts/d-2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "d-2",
                "message": {
                    "id": "m-2",
                    "snippet": "preview two",
                    "internalDate": "1714665600000",
                    "payload": {
                        "headers": [
                            {"name": "Subject", "value": "Second draft"},
                        ]
                    }
                }
            })))
            .mount(&server)
            .await;

        let c = auth(&server);
        let base = format!("{}/gmail/v1", server.uri());
        let drafts = list_drafts_impl(&c, &base).await.unwrap();
        assert_eq!(drafts.len(), 2);
        assert_eq!(drafts[0].id.as_str(), "d-1");
        assert_eq!(drafts[0].subject, "First draft");
        assert_eq!(drafts[0].snippet, "preview one");
        assert_eq!(drafts[1].subject, "Second draft");
        assert_eq!(drafts[1].snippet, "preview two");
        // Date came from internalDate, not from chrono::Utc::now().
        assert_eq!(drafts[0].date.timestamp_millis(), 1_714_579_200_000);
    }

    #[tokio::test]
    async fn list_drafts_handles_missing_subject_header() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/drafts"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "drafts": [{"id":"d-x","message":{"id":"m-x","threadId":"t-x"}}]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/drafts/d-x"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "d-x",
                "message": {
                    "id": "m-x",
                    "snippet": "no subject here",
                    "internalDate": "1714579200000",
                    "payload": {"headers": []}
                }
            })))
            .mount(&server)
            .await;

        let c = auth(&server);
        let base = format!("{}/gmail/v1", server.uri());
        let drafts = list_drafts_impl(&c, &base).await.unwrap();
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].subject, "");
        assert_eq!(drafts[0].snippet, "no subject here");
    }
}

#[derive(Serialize)]
struct RawHolder<'a> {
    raw: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "threadId")]
    thread_id: Option<&'a str>,
}

#[derive(Serialize)]
struct DraftPayload<'a> {
    message: RawHolder<'a>,
}

#[derive(Deserialize)]
struct DraftCreateResp {
    id: String,
}

#[derive(Deserialize)]
struct DraftListResp {
    #[serde(default)]
    drafts: Vec<DraftListEntry>,
}

/// The list endpoint only returns stub fields. We just need the id to fetch
/// each draft's full metadata in a follow-up call.
#[derive(Deserialize)]
struct DraftListEntry {
    id: String,
}

/// Per-draft response from `GET /users/me/drafts/{id}?format=metadata`. We pull
/// the snippet and Subject header out so list_drafts can return non-empty data.
#[derive(Deserialize)]
struct DraftDetail {
    id: String,
    message: DraftDetailMsg,
}

#[derive(Deserialize)]
struct DraftDetailMsg {
    id: String,
    #[serde(default)]
    snippet: String,
    #[serde(rename = "internalDate", default)]
    internal_date: String,
    #[serde(default)]
    payload: Option<DraftDetailPayload>,
}

#[derive(Deserialize)]
struct DraftDetailPayload {
    #[serde(default)]
    headers: Vec<DraftDetailHeader>,
}

#[derive(Deserialize)]
struct DraftDetailHeader {
    name: String,
    value: String,
}

#[derive(Deserialize)]
struct SendResp {
    id: String,
}

pub fn build_mime(input: &DraftInput, from: Option<&str>) -> Result<Vec<u8>> {
    use mail_builder::MessageBuilder;
    let mut builder = MessageBuilder::new();
    if let Some(f) = from {
        builder = builder.from(f);
    }
    builder = builder.to(input.to.iter().map(|a| a.email.clone()).collect::<Vec<_>>());
    if !input.cc.is_empty() {
        builder = builder.cc(input.cc.iter().map(|a| a.email.clone()).collect::<Vec<_>>());
    }
    if !input.bcc.is_empty() {
        builder = builder.bcc(
            input
                .bcc
                .iter()
                .map(|a| a.email.clone())
                .collect::<Vec<_>>(),
        );
    }
    builder = builder.subject(input.subject.clone());
    if let Some(t) = &input.body_text {
        builder = builder.text_body(t.clone());
    }
    if let Some(h) = &input.body_html {
        builder = builder.html_body(h.clone());
    }
    if let Some(rep) = &input.in_reply_to {
        builder = builder.in_reply_to(rep.as_str());
    }
    let bytes = builder
        .write_to_vec()
        .map_err(|e| Error::Provider(format!("mime build failed: {e}")))?;
    Ok(bytes)
}

fn b64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

pub async fn create_draft_impl(
    client: &AuthClient,
    base: &str,
    input: &DraftInput,
    from: Option<&str>,
) -> Result<DraftId> {
    let mime = build_mime(input, from)?;
    let raw = b64url(&mime);
    let payload = DraftPayload {
        message: RawHolder {
            raw: &raw,
            thread_id: input.thread_id.as_ref().map(|t| t.as_str()),
        },
    };
    let url = format!("{base}/users/me/drafts");
    let resp: DraftCreateResp = client
        .post_json(&url, &payload)
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
    from: Option<&str>,
) -> Result<()> {
    let mime = build_mime(input, from)?;
    let raw = b64url(&mime);
    let payload = DraftPayload {
        message: RawHolder {
            raw: &raw,
            thread_id: input.thread_id.as_ref().map(|t| t.as_str()),
        },
    };
    let url = format!("{base}/users/me/drafts/{}", id.as_str());
    client.put_json(&url, &payload).await?.error_for_status()?;
    Ok(())
}

pub async fn list_drafts_impl(client: &AuthClient, base: &str) -> Result<Vec<DraftSummary>> {
    let list_url = format!("{base}/users/me/drafts");
    let resp: DraftListResp = client
        .get(&list_url)
        .await?
        .error_for_status()?
        .json()
        .await?;

    // Gmail's drafts list endpoint only returns id + message stub. Fetch each
    // draft's metadata individually to populate subject + snippet + date.
    let mut out = Vec::with_capacity(resp.drafts.len());
    for d in resp.drafts {
        let detail_url = format!(
            "{base}/users/me/drafts/{}?format=metadata&metadataHeaders=Subject",
            d.id
        );
        let detail: DraftDetail = client
            .get(&detail_url)
            .await?
            .error_for_status()?
            .json()
            .await?;

        let subject = detail
            .message
            .payload
            .as_ref()
            .and_then(|p| {
                p.headers
                    .iter()
                    .find(|h| h.name.eq_ignore_ascii_case("Subject"))
            })
            .map(|h| h.value.clone())
            .unwrap_or_default();

        let date = detail
            .message
            .internal_date
            .parse::<i64>()
            .ok()
            .and_then(chrono::DateTime::from_timestamp_millis)
            .unwrap_or_else(chrono::Utc::now);

        out.push(DraftSummary {
            id: DraftId::from(detail.id),
            message_id: MessageId::from(detail.message.id),
            subject,
            snippet: detail.message.snippet,
            date,
        });
    }
    Ok(out)
}

pub async fn send_draft_impl(client: &AuthClient, base: &str, id: &DraftId) -> Result<MessageId> {
    let url = format!("{base}/users/me/drafts/send");
    let resp: SendResp = client
        .post_json(&url, &serde_json::json!({"id": id.as_str()}))
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(MessageId::from(resp.id))
}

pub async fn send_message_impl(
    client: &AuthClient,
    base: &str,
    m: &OutgoingMessage,
    from: Option<&str>,
) -> Result<MessageId> {
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
    let mime = build_mime(&input, from)?;
    let raw = b64url(&mime);
    let url = format!("{base}/users/me/messages/send");
    let payload =
        serde_json::json!({"raw": raw, "threadId": m.thread_id.as_ref().map(|t| t.as_str())});
    let resp: SendResp = client
        .post_json(&url, &payload)
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(MessageId::from(resp.id))
}
