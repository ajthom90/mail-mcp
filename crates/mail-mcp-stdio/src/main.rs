use anyhow::{Context, Result};
use clap::Parser;
use mail_mcp_core::ipc::messages::McpEndpointInfo;
use std::io::{BufRead, Write};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about = "MCP stdio shim — forwards stdio MCP frames to the local mail-mcp daemon over HTTP.")]
struct Args {
    /// Override the path to endpoint.json. Default: platform data dir.
    #[arg(long, env = "MAIL_MCP_ENDPOINT_FILE")]
    endpoint_file: Option<PathBuf>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let endpoint_path = args
        .endpoint_file
        .or_else(|| {
            mail_mcp_core::paths::Paths::default_for_user()
                .ok()
                .map(|p| p.endpoint_json())
        })
        .context("could not locate endpoint.json")?;
    let info: McpEndpointInfo = serde_json::from_slice(
        &std::fs::read(&endpoint_path)
            .with_context(|| format!("reading {}", endpoint_path.display()))?,
    )?;

    // Use a sync I/O loop. MCP stdio framing per spec: each message is a single line of
    // JSON terminated by '\n'. (We do not handle Content-Length framing; that style is
    // optional in the spec and not used by Claude Desktop.)
    let stdin = std::io::stdin().lock();
    let mut stdout = std::io::stdout().lock();
    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!("mail-mcp-stdio/", env!("CARGO_PKG_VERSION")))
        .build()?;

    for line in stdin.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let resp = client
            .post(&info.url)
            .bearer_auth(&info.bearer_token)
            .header("Content-Type", "application/json")
            .body(line)
            .send()?;
        let status = resp.status();
        let body = resp.text()?;
        if !status.is_success() {
            // Synthesize a JSON-RPC error so the MCP client surfaces something useful.
            let synth = serde_json::json!({
                "jsonrpc":"2.0",
                "id": null,
                "error": {"code": -32099, "message": format!("daemon http {}: {}", status, body)}
            });
            writeln!(stdout, "{}", synth)?;
        } else {
            writeln!(stdout, "{}", body.trim_end())?;
        }
        stdout.flush()?;
    }
    Ok(())
}
