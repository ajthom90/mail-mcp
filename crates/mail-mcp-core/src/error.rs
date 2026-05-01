use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("config error: {0}")]
    Config(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("permission denied: {0}")]
    PermissionDenied(String),

    #[error("provider error: {0}")]
    Provider(String),

    #[error("oauth error: {0}")]
    OAuth(String),

    #[error("storage error: {0}")]
    Storage(#[from] sqlx::Error),

    #[error("keychain error: {0}")]
    Keychain(#[from] keyring::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("approval timed out")]
    ApprovalTimeout,

    #[error("approval rejected by user")]
    ApprovalRejected,

    #[error("internal: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, Error>;
