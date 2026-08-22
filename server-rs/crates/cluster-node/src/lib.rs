//! The node agent: the half of agent-start that actually runs agents.
//!
//! A node owns PTYs, worktrees and the local repository mirror cache.
//! It never listens on a socket — it dials the control plane and does
//! what it is told over that one connection — so a machine behind NAT
//! (a laptop, a home box, a Pod) can join a cluster without any inbound
//! reachability.
//!
//! `--role all` runs this same runtime in-process over a loopback link,
//! which is what keeps the single-host default on the same code path as
//! a real cluster instead of a second, quietly diverging one.

use anyhow::{anyhow, Result};
use cluster_proto::{
    AssignOk, AssignSpec, ControlFrame, FinalizeOk, FinalizeSpec, Hello, IsolationProfile,
    NodeFrame, NodeLink, ProjectRef, SessionEventKind, StreamTarget,
};
use executor::Executor;
use metrics_probe::MetricsProbe;
use parking_lot::Mutex;
use pty_manager::{PtyManager, PtySession, PtySpawnSpec};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

mod client;
mod identity;
pub use client::{connect_url, run_remote, CONNECT_PATH};
pub use identity::{load_identity, save_identity, Identity};

/// Registration was refused by the control plane. A distinct type
/// because the client has to tell "retry forever" from "stop": the
/// credential is wrong and no amount of reconnecting will fix it.
#[derive(Debug, thiserror::Error)]
#[error("registration rejected: {0}")]
pub struct Rejected(pub String);

/// How often the node reports in when the control plane has not said
/// otherwise. Three missed beats is what marks a node NotReady, so this
/// also sets the detection latency for a dead machine.
pub const DEFAULT_HEARTBEAT_SECS: u64 = 10;

#[derive(Debug, Clone)]
pub struct NodeConfig {
    pub name: String,
    /// Operator cap on concurrent sessions, independent of hardware.
    pub max_sessions: u32,
    pub labels: Vec<(String, String)>,
    /// Executor backend name (`process` today).
    pub executor: String,
    /// Where to persist the node id + long-lived token. `None` for the
    /// in-process node, which re-registers from scratch every boot.
    pub identity_path: Option<PathBuf>,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            name: hostname(),
            max_sessions: 4,
            labels: Vec::new(),
            executor: "process".into(),
            identity_path: None,
        }
    }
}

pub fn hostname() -> String {
    std::env::var("AGENT_START_NODE_NAME")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .or_else(|| std::env::var("HOSTNAME").ok().filter(|s| !s.is_empty()))
        .unwrap_or_else(|| "node".into())
}

/// A relayed PTY stream in flight.
struct Channel {
    session: Arc<PtySession>,
    pump: tokio::task::JoinHandle<()>,
    /// Distinguishes this channel from a later one that reused the same
    /// `ch`, so a pump cleaning up after itself cannot delete its
    /// successor's entry.
    token: u64,
}

/// Where one session's files live on this node.
#[derive(Debug, Clone)]
struct SessionPaths {
    /// Directory the agent ran in — the worktree when there is one.
    cwd: PathBuf,
    /// The repository the worktree was cut from, when it was cut at all.
    repo_root: PathBuf,
    worktree: Option<PathBuf>,
    /// When the agent exited, if it has. `Finalize` arrives *after* the
    /// exit, so the entry cannot be dropped the moment the process ends;
    /// it is swept once no finalize can still be coming.
    exited_at: Option<Instant>,
}

/// How long a finished session's paths are kept for a `Finalize` that
/// may still arrive. Comfortably past the control plane's finalize
/// timeout, so the sweep never removes an entry someone is about to ask
/// for.
const PATHS_RETENTION: Duration = Duration::from_secs(900);

pub struct NodeRuntime {
    cfg: NodeConfig,
    pty: Arc<PtyManager>,
    executor: Box<dyn Executor>,
    /// One lock per project, held while its mirror is being prepared.
    /// Two sessions for the same cold project arrive together often —
    /// a burst from the scheduler is the normal case — and without this
    /// the second `git clone --mirror` finds the first one's
    /// half-written directory, wipes it, and breaks both.
    mirror_locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    /// Sandbox handles per session, so `destroy` can undo what `create` did.
    handles: Mutex<HashMap<String, executor::Handle>>,
    /// Where each session's files ended up. `Finalize` arrives after the
    /// agent has exited — and therefore after the sandbox handle is
    /// gone — so the paths are kept separately and outlive the process.
    paths: Mutex<HashMap<String, SessionPaths>>,
    channels: Mutex<HashMap<u32, Channel>>,
    /// Set once the link is up; the PTY exit hook needs it to report
    /// session death without holding a reference to the link itself.
    tx: Mutex<Option<mpsc::Sender<NodeFrame>>>,
    hb_seq: AtomicU64,
    channel_token: AtomicU64,
}

