//! Tracing setup + redaction. The `redact` function is reusable from any caller that wants
//! to scrub a free-form string before logging it. The `init_tracing` helper wires up a
//! daily-rotating file appender plus a stderr layer.

use std::path::Path;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_email_in_string() {
        let s = redact("recipient: alice@example.com, subject: hi");
        assert!(!s.contains("alice@example.com"));
        assert!(s.contains("[redacted-email]"));
    }

    #[test]
    fn redact_oauth_code_in_string() {
        let s = redact("got code 4/0AY0e-g7-1234567890abcdef in callback");
        assert!(!s.contains("4/0AY0e-g7-1234567890abcdef"));
        assert!(s.contains("[redacted-code]"));
    }

    #[test]
    fn redact_bearer_token() {
        let s = redact("Authorization: Bearer ya29.aXcdefghijklmnopqrstuvwxyz0123");
        assert!(!s.contains("ya29.aXcdefghijklmnopqrstuvwxyz0123"));
        assert!(s.contains("[redacted-bearer]"));
    }
}

/// Replace sensitive substrings with stable redaction markers. Used in custom log layers
/// and ad-hoc by callers when including potentially-sensitive strings in fields.
pub fn redact(s: &str) -> String {
    use std::sync::OnceLock;
    static EMAIL: OnceLock<regex::Regex> = OnceLock::new();
    static CODE: OnceLock<regex::Regex> = OnceLock::new();
    static BEARER: OnceLock<regex::Regex> = OnceLock::new();
    let email =
        EMAIL.get_or_init(|| regex::Regex::new(r"\b[\w._%+-]+@[\w.-]+\.[A-Za-z]{2,}\b").unwrap());
    let code = CODE.get_or_init(|| regex::Regex::new(r"\b4/[A-Za-z0-9_\-]{12,}\b").unwrap());
    let bearer =
        BEARER.get_or_init(|| regex::Regex::new(r"(?i)bearer\s+[A-Za-z0-9._\-]{20,}").unwrap());
    let s = email.replace_all(s, "[redacted-email]");
    let s = code.replace_all(&s, "[redacted-code]");
    let s = bearer.replace_all(&s, "[redacted-bearer]");
    s.into_owned()
}

/// Initialize the global tracing subscriber. Call exactly once at process start.
/// Returns a guard that must outlive the process — drop it on shutdown to flush.
pub fn init_tracing(logs_dir: &Path, json: bool) -> std::io::Result<WorkerGuard> {
    use tracing_subscriber::fmt;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    std::fs::create_dir_all(logs_dir)?;
    let appender = rolling::daily(logs_dir, "daemon.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(appender);

    let env = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,mail_mcp=debug"));

    let file_layer: Box<dyn tracing_subscriber::Layer<_> + Send + Sync> = if json {
        Box::new(fmt::layer().json().with_writer(non_blocking))
    } else {
        Box::new(fmt::layer().with_writer(non_blocking))
    };

    let stderr_layer = fmt::layer().with_writer(std::io::stderr).with_target(false);

    tracing_subscriber::registry()
        .with(env)
        .with(file_layer)
        .with(stderr_layer)
        .init();

    Ok(guard)
}
