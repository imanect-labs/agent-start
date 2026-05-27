//! Commit-graph data for the log view.
//!
//! We emit structured commits (sha, parents, decorations) and let the
//! frontend compute the visual lanes from `parents` — reproducing the
//! ASCII `--graph` art server-side would be far less useful to render.

use crate::exec::{run_raw, GitError};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct CommitNode {
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
    /// Branch/tag decorations on this commit (HEAD arrow stripped).
    pub refs: Vec<String>,
}

/// Up to `limit` commits across all refs, newest first. `limit` is clamped
/// to [1, 1000] to bound memory.
pub fn log_graph(repo: &Path, limit: usize) -> Result<Vec<CommitNode>, GitError> {
    let limit = limit.clamp(1, 1000).to_string();
    // %x1f (Unit Separator) delimits fields; -z makes NUL delimit commits,
    // so neither collides with subject text.
    let fmt = "--pretty=format:%H%x1f%h%x1f%P%x1f%an%x1f%ae%x1f%at%x1f%D%x1f%s";
    let raw = run_raw(
        repo,
        &["log", "--all", "--date-order", "-z", fmt, "-n", &limit],
    )?;

    let mut out = Vec::new();
    for rec in raw.split(|b| *b == 0) {
        let s = String::from_utf8_lossy(rec);
        let s = s.trim_start_matches('\n');
        if s.is_empty() {
            continue;
        }
        let f: Vec<&str> = s.splitn(8, '\u{1f}').collect();
        if f.len() < 8 {
            continue;
        }
        out.push(CommitNode {
            sha: f[0].to_string(),
            short_sha: f[1].to_string(),
            parents: f[2].split_whitespace().map(str::to_string).collect(),
            author_name: f[3].to_string(),
            author_email: f[4].to_string(),
            authored_at: f[5].trim().parse().unwrap_or(0),
            refs: parse_refs(f[6]),
            subject: f[7].to_string(),
        });
    }
    Ok(out)
}

fn parse_refs(d: &str) -> Vec<String> {
    d.split(", ")
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .map(|r| r.strip_prefix("HEAD -> ").unwrap_or(r).to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;
    use tempfile::TempDir;

    fn git(repo: &Path, args: &[&str]) {
        assert!(Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .status()
            .unwrap()
            .success());
    }

    fn repo() -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        git(p, &["init", "-q", "-b", "main"]);
        git(p, &["config", "user.email", "t@example.com"]);
        git(p, &["config", "user.name", "Test"]);
        dir
    }

    #[test]
    fn linear_history_links_parents() {
        let dir = repo();
        let p = dir.path();
        fs::write(p.join("a.txt"), "one\n").unwrap();
        git(p, &["add", "a.txt"]);
        git(p, &["commit", "-q", "-m", "first"]);
        fs::write(p.join("a.txt"), "two\n").unwrap();
        git(p, &["commit", "-aqm", "second"]);

        let nodes = log_graph(p, 100).unwrap();
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].subject, "second");
        assert_eq!(nodes[1].subject, "first");
        // Newest commit's parent is the older commit.
        assert_eq!(nodes[0].parents, vec![nodes[1].sha.clone()]);
        // The tip carries the `main` decoration.
        assert!(nodes[0].refs.iter().any(|r| r == "main"));
    }
}
