use crate::mcp::tools::ToolSpec;
use mail_mcp_core::error::Error as CoreError;
use mail_mcp_core::permissions::approvals::{ApprovalQueue, ApprovalRequest};
use mail_mcp_core::permissions::enforce::{enforce, EnforceOutcome, SessionTrust};
use mail_mcp_core::permissions::Permissions;
use mail_mcp_core::providers::r#trait::MailProvider;
use mail_mcp_core::providers::types::*;
use mail_mcp_core::storage::Storage;
use mail_mcp_core::types::{AccountId, DraftId, FolderId, LabelId, MessageId, ThreadId};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

pub struct DispatchContext {
    pub storage: Storage,
    pub providers: ProviderRegistry,
    pub approvals: ApprovalQueue,
    pub trust: SessionTrust,
    pub mcp_paused: Arc<std::sync::atomic::AtomicBool>,
}

#[derive(Default, Clone)]
pub struct ProviderRegistry {
    map: Arc<tokio::sync::RwLock<HashMap<AccountId, Arc<dyn MailProvider>>>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn insert(&self, id: AccountId, provider: Arc<dyn MailProvider>) {
        self.map.write().await.insert(id, provider);
    }

    pub async fn remove(&self, id: AccountId) {
        self.map.write().await.remove(&id);
    }

    pub async fn get(&self, id: AccountId) -> Option<Arc<dyn MailProvider>> {
        self.map.read().await.get(&id).cloned()
    }
}

pub async fn dispatch(
    ctx: &DispatchContext,
    tool: &ToolSpec,
    args: Value,
) -> Result<Value, CoreError> {
    if ctx.mcp_paused.load(std::sync::atomic::Ordering::Relaxed) {
        return Err(CoreError::PermissionDenied(
            "MCP is paused — open mail-mcp Settings to resume.".into(),
        ));
    }

    let account_id = parse_account_id(&args)?;
    let provider = ctx
        .providers
        .get(account_id)
        .await
        .ok_or_else(|| CoreError::NotFound(format!("account {account_id}")))?;
    let perms = Permissions::for_account(&ctx.storage, account_id).await?;
    let summary = tool.name.to_string();
    let request = ApprovalRequest {
        account: account_id,
        category: tool.category,
        summary,
        details: args.clone(),
    };
    let outcome = enforce(&perms, &ctx.approvals, &ctx.trust, tool.category, request).await?;

    match outcome {
        EnforceOutcome::Blocked => Err(CoreError::PermissionDenied(format!(
            "{}: blocked by user policy",
            tool.name
        ))),
        EnforceOutcome::ConvertToDraft => convert_send_to_draft(&*provider, tool.name, args).await,
        EnforceOutcome::Proceed => execute(&*provider, tool.name, args).await,
    }
}

