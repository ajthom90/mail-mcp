use std::process::Stdio;
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore] // run with: cargo test -p mail-mcp-daemon -- --ignored
async fn daemon_serves_mcp_tools_list() {
    // Spawn the daemon with a tempdir as root; we don't add accounts in this smoke test
    // (those require a real Google OAuth client). We just verify tools/list works.
    let dir = tempfile::tempdir().unwrap();
    let bin = env!("CARGO_BIN_EXE_mail-mcp-daemon");
    let mut child = std::process::Command::new(bin)
        .arg("--root")
        .arg(dir.path())
        .arg("--google-client-id")
        .arg("dummy")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    // Wait for endpoint.json to appear.
    let endpoint_path = dir.path().join("data/endpoint.json");
    for _ in 0..40 {
        if endpoint_path.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(endpoint_path.exists(), "endpoint.json should appear");

    let info: mail_mcp_core::ipc::messages::McpEndpointInfo =
        serde_json::from_slice(&std::fs::read(&endpoint_path).unwrap()).unwrap();

    let resp: serde_json::Value = reqwest::Client::new()
        .post(&info.url)
        .bearer_auth(&info.bearer_token)
        .json(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let names: Vec<_> = resp["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    assert!(names.contains(&"search".to_string()));
    assert!(names.contains(&"send_message".to_string()));

    let _ = child.kill();
    let _ = child.wait();
}
