//! Approval queue. Used by the policy-enforcement layer: when a tool call requires
//! confirmation, an approval is enqueued and broadcast to subscribed tray apps;
//! the call awaits the user decision (or times out).

use crate::error::Error;
use crate::types::AccountId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, oneshot, Mutex};
use ulid::Ulid;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permissions::Category;
    use crate::types::AccountId;

    #[tokio::test]
    async fn approve_resolves_request() {
        let q = ApprovalQueue::new(Duration::from_secs(5));
        let req = ApprovalRequest {
            account: AccountId::new(),
            category: Category::Send,
            summary: "send to alice@example.com".into(),
            details: serde_json::json!({"to": "alice@example.com"}),
        };
        let (id, fut) = q.enqueue(req).await;
        // Approve from another task.
        let q2 = q.clone();
        tokio::spawn(async move { q2.decide(id, ApprovalDecision::Approve).await.unwrap() });
        let outcome = fut.await.unwrap();
        assert!(matches!(outcome, ApprovalOutcome::Approved));
    }

    #[tokio::test]
    async fn reject_resolves_request() {
        let q = ApprovalQueue::new(Duration::from_secs(5));
        let (id, fut) = q
            .enqueue(ApprovalRequest {
                account: AccountId::new(),
                category: Category::Send,
                summary: "x".into(),
                details: serde_json::json!({}),
            })
            .await;
        let q2 = q.clone();
        tokio::spawn(async move { q2.decide(id, ApprovalDecision::Reject).await.unwrap() });
        let outcome = fut.await.unwrap();
        assert!(matches!(outcome, ApprovalOutcome::Rejected));
    }

    #[tokio::test(start_paused = true)]
    async fn timeout_yields_timeout_outcome() {
        let q = ApprovalQueue::new(Duration::from_secs(2));
        let (_id, fut) = q
            .enqueue(ApprovalRequest {
                account: AccountId::new(),
                category: Category::Send,
                summary: "x".into(),
                details: serde_json::json!({}),
            })
            .await;
        tokio::time::advance(Duration::from_secs(3)).await;
        let outcome = fut.await.unwrap();
        assert!(matches!(outcome, ApprovalOutcome::Timeout));
    }

    #[tokio::test]
    async fn list_returns_pending() {
        let q = ApprovalQueue::new(Duration::from_secs(60));
        let (id, _fut) = q
            .enqueue(ApprovalRequest {
                account: AccountId::new(),
                category: Category::Trash,
                summary: "trash 3 messages".into(),
                details: serde_json::json!({"count": 3}),
            })
            .await;
        let pending = q.list().await;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, id);
    }

    #[tokio::test]
    async fn subscribe_receives_event() {
        let q = ApprovalQueue::new(Duration::from_secs(60));
        let mut rx = q.subscribe();
        let req = ApprovalRequest {
            account: AccountId::new(),
            category: Category::Send,
            summary: "x".into(),
            details: serde_json::json!({}),
        };
        let q2 = q.clone();
        let r2 = req.clone();
        tokio::spawn(async move { q2.enqueue(r2).await });
        let evt = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .unwrap()
            .unwrap();
        match evt {
            ApprovalEvent::Requested(p) => {
                assert_eq!(p.summary, req.summary);
            }
            ApprovalEvent::Resolved { .. } => panic!("expected Requested"),
        }
    }
}

/// Identifier for a single pending approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ApprovalId(pub Ulid);

impl ApprovalId {
    pub fn new() -> Self {
        Self(Ulid::new())
    }
}

