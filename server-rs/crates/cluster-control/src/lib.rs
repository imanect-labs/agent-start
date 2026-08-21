//! The control plane: node registry, scheduler, and stream relay.
//!
//! It owns no PTYs and clones no repositories. Its job is to know which
//! nodes exist, decide where a session should run, and carry bytes
//! between a browser and whichever machine ended up running the agent.
//!
//! The in-process node of `--role all` registers through exactly the
//! same path as a remote one, so a single-host install exercises the
//! scheduler, the registry and the relay on every launch rather than
//! leaving them to rot behind a feature flag.

use chrono::Utc;
use cluster_proto::{
    AssignOk, AssignSpec, ControlFrame, ControlLink, Hello, IsolationProfile, NodeCapacity,
    NodeFrame, NodeMetrics, Resources, SessionEventKind, StreamTarget, LINK_CHANNEL_CAP,
};
use parking_lot::{Mutex, RwLock};
use sha2::{Digest, Sha256};
use state::Db;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};

mod scheduler;
pub use scheduler::{Candidate, Demand, NoFit};

/// Callback the host installs to clean up after a session that ended on
/// its own. Keeps the control plane ignorant of noVNC, code-server and
/// the rest of the host's per-session machinery.
pub type SessionExitHook = Arc<dyn Fn(String) + Send + Sync>;

#[derive(Debug, Clone)]
pub struct ControlOptions {
    /// How long a node has to bring a session up before the placement is
    /// abandoned. Generous: a cold mirror clone of a large repository is
    /// minutes of honest work, not a hang.
    pub assign_timeout: Duration,
    pub heartbeat_secs: u64,
    /// Missed-heartbeat threshold before a node stops receiving work.
    pub notready_after: Duration,
    /// …and before its sessions are declared lost.
    pub lost_after: Duration,
    /// Token accepted from any node, for operators who would rather
    /// manage one shared secret than mint per-node join tokens.
    pub static_token: Option<String>,
}

impl Default for ControlOptions {
    fn default() -> Self {
        Self {
            assign_timeout: Duration::from_secs(300),
            heartbeat_secs: cluster_node_heartbeat_default(),
            notready_after: Duration::from_secs(35),
            lost_after: Duration::from_secs(120),
            static_token: None,
        }
    }
}

const fn cluster_node_heartbeat_default() -> u64 {
    10
}

#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error("{0}")]
    NoFit(NoFit),
    #[error("node {node} did not accept the session within {secs}s")]
    Timeout { node: String, secs: u64 },
    #[error("node {node} disconnected while starting the session")]
    Disconnected { node: String },
    #[error("{0}")]
    Node(String),
}

/// Static facts about a node, refreshed at registration.
#[derive(Debug, Clone)]
pub struct NodeInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub os: String,
    pub arch: String,
    pub executors: Vec<IsolationProfile>,
    pub capacity: NodeCapacity,
    pub labels: Vec<(String, String)>,
    pub is_local: bool,
    pub created_at_ms: i64,
}

/// Everything about a node that changes while it is connected.
#[derive(Debug, Default)]
struct NodeDynamic {
    connected: bool,
    cordoned: bool,
    max_sessions: u32,
    metrics: NodeMetrics,
    /// Sessions the control plane believes are running here.
    running: HashSet<String>,
    reserved: Resources,
    repo_cache: HashSet<String>,
    last_heartbeat: Option<Instant>,
    last_assign: Option<Instant>,
    disconnected_at: Option<Instant>,
}

pub struct NodeHandle {
    pub id: String,
    pub name: String,
    tx: mpsc::Sender<ControlFrame>,
    info: RwLock<NodeInfo>,
    dynamic: RwLock<NodeDynamic>,
    pending: Mutex<HashMap<String, oneshot::Sender<Result<AssignOk, String>>>>,
    channels: Mutex<HashMap<u32, mpsc::Sender<StreamMsg>>>,
    next_ch: AtomicU32,
}

impl NodeHandle {
    pub fn info(&self) -> NodeInfo {
        self.info.read().clone()
    }

    fn close_channel(&self, ch: u32) {
        self.channels.lock().remove(&ch);
        // Best-effort: if the queue is full the node will notice the
        // stream is dead when its next write fails.
        let _ = self.tx.try_send(ControlFrame::StreamClose { ch });
    }
}

