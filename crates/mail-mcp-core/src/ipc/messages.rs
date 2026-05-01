use crate::permissions::approvals::PendingApproval;
use crate::permissions::Policy;
use crate::types::{Account, AccountId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountListItem {
    pub id: AccountId,
    pub label: String,
    pub provider: String,
    pub email: String,
    pub status: AccountStatus,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountStatus {
    Ok,
    NeedsReauth,
    NetworkError,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountAddOAuthInProgress {
    pub challenge_id: String,
    pub auth_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionMap {
    pub read: Policy,
    pub modify: Policy,
    pub trash: Policy,
    pub draft: Policy,
    pub send: Policy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpEndpointInfo {
    pub url: String,
    pub bearer_token: String,
    pub stdio_shim_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Status {
    pub version: String,
    pub uptime_secs: u64,
    pub account_count: u32,
    pub mcp_paused: bool,
    pub onboarding_complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionAck {
    pub subscribed: Vec<String>,
}

/// Notifications pushed from daemon → client outside of any specific request/response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum Notification {
    #[serde(rename = "approval.requested")]
    ApprovalRequested(PendingApproval),
    #[serde(rename = "approval.resolved")]
    ApprovalResolved { id: String, decision: String },
    #[serde(rename = "account.added")]
    AccountAdded(Account),
    #[serde(rename = "account.removed")]
    AccountRemoved { account_id: AccountId },
    #[serde(rename = "account.needs_reauth")]
    AccountNeedsReauth { account_id: AccountId },
    #[serde(rename = "mcp.paused_changed")]
    McpPausedChanged { paused: bool },
}
