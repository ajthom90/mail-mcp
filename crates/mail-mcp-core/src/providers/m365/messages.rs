// Wired up by Task 8 (provider.rs MailProvider impl).
#![allow(dead_code)]

//! Search / thread / message fetch for Microsoft Graph.
//!
//! Graph's mail surface is shaped quite differently from Gmail's:
//!
//! * **No threads endpoint.** Graph groups messages with `conversationId`
//!   but doesn't expose a thread resource. We synthesize `ThreadSummary` by
//!   grouping search results client-side; the representative message is the
//!   latest matching one. `get_thread` fetches every message with the
//!   matching conversationId.
//! * **`@odata.nextLink` pagination.** Graph returns a fully-qualified URL
//!   for the next page; we surface that URL as the opaque cursor and pass
//!   it back verbatim on the next call. (Same shape as Gmail's pageToken,
//!   different content.)
//! * **`$search` is OData full-text search.** Subset of Gmail's query
//!   language; the daemon's MCP tool description documents the differences
//!   so model-side queries can adapt.

use super::parse::{decode, RawMessage};
use crate::error::Result;
use crate::providers::gmail::AuthClient;
use crate::providers::r#trait::SearchResults;
use crate::providers::types::{Message, SearchQuery, Thread, ThreadSummary};
use crate::types::{MessageId, ThreadId};
use serde::Deserialize;

#[derive(Deserialize)]
struct ListResponse<T> {
    value: Vec<T>,
    #[serde(rename = "@odata.nextLink", default)]
    next_link: Option<String>,
}

pub async fn search_impl(
    client: &AuthClient,
    base: &str,
    q: &SearchQuery,
) -> Result<SearchResults> {
    let url = build_search_url(base, q);
    let resp: ListResponse<RawMessage> =
        client.get(&url).await?.error_for_status()?.json().await?;

    // Group by conversationId; latest receivedDateTime wins as the
    // thread representative. Graph already returns search results in
    // descending receivedDateTime so the first occurrence is freshest.
    let mut seen = std::collections::BTreeSet::new();
    let mut summaries = Vec::with_capacity(resp.value.len());
    for raw in resp.value {
        if !seen.insert(raw.conversation_id.clone()) {
            continue;
        }
        let m = decode(raw)?;
        summaries.push(ThreadSummary {
            id: m.thread_id.clone(),
            last_message_id: m.id.clone(),
            subject: m.subject.clone(),
            snippet: m.snippet.clone(),
            from: m.from.clone(),
            date: m.date,
            // We don't know the true count without an extra round trip;
            // 1 is a safe lower bound that the tray UI treats as "at
            // least one message". get_thread returns the real count.
            message_count: 1,
            unread: !m.flags.read,
            starred: m.flags.starred,
            labels: m.labels.clone(),
            folder: m.folder.clone(),
        });
    }

    Ok(SearchResults {
        threads: summaries,
        next_cursor: resp.next_link,
    })
}

pub async fn get_thread_impl(
    client: &AuthClient,
    base: &str,
    id: &ThreadId,
) -> Result<Thread> {
    let url = format!(
        "{base}/me/messages?$filter=conversationId eq '{}'&$orderby=receivedDateTime asc&$top=200",
        urlencode(id.as_str()),
    );
    let resp: ListResponse<RawMessage> =
        client.get(&url).await?.error_for_status()?.json().await?;

    let mut messages = Vec::with_capacity(resp.value.len());
    for raw in resp.value {
        messages.push(decode(raw)?);
    }
    let subject = messages
        .first()
        .map(|m| m.subject.clone())
        .unwrap_or_default();
    Ok(Thread {
        id: id.clone(),
        subject,
        messages,
    })
}

pub async fn get_message_impl(
    client: &AuthClient,
    base: &str,
    id: &MessageId,
) -> Result<Message> {
    let url = format!("{base}/me/messages/{}", urlencode(id.as_str()));
    let raw: RawMessage = client.get(&url).await?.error_for_status()?.json().await?;
    decode(raw)
}

fn build_search_url(base: &str, q: &SearchQuery) -> String {
    // If the cursor is a fully-qualified @odata.nextLink, use it verbatim.
    if let Some(cursor) = &q.cursor {
        if cursor.starts_with("http://") || cursor.starts_with("https://") {
            return cursor.clone();
        }
    }
    let mut url = format!("{base}/me/messages?$top={}", q.limit.unwrap_or(25));
    url.push_str("&$orderby=receivedDateTime desc");
    if let Some(text) = &q.text {
        url.push_str("&$search=");
        // Graph's $search wants the value double-quoted then URL-encoded.
        url.push_str(&urlencode(&format!("\"{text}\"")));
    }
    if let Some(folder) = &q.folder {
        url.push_str("&$filter=parentFolderId eq '");
        url.push_str(&urlencode(folder.as_str()));
        url.push('\'');
    }
    // Graph categories ($search) cannot combine with $filter eq; if both label
    // and folder are set, the folder filter wins for now and label is dropped.
    // Tasks 5+ refinement could OR them with separate calls.
    url
}

