//! IPC server. Accepts UDS connections, reads newline-delimited JSON-RPC frames,
//! dispatches via a `Handler` trait, and pushes broadcast notifications.

use crate::error::{Error, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::broadcast;

#[cfg(test)]
mod tests {
    use super::super::transport::IpcStream;
    use super::*;
    use std::path::PathBuf;
    use tokio::io::AsyncBufReadExt;

    struct Echo;
    #[async_trait]
    impl Handler for Echo {
        async fn handle(
            &self,
            method: &str,
            params: serde_json::Value,
        ) -> Result<serde_json::Value> {
            match method {
                "echo" => Ok(params),
                "boom" => Err(Error::Internal("nope".into())),
                _ => Err(Error::NotFound(method.into())),
            }
        }
    }

    fn sock_path() -> PathBuf {
        let dir = tempfile::tempdir().unwrap().keep();
        dir.join("ipc.sock")
    }

    #[tokio::test]
    async fn echo_request_returns_result() {
        let path = sock_path();
        let (notif_tx, _) = broadcast::channel(8);
        let server = Server::new(Arc::new(Echo), notif_tx);
        let p = path.clone();
        let _h = tokio::spawn(async move { server.bind_and_serve(&p).await.unwrap() });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let mut sock = IpcStream::connect(&path).await.unwrap();
        sock.write_all(
            b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"echo\",\"params\":{\"a\":1}}\n",
        )
        .await
        .unwrap();
        let mut reader = BufReader::new(sock);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["id"], 1);
        assert_eq!(v["result"], serde_json::json!({"a":1}));
    }

    #[tokio::test]
    async fn unknown_method_returns_error() {
        let path = sock_path();
        let (notif_tx, _) = broadcast::channel(8);
        let server = Server::new(Arc::new(Echo), notif_tx);
        let p = path.clone();
        let _h = tokio::spawn(async move { server.bind_and_serve(&p).await.unwrap() });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let mut sock = IpcStream::connect(&path).await.unwrap();
        sock.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"nope\"}\n")
            .await
            .unwrap();
        let mut reader = BufReader::new(sock);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert!(v["error"].is_object());
    }

    #[tokio::test]
    async fn notifications_are_broadcast_to_subscribers() {
        let path = sock_path();
        let (notif_tx, _) = broadcast::channel(8);
        let tx2 = notif_tx.clone();
        let server = Server::new(Arc::new(Echo), notif_tx);
        let p = path.clone();
        let _h = tokio::spawn(async move { server.bind_and_serve(&p).await.unwrap() });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let sock = IpcStream::connect(&path).await.unwrap();
        let (rx, mut tx) = tokio::io::split(sock);
        // Subscribe to the event we'll broadcast below.
        tx.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"subscribe\",\"params\":{\"events\":[\"mcp.paused_changed\"]}}\n").await.unwrap();

        let mut reader = BufReader::new(rx);
        let mut line = String::new();
        // First line: the subscribe response. Reading it confirms the server has applied
        // the subscription before we push the broadcast.
        reader.read_line(&mut line).await.unwrap();
        line.clear();

        // Push a notification on the broadcast channel.
        let _ = tx2.send(Notification::McpPausedChanged { paused: true });

        // Next line: the broadcast notification.
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            reader.read_line(&mut line),
        )
        .await
        .unwrap()
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["method"], "mcp.paused_changed");
    }

    #[tokio::test]
    async fn notifications_fired_before_subscribe_do_not_arrive() {
        // Issue #6 regression: under the v0.1a code, the fan-out task
        // subscribed to the broadcast channel at accept-time but used an
        // empty `subscribed` filter until the client's `subscribe` RPC was
        // processed. A notification fired in that window was DROPPED — the
        // client's first subscribe message would race with the notification.
        //
        // Under the fixed code, the fan-out task is only armed after the
        // first `subscribe` arrives, so events fired before subscribe are
        // simply not delivered (forward-only semantics) — but events fired
        // AFTER subscribe are reliably delivered.
        let path = sock_path();
        let (notif_tx, _) = broadcast::channel(8);
        let tx2 = notif_tx.clone();
        let server = Server::new(Arc::new(Echo), notif_tx);
        let p = path.clone();
        let _h = tokio::spawn(async move { server.bind_and_serve(&p).await.unwrap() });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let sock = IpcStream::connect(&path).await.unwrap();
        let (rx, mut tx) = tokio::io::split(sock);
        let mut reader = BufReader::new(rx);

        // Connect, then fire a "pre-subscribe" notification BEFORE sending
        // any subscribe RPC. The fan-out task isn't armed yet, so this
        // event is dropped on the floor (intentional: forward-only).
        let _ = tx2.send(Notification::McpPausedChanged { paused: true });
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        // Now subscribe.
        tx.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"subscribe\",\"params\":{\"events\":[\"mcp.paused_changed\"]}}\n").await.unwrap();
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        line.clear();

        // Verify the pre-subscribe notification was NOT replayed: a fresh
        // post-subscribe one must come through, but no buffered earlier one.
        let _ = tx2.send(Notification::McpPausedChanged { paused: false });
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            reader.read_line(&mut line),
        )
        .await
        .unwrap()
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["method"], "mcp.paused_changed");
        assert_eq!(v["params"]["paused"], false);
    }

    #[tokio::test]
    async fn resubscribe_replaces_filter_without_double_fanout() {
        // Re-subscribing to a different event set should update the filter
        // but NOT spawn a second fan-out task (which would deliver each
        // notification twice).
        let path = sock_path();
        let (notif_tx, _) = broadcast::channel(8);
        let tx2 = notif_tx.clone();
        let server = Server::new(Arc::new(Echo), notif_tx);
        let p = path.clone();
        let _h = tokio::spawn(async move { server.bind_and_serve(&p).await.unwrap() });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let sock = IpcStream::connect(&path).await.unwrap();
        let (rx, mut tx) = tokio::io::split(sock);
        let mut reader = BufReader::new(rx);

        // Subscribe twice.
        tx.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"subscribe\",\"params\":{\"events\":[\"mcp.paused_changed\"]}}\n").await.unwrap();
        let mut buf = String::new();
        reader.read_line(&mut buf).await.unwrap(); // sub ack 1
        buf.clear();
        tx.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"subscribe\",\"params\":{\"events\":[\"mcp.paused_changed\",\"account.added\"]}}\n").await.unwrap();
        reader.read_line(&mut buf).await.unwrap(); // sub ack 2
        buf.clear();

        // Fire ONE notification. We should see exactly one frame, not two.
        let _ = tx2.send(Notification::McpPausedChanged { paused: true });
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            reader.read_line(&mut buf),
        )
        .await
        .unwrap()
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&buf).unwrap();
        assert_eq!(v["method"], "mcp.paused_changed");

        // No duplicate frame within 200ms.
        let mut dup = String::new();
        let res = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            reader.read_line(&mut dup),
        )
        .await;
        assert!(
            res.is_err(),
            "expected no duplicate notification, got: {dup}"
        );
    }
}

