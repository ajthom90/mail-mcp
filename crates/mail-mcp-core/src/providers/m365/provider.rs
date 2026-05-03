//! Microsoft Graph provider. Wires the per-feature `_impl` functions in
//! sibling modules (parse / folders / messages / triage / compose) into the
//! `MailProvider` trait.

use super::{compose, folders, messages, triage};
use crate::error::Result;
use crate::providers::gmail::AuthClient;
use crate::providers::r#trait::{MailProvider, SearchResults};
use crate::providers::types::*;
use crate::types::{DraftId, FolderId, LabelId, MessageId, ThreadId};
use async_trait::async_trait;

const PROD_BASE: &str = "https://graph.microsoft.com/v1.0";

pub struct M365Provider {
    pub client: AuthClient,
    pub base: String,
    pub user_email: String,
}

impl M365Provider {
    pub fn new(client: AuthClient, user_email: String) -> Self {
        Self {
            client,
            base: PROD_BASE.into(),
            user_email,
        }
    }

    pub fn with_base(client: AuthClient, base: String, user_email: String) -> Self {
        Self {
            client,
            base,
            user_email,
        }
    }
}

#[async_trait]
impl MailProvider for M365Provider {
    async fn search(&self, q: &SearchQuery) -> Result<SearchResults> {
        messages::search_impl(&self.client, &self.base, q).await
    }
    async fn get_thread(&self, id: &ThreadId) -> Result<Thread> {
        messages::get_thread_impl(&self.client, &self.base, id).await
    }
    async fn get_message(&self, id: &MessageId) -> Result<Message> {
        messages::get_message_impl(&self.client, &self.base, id).await
    }
    async fn list_folders(&self) -> Result<Vec<Folder>> {
        folders::list_folders_impl(&self.client, &self.base).await
    }
    async fn list_labels(&self) -> Result<Vec<Label>> {
        folders::list_labels_impl(&self.client, &self.base).await
    }
    async fn list_drafts(&self) -> Result<Vec<DraftSummary>> {
        compose::list_drafts_impl(&self.client, &self.base).await
    }
    async fn mark_read(&self, ids: &[MessageId], read: bool) -> Result<()> {
        triage::mark_read_impl(&self.client, &self.base, ids, read).await
    }
    async fn star(&self, ids: &[MessageId], starred: bool) -> Result<()> {
        triage::star_impl(&self.client, &self.base, ids, starred).await
    }
    async fn label(&self, ids: &[MessageId], label: &LabelId, on: bool) -> Result<()> {
        triage::label_impl(&self.client, &self.base, ids, label, on).await
    }
    async fn move_to(&self, ids: &[MessageId], folder: &FolderId) -> Result<()> {
        triage::move_to_impl(&self.client, &self.base, ids, folder).await
    }
    async fn archive(&self, ids: &[MessageId]) -> Result<()> {
        triage::archive_impl(&self.client, &self.base, ids).await
    }
    async fn trash(&self, ids: &[MessageId]) -> Result<()> {
        triage::trash_impl(&self.client, &self.base, ids).await
    }
    async fn untrash(&self, ids: &[MessageId]) -> Result<()> {
        triage::untrash_impl(&self.client, &self.base, ids).await
    }
    async fn create_draft(&self, d: &DraftInput) -> Result<DraftId> {
        compose::create_draft_impl(&self.client, &self.base, d).await
    }
    async fn update_draft(&self, id: &DraftId, d: &DraftInput) -> Result<()> {
        compose::update_draft_impl(&self.client, &self.base, id, d).await
    }
    async fn send_message(&self, m: &OutgoingMessage) -> Result<MessageId> {
        compose::send_message_impl(&self.client, &self.base, m).await
    }
    async fn send_draft(&self, id: &DraftId) -> Result<MessageId> {
        compose::send_draft_impl(&self.client, &self.base, id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_is_object_safe() {
        fn _f(_: Box<dyn MailProvider>) {}
    }
}
