//! mail-mcp core library.

pub mod error;
pub mod types;

#[cfg(test)]
mod tests {
    // (existing test)
    use super::error::Error;

    #[test]
    fn error_display_includes_message() {
        let e = Error::Config("missing accounts dir".into());
        assert!(format!("{e}").contains("missing accounts dir"));
    }
}
