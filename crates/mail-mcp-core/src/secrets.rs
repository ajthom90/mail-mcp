use crate::error::{Error, Result};
use crate::types::AccountId;
use keyring::Entry;

/// Categories of per-account secrets. Each maps to a distinct keychain "account" name
/// under the same service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyKind {
    RefreshToken,
    ImapPassword,
    SmtpPassword,
}

impl KeyKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            KeyKind::RefreshToken => "refresh_token",
            KeyKind::ImapPassword => "imap_password",
            KeyKind::SmtpPassword => "smtp_password",
        }
    }
}

/// Service name format used in the OS keychain. One service per account, distinguishable
/// by suffix so users can audit credentials in Keychain Access etc.
pub fn service_name(id: AccountId) -> String {
    format!("mail-mcp.{id}")
}

/// Cross-platform OS keychain access. Cheap to construct; not Send-bound, not Clone.
#[derive(Default)]
pub struct SecretStore;

impl SecretStore {
    pub fn new() -> Self {
        Self
    }

    pub fn set(&self, id: AccountId, kind: KeyKind, value: &str) -> Result<()> {
        let entry = Entry::new(&service_name(id), kind.as_str())?;
        entry.set_password(value)?;
        Ok(())
    }

    pub fn get(&self, id: AccountId, kind: KeyKind) -> Result<Option<String>> {
        let entry = Entry::new(&service_name(id), kind.as_str())?;
        match entry.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(Error::Keychain(e)),
        }
    }

    pub fn delete(&self, id: AccountId, kind: KeyKind) -> Result<()> {
        let entry = Entry::new(&service_name(id), kind.as_str())?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(Error::Keychain(e)),
        }
    }

    /// Best-effort cleanup of every secret bound to an account (called on account delete).
    pub fn purge(&self, id: AccountId) -> Result<()> {
        let _ = self.delete(id, KeyKind::RefreshToken);
        let _ = self.delete(id, KeyKind::ImapPassword);
        let _ = self.delete(id, KeyKind::SmtpPassword);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_name_format() {
        let id = AccountId::new();
        let s = service_name(id);
        assert!(s.starts_with("mail-mcp."));
        assert!(s.ends_with(&id.to_string()));
    }

    #[test]
    fn key_kind_strings_are_stable() {
        assert_eq!(KeyKind::RefreshToken.as_str(), "refresh_token");
        assert_eq!(KeyKind::ImapPassword.as_str(), "imap_password");
        assert_eq!(KeyKind::SmtpPassword.as_str(), "smtp_password");
    }

    // Round-trip tests are kept out of CI (they need a real keychain).
    // Run locally with: cargo test -p mail-mcp-core --features keychain-tests
    #[cfg(feature = "keychain-tests")]
    #[test]
    fn round_trip_secret() {
        let id = AccountId::new();
        let secrets = SecretStore::new();
        secrets
            .set(id, KeyKind::RefreshToken, "tok-abc-123")
            .unwrap();
        let got = secrets.get(id, KeyKind::RefreshToken).unwrap();
        assert_eq!(got.as_deref(), Some("tok-abc-123"));
        secrets.delete(id, KeyKind::RefreshToken).unwrap();
        assert_eq!(secrets.get(id, KeyKind::RefreshToken).unwrap(), None);
    }
}
