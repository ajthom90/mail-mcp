//! Decoding raw Microsoft Graph message JSON into our domain `Message` type.
//!
//! Graph's mail resource is shaped quite differently from Gmail's — addresses
//! arrive pre-parsed (no header-string parsing required), bodies arrive in a
//! single `body.content` field rather than walked MIME parts, and "labels"
//! are surfaced as `categories`. Attachments aren't included in the message
//! payload; fetching them needs a separate `/messages/{id}/attachments` call,
//! so `Message::attachments` stays empty here and is filled in later when /
//! if a caller asks for them.

use crate::error::Error;
use crate::providers::types::{EmailAddress, Message, MessageFlags};
use crate::types::{LabelId, MessageId, ThreadId};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct RawMessage {
    pub id: String,
    #[serde(rename = "conversationId")]
    pub conversation_id: String,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(rename = "bodyPreview", default)]
    pub body_preview: Option<String>,
    #[serde(default)]
    pub from: Option<RawRecipient>,
    #[serde(rename = "toRecipients", default)]
    pub to_recipients: Vec<RawRecipient>,
    #[serde(rename = "ccRecipients", default)]
    pub cc_recipients: Vec<RawRecipient>,
    #[serde(rename = "bccRecipients", default)]
    pub bcc_recipients: Vec<RawRecipient>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(rename = "isRead", default)]
    pub is_read: bool,
    #[serde(default)]
    pub flag: Option<RawFlag>,
    #[serde(rename = "isDraft", default)]
    pub is_draft: bool,
    #[serde(rename = "parentFolderId", default)]
    pub parent_folder_id: Option<String>,
    #[serde(rename = "receivedDateTime", default)]
    pub received_date_time: Option<String>,
    #[serde(default)]
    pub body: Option<RawBody>,
}

#[derive(Deserialize)]
pub struct RawRecipient {
    #[serde(rename = "emailAddress", default)]
    pub email_address: Option<RawAddress>,
}

#[derive(Deserialize)]
pub struct RawAddress {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub address: Option<String>,
}

#[derive(Deserialize)]
pub struct RawFlag {
    #[serde(rename = "flagStatus", default)]
    pub flag_status: Option<String>,
}

#[derive(Deserialize)]
pub struct RawBody {
    #[serde(rename = "contentType", default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
}

pub fn decode(raw: RawMessage) -> Result<Message, Error> {
    let from = raw.from.as_ref().and_then(to_email_address);
    let to: Vec<_> = raw.to_recipients.iter().filter_map(to_email_address).collect();
    let cc: Vec<_> = raw.cc_recipients.iter().filter_map(to_email_address).collect();
    let bcc: Vec<_> = raw.bcc_recipients.iter().filter_map(to_email_address).collect();

    let date = raw
        .received_date_time
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .ok_or_else(|| Error::Provider("message missing receivedDateTime".into()))?;

    let (body_text, body_html) = match raw.body {
        Some(RawBody { content_type, content: Some(c), .. }) => {
            match content_type.as_deref() {
                Some("html") | Some("Html") | Some("HTML") => (None, Some(c)),
                _ => (Some(c), None),
            }
        }
        _ => (None, None),
    };

    let labels: Vec<LabelId> = raw.categories.into_iter().map(LabelId::from).collect();

    let starred = matches!(
        raw.flag.as_ref().and_then(|f| f.flag_status.as_deref()),
        Some("flagged") | Some("complete")
    );

    let flags = MessageFlags {
        read: raw.is_read,
        starred,
        draft: raw.is_draft,
    };

    Ok(Message {
        id: MessageId::from(raw.id),
        thread_id: ThreadId::from(raw.conversation_id),
        from,
        to,
        cc,
        bcc,
        subject: raw.subject.unwrap_or_default(),
        date,
        body_text,
        body_html,
        labels,
        folder: raw.parent_folder_id.map(crate::types::FolderId::from),
        flags,
        attachments: Vec::new(),
        snippet: raw.body_preview.unwrap_or_default(),
    })
}

