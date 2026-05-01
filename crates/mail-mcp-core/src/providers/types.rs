use crate::types::{DraftId, FolderId, LabelId, MessageId, ThreadId};
use serde::{Deserialize, Serialize};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_query_serializes_with_optional_fields() {
        let q = SearchQuery {
            text: Some("hello".into()),
            folder: None,
            label: None,
            limit: Some(20),
            cursor: None,
        };
        let s = serde_json::to_string(&q).unwrap();
        assert!(s.contains("\"text\":\"hello\""));
        assert!(s.contains("\"limit\":20"));
        assert!(!s.contains("folder"));
    }

    #[test]
    fn message_round_trips_via_json() {
        let m = Message {
            id: crate::types::MessageId::from("m-1"),
            thread_id: crate::types::ThreadId::from("t-1"),
            from: Some(EmailAddress {
                name: Some("A".into()),
                email: "a@x".into(),
            }),
            to: vec![EmailAddress {
                name: None,
                email: "b@x".into(),
            }],
            cc: vec![],
            bcc: vec![],
            subject: "hi".into(),
            date: chrono::Utc::now(),
            body_text: Some("hello".into()),
            body_html: None,
            labels: vec![],
            folder: None,
            flags: MessageFlags {
                read: true,
                starred: false,
                draft: false,
            },
            attachments: vec![],
            snippet: "hi there".into(),
        };
        let s = serde_json::to_string(&m).unwrap();
        let _: Message = serde_json::from_str(&s).unwrap();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailAddress {
    pub email: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchQuery {
    /// Free-text query in provider-native syntax (Gmail's search operators, M365's $search,
    /// or IMAP X-GM-RAW / SEARCH terms). Documented in the MCP tool description.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub folder: Option<FolderId>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub label: Option<LabelId>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub limit: Option<u32>,
    /// Opaque pagination cursor returned by the provider; pass back to fetch the next page.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadSummary {
    pub id: ThreadId,
    pub last_message_id: MessageId,
    pub subject: String,
    pub snippet: String,
    pub from: Option<EmailAddress>,
    pub date: chrono::DateTime<chrono::Utc>,
    pub message_count: u32,
    pub unread: bool,
    pub starred: bool,
    pub labels: Vec<LabelId>,
    pub folder: Option<FolderId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thread {
    pub id: ThreadId,
    pub subject: String,
    pub messages: Vec<Message>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MessageFlags {
    pub read: bool,
    pub starred: bool,
    pub draft: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: MessageId,
    pub thread_id: ThreadId,
    pub from: Option<EmailAddress>,
    #[serde(default)]
    pub to: Vec<EmailAddress>,
    #[serde(default)]
    pub cc: Vec<EmailAddress>,
    #[serde(default)]
    pub bcc: Vec<EmailAddress>,
    pub subject: String,
    pub date: chrono::DateTime<chrono::Utc>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub body_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub body_html: Option<String>,
    #[serde(default)]
    pub labels: Vec<LabelId>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub folder: Option<FolderId>,
    #[serde(default)]
    pub flags: MessageFlags,
    #[serde(default)]
    pub attachments: Vec<AttachmentMeta>,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentMeta {
    pub id: String,
    pub filename: String,
    pub mime_type: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DraftSummary {
    pub id: DraftId,
    pub message_id: MessageId,
    pub subject: String,
    pub snippet: String,
    pub date: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DraftInput {
    pub to: Vec<EmailAddress>,
    #[serde(default)]
    pub cc: Vec<EmailAddress>,
    #[serde(default)]
    pub bcc: Vec<EmailAddress>,
    pub subject: String,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
    /// If set, this draft is a reply to / forward of an existing message. Provider
    /// implementations may use this to set In-Reply-To / References headers.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub in_reply_to: Option<MessageId>,
    #[serde(default)]
    pub thread_id: Option<ThreadId>,
}

/// An outgoing message that bypasses drafts. (Used for direct send paths.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutgoingMessage {
    pub to: Vec<EmailAddress>,
    #[serde(default)]
    pub cc: Vec<EmailAddress>,
    #[serde(default)]
    pub bcc: Vec<EmailAddress>,
    pub subject: String,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub in_reply_to: Option<MessageId>,
    #[serde(default)]
    pub thread_id: Option<ThreadId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Folder {
    pub id: FolderId,
    pub name: String,
    /// True for system folders that should not be renamed/deleted (INBOX, SENT, TRASH).
    pub system: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Label {
    pub id: LabelId,
    pub name: String,
    pub system: bool,
}
