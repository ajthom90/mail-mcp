use mail_mcp_core::ipc::messages::McpEndpointInfo;
use std::io::Write as _;
use std::process::{Command, Stdio};

#[test]
fn shim_forwards_to_endpoint() {
    // Set up a mock server that echoes the body back as a JSON-RPC result.
    let server = std::sync::Arc::new(tokio::runtime::Runtime::new().unwrap().block_on(async {
        let s = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/mcp"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "jsonrpc":"2.0", "id":1, "result":{"ok":true}
                })),
            )
            .mount(&s)
            .await;
        s
    }));
    let info = McpEndpointInfo {
        url: format!("{}/mcp", server.uri()),
        bearer_token: "tok".into(),
        stdio_shim_path: None,
    };
    let dir = tempfile::tempdir().unwrap();
    let ep = dir.path().join("endpoint.json");
    std::fs::write(&ep, serde_json::to_vec(&info).unwrap()).unwrap();

    let bin = env!("CARGO_BIN_EXE_mail-mcp-stdio");
    let mut child = Command::new(bin)
        .arg("--endpoint-file")
        .arg(&ep)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(
        stdin,
        "{}",
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"ping"})
    )
    .unwrap();
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("\"ok\":true"));
}
