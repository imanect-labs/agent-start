//! Wire vocabulary shared by the control plane and the node agents.
//!
//! Both roles link this crate and nothing else of each other, so the
//! frames below are the *only* coupling between them. Two transports
//! carry the frames:
//!
//! * **loopback** — `--role all` runs a node inside the control-plane
//!   process; frames move through in-memory channels, never serialized.
//! * **websocket** — a remote `--role node` dials the control plane and
//!   the frames are serialized as JSON text (byte payloads base64'd).
//!
//! Keeping one frame enum for both means the single-host default
//! exercises the same code path as a real cluster.

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

pub mod b64 {
    //! Serde helper: `Vec<u8>` as base64 so PTY bytes survive JSON.
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine as _;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(v: &[u8], s: S) -> Result<S::Ok, S::Error> {
        STANDARD.encode(v).serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        STANDARD.decode(&s).map_err(serde::de::Error::custom)
    }
}

/// How strongly a session is separated from the host it runs on.
/// Doubles as a scheduling constraint: a task asking for `MicroVm` can
/// only land on a node whose executor provides it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum IsolationProfile {
    /// Plain child process on the node. No isolation (the v0.2 behaviour).
    #[default]
    Process,
    /// OCI container (docker / podman / k8s Pod).
    Container,
    /// Hardware-virtualized sandbox (Firecracker, Kata).
    MicroVm,
}

impl IsolationProfile {
    /// `self` is satisfiable by a backend offering `other`.
    /// Stronger isolation always satisfies a weaker request.
    pub fn satisfied_by(self, other: IsolationProfile) -> bool {
        other >= self
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Process => "process",
            Self::Container => "container",
            Self::MicroVm => "microvm",
        }
    }
}

/// CPU (in thousandths of a core) and memory (MiB) a session asks for.
/// Mirrors the K8s "requests" concept: used for admission and bin-packing,
/// not enforced by the `process` backend.
/// `Default` is deliberately **zero**, not a typical request size: this
/// type is used both for "what a session asks for" and for "what a node
/// currently has reserved", and a non-zero default silently inflates the
/// latter. Ask for [`Resources::default_request`] when you want a
/// session-sized default.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Resources {
    #[serde(rename = "cpuMillis")]
    pub cpu_millis: u32,
    #[serde(rename = "memMb")]
    pub mem_mb: u32,
}

impl Resources {
    pub const ZERO: Self = Self {
        cpu_millis: 0,
        mem_mb: 0,
    };

    /// One core and 2 GiB — enough for an agent CLI plus a test run.
    pub const fn default_request() -> Self {
        Self {
            cpu_millis: 1000,
            mem_mb: 2048,
        }
    }
}

/// What a node can host in total, as reported at `Hello` time.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct NodeCapacity {
    #[serde(rename = "cpuMillis")]
    pub cpu_millis: u32,
    #[serde(rename = "memMb")]
    pub mem_mb: u32,
    #[serde(rename = "maxSessions")]
    pub max_sessions: u32,
}

/// Sampled utilization, reported on every heartbeat. Already smoothed
/// (EWMA) on the node so the scheduler does not chase spikes.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub struct NodeMetrics {
    #[serde(rename = "cpuUtil")]
    pub cpu_util: f32,
    #[serde(rename = "memUtil")]
    pub mem_util: f32,
    pub load1: f32,
}

/// Enough of a project for a node that has never seen it to obtain the
/// source: either it already has the path, or it can clone the URL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectRef {
    /// Stable id derived from the project path on the control plane.
    /// Also the key of the node's bare-mirror cache directory.
    pub id: String,
    pub name: String,
    /// Path on the *control plane's* filesystem. A node uses it only
    /// when the same path happens to exist locally (always true for the
    /// loopback node, which is why single-host behaviour is unchanged).
    #[serde(rename = "localPath")]
    pub local_path: String,
    /// `origin` remote of the project, when it has one. Without it the
    /// project cannot travel to another node.
    #[serde(rename = "cloneUrl", skip_serializing_if = "Option::is_none")]
    pub clone_url: Option<String>,
}

/// Everything a node needs to bring one session to life.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssignSpec {
    pub session: String,
    pub cli: String,
    /// Full shell command line (already built and sanitized by the
    /// control plane). Empty means "interactive login shell".
    pub command: String,
    pub shell: String,
    pub project: ProjectRef,
    #[serde(rename = "createWorktree")]
    pub create_worktree: bool,
    /// True when the CLI is Claude and the worktree must be marked trusted.
    #[serde(rename = "markClaudeTrusted")]
    pub mark_claude_trusted: bool,
    pub requests: Resources,
    pub isolation: IsolationProfile,
    /// Extra environment for the agent process, on top of the
    /// `AGENT_START_*` variables the node derives itself.
    #[serde(default)]
    pub env: Vec<(String, String)>,
}

