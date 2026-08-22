//! Pluggable session backends.
//!
//! An `Executor` owns the *sandbox*, not the terminal. It prepares an
//! execution environment for one session and then answers a single
//! question: given the command the user asked for, what should the node
//! actually run under a PTY? For the `process` backend the answer is
//! "exactly that command". For a container backend it becomes
//! `docker exec …`; for a microVM, a console attach. PTY multiplexing,
//! ring buffers, and streaming stay in `pty-manager` for every backend,
//! so there is one terminal implementation rather than one per sandbox.
//!
//! Backends beyond `process` arrive in later phases (see
//! `docs/multinode-cloud-design.ja.md` §2.2); the trait is shaped now so
//! the node runtime never has to learn about them.

use async_trait::async_trait;
use cluster_proto::{IsolationProfile, Resources};
use std::path::PathBuf;

mod process;
pub use process::ProcessExecutor;

#[derive(Debug, thiserror::Error)]
pub enum ExecError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Backend(String),
}

/// One session's request for an execution environment, after the node
/// has resolved the source tree.
#[derive(Debug, Clone)]
pub struct SessionSpec {
    pub session: String,
    /// Working directory on the node (worktree, or the project itself).
    pub cwd: PathBuf,
    pub shell: String,
    /// Command line for the agent CLI. Empty = interactive login shell.
    pub command: String,
    pub env: Vec<(String, String)>,
    pub requests: Resources,
}

/// A live execution environment. `process` carries no state beyond the
/// session name; container backends put their container id in `id`.
#[derive(Debug, Clone)]
pub struct Handle {
    pub id: String,
    pub backend: &'static str,
}

/// What the node hands to `pty-manager` to get a terminal.
#[derive(Debug, Clone)]
pub struct LaunchPlan {
    pub shell: String,
    pub command: String,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ResourceUsage {
    pub cpu_millis: u32,
    pub mem_mb: u32,
}

#[async_trait]
pub trait Executor: Send + Sync {
    fn name(&self) -> &'static str;

    /// The isolation this backend actually provides. Advertised to the
    /// control plane at registration and used as a scheduling filter.
    fn profile(&self) -> IsolationProfile;

    /// Prepare the environment. Called once per session, before any PTY
    /// exists, and is allowed to be slow (image pull, VM boot).
    async fn create(&self, spec: &SessionSpec) -> Result<Handle, ExecError>;

    /// Translate the user's command into what the node runs on the host.
    /// Pure and synchronous: it is called again for every extra window.
    fn launch_plan(&self, handle: &Handle, spec: &SessionSpec) -> LaunchPlan;

    /// Tear the environment down. Must be idempotent — it runs both on
    /// explicit delete and on the child's own exit.
    async fn destroy(&self, handle: &Handle) -> Result<(), ExecError>;

    async fn stat(&self, _handle: &Handle) -> Result<ResourceUsage, ExecError> {
        Ok(ResourceUsage::default())
    }
}

/// Resolve a backend by config name. Unknown names fall back to
/// `process` with a warning rather than refusing to boot: a node whose
/// operator typo'd `docekr` should still join the cluster and say so.
pub fn build(name: &str) -> Box<dyn Executor> {
    match name {
        "" | "process" => Box::new(ProcessExecutor),
        other => {
            tracing::warn!(
                executor = %other,
                "unknown executor backend; falling back to `process`"
            );
            Box::new(ProcessExecutor)
        }
    }
}
