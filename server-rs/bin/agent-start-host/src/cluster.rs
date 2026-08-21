//! Host-side cluster wiring: role selection, the in-process node, and
//! the WebSocket endpoint remote nodes dial.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use cluster_control::{ControlOptions, ControlPlane};
use cluster_node::{NodeConfig, NodeRuntime};
use cluster_proto::{ControlFrame, ControlLink, NodeFrame, LINK_CHANNEL_CAP};
use futures_util::{SinkExt, StreamExt};
use pty_manager::PtyManager;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::app::Shared;

/// Path remote nodes dial. Defined by the node client so both ends
/// cannot drift apart.
pub use cluster_node::CONNECT_PATH;

/// Which halves of agent-start this process runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Role {
    /// Control plane *and* an in-process node — the single-host default,
    /// and behaviourally identical to pre-cluster agent-start.
    #[default]
    All,
    /// Scheduler, API and UI only. Work runs on nodes that dial in.
    Control,
    /// Agent execution only. No HTTP surface; dials a control plane.
    Node,
}

impl std::str::FromStr for Role {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "all" | "" => Ok(Self::All),
            "control" => Ok(Self::Control),
            "node" => Ok(Self::Node),
            other => Err(format!(
                "unknown role `{other}` (expected `all`, `control`, or `node`)"
            )),
        }
    }
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Control => "control",
            Self::Node => "node",
        }
    }

    pub fn runs_control_plane(self) -> bool {
        matches!(self, Self::All | Self::Control)
    }
}

#[derive(Debug, Clone, Default)]
pub struct ClusterArgs {
    pub role: Role,
    pub join_url: Option<String>,
    pub join_token: Option<String>,
    pub node_name: Option<String>,
    pub max_sessions: Option<u32>,
    /// `key=value` pairs advertised for scheduling.
    pub labels: Vec<(String, String)>,
    pub executor: Option<String>,
    /// Shared secret every node may present, as an alternative to
    /// minting per-node join tokens.
    pub cluster_token: Option<String>,
}

impl ClusterArgs {
    pub fn node_config(&self, identity_path: Option<std::path::PathBuf>) -> NodeConfig {
        NodeConfig {
            name: self
                .node_name
                .clone()
                .unwrap_or_else(cluster_node::hostname),
            max_sessions: self.max_sessions.unwrap_or(4),
            labels: self.labels.clone(),
            executor: self.executor.clone().unwrap_or_else(|| "process".into()),
            identity_path,
        }
    }
}

/// Build the control plane and, for `--role all`, attach the in-process
/// node over a loopback link. The local node registers exactly like a
/// remote one — same `Hello`, same scheduler, same relay — so there is
/// no second, untested path for the single-host case.
pub fn start_control_plane(
    db: state::Db,
    pty: Arc<PtyManager>,
    args: &ClusterArgs,
) -> Arc<ControlPlane> {
    let control = ControlPlane::new(
        db,
        ControlOptions {
            static_token: args.cluster_token.clone(),
            ..Default::default()
        },
    );
    control.spawn_reaper();

    if args.role == Role::All {
        let (control_link, node_link) = cluster_proto::loopback();
        let runtime = NodeRuntime::new(args.node_config(None), pty);
        tokio::spawn(control.clone().accept(control_link));
        tokio::spawn(async move {
            if let Err(e) = runtime.run(node_link, String::new(), None).await {
                tracing::error!(error = %e, "in-process node stopped");
            }
        });
    }
    control
}

/// `GET /cluster/v1/connect` — a remote node's end of the link.
///
/// The node is always the dialer, so this is the only inbound surface
/// the cluster needs, and nodes behind NAT join without any port
/// forwarding. Credentials travel in the first frame rather than in a
/// header so both transports authenticate identically.
pub async fn ws_cluster_connect(ws: WebSocketUpgrade, State(app): State<Shared>) -> Response {
    ws.on_upgrade(move |socket| serve_node(socket, app))
}

