use super::approvals::{ApprovalQueue, ApprovalRequest};
use super::{Category, Permissions, Policy};
use crate::error::Error;
use crate::types::AccountId;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[cfg(test)]
mod tests {
    use super::super::approvals::ApprovalDecision;
    use super::*;
    use std::time::Duration;

    fn perms_for(map: &[(Category, Policy)]) -> Permissions {
        let mut p = Permissions::with_defaults();
        for (c, pol) in map {
            p.override_for(*c, *pol);
        }
        p
    }

    fn req() -> ApprovalRequest {
        ApprovalRequest {
            account: AccountId::new(),
            category: Category::Send,
            summary: "x".into(),
            details: serde_json::json!({}),
        }
    }

    #[tokio::test]
    async fn allow_returns_proceed() {
        let queue = ApprovalQueue::new(Duration::from_secs(5));
        let trust = SessionTrust::new();
        let p = perms_for(&[(Category::Send, Policy::Allow)]);
        let outcome = enforce(&p, &queue, &trust, Category::Send, req()).await.unwrap();
        assert_eq!(outcome, EnforceOutcome::Proceed);
    }

    #[tokio::test]
    async fn block_returns_blocked() {
        let queue = ApprovalQueue::new(Duration::from_secs(5));
        let trust = SessionTrust::new();
        let p = perms_for(&[(Category::Send, Policy::Block)]);
        let outcome = enforce(&p, &queue, &trust, Category::Send, req()).await.unwrap();
        assert_eq!(outcome, EnforceOutcome::Blocked);
    }

    #[tokio::test]
    async fn draftify_returns_convert_to_draft() {
        let queue = ApprovalQueue::new(Duration::from_secs(5));
        let trust = SessionTrust::new();
        let p = perms_for(&[(Category::Send, Policy::Draftify)]);
        let outcome = enforce(&p, &queue, &trust, Category::Send, req()).await.unwrap();
        assert_eq!(outcome, EnforceOutcome::ConvertToDraft);
    }

    #[tokio::test]
    async fn draftify_for_non_send_falls_back_to_confirm() {
        let queue = ApprovalQueue::new(Duration::from_secs(5));
        let trust = SessionTrust::new();
        let p = perms_for(&[(Category::Trash, Policy::Draftify)]);
        let q2 = queue.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            let pending = q2.list().await;
            q2.decide(pending[0].id, ApprovalDecision::Approve).await.unwrap();
        });
        let mut r = req();
        r.category = Category::Trash;
        let outcome = enforce(&p, &queue, &trust, Category::Trash, r).await.unwrap();
        assert_eq!(outcome, EnforceOutcome::Proceed);
    }

    #[tokio::test]
    async fn session_trust_short_circuits_after_first_approval() {
        let queue = ApprovalQueue::new(Duration::from_secs(5));
        let trust = SessionTrust::new();
        let p = perms_for(&[(Category::Send, Policy::Session)]);
        let q2 = queue.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            let pending = q2.list().await;
            q2.decide(pending[0].id, ApprovalDecision::Approve).await.unwrap();
        });
        let r = req();
        let acct = r.account;
        let first = enforce(&p, &queue, &trust, Category::Send, r.clone()).await.unwrap();
        assert_eq!(first, EnforceOutcome::Proceed);
        // Second call must NOT enqueue another approval.
        let pre_count = queue.list().await.len();
        let mut r2 = r.clone();
        r2.account = acct;
        let second = enforce(&p, &queue, &trust, Category::Send, r2).await.unwrap();
        assert_eq!(second, EnforceOutcome::Proceed);
        assert_eq!(queue.list().await.len(), pre_count);
    }

    #[tokio::test]
    async fn confirm_rejection_returns_blocked() {
        let queue = ApprovalQueue::new(Duration::from_secs(5));
        let trust = SessionTrust::new();
        let p = perms_for(&[(Category::Send, Policy::Confirm)]);
        let q2 = queue.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            let pending = q2.list().await;
            q2.decide(pending[0].id, ApprovalDecision::Reject).await.unwrap();
        });
        let outcome = enforce(&p, &queue, &trust, Category::Send, req()).await.unwrap();
        assert_eq!(outcome, EnforceOutcome::Blocked);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforceOutcome {
    /// Caller may execute the operation.
    Proceed,
    /// Caller MUST refuse the call (return MCP error).
    Blocked,
    /// (Send-only.) Caller MUST rewrite send → create_draft.
    ConvertToDraft,
}

/// Per-(daemon-process-lifetime) record of which (account, category) pairs the user has
/// already approved when policy is Session.
#[derive(Default, Clone)]
pub struct SessionTrust {
    inner: Arc<Mutex<HashMap<(AccountId, Category), bool>>>,
}

impl SessionTrust {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_trusted(&self, account: AccountId, cat: Category) -> bool {
        self.inner.lock().unwrap().get(&(account, cat)).copied().unwrap_or(false)
    }

    pub fn grant(&self, account: AccountId, cat: Category) {
        self.inner.lock().unwrap().insert((account, cat), true);
    }

    pub fn revoke_all(&self) {
        self.inner.lock().unwrap().clear();
    }
}

pub async fn enforce(
    perms: &Permissions,
    queue: &ApprovalQueue,
    trust: &SessionTrust,
    category: Category,
    request: ApprovalRequest,
) -> Result<EnforceOutcome, Error> {
    let policy = perms.policy_for(category);
    match policy {
        Policy::Allow => Ok(EnforceOutcome::Proceed),
        Policy::Block => Ok(EnforceOutcome::Blocked),
        Policy::Draftify => {
            if matches!(category, Category::Send) {
                Ok(EnforceOutcome::ConvertToDraft)
            } else {
                // Draftify is Send-only; for any other category fall back to Confirm.
                ask_confirm(queue, request).await
            }
        }
        Policy::Confirm => ask_confirm(queue, request).await,
        Policy::Session => {
            let account = request.account;
            if trust.is_trusted(account, category) {
                Ok(EnforceOutcome::Proceed)
            } else {
                let outcome = ask_confirm(queue, request).await?;
                if matches!(outcome, EnforceOutcome::Proceed) {
                    trust.grant(account, category);
                }
                Ok(outcome)
            }
        }
    }
}

async fn ask_confirm(queue: &ApprovalQueue, request: ApprovalRequest) -> Result<EnforceOutcome, Error> {
    let (_id, fut) = queue.enqueue(request).await;
    use super::approvals::ApprovalOutcome;
    match fut.await? {
        ApprovalOutcome::Approved => Ok(EnforceOutcome::Proceed),
        ApprovalOutcome::Rejected => Ok(EnforceOutcome::Blocked),
        ApprovalOutcome::Timeout => Ok(EnforceOutcome::Blocked),
    }
}
