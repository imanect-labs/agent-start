//! Mutating git endpoints — staging, commit, discard (and, in later
//! slices, branches and remote sync). Every handler routes its `path`
//! through [`assert_allowed_repo_path`](super::git::assert_allowed_repo_path)
//! so it can only operate on repos under the configured roots.

use super::err;
use super::git::assert_allowed_repo_path;
use agent_start_api::{
    GitBranch, GitBranchesBody, GitCheckoutRequest, GitCommitNode, GitCommitRequest,
    GitCommitResponse, GitCreateBranchRequest, GitDeleteBranchRequest, GitDiscardRequest,
    GitLogBody, GitPathsRequest, GitSyncRequest, GitSyncResponse, GitTreeBody, GitTreeEntry,
};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use std::path::PathBuf;

use crate::app::Shared;

/// Map a `git_ops::GitError` to an HTTP response. A failed git invocation
/// is almost always user/repo state (bad branch name, conflict, nothing
/// to commit), so it maps to 400 rather than 500.
pub(crate) fn git_err(e: git_ops::GitError) -> Response {
    match e {
        git_ops::GitError::Io(_) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        git_ops::GitError::Failed { .. } => err(StatusCode::BAD_REQUEST, e.to_string()),
    }
}

/// Resolve and validate a repo path, returning early on failure. The
/// error is boxed (like `assert_allowed_repo_path`) to keep the `Result`
/// small.
fn repo(app: &Shared, path: &str) -> Result<PathBuf, Box<Response>> {
    let resolved = assert_allowed_repo_path(app, path)?;
    if !git_ops::is_git_repo(&resolved) {
        return Err(Box::new(err(StatusCode::BAD_REQUEST, "not a git repo")));
    }
    Ok(resolved)
}

fn ok() -> Response {
    Json(serde_json::json!({"ok": true})).into_response()
}

pub async fn git_stage(State(app): State<Shared>, Json(req): Json<GitPathsRequest>) -> Response {
    let resolved = match repo(&app, &req.path) {
        Ok(p) => p,
        Err(r) => return *r,
    };
    match git_ops::stage(&resolved, &req.files) {
        Ok(()) => ok(),
        Err(e) => git_err(e),
    }
}

pub async fn git_unstage(State(app): State<Shared>, Json(req): Json<GitPathsRequest>) -> Response {
    let resolved = match repo(&app, &req.path) {
        Ok(p) => p,
        Err(r) => return *r,
    };
    match git_ops::unstage(&resolved, &req.files) {
        Ok(()) => ok(),
        Err(e) => git_err(e),
    }
}

pub async fn git_commit(State(app): State<Shared>, Json(req): Json<GitCommitRequest>) -> Response {
    let resolved = match repo(&app, &req.path) {
        Ok(p) => p,
        Err(r) => return *r,
    };
    match git_ops::commit(&resolved, &req.message) {
        Ok(r) => Json(GitCommitResponse {
            sha: r.sha,
            summary: r.summary,
        })
        .into_response(),
        Err(e) => git_err(e),
    }
}

pub async fn git_discard(
    State(app): State<Shared>,
    Json(req): Json<GitDiscardRequest>,
) -> Response {
    let resolved = match repo(&app, &req.path) {
        Ok(p) => p,
        Err(r) => return *r,
    };
    match git_ops::discard(&resolved, &req.files) {
        Ok(()) => ok(),
        Err(e) => git_err(e),
    }
}

#[derive(Debug, Deserialize)]
pub struct PathQuery {
    pub path: Option<String>,
}

pub async fn git_branches(State(app): State<Shared>, Query(q): Query<PathQuery>) -> Response {
    let Some(path) = q.path else {
        return err(StatusCode::BAD_REQUEST, "path is required");
    };
    let resolved = match repo(&app, &path) {
        Ok(p) => p,
        Err(r) => return *r,
    };
    match git_ops::list_branches(&resolved) {
        Ok(list) => Json(GitBranchesBody {
            branches: list
                .into_iter()
                .map(|b| GitBranch {
                    name: b.name,
                    current: b.current,
                    upstream: b.upstream,
                    ahead: b.ahead,
                    behind: b.behind,
                    is_remote: b.is_remote,
                })
                .collect(),
        })
        .into_response(),
        Err(e) => git_err(e),
    }
}

pub async fn git_create_branch(
    State(app): State<Shared>,
    Json(req): Json<GitCreateBranchRequest>,
) -> Response {
    let resolved = match repo(&app, &req.path) {
        Ok(p) => p,
        Err(r) => return *r,
    };
    let base = req.base.as_deref();
    let res = if req.checkout {
        git_ops::create_and_checkout(&resolved, &req.name, base)
    } else {
        git_ops::create_branch(&resolved, &req.name, base)
    };
    match res {
        Ok(()) => ok(),
        Err(e) => git_err(e),
    }
}

