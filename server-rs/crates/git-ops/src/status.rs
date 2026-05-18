//! `git status --porcelain -z` parsing — drives the diff viewer's
//! left-hand file list.

use crate::exec::{run, run_raw, GitError};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct GitFile {
    pub path: String,
    /// Two-character porcelain XY code: e.g. " M", "M ", "??", "MM", "AD".
    pub xy: String,
    pub staged: bool,
    pub unstaged: bool,
    pub untracked: bool,
    /// For renames/copies — the original path the file was moved from.
    #[serde(rename = "origPath", skip_serializing_if = "Option::is_none")]
    pub orig_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GitStatus {
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub files: Vec<GitFile>,
}

pub fn git_status(repo: &Path) -> Result<GitStatus, GitError> {
    let mut out = GitStatus {
        branch: None,
        upstream: None,
        ahead: 0,
        behind: 0,
        files: Vec::new(),
    };

    if let Ok(s) = run(repo, &["rev-parse", "--abbrev-ref", "HEAD"]) {
        let trimmed = s.trim();
        if !trimmed.is_empty() && trimmed != "HEAD" {
            out.branch = Some(trimmed.to_string());
        }
    }
    if let Ok(s) = run(repo, &["rev-parse", "--abbrev-ref", "@{upstream}"]) {
        let trimmed = s.trim();
        if !trimmed.is_empty() {
            out.upstream = Some(trimmed.to_string());
        }
    }
    if let Some(upstream) = &out.upstream {
        if let Ok(s) = run(
            repo,
            &[
                "rev-list",
                "--left-right",
                "--count",
                &format!("{upstream}...HEAD"),
            ],
        ) {
            let mut parts = s.split_whitespace();
            if let (Some(b), Some(a)) = (parts.next(), parts.next()) {
                out.behind = b.parse().unwrap_or(0);
                out.ahead = a.parse().unwrap_or(0);
            }
        }
    }

    let raw = run_raw(repo, &["status", "--porcelain", "-z"])?;
    out.files = parse_porcelain(&raw);
    Ok(out)
}

fn parse_porcelain(raw: &[u8]) -> Vec<GitFile> {
    // Each NUL-separated record is either "XY <path>" or, for rename /
    // copy entries, "XY <new>\0<old>". We rebuild lines first, then
    // peek ahead for the rename target.
    let mut chunks: Vec<&[u8]> = raw.split(|b| *b == 0).filter(|s| !s.is_empty()).collect();
    let mut files = Vec::new();
    let mut i = 0;
    while i < chunks.len() {
        let chunk = chunks[i];
        if chunk.len() < 3 {
            i += 1;
            continue;
        }
        let xy = std::str::from_utf8(&chunk[..2]).unwrap_or("??").to_string();
        let path = String::from_utf8_lossy(&chunk[3..]).into_owned();
        let orig_path = if xy.starts_with('R') || xy.starts_with('C') {
            i += 1;
            chunks
                .get(i)
                .map(|b| String::from_utf8_lossy(b).into_owned())
        } else {
            None
        };
        let x = chunk[0] as char;
        let y = chunk[1] as char;
        let staged = x != ' ' && x != '?';
        let unstaged = y != ' ' && y != '?';
        let untracked = xy == "??";
        files.push(GitFile {
            path,
            xy,
            staged,
            unstaged,
            untracked,
            orig_path,
        });
        i += 1;
    }
    chunks.clear();
    files
}
