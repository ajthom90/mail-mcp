use crate::error::{Error, Result};
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use sqlx::ConnectOptions;
use std::path::Path;
use std::str::FromStr;

/// Async wrapper around the SQLite state DB.
///
/// Migrations are embedded and applied automatically on `open`.
#[derive(Clone)]
pub struct Storage {
    pool: SqlitePool,
}

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

impl Storage {
    /// Open (or create) the SQLite DB at the given path and run migrations.
    pub async fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let url = format!("sqlite://{}", path.display());
        let opts = sqlx::sqlite::SqliteConnectOptions::from_str(&url)
            .map_err(|e| Error::Config(format!("bad sqlite url: {e}")))?
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .foreign_keys(true)
            .disable_statement_logging();

        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(opts)
            .await?;

        MIGRATOR
            .run(&pool)
            .await
            .map_err(|e| Error::Config(format!("migrate: {e}")))?;

        Ok(Self { pool })
    }

    /// Access the inner pool for module-internal queries.
    #[allow(dead_code)]
    pub(crate) fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn set_app_state(&self, key: &str, value: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO app_state (key, value) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_app_state(&self, key: &str) -> Result<Option<String>> {
        let row: Option<(String,)> = sqlx::query_as("SELECT value FROM app_state WHERE key = ?1")
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|(v,)| v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn open_creates_schema() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("state.db");
        let store = Storage::open(&db_path).await.unwrap();
        // Smoke: app_state table exists; we can write/read.
        store.set_app_state("schema_version", "1").await.unwrap();
        let v = store.get_app_state("schema_version").await.unwrap();
        assert_eq!(v.as_deref(), Some("1"));
    }

    #[tokio::test]
    async fn missing_app_state_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Storage::open(&tmp.path().join("state.db")).await.unwrap();
        let v = store.get_app_state("nope").await.unwrap();
        assert_eq!(v, None);
    }
}
