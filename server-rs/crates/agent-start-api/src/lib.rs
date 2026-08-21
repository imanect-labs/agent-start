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

/// Result of checking GitHub Releases for a newer `agent-start` build.
/// Best-effort: when the upstream check fails (offline, rate-limited) the
/// host returns `available: false` with `latest`/`html_url` left `None`
/// rather than erroring the request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCheckBody {
    /// The currently-running host version (`CARGO_PKG_VERSION`).
    pub current: String,
    /// Latest release tag from GitHub, if the check succeeded.
    pub latest: Option<String>,
    /// True when `latest` is strictly newer than `current`.
    pub available: bool,
    /// Browser URL of the latest release, for "view release" links.
    #[serde(rename = "htmlUrl", skip_serializing_if = "Option::is_none")]
    pub html_url: Option<String>,
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
    /// Launch mode: `"pty"` (default) or `"chat"`. The UI uses this to
    /// open a ChatTab instead of a terminal (#34).
    #[serde(default)]
    pub mode: String,
}

/// One selectable chat model exposed to the UI (#34, decision 8).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatModelInfo {
    pub id: String,
    pub label: String,
}

/// Chat-mode config surfaced to the UI: the model menu + default.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChatConfigBody {
    pub models: Vec<ChatModelInfo>,
    #[serde(rename = "defaultModel", skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigPaths {
    pub config: String,
    pub preferences: String,
    #[serde(rename = "worktreeRoot")]
    pub worktree_root: String,
}

