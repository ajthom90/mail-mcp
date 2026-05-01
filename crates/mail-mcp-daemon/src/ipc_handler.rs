//! `Handler` impl that fronts the daemon's state to IPC callers (the tray app + admin
//! CLI). All concrete IPC methods live here.

use crate::mcp::dispatch::ProviderRegistry;
use async_trait::async_trait;
use mail_mcp_core::accounts::{AccountStore, NewAccount};
use mail_mcp_core::error::{Error as CoreError, Result as CoreResult};
use mail_mcp_core::ipc::messages::*;
use mail_mcp_core::ipc::server::Handler;
use mail_mcp_core::oauth::{self, OAuthTokens};
use mail_mcp_core::paths::Paths;
use mail_mcp_core::permissions::approvals::{ApprovalDecision, ApprovalId, ApprovalQueue};
use mail_mcp_core::permissions::{Category, Permissions, Policy};
use mail_mcp_core::providers::gmail::{AuthClient, GmailProvider};
use mail_mcp_core::secrets::{KeyKind, SecretStore};
use mail_mcp_core::storage::Storage;
use mail_mcp_core::types::{AccountId, ProviderKind};
use serde_json::Value;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, Mutex};
use ulid::Ulid;

pub struct DaemonHandler {
    pub storage: Storage,
    pub secrets: SecretStore,
    pub providers: ProviderRegistry,
    pub approvals: ApprovalQueue,
    pub started_at: Instant,
    pub mcp_endpoint: McpEndpointInfo,
    pub mcp_paused: Arc<AtomicBool>,
    pub oauth_cfg: oauth::ProviderConfig,
    pub http: reqwest::Client,
    #[allow(dead_code)]
    pub paths: Paths,
    pub notif_tx: broadcast::Sender<mail_mcp_core::ipc::messages::Notification>,
    pub pending_oauth: Arc<Mutex<HashMap<String, PendingOAuth>>>,
}

pub struct PendingOAuth {
    #[allow(dead_code)]
    pub label_hint: Option<String>,
    pub join: tokio::task::JoinHandle<CoreResult<(OAuthTokens, String /*email*/)>>,
}

#[async_trait]
impl Handler for DaemonHandler {
    async fn handle(&self, method: &str, params: Value) -> CoreResult<Value> {
        match method {
            "status" => {
                let s = Status {
                    version: env!("CARGO_PKG_VERSION").to_string(),
                    uptime_secs: self.started_at.elapsed().as_secs(),
                    account_count: AccountStore::list(&self.storage).await?.len() as u32,
                    mcp_paused: self.mcp_paused.load(Ordering::Relaxed),
                    onboarding_complete: self
                        .storage
                        .get_app_state("onboarding_complete")
                        .await?
                        .as_deref()
                        == Some("true"),
                };
                Ok(serde_json::to_value(s)?)
            }
            "accounts.list" => {
                let accs = AccountStore::list(&self.storage).await?;
                let items: Vec<AccountListItem> = accs
                    .into_iter()
                    .map(|a| AccountListItem {
                        id: a.id,
                        label: a.label,
                        provider: a.provider.as_str().into(),
                        email: a.email,
                        status: AccountStatus::Ok,
                    })
                    .collect();
                Ok(serde_json::to_value(items)?)
            }
            "accounts.add_oauth" => self.add_oauth(params).await,
            "accounts.complete_oauth" => self.complete_oauth(params).await,
            "accounts.cancel_oauth" => self.cancel_oauth(params).await,
            "accounts.remove" => self.remove_account(params).await,
            "permissions.get" => self.permissions_get(params).await,
            "permissions.set" => self.permissions_set(params).await,
            "approvals.list" => Ok(serde_json::to_value(self.approvals.list().await)?),
            "approvals.decide" => self.approvals_decide(params).await,
            "settings.set_autostart" => Ok(serde_json::json!({})),
            "settings.set_onboarding_complete" => {
                let complete = params
                    .get("complete")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                self.storage
                    .set_app_state(
                        "onboarding_complete",
                        if complete { "true" } else { "false" },
                    )
                    .await?;
                Ok(serde_json::json!({}))
            }
            "mcp.endpoint" => Ok(serde_json::to_value(&self.mcp_endpoint)?),
            "mcp.pause" => {
                let paused = params
                    .get("paused")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                self.mcp_paused.store(paused, Ordering::Relaxed);
                let _ = self
                    .notif_tx
                    .send(Notification::McpPausedChanged { paused });
                Ok(serde_json::json!({}))
            }
            other => Err(CoreError::NotFound(other.into())),
        }
    }
}

