use crate::error::Result;
use crate::providers::r#trait::SearchResults;
use crate::providers::types::{SearchQuery, Thread, ThreadSummary};
use crate::types::{MessageId, ThreadId};
use super::client::AuthClient;
use super::parse::{decode, RawMessage};
use serde::Deserialize;

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path, query_param};
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
    async fn search_returns_thread_summaries() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/gmail/v1/users/me/threads"))
            .and(query_param("q", "from:alice"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "threads": [
                    {"id":"t1","historyId":"1","snippet":"hi"},
                    {"id":"t2","historyId":"2","snippet":"bye"}
                ],
                "nextPageToken": "NEXT"
            })))
            .mount(&server).await;
        // Each thread.get fetch:
        for tid in &["t1", "t2"] {
            Mock::given(method("GET"))
                .and(path(format!("/gmail/v1/users/me/threads/{tid}")))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "id": tid,
                    "messages": [{
                        "id": format!("m-{tid}"),
                        "threadId": tid,
                        "labelIds": ["INBOX","UNREAD"],
                        "snippet": "snip",
                        "internalDate": "1714579200000",
                        "payload": {
                            "mimeType":"text/plain",
                            "headers":[
                                {"name":"From","value":"Alice <alice@example.com>"},
                                {"name":"To","value":"me@example.com"},
                                {"name":"Subject","value":format!("Subject {tid}")},
                                {"name":"Date","value":"Wed, 01 May 2024 12:00:00 +0000"}
                            ],
                            "body":{"size":5, "data":"aGVsbG8"}
                        }
                    }]
                })))
                .mount(&server).await;
        }

        let c = auth(&server);
        let base = format!("{}/gmail/v1", server.uri());
        let q = SearchQuery { text: Some("from:alice".into()), limit: Some(10), ..Default::default() };
        let res = search_impl(&c, &base, &q).await.unwrap();
        assert_eq!(res.threads.len(), 2);
        assert_eq!(res.threads[0].subject, "Subject t1");
        assert_eq!(res.next_cursor.as_deref(), Some("NEXT"));
    }
}

#[derive(Deserialize)]
struct ThreadsList {
    #[serde(default)]
    threads: Vec<ThreadStub>,
    #[serde(rename = "nextPageToken", default)]
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
struct ThreadStub {
    id: String,
}

#[derive(Deserialize)]
struct ThreadFull {
    #[allow(dead_code)]
    id: String,
    messages: Vec<RawMessage>,
}

pub async fn search_impl(
    client: &AuthClient,
    base: &str,
    q: &SearchQuery,
) -> Result<SearchResults> {
    let mut url = format!("{base}/users/me/threads");
    let mut sep = '?';
    if let Some(t) = &q.text {
        url.push(sep);
        url.push_str("q=");
        url.push_str(&urlencode(t));
        sep = '&';
    }
    if let Some(label) = &q.label {
        url.push(sep);
        url.push_str("labelIds=");
        url.push_str(label.as_str());
        sep = '&';
    }
    if let Some(folder) = &q.folder {
        url.push(sep);
        url.push_str("labelIds=");
        url.push_str(folder.as_str());
        sep = '&';
    }
    if let Some(limit) = q.limit {
        url.push(sep);
        url.push_str("maxResults=");
        url.push_str(&limit.to_string());
        sep = '&';
    }
    if let Some(cursor) = &q.cursor {
        url.push(sep);
        url.push_str("pageToken=");
        url.push_str(cursor);
    }
    let list: ThreadsList = client.get(&url).await?.error_for_status()?.json().await?;
    let mut summaries = Vec::with_capacity(list.threads.len());
    for stub in list.threads {
        let thread = get_thread_full(client, base, &ThreadId::from(stub.id)).await?;
        let last = thread.messages.last().cloned().ok_or_else(|| {
            crate::error::Error::Provider("thread has no messages".into())
        })?;
        summaries.push(ThreadSummary {
            id: thread.id.clone(),
            last_message_id: last.id.clone(),
            subject: thread.subject.clone(),
            snippet: last.snippet.clone(),
            from: last.from.clone(),
            date: last.date,
            message_count: thread.messages.len() as u32,
            unread: !last.flags.read,
            starred: last.flags.starred,
            labels: last.labels.clone(),
            folder: last.folder.clone(),
        });
    }
    Ok(SearchResults {
        threads: summaries,
        next_cursor: list.next_page_token,
    })
}

pub async fn get_thread_full(client: &AuthClient, base: &str, id: &ThreadId) -> Result<Thread> {
    let url = format!("{base}/users/me/threads/{}", id.as_str());
    let raw: ThreadFull = client.get(&url).await?.error_for_status()?.json().await?;
    let mut messages = Vec::with_capacity(raw.messages.len());
    for m in raw.messages {
        messages.push(decode(m)?);
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

pub async fn get_message_impl(client: &AuthClient, base: &str, id: &MessageId) -> Result<crate::providers::types::Message> {
    let url = format!("{base}/users/me/messages/{}?format=full", id.as_str());
    let raw: RawMessage = client.get(&url).await?.error_for_status()?.json().await?;
    decode(raw)
}

fn urlencode(s: &str) -> String {
    use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
    utf8_percent_encode(s, NON_ALPHANUMERIC).to_string()
}
