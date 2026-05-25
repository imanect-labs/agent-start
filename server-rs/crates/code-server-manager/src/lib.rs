//! Spawns and tracks `code-server` child processes — one per agent-start
//! session — so the host can reverse-proxy a browser-based VSCode UI to
//! a worktree. Mirrors the lifecycle of `pty-manager` but for an out-of-
//! process HTTP server we don't drive interactively.
//!
//! Discovery: `AGENT_START_CODE_SERVER_BIN` overrides; otherwise the
//! binary must be on `PATH` (no auto-download in v1).
//!
//! Auth: child binds `127.0.0.1:<auto>` with `--auth none`. Reachability
//! matches whatever the host itself exposes.

use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::process::{Child, Command};

#[derive(Debug, thiserror::Error)]
pub enum CodeServerError {
    #[error("code-server binary not found on PATH; install it or set AGENT_START_CODE_SERVER_BIN")]
    NotInstalled,
    #[error("worktree does not exist: {0}")]
    MissingWorktree(PathBuf),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("code-server did not start listening within {0:?}")]
    StartTimeout(Duration),
    #[error("spawn: {0}")]
    Spawn(String),
}

pub struct Instance {
    pub port: u16,
    pub pid: u32,
    child: Mutex<Option<Child>>,
}

impl Instance {
    pub fn port(&self) -> u16 {
        self.port
    }
    pub fn pid(&self) -> u32 {
        self.pid
    }
}

#[derive(Default)]
pub struct CodeServerManager {
    instances: Mutex<HashMap<String, Arc<Instance>>>,
}

impl CodeServerManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the bound port if an instance is already running for this
    /// session, regardless of whether the child is still alive (best
    /// effort — actual liveness is determined by the proxy connect).
    pub fn port_for(&self, session: &str) -> Option<u16> {
        self.instances.lock().get(session).map(|i| i.port)
    }

    pub fn get(&self, session: &str) -> Option<Arc<Instance>> {
        self.instances.lock().get(session).cloned()
    }

    /// Idempotent spawn. Returns the listening port, spawning the child
    /// only if no instance exists for this session.
    pub async fn ensure(
        &self,
        session: &str,
        worktree: &Path,
    ) -> Result<Arc<Instance>, CodeServerError> {
        self.ensure_with_base(session, worktree, None).await
    }

    /// Same as [`Self::ensure`] but lets the caller pass the absolute
    /// proxy path so code-server emits links prefixed with `/v/<name>`.
    pub async fn ensure_with_base(
        &self,
        session: &str,
        worktree: &Path,
        abs_proxy_base_path: Option<&str>,
    ) -> Result<Arc<Instance>, CodeServerError> {
        if let Some(existing) = self.instances.lock().get(session).cloned() {
            return Ok(existing);
        }
        if !worktree.exists() {
            return Err(CodeServerError::MissingWorktree(worktree.to_path_buf()));
        }
        let bin = resolve_binary()?;
        let port = allocate_port()?;
        let home = config_loader::agent_start_home().join("code-server");
        let user_data = home.join("user");
        let exts = home.join("exts");
        std::fs::create_dir_all(&user_data)?;
        std::fs::create_dir_all(&exts)?;

        let mut cmd = Command::new(&bin);
        cmd.arg("--auth")
            .arg("none")
            .arg("--bind-addr")
            .arg(format!("127.0.0.1:{port}"))
            .arg("--user-data-dir")
            .arg(&user_data)
            .arg("--extensions-dir")
            .arg(&exts)
            .arg("--disable-telemetry")
            .arg("--disable-update-check");
        if let Some(base) = abs_proxy_base_path {
            // code-server rewrites generated URLs to live under this
            // prefix, which is what makes the reverse proxy actually
            // usable (otherwise `/static/...` etc. miss the proxy).
            cmd.arg("--abs-proxy-base-path").arg(base);
        }
        // Inherit stdout/stderr so the host's terminal shows code-server
        // logs — invaluable when debugging the reverse proxy path.
        cmd.arg(worktree)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .stdin(Stdio::null())
            .kill_on_drop(true);

        let child = cmd
            .spawn()
            .map_err(|e| CodeServerError::Spawn(e.to_string()))?;
        let pid = child.id().unwrap_or(0);

        wait_until_listening(port, Duration::from_secs(15)).await?;

        let instance = Arc::new(Instance {
            port,
            pid,
            child: Mutex::new(Some(child)),
        });
        self.instances
            .lock()
            .insert(session.to_string(), instance.clone());
        tracing::info!(session, pid, port, "spawned code-server");
        Ok(instance)
    }

    /// Kill the child and forget the instance. No-op if absent.
    pub async fn kill(&self, session: &str) {
        let inst = self.instances.lock().remove(session);
        if let Some(inst) = inst {
            let child = inst.child.lock().take();
            if let Some(mut child) = child {
                let _ = child.start_kill();
                let _ = child.wait().await;
            }
            tracing::info!(session, "killed code-server");
        }
    }

    /// Kill every tracked instance. Best-effort cleanup on host shutdown.
    pub async fn kill_all(&self) {
        let names: Vec<String> = self.instances.lock().keys().cloned().collect();
        for name in names {
            self.kill(&name).await;
        }
    }
}

fn resolve_binary() -> Result<PathBuf, CodeServerError> {
    if let Some(p) = std::env::var_os("AGENT_START_CODE_SERVER_BIN") {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return Ok(pb);
        }
        return Err(CodeServerError::NotInstalled);
    }
    which_in_path("code-server").ok_or(CodeServerError::NotInstalled)
}

fn which_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let cand = dir.join(name);
        if cand.is_file() {
            return Some(cand);
        }
    }
    None
}

fn allocate_port() -> Result<u16, CodeServerError> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

async fn wait_until_listening(port: u16, deadline: Duration) -> Result<(), CodeServerError> {
    let start = Instant::now();
    loop {
        if TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
            return Ok(());
        }
        if start.elapsed() >= deadline {
            return Err(CodeServerError::StartTimeout(deadline));
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocates_distinct_ports() {
        let a = allocate_port().unwrap();
        let b = allocate_port().unwrap();
        assert_ne!(a, 0);
        assert_ne!(b, 0);
    }

    #[test]
    fn resolve_missing_binary_errors() {
        std::env::set_var(
            "AGENT_START_CODE_SERVER_BIN",
            "/nonexistent/code-server-xyz",
        );
        let r = resolve_binary();
        std::env::remove_var("AGENT_START_CODE_SERVER_BIN");
        assert!(matches!(r, Err(CodeServerError::NotInstalled)));
    }
}