impl NodeRuntime {
    /// `pty` is shared with the host process in `--role all` so the
    /// existing in-process terminal fast path keeps working: a session
    /// started through the cluster path lands in the very same manager
    /// the WebSocket handler already looks in.
    pub fn new(cfg: NodeConfig, pty: Arc<PtyManager>) -> Arc<Self> {
        let executor = executor::build(&cfg.executor);
        tracing::info!(
            node = %cfg.name,
            executor = executor.name(),
            isolation = executor.profile().as_str(),
            "node runtime ready"
        );
        let me = Arc::new(Self {
            cfg,
            pty,
            executor,
            mirror_locks: Mutex::new(HashMap::new()),
            handles: Mutex::new(HashMap::new()),
            paths: Mutex::new(HashMap::new()),
            channels: Mutex::new(HashMap::new()),
            tx: Mutex::new(None),
            hb_seq: AtomicU64::new(0),
            channel_token: AtomicU64::new(0),
        });
        me.install_exit_hook();
        me
    }

    /// Report a session's death upstream. Only window 0 ends a session;
    /// auxiliary windows just disappear.
    fn install_exit_hook(self: &Arc<Self>) {
        let weak: Weak<Self> = Arc::downgrade(self);
        self.pty.set_exit_hook(Arc::new(
            move |name: &str, window: u32, code: Option<i32>| {
                if window != 0 {
                    return;
                }
                let Some(me) = weak.upgrade() else { return };
                let name = name.to_string();
                tokio::spawn(async move {
                    me.release_session(&name).await;
                    me.send(NodeFrame::SessionEvent {
                        session: name,
                        event: SessionEventKind::Exited { code },
                    })
                    .await;
                });
            },
        ));
    }

    async fn send(&self, frame: NodeFrame) {
        let tx = self.tx.lock().clone();
        if let Some(tx) = tx {
            if tx.send(frame).await.is_err() {
                tracing::debug!("control link closed; dropping frame");
            }
        }
    }

    /// Drive one connection to the control plane until it drops.
    /// Returns `Ok(())` on a clean close so the caller can reconnect.
    pub async fn run(
        self: Arc<Self>,
        link: NodeLink,
        token: String,
        node_id: Option<String>,
    ) -> Result<()> {
        let result = self.clone().serve(link, token, node_id).await;
        // Drop the sender for the link that just died. It is what the
        // PTY exit hook writes to, and — more importantly — a live
        // sender keeps the transport's writer task parked in `recv()`,
        // which would stop the client from ever reconnecting.
        *self.tx.lock() = None;
        result
    }

    async fn serve(
        self: Arc<Self>,
        mut link: NodeLink,
        token: String,
        node_id: Option<String>,
    ) -> Result<()> {
        *self.tx.lock() = Some(link.tx.clone());

        let mut probe = MetricsProbe::new();
        let capacity = probe.capacity(self.cfg.max_sessions);
        let hello = Hello {
            name: self.cfg.name.clone(),
            token,
            node_id,
            version: env!("CARGO_PKG_VERSION").to_string(),
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            executors: vec![self.executor.profile()],
            capacity,
            labels: self.cfg.labels.clone(),
            running: self.running_sessions(),
        };
        link.tx
            .send(NodeFrame::Hello(Box::new(hello)))
            .await
            .map_err(|_| anyhow!("control link closed before registration"))?;

        let mut hb_secs = DEFAULT_HEARTBEAT_SECS;
        let mut hb = tokio::time::interval(std::time::Duration::from_secs(hb_secs));
        hb.tick().await; // the first tick fires immediately

        loop {
            tokio::select! {
                frame = link.rx.recv() => {
                    let Some(frame) = frame else {
                        tracing::info!("control link closed");
                        return Ok(());
                    };
                    if let ControlFrame::Welcome { heartbeat_secs, .. } = &frame {
                        if *heartbeat_secs > 0 && *heartbeat_secs != hb_secs {
                            hb_secs = *heartbeat_secs;
                            hb = tokio::time::interval(std::time::Duration::from_secs(hb_secs));
                            hb.tick().await;
                        }
                    }
                    if let ControlFrame::Rejected { reason } = &frame {
                        return Err(Rejected(reason.clone()).into());
                    }
                    self.clone().on_control(frame).await;
                }
                _ = hb.tick() => {
                    self.sweep_finished_paths();
                    let metrics = probe.sample();
                    // Scanning the cache directory is synchronous, and
                    // on a network filesystem it is not fast. Off the
                    // frame loop it goes, or control frames wait on it.
                    let repo_cache = tokio::task::spawn_blocking(cached_project_ids)
                        .await
                        .unwrap_or_default();
                    self.send(NodeFrame::Heartbeat {
                        seq: self.hb_seq.fetch_add(1, Ordering::Relaxed),
                        metrics,
                        running: self.running_sessions(),
                        repo_cache,
                    })
                    .await;
                }
            }
        }
    }

