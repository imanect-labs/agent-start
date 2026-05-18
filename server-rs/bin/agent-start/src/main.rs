//! agent-start: thin HTTP client that talks to a running
//! `agent-start-host`. This binary intentionally stays small so it can
//! be `cargo install`-ed on a developer laptop to drive a remote host
//! over tailnet.

use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "agent-start", version, about = "agent-start CLI")]
struct Cli {
    /// Host base URL (defaults to the running host's manifest or http://127.0.0.1:3030).
    #[arg(long, global = true)]
    url: Option<String>,

    /// Emit JSON instead of human output where possible.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Spawn `agent-start-host` (foreground).
    Start {
        #[arg(long)]
        bind: Option<String>,
        #[arg(long)]
        port: Option<u16>,
        /// Path to the front-end SPA `dist/`. Passed through to the host.
        #[arg(long)]
        frontend_dist: Option<std::path::PathBuf>,
    },
    /// Stop the running host process (best-effort via PID from manifest).
    Stop,
    /// Print whether the host is reachable + its version.
    Status,
    /// List projects via the running host.
    Projects,
    /// List active sessions via the running host.
    Sessions,
    /// Print the agent-start CLI version.
    Version,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Version => {
            println!("agent-start {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Cmd::Start {
            bind,
            port,
            frontend_dist,
        } => start_host(bind, port, frontend_dist).await,
        Cmd::Stop => stop_host(),
        Cmd::Status => status(&resolve_url(cli.url.as_deref())?, cli.json).await,
        Cmd::Projects => projects(&resolve_url(cli.url.as_deref())?, cli.json).await,
        Cmd::Sessions => sessions(&resolve_url(cli.url.as_deref())?, cli.json).await,
    }
}

fn resolve_url(url: Option<&str>) -> Result<String> {
    if let Some(u) = url {
        return Ok(u.trim_end_matches('/').to_string());
    }
    if let Ok(env) = std::env::var("AGENT_START_URL") {
        return Ok(env.trim_end_matches('/').to_string());
    }
    if let Some(u) = read_manifest_url() {
        return Ok(u);
    }
    Ok("http://127.0.0.1:3030".to_string())
}

fn read_manifest_url() -> Option<String> {
    let path = dirs::data_local_dir()
        .or_else(dirs::home_dir)?
        .join("agent-start")
        .join("runtime")
        .join("manifest.json");
    let raw = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    v.get("url")?
        .as_str()
        .map(|s| s.trim_end_matches('/').to_string())
}

async fn start_host(
    bind: Option<String>,
    port: Option<u16>,
    frontend_dist: Option<std::path::PathBuf>,
) -> Result<()> {
    let mut cmd = std::process::Command::new("agent-start-host");
    if let Some(b) = bind {
        cmd.args(["--bind", &b]);
    }
    if let Some(p) = port {
        cmd.args(["--port", &p.to_string()]);
    }
    if let Some(d) = frontend_dist {
        cmd.arg("--frontend-dist").arg(d);
    }
    let status = cmd.status()?;
    if !status.success() {
        return Err(anyhow!("agent-start-host exited with {status}"));
    }
    Ok(())
}

fn stop_host() -> Result<()> {
    let path = dirs::data_local_dir()
        .or_else(dirs::home_dir)
        .ok_or_else(|| anyhow!("no data dir"))?
        .join("agent-start")
        .join("runtime")
        .join("manifest.json");
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| anyhow!("no manifest at {}: {e}", path.display()))?;
    let v: serde_json::Value = serde_json::from_str(&raw)?;
    let pid = v
        .get("pid")
        .and_then(|p| p.as_i64())
        .ok_or_else(|| anyhow!("manifest has no pid"))?;
    let status = std::process::Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .status()?;
    if !status.success() {
        return Err(anyhow!("kill -TERM {pid} failed: {status}"));
    }
    println!("sent SIGTERM to pid {pid}");
    Ok(())
}

async fn status(url: &str, json: bool) -> Result<()> {
    let res: agent_start_api::VersionBody = reqwest::Client::new()
        .get(format!("{url}/v1/version"))
        .send()
        .await?
        .json()
        .await?;
    if json {
        println!("{}", serde_json::to_string(&res)?);
    } else {
        println!("{} {} @ {}", res.name, res.version, url);
    }
    Ok(())
}

async fn projects(url: &str, json: bool) -> Result<()> {
    let res: agent_start_api::ProjectsBody = reqwest::Client::new()
        .get(format!("{url}/api/projects"))
        .send()
        .await?
        .json()
        .await?;
    if json {
        println!("{}", serde_json::to_string(&res)?);
    } else {
        for p in res.projects {
            println!("{}\t{}\tgit={}", p.name, p.path, p.is_git);
        }
    }
    Ok(())
}

async fn sessions(url: &str, json: bool) -> Result<()> {
    let res: agent_start_api::SessionsBody = reqwest::Client::new()
        .get(format!("{url}/api/sessions"))
        .send()
        .await?
        .json()
        .await?;
    if json {
        println!("{}", serde_json::to_string(&res)?);
    } else {
        for s in res.sessions {
            println!("{}\t{}\t{}\t{}", s.name, s.cli, s.path, s.created_at);
        }
    }
    Ok(())
}