/// What the node reports back once the session is running.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssignOk {
    pub session: String,
    /// Directory the agent actually runs in, on the node.
    pub cwd: String,
    #[serde(rename = "worktreePath")]
    pub worktree_path: String,
    #[serde(rename = "origPath")]
    pub orig_path: String,
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionEventKind {
    Exited { code: Option<i32> },
    Failed { error: String },
}

/// What the control plane asks a node to do with a finished session's
/// worktree: turn whatever the agent produced into a commit, a pushed
/// branch, and (optionally) a pull request.
///
/// This runs *on the node* because that is where the worktree is. Doing
/// it centrally would work only for the in-process node and silently do
/// the wrong thing — or nothing — for every other machine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalizeSpec {
    /// Message for the commit made from the agent's uncommitted work.
    /// When the worktree is clean, nothing is committed and the branch
    /// is pushed as-is.
    #[serde(rename = "commitMessage")]
    pub commit_message: String,
    /// Push the session branch to `origin` (always with `-u`, never
    /// forced).
    pub push: bool,
    /// Open a pull request with `gh` after a successful push.
    #[serde(rename = "openPr")]
    pub open_pr: bool,
    #[serde(rename = "prTitle", default)]
    pub pr_title: String,
    #[serde(rename = "prBody", default)]
    pub pr_body: String,
    #[serde(default)]
    pub draft: bool,
    /// Base branch for the PR. Empty means the repository default.
    #[serde(rename = "baseBranch", default)]
    pub base_branch: String,
}

/// What the node managed to do. Partial success is normal and expected:
/// a commit can land while `gh` is missing, so each step reports itself
/// rather than collapsing into one boolean.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FinalizeOk {
    // Every field defaults: a node one version behind that omits one
    // must not make the whole frame undecodable, which the caller would
    // experience as a finalize that never answers.
    #[serde(default)]
    pub committed: bool,
    #[serde(default)]
    pub sha: String,
    #[serde(default)]
    pub branch: String,
    #[serde(default)]
    pub pushed: bool,
    #[serde(rename = "prUrl", default)]
    pub pr_url: String,
    /// Non-fatal notes for the user ("nothing to commit", "gh is not
    /// installed, so no PR was opened").
    #[serde(default)]
    pub notes: Vec<String>,
}

/// Target of a relayed stream. Today only PTY windows; HTTP port
/// forwarding for code-server lands with the container executors.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum StreamTarget {
    Pty { window: u32 },
}