/// A frame relayed from a node's PTY toward a browser.
#[derive(Debug)]
pub enum StreamMsg {
    Data(Vec<u8>),
    Closed(String),
}

/// A live relay to one PTY window on one node.
///
/// The receiving half comes out via `take_rx` so a caller can pump
/// output and accept input at the same time while the stream itself
/// stays owned — dropping it is what closes the channel on the node.
pub struct PtyStream {
    node: Arc<NodeHandle>,
    ch: u32,
    rx: Option<mpsc::Receiver<StreamMsg>>,
}

impl PtyStream {
    /// Take the output half. Returns `None` on a second call.
    pub fn take_rx(&mut self) -> Option<mpsc::Receiver<StreamMsg>> {
        self.rx.take()
    }

    pub async fn write(&self, data: Vec<u8>) {
        let _ = self
            .node
            .tx
            .send(ControlFrame::StreamData { ch: self.ch, data })
            .await;
    }

    pub async fn resize(&self, cols: u16, rows: u16) {
        let _ = self
            .node
            .tx
            .send(ControlFrame::StreamResize {
                ch: self.ch,
                cols,
                rows,
            })
            .await;
    }

    pub fn node_name(&self) -> &str {
        &self.node.name
    }
}

impl Drop for PtyStream {
    fn drop(&mut self) {
        self.node.close_channel(self.ch);
    }
}

#[derive(Debug, Clone)]
struct Placement {
    node_id: String,
    requests: Resources,
    /// When the placement was made. Heartbeat reconciliation needs it:
    /// a session assigned moments ago may legitimately not appear in a
    /// report that was already in flight.
    assigned_at: Instant,
}

/// One node as the API surfaces it.
#[derive(Debug, Clone)]
pub struct NodeView {
    pub info: NodeInfo,
    pub connected: bool,
    pub cordoned: bool,
    pub status: NodeState,
    pub max_sessions: u32,
    pub running: Vec<String>,
    pub reserved: Resources,
    pub metrics: NodeMetrics,
    pub repo_cache: Vec<String>,
    pub last_heartbeat_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeState {
    Ready,
    NotReady,
    Cordoned,
    Lost,
}

impl NodeState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::NotReady => "notready",
            Self::Cordoned => "cordoned",
            Self::Lost => "lost",
        }
    }
}

/// A scheduled session, as reported back to the caller.
#[derive(Debug, Clone)]
pub struct StartOutcome {
    pub node_id: String,
    pub node_name: String,
    pub is_local: bool,
    pub assigned: AssignOk,
}

pub struct ControlPlane {
    db: Db,
    opts: ControlOptions,
    nodes: RwLock<HashMap<String, Arc<NodeHandle>>>,
    placements: RwLock<HashMap<String, Placement>>,
    local_node_id: RwLock<Option<String>>,
    on_session_exit: RwLock<Option<SessionExitHook>>,
}

impl ControlPlane {
    pub fn new(db: Db, opts: ControlOptions) -> Arc<Self> {
        Arc::new(Self {
            db,
            opts,
            nodes: RwLock::new(HashMap::new()),
            placements: RwLock::new(HashMap::new()),
            local_node_id: RwLock::new(None),
            on_session_exit: RwLock::new(None),
        })
    }

    pub fn set_session_exit_hook(&self, hook: SessionExitHook) {
        *self.on_session_exit.write() = Some(hook);
    }

    pub fn heartbeat_secs(&self) -> u64 {
        self.opts.heartbeat_secs
    }

    pub fn local_node_id(&self) -> Option<String> {
        self.local_node_id.read().clone()
    }

    /// True when the session runs on the in-process node — the case
    /// where the host can still touch its files and PTYs directly.
    pub fn is_local_session(&self, session: &str) -> bool {
        match self.placements.read().get(session) {
            Some(p) => Some(&p.node_id) == self.local_node_id.read().as_ref(),
            // Unknown sessions predate the cluster layer: local.
            None => true,
        }
    }

    pub fn node_for_session(&self, session: &str) -> Option<Arc<NodeHandle>> {
        let node_id = self.placements.read().get(session)?.node_id.clone();
        self.nodes.read().get(&node_id).cloned()
    }

