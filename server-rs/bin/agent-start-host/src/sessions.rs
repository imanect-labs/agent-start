//! Live session metadata held in memory alongside SQLite persistence.
//!
//! Entries are inserted by `start_session` and removed by either
//! `delete_session` or the PtyManager exit hook (for natural child
//! exits). On boot we additionally rehydrate rows whose worktree dir
//! still exists on disk — the PTYs are gone but the worktree is real
//! work the user shouldn't lose track of.

use agent_start_api::Session;
use std::path::Path as StdPath;

/// Environment handed to a freshly spawned session process (PTY or chat).
/// Exposes the worktree/orig paths and session name to the agent CLI.
pub fn launch_env(orig: &StdPath, name: &str, cwd: &StdPath) -> Vec<(String, String)> {
    vec![
        (
            "AGENT_START_ROOT_PATH".into(),
            orig.to_string_lossy().into_owned(),
        ),
        ("AGENT_START_WORKSPACE_NAME".into(), name.to_string()),
        (
            "AGENT_START_WORKSPACE_PATH".into(),
            cwd.to_string_lossy().into_owned(),
        ),
        ("TERM".into(), "xterm-256color".into()),
    ]
}

#[derive(Debug, Clone)]
pub struct SessionDirectory {
    pub name: String,
    pub created_at_ms: i64,
    pub cli: String,
    pub cwd: String,
    pub worktree_path: String,
    pub orig_path: String,
    /// False when rehydrated from SQLite after a restart — the worktree
    /// is on disk but no PTY is running. Such entries can be deleted
    /// (optionally removing the worktree) or relaunched into a fresh
    /// session pointed at the same orig_path.
    pub live: bool,
    /// Persisted PTY scrollback. Populated only for rehydrated stopped
    /// sessions so the WS handler can replay last-known terminal state
    /// without a live PtySession.
    pub history: Vec<u8>,
}

impl SessionDirectory {
    pub fn to_api(&self, attached: bool) -> Session {
        Session {
            name: self.name.clone(),
            created_at: self.created_at_ms,
            attached: attached && self.live,
            stopped: !self.live,
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
