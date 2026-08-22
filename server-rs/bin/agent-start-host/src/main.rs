//! agent-start-host: the long-running HTTP/WebSocket daemon that drives
//! the agent-start Web UI. Replaces `server.mjs` + `server/terminal.mjs`.

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

mod app;
mod chat;
mod cluster;
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

    /// Which halves to run: `all` (default — control plane plus an
    /// in-process node, identical to previous releases), `control`
    /// (scheduler + UI only), or `node` (agent execution only).
    #[arg(long, global = true, value_name = "all|control|node")]
    role: Option<String>,

    /// Control-plane URL this node registers with. Required for
    /// `--role node`; accepts `http(s)://host:port`.
    #[arg(long, global = true, value_name = "URL")]
    join_url: Option<String>,

    /// Join token issued by `POST /api/join-tokens`. Only needed the
    /// first time a node registers — after that it keeps its own.
    #[arg(long, global = true, value_name = "TOKEN")]
    join_token: Option<String>,

    /// Shared secret the control plane accepts from any node, instead of
    /// per-node join tokens.
    #[arg(long, global = true, value_name = "TOKEN")]
    cluster_token: Option<String>,

    /// Name this node registers under. Defaults to the hostname.
    #[arg(long, global = true, value_name = "NAME")]
    node_name: Option<String>,

    /// Cap on concurrent sessions this node accepts.
    #[arg(long, global = true, value_name = "N")]
    max_sessions: Option<u32>,

    /// Scheduling label, `key=value`. Repeatable.
    #[arg(long = "label", global = true, value_name = "KEY=VALUE")]
    labels: Vec<String>,

    /// Session backend this node provides. Only `process` today;
    /// container and microVM backends land in later phases.
    #[arg(long, global = true, value_name = "NAME")]
    executor: Option<String>,
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
    let cluster_args = build_cluster_args(&cli)?;

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
        Cmd::Start => {
            // A node runs no HTTP surface of its own: it dials the
            // control plane and does what it is told.
            if cluster_args.role == cluster::Role::Node {
                return cluster::run_node_only(cluster_args).await;
            }
            app::run(bind, port, frontend_dist, cluster_args).await
        }
        Cmd::Update(args) => update::run(args),
    }
}

/// Fold command-line flags and `AGENT_START_*` environment overrides
/// into the cluster configuration. Flags win over the environment, and
/// the environment wins over the single-host defaults — the same
/// precedence the bind/port options already use.
fn build_cluster_args(cli: &Cli) -> Result<cluster::ClusterArgs> {
    let role_raw = cli
        .role
        .clone()
        .or_else(|| std::env::var("AGENT_START_ROLE").ok())
        .unwrap_or_default();
    let role: cluster::Role = role_raw.parse().map_err(anyhow::Error::msg)?;

    let mut labels = Vec::new();
    for raw in &cli.labels {
        labels.push(cluster::parse_label(raw).map_err(anyhow::Error::msg)?);
    }
    if labels.is_empty() {
        if let Ok(env) = std::env::var("AGENT_START_NODE_LABELS") {
            for raw in env.split(',').filter(|s| !s.trim().is_empty()) {
                labels.push(cluster::parse_label(raw).map_err(anyhow::Error::msg)?);
            }
        }
    }

    Ok(cluster::ClusterArgs {
        role,
        join_url: cli
            .join_url
            .clone()
            .or_else(|| std::env::var("AGENT_START_JOIN_URL").ok()),
        join_token: cli
            .join_token
            .clone()
            .or_else(|| std::env::var("AGENT_START_JOIN_TOKEN").ok()),
        node_name: cli
            .node_name
            .clone()
            .or_else(|| std::env::var("AGENT_START_NODE_NAME").ok()),
        max_sessions: cli.max_sessions.or_else(|| {
            std::env::var("AGENT_START_MAX_SESSIONS")
                .ok()
                .and_then(|s| s.parse().ok())
        }),
        labels,
        executor: cli
            .executor
            .clone()
            .or_else(|| std::env::var("AGENT_START_EXECUTOR").ok()),
        cluster_token: cli
            .cluster_token
            .clone()
            .or_else(|| std::env::var("AGENT_START_CLUSTER_TOKEN").ok())
            .filter(|s| !s.is_empty()),
    })
}

fn init_tracing() {
    // The cluster crates are named explicitly: without them a
    // `--role node` process would log its startup line and then go
    // silent, hiding registration, reconnects and assignment failures —
    // exactly what an operator needs to see.
    let filter = EnvFilter::try_from_env("AGENT_START_LOG")
        .or_else(|_| {
            EnvFilter::try_new(
                "agent_start_host=info,cluster_node=info,cluster_control=info,executor=info,\
                 tower_http=info,axum=info",
            )
        })
        .unwrap();
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .compact()
        .init();
}