fn parse_account_id(args: &Value) -> Result<AccountId, CoreError> {
    let s = args
        .get("account_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| CoreError::Provider("missing account_id".into()))?;
    AccountId::from_str(s).map_err(|e| CoreError::Provider(format!("bad account_id: {e}")))
}

#[derive(Deserialize)]
struct SendArgs {
    account_id: String,
    to: Vec<String>,
    #[serde(default)]
    cc: Vec<String>,
    #[serde(default)]
    bcc: Vec<String>,
    subject: String,
    body_text: Option<String>,
    body_html: Option<String>,
    in_reply_to: Option<String>,
    thread_id: Option<String>,
}

fn addrs(v: &[String]) -> Vec<EmailAddress> {
    v.iter()
        .map(|s| EmailAddress {
            email: s.clone(),
            name: None,
        })
        .collect()
}

async fn convert_send_to_draft(
    provider: &dyn MailProvider,
    tool: &str,
    args: Value,
) -> Result<Value, CoreError> {
    let s: SendArgs = serde_json::from_value(args.clone())
        .map_err(|e| CoreError::Provider(format!("bad send args: {e}")))?;
    let _ = (tool, s.account_id); // tool param not needed in conversion
    let input = DraftInput {
        to: addrs(&s.to),
        cc: addrs(&s.cc),
        bcc: addrs(&s.bcc),
        subject: s.subject,
        body_text: s.body_text,
        body_html: s.body_html,
        in_reply_to: s.in_reply_to.map(MessageId::from),
        thread_id: s.thread_id.map(ThreadId::from),
    };
    let id = provider.create_draft(&input).await?;
    Ok(serde_json::json!({
        "result": "draft_created",
        "draft_id": id,
        "note": "Send was converted to a draft per user policy. Please review and send from your mail client."
    }))
}

async fn execute(provider: &dyn MailProvider, tool: &str, args: Value) -> Result<Value, CoreError> {
    match tool {
        "search" => {
            let q = SearchQuery {
                text: args.get("query").and_then(|v| v.as_str()).map(String::from),
                folder: args
                    .get("folder_id")
                    .and_then(|v| v.as_str())
                    .map(FolderId::from),
                label: args
                    .get("label_id")
                    .and_then(|v| v.as_str())
                    .map(LabelId::from),
                limit: args.get("limit").and_then(|v| v.as_u64()).map(|n| n as u32),
                cursor: args
                    .get("cursor")
                    .and_then(|v| v.as_str())
                    .map(String::from),
            };
            let res = provider.search(&q).await?;
            Ok(serde_json::to_value(res)?)
        }
        "get_thread" => {
            let id = ThreadId::from(
                args.get("thread_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default(),
            );
            Ok(serde_json::to_value(provider.get_thread(&id).await?)?)
        }
        "get_message" => {
            let id = MessageId::from(
                args.get("message_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default(),
            );
            Ok(serde_json::to_value(provider.get_message(&id).await?)?)
        }
        "list_folders" => Ok(serde_json::to_value(provider.list_folders().await?)?),
        "list_labels" => Ok(serde_json::to_value(provider.list_labels().await?)?),
        "list_drafts" => Ok(serde_json::to_value(provider.list_drafts().await?)?),
        "mark_read" => {
            let ids: Vec<MessageId> = args
                .get("message_ids")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|s| s.as_str())
                        .map(MessageId::from)
                        .collect()
                })
                .unwrap_or_default();
            let read = args.get("read").and_then(|v| v.as_bool()).unwrap_or(true);
            provider.mark_read(&ids, read).await?;
            Ok(serde_json::json!({"ok": true}))
        }
        "star" => {
            let ids: Vec<MessageId> = args
                .get("message_ids")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|s| s.as_str())
                        .map(MessageId::from)
                        .collect()
                })
                .unwrap_or_default();
            let on = args
                .get("starred")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            provider.star(&ids, on).await?;
            Ok(serde_json::json!({"ok": true}))
        }
        "label" => {
            let ids: Vec<MessageId> = args
                .get("message_ids")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|s| s.as_str())
                        .map(MessageId::from)
                        .collect()
                })
                .unwrap_or_default();
            let label = LabelId::from(
                args.get("label_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default(),
            );
            let on = args.get("on").and_then(|v| v.as_bool()).unwrap_or(true);
            provider.label(&ids, &label, on).await?;
            Ok(serde_json::json!({"ok": true}))
        }
        "move_to" => {
            let ids: Vec<MessageId> = args
                .get("message_ids")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|s| s.as_str())
                        .map(MessageId::from)
                        .collect()
                })
                .unwrap_or_default();
            let folder = FolderId::from(
                args.get("folder_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default(),
            );
            provider.move_to(&ids, &folder).await?;
            Ok(serde_json::json!({"ok": true}))
        }
        "archive" => {
            let ids: Vec<MessageId> = args
                .get("message_ids")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|s| s.as_str())
                        .map(MessageId::from)
                        .collect()
                })
                .unwrap_or_default();
            provider.archive(&ids).await?;
            Ok(serde_json::json!({"ok": true}))
        }
        "trash" => {
            let ids: Vec<MessageId> = args
                .get("message_ids")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|s| s.as_str())
                        .map(MessageId::from)
                        .collect()
                })
                .unwrap_or_default();
            provider.trash(&ids).await?;
            Ok(serde_json::json!({"ok": true}))
        }
        "untrash" => {
            let ids: Vec<MessageId> = args
                .get("message_ids")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|s| s.as_str())
                        .map(MessageId::from)
                        .collect()
                })
                .unwrap_or_default();
            provider.untrash(&ids).await?;
            Ok(serde_json::json!({"ok": true}))
        }
        "create_draft" => {
            let s: SendArgs = serde_json::from_value(args)
                .map_err(|e| CoreError::Provider(format!("bad args: {e}")))?;
            let input = DraftInput {
                to: addrs(&s.to),
                cc: addrs(&s.cc),
                bcc: addrs(&s.bcc),
                subject: s.subject,
                body_text: s.body_text,
                body_html: s.body_html,
                in_reply_to: s.in_reply_to.map(MessageId::from),
                thread_id: s.thread_id.map(ThreadId::from),
            };
            let id = provider.create_draft(&input).await?;
            Ok(serde_json::json!({"draft_id": id}))
        }
        "update_draft" => {
            #[derive(Deserialize)]
            struct UpdateArgs {
                draft_id: String,
                to: Vec<String>,
                #[serde(default)]
                cc: Vec<String>,
                #[serde(default)]
                bcc: Vec<String>,
                subject: String,
                body_text: Option<String>,
                body_html: Option<String>,
            }
            let s: UpdateArgs = serde_json::from_value(args)
                .map_err(|e| CoreError::Provider(format!("bad args: {e}")))?;
            let input = DraftInput {
                to: addrs(&s.to),
                cc: addrs(&s.cc),
                bcc: addrs(&s.bcc),
                subject: s.subject,
                body_text: s.body_text,
                body_html: s.body_html,
                in_reply_to: None,
                thread_id: None,
            };
            provider
                .update_draft(&DraftId::from(s.draft_id), &input)
                .await?;
            Ok(serde_json::json!({"ok": true}))
        }
        "send_message" => {
            let s: SendArgs = serde_json::from_value(args)
                .map_err(|e| CoreError::Provider(format!("bad args: {e}")))?;
            let m = OutgoingMessage {
                to: addrs(&s.to),
                cc: addrs(&s.cc),
                bcc: addrs(&s.bcc),
                subject: s.subject,
                body_text: s.body_text,
                body_html: s.body_html,
                in_reply_to: s.in_reply_to.map(MessageId::from),
                thread_id: s.thread_id.map(ThreadId::from),
            };
            let id = provider.send_message(&m).await?;
            Ok(serde_json::json!({"message_id": id}))
        }
        "send_draft" => {
            let did = DraftId::from(
                args.get("draft_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default(),
            );
            let id = provider.send_draft(&did).await?;
            Ok(serde_json::json!({"message_id": id}))
        }
        "list_accounts" => {
            // dispatched separately by the caller (no provider needed)
            unreachable!("list_accounts should be handled before dispatch")
        }
        other => Err(CoreError::NotFound(format!("tool {other}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use mail_mcp_core::accounts::{AccountStore, NewAccount};
    use mail_mcp_core::types::ProviderKind;

    struct FakeProvider {
        last_call: tokio::sync::Mutex<Option<String>>,
    }
    #[async_trait]
    impl MailProvider for FakeProvider {
        async fn search(
            &self,
            _q: &SearchQuery,
        ) -> Result<mail_mcp_core::providers::r#trait::SearchResults, CoreError> {
            *self.last_call.lock().await = Some("search".into());
            Ok(mail_mcp_core::providers::r#trait::SearchResults {
                threads: vec![],
                next_cursor: None,
            })
        }
        async fn get_thread(&self, _id: &ThreadId) -> Result<Thread, CoreError> {
            unimplemented!()
        }
        async fn get_message(&self, _id: &MessageId) -> Result<Message, CoreError> {
            unimplemented!()
        }
        async fn list_folders(&self) -> Result<Vec<Folder>, CoreError> {
            Ok(vec![])
        }
        async fn list_labels(&self) -> Result<Vec<Label>, CoreError> {
            Ok(vec![])
        }
        async fn list_drafts(&self) -> Result<Vec<DraftSummary>, CoreError> {
            Ok(vec![])
        }
        async fn mark_read(&self, _ids: &[MessageId], _r: bool) -> Result<(), CoreError> {
            Ok(())
        }
        async fn star(&self, _ids: &[MessageId], _s: bool) -> Result<(), CoreError> {
            Ok(())
        }
        async fn label(&self, _ids: &[MessageId], _l: &LabelId, _o: bool) -> Result<(), CoreError> {
            Ok(())
        }
        async fn move_to(&self, _ids: &[MessageId], _f: &FolderId) -> Result<(), CoreError> {
            Ok(())
        }
        async fn archive(&self, _ids: &[MessageId]) -> Result<(), CoreError> {
            Ok(())
        }
        async fn trash(&self, _ids: &[MessageId]) -> Result<(), CoreError> {
            Ok(())
        }
        async fn untrash(&self, _ids: &[MessageId]) -> Result<(), CoreError> {
            Ok(())
        }
        async fn create_draft(&self, _d: &DraftInput) -> Result<DraftId, CoreError> {
            *self.last_call.lock().await = Some("create_draft".into());
            Ok(DraftId::from("d-fake"))
        }
        async fn update_draft(&self, _id: &DraftId, _d: &DraftInput) -> Result<(), CoreError> {
            Ok(())
        }
        async fn send_message(&self, _m: &OutgoingMessage) -> Result<MessageId, CoreError> {
            *self.last_call.lock().await = Some("send_message".into());
            Ok(MessageId::from("m-fake"))
        }
        async fn send_draft(&self, _id: &DraftId) -> Result<MessageId, CoreError> {
            Ok(MessageId::from("m-fake"))
        }
    }

    async fn ctx() -> (DispatchContext, AccountId, Arc<FakeProvider>) {
        let tmp = tempfile::tempdir().unwrap();
        let storage = Storage::open(&tmp.path().join("s.db")).await.unwrap();
        std::mem::forget(tmp);
        let id = AccountStore::create(
            &storage,
            &NewAccount {
                label: "x".into(),
                provider: ProviderKind::Gmail,
                email: "x@x".into(),
                config: serde_json::json!({}),
                scopes: vec![],
            },
        )
        .await
        .unwrap();
        Permissions::install_defaults(&storage, id).await.unwrap();
        let providers = ProviderRegistry::new();
        let fake = Arc::new(FakeProvider {
            last_call: tokio::sync::Mutex::new(None),
        });
        providers.insert(id, fake.clone()).await;
        let ctx = DispatchContext {
            storage,
            providers,
            approvals: ApprovalQueue::new(std::time::Duration::from_secs(5)),
            trust: SessionTrust::new(),
            mcp_paused: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };
        (ctx, id, fake)
    }

    #[tokio::test]
    async fn search_proceeds_with_default_policy() {
        let (ctx, id, fake) = ctx().await;
        let tools = crate::mcp::tools::tools();
        let tool = tools.iter().find(|t| t.name == "search").unwrap();
        let args = serde_json::json!({"account_id": id.to_string(), "query": "alice"});
        dispatch(&ctx, tool, args).await.unwrap();
        assert_eq!(fake.last_call.lock().await.as_deref(), Some("search"));
    }

    #[tokio::test]
    async fn send_message_default_policy_converts_to_draft() {
        let (ctx, id, fake) = ctx().await;
        let tools = crate::mcp::tools::tools();
        let tool = tools.iter().find(|t| t.name == "send_message").unwrap();
        let args = serde_json::json!({
            "account_id": id.to_string(),
            "to": ["alice@example.com"],
            "subject": "Hi",
            "body_text": "hello"
        });
        let res = dispatch(&ctx, tool, args).await.unwrap();
        assert_eq!(res["result"], "draft_created");
        assert_eq!(fake.last_call.lock().await.as_deref(), Some("create_draft"));
    }
}
