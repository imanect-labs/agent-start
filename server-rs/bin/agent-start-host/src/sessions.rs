//! Live session metadata held in memory alongside SQLite persistence.

use agent_start_api::Session;
use state::SessionRow;

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
    pub fn from_row(row: &SessionRow) -> Self {
        Self {
            name: row.name.clone(),
            created_at_ms: row.created_at_ms,
            cli: row.cli.clone(),
            cwd: row.cwd.clone(),
            worktree_path: row.worktree_path.clone(),
            orig_path: row.orig_path.clone(),
        }
    }

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