    /// Drop the paths of sessions that ended long enough ago that no
    /// finalize can still arrive. Without this a node that runs for
    /// weeks accumulates one entry per session it ever ran.
    fn sweep_finished_paths(&self) {
        self.paths.lock().retain(|_, p| match p.exited_at {
            Some(at) => at.elapsed() < PATHS_RETENTION,
            None => true,
        });
    }

    fn mirror_lock(&self, project_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.mirror_locks
            .lock()
            .entry(project_id.to_string())
            .or_default()
            .clone()
    }

    pub fn identity_path(&self) -> Option<PathBuf> {
        self.cfg.identity_path.clone()
    }

    fn running_sessions(&self) -> Vec<String> {
        self.handles.lock().keys().cloned().collect()
    }

    async fn on_control(self: Arc<Self>, frame: ControlFrame) {
        match frame {
            ControlFrame::Welcome { node_id, token, .. } => {
                tracing::info!(node_id = %node_id, "registered with control plane");
                if let Some(path) = &self.cfg.identity_path {
                    if let Err(e) = identity::save_identity(path, &Identity { node_id, token }) {
                        tracing::warn!(error = %e, "failed to persist node identity");
                    }
                }
            }
            ControlFrame::Rejected { .. } => {}
            ControlFrame::Assign { assign_id, spec } => {
                // Off the frame loop: a cold repository mirror can take
                // minutes and heartbeats must keep flowing meanwhile.
                tokio::spawn(async move {
                    let result = self.clone().start_session(&spec).await;
                    if let Err(e) = &result {
                        tracing::warn!(session = %spec.session, error = %e, "assign failed");
                    }
                    self.send(NodeFrame::Ack {
                        assign_id,
                        result: result.map_err(|e| e.to_string()),
                    })
                    .await;
                });
            }
            ControlFrame::Cancel {
                session,
                delete_worktree,
            } => {
                tokio::spawn(async move {
                    self.stop_session(&session, delete_worktree).await;
                });
            }
            ControlFrame::Finalize {
                request_id,
                session,
                spec,
            } => {
                // `git push` and `gh pr create` are network calls that
                // can take a while; keep the frame loop (and therefore
                // heartbeats) moving.
                tokio::spawn(async move {
                    let result = self.finalize_session(&session, &spec).await;
                    if let Err(e) = &result {
                        tracing::warn!(session = %session, error = %e, "finalize failed");
                    }
                    self.send(NodeFrame::FinalizeResult {
                        request_id,
                        result: result.map_err(|e| e.to_string()),
                    })
                    .await;
                });
            }
            ControlFrame::StreamOpen {
                ch,
                session,
                target,
            } => {
                self.open_stream(ch, &session, target).await;
            }
            ControlFrame::StreamData { ch, data } => {
                let session = self.channels.lock().get(&ch).map(|c| c.session.clone());
                if let Some(session) = session {
                    if let Err(e) = session.write(&data) {
                        tracing::debug!(ch, error = %e, "pty write failed");
                    }
                }
            }
            ControlFrame::StreamResize { ch, cols, rows } => {
                let session = self.channels.lock().get(&ch).map(|c| c.session.clone());
                if let Some(session) = session {
                    let _ = session.resize(cols, rows);
                }
            }
            ControlFrame::StreamClose { ch } => {
                self.close_channel(ch);
            }
            ControlFrame::Ping { seq } => {
                self.send(NodeFrame::Pong { seq }).await;
            }
        }
    }

