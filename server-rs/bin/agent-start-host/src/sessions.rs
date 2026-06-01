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
    /// Short human-readable title derived from the initial task. Empty
    /// until known; the sidebar falls back to the session name.
    pub title: String,
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
            title: self.title.clone(),
        }
    }
}

/// Derive a short, sidebar-friendly session title from an initial prompt or
/// the first chat message: take the first non-blank line, collapse internal
/// whitespace, and truncate to a readable length. Returns an empty string
/// when the input has no usable text.
pub fn summarize_title(text: &str) -> String {
    const MAX_CHARS: usize = 80;
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let collapsed = line.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() > MAX_CHARS {
        let head: String = collapsed.chars().take(MAX_CHARS).collect();
        format!("{}…", head.trim_end())
    } else {
        collapsed
    }
}

#[cfg(test)]
mod tests {
    use super::summarize_title;

    #[test]
    fn takes_first_non_blank_line_and_collapses_whitespace() {
        assert_eq!(
            summarize_title("\n\n  Fix   the   login bug  \nmore details here"),
            "Fix the login bug"
        );
    }

    #[test]
    fn empty_input_yields_empty_title() {
        assert_eq!(summarize_title("   \n\t  "), "");
    }

    #[test]
    fn long_lines_are_truncated_with_ellipsis() {
        let title = summarize_title(&"a".repeat(100));
        assert!(title.ends_with('…'));
        // 80 chars of content + the ellipsis.
        assert_eq!(title.chars().count(), 81);
    }
}
