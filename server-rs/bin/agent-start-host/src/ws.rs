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
    let Some(session) = app.pty.get(&name, window) else {
        return (StatusCode::NOT_FOUND, "session not found").into_response();
    };
    ws.on_upgrade(move |socket| handle(socket, session, app))
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
