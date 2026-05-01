//! mail-mcp core library.

pub mod error;

#[cfg(test)]
mod tests {
    use super::error::Error;

    #[test]
    fn error_display_includes_message() {
        let e = Error::Config("missing accounts dir".into());
        assert!(format!("{e}").contains("missing accounts dir"));
    }
}
