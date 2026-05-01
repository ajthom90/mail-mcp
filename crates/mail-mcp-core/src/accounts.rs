use crate::error::{Error, Result};
use crate::storage::Storage;
use crate::types::{Account, AccountId, ProviderKind};
use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewAccount {
    pub label: String,
    pub provider: ProviderKind,
    pub email: String,
    #[serde(default = "empty_object")]
    pub config: serde_json::Value,
    #[serde(default)]
    pub scopes: Vec<String>,
}

fn empty_object() -> serde_json::Value {
    serde_json::json!({})
}

/// Methods over `Storage` for the `accounts` table. Free functions for ergonomics
/// (we don't need a separate type since `Storage` is the handle).
pub struct AccountStore;

impl AccountStore {
    pub async fn create(store: &Storage, new: &NewAccount) -> Result<AccountId> {
        let id = AccountId::new();
        let scopes_json = serde_json::to_string(&new.scopes)?;
        let config_json = serde_json::to_string(&new.config)?;
        let now = Utc::now().timestamp();
        sqlx::query(
            "INSERT INTO accounts (id, label, provider, email, config_json, scopes_json, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"
        )
        .bind(id.to_string())
        .bind(&new.label)
        .bind(new.provider.as_str())
        .bind(&new.email)
        .bind(config_json)
        .bind(scopes_json)
        .bind(now)
        .execute(store.pool())
        .await?;
        Ok(id)
    }

    pub async fn list(store: &Storage) -> Result<Vec<Account>> {
        let rows: Vec<AccountRow> =
            sqlx::query_as("SELECT id, label, provider, email, config_json, scopes_json, created_at, last_validated FROM accounts ORDER BY created_at ASC")
                .fetch_all(store.pool())
                .await?;
        rows.into_iter().map(AccountRow::into_account).collect()
    }

    pub async fn get(store: &Storage, id: AccountId) -> Result<Option<Account>> {
        let row: Option<AccountRow> =
            sqlx::query_as("SELECT id, label, provider, email, config_json, scopes_json, created_at, last_validated FROM accounts WHERE id = ?1")
                .bind(id.to_string())
                .fetch_optional(store.pool())
                .await?;
        row.map(AccountRow::into_account).transpose()
    }

    pub async fn delete(store: &Storage, id: AccountId) -> Result<()> {
        let res = sqlx::query("DELETE FROM accounts WHERE id = ?1")
            .bind(id.to_string())
            .execute(store.pool())
            .await?;
        if res.rows_affected() == 0 {
            return Err(Error::NotFound(format!("account {id}")));
        }
        Ok(())
    }

    pub async fn touch_last_validated(store: &Storage, id: AccountId) -> Result<()> {
        let now = Utc::now().timestamp();
        sqlx::query("UPDATE accounts SET last_validated = ?1 WHERE id = ?2")
            .bind(now)
            .bind(id.to_string())
            .execute(store.pool())
            .await?;
        Ok(())
    }
}

#[derive(sqlx::FromRow)]
struct AccountRow {
    id: String,
    label: String,
    provider: String,
    email: String,
    config_json: String,
    scopes_json: String,
    created_at: i64,
    last_validated: Option<i64>,
}

impl AccountRow {
    fn into_account(self) -> Result<Account> {
        let id = AccountId::from_str(&self.id)
            .map_err(|e| Error::Internal(format!("bad account id in db: {e}")))?;
        let provider = ProviderKind::from_str(&self.provider)
            .map_err(|e| Error::Internal(format!("bad provider in db: {e}")))?;
        let config: serde_json::Value = serde_json::from_str(&self.config_json)?;
        let scopes: Vec<String> = serde_json::from_str(&self.scopes_json)?;
        let created_at: DateTime<Utc> = Utc
            .timestamp_opt(self.created_at, 0)
            .single()
            .ok_or_else(|| Error::Internal("bad created_at in db".into()))?;
        let last_validated = self
            .last_validated
            .and_then(|t| Utc.timestamp_opt(t, 0).single());
        Ok(Account {
            id,
            label: self.label,
            provider,
            email: self.email,
            config,
            scopes,
            created_at,
            last_validated,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Storage;
    use crate::types::ProviderKind;

    async fn fresh_store() -> Storage {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("s.db");
        let s = Storage::open(&path).await.unwrap();
        // Leak the tempdir so the file stays alive for the test.
        std::mem::forget(tmp);
        s
    }

    #[tokio::test]
    async fn insert_and_list_account() {
        let store = fresh_store().await;
        let acc = NewAccount {
            label: "Personal Gmail".into(),
            provider: ProviderKind::Gmail,
            email: "alice@example.com".into(),
            config: serde_json::json!({}),
            scopes: vec!["gmail.modify".into()],
        };
        let id = AccountStore::create(&store, &acc).await.unwrap();
        let listed = AccountStore::list(&store).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, id);
        assert_eq!(listed[0].email, "alice@example.com");
    }

    #[tokio::test]
    async fn get_returns_none_for_missing() {
        let store = fresh_store().await;
        let id = crate::types::AccountId::new();
        let got = AccountStore::get(&store, id).await.unwrap();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn delete_removes_account() {
        let store = fresh_store().await;
        let id = AccountStore::create(
            &store,
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
        AccountStore::delete(&store, id).await.unwrap();
        assert!(AccountStore::get(&store, id).await.unwrap().is_none());
    }
}
