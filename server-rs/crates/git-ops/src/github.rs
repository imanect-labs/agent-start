//! Thin wrappers around the `gh` CLI for reading GitHub issues.
//!
//! We shell out to `gh` rather than calling the REST API directly so we
//! reuse the user's existing `gh auth` credentials — no token plumbing.
//! `gh` resolves the target repository from the `origin` remote of the
//! directory we run it in, so callers just pass the repo path.

use crate::exec::GitError;
use serde::Deserialize;
use std::path::Path;
use std::process::Command;

/// One row of `gh issue list` — the lightweight shape shown in the list view.
#[derive(Debug, Clone)]
pub struct IssueSummary {
    pub number: u64,
    pub title: String,
    pub state: String,
    pub labels: Vec<String>,
    pub updated_at: String,
    pub author: String,
}

/// Full `gh issue view` payload, including the markdown body.
#[derive(Debug, Clone)]
pub struct IssueDetail {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub state: String,
    pub labels: Vec<String>,
    pub url: String,
    pub author: String,
}

// --- raw `gh` JSON shapes ---------------------------------------------------

#[derive(Debug, Deserialize)]
struct RawAuthor {
    #[serde(default)]
    login: String,
}

#[derive(Debug, Deserialize)]
struct RawLabel {
    #[serde(default)]
    name: String,
}

#[derive(Debug, Deserialize)]
struct RawList {
    number: u64,
    #[serde(default)]
    title: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    labels: Vec<RawLabel>,
    #[serde(rename = "updatedAt", default)]
    updated_at: String,
    #[serde(default)]
    author: Option<RawAuthor>,
}

#[derive(Debug, Deserialize)]
struct RawDetail {
    number: u64,
    #[serde(default)]
    title: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    labels: Vec<RawLabel>,
    #[serde(default)]
    url: String,
    #[serde(default)]
    author: Option<RawAuthor>,
}

fn run_gh(repo: &Path, args: &[&str]) -> Result<Vec<u8>, GitError> {
    let mut cmd = Command::new("gh");
    cmd.current_dir(repo);
    for a in args {
        cmd.arg(a);
    }
    let output = cmd.output()?;
    if !output.status.success() {
        return Err(GitError::Failed {
            cmd: format!("gh {}", args.join(" ")),
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    Ok(output.stdout)
}

fn label_names(labels: Vec<RawLabel>) -> Vec<String> {
    labels
        .into_iter()
        .map(|l| l.name)
        .filter(|n| !n.is_empty())
        .collect()
}

fn login(author: Option<RawAuthor>) -> String {
    author.map(|a| a.login).unwrap_or_default()
}

/// `gh issue list` for the repo at `repo`, newest first, capped at `limit`.
///
/// `gh` has no cursor/page model, so the UI paginates by re-requesting a
/// larger `limit`. An optional `search` string is forwarded verbatim to
/// `gh issue list --search` (a GitHub issue search query).
pub fn list_issues(repo: &Path, limit: u32, search: Option<&str>) -> Result<Vec<IssueSummary>, GitError> {
    let limit = limit.to_string();
    let mut args: Vec<&str> = vec![
        "issue",
        "list",
        "--json",
        "number,title,state,labels,updatedAt,author",
        "--limit",
        &limit,
    ];
    let search = search.map(str::trim).filter(|s| !s.is_empty());
    if let Some(s) = search {
        args.push("--search");
        args.push(s);
    }
    let out = run_gh(repo, &args)?;
    let raw: Vec<RawList> = serde_json::from_slice(&out).map_err(|e| GitError::Failed {
        cmd: "gh issue list (parse)".into(),
        code: None,
        stderr: e.to_string(),
    })?;
    Ok(raw
        .into_iter()
        .map(|r| IssueSummary {
            number: r.number,
            title: r.title,
            state: r.state,
            labels: label_names(r.labels),
            updated_at: r.updated_at,
            author: login(r.author),
        })
        .collect())
}

/// `gh issue view <number>` for the repo at `repo`, including the body.
pub fn view_issue(repo: &Path, number: u64) -> Result<IssueDetail, GitError> {
    let num = number.to_string();
    let out = run_gh(
        repo,
        &[
            "issue",
            "view",
            &num,
            "--json",
            "number,title,body,state,labels,url,author",
        ],
    )?;
    let r: RawDetail = serde_json::from_slice(&out).map_err(|e| GitError::Failed {
        cmd: "gh issue view (parse)".into(),
        code: None,
        stderr: e.to_string(),
    })?;
    Ok(IssueDetail {
        number: r.number,
        title: r.title,
        body: r.body,
        state: r.state,
        labels: label_names(r.labels),
        url: r.url,
        author: login(r.author),
    })
}