/// Partial update body for `PUT /api/config`. Every field is optional; only
/// supplied keys override the persisted config.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ConfigPatch {
    pub roots: Option<Vec<String>>,
    #[serde(rename = "sessionPrefix")]
    pub session_prefix: Option<String>,
    pub shell: Option<String>,
    #[serde(rename = "showHidden")]
    pub show_hidden: Option<bool>,
    #[serde(rename = "gitOnly")]
    pub git_only: Option<bool>,
    #[serde(rename = "defaultCli")]
    pub default_cli: Option<String>,
    /// Replaces the full `clis` map when present.
    pub clis: Option<std::collections::BTreeMap<String, CliConfigPatch>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliConfigPatch {
    pub command: String,
    #[serde(
        rename = "skipPermissionsFlag",
        skip_serializing_if = "Option::is_none"
    )]
    pub skip_permissions_flag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigBody {
    pub clis: Vec<CliInfo>,
    #[serde(rename = "defaultCli")]
    pub default_cli: String,
    #[serde(rename = "sessionPrefix")]
    pub session_prefix: String,
    pub roots: Vec<String>,
    pub shell: String,
    #[serde(rename = "showHidden")]
    pub show_hidden: bool,
    #[serde(rename = "gitOnly")]
    pub git_only: bool,
    pub paths: ConfigPaths,
    #[serde(default)]
    pub chat: ChatConfigBody,
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
    #[serde(rename = "guiOpenInNewTab", default)]
    pub gui_open_in_new_tab: bool,
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
    #[serde(rename = "guiOpenInNewTab")]
    pub gui_open_in_new_tab: Option<bool>,
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
pub struct PendingProject {
    pub name: String,
    pub path: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectsBody {
    pub projects: Vec<Project>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending: Vec<PendingProject>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsEntry {
    pub name: String,
    pub path: String,
    #[serde(rename = "isDir")]
    pub is_dir: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsTreeBody {
    pub entries: Vec<FsEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsFileBody {
    pub content: String,
    pub sha: String,
    pub eol: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FsWriteRequest {
    pub path: String,
    pub content: String,
    #[serde(rename = "baseSha")]
    pub base_sha: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloneRequest {
    pub url: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportRequest {
    pub src: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectOpResponse {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub name: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    pub attached: bool,
    /// True for sessions that were rehydrated from disk after a host
    /// restart: their worktree still exists but no PTY is running.
    #[serde(default)]
    pub stopped: bool,
    pub path: String,
    pub cli: String,
    #[serde(rename = "worktreePath")]
    pub worktree_path: String,
    #[serde(rename = "origPath")]
    pub orig_path: String,
    /// Short human-readable title derived from the initial task. Empty
    /// when not yet known; the frontend falls back to the session name.
    #[serde(default)]
    pub title: String,
    /// Cluster node running this session. Empty for the local node, so
    /// single-host clients can keep ignoring the field entirely.
    #[serde(rename = "nodeId", default)]
    pub node_id: String,
    #[serde(rename = "nodeName", default)]
    pub node_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionsBody {
    pub sessions: Vec<Session>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// Optional initial prompt handed to the agent CLI as a positional
    /// argument (e.g. launching a session from a GitHub issue). Ignored
    /// for the bare-shell CLI.
    #[serde(default)]
    pub prompt: Option<String>,
    /// CPU (thousandths of a core) the session reserves on its node.
    #[serde(rename = "cpuMillis", default)]
    pub cpu_millis: Option<u32>,
    /// Memory (MiB) the session reserves on its node.
    #[serde(rename = "memMb", default)]
    pub mem_mb: Option<u32>,
    /// Minimum isolation the session needs: `process`, `container` or
    /// `microvm`. Nodes that cannot provide it are not considered.
    #[serde(default)]
    pub isolation: Option<String>,
    /// `key=value` node labels that must all match.
    #[serde(rename = "nodeSelector", default)]
    pub node_selector: Option<Vec<String>>,
    /// Pin the session to one node by id, bypassing scoring (the node
    /// still has to pass the hard filters).
    #[serde(rename = "nodeId", default)]
    pub node_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartSessionResponse {
    pub name: String,
    pub command: String,
    pub cli: String,
    pub cwd: String,
    #[serde(rename = "worktreePath")]
    pub worktree_path: Option<String>,
    #[serde(rename = "nodeId", default)]
    pub node_id: String,
    #[serde(rename = "nodeName", default)]
    pub node_name: String,
}

// ---- cluster ---------------------------------------------------------

/// One node in `GET /api/nodes`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeSummary {
    pub id: String,
    pub name: String,
    /// `ready` | `notready` | `cordoned` | `lost`.
    pub status: String,
    pub connected: bool,
    pub cordoned: bool,
    /// True for the node running inside the control-plane process.
    #[serde(rename = "isLocal")]
    pub is_local: bool,
    pub version: String,
    pub os: String,
    pub arch: String,
    /// Isolation profiles this node can provide.
    pub executors: Vec<String>,
    #[serde(rename = "capacityCpuMillis")]
    pub capacity_cpu_millis: u32,
    #[serde(rename = "capacityMemMb")]
    pub capacity_mem_mb: u32,
    #[serde(rename = "reservedCpuMillis")]
    pub reserved_cpu_millis: u32,
    #[serde(rename = "reservedMemMb")]
    pub reserved_mem_mb: u32,
    #[serde(rename = "maxSessions")]
    pub max_sessions: u32,
    #[serde(rename = "cpuUtil")]
    pub cpu_util: f32,
    #[serde(rename = "memUtil")]
    pub mem_util: f32,
    pub load1: f32,
    pub labels: Vec<NodeLabel>,
    pub sessions: Vec<String>,
    #[serde(rename = "cachedProjects")]
    pub cached_projects: usize,
    #[serde(rename = "lastHeartbeatMs")]
    pub last_heartbeat_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeLabel {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodesBody {
    pub nodes: Vec<NodeSummary>,
    /// True when the host runs a scheduler at all (`--role all` /
    /// `--role control`). The UI hides cluster affordances otherwise.
    pub clustered: bool,
}

/// Partial update for `PATCH /api/nodes/:id`.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct NodePatch {
    pub labels: Option<Vec<NodeLabel>>,
    #[serde(rename = "maxSessions")]
    pub max_sessions: Option<u32>,
    pub cordoned: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct JoinTokenRequest {
    /// Lifetime in seconds; defaults to one hour.
    #[serde(rename = "ttlSecs")]
    pub ttl_secs: Option<u64>,
    /// How many nodes may register with it; defaults to one.
    pub uses: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinTokenResponse {
    pub token: String,
    #[serde(rename = "expiresAtMs")]
    pub expires_at_ms: i64,
    pub uses: u32,
    /// Ready-to-paste command for the new node.
    pub command: String,
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

/// One GitHub issue row in the project's issue list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueSummary {
    pub number: u64,
    pub title: String,
    pub state: String,
    pub labels: Vec<String>,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
    pub author: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssuesBody {
    pub issues: Vec<IssueSummary>,
}

/// Full GitHub issue, including the markdown body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueDetail {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub state: String,
    pub labels: Vec<String>,
    pub url: String,
    pub author: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueDetailBody {
    pub issue: IssueDetail,
}

/// Shared body for `stage` / `unstage`: a repo path plus the files to act
/// on. An empty `files` list means "all".
#[derive(Debug, Clone, Deserialize)]
pub struct GitPathsRequest {
    pub path: String,
    #[serde(default)]
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitCommitRequest {
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitCommitResponse {
    pub sha: String,
    pub summary: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitDiscardRequest {
    pub path: String,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitBranch {
    pub name: String,
    pub current: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    #[serde(rename = "isRemote")]
    pub is_remote: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitBranchesBody {
    pub branches: Vec<GitBranch>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitCreateBranchRequest {
    pub path: String,
    pub name: String,
    #[serde(default)]
    pub base: Option<String>,
    #[serde(default)]
    pub checkout: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitCheckoutRequest {
    pub path: String,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitDeleteBranchRequest {
    pub path: String,
    pub name: String,
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitSyncRequest {
    pub path: String,
    #[serde(default)]
    pub remote: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(rename = "setUpstream", default)]
    pub set_upstream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitSyncResponse {
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitCommitNode {
    pub sha: String,
    #[serde(rename = "shortSha")]
    pub short_sha: String,
    pub parents: Vec<String>,
    pub subject: String,
    #[serde(rename = "authorName")]
    pub author_name: String,
    #[serde(rename = "authorEmail")]
    pub author_email: String,
    #[serde(rename = "authoredAt")]
    pub authored_at: i64,
    pub refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitLogBody {
    pub commits: Vec<GitCommitNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitTreeEntry {
    pub path: String,
    pub name: String,
    #[serde(rename = "isDir")]
    pub is_dir: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitTreeBody {
    pub entries: Vec<GitTreeEntry>,
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

/// One inline image attached to a chat user message (#34). `data` is the
/// raw base64 payload (no `data:` URL prefix).
#[derive(Debug, Clone, Deserialize)]
pub struct ChatImageInput {
    #[serde(rename = "mediaType")]
    pub media_type: String,
    pub data: String,
    /// Small downscaled thumbnail (data URL) for display in the transcript.
    /// Kept tiny so it can be persisted with the message without bloat.
    #[serde(default)]
    pub thumb: Option<String>,
}

/// WebSocket protocol messages from the browser — JSON over
/// `/ws/chat?session=<name>` (#34, decision 3).
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatClientMessage {
    /// Submit a user turn (text + optional inline images).
    UserMessage {
        #[serde(default)]
        text: String,
        #[serde(default)]
        images: Vec<ChatImageInput>,
    },
    /// Best-effort interrupt of the in-flight generation.
    Interrupt,
    /// Switch the active model (respawns the conversation with `--resume`).
    SetModel { model: String },
    /// Answer a pending AskUserQuestion / ExitPlanMode permission request
    /// (#95). `answers` carries the selected labels for AskUserQuestion
    /// (`{ "<question>": "<label>" | ["<label>", ...] }`); `message` is an
    /// optional rejection note when `allow` is false.
    PermissionResponse {
        request_id: String,
        allow: bool,
        #[serde(default)]
        answers: Option<serde_json::Value>,
        #[serde(default)]
        message: Option<String>,
    },
    /// Toggle the session's permission mode, e.g. enter/leave plan mode
    /// (#95). `mode = None` restores the default. Respawns with `--resume`.
    SetPermissionMode {
        #[serde(default)]
        mode: Option<String>,
    },
}