use super::messages::Notification;

#[async_trait]
pub trait Handler: Send + Sync {
    async fn handle(&self, method: &str, params: serde_json::Value) -> Result<serde_json::Value>;
}

pub struct Server {
    handler: Arc<dyn Handler>,
    notifications: broadcast::Sender<Notification>,
}

impl Server {
    pub fn new(handler: Arc<dyn Handler>, notifications: broadcast::Sender<Notification>) -> Self {
        Self {
            handler,
            notifications,
        }
    }

    /// Bind the IPC endpoint at `path` and serve forever. On Unix this is a
    /// UDS path (mkdir-p + chmod tightening happens inside `IpcListener::bind`);
    /// on Windows it's a named-pipe address (`\\.\pipe\mail-mcp-<USERNAME>`).
    pub async fn bind_and_serve(self, path: &Path) -> Result<()> {
        let listener = super::transport::IpcListener::bind(path)?;
        loop {
            let sock = listener.accept().await?;
            let handler = self.handler.clone();
            // Pass the Sender, not a Receiver: serve_conn defers
            // broadcast::subscribe() until the client's `subscribe` RPC arrives,
            // which closes the v0.1a race where a notification fired before
            // `subscribed` was populated and got silently dropped.
            let notif_sender = self.notifications.clone();
            tokio::spawn(async move {
                if let Err(e) = serve_conn(sock, handler, notif_sender).await {
                    tracing::warn!(?e, "ipc client error");
                }
            });
        }
    }
}

