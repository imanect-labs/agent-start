//! Chat-mode (#34) helpers shared by `start_session` and boot rehydration.
//!
//! A chat session is an ordinary session row whose CLI runs `claude`
//! headless in stream-json mode (`CliConfig.mode == "chat"`). The chat
//! process is owned by `ChatManager`; this module wires its lossless
//! persistence sink to SQLite and builds the spawn spec.

use crate::app::Shared;
use chat_manager::ChatSession;
use std::sync::Arc;

/// How many times to retry a transient chat-message persist before giving up.
const MAX_PERSIST_RETRIES: u32 = 5;

/// Attach the lossless persistence task to a chat session. Every committed
/// message (user / assistant / tool echo) is written to `chat_messages`,
/// and Claude's resumable `session_id` is persisted as soon as it is known
/// so a host restart can `--resume` the same conversation.
pub fn attach_persistence(state: Shared, session: Arc<ChatSession>) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    session.set_commit_sink(tx);
    let name = session.name().to_string();
    let weak = Arc::downgrade(&session);
    tokio::spawn(async move {
        let mut last_sid = String::new();
        // Set the sidebar title from the first user turn — but only once, and
        // only for sessions that don't already have one (issue launches set
        // it at creation from the prompt).
        let mut title_attempted = false;
        while let Some(ev) = rx.recv().await {
            // Retry on transient SQLite failures (e.g. SQLITE_BUSY) so the
            // transcript stays lossless; give up after a bounded number of
            // attempts rather than spinning forever on a permanent error.
            // The mpsc queue holds later events in order while we retry.
            for attempt in 0..MAX_PERSIST_RETRIES {
                match state::append_chat_message(&state.db, &name, ev.seq, &ev.role, &ev.json).await
                {
                    Ok(()) => break,
                    Err(e) => {
                        let last = attempt + 1 == MAX_PERSIST_RETRIES;
                        tracing::warn!(
                            error = %e, session = %name, seq = ev.seq, attempt, give_up = last,
                            "failed to persist chat message"
                        );
                        if last {
                            break;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    }
                }
            }
            if let Some(s) = weak.upgrade() {
                let sid = s.claude_session_id();
                if !sid.is_empty() && sid != last_sid {
                    last_sid = sid.clone();
                    if let Err(e) = state::set_claude_session_id(&state.db, &name, &sid).await {
                        tracing::warn!(error = %e, session = %name, "failed to persist claude session id");
                    }
                }
            }

            if !title_attempted && ev.role == "user_input" {
                title_attempted = true;
                // Only fill in a title the session is still missing; an
                // issue launch already set one at creation time.
                let needs_title = state
                    .sessions
                    .read()
                    .get(&name)
                    .map(|d| d.title.is_empty())
                    .unwrap_or(false);
                if needs_title {
                    if let Some(text) = first_user_text(&ev.json) {
                        let title = crate::sessions::summarize_title(&text);
                        if !title.is_empty() {
                            if let Some(d) = state.sessions.write().get_mut(&name) {
                                d.title = title.clone();
                            }
                            if let Err(e) =
                                state::update_session_title(&state.db, &name, &title).await
                            {
                                tracing::warn!(error = %e, session = %name, "failed to persist session title");
                            }
                        }
                    }
                }
            }
        }
    });
}

/// Pull the first text block out of a synthesized `user_input` envelope
/// (`{"type":"user_input","content":[{"type":"text","text":...}, …]}`).
fn first_user_text(json: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let content = value.get("content")?.as_array()?;
    for block in content {
        if block.get("type").and_then(|t| t.as_str()) == Some("text") {
            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                if !text.trim().is_empty() {
                    return Some(text.to_string());
                }
            }
        }
    }
    None
}
