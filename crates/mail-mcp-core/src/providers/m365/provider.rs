//! Microsoft Graph provider. The v0.2 build-out (Tasks 3-7) replaces each
//! `unimplemented!()` with the real Graph API calls; until then the type
//! exists so the daemon's `provider` dispatch can recognise `"m365"` and
//! return a structured "not implemented yet" error in tests.

use crate::error::Result;
use crate::providers::r#trait::{MailProvider, SearchResults};
use crate::providers::types::*;
use crate::providers::gmail::AuthClient;
use crate::types::{DraftId, FolderId, LabelId, MessageId, ThreadId};
use async_trait::async_trait;

#[derive(Clone)]
pub struct M365Provider {
    /// Reuse Gmail's AuthClient — it speaks generic OAuth 2.0 against whatever
    /// `ProviderConfig.token_url` was configured. The Microsoft `ProviderConfig`
    /// from `oauth::microsoft` points it at the Graph token endpoint.
    #[allow(dead_code)] // populated in v0.2 Task 5+ when the HTTP wiring lands
    auth: AuthClient,
}

impl M365Provider {
    pub fn new(auth: AuthClient) -> Self {
        Self { auth }
    }
}

#[async_trait]
impl MailProvider for M365Provider {
    async fn search(&self, _q: &SearchQuery) -> Result<SearchResults> {
        unimplemented!("v0.2 Task 5: m365 search via /me/messages?$search=")
    }
    async fn get_thread(&self, _id: &ThreadId) -> Result<Thread> {
        unimplemented!("v0.2 Task 5: m365 get_thread via /me/messages?$filter=conversationId")
    }
    async fn get_message(&self, _id: &MessageId) -> Result<Message> {
        unimplemented!("v0.2 Task 5: m365 get_message via /me/messages/{{id}}")
    }
    async fn list_folders(&self) -> Result<Vec<Folder>> {
        unimplemented!("v0.2 Task 4: m365 list_folders via /me/mailFolders")
    }
    async fn list_labels(&self) -> Result<Vec<Label>> {
        unimplemented!("v0.2 Task 4: m365 doesn't have labels — return categories")
    }
    async fn list_drafts(&self) -> Result<Vec<DraftSummary>> {
        unimplemented!("v0.2 Task 7: m365 list_drafts via /me/mailFolders/Drafts/messages")
    }
    async fn mark_read(&self, _ids: &[MessageId], _read: bool) -> Result<()> {
        unimplemented!("v0.2 Task 6")
    }
    async fn star(&self, _ids: &[MessageId], _starred: bool) -> Result<()> {
        unimplemented!("v0.2 Task 6")
    }
    async fn label(&self, _ids: &[MessageId], _label: &LabelId, _on: bool) -> Result<()> {
        unimplemented!("v0.2 Task 6: maps to Graph categories")
    }
    async fn move_to(&self, _ids: &[MessageId], _folder: &FolderId) -> Result<()> {
        unimplemented!("v0.2 Task 6")
    }
    async fn archive(&self, _ids: &[MessageId]) -> Result<()> {
        unimplemented!("v0.2 Task 6")
    }
    async fn trash(&self, _ids: &[MessageId]) -> Result<()> {
        unimplemented!("v0.2 Task 6")
    }
    async fn untrash(&self, _ids: &[MessageId]) -> Result<()> {
        unimplemented!("v0.2 Task 6")
    }
    async fn create_draft(&self, _d: &DraftInput) -> Result<DraftId> {
        unimplemented!("v0.2 Task 7")
    }
    async fn update_draft(&self, _id: &DraftId, _d: &DraftInput) -> Result<()> {
        unimplemented!("v0.2 Task 7")
    }
    async fn send_message(&self, _m: &OutgoingMessage) -> Result<MessageId> {
        unimplemented!("v0.2 Task 7: /me/sendMail")
    }
    async fn send_draft(&self, _id: &DraftId) -> Result<MessageId> {
        unimplemented!("v0.2 Task 7: /me/messages/{{id}}/send")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_is_object_safe() {
        fn _assert(_: &dyn MailProvider) {}
    }
}