    /// Re-attach a session the control plane learned about from the
    /// database rather than from a placement (a restart, or a row
    /// written before this node had an id).
    pub fn adopt_session(&self, session: &str, node_id: &str, requests: Resources) {
        self.placements.write().insert(
            session.to_string(),
            Placement {
                node_id: node_id.to_string(),
                requests,
                assigned_at: Instant::now(),
            },
        );
        if let Some(node) = self.nodes.read().get(node_id) {
            node.dynamic.write().running.insert(session.to_string());
        }
    }

    // ---- registration ------------------------------------------------

    /// Serve one node connection until it drops. Both transports funnel
    /// here, so authentication, registration and teardown exist once.
    pub async fn accept(self: Arc<Self>, link: ControlLink) {
        let ControlLink {
            tx,
            mut rx,
            trusted,
            local,
        } = link;

        // A peer that connects and says nothing must not hold a slot.
        let hello = match tokio::time::timeout(Duration::from_secs(15), rx.recv()).await {
            Ok(Some(NodeFrame::Hello(h))) => *h,
            Ok(Some(other)) => {
                tracing::warn!(frame = ?std::mem::discriminant(&other), "expected hello");
                return;
            }
            _ => return,
        };

        let (node_id, token) = match self.authenticate(&hello, trusted).await {
            Ok(v) => v,
            Err(reason) => {
                tracing::warn!(node = %hello.name, %reason, "rejecting node registration");
                let _ = tx.send(ControlFrame::Rejected { reason }).await;
                return;
            }
        };

        let node = self
            .register(&node_id, &token, &hello, local, tx.clone())
            .await;

        if tx
            .send(ControlFrame::Welcome {
                node_id: node_id.clone(),
                token,
                heartbeat_secs: self.opts.heartbeat_secs,
            })
            .await
            .is_err()
        {
            return;
        }

        tracing::info!(
            node = %node.name,
            id = %node_id,
            local,
            cpu_millis = hello.capacity.cpu_millis,
            mem_mb = hello.capacity.mem_mb,
            "node joined"
        );

        // Sessions the node was already running (it outlived us). Their
        // original requests died with the previous control plane, so
        // charge them the default rather than nothing — under-counting
        // would let the node be oversubscribed.
        for session in &hello.running {
            self.adopt_session(session, &node_id, Resources::default_request());
        }

        while let Some(frame) = rx.recv().await {
            self.on_node_frame(&node, frame).await;
        }

        self.on_disconnect(&node).await;
    }

    /// Decide whether a node may join, and under which identity.
    /// Returns `(node_id, fresh_long_lived_token)`.
    async fn authenticate(&self, hello: &Hello, trusted: bool) -> Result<(String, String), String> {
        // The in-process node shares our address space; there is no
        // channel for a third party to impersonate it on.
        if trusted {
            let id = hello
                .node_id
                .clone()
                .unwrap_or_else(|| format!("local-{}", uuid::Uuid::new_v4()));
            return Ok((id, String::new()));
        }
        if hello.token.trim().is_empty() {
            return Err("a join token is required".into());
        }
        let presented = hash_token(&hello.token);

        // Returning node: its stored token is the credential.
        if let Some(id) = hello.node_id.as_deref() {
            match state::get_node(&self.db, id).await {
                Ok(Some(row)) if !row.token_hash.is_empty() && row.token_hash == presented => {
                    // Rotate on every reconnect: a token that leaks is
                    // only good until the node next comes back.
                    return Ok((row.id, new_token()));
                }
                Ok(_) => {}
                Err(e) => return Err(format!("registry unavailable: {e}")),
            }
        }

        let accepted = match &self.opts.static_token {
            Some(t) if constant_time_eq(t, &hello.token) => true,
            _ => state::consume_join_token(&self.db, &presented)
                .await
                .unwrap_or(false),
        };
        if !accepted {
            return Err("join token is invalid, expired, or already used".into());
        }

        // A re-provisioned machine reclaims its own name rather than
        // leaving a ghost row behind.
        let id = match state::get_node_by_name(&self.db, &hello.name).await {
            Ok(Some(row)) => row.id,
            _ => uuid::Uuid::new_v4().to_string(),
        };
        Ok((id, new_token()))
    }

