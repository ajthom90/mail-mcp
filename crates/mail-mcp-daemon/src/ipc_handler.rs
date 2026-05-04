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
use mail_mcp_core::providers::m365::M365Provider;
use mail_mcp_core::providers::r#trait::MailProvider;
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
    pub oauth_cfg_google: oauth::ProviderConfig,
    pub oauth_cfg_microsoft: oauth::ProviderConfig,
    pub http: reqwest::Client,
    #[allow(dead_code)]
    pub paths: Paths,
    pub notif_tx: broadcast::Sender<mail_mcp_core::ipc::messages::Notification>,
    pub pending_oauth: Arc<Mutex<HashMap<String, PendingOAuth>>>,
}

pub struct PendingOAuth {
    #[allow(dead_code)]
    pub label_hint: Option<String>,
    pub provider: ProviderKind,
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
        let provider_str = params
            .get("provider")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CoreError::Provider("missing provider".into()))?
            .to_string();
        let provider = ProviderKind::from_str(&provider_str).map_err(CoreError::Provider)?;
        let cfg = match provider {
            ProviderKind::Gmail => self.oauth_cfg_google.clone(),
            ProviderKind::Microsoft365 => self.oauth_cfg_microsoft.clone(),
            ProviderKind::Imap => {
                return Err(CoreError::Provider(
                    "imap provider not supported by add_oauth".into(),
                ))
            }
        };
        let challenge = oauth::begin_authorization(&cfg, None).await?;
        let challenge_id = Ulid::new().to_string();
        let auth_url = challenge.auth_url.clone();

        // Spawn a task to await the callback + exchange the code + fetch user email
        // from the provider's userinfo endpoint.
        let http = self.http.clone();
        let join = tokio::spawn(async move {
            let tokens =
                oauth::complete_authorization(&http, &cfg, challenge, Duration::from_secs(300))
                    .await?;
            let email = fetch_user_email(&http, provider, &tokens.access_token).await?;
            Ok::<_, CoreError>((tokens, email))
        });

        self.pending_oauth.lock().await.insert(
            challenge_id.clone(),
            PendingOAuth {
                label_hint: None,
                provider,
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
                provider: pending.provider,
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
        let Some(rt) = tokens.refresh_token.clone() else {
            return Err(CoreError::OAuth(format!(
                "{} did not return a refresh_token",
                pending.provider.as_str()
            )));
        };
        self.secrets.set(id, KeyKind::RefreshToken, &rt)?;

        // Build provider and register. Hand AuthClient a rotation callback so
        // that a rotated refresh token is persisted to the keychain immediately
        // — closes #2 (Microsoft rotates on every refresh; Google occasionally).
        let provider = build_provider(
            self.http.clone(),
            &self.oauth_cfg_google,
            &self.oauth_cfg_microsoft,
            pending.provider,
            tokens,
            email.clone(),
            make_rotation_callback(self.secrets, id),
        );
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

/// Hydrate `ProviderRegistry` from persisted accounts at daemon startup. For each
/// account we read the refresh token from the keychain and construct the right
/// concrete provider for `account.provider`.
pub async fn hydrate_providers(
    storage: &Storage,
    secrets: &SecretStore,
    http: &reqwest::Client,
    cfg_google: &oauth::ProviderConfig,
    cfg_microsoft: &oauth::ProviderConfig,
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
        let provider = build_provider(
            http.clone(),
            cfg_google,
            cfg_microsoft,
            acc.provider,
            tokens,
            acc.email.clone(),
            make_rotation_callback(*secrets, acc.id),
        );
        registry.insert(acc.id, provider).await;
    }
    Ok(())
}

/// Construct the concrete `MailProvider` for a given `ProviderKind`, sharing the
/// `AuthClient` + rotation-callback setup. Skips IMAP (returns a no-op stub —
/// IMAP support is its own milestone).
fn build_provider(
    http: reqwest::Client,
    cfg_google: &oauth::ProviderConfig,
    cfg_microsoft: &oauth::ProviderConfig,
    kind: ProviderKind,
    tokens: OAuthTokens,
    email: String,
    rotation_cb: mail_mcp_core::providers::gmail::RefreshRotationCallback,
) -> Arc<dyn MailProvider> {
    match kind {
        ProviderKind::Gmail => {
            let auth = AuthClient::with_rotation_callback(
                http,
                cfg_google.clone(),
                tokens,
                Some(rotation_cb),
            );
            Arc::new(GmailProvider::new(auth, email))
        }
        ProviderKind::Microsoft365 => {
            let auth = AuthClient::with_rotation_callback(
                http,
                cfg_microsoft.clone(),
                tokens,
                Some(rotation_cb),
            );
            Arc::new(M365Provider::new(auth, email))
        }
        ProviderKind::Imap => {
            // Unreachable for now — add_oauth rejects "imap" and no persisted
            // accounts can have it. If we add IMAP later, build the provider here.
            unreachable!("imap provider not implemented")
        }
    }
}

/// Fetch the user's primary email address from the provider's userinfo endpoint.
async fn fetch_user_email(
    http: &reqwest::Client,
    kind: ProviderKind,
    access_token: &str,
) -> CoreResult<String> {
    let (url, primary, fallback) = match kind {
        ProviderKind::Gmail => (
            "https://openidconnect.googleapis.com/v1/userinfo",
            "email",
            None,
        ),
        ProviderKind::Microsoft365 => (
            "https://graph.microsoft.com/v1.0/me",
            "mail",
            Some("userPrincipalName"),
        ),
        ProviderKind::Imap => unreachable!("imap has no userinfo endpoint"),
    };
    let resp = http
        .get(url)
        .bearer_auth(access_token)
        .send()
        .await?
        .error_for_status()?
        .json::<serde_json::Value>()
        .await?;
    let primary_val = resp.get(primary).and_then(|v| v.as_str());
    let fallback_val = fallback.and_then(|f| resp.get(f).and_then(|v| v.as_str()));
    Ok(primary_val
        .or(fallback_val)
        .unwrap_or("unknown")
        .to_string())
}

/// Build a callback that persists a rotated Gmail refresh token to the
/// per-account keychain entry. Logs (non-fatal) if the keychain write fails —
/// the in-memory token is still good for the rest of the process lifetime.
fn make_rotation_callback(
    secrets: SecretStore,
    account_id: AccountId,
) -> mail_mcp_core::providers::gmail::RefreshRotationCallback {
    Arc::new(move |new_rt: &str| {
        if let Err(e) = secrets.set(account_id, KeyKind::RefreshToken, new_rt) {
            tracing::warn!(
                ?e,
                ?account_id,
                "failed to persist rotated refresh token to keychain"
            );
        }
    })
}