    // ---- sessions ---------------------------------------------------

    async fn start_session(self: Arc<Self>, spec: &AssignSpec) -> Result<AssignOk> {
        let project = spec.project.clone();
        let session = spec.session.clone();
        // A project that is not already on this node can only be reached
        // through its mirror, and a bare mirror has no working tree — so
        // remote nodes always cut a worktree, whatever the request said.
        let local = is_local_project(&project);
        let create_worktree = spec.create_worktree || !local;

        // Serialize per project, not globally: a cold mirror of a large
        // repository takes minutes and must not stall sessions for
        // unrelated projects.
        let guard = self.mirror_lock(&project.id);
        let _held = guard.lock().await;
        let repo_root = tokio::task::spawn_blocking(move || resolve_source(&project))
            .await
            .map_err(|e| anyhow!("source resolution panicked: {e}"))??;
        drop(_held);

        let (cwd, worktree_path) = if create_worktree {
            let repo = repo_root.clone();
            let name = session.clone();
            let created =
                tokio::task::spawn_blocking(move || git_ops::create_worktree(&repo, &name))
                    .await
                    .map_err(|e| anyhow!("worktree creation panicked: {e}"))?
                    .map_err(|e| anyhow!("worktree creation failed: {e}"))?;
            (created.worktree_path.clone(), Some(created.worktree_path))
        } else {
            (repo_root.clone(), None)
        };

        if spec.mark_claude_trusted {
            let _ = workspace_manager::mark_claude_trusted(&cwd);
        }

        let mut env = workspace_manager::launch_env(&repo_root, &session, &cwd);
        env.extend(spec.env.iter().cloned());

        let exec_spec = executor::SessionSpec {
            session: session.clone(),
            cwd: cwd.clone(),
            shell: spec.shell.clone(),
            command: spec.command.clone(),
            env,
            requests: spec.requests,
        };

        let rollback = |worktree: &Option<PathBuf>, repo: &Path| {
            if let Some(wt) = worktree {
                let _ = git_ops::remove_worktree(wt, Some(repo), true);
            }
        };

        let handle = match self.executor.create(&exec_spec).await {
            Ok(h) => h,
            Err(e) => {
                rollback(&worktree_path, &repo_root);
                return Err(anyhow!("{e}"));
            }
        };
        let plan = self.executor.launch_plan(&handle, &exec_spec);

        // Register before spawning. The PTY exit hook fires as soon as
        // the child dies — which can be immediately, for a command that
        // fails to start — and it looks the sandbox up here to tear it
        // down. Inserting afterwards leaks the sandbox in that race.
        self.handles.lock().insert(session.clone(), handle.clone());
        self.paths.lock().insert(
            session.clone(),
            SessionPaths {
                cwd: cwd.clone(),
                repo_root: repo_root.clone(),
                worktree: worktree_path.clone(),
                exited_at: None,
            },
        );

        let pty = match self.pty.spawn(PtySpawnSpec {
            name: session.clone(),
            window: 0,
            cwd: plan.cwd,
            shell: plan.shell,
            command: plan.command,
            env: plan.env,
            cols: 80,
            rows: 24,
        }) {
            Ok(p) => p,
            Err(e) => {
                self.handles.lock().remove(&session);
                self.paths.lock().remove(&session);
                let _ = self.executor.destroy(&handle).await;
                rollback(&worktree_path, &repo_root);
                return Err(anyhow!("{e}"));
            }
        };

        Ok(AssignOk {
            session,
            cwd: cwd.to_string_lossy().into_owned(),
            worktree_path: worktree_path
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
            // `orig_path` is only meaningful when a worktree was cut —
            // it is what `git worktree remove` has to be run against.
            orig_path: if worktree_path.is_some() {
                repo_root.to_string_lossy().into_owned()
            } else {
                String::new()
            },
            pid: pty.pid(),
        })
    }

