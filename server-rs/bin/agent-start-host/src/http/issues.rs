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

/// Cap the list so a repo with thousands of issues stays responsive.
const ISSUE_LIST_LIMIT: u32 = 50;

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub path: Option<String>,
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
    match git_ops::list_issues(&resolved, ISSUE_LIST_LIMIT) {
        Ok(issues) => Json(IssuesBody {
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
        Err(e) => err(StatusCode::BAD_GATEWAY, e.to_string()),
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
    let resolved = match assert_allowed_repo_path(&app, &path) {
        Ok(p) => p,
        Err(resp) => return *resp,
    };
    if !git_ops::is_git_repo(&resolved) {
        return err(StatusCode::BAD_REQUEST, "not a git repo");
    }
    match git_ops::view_issue(&resolved, number) {
        Ok(i) => Json(IssueDetailBody {
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
        Err(e) => err(StatusCode::BAD_GATEWAY, e.to_string()),
    }
}
