//! Decoding raw Gmail message JSON into our domain `Message` type.

use crate::providers::types::{AttachmentMeta, EmailAddress, Message, MessageFlags};
use crate::types::{LabelId, MessageId, ThreadId};
use base64::Engine;
use serde::Deserialize;

#[cfg(test)]
mod tests {
    use super::*;

    fn raw_msg() -> serde_json::Value {
        serde_json::json!({
            "id": "m1",
            "threadId": "t1",
            "labelIds": ["INBOX","UNREAD"],
            "snippet": "hi there",
            "internalDate": "1714579200000",
            "payload": {
                "mimeType": "multipart/alternative",
                "headers": [
                    {"name":"From","value":"Alice <alice@example.com>"},
                    {"name":"To","value":"bob@example.com"},
                    {"name":"Subject","value":"Hello"},
                    {"name":"Date","value":"Wed, 01 May 2024 12:00:00 +0000"}
                ],
                "parts": [
                    {
                        "mimeType":"text/plain",
                        "body":{"size":5, "data":"aGVsbG8"}
                    },
                    {
                        "mimeType":"text/html",
                        "body":{"size":12, "data":"PGI-aGVsbG88L2I-"}
                    }
                ]
            }
        })
    }

    #[test]
    fn parses_subject_and_addresses() {
        let raw: RawMessage = serde_json::from_value(raw_msg()).unwrap();
        let m = decode(raw).unwrap();
        assert_eq!(m.subject, "Hello");
        assert_eq!(m.from.unwrap().email, "alice@example.com");
        assert_eq!(m.to[0].email, "bob@example.com");
        assert_eq!(m.body_text.as_deref(), Some("hello"));
        assert_eq!(m.body_html.as_deref(), Some("<b>hello</b>"));
        assert!(!m.flags.read); // because UNREAD label is present
        assert_eq!(m.id.as_str(), "m1");
        assert_eq!(m.thread_id.as_str(), "t1");
    }
}

#[derive(Deserialize)]
pub struct RawMessage {
    pub id: String,
    #[serde(rename = "threadId")]
    pub thread_id: String,
    #[serde(rename = "labelIds", default)]
    pub label_ids: Vec<String>,
    #[serde(default)]
    pub snippet: String,
    #[serde(rename = "internalDate", default)]
    pub internal_date: String,
    #[serde(default)]
    pub payload: Option<RawPart>,
}

#[derive(Deserialize)]
pub struct RawPart {
    #[serde(rename = "mimeType", default)]
    pub mime_type: String,
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub headers: Vec<RawHeader>,
    #[serde(default)]
    pub body: Option<RawBody>,
    #[serde(default)]
    pub parts: Vec<RawPart>,
}

#[derive(Deserialize)]
pub struct RawHeader {
    pub name: String,
    pub value: String,
}

#[derive(Deserialize)]
pub struct RawBody {
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub data: Option<String>,
    #[serde(default)]
    pub attachment_id: Option<String>,
}

pub fn decode(raw: RawMessage) -> Result<Message, crate::error::Error> {
    use crate::error::Error;

    let payload = raw
        .payload
        .ok_or_else(|| Error::Provider("message missing payload".into()))?;
    let headers: std::collections::HashMap<String, String> = payload
        .headers
        .iter()
        .map(|h| (h.name.to_ascii_lowercase(), h.value.clone()))
        .collect();
    let from = headers.get("from").map(|s| parse_address(s));
    let to = headers
        .get("to")
        .map(|s| parse_address_list(s))
        .unwrap_or_default();
    let cc = headers
        .get("cc")
        .map(|s| parse_address_list(s))
        .unwrap_or_default();
    let bcc = headers
        .get("bcc")
        .map(|s| parse_address_list(s))
        .unwrap_or_default();
    let subject = headers.get("subject").cloned().unwrap_or_default();
    let date = parse_date(headers.get("date").map(String::as_str), &raw.internal_date);

    let mut body_text = None;
    let mut body_html = None;
    let mut attachments = Vec::new();
    walk_parts(&payload, &mut body_text, &mut body_html, &mut attachments);

    let labels = raw
        .label_ids
        .iter()
        .filter(|id| !is_system_label(id))
        .map(|s| LabelId::from(s.clone()))
        .collect();

    let flags = MessageFlags {
        read: !raw.label_ids.iter().any(|s| s == "UNREAD"),
        starred: raw.label_ids.iter().any(|s| s == "STARRED"),
        draft: raw.label_ids.iter().any(|s| s == "DRAFT"),
    };

    Ok(Message {
        id: MessageId::from(raw.id),
        thread_id: ThreadId::from(raw.thread_id),
        from,
        to,
        cc,
        bcc,
        subject,
        date,
        body_text,
        body_html,
        labels,
        folder: primary_folder(&raw.label_ids),
        flags,
        attachments,
        snippet: raw.snippet,
    })
}

