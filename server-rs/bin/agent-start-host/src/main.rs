//! agent-start-host: the long-running HTTP/WebSocket daemon that drives
//! the agent-start Web UI. Replaces `server.mjs` + `server/terminal.mjs`.

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

mod app;
mod chat;
mod http;
mod manifest;
mod sessions;
mod update;
mod ws;
mod ws_chat;

#[derive(Debug, Parser)]
#[command(name = "agent-start-host", version, about = "agent-start host daemon")]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,

    /// Address to bind. Defaults to 127.0.0.1.
    #[arg(long, global = true)]
    bind: Option<String>,

    /// Port to listen on. Defaults to 3030.
    #[arg(long, global = true)]
    port: Option<u16>,

    /// Path to the built front-end SPA (Vite `dist/`). When set and the
    /// directory exists, the host serves it as a fallback for unmatched
    /// routes (with `index.html` for SPA deep links). Skip during dev —
    /// the front is then served by `vp dev` on its own port.
    #[arg(long, global = true)]
    frontend_dist: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Start the host server (foreground).
    Start,
    /// Print server version + build info.
    Version,
    /// Upgrade agent-start-host in place by re-running the official installer.
    Update(update::UpdateArgs),
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let cli = Cli::parse();

    let bind = cli
        .bind
        .or_else(|| std::env::var("AGENT_START_BIND").ok())
        .unwrap_or_else(|| "127.0.0.1".to_string());
    let port = cli
        .port
        .or_else(|| {
            std::env::var("AGENT_START_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
        })
        .or_else(|| std::env::var("PORT").ok().and_then(|s| s.parse().ok()))
        .unwrap_or(3030);

    let frontend_dist = cli
        .frontend_dist
        .or_else(|| std::env::var("AGENT_START_FRONTEND_DIST").ok().map(PathBuf::from))
        .and_then(|p| {
            if p.is_dir() {
                Some(p)
            } else {
                tracing::warn!(path = %p.display(), "frontend-dist path does not exist; static serving disabled");
                None
            }
        });

    match cli.cmd.unwrap_or(Cmd::Start) {
        Cmd::Version => {
            println!("agent-start-host {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Cmd::Start => app::run(bind, port, frontend_dist).await,
        Cmd::Update(args) => update::run(args),
    }
}

fn init_tracing() {
    let filter = EnvFilter::try_from_env("AGENT_START_LOG")
        .or_else(|_| EnvFilter::try_new("agent_start_host=info,tower_http=info,axum=info"))
        .unwrap();
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .compact()
        .init();
}