async fn serve_node(socket: WebSocket, app: Shared) {
    let Some(control) = app.cluster.clone() else {
        return;
    };
    let (mut sink, mut stream) = socket.split();
    let (ctl_tx, mut ctl_rx) = mpsc::channel::<ControlFrame>(LINK_CHANNEL_CAP);
    let (node_tx, node_rx) = mpsc::channel::<NodeFrame>(LINK_CHANNEL_CAP);

    let writer = tokio::spawn(async move {
        while let Some(frame) = ctl_rx.recv().await {
            let json = match serde_json::to_string(&frame) {
                Ok(j) => j,
                Err(e) => {
                    tracing::warn!(error = %e, "failed to encode control frame");
                    continue;
                }
            };
            if sink.send(Message::Text(json.into())).await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    });

    let reader = tokio::spawn(async move {
        while let Some(msg) = stream.next().await {
            let text = match msg {
                Ok(Message::Text(t)) => t,
                Ok(Message::Close(_)) | Err(_) => break,
                Ok(_) => continue,
            };
            match serde_json::from_str::<NodeFrame>(&text) {
                Ok(frame) => {
                    if node_tx.send(frame).await.is_err() {
                        break;
                    }
                }
                Err(e) => tracing::warn!(error = %e, "undecodable node frame"),
            }
        }
    });

    control
        .accept(ControlLink {
            tx: ctl_tx,
            rx: node_rx,
            // Remote peers authenticate with a token, and their files
            // are their own.
            trusted: false,
            local: false,
        })
        .await;

    // Both halves must be aborted, not awaited. The registry keeps a
    // frame sender for this node until it is evicted, so the writer
    // would sit in `recv()` forever — and a live receiver makes sends to
    // a dead connection *succeed*, which turns "node is gone" into a
    // terminal that opens and then never produces a byte.
    reader.abort();
    writer.abort();
}

/// `--role node`: run nothing but the agent runtime, dialing the
/// control plane named on the command line.
pub async fn run_node_only(args: ClusterArgs) -> anyhow::Result<()> {
    let join_url = args
        .join_url
        .clone()
        .ok_or_else(|| anyhow::anyhow!("--role node requires --join-url"))?;
    let identity_path = config_loader::host_state_dir().join("node-identity.json");
    let known = cluster_node::load_identity(&identity_path).is_some();
    let join_token = match (&args.join_token, known) {
        (Some(t), _) => t.clone(),
        // A node that already registered carries its own credential; it
        // only needs a join token the first time.
        (None, true) => String::new(),
        (None, false) => {
            anyhow::bail!("--role node requires --join-token on first registration")
        }
    };

    let cfg = args.node_config(Some(identity_path));
    tracing::info!(
        node = %cfg.name,
        url = %join_url,
        executor = %cfg.executor,
        max_sessions = cfg.max_sessions,
        "starting node agent"
    );
    let runtime = NodeRuntime::new(cfg, Arc::new(PtyManager::new()));
    cluster_node::run_remote(runtime, join_url, join_token).await
}

/// Parse a `key=value` command-line label. Rejects the empty key so a
/// stray `=gpu` cannot produce a label nothing can select on.
pub fn parse_label(raw: &str) -> Result<(String, String), String> {
    let (k, v) = raw
        .split_once('=')
        .ok_or_else(|| format!("label `{raw}` must be key=value"))?;
    let (k, v) = (k.trim(), v.trim());
    if k.is_empty() {
        return Err(format!("label `{raw}` has an empty key"));
    }
    Ok((k.to_string(), v.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn roles_parse_from_the_command_line() {
        assert_eq!(Role::from_str("all").unwrap(), Role::All);
        assert_eq!(Role::from_str("Control").unwrap(), Role::Control);
        assert_eq!(Role::from_str("node").unwrap(), Role::Node);
        assert!(Role::from_str("worker")
            .unwrap_err()
            .contains("unknown role"));
    }

    #[test]
    fn the_default_role_still_runs_a_control_plane() {
        assert_eq!(Role::default(), Role::All);
        assert!(Role::default().runs_control_plane());
        assert!(!Role::Node.runs_control_plane());
    }

    #[test]
    fn labels_need_a_key_and_a_value() {
        assert_eq!(
            parse_label("gpu=true").unwrap(),
            ("gpu".to_string(), "true".to_string())
        );
        assert!(parse_label("gpu").is_err());
        assert!(parse_label("=true").is_err());
    }
}
