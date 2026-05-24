//! WebSocket handler for `/ws/terminal?session=<name>&window=<n>`.
//!
//! Wire-compatible with the old Node implementation: clients send
//! `{type:"input"|"resize"|"scroll", ...}` JSON frames, server pushes
//! raw PTY bytes as binary frames. On connect we replay the entire ring
//! buffer so reconnects are seamless. `window` defaults to 0 (the
//! session's primary PTY).

use agent_start_api::ClientMessage;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;

use crate::app::Shared;

#[derive(Debug, Deserialize)]
pub struct TermQuery {
    pub session: Option<String>,
    /// Defaults to 0 — the session's primary PTY.
    pub window: Option<u32>,
}

pub async fn ws_terminal(
    ws: WebSocketUpgrade,
    State(app): State<Shared>,
    Query(q): Query<TermQuery>,
) -> Response {
    let Some(name) = q.session else {
        return (
            StatusCode::BAD_REQUEST,
            "session query parameter is required",
        )
            .into_response();
    };
    if !workspace_manager::is_valid_session_name(&name) {
        return (StatusCode::BAD_REQUEST, "invalid session name").into_response();
    }
    let window = q.window.unwrap_or(0);
    if let Some(session) = app.pty.get(&name, window) {
        return ws.on_upgrade(move |socket| handle(socket, session, app));
    }
    // No live PTY — if we rehydrated this name from disk after a host
    // restart, hold the WS open so xterm doesn't loop-reconnect. For
    // window 0 we also replay snapshotted scrollback when available.
    let stopped = app.sessions.read().get(&name).filter(|d| !d.live).is_some();
    if !stopped {
        return (StatusCode::NOT_FOUND, "session not found").into_response();
    }
    let history = if window == 0 {
        app.sessions
            .read()
            .get(&name)
            .map(|d| d.history.clone())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    ws.on_upgrade(move |socket| handle_stopped(socket, history))
}

async fn handle_stopped(socket: WebSocket, history: Vec<u8>) {
    let (mut sink, mut stream) = socket.split();
    if history.is_empty() {
        let _ = sink
            .send(Message::Binary(
                b"\r\n(no terminal history saved for this session)\r\n".to_vec(),
            ))
            .await;
    } else {
        let _ = sink.send(Message::Binary(history)).await;
        let _ = sink
            .send(Message::Binary(
                b"\r\n\x1b[2m-- session stopped (restored from snapshot) --\x1b[0m\r\n".to_vec(),
            ))
            .await;
    }
    // Keep the socket open so xterm.js doesn't show a noisy disconnect
    // and immediately reconnect (which would clear+replay forever). The
    // client can't actually drive the PTY because there is none — we
    // just drain any frames it sends and ignore them until close.
    while let Some(msg) = stream.next().await {
        match msg {
            Ok(axum::extract::ws::Message::Close(_)) | Err(_) => break,
            _ => {}
        }
    }
}

async fn handle(socket: WebSocket, session: std::sync::Arc<pty_manager::PtySession>, _app: Shared) {
    let (mut sink, mut stream) = socket.split();

    let (history, mut rx) = session.subscribe();
    if !history.is_empty() {
        if let Err(err) = sink.send(Message::Binary(history)).await {
            tracing::debug!(?err, "failed to send history; closing");
            return;
        }
    }

    let writer_session = session.clone();
    let writer = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(chunk) => {
                    if sink.send(Message::Binary(chunk)).await.is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    // We dropped some chunks; resend the current ring buffer so
                    // the client recovers without an explicit signal.
                    let snap = writer_session.subscribe().0;
                    if !snap.is_empty() && sink.send(Message::Binary(snap)).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    while let Some(frame) = stream.next().await {
        let Ok(msg) = frame else {
            break;
        };
        match msg {
            Message::Text(text) => {
                let Ok(client_msg) = serde_json::from_str::<ClientMessage>(&text) else {
                    continue;
                };
                match client_msg {
                    ClientMessage::Input { data } => {
                        if session.write(data.as_bytes()).is_err() {
                            break;
                        }
                    }
                    ClientMessage::Resize { cols, rows } => {
                        let _ = session.resize(cols.max(1), rows.max(1));
                    }
                    ClientMessage::Scroll { .. } => {
                        // Legacy tmux scroll request; xterm.js handles
                        // scrollback locally now, so we ignore this.
                    }
                }
            }
            Message::Binary(_) | Message::Ping(_) | Message::Pong(_) => {}
            Message::Close(_) => break,
        }
    }
    writer.abort();
}
