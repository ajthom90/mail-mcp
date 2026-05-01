use crate::error::{Error, Result};
use crate::storage::Storage;
use crate::types::AccountId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;

pub mod approvals;

/// Operation categories that map to MCP tools (see spec § Tool surface).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Read,
    Modify,
    Trash,
    Draft,
    Send,
}

impl Category {
    pub fn as_str(&self) -> &'static str {
        match self {
            Category::Read => "read",
            Category::Modify => "modify",
            Category::Trash => "trash",
            Category::Draft => "draft",
            Category::Send => "send",
        }
    }

    pub const ALL: &'static [Category] = &[
        Category::Read,
        Category::Modify,
        Category::Trash,
        Category::Draft,
        Category::Send,
    ];
}

impl FromStr for Category {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "read" => Ok(Category::Read),
            "modify" => Ok(Category::Modify),
            "trash" => Ok(Category::Trash),
            "draft" => Ok(Category::Draft),
            "send" => Ok(Category::Send),
            other => Err(format!("unknown category: {other}")),
        }
    }
}

/// Policy applied per-(account, category).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Policy {
    /// Execute immediately.
    Allow,
    /// Block, prompt the user via the approvals queue, await decision.
    Confirm,
    /// First call in a daemon-process-lifetime is treated as Confirm; if approved, subsequent calls in the same session execute immediately.
    Session,
    /// (Send-category only.) Silently rewrite send_message/send_draft into create_draft.
    Draftify,
    /// Refuse with an MCP error.
    Block,
}

impl Policy {
    pub fn as_str(&self) -> &'static str {
        match self {
            Policy::Allow => "allow",
            Policy::Confirm => "confirm",
            Policy::Session => "session",
            Policy::Draftify => "draftify",
            Policy::Block => "block",
        }
    }
}

impl FromStr for Policy {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "allow" => Ok(Policy::Allow),
            "confirm" => Ok(Policy::Confirm),
            "session" => Ok(Policy::Session),
            "draftify" => Ok(Policy::Draftify),
            "block" => Ok(Policy::Block),
            other => Err(format!("unknown policy: {other}")),
        }
    }
}

/// Default policies set at account creation time (matches the first-run wizard defaults).
pub fn default_policy(category: Category) -> Policy {
    match category {
        Category::Read => Policy::Allow,
        Category::Modify => Policy::Allow,
        Category::Trash => Policy::Confirm,
        Category::Draft => Policy::Allow,
        Category::Send => Policy::Draftify,
    }
}

/// In-memory snapshot of an account's policy table.
#[derive(Debug, Clone)]
pub struct Permissions {
    map: HashMap<Category, Policy>,
}

impl Permissions {
    pub async fn for_account(store: &Storage, id: AccountId) -> Result<Self> {
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT category, policy FROM permissions WHERE account_id = ?1")
                .bind(id.to_string())
                .fetch_all(store.pool())
                .await?;
        let mut map: HashMap<Category, Policy> = HashMap::new();
        for cat in Category::ALL {
            map.insert(*cat, default_policy(*cat));
        }
        for (cat, pol) in rows {
            let category = Category::from_str(&cat)
                .map_err(|e| Error::Internal(format!("bad category in db: {e}")))?;
            let policy = Policy::from_str(&pol)
                .map_err(|e| Error::Internal(format!("bad policy in db: {e}")))?;
            map.insert(category, policy);
        }
        Ok(Permissions { map })
    }

    pub fn policy_for(&self, category: Category) -> Policy {
        self.map
            .get(&category)
            .copied()
            .unwrap_or_else(|| default_policy(category))
    }

    pub async fn set(
        store: &Storage,
        id: AccountId,
        category: Category,
        policy: Policy,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO permissions (account_id, category, policy) VALUES (?1, ?2, ?3) \
             ON CONFLICT(account_id, category) DO UPDATE SET policy = excluded.policy",
        )
        .bind(id.to_string())
        .bind(category.as_str())
        .bind(policy.as_str())
        .execute(store.pool())
        .await?;
        Ok(())
    }

    /// Persist all default policies for a freshly-created account.
    pub async fn install_defaults(store: &Storage, id: AccountId) -> Result<()> {
        for cat in Category::ALL {
            Self::set(store, id, *cat, default_policy(*cat)).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::{AccountStore, NewAccount};
    use crate::storage::Storage;
    use crate::types::{AccountId, ProviderKind};

    async fn account_in_fresh_store() -> (Storage, AccountId) {
        let tmp = tempfile::tempdir().unwrap();
        let s = Storage::open(&tmp.path().join("s.db")).await.unwrap();
        std::mem::forget(tmp);
        let id = AccountStore::create(
            &s,
            &NewAccount {
                label: "x".into(),
                provider: ProviderKind::Gmail,
                email: "x@example.com".into(),
                config: serde_json::json!({}),
                scopes: vec![],
            },
        )
        .await
        .unwrap();
        (s, id)
    }

    #[tokio::test]
    async fn defaults_are_used_when_unset() {
        let (store, id) = account_in_fresh_store().await;
        let pm = Permissions::for_account(&store, id).await.unwrap();
        assert_eq!(pm.policy_for(Category::Read), Policy::Allow);
        assert_eq!(pm.policy_for(Category::Modify), Policy::Allow);
        assert_eq!(pm.policy_for(Category::Trash), Policy::Confirm);
        assert_eq!(pm.policy_for(Category::Draft), Policy::Allow);
        assert_eq!(pm.policy_for(Category::Send), Policy::Draftify);
    }

    #[tokio::test]
    async fn set_overrides_default() {
        let (store, id) = account_in_fresh_store().await;
        Permissions::set(&store, id, Category::Send, Policy::Confirm)
            .await
            .unwrap();
        let pm = Permissions::for_account(&store, id).await.unwrap();
        assert_eq!(pm.policy_for(Category::Send), Policy::Confirm);
        assert_eq!(pm.policy_for(Category::Read), Policy::Allow);
    }

    #[test]
    fn category_round_trip_str() {
        for c in [
            Category::Read,
            Category::Modify,
            Category::Trash,
            Category::Draft,
            Category::Send,
        ] {
            assert_eq!(Category::from_str(c.as_str()).unwrap(), c);
        }
    }

    #[test]
    fn policy_round_trip_str() {
        for p in [
            Policy::Allow,
            Policy::Confirm,
            Policy::Session,
            Policy::Draftify,
            Policy::Block,
        ] {
            assert_eq!(Policy::from_str(p.as_str()).unwrap(), p);
        }
    }
}