    async fn register(
        self: &Arc<Self>,
        node_id: &str,
        token: &str,
        hello: &Hello,
        is_local: bool,
        tx: mpsc::Sender<ControlFrame>,
    ) -> Arc<NodeHandle> {
        let now = Utc::now().timestamp_millis();
        // Operator settings live in the database, not in the node's
        // self-report: a cordon must survive the node reconnecting.
        let stored = state::get_node(&self.db, node_id).await.ok().flatten();
        let max_sessions = stored
            .as_ref()
            .map(|r| r.max_sessions as u32)
            .filter(|v| *v > 0)
            .unwrap_or(hello.capacity.max_sessions);
        let cordoned = stored.as_ref().map(|r| r.cordoned).unwrap_or(false);
        let stored_labels = stored
            .as_ref()
            .and_then(|r| parse_labels(&r.labels))
            .unwrap_or_default();
        let labels = if stored_labels.is_empty() {
            hello.labels.clone()
        } else {
            stored_labels
        };
        let created_at_ms = stored.as_ref().map(|r| r.created_at_ms).unwrap_or(now);

        let info = NodeInfo {
            id: node_id.to_string(),
            name: hello.name.clone(),
            version: hello.version.clone(),
            os: hello.os.clone(),
            arch: hello.arch.clone(),
            executors: hello.executors.clone(),
            capacity: hello.capacity,
            labels: labels.clone(),
            is_local,
            created_at_ms,
        };

        let row = state::NodeRow {
            id: node_id.to_string(),
            name: hello.name.clone(),
            token_hash: if token.is_empty() {
                String::new()
            } else {
                hash_token(token)
            },
            status: NodeState::Ready.as_str().to_string(),
            version: hello.version.clone(),
            os: hello.os.clone(),
            arch: hello.arch.clone(),
            executors: hello
                .executors
                .iter()
                .map(|p| p.as_str())
                .collect::<Vec<_>>()
                .join(","),
            capacity_cpu_millis: hello.capacity.cpu_millis as i64,
            capacity_mem_mb: hello.capacity.mem_mb as i64,
            max_sessions: max_sessions as i64,
            labels: serialize_labels(&labels),
            cordoned,
            is_local,
            last_heartbeat_ms: now,
            created_at_ms,
        };
        if let Err(e) = state::upsert_node(&self.db, &row).await {
            tracing::warn!(error = %e, "failed to persist node registration");
        }

        let handle = Arc::new(NodeHandle {
            id: node_id.to_string(),
            name: hello.name.clone(),
            tx,
            info: RwLock::new(info),
            dynamic: RwLock::new(NodeDynamic {
                connected: true,
                cordoned,
                max_sessions,
                reserved: Resources::ZERO,
                last_heartbeat: Some(Instant::now()),
                ..Default::default()
            }),
            pending: Mutex::new(HashMap::new()),
            channels: Mutex::new(HashMap::new()),
            next_ch: AtomicU32::new(1),
        });

        // Replacing a stale handle for the same id (a node that
        // reconnected before we noticed the old link had died).
        let previous = self
            .nodes
            .write()
            .insert(node_id.to_string(), handle.clone());
        if let Some(prev) = previous {
            prev.dynamic.write().connected = false;
            fail_pending(&prev, "node reconnected");
        }
        if is_local {
            *self.local_node_id.write() = Some(node_id.to_string());
        }
        handle
    }

    async fn on_disconnect(&self, node: &Arc<NodeHandle>) {
        {
            let mut d = node.dynamic.write();
            d.connected = false;
            d.disconnected_at = Some(Instant::now());
        }
        fail_pending(node, "node disconnected");
        node.channels.lock().clear();
        let _ = state::set_node_status(
            &self.db,
            &node.id,
            NodeState::NotReady.as_str(),
            Utc::now().timestamp_millis(),
        )
        .await;
        tracing::warn!(node = %node.name, "node disconnected");
    }