fn urlencode(s: &str) -> String {
    use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
    utf8_percent_encode(s, NON_ALPHANUMERIC).to_string()
}

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

    fn raw_message(id: &str, conv: &str, subject: &str, ts: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "conversationId": conv,
            "subject": subject,
            "bodyPreview": "preview",
            "from": { "emailAddress": { "name": "Alice", "address": "alice@example.com" } },
            "toRecipients": [],
            "ccRecipients": [],
            "bccRecipients": [],
            "categories": [],
            "isRead": true,
            "flag": { "flagStatus": "notFlagged" },
            "isDraft": false,
            "receivedDateTime": ts,
            "body": { "contentType": "text", "content": "body" }
        })
    }

    #[tokio::test]
    async fn search_groups_by_conversation_id() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1.0/me/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "value": [
                    raw_message("m1", "c-A", "Reply 2",   "2026-05-01T12:00:00Z"),
                    raw_message("m2", "c-A", "Reply 1",   "2026-05-01T11:00:00Z"),
                    raw_message("m3", "c-B", "Other",     "2026-05-01T10:00:00Z"),
                ]
            })))
            .mount(&server)
            .await;
        let c = auth(&server);
        let q = SearchQuery {
            text: Some("anything".into()),
            ..SearchQuery::default()
        };
        let r = search_impl(&c, &format!("{}/v1.0", server.uri()), &q)
            .await
            .unwrap();
        // Two distinct conversationIds → two summaries; m1 wins for c-A.
        assert_eq!(r.threads.len(), 2);
        assert_eq!(r.threads[0].subject, "Reply 2");
        assert_eq!(r.threads[1].subject, "Other");
    }

    #[tokio::test]
    async fn search_surfaces_next_link_as_cursor() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1.0/me/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "value": [],
                "@odata.nextLink": "https://graph.microsoft.com/v1.0/me/messages?$skiptoken=ABCD"
            })))
            .mount(&server)
            .await;
        let c = auth(&server);
        let r = search_impl(
            &c,
            &format!("{}/v1.0", server.uri()),
            &SearchQuery::default(),
        )
        .await
        .unwrap();
        assert_eq!(
            r.next_cursor.as_deref(),
            Some("https://graph.microsoft.com/v1.0/me/messages?$skiptoken=ABCD")
        );
    }

    #[tokio::test]
    async fn get_thread_collects_messages_in_conversation() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1.0/me/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "value": [
                    raw_message("m1", "c-A", "First",  "2026-05-01T10:00:00Z"),
                    raw_message("m2", "c-A", "Second", "2026-05-01T11:00:00Z"),
                ]
            })))
            .mount(&server)
            .await;
        let c = auth(&server);
        let t = get_thread_impl(
            &c,
            &format!("{}/v1.0", server.uri()),
            &ThreadId::from("c-A"),
        )
        .await
        .unwrap();
        assert_eq!(t.messages.len(), 2);
        assert_eq!(t.subject, "First");
    }

    #[tokio::test]
    async fn get_message_decodes_single_payload() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1.0/me/messages/AAMkAD-msgid"))
            .respond_with(ResponseTemplate::new(200).set_body_json(raw_message(
                "AAMkAD-msgid",
                "convA",
                "Hello",
                "2026-05-01T12:00:00Z",
            )))
            .mount(&server)
            .await;
        let c = auth(&server);
        let m = get_message_impl(
            &c,
            &format!("{}/v1.0", server.uri()),
            &MessageId::from("AAMkAD-msgid"),
        )
        .await
        .unwrap();
        assert_eq!(m.subject, "Hello");
        assert_eq!(m.id.as_str(), "AAMkAD-msgid");
    }

    #[test]
    fn build_search_url_uses_cursor_when_full_url() {
        let q = SearchQuery {
            cursor: Some("https://graph.microsoft.com/v1.0/me/messages?$skiptoken=NEXT".into()),
            ..SearchQuery::default()
        };
        let url = build_search_url("https://x/y", &q);
        assert_eq!(
            url,
            "https://graph.microsoft.com/v1.0/me/messages?$skiptoken=NEXT"
        );
    }

    #[test]
    fn build_search_url_includes_search_text() {
        let q = SearchQuery {
            text: Some("from:alice".into()),
            ..SearchQuery::default()
        };
        let url = build_search_url("https://x/v1.0", &q);
        assert!(url.contains("$search="));
        assert!(url.contains("alice"));
    }
}
