mod ipc_client;

use anyhow::Result;
use clap::{Parser, Subcommand};
use ipc_client::IpcClient;
use mail_mcp_core::paths::Paths;
use std::path::PathBuf;

/// mail-mcp-admin — manage the local mail-mcp daemon (accounts, permissions, status).
#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Cli {
    /// Override the IPC socket path. Default: platform runtime dir.
    #[arg(long, env = "MAIL_MCP_SOCKET")]
    socket: Option<PathBuf>,
    /// Override the data root. Default: platform data dir.
    #[arg(long, env = "MAIL_MCP_ROOT")]
    root: Option<PathBuf>,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Show daemon status.
    Status,
    /// Print the MCP endpoint URL + bearer token (for configuring AI clients).
    Endpoint,
    /// Pause the MCP server (refuses tool calls until resumed).
    Pause,
    /// Resume the MCP server.
    Resume,
    /// Account management.
    #[command(subcommand)]
    Accounts(AccountsCmd),
    /// Permission management.
    #[command(subcommand)]
    Permissions(PermissionsCmd),
    /// Approval queue.
    #[command(subcommand)]
    Approvals(ApprovalsCmd),
}

#[derive(Subcommand, Debug)]
enum AccountsCmd {
    /// List connected accounts.
    List,
    /// Add a new Gmail account via OAuth (opens browser).
    AddGmail {
        /// Friendly label for the account; defaults to the email.
        #[arg(long)]
        label: Option<String>,
    },
    /// Remove an account by id.
    Remove { id: String },
}

#[derive(Subcommand, Debug)]
enum PermissionsCmd {
    /// Show all permissions for an account.
    Get { account_id: String },
    /// Set a permission policy.
    Set {
        account_id: String,
        /// One of: read | modify | trash | draft | send
        category: String,
        /// One of: allow | confirm | session | draftify | block
        policy: String,
    },
}

#[derive(Subcommand, Debug)]
enum ApprovalsCmd {
    /// List pending approvals.
    List,
    /// Approve a pending request.
    Approve { id: String },
    /// Reject a pending request.
    Reject { id: String },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let paths = match cli.root {
        Some(r) => Paths::with_root(r),
        None => Paths::default_for_user()?,
    };
    let socket = cli.socket.unwrap_or_else(|| paths.ipc_socket());
    let mut client = IpcClient::connect(&socket).await?;

    match cli.cmd {
        Cmd::Status => {
            let v = client.call("status", serde_json::json!({})).await?;
            println!("{}", serde_json::to_string_pretty(&v)?);
        }
        Cmd::Endpoint => {
            let v = client.call("mcp.endpoint", serde_json::json!({})).await?;
            println!("{}", serde_json::to_string_pretty(&v)?);
        }
        Cmd::Pause => {
            client
                .call("mcp.pause", serde_json::json!({"paused": true}))
                .await?;
            println!("MCP paused");
        }
        Cmd::Resume => {
            client
                .call("mcp.pause", serde_json::json!({"paused": false}))
                .await?;
            println!("MCP resumed");
        }
        Cmd::Accounts(AccountsCmd::List) => {
            let v = client.call("accounts.list", serde_json::json!({})).await?;
            println!("{}", serde_json::to_string_pretty(&v)?);
        }
        Cmd::Accounts(AccountsCmd::AddGmail { label }) => {
            let v = client
                .call(
                    "accounts.add_oauth",
                    serde_json::json!({"provider": "gmail"}),
                )
                .await?;
            let challenge_id = v["challenge_id"].as_str().unwrap_or("").to_string();
            let auth_url = v["auth_url"].as_str().unwrap_or("").to_string();
            println!("Opening browser:\n  {auth_url}\nWaiting for sign-in to complete...");
            // Best-effort browser launch.
            #[cfg(target_os = "macos")]
            {
                let _ = std::process::Command::new("open").arg(&auth_url).status();
            }
            #[cfg(target_os = "linux")]
            {
                let _ = std::process::Command::new("xdg-open")
                    .arg(&auth_url)
                    .status();
            }
            #[cfg(target_os = "windows")]
            {
                let _ = std::process::Command::new("rundll32")
                    .args(["url.dll,FileProtocolHandler", &auth_url])
                    .status();
            }

            let v = client
                .call(
                    "accounts.complete_oauth",
                    serde_json::json!({
                        "challenge_id": challenge_id,
                        "label": label.unwrap_or_default(),
                    }),
                )
                .await?;
            println!("Account added:\n{}", serde_json::to_string_pretty(&v)?);
        }
        Cmd::Accounts(AccountsCmd::Remove { id }) => {
            client
                .call("accounts.remove", serde_json::json!({"account_id": id}))
                .await?;
            println!("removed");
        }
        Cmd::Permissions(PermissionsCmd::Get { account_id }) => {
            let v = client
                .call(
                    "permissions.get",
                    serde_json::json!({"account_id": account_id}),
                )
                .await?;
            println!("{}", serde_json::to_string_pretty(&v)?);
        }
        Cmd::Permissions(PermissionsCmd::Set {
            account_id,
            category,
            policy,
        }) => {
            client
                .call(
                    "permissions.set",
                    serde_json::json!({
                        "account_id": account_id, "category": category, "policy": policy
                    }),
                )
                .await?;
            println!("set");
        }
        Cmd::Approvals(ApprovalsCmd::List) => {
            let v = client.call("approvals.list", serde_json::json!({})).await?;
            println!("{}", serde_json::to_string_pretty(&v)?);
        }
        Cmd::Approvals(ApprovalsCmd::Approve { id }) => {
            client
                .call(
                    "approvals.decide",
                    serde_json::json!({"id": id, "decision": "approve"}),
                )
                .await?;
            println!("approved");
        }
        Cmd::Approvals(ApprovalsCmd::Reject { id }) => {
            client
                .call(
                    "approvals.decide",
                    serde_json::json!({"id": id, "decision": "reject"}),
                )
                .await?;
            println!("rejected");
        }
    }
    Ok(())
}