    async fn on_node_frame(&self, node: &Arc<NodeHandle>, frame: NodeFrame) {
        match frame {
            NodeFrame::Hello(_) => {
                tracing::warn!(node = %node.name, "duplicate hello ignored");
            }
            NodeFrame::Heartbeat {
                metrics,
                running,
                repo_cache,
                ..
            } => {
                let vanished = {
                    let mut d = node.dynamic.write();
                    d.metrics = metrics;
                    d.last_heartbeat = Some(Instant::now());
                    d.connected = true;
                    d.repo_cache = repo_cache.iter().cloned().collect();
                    d.running
                        .iter()
                        .filter(|s| !running.contains(*s))
                        .cloned()
                        .collect::<Vec<_>>()
                };
                // The node is the authority on what is actually running.
                // Anything we still believe in that it does not report
                // has died without an exit event reaching us — release
                // it, or its reservation pins capacity forever. The
                // grace period covers reports that were already in
                // flight when the session started.
                let grace = Duration::from_secs(self.opts.heartbeat_secs * 2);
                for session in vanished {
                    let fresh = self
                        .placements
                        .read()
                        .get(&session)
                        .map(|p| p.assigned_at.elapsed() < grace)
                        .unwrap_or(false);
                    if fresh {
                        continue;
                    }
                    tracing::warn!(
                        session = %session,
                        node = %node.name,
                        "session vanished without an exit event; releasing"
                    );
                    self.expire_session(&session);
                }
                let _ = state::set_node_status(
                    &self.db,
                    &node.id,
                    NodeState::Ready.as_str(),
                    Utc::now().timestamp_millis(),
                )
                .await;
                let _ = state::record_node_metrics(
                    &self.db,
                    &node.id,
                    metrics.cpu_util as f64,
                    metrics.mem_util as f64,
                    metrics.load1 as f64,
                    running.len() as i64,
                )
                .await;
                let _ = state::replace_repo_cache(&self.db, &node.id, &repo_cache).await;
            }
            NodeFrame::Ack { assign_id, result } => {
                let waiter = node.pending.lock().remove(&assign_id);
                match waiter {
                    Some(tx) => {
                        let _ = tx.send(result);
                    }
                    // The requester gave up; undo the side effect so the
                    // node does not hold an orphaned session forever.
                    None => {
                        if let Ok(ok) = result {
                            tracing::warn!(session = %ok.session, "late assign ack; cancelling");
                            let _ = node
                                .tx
                                .send(ControlFrame::Cancel {
                                    session: ok.session,
                                    delete_worktree: true,
                                })
                                .await;
                        }
                    }
                }
            }
            NodeFrame::SessionEvent { session, event } => {
                if let SessionEventKind::Failed { error } = &event {
                    tracing::warn!(session = %session, error = %error, "session failed on node");
                }
                self.release(&session);
                let hook = self.on_session_exit.read().clone();
                if let Some(hook) = hook {
                    hook(session);
                }
            }
            NodeFrame::Stream { ch, data, .. } => {
                let sink = node.channels.lock().get(&ch).cloned();
                if let Some(sink) = sink {
                    // Bounded: a browser that stops reading throttles
                    // the relay rather than growing it without bound.
                    if sink.send(StreamMsg::Data(data)).await.is_err() {
                        node.close_channel(ch);
                    }
                }
            }
            NodeFrame::StreamClose { ch, reason } => {
                let sink = node.channels.lock().remove(&ch);
                if let Some(sink) = sink {
                    let _ = sink.send(StreamMsg::Closed(reason)).await;
                }
            }
            NodeFrame::Pong { .. } => {}
        }
    }

    /// Drop a session's reservation. Idempotent.
    fn release(&self, session: &str) {
        let placement = self.placements.write().remove(session);
        let Some(placement) = placement else { return };
        let nodes = self.nodes.read();
        let Some(node) = nodes.get(&placement.node_id) else {
            return;
        };
        let mut d = node.dynamic.write();
        d.running.remove(session);
        d.reserved.cpu_millis = d
            .reserved
            .cpu_millis
            .saturating_sub(placement.requests.cpu_millis);
        d.reserved.mem_mb = d.reserved.mem_mb.saturating_sub(placement.requests.mem_mb);
    }

    // ---- scheduling ---------------------------------------------------

