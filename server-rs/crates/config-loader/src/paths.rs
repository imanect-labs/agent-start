//! On-disk path resolution.
//!
//! Every persisted file lives under `~/.agent-start/`. Individual paths
//! can be overridden via environment variables (`AGENT_START_HOME`,
//! `AGENT_START_CONFIG`, etc.) — useful for tests, ephemeral containers,
//! and multi-user setups.

use std::path::PathBuf;

pub(crate) fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

/// Root of all agent-start state on disk. Override with
/// `AGENT_START_HOME=<path>`; defaults to `~/.agent-start/`.
pub fn agent_start_home() -> PathBuf {
    if let Some(p) = std::env::var_os("AGENT_START_HOME") {
        return PathBuf::from(p);
    }
    home().join(".agent-start")
}

pub fn config_path() -> PathBuf {
    if let Some(p) = std::env::var_os("AGENT_START_CONFIG") {
        return PathBuf::from(p);
    }
    agent_start_home().join("config.json")
}

pub fn preferences_path() -> PathBuf {
    if let Some(p) = std::env::var_os("AGENT_START_PREFS") {
        return PathBuf::from(p);
    }
    agent_start_home().join("preferences.json")
}

pub fn worktree_root() -> PathBuf {
    if let Some(p) = std::env::var_os("AGENT_START_WORKTREE_ROOT") {
        return PathBuf::from(p);
    }
    agent_start_home().join("worktrees")
}

/// Default directory where cloned/imported projects live. Override with
/// `AGENT_START_PROJECTS=<path>`; defaults to `~/.agent-start/projects/`.
pub fn projects_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("AGENT_START_PROJECTS") {
        return PathBuf::from(p);
    }
    agent_start_home().join("projects")
}

pub fn host_state_dir() -> PathBuf {
    agent_start_home()
}

fn shellexpand_home(p: &str) -> String {
    if let Some(rest) = p.strip_prefix("~/") {
        home().join(rest).to_string_lossy().into_owned()
    } else if p == "~" {
        home().to_string_lossy().into_owned()
    } else {
        p.to_string()
    }
}

/// Expand a leading `~` / `~/...` in a `roots` entry to the user's home.
pub fn expand_root(root: &str) -> PathBuf {
    PathBuf::from(shellexpand_home(root))
}
