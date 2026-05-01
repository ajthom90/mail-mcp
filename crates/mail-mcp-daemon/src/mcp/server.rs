//! HTTP/SSE MCP server.
//!
//! For v0.1a we implement the JSON-RPC over HTTP path that MCP requires:
//!   POST /mcp        — single request/response (the MCP "Streamable HTTP" transport entry)
//!   GET  /mcp        — server-sent events stream for server-initiated messages
//!   Authorization: Bearer <token>  on every request (token from endpoint.json)
//!
//! We don't pull in the full rmcp::transport::http stack because we need fine-grained
//! control over policy enforcement around `tools/call`. The protocol surface required
//! for MCP clients is small enough to implement directly:
//!   initialize, tools/list, tools/call, ping
//!
//! Capabilities advertised: { "tools": { "listChanged": false } }.

use crate::mcp::dispatch::{dispatch, DispatchContext};
use crate::mcp::tools::tools;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::net::SocketAddr;
use std::sync::Arc;

#[derive(Clone)]
pub struct McpServer {
    state: Arc<McpState>,
}

pub struct McpState {
    pub ctx: DispatchContext,
    pub bearer_token: String,
    pub server_info: ServerInfo,
}

#[derive(Clone, Serialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
}

impl McpServer {
    pub fn new(state: McpState) -> Self {
        Self { state: Arc::new(state) }
    }

    pub fn router(&self) -> Router {
        Router::new()
            .route("/mcp", post(handle_post))
            .route("/mcp", get(handle_sse))
            .route("/health", get(handle_health))
            .with_state(self.state.clone())
    }

    /// Start serving on 127.0.0.1 with an OS-assigned port. Returns the bound address.
    pub async fn serve(self) -> anyhow::Result<(SocketAddr, tokio::task::JoinHandle<()>)> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let app = self.router();
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Ok((addr, handle))
    }
}

async fn handle_health(State(_state): State<Arc<McpState>>) -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

#[derive(Deserialize)]
struct JsonRpcReq {
    #[serde(default)]
    id: Value,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Serialize)]
struct JsonRpcOk<T: Serialize> {
    jsonrpc: &'static str,
    id: Value,
    result: T,
}

#[derive(Serialize)]
struct JsonRpcErr {
    jsonrpc: &'static str,
    id: Value,
    error: ErrBody,
}

#[derive(Serialize)]
struct ErrBody {
    code: i32,
    message: String,
}

fn ok<T: Serialize>(id: Value, result: T) -> Value {
    serde_json::to_value(JsonRpcOk { jsonrpc: "2.0", id, result }).unwrap()
}

fn err(id: Value, code: i32, message: impl Into<String>) -> Value {
    serde_json::to_value(JsonRpcErr {
        jsonrpc: "2.0",
        id,
        error: ErrBody { code, message: message.into() },
    })
    .unwrap()
}

fn check_auth(headers: &HeaderMap, expected: &str) -> bool {
    let Some(h) = headers.get(axum::http::header::AUTHORIZATION) else { return false; };
    let Ok(s) = h.to_str() else { return false; };
    s.strip_prefix("Bearer ").map(|t| t == expected).unwrap_or(false)
}