    /// Forget a session's sandbox without touching its worktree. Called
    /// from the PTY exit hook, where the user has not asked for cleanup.
    async fn release_session(&self, session: &str) {
        // The paths stay: a `Finalize` for this session may still be on
        // its way. Stamping the exit is what lets the sweep below drop
        // them once that can no longer be true.
        if let Some(paths) = self.paths.lock().get_mut(session) {
            paths.exited_at.get_or_insert_with(Instant::now);
        }
        let handle = self.handles.lock().remove(session);
        if let Some(handle) = handle {
            if let Err(e) = self.executor.destroy(&handle).await {
                tracing::warn!(session = %session, error = %e, "executor destroy failed");
            }
        }
    }

    /// Commit / push / open a PR from a finished session's worktree.
    ///
    /// The paths come from the record made at assign time rather than
    /// from the session name: a session started without `createWorktree`
    /// runs directly in the project, and finalizing the wrong directory
    /// would commit to the user's checkout.
    async fn finalize_session(&self, session: &str, spec: &FinalizeSpec) -> Result<FinalizeOk> {
        let paths = self.paths.lock().get(session).cloned();
        let paths = paths.ok_or_else(|| {
            anyhow!("session `{session}` is not known to this node; nothing to finalize")
        })?;
        // The worktree can be removed out from under us (an impatient
        // delete, a crashed node reusing the directory). Say so rather
        // than letting git report a confusing error about a missing repo.
        if !paths.cwd.is_dir() {
            return Err(anyhow!(
                "the session's working directory is gone: {}",
                paths.cwd.display()
            ));
        }

        let req = git_ops::FinalizeRequest {
            commit_message: spec.commit_message.clone(),
            push: spec.push,
            open_pr: spec.open_pr,
            pr_title: spec.pr_title.clone(),
            pr_body: spec.pr_body.clone(),
            draft: spec.draft,
            base_branch: spec.base_branch.clone(),
        };
        let cwd = paths.cwd.clone();
        let report = tokio::task::spawn_blocking(move || git_ops::finalize(&cwd, &req))
            .await
            .map_err(|e| anyhow!("finalize panicked: {e}"))?
            .map_err(|e| anyhow!("{e}"))?;

        Ok(FinalizeOk {
            committed: report.committed,
            sha: report.sha,
            branch: report.branch,
            pushed: report.pushed,
            pr_url: report.pr_url,
            notes: report.notes,
        })
    }

    async fn stop_session(&self, session: &str, delete_worktree: bool) {
        // Capture the worktree before the PTYs go: once the session is
        // gone we have no record of where it lived.
        let paths = self.paths.lock().remove(session);
        let windows = self.pty.remove_session(session);
        for w in &windows {
            w.kill();
        }
        self.release_session(session).await;

        if delete_worktree {
            // Prefer the path recorded at assign time: a session that
            // ran without a worktree has nothing to remove, and deleting
            // the conventional path would target somebody else's files.
            if let Some(paths) = paths {
                let Some(wt) = paths.worktree else { return };
                let repo = paths.repo_root;
                let _ = tokio::task::spawn_blocking(move || {
                    git_ops::remove_worktree(&wt, Some(&repo), true)
                })
                .await;
                return;
            }
            let wt = git_ops::worktree_path_for(session);
            if wt.is_dir() {
                let path = wt.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    git_ops::remove_worktree(&path, None, true)
                })
                .await;
            }
        }
    }

    // ---- relayed streams --------------------------------------------

    async fn open_stream(self: &Arc<Self>, ch: u32, session: &str, target: StreamTarget) {
        let StreamTarget::Pty { window } = target;
        let Some(pty) = self.pty.get(session, window) else {
            self.send(NodeFrame::StreamClose {
                ch,
                reason: "session not found".into(),
            })
            .await;
            return;
        };

        let (snapshot, mut rx) = pty.subscribe();
        let tx = self.tx.lock().clone();
        let Some(tx) = tx else { return };

        let token = self.channel_token.fetch_add(1, Ordering::Relaxed);
        let weak: Weak<Self> = Arc::downgrade(self);
        let pump = tokio::spawn(async move {
            // Whatever ends this pump — the PTY closing, the consumer
            // going away — the channel entry has to go with it, or a
            // long-lived node accumulates dead channels holding PTY
            // handles alive.
            let _guard = ChannelGuard { weak, ch, token };
            let mut seq = 0u64;
            // Replay the scrollback first so a reattaching browser sees
            // the same screen it would on a local session.
            if !snapshot.is_empty()
                && tx
                    .send(NodeFrame::Stream {
                        ch,
                        seq,
                        data: snapshot,
                    })
                    .await
                    .is_err()
            {
                return;
            }
            seq += 1;
            loop {
                match rx.recv().await {
                    Ok(chunk) => {
                        // `send` is bounded, so a slow consumer throttles
                        // the pump rather than growing the queue.
                        if tx
                            .send(NodeFrame::Stream {
                                ch,
                                seq,
                                data: chunk,
                            })
                            .await
                            .is_err()
                        {
                            return;
                        }
                        seq += 1;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::debug!(ch, dropped = n, "stream consumer lagged");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        let _ = tx
                            .send(NodeFrame::StreamClose {
                                ch,
                                reason: "pty closed".into(),
                            })
                            .await;
                        return;
                    }
                }
            }
        });

        if let Some(old) = self.channels.lock().insert(
            ch,
            Channel {
                session: pty,
                pump,
                token,
            },
        ) {
            old.pump.abort();
        }
    }

    fn close_channel(&self, ch: u32) {
        if let Some(c) = self.channels.lock().remove(&ch) {
            c.pump.abort();
        }
    }
}