fn to_email_address(r: &RawRecipient) -> Option<EmailAddress> {
    let addr = r.email_address.as_ref()?;
    let email = addr.address.as_ref()?.clone();
    Some(EmailAddress {
        email,
        name: addr.name.clone().filter(|s| !s.is_empty()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw_msg() -> serde_json::Value {
        serde_json::json!({
            "id": "AAMkAD-msgid",
            "conversationId": "AAQkAD-convid",
            "subject": "Hello from Graph",
            "bodyPreview": "Quick preview",
            "from": { "emailAddress": { "name": "Alice", "address": "alice@example.com" } },
            "toRecipients": [
                { "emailAddress": { "name": "Bob", "address": "bob@example.com" } }
            ],
            "ccRecipients": [],
            "bccRecipients": [],
            "categories": ["Followups", "VIP"],
            "isRead": false,
            "flag": { "flagStatus": "flagged" },
            "isDraft": false,
            "parentFolderId": "INBOXFOLDERID",
            "receivedDateTime": "2026-05-01T12:00:00Z",
            "body": { "contentType": "html", "content": "<p>Hello!</p>" }
        })
    }

    #[test]
    fn parses_subject_and_addresses() {
        let raw: RawMessage = serde_json::from_value(raw_msg()).unwrap();
        let m = decode(raw).unwrap();
        assert_eq!(m.subject, "Hello from Graph");
        assert_eq!(m.from.as_ref().unwrap().email, "alice@example.com");
        assert_eq!(m.from.as_ref().unwrap().name.as_deref(), Some("Alice"));
        assert_eq!(m.to.len(), 1);
        assert_eq!(m.to[0].email, "bob@example.com");
        assert_eq!(m.id.as_str(), "AAMkAD-msgid");
        assert_eq!(m.thread_id.as_str(), "AAQkAD-convid");
    }

    #[test]
    fn parses_body_html_when_content_type_html() {
        let raw: RawMessage = serde_json::from_value(raw_msg()).unwrap();
        let m = decode(raw).unwrap();
        assert_eq!(m.body_html.as_deref(), Some("<p>Hello!</p>"));
        assert!(m.body_text.is_none());
    }

    #[test]
    fn parses_body_text_when_content_type_text() {
        let mut v = raw_msg();
        v["body"]["contentType"] = serde_json::json!("text");
        v["body"]["content"] = serde_json::json!("plain hello");
        let raw: RawMessage = serde_json::from_value(v).unwrap();
        let m = decode(raw).unwrap();
        assert_eq!(m.body_text.as_deref(), Some("plain hello"));
        assert!(m.body_html.is_none());
    }

    #[test]
    fn flagged_status_marks_starred() {
        let raw: RawMessage = serde_json::from_value(raw_msg()).unwrap();
        let m = decode(raw).unwrap();
        assert!(m.flags.starred);
        assert!(!m.flags.read);
        assert!(!m.flags.draft);
    }

    #[test]
    fn not_flagged_status_unstars() {
        let mut v = raw_msg();
        v["flag"]["flagStatus"] = serde_json::json!("notFlagged");
        let raw: RawMessage = serde_json::from_value(v).unwrap();
        let m = decode(raw).unwrap();
        assert!(!m.flags.starred);
    }

    #[test]
    fn categories_become_labels() {
        let raw: RawMessage = serde_json::from_value(raw_msg()).unwrap();
        let m = decode(raw).unwrap();
        let labels: Vec<&str> = m.labels.iter().map(|l| l.as_str()).collect();
        assert!(labels.contains(&"Followups"));
        assert!(labels.contains(&"VIP"));
    }

    #[test]
    fn snippet_uses_body_preview() {
        let raw: RawMessage = serde_json::from_value(raw_msg()).unwrap();
        let m = decode(raw).unwrap();
        assert_eq!(m.snippet, "Quick preview");
    }

    #[test]
    fn missing_received_date_returns_provider_error() {
        let mut v = raw_msg();
        v.as_object_mut().unwrap().remove("receivedDateTime");
        let raw: RawMessage = serde_json::from_value(v).unwrap();
        let err = decode(raw).unwrap_err();
        assert!(format!("{err:?}").contains("receivedDateTime"));
    }

    #[test]
    fn empty_name_drops_to_none() {
        let mut v = raw_msg();
        v["from"]["emailAddress"]["name"] = serde_json::json!("");
        let raw: RawMessage = serde_json::from_value(v).unwrap();
        let m = decode(raw).unwrap();
        assert_eq!(m.from.unwrap().name, None);
    }
}
