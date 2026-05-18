//! Live session metadata held in memory alongside SQLite persistence.
//!
//! Entries are inserted by `start_session` and removed by either
//! `delete_session` or the PtyManager exit hook (for natural child
//! exits). We deliberately do *not* hydrate from SQLite on startup —
//! PTYs cannot survive a host restart, so any prior row would be a
//! zombie. See `app::run`'s call to `state::mark_all_running_dead`.

use agent_start_api::Session;

#[derive(Debug, Clone)]
pub struct SessionDirectory {
    pub name: String,
    pub created_at_ms: i64,
    pub cli: String,
    pub cwd: String,
    pub worktree_path: String,
    pub orig_path: String,
}

impl SessionDirectory {
    pub fn to_api(&self, attached: bool) -> Session {
        Session {
            name: self.name.clone(),
            created_at: self.created_at_ms,
            attached,
            path: if self.worktree_path.is_empty() {
                self.cwd.clone()
            } else {
                self.worktree_path.clone()
            },
            cli: self.cli.clone(),
            worktree_path: self.worktree_path.clone(),
            orig_path: self.orig_path.clone(),
        }
    }
}
