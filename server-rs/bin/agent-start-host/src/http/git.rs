//! `/api/git/status` and `/api/git/diff` — backs the right-hand
//! Files/diff pane in the desktop UI.

use super::err;
use agent_start_api::{GitDiffBody, GitFile as ApiGitFile, GitStatusBody};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use std::path::{Path, PathBuf};

use crate::app::Shared;

#[derive(Debug, Deserialize)]
pub struct StatusQuery {
    pub path: Option<String>,
}

pub async fn git_status(State(app): State<Shared>, Query(q): Query<StatusQuery>) -> Response {
    let Some(path) = q.path else {
        return err(StatusCode::BAD_REQUEST, "path is required");
    };
    let resolved = match assert_allowed_repo_path(&app, &path) {
        Ok(p) => p,
        Err(resp) => return *resp,
    };
    if !git_ops::is_git_repo(&resolved) {
        return Json(GitStatusBody {
            is_git: false,
            branch: None,
            upstream: None,
            ahead: 0,
            behind: 0,
            files: vec![],
        })
        .into_response();
    }
    match git_ops::git_status(&resolved) {
        Ok(s) => Json(GitStatusBody {
            is_git: true,
            branch: s.branch,
            upstream: s.upstream,
            ahead: s.ahead,
            behind: s.behind,
            files: s
                .files
                .into_iter()
                .map(|f| ApiGitFile {
                    path: f.path,
                    xy: f.xy,
                    staged: f.staged,
                    unstaged: f.unstaged,
                    untracked: f.untracked,
                    orig_path: f.orig_path,
                })
                .collect(),
        })
        .into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[derive(Debug, Deserialize)]
pub struct DiffQuery {
    pub path: Option<String>,
    pub file: Option<String>,
    pub mode: Option<String>,
}

pub async fn git_diff(State(app): State<Shared>, Query(q): Query<DiffQuery>) -> Response {
    let Some(path) = q.path else {
        return err(StatusCode::BAD_REQUEST, "path is required");
    };
    let Some(file) = q.file else {
        return err(StatusCode::BAD_REQUEST, "file is required");
    };
    if file.starts_with('/') || file.split('/').any(|seg| seg == "..") {
        return err(StatusCode::BAD_REQUEST, "invalid file path");
    }
    let resolved = match assert_allowed_repo_path(&app, &path) {
        Ok(p) => p,
        Err(resp) => return *resp,
    };
    if !git_ops::is_git_repo(&resolved) {
        return err(StatusCode::BAD_REQUEST, "not a git repo");
    }
    let mode = git_ops::DiffMode::parse(q.mode.as_deref().unwrap_or("worktree"));
    match git_ops::git_diff(&resolved, &file, mode) {
        Ok(d) => Json(GitDiffBody {
            diff: d.diff,
            truncated: d.truncated,
            is_untracked: d.is_untracked,
        })
        .into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

/// Mirrors `lib/git.ts::assertAllowedRepoPath`: the path must exist
/// and be a directory, AND it must be either under a configured
/// `roots` entry, under the worktree root, or the cwd/worktreePath/origPath
/// of a live session (covers worktrees whose root was reconfigured
/// after the session started).
pub(crate) fn assert_allowed_repo_path(app: &Shared, cwd: &str) -> Result<PathBuf, Box<Response>> {
    let resolved = std::fs::canonicalize(cwd).unwrap_or_else(|_| PathBuf::from(cwd));
    let cfg = config_loader::load_config()
        .map_err(|e| Box::new(err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())))?;
    let inside_roots = config_loader::is_path_under_roots(&cfg, &resolved);
    let inside_worktrees = under(&resolved, &config_loader::worktree_root());
    let matched_session = app.sessions.read().values().any(|d| {
        same_path(&resolved, &d.cwd)
            || same_path(&resolved, &d.worktree_path)
            || same_path(&resolved, &d.orig_path)
    });
    if !inside_roots && !inside_worktrees && !matched_session {
        return Err(Box::new(err(
            StatusCode::BAD_REQUEST,
            "path is outside configured roots",
        )));
    }
    match std::fs::metadata(&resolved) {
        Ok(m) if m.is_dir() => Ok(resolved),
        _ => Err(Box::new(err(
            StatusCode::BAD_REQUEST,
            "path is not a directory",
        ))),
    }
}

fn under(p: &Path, root: &Path) -> bool {
    let root_canon = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    p == root_canon || p.starts_with(&root_canon)
}

fn same_path(a: &Path, b: &str) -> bool {
    if b.is_empty() {
        return false;
    }
    let bp = std::fs::canonicalize(b).unwrap_or_else(|_| PathBuf::from(b));
    a == bp
}