/// node → control.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "kebab-case")]
pub enum NodeFrame {
    Hello(Box<Hello>),
    Heartbeat {
        seq: u64,
        metrics: NodeMetrics,
        running: Vec<String>,
        #[serde(rename = "repoCache")]
        repo_cache: Vec<String>,
    },
    /// Outcome of an `Assign`, correlated by `assignId`.
    Ack {
        #[serde(rename = "assignId")]
        assign_id: String,
        result: Result<AssignOk, String>,
    },
    SessionEvent {
        session: String,
        event: SessionEventKind,
    },
    /// Outcome of a `Finalize`, correlated by `requestId`.
    FinalizeResult {
        #[serde(rename = "requestId")]
        request_id: String,
        result: Result<FinalizeOk, String>,
    },
    /// Payload for a relayed stream. `seq` is per-channel and lets the
    /// control plane spot gaps without inspecting the bytes.
    Stream {
        ch: u32,
        seq: u64,
        #[serde(with = "b64")]
        data: Vec<u8>,
    },
    StreamClose {
        ch: u32,
        reason: String,
    },
    Pong {
        seq: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hello {
    /// Operator-chosen node name (defaults to the machine hostname).
    pub name: String,
    /// Shared join token, or the node's own long-lived token on reconnect.
    pub token: String,
    /// Set when this node has registered before, so the control plane
    /// can reuse its row (and its recorded labels) instead of minting a
    /// second identity for the same machine.
    #[serde(rename = "nodeId", skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    pub version: String,
    pub os: String,
    pub arch: String,
    /// Isolation profiles this node's configured executor can provide.
    pub executors: Vec<IsolationProfile>,
    pub capacity: NodeCapacity,
    pub labels: Vec<(String, String)>,
    /// Sessions still alive from before a control-plane restart, so the
    /// registry can adopt them instead of orphaning them.
    #[serde(default)]
    pub running: Vec<String>,
}

/// control → node.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "kebab-case")]
pub enum ControlFrame {
    Welcome {
        #[serde(rename = "nodeId")]
        node_id: String,
        /// Long-lived per-node token; replaces the join token on reconnect.
        token: String,
        #[serde(rename = "heartbeatSecs")]
        heartbeat_secs: u64,
    },
    /// Registration was refused (bad token, duplicate name). The node
    /// logs the reason and backs off rather than hammering the endpoint.
    Rejected {
        reason: String,
    },
    Assign {
        #[serde(rename = "assignId")]
        assign_id: String,
        spec: Box<AssignSpec>,
    },
    Cancel {
        session: String,
        #[serde(rename = "deleteWorktree")]
        delete_worktree: bool,
    },
    /// Commit / push / open a PR from a finished session's worktree.
    Finalize {
        #[serde(rename = "requestId")]
        request_id: String,
        session: String,
        spec: Box<FinalizeSpec>,
    },
    StreamOpen {
        ch: u32,
        session: String,
        target: StreamTarget,
    },
    StreamData {
        ch: u32,
        #[serde(with = "b64")]
        data: Vec<u8>,
    },
    StreamResize {
        ch: u32,
        cols: u16,
        rows: u16,
    },
    StreamClose {
        ch: u32,
    },
    Ping {
        seq: u64,
    },
}

/// Buffered depth of each direction of a link. Deep enough to absorb a
/// burst of PTY output without blocking the reader task, shallow enough
/// that a stalled consumer applies backpressure instead of growing
/// without bound.
pub const LINK_CHANNEL_CAP: usize = 512;

/// One node's end of the connection.
pub struct NodeLink {
    pub tx: mpsc::Sender<NodeFrame>,
    pub rx: mpsc::Receiver<ControlFrame>,
}

/// The control plane's end of the same connection.
pub struct ControlLink {
    pub tx: mpsc::Sender<ControlFrame>,
    pub rx: mpsc::Receiver<NodeFrame>,
    /// True when the peer needs no credential — it is inside this
    /// process and there is no channel to impersonate it on.
    pub trusted: bool,
    /// True when the peer shares this machine's filesystem. Only such a
    /// node can run a project that has no origin remote to clone from.
    /// Distinct from `trusted`: trust is about authentication, locality
    /// is about which files the node can see.
    pub local: bool,
}

/// Build both ends of an in-process link.
pub fn loopback() -> (ControlLink, NodeLink) {
    let (ctl_tx, ctl_rx) = mpsc::channel(LINK_CHANNEL_CAP);
    let (node_tx, node_rx) = mpsc::channel(LINK_CHANNEL_CAP);
    (
        ControlLink {
            tx: ctl_tx,
            rx: node_rx,
            trusted: true,
            local: true,
        },
        NodeLink {
            tx: node_tx,
            rx: ctl_rx,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stronger_isolation_satisfies_weaker_requests() {
        assert!(IsolationProfile::Process.satisfied_by(IsolationProfile::MicroVm));
        assert!(IsolationProfile::Container.satisfied_by(IsolationProfile::Container));
        assert!(!IsolationProfile::MicroVm.satisfied_by(IsolationProfile::Container));
    }

    #[test]
    fn stream_payloads_survive_a_json_round_trip() {
        let frame = NodeFrame::Stream {
            ch: 3,
            seq: 9,
            // Deliberately not valid UTF-8: PTY output rarely is.
            data: vec![0x1b, 0x5b, 0x32, 0x4a, 0xff, 0x00],
        };
        let json = serde_json::to_string(&frame).unwrap();
        let back: NodeFrame = serde_json::from_str(&json).unwrap();
        match back {
            NodeFrame::Stream { ch, seq, data } => {
                assert_eq!((ch, seq), (3, 9));
                assert_eq!(data, vec![0x1b, 0x5b, 0x32, 0x4a, 0xff, 0x00]);
            }
            other => panic!("wrong frame: {other:?}"),
        }
    }

    #[test]
    fn ack_carries_either_outcome() {
        let err = NodeFrame::Ack {
            assign_id: "a1".into(),
            result: Err("worktree creation failed".into()),
        };
        let json = serde_json::to_string(&err).unwrap();
        let back: NodeFrame = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, NodeFrame::Ack { result: Err(e), .. } if e.contains("worktree")));
    }
}
