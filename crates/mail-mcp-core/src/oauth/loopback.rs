//! Localhost listener that catches the OAuth `redirect_uri` callback.

use crate::error::{Error, Result};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn captures_code_and_state() {
        let listener = LoopbackListener::bind("expected-state").await.unwrap();
        let port = listener.port();
        // Simulate the browser redirect.
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            let mut sock = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
                .await
                .unwrap();
            sock.write_all(b"GET /callback?code=abc123&state=expected-state HTTP/1.1\r\nHost: localhost\r\n\r\n").await.unwrap();
            // Read at least the response header so the server fully writes.
            let mut buf = [0u8; 1024];
            let _ = tokio::time::timeout(Duration::from_secs(1), sock.read(&mut buf)).await;
        });
        let captured = listener
            .await_callback(Duration::from_secs(2))
            .await
            .unwrap();
        assert_eq!(captured.code, "abc123");
    }

    #[tokio::test]
    async fn ignores_mismatched_state_and_keeps_waiting() {
        // Issue #4: a spurious local connection (browser preconnect, port
        // scanner) that hits the loopback before the real browser callback
        // must NOT abort the OAuth flow. Send a bad-state probe first, then
        // the real callback, and expect the real code to come through.
        let listener = LoopbackListener::bind("expected-state").await.unwrap();
        let port = listener.port();
        tokio::spawn(async move {
            // First connection: wrong state.
            tokio::time::sleep(Duration::from_millis(10)).await;
            let mut probe = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
                .await
                .unwrap();
            let _ = probe
                .write_all(
                    b"GET /callback?code=ignored&state=BAD HTTP/1.1\r\nHost: localhost\r\n\r\n",
                )
                .await;
            let mut buf = [0u8; 1024];
            let _ = tokio::time::timeout(Duration::from_secs(1), probe.read(&mut buf)).await;

            // Second connection: the real browser callback.
            tokio::time::sleep(Duration::from_millis(20)).await;
            let mut real = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
                .await
                .unwrap();
            real.write_all(
                b"GET /callback?code=real-code&state=expected-state HTTP/1.1\r\nHost: localhost\r\n\r\n",
            )
            .await
            .unwrap();
            let mut buf = [0u8; 1024];
            let _ = tokio::time::timeout(Duration::from_secs(1), real.read(&mut buf)).await;
        });
        let captured = listener
            .await_callback(Duration::from_secs(2))
            .await
            .unwrap();
        assert_eq!(captured.code, "real-code");
    }

    #[tokio::test]
    async fn provider_error_param_aborts() {
        // OAuth provider explicitly returned ?error=access_denied — the user
        // declined consent. That IS a hard failure; surface it instead of
        // waiting forever.
        let listener = LoopbackListener::bind("expected-state").await.unwrap();
        let port = listener.port();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            let mut sock = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
                .await
                .unwrap();
            let _ = sock
                .write_all(b"GET /callback?error=access_denied HTTP/1.1\r\nHost: localhost\r\n\r\n")
                .await;
            let mut buf = [0u8; 1024];
            let _ = tokio::time::timeout(Duration::from_secs(1), sock.read(&mut buf)).await;
        });
        let res = listener.await_callback(Duration::from_secs(2)).await;
        match res {
            Err(Error::OAuth(msg)) => assert!(msg.contains("access_denied"), "got: {msg}"),
            other => panic!("expected OAuth error containing access_denied, got {other:?}"),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn timeout_returns_err() {
        let listener = LoopbackListener::bind("expected-state").await.unwrap();
        // Don't connect; advance time past the timeout.
        let fut = listener.await_callback(Duration::from_secs(60));
        tokio::time::advance(Duration::from_secs(120)).await;
        let res = fut.await;
        assert!(matches!(res, Err(Error::OAuth(_))));
    }
}

pub struct LoopbackListener {
    listener: TcpListener,
    expected_state: String,
}

#[derive(Debug, Clone)]
pub struct CapturedCallback {
    pub code: String,
}