impl Default for ApprovalId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ApprovalId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub account: AccountId,
    pub category: crate::permissions::Category,
    /// Short, AI-friendly description for display.
    pub summary: String,
    /// Full structured details — recipient, subject, count, etc. — for the dialog.
    pub details: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingApproval {
    pub id: ApprovalId,
    pub request: ApprovalRequest,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl std::ops::Deref for PendingApproval {
    type Target = ApprovalRequest;
    fn deref(&self) -> &Self::Target {
        &self.request
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApprovalDecision {
    Approve,
    Reject,
}

#[derive(Debug, Clone, Copy)]
pub enum ApprovalOutcome {
    Approved,
    Rejected,
    Timeout,
}

#[derive(Debug, Clone)]
pub enum ApprovalEvent {
    Requested(PendingApproval),
    Resolved {
        id: ApprovalId,
        decision: ApprovalDecision,
    },
}

/// In-memory queue of pending approvals.
#[derive(Clone)]
pub struct ApprovalQueue {
    inner: Arc<Mutex<Inner>>,
    events: broadcast::Sender<ApprovalEvent>,
    timeout: Duration,
}

struct Inner {
    pending: HashMap<ApprovalId, (PendingApproval, oneshot::Sender<ApprovalOutcome>)>,
}

impl ApprovalQueue {
    pub fn new(timeout: Duration) -> Self {
        let (tx, _rx) = broadcast::channel(64);
        Self {
            inner: Arc::new(Mutex::new(Inner {
                pending: HashMap::new(),
            })),
            events: tx,
            timeout,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ApprovalEvent> {
        self.events.subscribe()
    }

    /// Enqueue a request and return an awaitable future that resolves when the user
    /// decides or the timeout elapses.
    pub async fn enqueue(&self, request: ApprovalRequest) -> (ApprovalId, ApprovalFuture) {
        let id = ApprovalId::new();
        let (tx, rx) = oneshot::channel();
        let pending = PendingApproval {
            id,
            request,
            created_at: chrono::Utc::now(),
        };
        {
            let mut inner = self.inner.lock().await;
            inner.pending.insert(id, (pending.clone(), tx));
        }
        let _ = self.events.send(ApprovalEvent::Requested(pending));
        let fut = ApprovalFuture {
            rx,
            timeout: self.timeout,
            queue: self.clone(),
            id,
        };
        (id, fut)
    }

    pub async fn decide(&self, id: ApprovalId, decision: ApprovalDecision) -> Result<(), Error> {
        let entry = {
            let mut inner = self.inner.lock().await;
            inner.pending.remove(&id)
        };
        let Some((_p, tx)) = entry else {
            return Err(Error::NotFound(format!("approval {id}")));
        };
        let outcome = match decision {
            ApprovalDecision::Approve => ApprovalOutcome::Approved,
            ApprovalDecision::Reject => ApprovalOutcome::Rejected,
        };
        let _ = tx.send(outcome);
        let _ = self.events.send(ApprovalEvent::Resolved { id, decision });
        Ok(())
    }

    pub async fn list(&self) -> Vec<PendingApproval> {
        let inner = self.inner.lock().await;
        inner.pending.values().map(|(p, _)| p.clone()).collect()
    }
}

pub struct ApprovalFuture {
    rx: oneshot::Receiver<ApprovalOutcome>,
    timeout: Duration,
    queue: ApprovalQueue,
    id: ApprovalId,
}

impl ApprovalFuture {
    pub async fn await_outcome(self) -> Result<ApprovalOutcome, Error> {
        match tokio::time::timeout(self.timeout, self.rx).await {
            Ok(Ok(outcome)) => Ok(outcome),
            Ok(Err(_)) => Err(Error::Internal("approval channel dropped".into())),
            Err(_) => {
                // Timeout: clean up and signal Timeout.
                let mut inner = self.queue.inner.lock().await;
                inner.pending.remove(&self.id);
                Ok(ApprovalOutcome::Timeout)
            }
        }
    }
}

impl std::future::IntoFuture for ApprovalFuture {
    type Output = Result<ApprovalOutcome, Error>;
    type IntoFuture = std::pin::Pin<Box<dyn std::future::Future<Output = Self::Output> + Send>>;
    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.await_outcome())
    }
}