    /// Place and start one session. On any failure the reservation is
    /// rolled back, so a rejected request costs the cluster nothing.
    pub async fn start_session(
        &self,
        mut spec: AssignSpec,
        demand: Demand,
    ) -> Result<StartOutcome, StartError> {
        let node = self.pick(&demand).map_err(StartError::NoFit)?;

        // `local_path` is a path on *this* machine. A remote node must
        // never treat a path that merely happens to exist there as the
        // same project — two machines can both have
        // `~/projects/api` holding entirely different code. Blank it out
        // so anything off-host is forced through the mirror, which is
        // identified by URL and therefore cannot be confused.
        if !node.info().is_local {
            spec.project.local_path.clear();
        }

        // Reserve before asking: two concurrent requests must not both
        // see the same free capacity.
        {
            let mut d = node.dynamic.write();
            d.reserved.cpu_millis += demand.requests.cpu_millis;
            d.reserved.mem_mb += demand.requests.mem_mb;
            d.running.insert(spec.session.clone());
            d.last_assign = Some(Instant::now());
        }
        self.placements.write().insert(
            spec.session.clone(),
            Placement {
                node_id: node.id.clone(),
                requests: demand.requests,
                assigned_at: Instant::now(),
            },
        );

        let assign_id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        node.pending.lock().insert(assign_id.clone(), tx);

        let session = spec.session.clone();
        let send = node
            .tx
            .send(ControlFrame::Assign {
                assign_id: assign_id.clone(),
                spec: Box::new(spec),
            })
            .await;
        if send.is_err() {
            node.pending.lock().remove(&assign_id);
            self.release(&session);
            return Err(StartError::Disconnected {
                node: node.name.clone(),
            });
        }

        let outcome = match tokio::time::timeout(self.opts.assign_timeout, rx).await {
            Ok(Ok(Ok(ok))) => ok,
            Ok(Ok(Err(msg))) => {
                self.release(&session);
                return Err(StartError::Node(msg));
            }
            Ok(Err(_)) => {
                self.release(&session);
                return Err(StartError::Disconnected {
                    node: node.name.clone(),
                });
            }
            Err(_) => {
                node.pending.lock().remove(&assign_id);
                self.release(&session);
                // The node may still be working; tell it to stop.
                let _ = node
                    .tx
                    .send(ControlFrame::Cancel {
                        session: session.clone(),
                        delete_worktree: true,
                    })
                    .await;
                return Err(StartError::Timeout {
                    node: node.name.clone(),
                    secs: self.opts.assign_timeout.as_secs(),
                });
            }
        };

        let info = node.info();
        Ok(StartOutcome {
            node_id: node.id.clone(),
            node_name: node.name.clone(),
            is_local: info.is_local,
            assigned: outcome,
        })
    }

    fn pick(&self, demand: &Demand) -> Result<Arc<NodeHandle>, NoFit> {
        let nodes: Vec<Arc<NodeHandle>> = self.nodes.read().values().cloned().collect();
        let candidates: Vec<Candidate> = nodes
            .iter()
            .map(|n| self.candidate(n, &demand.project_id))
            .collect();
        let chosen = scheduler::select(&candidates, demand)?;
        nodes
            .into_iter()
            .find(|n| n.id == chosen.id)
            .ok_or(NoFit::NoNodes)
    }

    fn candidate(&self, node: &Arc<NodeHandle>, project_id: &str) -> Candidate {
        let info = node.info.read();
        let d = node.dynamic.read();
        Candidate {
            id: node.id.clone(),
            name: node.name.clone(),
            is_local: info.is_local,
            ready: d.connected && !self.stale(&d),
            cordoned: d.cordoned,
            max_sessions: d.max_sessions,
            running: d.running.len() as u32,
            capacity: Resources {
                cpu_millis: info.capacity.cpu_millis,
                mem_mb: info.capacity.mem_mb,
            },
            reserved: d.reserved,
            cpu_util: d.metrics.cpu_util,
            mem_util: d.metrics.mem_util,
            profiles: info.executors.clone(),
            labels: info.labels.clone(),
            has_repo_cache: d.repo_cache.iter().any(|p| p == project_id),
            secs_since_assign: d.last_assign.map(|t| t.elapsed().as_secs()),
        }
    }

    fn stale(&self, d: &NodeDynamic) -> bool {
        match d.last_heartbeat {
            Some(t) => t.elapsed() > self.opts.notready_after,
            None => false,
        }
    }

    // ---- session control ----------------------------------------------

    pub async fn cancel_session(&self, session: &str, delete_worktree: bool) {
        let node = self.node_for_session(session);
        self.release(session);
        if let Some(node) = node {
            let _ = node
                .tx
                .send(ControlFrame::Cancel {
                    session: session.to_string(),
                    delete_worktree,
                })
                .await;
        }
    }

