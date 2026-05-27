//! agent-start: thin HTTP client that talks to a running
//! `agent-start-host`. This binary intentionally stays small so it can
//! be `cargo install`-ed on a developer laptop to drive a remote host
//! over tailnet.

use agent_start_api::{
    CloneRequest, DeleteSessionResponse, ImportRequest, ProjectOpResponse, ProjectsBody,
    SessionsBody, StartSessionRequest, StartSessionResponse, UpdateCheckBody, VersionBody,
};
use anyhow::{anyhow, Context, Result};
use clap::{Args, Parser, Subcommand};
use serde::de::DeserializeOwned;
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(name = "agent-start", version, about = "agent-start CLI")]
struct Cli {
    /// Host base URL (defaults to the running host's manifest or http://127.0.0.1:3030).
    #[arg(long, global = true)]
    url: Option<String>,

    /// Emit JSON instead of human output where possible.
    #[arg(long, global = true)]
    json: bool,

    /// Suppress human-readable status/success messages (errors still print).
    #[arg(long, global = true)]
    quiet: bool,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Spawn `agent-start-host` (foreground, or detached with --daemon).
    Start {
        #[arg(long)]
        bind: Option<String>,
        #[arg(long)]
        port: Option<u16>,
        /// Path to the front-end SPA `dist/`. Passed through to the host.
        #[arg(long)]
        frontend_dist: Option<std::path::PathBuf>,
        /// Detach the host into the background and return immediately.
        #[arg(long)]
        daemon: bool,
    },
    /// Stop the running host process (best-effort via PID from manifest).
    Stop,
    /// Print whether the host is reachable + its version.
    Status,
    /// Manage projects on the host.
    Project {
        #[command(subcommand)]
        cmd: ProjectCmd,
    },
    /// Manage sessions on the host.
    Session {
        #[command(subcommand)]
        cmd: SessionCmd,
    },
    /// List projects via the running host (alias for `project list`).
    #[command(hide = true)]
    Projects,
    /// List active sessions via the running host (alias for `session list`).
    #[command(hide = true)]
    Sessions,
    /// Upgrade the installed binaries by re-running the official installer.
    Update(UpdateArgs),
    /// Print the agent-start CLI version.
    Version,
}

#[derive(Debug, Subcommand)]
enum ProjectCmd {
    /// List projects known to the host.
    List,
    /// Add a project by cloning a git URL or importing a local directory.
    Add(ProjectAddArgs),
    /// Remove a project by name.
    Remove { name: String },
}

#[derive(Debug, Args)]
struct ProjectAddArgs {
    /// Clone a git repository into the host's projects directory.
    #[arg(long, conflicts_with = "import")]
    clone: Option<String>,
    /// Import an existing local directory as a project.
    #[arg(long)]
    import: Option<String>,
    /// Override the project name (defaults to the repo/dir name).
    #[arg(long)]
    name: Option<String>,
}

#[derive(Debug, Subcommand)]
enum SessionCmd {
    /// List active sessions.
    List,
    /// Start a new session on the host.
    Create(SessionCreateArgs),
    /// Stop a session by name (tears down its worktree if it has one).
    Stop { name: String },
}

#[derive(Debug, Args)]
struct SessionCreateArgs {
    /// Path of the project to launch the session in.
    #[arg(long)]
    project: String,
    /// CLI to run (e.g. claude, codex). Defaults to the host's default.
    #[arg(long)]
    cli: Option<String>,
    /// Pass the CLI's skip-permissions flag (e.g. --dangerously-skip-permissions).
    #[arg(long)]
    skip_permissions: bool,
    /// Start the session inside a fresh git worktree.
    #[arg(long)]
    worktree: bool,
    /// Initial prompt handed to the agent CLI as a positional argument.
    #[arg(long)]
    prompt: Option<String>,
    /// Extra flag to pass through to the CLI (repeatable).
    #[arg(long = "arg")]
    extra: Vec<String>,
}