/// Removes a relayed channel when its pump task ends, however it ends.
struct ChannelGuard {
    weak: Weak<NodeRuntime>,
    ch: u32,
    token: u64,
}

impl Drop for ChannelGuard {
    fn drop(&mut self) {
        let Some(rt) = self.weak.upgrade() else {
            return;
        };
        let mut channels = rt.channels.lock();
        // Only if this entry is still ours: `ch` may already have been
        // handed to a newer stream.
        if channels.get(&self.ch).map(|c| c.token) == Some(self.token) {
            channels.remove(&self.ch);
        }
    }
}

/// Root of this node's mirror cache for `project_id`.
pub fn repo_cache_dir(project_id: &str) -> PathBuf {
    config_loader::agent_start_home()
        .join("cache")
        .join(project_id)
        .join(".repo")
}

fn cache_root() -> PathBuf {
    config_loader::agent_start_home().join("cache")
}

/// Project ids this node already holds a mirror for. Reported on every
/// heartbeat and turned into the scheduler's cache-affinity score.
fn cached_project_ids() -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(cache_root()) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|e| e.path().join(".repo").join("HEAD").exists())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect()
}

fn is_local_project(project: &ProjectRef) -> bool {
    !project.local_path.is_empty() && Path::new(&project.local_path).is_dir()
}

/// Find the repository this session should branch from: the path the
/// control plane named if this node happens to have it, otherwise the
/// node-local mirror (cloned on first use).
fn resolve_source(project: &ProjectRef) -> Result<PathBuf> {
    if is_local_project(project) {
        return Ok(PathBuf::from(&project.local_path));
    }
    let url = project.clone_url.as_deref().ok_or_else(|| {
        anyhow!(
            "project `{}` is not present on this node and has no origin remote to clone from",
            project.name
        )
    })?;
    let dest = repo_cache_dir(&project.id);
    git_ops::ensure_mirror(url, &dest).map_err(|e| anyhow!("mirror clone failed: {e}"))?;
    Ok(dest)
}

/// Isolation profiles a backend name provides, without building it.
/// Used by the CLI to report capabilities in `--help`-style output.
pub fn profile_for(executor_name: &str) -> IsolationProfile {
    executor::build(executor_name).profile()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(local: &str, url: Option<&str>) -> ProjectRef {
        ProjectRef {
            id: "demo-0000000000000000".into(),
            name: "demo".into(),
            local_path: local.into(),
            clone_url: url.map(str::to_string),
        }
    }

    #[test]
    fn a_project_present_on_this_node_is_used_as_is() {
        let dir = tempfile::tempdir().unwrap();
        let p = project(&dir.path().to_string_lossy(), None);
        assert!(is_local_project(&p));
        assert_eq!(resolve_source(&p).unwrap(), dir.path());
    }

    #[test]
    fn a_missing_project_without_an_origin_is_a_clear_error() {
        let p = project("/nonexistent/agent-start/demo", None);
        assert!(!is_local_project(&p));
        let err = resolve_source(&p).unwrap_err().to_string();
        assert!(err.contains("no origin remote"), "unhelpful error: {err}");
    }

    #[test]
    fn hostname_is_never_empty() {
        assert!(!hostname().is_empty());
    }
}