    /// Open a relayed terminal onto a session running on another node.
    ///
    /// Returns `None` when the node is not currently connected. The
    /// frame channel outlives the socket — the registry holds a sender
    /// until the node is evicted — so sending would quietly succeed and
    /// leave the caller waiting forever for output that cannot come.
    pub async fn open_pty_stream(&self, session: &str, window: u32) -> Option<PtyStream> {
        let node = self.node_for_session(session)?;
        if !node.dynamic.read().connected {
            return None;
        }
        let ch = node.next_ch.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel(LINK_CHANNEL_CAP);
        node.channels.lock().insert(ch, tx);
        if node
            .tx
            .send(ControlFrame::StreamOpen {
                ch,
                session: session.to_string(),
                target: StreamTarget::Pty { window },
            })
            .await
            .is_err()
        {
            node.channels.lock().remove(&ch);
            return None;
        }
        Some(PtyStream {
            node,
            ch,
            rx: Some(rx),
        })
    }

    // ---- registry API --------------------------------------------------

    pub fn nodes(&self) -> Vec<NodeView> {
        let nodes: Vec<Arc<NodeHandle>> = self.nodes.read().values().cloned().collect();
        let mut out: Vec<NodeView> = nodes.iter().map(|n| self.view(n)).collect();
        out.sort_by(|a, b| a.info.name.cmp(&b.info.name));
        out
    }

    pub fn node(&self, id: &str) -> Option<NodeView> {
        let node = self.nodes.read().get(id).cloned()?;
        Some(self.view(&node))
    }

    fn view(&self, node: &Arc<NodeHandle>) -> NodeView {
        let info = node.info();
        let d = node.dynamic.read();
        let status = if !d.connected {
            match d.disconnected_at {
                Some(t) if t.elapsed() > self.opts.lost_after => NodeState::Lost,
                _ => NodeState::NotReady,
            }
        } else if d.cordoned {
            NodeState::Cordoned
        } else if self.stale(&d) {
            NodeState::NotReady
        } else {
            NodeState::Ready
        };
        let mut running: Vec<String> = d.running.iter().cloned().collect();
        running.sort();
        let mut repo_cache: Vec<String> = d.repo_cache.iter().cloned().collect();
        repo_cache.sort();
        NodeView {
            info,
            connected: d.connected,
            cordoned: d.cordoned,
            status,
            max_sessions: d.max_sessions,
            running,
            reserved: d.reserved,
            metrics: d.metrics,
            repo_cache,
            last_heartbeat_ms: d
                .last_heartbeat
                .map(|t| Utc::now().timestamp_millis() - t.elapsed().as_millis() as i64)
                .unwrap_or(0),
        }
    }

    pub async fn patch_node(
        &self,
        id: &str,
        labels: Option<Vec<(String, String)>>,
        max_sessions: Option<u32>,
        cordoned: Option<bool>,
    ) -> Result<NodeView, String> {
        let node = self
            .nodes
            .read()
            .get(id)
            .cloned()
            .ok_or_else(|| "unknown node".to_string())?;
        {
            let mut d = node.dynamic.write();
            if let Some(v) = max_sessions {
                d.max_sessions = v;
            }
            if let Some(v) = cordoned {
                d.cordoned = v;
            }
        }
        if let Some(labels) = &labels {
            node.info.write().labels = labels.clone();
        }
        state::update_node_settings(
            &self.db,
            id,
            labels.as_ref().map(|l| serialize_labels(l)).as_deref(),
            max_sessions.map(i64::from),
            cordoned,
        )
        .await
        .map_err(|e| e.to_string())?;
        Ok(self.view(&node))
    }

    /// Forget a node. Its sessions are released — they are gone with it.
    pub async fn remove_node(&self, id: &str) -> Result<(), String> {
        let node = self.nodes.write().remove(id);
        if let Some(node) = &node {
            let sessions: Vec<String> = node.dynamic.read().running.iter().cloned().collect();
            for s in sessions {
                self.expire_session(&s);
            }
        }
        state::delete_node(&self.db, id)
            .await
            .map_err(|e| e.to_string())
    }

