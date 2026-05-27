//! `/api/projects/issues` and `/api/projects/issue` — read the remote
//! GitHub issues of a project so the UI can list them and launch a
//! session against a chosen one. Backed by the `gh` CLI (see
//! `git_ops::github`).

use super::err;
use super::git::assert_allowed_repo_path;
use agent_start_api::{
    IssueDetail as ApiIssueDetail, IssueDetailBody, IssueSummary as ApiIssueSummary, IssuesBody,
};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;

use crate::app::Shared;

/// Default page size, and the hard cap a client may request — keeps a
/// repo with thousands of issues from spawning an unbounded `gh` fetch.
const ISSUE_LIST_DEFAULT: u32 = 30;
const ISSUE_LIST_MAX: u32 = 300;

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub path: Option<String>,
    pub limit: Option<u32>,
    pub search: Option<String>,
}

pub async fn list_issues(State(app): State<Shared>, Query(q): Query<ListQuery>) -> Response {
    let Some(path) = q.path else {
        return err(StatusCode::BAD_REQUEST, "path is required");
    };
    let resolved = match assert_allowed_repo_path(&app, &path) {
        Ok(p) => p,
        Err(resp) => return *resp,
    };
    if !git_ops::is_git_repo(&resolved) {
        return err(StatusCode::BAD_REQUEST, "not a git repo");
    }
    let limit = q
        .limit
        .unwrap_or(ISSUE_LIST_DEFAULT)
        .clamp(1, ISSUE_LIST_MAX);
    // `gh` shells out and hits the network — offload it so it doesn't block
    // a Tokio worker thread (mirrors the git_write sync handlers).
    let search = q.search;
    let res = tokio::task::spawn_blocking(move || {
        git_ops::list_issues(&resolved, limit, search.as_deref())
    })
    .await;
    match res {
        Ok(Ok(issues)) => Json(IssuesBody {
            issues: issues
                .into_iter()
                .map(|i| ApiIssueSummary {
                    number: i.number,
                    title: i.title,
                    state: i.state,
                    labels: i.labels,
                    updated_at: i.updated_at,
                    author: i.author,
                })
                .collect(),
        })
        .into_response(),
        Ok(Err(e)) => err(StatusCode::BAD_GATEWAY, e.to_string()),
        Err(e) => err(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("join error: {e}"),
        ),
    }
}

#[derive(Debug, Deserialize)]
pub struct ViewQuery {
    pub path: Option<String>,
    pub number: Option<u64>,
}

pub async fn view_issue(State(app): State<Shared>, Query(q): Query<ViewQuery>) -> Response {
    let Some(path) = q.path else {
        return err(StatusCode::BAD_REQUEST, "path is required");
    };
    let Some(number) = q.number else {
        return err(StatusCode::BAD_REQUEST, "number is required");
    };
    if number < 1 {
        return err(StatusCode::BAD_REQUEST, "number must be >= 1");
    }
    let resolved = match assert_allowed_repo_path(&app, &path) {
        Ok(p) => p,
        Err(resp) => return *resp,
    };
    if !git_ops::is_git_repo(&resolved) {
        return err(StatusCode::BAD_REQUEST, "not a git repo");
    }
    let res = tokio::task::spawn_blocking(move || git_ops::view_issue(&resolved, number)).await;
    match res {
        Ok(Ok(i)) => Json(IssueDetailBody {
            issue: ApiIssueDetail {
                number: i.number,
                title: i.title,
                body: i.body,
                state: i.state,
                labels: i.labels,
                url: i.url,
                author: i.author,
            },
        })
        .into_response(),
        Ok(Err(e)) => err(StatusCode::BAD_GATEWAY, e.to_string()),
        Err(e) => err(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("join error: {e}"),
        ),
    }
}