impl DaemonHandler {
    async fn add_oauth(&self, params: Value) -> CoreResult<Value> {
        let provider = params
            .get("provider")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CoreError::Provider("missing provider".into()))?
            .to_string();
        if provider != "gmail" {
            return Err(CoreError::Provider(format!(
                "provider not supported in v0.1a: {provider}"
            )));
        }
        let challenge = oauth::begin_authorization(&self.oauth_cfg, None).await?;
        let challenge_id = Ulid::new().to_string();
        let auth_url = challenge.auth_url.clone();

        // Spawn a task to await the callback + exchange the code + fetch user info.
        let cfg = self.oauth_cfg.clone();
        let http = self.http.clone();
        let join = tokio::spawn(async move {
            let tokens =
                oauth::complete_authorization(&http, &cfg, challenge, Duration::from_secs(300))
                    .await?;
            // Fetch user email via Google's userinfo endpoint.
            let resp = http
                .get("https://openidconnect.googleapis.com/v1/userinfo")
                .bearer_auth(&tokens.access_token)
                .send()
                .await?
                .error_for_status()?
                .json::<serde_json::Value>()
                .await?;
            let email = resp
                .get("email")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            Ok::<_, CoreError>((tokens, email))
        });

        self.pending_oauth.lock().await.insert(
            challenge_id.clone(),
            PendingOAuth {
                label_hint: None,
                join,
            },
        );
        Ok(serde_json::to_value(AccountAddOAuthInProgress {
            challenge_id,
            auth_url,
        })?)
    }

    async fn complete_oauth(&self, params: Value) -> CoreResult<Value> {
        let cid = params
            .get("challenge_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CoreError::Provider("missing challenge_id".into()))?
            .to_string();
        let label = params
            .get("label")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let pending = self
            .pending_oauth
            .lock()
            .await
            .remove(&cid)
            .ok_or_else(|| CoreError::NotFound(format!("challenge {cid}")))?;
        let (tokens, email) = pending
            .join
            .await
            .map_err(|e| CoreError::Internal(format!("oauth task panicked: {e}")))??;

        // Persist: account row + refresh token in keychain.
        let label = if label.is_empty() {
            email.clone()
        } else {
            label
        };
        let id = AccountStore::create(
            &self.storage,
            &NewAccount {
                label,
                provider: ProviderKind::Gmail,
                email: email.clone(),
                config: serde_json::json!({}),
                scopes: tokens
                    .scope
                    .iter()
                    .flat_map(|s| s.split(' ').map(String::from))
                    .collect(),
            },
        )
        .await?;
        Permissions::install_defaults(&self.storage, id).await?;
        if let Some(rt) = &tokens.refresh_token {
            self.secrets.set(id, KeyKind::RefreshToken, rt)?;
        } else {
            return Err(CoreError::OAuth("Google did not return a refresh_token; ensure prompt=consent + access_type=offline".into()));
        }

        // Build provider and register.
        let auth_client = AuthClient::new(self.http.clone(), self.oauth_cfg.clone(), tokens);
        let provider = Arc::new(GmailProvider::new(auth_client, email.clone()));
        self.providers.insert(id, provider).await;

        let account = AccountStore::get(&self.storage, id)
            .await?
            .ok_or_else(|| CoreError::Internal("account vanished".into()))?;
        let _ = self
            .notif_tx
            .send(Notification::AccountAdded(account.clone()));
        Ok(serde_json::to_value(account)?)
    }

    async fn cancel_oauth(&self, params: Value) -> CoreResult<Value> {
        let cid = params
            .get("challenge_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CoreError::Provider("missing challenge_id".into()))?
            .to_string();
        if let Some(p) = self.pending_oauth.lock().await.remove(&cid) {
            p.join.abort();
        }
        Ok(serde_json::json!({}))
    }

    async fn remove_account(&self, params: Value) -> CoreResult<Value> {
        let id_str = params
            .get("account_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CoreError::Provider("missing account_id".into()))?;
        let id =
            AccountId::from_str(id_str).map_err(|e| CoreError::Provider(format!("bad id: {e}")))?;
        AccountStore::delete(&self.storage, id).await?;
        let _ = self.secrets.purge(id);
        self.providers.remove(id).await;
        let _ = self
            .notif_tx
            .send(Notification::AccountRemoved { account_id: id });
        Ok(serde_json::json!({}))
    }

    async fn permissions_get(&self, params: Value) -> CoreResult<Value> {
        let id_str = params
            .get("account_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CoreError::Provider("missing account_id".into()))?;
        let id =
            AccountId::from_str(id_str).map_err(|e| CoreError::Provider(format!("bad id: {e}")))?;
        let perms = Permissions::for_account(&self.storage, id).await?;
        Ok(serde_json::to_value(PermissionMap {
            read: perms.policy_for(Category::Read),
            modify: perms.policy_for(Category::Modify),
            trash: perms.policy_for(Category::Trash),
            draft: perms.policy_for(Category::Draft),
            send: perms.policy_for(Category::Send),
        })?)
    }

    async fn permissions_set(&self, params: Value) -> CoreResult<Value> {
        let id_str = params
            .get("account_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CoreError::Provider("missing account_id".into()))?;
        let id =
            AccountId::from_str(id_str).map_err(|e| CoreError::Provider(format!("bad id: {e}")))?;
        let cat_str = params
            .get("category")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CoreError::Provider("missing category".into()))?;
        let pol_str = params
            .get("policy")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CoreError::Provider("missing policy".into()))?;
        let cat = Category::from_str(cat_str).map_err(CoreError::Provider)?;
        let pol = Policy::from_str(pol_str).map_err(CoreError::Provider)?;
        Permissions::set(&self.storage, id, cat, pol).await?;
        Ok(serde_json::json!({}))
    }

    async fn approvals_decide(&self, params: Value) -> CoreResult<Value> {
        let id_str = params
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CoreError::Provider("missing id".into()))?;
        let id = ApprovalId(
            Ulid::from_str(id_str).map_err(|e| CoreError::Provider(format!("bad id: {e}")))?,
        );
        let dec_str = params
            .get("decision")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let decision = match dec_str {
            "approve" => ApprovalDecision::Approve,
            "reject" => ApprovalDecision::Reject,
            other => return Err(CoreError::Provider(format!("bad decision: {other}"))),
        };
        self.approvals.decide(id, decision).await?;
        Ok(serde_json::json!({}))
    }
}

/// Hydrate `ProviderRegistry` from persisted accounts at daemon startup. For each account
/// we read the refresh token from the keychain and construct a Gmail provider.
pub async fn hydrate_providers(
    storage: &Storage,
    secrets: &SecretStore,
    http: &reqwest::Client,
    cfg: &oauth::ProviderConfig,
    registry: &ProviderRegistry,
) -> CoreResult<()> {
    for acc in AccountStore::list(storage).await? {
        let Some(refresh) = secrets.get(acc.id, KeyKind::RefreshToken)? else {
            continue;
        };
        let tokens = oauth::OAuthTokens {
            access_token: String::new(),
            refresh_token: Some(refresh),
            expires_at: chrono::Utc::now() - chrono::Duration::seconds(1),
            scope: None,
        };
        let auth = AuthClient::new(http.clone(), cfg.clone(), tokens);
        let provider = Arc::new(GmailProvider::new(auth, acc.email.clone()));
        registry.insert(acc.id, provider).await;
    }
    Ok(())
}
