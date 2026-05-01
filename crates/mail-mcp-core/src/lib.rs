//! mail-mcp core library.

pub mod accounts;
pub mod cache;
pub mod error;
pub mod logging;
pub mod paths;
pub mod permissions;
pub mod secrets;
pub mod storage;
pub mod types;

#[cfg(test)]
mod tests {
    use super::error::Error;

    #[test]
    fn error_display_includes_message() {
        let e = Error::Config("missing accounts dir".into());
        assert!(format!("{e}").contains("missing accounts dir"));
    }
}
