//! Catalog of MCP tools exposed by the daemon. Each tool entry describes its name,
//! human description, JSON Schema for arguments, and which permissions Category it falls
//! under (used by the dispatch layer to apply the policy).

use mail_mcp_core::permissions::Category;
use serde::Serialize;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_include_send_message_in_send_category() {
        let t = tools().into_iter().find(|t| t.name == "send_message").unwrap();
        assert_eq!(t.category, Category::Send);
    }

    #[test]
    fn tools_include_search_in_read_category() {
        let t = tools().into_iter().find(|t| t.name == "search").unwrap();
        assert_eq!(t.category, Category::Read);
    }

    #[test]
    fn every_tool_has_a_schema_object() {
        for t in tools() {
            assert!(t.input_schema.is_object(), "tool {} schema is not an object", t.name);
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: serde_json::Value,
    pub category: Category,
}

/// All MCP tools exposed in v0.1a.
pub fn tools() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: "list_accounts",
            description: "List all connected mail accounts. Returns account ids, labels, providers, emails. Use this first to find an account_id before any other tool call.",
            input_schema: serde_json::json!({"type":"object","properties":{},"additionalProperties":false}),
            category: Category::Read,
        },
        ToolSpec {
            name: "search",
            description: "Search threads on a single account. The query string uses provider-native syntax (Gmail's search operators e.g. 'from:alice subject:invoice'). Returns thread summaries plus an opaque cursor for pagination.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "account_id": {"type":"string"},
                    "query": {"type":"string"},
                    "label_id": {"type":"string"},
                    "folder_id": {"type":"string"},
                    "limit": {"type":"integer", "minimum": 1, "maximum": 100, "default": 20},
                    "cursor": {"type":"string"}
                },
                "required": ["account_id"],
                "additionalProperties": false
            }),
            category: Category::Read,
        },
        ToolSpec {
            name: "get_thread",
            description: "Get a full mail thread (all messages, including bodies) by id.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {"account_id": {"type":"string"}, "thread_id": {"type":"string"}},
                "required": ["account_id","thread_id"],
                "additionalProperties": false
            }),
            category: Category::Read,
        },
        ToolSpec {
            name: "get_message",
            description: "Get a single message by id with full body and attachment metadata.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {"account_id": {"type":"string"}, "message_id": {"type":"string"}},
                "required": ["account_id","message_id"],
                "additionalProperties": false
            }),
            category: Category::Read,
        },
        ToolSpec {
            name: "list_folders",
            description: "List system folders. On Gmail these are the system labels (INBOX, SENT, TRASH, DRAFT, SPAM); on M365/IMAP these are real folders.",
            input_schema: serde_json::json!({"type":"object","properties":{"account_id":{"type":"string"}},"required":["account_id"],"additionalProperties":false}),
            category: Category::Read,
        },
        ToolSpec {
            name: "list_labels",
            description: "List user-created labels (Gmail) / categories (M365) / IMAP keywords. Excludes system folders, which are returned by list_folders.",
            input_schema: serde_json::json!({"type":"object","properties":{"account_id":{"type":"string"}},"required":["account_id"],"additionalProperties":false}),
            category: Category::Read,
        },
        ToolSpec {
            name: "list_drafts",
            description: "List existing drafts on the account.",
            input_schema: serde_json::json!({"type":"object","properties":{"account_id":{"type":"string"}},"required":["account_id"],"additionalProperties":false}),
            category: Category::Read,
        },
        ToolSpec {
            name: "mark_read",
            description: "Mark messages as read or unread.",
            input_schema: serde_json::json!({
                "type":"object",
                "properties": {
                    "account_id": {"type":"string"},
                    "message_ids": {"type":"array","items":{"type":"string"}},
                    "read": {"type":"boolean"}
                },
                "required":["account_id","message_ids","read"],
                "additionalProperties":false
            }),
            category: Category::Modify,
        },
        ToolSpec {
            name: "star",
            description: "Star or unstar messages.",
            input_schema: serde_json::json!({
                "type":"object",
                "properties": {
                    "account_id": {"type":"string"},
                    "message_ids": {"type":"array","items":{"type":"string"}},
                    "starred": {"type":"boolean"}
                },
                "required":["account_id","message_ids","starred"],
                "additionalProperties":false
            }),
            category: Category::Modify,
        },
        ToolSpec {
            name: "label",
            description: "Add or remove a label on messages. On Gmail this maps to label modify; on IMAP to keyword set/clear.",
            input_schema: serde_json::json!({
                "type":"object",
                "properties": {
                    "account_id": {"type":"string"},
                    "message_ids": {"type":"array","items":{"type":"string"}},
                    "label_id": {"type":"string"},
                    "on": {"type":"boolean"}
                },
                "required":["account_id","message_ids","label_id","on"],
                "additionalProperties":false
            }),
            category: Category::Modify,
        },
        ToolSpec {
            name: "move_to",
            description: "Move messages to a folder. On Gmail this re-labels (e.g., move from INBOX to TRASH); on IMAP/M365 it's a real folder move.",
            input_schema: serde_json::json!({
                "type":"object",
                "properties": {
                    "account_id": {"type":"string"},
                    "message_ids": {"type":"array","items":{"type":"string"}},
                    "folder_id": {"type":"string"}
                },
                "required":["account_id","message_ids","folder_id"],
                "additionalProperties":false
            }),
            category: Category::Modify,
        },
        ToolSpec {
            name: "archive",
            description: "Archive messages (Gmail: remove INBOX label; IMAP/M365: move to All Mail / Archive folder).",
            input_schema: serde_json::json!({
                "type":"object",
                "properties": {
                    "account_id": {"type":"string"},
                    "message_ids": {"type":"array","items":{"type":"string"}}
                },
                "required":["account_id","message_ids"],
                "additionalProperties":false
            }),
            category: Category::Modify,
        },
        ToolSpec {
            name: "trash",
            description: "Move messages to Trash. Reversible until the user empties Trash.",
            input_schema: serde_json::json!({
                "type":"object",
                "properties": {
                    "account_id": {"type":"string"},
                    "message_ids": {"type":"array","items":{"type":"string"}}
                },
                "required":["account_id","message_ids"],
                "additionalProperties":false
            }),
            category: Category::Trash,
        },
        ToolSpec {
            name: "untrash",
            description: "Restore messages from Trash.",
            input_schema: serde_json::json!({
                "type":"object",
                "properties": {
                    "account_id": {"type":"string"},
                    "message_ids": {"type":"array","items":{"type":"string"}}
                },
                "required":["account_id","message_ids"],
                "additionalProperties":false
            }),
            category: Category::Trash,
        },
        ToolSpec {
            name: "create_draft",
            description: "Create a new draft. The draft is saved on the provider's server and will be visible in the user's drafts folder.",
            input_schema: serde_json::json!({
                "type":"object",
                "properties": {
                    "account_id": {"type":"string"},
                    "to": {"type":"array","items":{"type":"string"}},
                    "cc": {"type":"array","items":{"type":"string"}},
                    "bcc": {"type":"array","items":{"type":"string"}},
                    "subject": {"type":"string"},
                    "body_text": {"type":"string"},
                    "body_html": {"type":"string"},
                    "in_reply_to": {"type":"string"},
                    "thread_id": {"type":"string"}
                },
                "required":["account_id","to","subject"],
                "additionalProperties":false
            }),
            category: Category::Draft,
        },
        ToolSpec {
            name: "update_draft",
            description: "Update an existing draft.",
            input_schema: serde_json::json!({
                "type":"object",
                "properties": {
                    "account_id": {"type":"string"},
                    "draft_id": {"type":"string"},
                    "to": {"type":"array","items":{"type":"string"}},
                    "cc": {"type":"array","items":{"type":"string"}},
                    "bcc": {"type":"array","items":{"type":"string"}},
                    "subject": {"type":"string"},
                    "body_text": {"type":"string"},
                    "body_html": {"type":"string"}
                },
                "required":["account_id","draft_id","to","subject"],
                "additionalProperties":false
            }),
            category: Category::Draft,
        },
        ToolSpec {
            name: "send_message",
            description: "Send a message directly. Subject to the user's send-policy (default: Convert to draft, in which case this returns a draft_created result instead of actually sending).",
            input_schema: serde_json::json!({
                "type":"object",
                "properties": {
                    "account_id": {"type":"string"},
                    "to": {"type":"array","items":{"type":"string"}},
                    "cc": {"type":"array","items":{"type":"string"}},
                    "bcc": {"type":"array","items":{"type":"string"}},
                    "subject": {"type":"string"},
                    "body_text": {"type":"string"},
                    "body_html": {"type":"string"},
                    "in_reply_to": {"type":"string"},
                    "thread_id": {"type":"string"}
                },
                "required":["account_id","to","subject"],
                "additionalProperties":false
            }),
            category: Category::Send,
        },
        ToolSpec {
            name: "send_draft",
            description: "Send an existing draft. Subject to send-policy like send_message.",
            input_schema: serde_json::json!({
                "type":"object",
                "properties": {
                    "account_id": {"type":"string"},
                    "draft_id": {"type":"string"}
                },
                "required":["account_id","draft_id"],
                "additionalProperties":false
            }),
            category: Category::Send,
        },
    ]
}
