//! mail-mcp core library.
#![allow(clippy::items_after_test_module)]

pub mod accounts;
pub mod cache;
pub mod error;
pub mod ipc;
pub mod logging;
pub mod oauth;
pub mod paths;
pub mod permissions;
pub mod providers;
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
