//! Shared request / response types for the agent-start host server.
//!
//! Field names mirror the Node/Next.js implementation so the existing
//! Next.js UI can call the Rust host through `next.config.mjs` rewrites
//! without source changes.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorBody {
    pub error: String,
}

impl ErrorBody {
    pub fn new(msg: impl Into<String>) -> Self {
        Self { error: msg.into() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthBody {
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionBody {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliInfo {
    pub key: String,
    pub label: String,
    pub command: String,
    #[serde(rename = "hasSkipFlag")]
    pub has_skip_flag: bool,
    #[serde(rename = "skipFlag")]
    pub skip_flag: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigBody {
    pub clis: Vec<CliInfo>,
    #[serde(rename = "defaultCli")]
    pub default_cli: String,
    #[serde(rename = "sessionPrefix")]
    pub session_prefix: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Preferences {
    pub cli: String,
    #[serde(rename = "skipPermissions")]
    pub skip_permissions: bool,
    #[serde(rename = "extraArgs")]
    pub extra_args: String,
    #[serde(rename = "createWorktree", default)]
    pub create_worktree: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreferencesBody {
    pub preferences: Preferences,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct PreferencesPatch {
    pub cli: Option<String>,
    #[serde(rename = "skipPermissions")]
    pub skip_permissions: Option<bool>,
    #[serde(rename = "extraArgs")]
    pub extra_args: Option<String>,
    #[serde(rename = "createWorktree")]
    pub create_worktree: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub path: String,
    pub root: String,
    #[serde(rename = "mtimeMs")]
    pub mtime_ms: u64,
    #[serde(rename = "isGit")]
    pub is_git: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectsBody {
    pub projects: Vec<Project>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub name: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    pub attached: bool,
    pub path: String,
    pub cli: String,
    #[serde(rename = "worktreePath")]
    pub worktree_path: String,
    #[serde(rename = "origPath")]
    pub orig_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionsBody {
    pub sessions: Vec<Session>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StartSessionRequest {
    #[serde(rename = "projectPath")]
    pub project_path: String,
    pub cli: Option<String>,
    #[serde(rename = "skipPermissions")]
    pub skip_permissions: Option<bool>,
    #[serde(rename = "extraArgs")]
    pub extra_args: Option<String>,
    #[serde(rename = "createWorktree")]
    pub create_worktree: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartSessionResponse {
    pub name: String,
    pub command: String,
    pub cli: String,
    pub cwd: String,
    #[serde(rename = "worktreePath")]
    pub worktree_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteSessionResponse {
    pub ok: bool,
    #[serde(rename = "worktreeRemoved")]
    pub worktree_removed: bool,
    #[serde(rename = "worktreeError", skip_serializing_if = "Option::is_none")]
    pub worktree_error: Option<String>,
}

/// Single window (PTY) inside a session — mirrors the per-session tab
/// model the desktop UI uses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowInfo {
    pub index: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowsBody {
    pub windows: Vec<WindowInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewWindowResponse {
    pub index: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitFile {
    pub path: String,
    pub xy: String,
    pub staged: bool,
    pub unstaged: bool,
    pub untracked: bool,
    #[serde(rename = "origPath", skip_serializing_if = "Option::is_none")]
    pub orig_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitStatusBody {
    #[serde(rename = "isGit")]
    pub is_git: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream: Option<String>,
    #[serde(default)]
    pub ahead: u32,
    #[serde(default)]
    pub behind: u32,
    #[serde(default)]
    pub files: Vec<GitFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitDiffBody {
    pub diff: String,
    pub truncated: bool,
    #[serde(rename = "isUntracked")]
    pub is_untracked: bool,
}

/// WebSocket protocol messages — JSON over `/ws/terminal?session=<name>`.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ClientMessage {
    Input {
        data: String,
    },
    Resize {
        cols: u16,
        rows: u16,
    },
    /// Legacy tmux scroll request. We accept the message but the new PTY
    /// manager performs scrollback in the client (xterm.js), so we just
    /// no-op rather than break old clients during the migration.
    Scroll {
        #[allow(dead_code)]
        direction: i32,
        #[allow(dead_code)]
        count: Option<u32>,
    },
}