    fn expire_session(&self, session: &str) {
        self.release(session);
        let hook = self.on_session_exit.read().clone();
        if let Some(hook) = hook {
            hook(session.to_string());
        }
    }

    /// Mint a join token. The plaintext is returned once and only the
    /// hash is stored, so a leaked database does not hand out cluster
    /// membership.
    pub async fn issue_join_token(&self, ttl: Duration, uses: u32) -> Result<String, String> {
        let token = new_token();
        let expires = Utc::now().timestamp_millis() + ttl.as_millis() as i64;
        state::create_join_token(
            &self.db,
            &uuid::Uuid::new_v4().to_string(),
            &hash_token(&token),
            expires,
            uses.max(1) as i64,
        )
        .await
        .map_err(|e| e.to_string())?;
        let _ = state::purge_spent_join_tokens(&self.db).await;
        Ok(token)
    }

    /// Watch for nodes that stopped talking. A node that misses enough
    /// heartbeats stops receiving work; one that stays gone has its
    /// sessions declared lost, because a PTY cannot outlive the process
    /// that owns it.
    pub fn spawn_reaper(self: &Arc<Self>) {
        let me = self.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(5));
            loop {
                tick.tick().await;
                me.reap().await;
            }
        });
    }

    async fn reap(&self) {
        let nodes: Vec<Arc<NodeHandle>> = self.nodes.read().values().cloned().collect();
        for node in nodes {
            let (gone, sessions) = {
                let d = node.dynamic.read();
                let unreachable = !d.connected
                    || d.last_heartbeat
                        .map(|t| t.elapsed() > self.opts.lost_after)
                        .unwrap_or(false);
                let long_gone = d
                    .disconnected_at
                    .map(|t| t.elapsed() > self.opts.lost_after)
                    .unwrap_or(false)
                    || (unreachable
                        && d.last_heartbeat
                            .map(|t| t.elapsed() > self.opts.lost_after)
                            .unwrap_or(false));
                (long_gone, d.running.iter().cloned().collect::<Vec<_>>())
            };
            if !gone {
                continue;
            }
            if sessions.is_empty() {
                continue;
            }
            tracing::warn!(
                node = %node.name,
                count = sessions.len(),
                "node has been unreachable past the grace period; releasing its sessions"
            );
            let _ = state::set_node_status(
                &self.db,
                &node.id,
                NodeState::Lost.as_str(),
                Utc::now().timestamp_millis(),
            )
            .await;
            for s in sessions {
                self.expire_session(&s);
            }
        }
    }
}

fn fail_pending(node: &Arc<NodeHandle>, reason: &str) {
    let waiters: Vec<_> = node.pending.lock().drain().map(|(_, tx)| tx).collect();
    for tx in waiters {
        let _ = tx.send(Err(reason.to_string()));
    }
}

fn hash_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    format!("{digest:x}")
}

fn new_token() -> String {
    // Two v4 UUIDs: 256 bits from the OS RNG, without another dependency.
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

/// Length-independent comparison for the shared static token.
fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn serialize_labels(labels: &[(String, String)]) -> String {
    let map: serde_json::Map<String, serde_json::Value> = labels
        .iter()
        .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
        .collect();
    serde_json::Value::Object(map).to_string()
}

fn parse_labels(raw: &str) -> Option<Vec<(String, String)>> {
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    let obj = value.as_object()?;
    Some(
        obj.iter()
            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_hashed_not_stored() {
        let t = new_token();
        assert_eq!(t.len(), 64);
        let h = hash_token(&t);
        assert_eq!(h.len(), 64);
        assert_ne!(h, t);
        assert_eq!(h, hash_token(&t), "hashing must be stable");
    }

    #[test]
    fn constant_time_eq_matches_normal_equality() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("abc", "abcd"));
    }

    #[test]
    fn labels_round_trip_through_storage() {
        let labels = vec![
            ("gpu".to_string(), "true".to_string()),
            ("arch".to_string(), "arm64".to_string()),
        ];
        let mut back = parse_labels(&serialize_labels(&labels)).unwrap();
        back.sort();
        let mut expected = labels;
        expected.sort();
        assert_eq!(back, expected);
    }

    #[test]
    fn malformed_stored_labels_are_ignored_rather_than_fatal() {
        assert!(parse_labels("not json").is_none());
        assert!(parse_labels("[1,2]").is_none());
    }
}