async fn handle_post(
    State(state): State<Arc<McpState>>,
    headers: HeaderMap,
    Json(req): Json<JsonRpcReq>,
) -> impl IntoResponse {
    if !check_auth(&headers, &state.bearer_token) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(err(req.id.clone(), -32001, "missing or invalid bearer token")),
        );
    }
    let resp = match req.method.as_str() {
        "initialize" => ok(req.id.clone(), serde_json::json!({
            "protocolVersion": "2025-06-18",
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": state.server_info,
        })),
        "ping" => ok(req.id.clone(), serde_json::json!({})),
        "tools/list" => {
            let list: Vec<_> = tools().into_iter().map(|t| serde_json::json!({
                "name": t.name,
                "description": t.description,
                "inputSchema": t.input_schema,
            })).collect();
            ok(req.id.clone(), serde_json::json!({"tools": list}))
        }
        "tools/call" => {
            let name = req.params.get("name").and_then(|v| v.as_str()).unwrap_or_default().to_string();
            let args = req.params.get("arguments").cloned().unwrap_or(Value::Null);
            // list_accounts is special: it's not provider-bound.
            if name == "list_accounts" {
                let items = mail_mcp_core::accounts::AccountStore::list(&state.ctx.storage).await;
                let v = items.map(|accs| serde_json::to_value(accs).unwrap()).unwrap_or(Value::Null);
                ok(req.id.clone(), serde_json::json!({
                    "content": [{"type":"text","text": v.to_string()}],
                    "structuredContent": v,
                    "isError": false
                }))
            } else if let Some(tool) = tools().into_iter().find(|t| t.name == name) {
                match dispatch(&state.ctx, &tool, args).await {
                    Ok(v) => ok(req.id.clone(), serde_json::json!({
                        "content": [{"type":"text","text": v.to_string()}],
                        "structuredContent": v,
                        "isError": false
                    })),
                    Err(e) => err(req.id.clone(), -32000, e.to_string()),
                }
            } else {
                err(req.id.clone(), -32601, format!("unknown tool: {name}"))
            }
        }
        other => err(req.id.clone(), -32601, format!("method not found: {other}")),
    };
    (StatusCode::OK, Json(resp))
}

async fn handle_sse(
    State(state): State<Arc<McpState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !check_auth(&headers, &state.bearer_token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    use axum::response::sse::{Event, KeepAlive, Sse};
    use futures::stream;
    // For v0.1a we don't push server-initiated MCP messages; just keep the connection alive.
    let stream = stream::pending::<Result<Event, std::convert::Infallible>>();
    Sse::new(stream).keep_alive(KeepAlive::default()).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mail_mcp_core::accounts::{AccountStore, NewAccount};
    use mail_mcp_core::permissions::approvals::ApprovalQueue;
    use mail_mcp_core::permissions::enforce::SessionTrust;
    use mail_mcp_core::storage::Storage;
    use mail_mcp_core::types::ProviderKind;
    use crate::mcp::dispatch::ProviderRegistry;
    use std::sync::atomic::AtomicBool;

    async fn server() -> (McpServer, String, SocketAddr) {
        let tmp = tempfile::tempdir().unwrap();
        let storage = Storage::open(&tmp.path().join("s.db")).await.unwrap();
        std::mem::forget(tmp);
        let _ = AccountStore::create(&storage, &NewAccount {
            label: "x".into(), provider: ProviderKind::Gmail, email: "x@x".into(),
            config: serde_json::json!({}), scopes: vec![],
        }).await.unwrap();
        let token = "test-bearer".to_string();
        let s = McpServer::new(McpState {
            ctx: DispatchContext {
                storage,
                providers: ProviderRegistry::new(),
                approvals: ApprovalQueue::new(std::time::Duration::from_secs(5)),
                trust: SessionTrust::new(),
                mcp_paused: Arc::new(AtomicBool::new(false)),
            },
            bearer_token: token.clone(),
            server_info: ServerInfo { name: "mail-mcp".into(), version: "0.1.0".into() },
        });
        let (addr, _h) = s.clone().serve().await.unwrap();
        (s, token, addr)
    }

    #[tokio::test]
    async fn rejects_missing_bearer() {
        let (_s, _tok, addr) = server().await;
        let url = format!("http://{addr}/mcp");
        let resp = reqwest::Client::new()
            .post(&url)
            .json(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"ping"}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn tools_list_returns_all_tools() {
        let (_s, tok, addr) = server().await;
        let url = format!("http://{addr}/mcp");
        let resp: serde_json::Value = reqwest::Client::new()
            .post(&url)
            .bearer_auth(&tok)
            .json(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
            .send().await.unwrap()
            .json().await.unwrap();
        let names: Vec<_> = resp["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
        assert!(names.contains(&"search".to_string()));
        assert!(names.contains(&"send_message".to_string()));
    }

    #[tokio::test]
    async fn initialize_returns_capabilities() {
        let (_s, tok, addr) = server().await;
        let url = format!("http://{addr}/mcp");
        let resp: serde_json::Value = reqwest::Client::new()
            .post(&url)
            .bearer_auth(&tok)
            .json(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize"}))
            .send().await.unwrap()
            .json().await.unwrap();
        assert!(resp["result"]["capabilities"]["tools"].is_object());
    }
}
