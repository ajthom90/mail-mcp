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
    use super::*;
    use std::path::PathBuf;
    use tokio::io::AsyncBufReadExt;
    use tokio::net::UnixStream;

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
        let mut sock = UnixStream::connect(&path).await.unwrap();
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
        let mut sock = UnixStream::connect(&path).await.unwrap();
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
        let sock = UnixStream::connect(&path).await.unwrap();
        let (rx, mut tx) = sock.into_split();
        // Subscribe to the event we'll broadcast below.
        tx.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"subscribe\",\"params\":{\"events\":[\"mcp.paused_changed\"]}}\n").await.unwrap();

        let mut reader = BufReader::new(rx);
        let mut line = String::new();
        // First line: the subscribe response. Reading it confirms the server has applied
        // the subscription before we push the broadcast (avoids a race where the fan-out
        // task sees the notification before `subscribed` is populated).
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

    /// Bind the UDS at `path` (unlinking any stale file) and serve forever.
    pub async fn bind_and_serve(self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
            }
        }
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        let listener = tokio::net::UnixListener::bind(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
        loop {
            let (sock, _addr) = listener.accept().await?;
            let handler = self.handler.clone();
            let notifications = self.notifications.subscribe();
            tokio::spawn(async move {
                if let Err(e) = serve_conn(sock, handler, notifications).await {
                    tracing::warn!(?e, "ipc client error");
                }
            });
        }
    }
}

async fn serve_conn(
    sock: tokio::net::UnixStream,
    handler: Arc<dyn Handler>,
    mut notifications: broadcast::Receiver<Notification>,
) -> Result<()> {
    let (rx, tx) = sock.into_split();
    let mut reader = BufReader::new(rx);
    let writer = Arc::new(tokio::sync::Mutex::new(tx));

    // Notification fan-out: forward every received notification onto the writer if the
    // client has subscribed to that method.
    let writer_for_notif = writer.clone();
    let subscribed = Arc::new(tokio::sync::RwLock::new(Vec::<String>::new()));
    let subscribed_for_notif = subscribed.clone();
    tokio::spawn(async move {
        while let Ok(n) = notifications.recv().await {
            let method = match &n {
                Notification::ApprovalRequested(_) => "approval.requested",
                Notification::ApprovalResolved { .. } => "approval.resolved",
                Notification::AccountAdded(_) => "account.added",
                Notification::AccountRemoved { .. } => "account.removed",
                Notification::AccountNeedsReauth { .. } => "account.needs_reauth",
                Notification::McpPausedChanged { .. } => "mcp.paused_changed",
            };
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
                params: serde_json::to_value(&n).unwrap(),
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
            *subscribed.write().await = events.clone();
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
