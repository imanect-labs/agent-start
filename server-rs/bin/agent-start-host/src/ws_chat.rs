//! WebSocket handler for `/ws/chat?session=<name>` (#34).
//!
//! The browser receives a stream of JSON *envelopes* — committed
//! transcript replayed from SQLite, then the in-flight buffer, then live
//! events forwarded verbatim from the `claude` process (decision 3). It
//! sends `ChatClientMessage` frames (user_message / interrupt / set_model).
//!
//! Disconnect never kills the conversation (decision 4): the chat process
//! keeps running and reconnects replay the transcript. A dead conversation
//! (crash / restart) is revived on the next `user_message` via `--resume`
//! (decision 12).

use agent_start_api::ChatClientMessage;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chat_manager::{ChatImage, ChatSession};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use std::sync::Arc;

use crate::app::Shared;

#[derive(Debug, Deserialize)]
pub struct ChatQuery {
    pub session: Option<String>,
}

pub async fn ws_chat(
    ws: WebSocketUpgrade,
    State(app): State<Shared>,
    Query(q): Query<ChatQuery>,
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
    let Some(session) = app.chat.get(&name) else {
        return (StatusCode::NOT_FOUND, "chat session not found").into_response();
    };
    ws.on_upgrade(move |socket| handle(socket, session, app, name))
}

async fn handle(socket: WebSocket, session: Arc<ChatSession>, app: Shared, name: String) {
    let (mut sink, mut stream) = socket.split();

    // 1. Replay the committed transcript (each row is a full envelope with
    //    its own `_seq`; the client dedupes by `_seq`).
    match state::load_chat_messages(&app.db, &name).await {
        Ok(rows) => {
            for row in rows {
                if sink
                    .send(Message::Text(row.content_json.into()))
                    .await
                    .is_err()
                {
                    return;
                }
            }
        }
        Err(e) => tracing::warn!(error = %e, session = %name, "failed to load chat transcript"),
    }

    // 2. Subscribe to live events + snapshot the in-flight buffer atomically.
    let (inflight, mut rx) = session.subscribe();

    // 3. Connection status so the UI can show running / dead + current model.
    let state_str = if session.is_alive() {
        "running"
    } else {
        "dead"
    };
    let status = serde_json::json!({
        "type": "chat_status",
        "state": state_str,
        "model": session.current_model(),
        "replayDone": true,
    })
    .to_string();
    if sink.send(Message::Text(status.into())).await.is_err() {
        return;
    }

    // 4. In-flight buffer (a partially streamed turn, if any).
    for line in inflight {
        if sink.send(Message::Text(line.into())).await.is_err() {
            return;
        }
    }

    // Writer task: forward live events to the browser.
    let writer = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(line) => {
                    if sink.send(Message::Text(line.into())).await.is_err() {
                        break;
                    }
                }
                // Dropped some events under load; keep going rather than
                // tearing down the socket. Committed messages are also
                // persisted, so a manual refresh recovers anything lost.
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    });

    // Reader loop: browser → conversation.
    while let Some(frame) = stream.next().await {
        let Ok(msg) = frame else { break };
        match msg {
            Message::Text(text) => {
                let Ok(cm) = serde_json::from_str::<ChatClientMessage>(&text) else {
                    continue;
                };
                handle_client_message(&app, &session, &name, cm).await;
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
    // Decision 4: do NOT kill the conversation on disconnect.
    writer.abort();
}

async fn handle_client_message(
    app: &Shared,
    session: &Arc<ChatSession>,
    name: &str,
    cm: ChatClientMessage,
) {
    match cm {
        ChatClientMessage::UserMessage { text, images } => {
            let imgs: Vec<ChatImage> = images
                .into_iter()
                .map(|i| ChatImage {
                    media_type: i.media_type,
                    data: i.data,
                    thumb: i.thumb,
                })
                .collect();
            // Revive a dead conversation before sending (decision 12).
            if !session.is_alive() {
                if let Err(e) = session.revive().await {
                    tracing::warn!(error = %e, session = %name, "chat revive failed");
                    session.inject(
                        serde_json::json!({
                            "type": "chat_error",
                            "message": format!("会話の再開に失敗しました: {e}"),
                        }),
                        false,
                    );
                    return;
                }
                mark_running(app, name).await;
            }
            if let Err(e) = session.send_user_message(&text, &imgs).await {
                tracing::warn!(error = %e, session = %name, "chat send failed");
                session.inject(
                    serde_json::json!({
                        "type": "chat_error",
                        "message": format!("送信に失敗しました: {e}"),
                    }),
                    false,
                );
            }
        }
        ChatClientMessage::Interrupt => {
            let _ = session.interrupt().await;
        }
        ChatClientMessage::SetModel { model } => {
            let was_dead = !session.is_alive();
            if let Err(e) = session.switch_model(&model).await {
                tracing::warn!(error = %e, session = %name, "model switch failed");
                session.inject(
                    serde_json::json!({
                        "type": "chat_error",
                        "message": format!("モデル切替に失敗しました: {e}"),
                    }),
                    false,
                );
                return;
            }
            if was_dead {
                mark_running(app, name).await;
            }
        }
    }
}

/// Flip the persisted + in-memory session state back to running after a
/// revive / model-switch brought a dead conversation back to life.
async fn mark_running(app: &Shared, name: &str) {
    if let Err(e) = state::mark_running(&app.db, name, None).await {
        tracing::warn!(error = %e, session = %name, "failed to mark chat running");
    }
    if let Some(d) = app.sessions.write().get_mut(name) {
        d.live = true;
    }
}