fn walk_parts(
    part: &RawPart,
    body_text: &mut Option<String>,
    body_html: &mut Option<String>,
    attachments: &mut Vec<AttachmentMeta>,
) {
    let mime = part.mime_type.to_ascii_lowercase();
    if mime == "text/plain" && body_text.is_none() {
        *body_text = part.body.as_ref().and_then(decode_body);
    } else if mime == "text/html" && body_html.is_none() {
        *body_html = part.body.as_ref().and_then(decode_body);
    } else if !part.filename.is_empty() {
        if let Some(body) = &part.body {
            attachments.push(AttachmentMeta {
                id: body.attachment_id.clone().unwrap_or_default(),
                filename: part.filename.clone(),
                mime_type: part.mime_type.clone(),
                size_bytes: body.size,
            });
        }
    }
    for sub in &part.parts {
        walk_parts(sub, body_text, body_html, attachments);
    }
}

fn decode_body(b: &RawBody) -> Option<String> {
    let data = b.data.as_ref()?;
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(data.as_bytes())
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
}

fn parse_address(raw: &str) -> EmailAddress {
    // Very small "Display Name <email>" parser — Gmail returns headers in this form.
    if let Some(open) = raw.rfind('<') {
        if let Some(close) = raw[open..].find('>') {
            let email = raw[open + 1..open + close].trim().to_string();
            let name = raw[..open].trim().trim_matches('"').trim().to_string();
            return EmailAddress {
                email,
                name: if name.is_empty() { None } else { Some(name) },
            };
        }
    }
    EmailAddress {
        email: raw.trim().to_string(),
        name: None,
    }
}

fn parse_address_list(raw: &str) -> Vec<EmailAddress> {
    raw.split(',').map(parse_address).collect()
}

fn parse_date(date_header: Option<&str>, internal_ms: &str) -> chrono::DateTime<chrono::Utc> {
    if let Some(h) = date_header {
        if let Ok(parsed) = chrono::DateTime::parse_from_rfc2822(h) {
            return parsed.with_timezone(&chrono::Utc);
        }
    }
    if let Ok(ms) = internal_ms.parse::<i64>() {
        if let Some(d) = chrono::DateTime::from_timestamp_millis(ms) {
            return d;
        }
    }
    chrono::Utc::now()
}

fn is_system_label(id: &str) -> bool {
    matches!(
        id,
        "INBOX"
            | "SENT"
            | "TRASH"
            | "DRAFT"
            | "SPAM"
            | "STARRED"
            | "UNREAD"
            | "IMPORTANT"
            | "CHAT"
            | "CATEGORY_PERSONAL"
            | "CATEGORY_SOCIAL"
            | "CATEGORY_PROMOTIONS"
            | "CATEGORY_UPDATES"
            | "CATEGORY_FORUMS"
    )
}

fn primary_folder(label_ids: &[String]) -> Option<crate::types::FolderId> {
    for sys in &["INBOX", "SENT", "TRASH", "DRAFT", "SPAM"] {
        if label_ids.iter().any(|id| id == sys) {
            return Some(crate::types::FolderId::from(*sys));
        }
    }
    None
}
