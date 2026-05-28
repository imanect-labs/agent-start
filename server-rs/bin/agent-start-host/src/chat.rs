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
        }
    });
}