impl LoopbackListener {
    /// Bind a fresh ephemeral port on 127.0.0.1.
    pub async fn bind(expected_state: impl Into<String>) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        Ok(Self {
            listener,
            expected_state: expected_state.into(),
        })
    }

    pub fn port(&self) -> u16 {
        self.listener.local_addr().unwrap().port()
    }

    pub fn redirect_uri(&self) -> String {
        format!("http://127.0.0.1:{}/callback", self.port())
    }

    /// Await the OAuth callback. Loops accepting connections until one carries
    /// a matching `state` value, or the overall `timeout` elapses. Mismatched
    /// or otherwise-spurious connections (browser preconnects, port scans) get
    /// a 400 response and the listener keeps waiting — fixes the v0.1a bug
    /// where any local connection that beat the browser to the port killed
    /// the entire OAuth flow.
    pub async fn await_callback(self, timeout: Duration) -> Result<CapturedCallback> {
        let LoopbackListener {
            listener,
            expected_state,
        } = self;
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(Error::OAuth(
                    "timed out waiting for browser callback".into(),
                ));
            }
            let accept = tokio::time::timeout(remaining, listener.accept()).await;
            let (sock, _) = match accept {
                Err(_) => {
                    return Err(Error::OAuth(
                        "timed out waiting for browser callback".into(),
                    ))
                }
                Ok(Err(e)) => return Err(Error::OAuth(format!("accept failed: {e}"))),
                Ok(Ok(pair)) => pair,
            };
            match handle_one(sock, &expected_state).await {
                Ok(Some(captured)) => return Ok(captured),
                Ok(None) => {
                    // State mismatch / malformed / probe — keep waiting.
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
    }
}

/// Handle one inbound HTTP request. Returns:
///   - Ok(Some(callback)) on success (state matches, code is present)
///   - Ok(None) on a spurious request (state mismatch, browser probe, etc.)
///     — caller should keep waiting for the real callback.
///   - Err for I/O errors that mean the listener itself is broken.
async fn handle_one(mut sock: TcpStream, expected_state: &str) -> Result<Option<CapturedCallback>> {
    let mut buf = [0u8; 4096];
    let n = sock.read(&mut buf).await?;
    let req = std::str::from_utf8(&buf[..n]).unwrap_or("");
    let request_line = req.lines().next().unwrap_or("");
    // GET /callback?code=...&state=... HTTP/1.1
    let mut parts = request_line.split_whitespace();
    let _method = parts.next();
    let target = parts.next().unwrap_or("");
    let qs = target.split_once('?').map(|(_, q)| q).unwrap_or("");
    let mut code: Option<String> = None;
    let mut state: Option<String> = None;
    let mut error: Option<String> = None;
    for pair in qs.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let v = percent_encoding::percent_decode_str(v)
            .decode_utf8_lossy()
            .into_owned();
        match k {
            "code" => code = Some(v),
            "state" => state = Some(v),
            "error" => error = Some(v),
            _ => {}
        }
    }

    let state_matches = state.as_deref() == Some(expected_state);
    let (status, body) = if let Some(e) = &error {
        (
            "400 Bad Request",
            format!("OAuth error: {e}\nYou can close this window."),
        )
    } else if !state_matches {
        (
            "400 Bad Request",
            "OAuth state mismatch. You can close this window.".into(),
        )
    } else if code.is_some() {
        (
            "200 OK",
            "Sign-in complete. You can close this window.".into(),
        )
    } else {
        (
            "400 Bad Request",
            "Missing code parameter. You can close this window.".into(),
        )
    };

    let resp = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = sock.write_all(resp.as_bytes()).await;
    let _ = sock.shutdown().await;

    // Anything other than a clean code+state-match means: not the callback we're
    // waiting for. The browser will eventually deliver the real one — keep
    // listening. (An OAuth provider error, however, is a hard failure: the user
    // declined consent or the request was malformed; surface it.)
    if let Some(e) = error {
        return Err(Error::OAuth(format!("provider returned error: {e}")));
    }
    if !state_matches {
        return Ok(None);
    }
    match code {
        Some(c) => Ok(Some(CapturedCallback { code: c })),
        None => Ok(None),
    }
}
