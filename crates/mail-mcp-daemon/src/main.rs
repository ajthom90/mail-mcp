mod ipc_handler;
mod lifecycle;
mod mcp;

use anyhow::Result;
use clap::Parser;
use mail_mcp_core::ipc::messages::McpEndpointInfo;
use mail_mcp_core::ipc::server::Server as IpcServer;
use mail_mcp_core::oauth;
use mail_mcp_core::paths::Paths;
use mail_mcp_core::permissions::approvals::ApprovalQueue;
use mail_mcp_core::permissions::enforce::SessionTrust;
use mail_mcp_core::secrets::SecretStore;
use mail_mcp_core::storage::Storage;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, Mutex};

/// mail-mcp-daemon — long-running per-user mail bridge for MCP clients.
#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    /// Override the data root (for testing). When set, all dirs become <root>/{data,logs,cache,run}.
    #[arg(long, env = "MAIL_MCP_ROOT")]
    root: Option<PathBuf>,
    /// Google OAuth client_id. Required for Gmail account add. In release builds this is
    /// baked into the binary via build.rs; for development pass it explicitly.
    #[arg(long, env = "MAIL_MCP_GOOGLE_CLIENT_ID")]
    google_client_id: String,
    /// Microsoft Graph OAuth client_id. Required for Microsoft 365 account add.
    /// Same baking convention as `google_client_id`.
    #[arg(long, env = "MAIL_MCP_MICROSOFT_CLIENT_ID")]
    microsoft_client_id: String,
    /// Force the bound HTTP port (otherwise OS-assigned). Useful in dev.
    #[arg(long, env = "MAIL_MCP_HTTP_PORT")]
    http_port: Option<u16>,
    /// Path on disk where we write the stdio-shim binary location for endpoint.json.
    #[arg(long, env = "MAIL_MCP_STDIO_SHIM")]
    stdio_shim_path: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let paths = match args.root {
        Some(r) => Paths::with_root(r),
        None => Paths::default_for_user()?,
    };
    paths.ensure_dirs()?;

    let _guard = mail_mcp_core::logging::init_tracing(paths.logs_dir(), false)?;
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        "starting mail-mcp-daemon"
    );

    let _pid = lifecycle::PidLock::acquire(&paths.pid_file())?;

    let storage = Storage::open(&paths.state_db()).await?;
    let secrets = SecretStore::new();
    let http = reqwest::Client::builder()
        .user_agent(concat!("mail-mcp/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let oauth_cfg_google = oauth::google::config(args.google_client_id);
    let oauth_cfg_microsoft = oauth::microsoft::config(args.microsoft_client_id);

    let providers = mcp::dispatch::ProviderRegistry::new();
    ipc_handler::hydrate_providers(
        &storage,
        &secrets,
        &http,
        &oauth_cfg_google,
        &oauth_cfg_microsoft,
        &providers,
    )
    .await?;

    let approvals = ApprovalQueue::new(Duration::from_secs(300));
    let trust = SessionTrust::new();
    let mcp_paused = Arc::new(AtomicBool::new(false));

    let bearer_token = lifecycle::fresh_bearer_token();
    let server_info = mcp::server::ServerInfo {
        name: "mail-mcp".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    };
    let mcp_state = mcp::server::McpState {
        ctx: mcp::dispatch::DispatchContext {
            storage: storage.clone(),
            providers: providers.clone(),
            approvals: approvals.clone(),
            trust: trust.clone(),
            mcp_paused: mcp_paused.clone(),
        },
        bearer_token: bearer_token.clone(),
        server_info: server_info.clone(),
    };
    let mcp_server = mcp::server::McpServer::new(mcp_state);
    let listener = if let Some(p) = args.http_port {
        tokio::net::TcpListener::bind(format!("127.0.0.1:{p}")).await?
    } else {
        tokio::net::TcpListener::bind("127.0.0.1:0").await?
    };
    let addr = listener.local_addr()?;
    let app = mcp_server.router();
    let http_handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let endpoint = McpEndpointInfo {
        url: lifecycle::endpoint_url(addr),
        bearer_token: bearer_token.clone(),
        stdio_shim_path: args.stdio_shim_path.map(|p| p.display().to_string()),
    };
    lifecycle::write_endpoint(&paths.endpoint_json(), &endpoint)?;

    // IPC notifications: same channel feeds both the IPC server and approval-event forwarding.
    let (notif_tx, _notif_rx) = broadcast::channel(64);
    // Forward approval events into the IPC notification channel.
    let mut approval_events = approvals.subscribe();
    let notif_for_approvals = notif_tx.clone();
    tokio::spawn(async move {
        while let Ok(evt) = approval_events.recv().await {
            use mail_mcp_core::ipc::messages::Notification;
            use mail_mcp_core::permissions::approvals::ApprovalEvent;
            let n = match evt {
                ApprovalEvent::Requested(p) => Notification::ApprovalRequested(p),
                ApprovalEvent::Resolved { id, decision } => Notification::ApprovalResolved {
                    id: id.to_string(),
                    decision: match decision {
                        mail_mcp_core::permissions::approvals::ApprovalDecision::Approve => {
                            "approve".into()
                        }
                        mail_mcp_core::permissions::approvals::ApprovalDecision::Reject => {
                            "reject".into()
                        }
                    },
                },
            };
            let _ = notif_for_approvals.send(n);
        }
    });

    let handler = ipc_handler::DaemonHandler {
        storage: storage.clone(),
        secrets,
        providers,
        approvals,
        started_at: Instant::now(),
        mcp_endpoint: endpoint,
        mcp_paused,
        oauth_cfg_google,
        oauth_cfg_microsoft,
        http,
        paths: paths.clone(),
        notif_tx: notif_tx.clone(),
        pending_oauth: Arc::new(Mutex::new(std::collections::HashMap::new())),
    };
    let ipc_server = IpcServer::new(Arc::new(handler), notif_tx.clone());
    let ipc_path = paths.ipc_socket();
    let ipc_handle = tokio::spawn(async move {
        if let Err(e) = ipc_server.bind_and_serve(&ipc_path).await {
            tracing::error!(?e, "ipc server failed");
        }
    });

    tracing::info!(addr = %addr, "mcp http server bound");

    // Wait for SIGINT / SIGTERM.
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigint = signal(SignalKind::interrupt())?;
        let mut sigterm = signal(SignalKind::terminate())?;
        tokio::select! {
            _ = sigint.recv() => tracing::info!("SIGINT received"),
            _ = sigterm.recv() => tracing::info!("SIGTERM received"),
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await?;
    }

    tracing::info!("shutting down");
    http_handle.abort();
    ipc_handle.abort();
    Ok(())
}
