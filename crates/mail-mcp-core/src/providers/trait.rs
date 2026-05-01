use super::types::*;
use crate::error::Result;
use crate::types::{DraftId, FolderId, LabelId, MessageId, ThreadId};
use async_trait::async_trait;

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-time check: the trait is object-safe.
    fn _assert_object_safe(_: &dyn MailProvider) {}

    /// A no-op stub to confirm the trait can be implemented and used through `dyn`.
    struct Stub;
    #[async_trait]
    impl MailProvider for Stub {
        async fn search(&self, _q: &SearchQuery) -> Result<SearchResults> {
            unimplemented!()
        }
        async fn get_thread(&self, _id: &ThreadId) -> Result<Thread> {
            unimplemented!()
        }
        async fn get_message(&self, _id: &MessageId) -> Result<Message> {
            unimplemented!()
        }
        async fn list_folders(&self) -> Result<Vec<Folder>> {
            unimplemented!()
        }
        async fn list_labels(&self) -> Result<Vec<Label>> {
            unimplemented!()
        }
        async fn list_drafts(&self) -> Result<Vec<DraftSummary>> {
            unimplemented!()
        }
        async fn mark_read(&self, _ids: &[MessageId], _read: bool) -> Result<()> {
            unimplemented!()
        }
        async fn star(&self, _ids: &[MessageId], _starred: bool) -> Result<()> {
            unimplemented!()
        }
        async fn label(&self, _ids: &[MessageId], _label: &LabelId, _on: bool) -> Result<()> {
            unimplemented!()
        }
        async fn move_to(&self, _ids: &[MessageId], _folder: &FolderId) -> Result<()> {
            unimplemented!()
        }
        async fn archive(&self, _ids: &[MessageId]) -> Result<()> {
            unimplemented!()
        }
        async fn trash(&self, _ids: &[MessageId]) -> Result<()> {
            unimplemented!()
        }
        async fn untrash(&self, _ids: &[MessageId]) -> Result<()> {
            unimplemented!()
        }
        async fn create_draft(&self, _d: &DraftInput) -> Result<DraftId> {
            unimplemented!()
        }
        async fn update_draft(&self, _id: &DraftId, _d: &DraftInput) -> Result<()> {
            unimplemented!()
        }
        async fn send_message(&self, _m: &OutgoingMessage) -> Result<MessageId> {
            unimplemented!()
        }
        async fn send_draft(&self, _id: &DraftId) -> Result<MessageId> {
            unimplemented!()
        }
    }

    #[test]
    fn stub_compiles_against_trait() {
        let s: Box<dyn MailProvider> = Box::new(Stub);
        let _: &dyn MailProvider = &*s;
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SearchResults {
    pub threads: Vec<ThreadSummary>,
    /// Cursor to pass back to fetch the next page; None when done.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// Provider-agnostic mail interface. Implementations are stateless wrappers over a
/// per-account auth + HTTP/IMAP client; cloning is cheap (Arc-shared client). Each method
/// is fallible; transport errors map to `Error::Provider` or `Error::Http`.
#[async_trait]
pub trait MailProvider: Send + Sync {
    // ---- Read ---------------------------------------------------------------
    async fn search(&self, q: &SearchQuery) -> Result<SearchResults>;
    async fn get_thread(&self, id: &ThreadId) -> Result<Thread>;
    async fn get_message(&self, id: &MessageId) -> Result<Message>;
    async fn list_folders(&self) -> Result<Vec<Folder>>;
    async fn list_labels(&self) -> Result<Vec<Label>>;
    async fn list_drafts(&self) -> Result<Vec<DraftSummary>>;

    // ---- Triage (reversible) ------------------------------------------------
    async fn mark_read(&self, ids: &[MessageId], read: bool) -> Result<()>;
    async fn star(&self, ids: &[MessageId], starred: bool) -> Result<()>;
    async fn label(&self, ids: &[MessageId], label: &LabelId, on: bool) -> Result<()>;
    async fn move_to(&self, ids: &[MessageId], folder: &FolderId) -> Result<()>;
    async fn archive(&self, ids: &[MessageId]) -> Result<()>;

    // ---- Triage (semi-reversible) ------------------------------------------
    async fn trash(&self, ids: &[MessageId]) -> Result<()>;
    async fn untrash(&self, ids: &[MessageId]) -> Result<()>;

    // ---- Compose ------------------------------------------------------------
    async fn create_draft(&self, d: &DraftInput) -> Result<DraftId>;
    async fn update_draft(&self, id: &DraftId, d: &DraftInput) -> Result<()>;
    async fn send_message(&self, m: &OutgoingMessage) -> Result<MessageId>;
    async fn send_draft(&self, id: &DraftId) -> Result<MessageId>;
}