#[derive(Debug, Args)]
struct UpdateArgs {
    /// Pin to a specific release tag (e.g. v0.2.0). Defaults to latest.
    #[arg(long)]
    version: Option<String>,
    /// Force service re-registration even if no existing unit is detected.
    #[arg(long)]
    service: bool,
    /// Skip service re-registration even if a unit is detected.
    #[arg(long, conflicts_with = "service")]
    no_service: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let json = cli.json;
    let quiet = cli.quiet;
    match cli.cmd {
        Cmd::Version => {
            println!("agent-start {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Cmd::Start {
            bind,
            port,
            frontend_dist,
            daemon,
        } => start_host(bind, port, frontend_dist, daemon, quiet).await,
        Cmd::Stop => stop_host(quiet),
        Cmd::Status => status(&resolve_url(cli.url.as_deref())?, json, quiet).await,
        Cmd::Projects
        | Cmd::Project {
            cmd: ProjectCmd::List,
        } => list_projects(&resolve_url(cli.url.as_deref())?, json).await,
        Cmd::Sessions
        | Cmd::Session {
            cmd: SessionCmd::List,
        } => list_sessions(&resolve_url(cli.url.as_deref())?, json).await,
        Cmd::Project {
            cmd: ProjectCmd::Add(args),
        } => project_add(&resolve_url(cli.url.as_deref())?, args, json, quiet).await,
        Cmd::Project {
            cmd: ProjectCmd::Remove { name },
        } => project_remove(&resolve_url(cli.url.as_deref())?, &name, json, quiet).await,
        Cmd::Session {
            cmd: SessionCmd::Create(args),
        } => session_create(&resolve_url(cli.url.as_deref())?, args, json, quiet).await,
        Cmd::Session {
            cmd: SessionCmd::Stop { name },
        } => session_stop(&resolve_url(cli.url.as_deref())?, &name, json, quiet).await,
        Cmd::Update(args) => run_update(args),
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

/// Directory holding the host's runtime manifest. Must match the host's
/// `manifest::manifest_path()` (both resolve to `config_loader`'s
/// `host_state_dir()/runtime`, i.e. `$AGENT_START_HOME/runtime` or
/// `~/.agent-start/runtime`) so `stop`/`status` find the running host.
fn runtime_dir() -> std::path::PathBuf {
    config_loader::host_state_dir().join("runtime")
}

fn read_manifest_url() -> Option<String> {
    let raw = std::fs::read_to_string(runtime_dir().join("manifest.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    v.get("url")?
        .as_str()
        .map(|s| s.trim_end_matches('/').to_string())
}

/// Print a human-readable status/success line unless `--quiet` is set.
fn info(quiet: bool, msg: impl AsRef<str>) {
    if !quiet {
        println!("{}", msg.as_ref());
    }
}

// --- HTTP helpers ------------------------------------------------------------

/// Read the response body, surfacing the host's `{ "error": ... }` payload as
/// an error when the status is non-2xx.
async fn read_body<T: DeserializeOwned>(resp: reqwest::Response) -> Result<T> {
    let status = resp.status();
    let text = resp.text().await.context("failed to read response body")?;
    if !status.is_success() {
        let msg = serde_json::from_str::<agent_start_api::ErrorBody>(&text)
            .map(|e| e.error)
            .unwrap_or_else(|_| text.clone());
        return Err(anyhow!("host returned {status}: {msg}"));
    }
    serde_json::from_str(&text).with_context(|| format!("failed to parse response: {text}"))
}

async fn get_json<T: DeserializeOwned>(url: &str, path: &str) -> Result<T> {
    let resp = reqwest::Client::new()
        .get(format!("{url}{path}"))
        .send()
        .await
        .with_context(|| format!("GET {url}{path} failed (is the host running?)"))?;
    read_body(resp).await
}

async fn post_json<B: Serialize, T: DeserializeOwned>(
    url: &str,
    path: &str,
    body: &B,
) -> Result<T> {
    let resp = reqwest::Client::new()
        .post(format!("{url}{path}"))
        .json(body)
        .send()
        .await
        .with_context(|| format!("POST {url}{path} failed (is the host running?)"))?;
    read_body(resp).await
}

async fn delete_json<T: DeserializeOwned>(url: &str, path: &str) -> Result<T> {
    let resp = reqwest::Client::new()
        .delete(format!("{url}{path}"))
        .send()
        .await
        .with_context(|| format!("DELETE {url}{path} failed (is the host running?)"))?;
    read_body(resp).await
}

// --- start / stop ------------------------------------------------------------

async fn start_host(
    bind: Option<String>,
    port: Option<u16>,
    frontend_dist: Option<std::path::PathBuf>,
    daemon: bool,
    quiet: bool,
) -> Result<()> {
    let mut cmd = std::process::Command::new("agent-start-host");
    if let Some(b) = &bind {
        cmd.args(["--bind", b]);
    }
    if let Some(p) = port {
        cmd.args(["--port", &p.to_string()]);
    }
    if let Some(d) = &frontend_dist {
        cmd.arg("--frontend-dist").arg(d);
    }

    if !daemon {
        let status = cmd
            .status()
            .context("failed to spawn agent-start-host (is it on PATH?)")?;
        if !status.success() {
            return Err(anyhow!("agent-start-host exited with {status}"));
        }
        return Ok(());
    }

    // Daemon mode: detach the host so it outlives this CLI invocation and a
    // terminal hangup, redirect its output to a log file, and return once it
    // answers /v1/health. systemd-user / launchd remain the boot-persistent
    // path (see `agent-start update` / install.sh); this is the ad-hoc one.
    use std::process::Stdio;
    let log_dir = runtime_dir();
    let _ = std::fs::create_dir_all(&log_dir);
    let log_path = log_dir.join("host.log");
    let out = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("failed to open daemon log {}", log_path.display()))?;
    let err = out.try_clone()?;
    cmd.stdin(Stdio::null())
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err));

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Start a new session so the host isn't killed by SIGHUP when the
        // controlling terminal closes.
        unsafe {
            cmd.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }

    let child = cmd
        .spawn()
        .context("failed to spawn agent-start-host (is it on PATH?)")?;
    let pid = child.id();

    // Poll the URL implied by the bind/port we just launched (NOT the
    // resolved manifest URL — that may point at a different, already-running
    // host). Mirror the host's own arg/env precedence so the poll target
    // matches where it actually binds. A wildcard bind like 0.0.0.0 is
    // reached via loopback. The host writes runtime/manifest.json with its
    // own PID, so `stop` keeps working.
    let effective_bind = bind
        .clone()
        .or_else(|| std::env::var("AGENT_START_BIND").ok());
    let poll_host = match effective_bind.as_deref() {
        None | Some("") | Some("0.0.0.0") | Some("::") => "127.0.0.1",
        Some(b) => b,
    };
    let effective_port = port
        .or_else(|| {
            std::env::var("AGENT_START_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
        })
        .or_else(|| std::env::var("PORT").ok().and_then(|s| s.parse().ok()))
        .unwrap_or(3030);
    let url = format!("http://{poll_host}:{effective_port}");
    let mut reachable = false;
    for _ in 0..50 {
        if get_json::<VersionBody>(&url, "/v1/version").await.is_ok() {
            reachable = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    if reachable {
        info(
            quiet,
            format!("agent-start-host started (pid {pid}) at {url}"),
        );
    } else {
        info(
            quiet,
            format!(
                "agent-start-host spawned (pid {pid}) but did not answer at {url} yet; \
                 check {}",
                log_path.display()
            ),
        );
    }
    Ok(())
}

fn stop_host(quiet: bool) -> Result<()> {
    let path = runtime_dir().join("manifest.json");
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| anyhow!("no manifest at {}: {e}", path.display()))?;
    let v: serde_json::Value = serde_json::from_str(&raw)?;
    let pid = v
        .get("pid")
        .and_then(|p| p.as_i64())
        .ok_or_else(|| anyhow!("manifest has no pid"))?;

    // Terminate via the platform's process tool. The host installs a SIGTERM
    // handler on Unix; Windows has no SIGTERM, so use taskkill.
    #[cfg(unix)]
    let (tool, status) = (
        "kill",
        std::process::Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .status()?,
    );
    #[cfg(windows)]
    let (tool, status) = (
        "taskkill",
        std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .status()?,
    );

    if !status.success() {
        return Err(anyhow!("{tool} for pid {pid} failed: {status}"));
    }
    info(quiet, format!("terminated host pid {pid}"));
    Ok(())
}

// --- status ------------------------------------------------------------------

async fn status(url: &str, json: bool, quiet: bool) -> Result<()> {
    let res: VersionBody = get_json(url, "/v1/version").await?;
    // Best-effort update check; never fail status on its account.
    let update = get_json::<UpdateCheckBody>(url, "/v1/update-check")
        .await
        .ok();
    if json {
        let payload = serde_json::json!({ "version": res, "update": update });
        println!("{}", serde_json::to_string(&payload)?);
        return Ok(());
    }
    info(quiet, format!("{} {} @ {}", res.name, res.version, url));
    if let Some(u) = update {
        if u.available {
            if let Some(latest) = u.latest {
                info(
                    quiet,
                    format!("update available: {latest} (run 'agent-start update')"),
                );
            }
        }
    }
    Ok(())
}

// --- projects ----------------------------------------------------------------

async fn list_projects(url: &str, json: bool) -> Result<()> {
    let res: ProjectsBody = get_json(url, "/api/projects").await?;
    if json {
        println!("{}", serde_json::to_string(&res)?);
    } else {
        for p in res.projects {
            println!("{}\t{}\tgit={}", p.name, p.path, p.is_git);
        }
    }
    Ok(())
}

async fn project_add(url: &str, args: ProjectAddArgs, json: bool, quiet: bool) -> Result<()> {
    let res: ProjectOpResponse = match (args.clone, args.import) {
        (Some(clone_url), None) => {
            let body = CloneRequest {
                url: clone_url,
                name: args.name,
            };
            post_json(url, "/api/projects/clone", &body).await?
        }
        (None, Some(src)) => {
            let body = ImportRequest {
                src,
                name: args.name,
            };
            post_json(url, "/api/projects/import", &body).await?
        }
        _ => {
            return Err(anyhow!(
                "pass exactly one of --clone <url> or --import <path>"
            ))
        }
    };
    if json {
        println!("{}", serde_json::to_string(&res)?);
    } else {
        info(quiet, format!("added project {} at {}", res.name, res.path));
    }
    Ok(())
}

async fn project_remove(url: &str, name: &str, json: bool, quiet: bool) -> Result<()> {
    let res: ProjectOpResponse = delete_json(url, &format!("/api/projects/{name}")).await?;
    if json {
        println!("{}", serde_json::to_string(&res)?);
    } else {
        info(quiet, format!("removed project {}", res.name));
    }
    Ok(())
}

// --- sessions ----------------------------------------------------------------

async fn list_sessions(url: &str, json: bool) -> Result<()> {
    let res: SessionsBody = get_json(url, "/api/sessions").await?;
    if json {
        println!("{}", serde_json::to_string(&res)?);
    } else {
        for s in res.sessions {
            println!("{}\t{}\t{}\t{}", s.name, s.cli, s.path, s.created_at);
        }
    }
    Ok(())
}

async fn session_create(url: &str, args: SessionCreateArgs, json: bool, quiet: bool) -> Result<()> {
    let extra_args = if args.extra.is_empty() {
        None
    } else {
        Some(args.extra.join(" "))
    };
    let body = StartSessionRequest {
        project_path: args.project,
        cli: args.cli,
        skip_permissions: Some(args.skip_permissions),
        extra_args,
        create_worktree: Some(args.worktree),
        prompt: args.prompt,
    };
    let res: StartSessionResponse = post_json(url, "/api/sessions", &body).await?;
    if json {
        println!("{}", serde_json::to_string(&res)?);
    } else {
        info(
            quiet,
            format!("started session {} ({}) in {}", res.name, res.cli, res.cwd),
        );
    }
    Ok(())
}

async fn session_stop(url: &str, name: &str, json: bool, quiet: bool) -> Result<()> {
    let res: DeleteSessionResponse = delete_json(url, &format!("/api/sessions/{name}")).await?;
    if json {
        println!("{}", serde_json::to_string(&res)?);
    } else {
        let wt = if res.worktree_removed {
            " (worktree removed)"
        } else {
            ""
        };
        info(quiet, format!("stopped session {name}{wt}"));
    }
    Ok(())
}

// --- update ------------------------------------------------------------------

/// Forward to `agent-start-host update`, which re-runs the official installer
/// and re-registers any systemd-user / launchd service. Keeping the upgrade
/// logic in the host binary avoids duplicating it here.
fn run_update(args: UpdateArgs) -> Result<()> {
    let mut cmd = std::process::Command::new("agent-start-host");
    cmd.arg("update");
    if let Some(v) = &args.version {
        cmd.args(["--version", v]);
    }
    if args.service {
        cmd.arg("--service");
    }
    if args.no_service {
        cmd.arg("--no-service");
    }
    let status = cmd.status().map_err(|e| {
        anyhow!("failed to run 'agent-start-host update' ({e}); is agent-start-host on PATH?")
    })?;
    if !status.success() {
        return Err(anyhow!("agent-start-host update exited with {status}"));
    }
    Ok(())
}
