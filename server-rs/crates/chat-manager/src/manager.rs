//! `ChatManager` — owns one `ChatSession` per session name.
//!
//! Mirrors `PtyManager`'s shape (a keyed map + an exit hook) so the host
//! can treat chat and PTY sessions uniformly where it matters (lifecycle,
//! "mark dead on exit"). Unlike PTY there is exactly one chat per session
//! name (decision 2: 1 session = 1 chat), so the key is just the name.

use crate::error::ChatError;
use crate::session::{ChatSession, ChatSpawnSpec};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

/// Fired once when a chat conversation's process exits unexpectedly (crash
/// or host-driven kill that wasn't a planned respawn). The host marks the
/// session `dead` in SQLite, mirroring the PTY `ExitHook`.
pub type ChatExitHook = Arc<dyn Fn(&str) + Send + Sync>;

#[derive(Default)]
pub struct ChatManager {
    sessions: Mutex<HashMap<String, Arc<ChatSession>>>,
    on_exit: Mutex<Option<ChatExitHook>>,
}

impl ChatManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_exit_hook(&self, hook: ChatExitHook) {
        *self.on_exit.lock() = Some(hook);
    }

    pub(crate) fn fire_exit(&self, name: &str) {
        let hook = self.on_exit.lock().clone();
        if let Some(hook) = hook {
            hook(name);
        }
    }

    pub fn get(&self, name: &str) -> Option<Arc<ChatSession>> {
        self.sessions.lock().get(name).cloned()
    }

    pub fn has(&self, name: &str) -> bool {
        self.sessions.lock().contains_key(name)
    }

    /// Create a chat session and spawn its process. Errors if a session of
    /// the same name already exists or the process fails to start.
    pub async fn spawn(
        self: &Arc<Self>,
        spec: ChatSpawnSpec,
    ) -> Result<Arc<ChatSession>, ChatError> {
        let name = spec.name.clone();
        if self.sessions.lock().contains_key(&name) {
            return Err(ChatError::Spawn(format!("chat already exists: {name}")));
        }
        let session = ChatSession::create(spec, Arc::downgrade(self));
        session.start().await?;
        self.sessions.lock().insert(name, session.clone());
        Ok(session)
    }

    /// Register a chat session without starting its process — used to
    /// rehydrate a stopped conversation after a host restart so the
    /// transcript is browsable and the first send can revive it.
    pub fn insert_dormant(self: &Arc<Self>, spec: ChatSpawnSpec) -> Arc<ChatSession> {
        let name = spec.name.clone();
        let session = ChatSession::create(spec, Arc::downgrade(self));
        self.sessions.lock().insert(name, session.clone());
        session
    }

    /// Drop a session from the map and kill its process.
    pub fn remove(&self, name: &str) -> Option<Arc<ChatSession>> {
        let session = self.sessions.lock().remove(name);
        if let Some(s) = &session {
            s.kill();
        }
        session
    }
}