async fn serve_conn(
    sock: super::transport::IpcStream,
    handler: Arc<dyn Handler>,
    notif_sender: broadcast::Sender<Notification>,
) -> Result<()> {
    // tokio::io::split works on any AsyncRead+AsyncWrite, so this is the
    // same on Unix (UnixStream) and Windows (NamedPipeServer).
    let (rx, tx) = tokio::io::split(sock);
    let mut reader = BufReader::new(rx);
    let writer = Arc::new(tokio::sync::Mutex::new(tx));

    // Notification fan-out: holds the spawned task handle (Some) once we've
    // started forwarding, plus the live filter list. We start the fan-out only
    // after the first `subscribe` RPC so notifications fired before that
    // point can't slip past an empty filter (issue #6).
    let subscribed = Arc::new(tokio::sync::RwLock::new(Vec::<String>::new()));
    let mut fanout_started = false;

    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            return Ok(());
        }
        let req: JsonRpcRequest = match serde_json::from_str(line.trim()) {
            Ok(r) => r,
            Err(e) => {
                let resp = JsonRpcResponse {
                    jsonrpc: "2.0",
                    id: serde_json::Value::Null,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32700,
                        message: format!("parse: {e}"),
                    }),
                };
                let mut w = writer.lock().await;
                let _ = w
                    .write_all(serde_json::to_string(&resp).unwrap().as_bytes())
                    .await;
                let _ = w.write_all(b"\n").await;
                continue;
            }
        };
        let id = req.id.clone();
        let result = if req.method == "subscribe" {
            let events: Vec<String> = req
                .params
                .as_ref()
                .and_then(|p| p.get("events"))
                .and_then(|e| serde_json::from_value(e.clone()).ok())
                .unwrap_or_default();
            // Update the filter BEFORE arming the broadcast receiver so any
            // notification it sees is filtered against the right list.
            *subscribed.write().await = events.clone();
            // First subscribe: spawn the fan-out task. Subsequent `subscribe`
            // calls just update the filter list (clients can re-subscribe
            // with a different event set).
            if !fanout_started {
                fanout_started = true;
                let mut notifications = notif_sender.subscribe();
                let writer_for_notif = writer.clone();
                let subscribed_for_notif = subscribed.clone();
                tokio::spawn(async move {
                    while let Ok(n) = notifications.recv().await {
                        let (method, params) = notification_to_method_and_params(&n);
                        let allowed = subscribed_for_notif
                            .read()
                            .await
                            .iter()
                            .any(|m| m == method);
                        if !allowed {
                            continue;
                        }
                        let frame = serde_json::to_string(&JsonRpcNotification {
                            jsonrpc: "2.0",
                            method: method.to_string(),
                            params,
                        })
                        .unwrap();
                        let mut w = writer_for_notif.lock().await;
                        if w.write_all(frame.as_bytes()).await.is_err() {
                            break;
                        }
                        if w.write_all(b"\n").await.is_err() {
                            break;
                        }
                    }
                });
            }
            Ok(serde_json::json!({"subscribed": events}))
        } else {
            handler
                .handle(&req.method, req.params.unwrap_or(serde_json::Value::Null))
                .await
        };
        let resp = match result {
            Ok(v) => JsonRpcResponse {
                jsonrpc: "2.0",
                id,
                result: Some(v),
                error: None,
            },
            Err(e) => JsonRpcResponse {
                jsonrpc: "2.0",
                id,
                result: None,
                error: Some(JsonRpcError {
                    code: error_code(&e),
                    message: e.to_string(),
                }),
            },
        };
        let mut w = writer.lock().await;
        w.write_all(serde_json::to_string(&resp).unwrap().as_bytes())
            .await?;
        w.write_all(b"\n").await?;
    }
}

/// Split a `Notification` into its JSON-RPC `method` (the variant tag) and
/// `params` (the variant's content with the tag stripped).
///
/// The `Notification` enum is serialized with `#[serde(tag="method", content="params")]`,
/// so `serde_json::to_value(&n)` produces `{method, params}`. Wrapping THAT in
/// another `JsonRpcNotification.params` would double-wrap and produce
/// `{"params": {"method": "...", "params": {...}}}` on the wire — clients
/// would need to dig two levels deep. Instead, we extract just the inner
/// `params` content here so the wire frame is the natural shape:
/// `{"jsonrpc":"2.0","method":"<m>","params":{<content>}}`.
fn notification_to_method_and_params(n: &Notification) -> (&'static str, serde_json::Value) {
    let method = match n {
        Notification::ApprovalRequested(_) => "approval.requested",
        Notification::ApprovalResolved { .. } => "approval.resolved",
        Notification::AccountAdded(_) => "account.added",
        Notification::AccountRemoved { .. } => "account.removed",
        Notification::AccountNeedsReauth { .. } => "account.needs_reauth",
        Notification::McpPausedChanged { .. } => "mcp.paused_changed",
    };
    let mut tagged = serde_json::to_value(n).unwrap_or(serde_json::Value::Null);
    let params = tagged
        .get_mut("params")
        .map(|v| v.take())
        .unwrap_or(serde_json::Value::Null);
    (method, params)
}

fn error_code(e: &Error) -> i32 {
    match e {
        Error::NotFound(_) => -32601,
        Error::PermissionDenied(_) => -32002,
        Error::ApprovalRejected => -32003,
        Error::ApprovalTimeout => -32004,
        _ => -32000,
    }
}

#[derive(Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    #[serde(default)]
    id: serde_json::Value,
    method: String,
    #[serde(default)]
    params: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

#[derive(Serialize)]
struct JsonRpcNotification {
    jsonrpc: &'static str,
    method: String,
    params: serde_json::Value,
}
