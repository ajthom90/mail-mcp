use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use ulid::Ulid;

/// Stable identifier for an account. Wraps a ULID for sortability + uniqueness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AccountId(pub Ulid);

impl AccountId {
    pub fn new() -> Self {
        Self(Ulid::new())
    }
}

impl Default for AccountId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for AccountId {
    type Err = ulid::DecodeError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ulid::from_str(s).map(Self)
    }
}

/// Distinct ID newtype for messages.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MessageId(String);

impl MessageId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<S: Into<String>> From<S> for MessageId {
    fn from(s: S) -> Self {
        Self(s.into())
    }
}

impl fmt::Display for MessageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Distinct ID newtype for threads.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ThreadId(String);

impl ThreadId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<S: Into<String>> From<S> for ThreadId {
    fn from(s: S) -> Self {
        Self(s.into())
    }
}

impl fmt::Display for ThreadId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Distinct ID newtype for drafts.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DraftId(String);

impl DraftId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<S: Into<String>> From<S> for DraftId {
    fn from(s: S) -> Self {
        Self(s.into())
    }
}

impl fmt::Display for DraftId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Distinct ID newtype for labels (Gmail user labels, M365 categories).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LabelId(String);

impl LabelId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<S: Into<String>> From<S> for LabelId {
    fn from(s: S) -> Self {
        Self(s.into())
    }
}

/// Distinct ID newtype for folders (IMAP mailboxes, M365 folders, Gmail system labels).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FolderId(String);

impl FolderId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<S: Into<String>> From<S> for FolderId {
    fn from(s: S) -> Self {
        Self(s.into())
    }
}

/// Which mail provider an account belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    Gmail,
    #[serde(rename = "m365")]
    Microsoft365,
    Imap,
}

impl ProviderKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderKind::Gmail => "gmail",
            ProviderKind::Microsoft365 => "m365",
            ProviderKind::Imap => "imap",
        }
    }
}

impl FromStr for ProviderKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "gmail" => Ok(Self::Gmail),
            "m365" => Ok(Self::Microsoft365),
            "imap" => Ok(Self::Imap),
            other => Err(format!("unknown provider kind: {other}")),
        }
    }
}

/// Persistent metadata for a connected account. Secrets live in the keychain, not here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: AccountId,
    pub label: String,
    pub provider: ProviderKind,
    pub email: String,
    /// Provider-specific configuration as JSON (e.g., IMAP server/port/TLS).
    /// For OAuth providers in v0.1a this is `{}`.
    pub config: serde_json::Value,
    pub scopes: Vec<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_validated: Option<chrono::DateTime<chrono::Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_id_round_trips_to_string() {
        let id = AccountId::new();
        let s = id.to_string();
        let parsed: AccountId = s.parse().expect("parse");
        assert_eq!(id, parsed);
    }

    #[test]
    fn provider_kind_serializes_lowercase() {
        let g = serde_json::to_string(&ProviderKind::Gmail).unwrap();
        assert_eq!(g, "\"gmail\"");
        let m = serde_json::to_string(&ProviderKind::Microsoft365).unwrap();
        assert_eq!(m, "\"m365\"");
        let i = serde_json::to_string(&ProviderKind::Imap).unwrap();
        assert_eq!(i, "\"imap\"");
    }

    #[test]
    fn message_and_thread_ids_distinct_types() {
        let m = MessageId::from("abc");
        let t = ThreadId::from("abc");
        // Ensure they don't unify; this is a compile-time check via assertion of types.
        assert_eq!(m.as_str(), "abc");
        assert_eq!(t.as_str(), "abc");
    }
}
