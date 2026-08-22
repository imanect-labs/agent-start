//! WebSocket transport for a remote `--role node`.
//!
//! The node is always the dialer. Everything after the handshake is the
//! same `NodeLink` the in-process node gets, so `NodeRuntime` has no
//! idea whether it is talking over a socket or a channel.

use crate::{Identity, NodeRuntime, Rejected};
use cluster_proto::{ControlFrame, NodeFrame, NodeLink, LINK_CHANNEL_CAP};
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

/// Path the control plane serves the cluster endpoint on.
pub const CONNECT_PATH: &str = "/cluster/v1/connect";

const RECONNECT_MIN: Duration = Duration::from_secs(1);
const RECONNECT_MAX: Duration = Duration::from_secs(30);
/// How long a connection must survive before it counts as healthy. A
/// control plane that accepts and immediately drops us would otherwise
/// reset the backoff on every attempt and be hammered once a second.
const STABLE_AFTER: Duration = Duration::from_secs(30);

/// Turn an operator-facing control-plane URL into the WebSocket URL of
/// the cluster endpoint. Accepts `http(s)://host[:port]` — the form
/// people already paste into a browser — as well as an explicit
/// `ws(s)://`, and tolerates a trailing slash.
pub fn connect_url(join_url: &str) -> String {
    let trimmed = join_url.trim().trim_end_matches('/');
    let base = if let Some(rest) = trimmed.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = trimmed.strip_prefix("http://") {
        format!("ws://{rest}")
    } else if trimmed.starts_with("ws://") || trimmed.starts_with("wss://") {
        trimmed.to_string()
    } else {
        format!("ws://{trimmed}")
    };
    if base.ends_with(CONNECT_PATH) {
        base
    } else {
        format!("{base}{CONNECT_PATH}")
    }
}

/// Connect, serve, reconnect — forever. Returns only if the control
/// plane rejects this node outright, which is an operator error (bad or
/// expired join token) that retrying cannot fix.
pub async fn run_remote(
    runtime: Arc<NodeRuntime>,
    join_url: String,
    join_token: String,
) -> anyhow::Result<()> {
    let url = connect_url(&join_url);
    let identity_path = runtime.identity_path();
    let mut backoff = RECONNECT_MIN;

    loop {
        // Re-read on every attempt: the previous connection may have
        // upgraded us from a join token to a long-lived one.
        let stored = identity_path.as_deref().and_then(crate::load_identity);
        let (token, node_id) = match stored {
            Some(Identity { node_id, token }) => (token, Some(node_id)),
            None => (join_token.clone(), None),
        };

        match tokio_tungstenite::connect_async(&url).await {
            Ok((socket, _)) => {
                tracing::info!(url = %url, "connected to control plane");
                let started = tokio::time::Instant::now();
                let outcome = serve(runtime.clone(), socket, token, node_id).await;
                if started.elapsed() >= STABLE_AFTER {
                    backoff = RECONNECT_MIN;
                }
                match outcome {
                    Ok(()) => tracing::info!("control plane closed the connection; reconnecting"),
                    Err(e) => {
                        // A rejection is terminal: the credential is
                        // wrong and every retry will be too.
                        if e.downcast_ref::<Rejected>().is_some() {
                            return Err(e);
                        }
                        tracing::warn!(error = %e, "cluster link failed; reconnecting");
                    }
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, url = %url, backoff = ?backoff, "cannot reach control plane");
            }
        }

        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(RECONNECT_MAX);
    }
}

async fn serve<S>(
    runtime: Arc<NodeRuntime>,
    socket: tokio_tungstenite::WebSocketStream<S>,
    token: String,
    node_id: Option<String>,
) -> anyhow::Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (mut sink, mut stream) = socket.split();
    let (node_tx, mut node_rx) = mpsc::channel::<NodeFrame>(LINK_CHANNEL_CAP);
    let (ctl_tx, ctl_rx) = mpsc::channel::<ControlFrame>(LINK_CHANNEL_CAP);

    let writer = tokio::spawn(async move {
        while let Some(frame) = node_rx.recv().await {
            let json = match serde_json::to_string(&frame) {
                Ok(j) => j,
                Err(e) => {
                    tracing::warn!(error = %e, "failed to encode node frame");
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
                Ok(Message::Text(t)) => t.to_string(),
                Ok(Message::Close(_)) | Err(_) => break,
                // Ping/Pong/Binary carry no control frames of ours.
                Ok(_) => continue,
            };
            match serde_json::from_str::<ControlFrame>(&text) {
                Ok(frame) => {
                    if ctl_tx.send(frame).await.is_err() {
                        break;
                    }
                }
                Err(e) => tracing::warn!(error = %e, "undecodable control frame"),
            }
        }
    });

    let link = NodeLink {
        tx: node_tx,
        rx: ctl_rx,
    };
    let result = runtime.run(link, token, node_id).await;

    // Abort both halves rather than awaiting them: a silent peer can
    // keep the reader parked forever, and the writer only ends when
    // every frame sender is gone.
    reader.abort();
    writer.abort();
    result
}

#[cfg(test)]
mod tests {
    use super::connect_url;

    #[test]
    fn http_urls_become_websocket_urls() {
        assert_eq!(
            connect_url("http://ctl.example.ts.net:3030"),
            "ws://ctl.example.ts.net:3030/cluster/v1/connect"
        );
        assert_eq!(
            connect_url("https://ctl.example.ts.net/"),
            "wss://ctl.example.ts.net/cluster/v1/connect"
        );
    }

    #[test]
    fn bare_hosts_and_explicit_ws_urls_both_work() {
        assert_eq!(connect_url("ctl:3030"), "ws://ctl:3030/cluster/v1/connect");
        assert_eq!(
            connect_url("wss://ctl/cluster/v1/connect"),
            "wss://ctl/cluster/v1/connect"
        );
    }
}