pub async fn git_checkout(
    State(app): State<Shared>,
    Json(req): Json<GitCheckoutRequest>,
) -> Response {
    let resolved = match repo(&app, &req.path) {
        Ok(p) => p,
        Err(r) => return *r,
    };
    match git_ops::checkout_branch(&resolved, &req.name) {
        Ok(()) => ok(),
        Err(e) => git_err(e),
    }
}

pub async fn git_delete_branch(
    State(app): State<Shared>,
    Json(req): Json<GitDeleteBranchRequest>,
) -> Response {
    let resolved = match repo(&app, &req.path) {
        Ok(p) => p,
        Err(r) => return *r,
    };
    match git_ops::delete_branch(&resolved, &req.name, req.force) {
        Ok(()) => ok(),
        Err(e) => git_err(e),
    }
}

/// Run a network-bound sync op (fetch/pull/push) on the blocking pool so
/// it can't stall the async runtime if git negotiates with a remote.
async fn run_sync<F>(app: Shared, req: GitSyncRequest, op: F) -> Response
where
    F: FnOnce(
            PathBuf,
            Option<String>,
            Option<String>,
            bool,
        ) -> Result<git_ops::SyncResult, git_ops::GitError>
        + Send
        + 'static,
{
    let resolved = match repo(&app, &req.path) {
        Ok(p) => p,
        Err(r) => return *r,
    };
    let GitSyncRequest {
        remote,
        branch,
        set_upstream,
        ..
    } = req;
    let res = tokio::task::spawn_blocking(move || op(resolved, remote, branch, set_upstream)).await;
    match res {
        Ok(Ok(r)) => Json(GitSyncResponse {
            stdout: r.stdout,
            stderr: r.stderr,
        })
        .into_response(),
        Ok(Err(e)) => git_err(e),
        Err(e) => err(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("join error: {e}"),
        ),
    }
}

pub async fn git_fetch(State(app): State<Shared>, Json(req): Json<GitSyncRequest>) -> Response {
    run_sync(app, req, |p, remote, _branch, _up| {
        git_ops::fetch(&p, remote.as_deref())
    })
    .await
}

pub async fn git_pull(State(app): State<Shared>, Json(req): Json<GitSyncRequest>) -> Response {
    run_sync(app, req, |p, remote, branch, _up| {
        git_ops::pull(&p, remote.as_deref(), branch.as_deref())
    })
    .await
}

pub async fn git_push(State(app): State<Shared>, Json(req): Json<GitSyncRequest>) -> Response {
    run_sync(app, req, |p, remote, branch, up| {
        git_ops::push(&p, remote.as_deref(), branch.as_deref(), up)
    })
    .await
}

#[derive(Debug, Deserialize)]
pub struct LogQuery {
    pub path: Option<String>,
    pub limit: Option<usize>,
}

pub async fn git_log(State(app): State<Shared>, Query(q): Query<LogQuery>) -> Response {
    let Some(path) = q.path else {
        return err(StatusCode::BAD_REQUEST, "path is required");
    };
    let resolved = match repo(&app, &path) {
        Ok(p) => p,
        Err(r) => return *r,
    };
    match git_ops::log_graph(&resolved, q.limit.unwrap_or(200)) {
        Ok(commits) => Json(GitLogBody {
            commits: commits
                .into_iter()
                .map(|c| GitCommitNode {
                    sha: c.sha,
                    short_sha: c.short_sha,
                    parents: c.parents,
                    subject: c.subject,
                    author_name: c.author_name,
                    author_email: c.author_email,
                    authored_at: c.authored_at,
                    refs: c.refs,
                })
                .collect(),
        })
        .into_response(),
        Err(e) => git_err(e),
    }
}

#[derive(Debug, Deserialize)]
pub struct TreeQuery {
    pub path: Option<String>,
    pub subdir: Option<String>,
}

pub async fn git_tree(State(app): State<Shared>, Query(q): Query<TreeQuery>) -> Response {
    let Some(path) = q.path else {
        return err(StatusCode::BAD_REQUEST, "path is required");
    };
    let resolved = match repo(&app, &path) {
        Ok(p) => p,
        Err(r) => return *r,
    };
    match git_ops::file_tree(&resolved, q.subdir.as_deref()) {
        Ok(entries) => Json(GitTreeBody {
            entries: entries
                .into_iter()
                .map(|e| GitTreeEntry {
                    path: e.path,
                    name: e.name,
                    is_dir: e.is_dir,
                })
                .collect(),
        })
        .into_response(),
        Err(e) => git_err(e),
    }
}
