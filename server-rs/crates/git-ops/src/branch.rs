//! Branch listing and lifecycle: list / create / checkout / delete.

use crate::exec::{reject_flag_like, run, GitError};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct BranchInfo {
    pub name: String,
    pub current: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    #[serde(rename = "isRemote")]
    pub is_remote: bool,
}

/// Validate a branch *name* by delegating to git's own ref-name rules,
/// after rejecting flag-like names so they can't be parsed as options.
fn validate_branch_name(repo: &Path, name: &str) -> Result<(), GitError> {
    reject_flag_like(name)?;
    run(repo, &["check-ref-format", "--branch", name]).map(|_| ())
}

/// Local and remote-tracking branches, with upstream + ahead/behind for
/// locals. Skips the `origin/HEAD` symbolic pointer.
pub fn list_branches(repo: &Path) -> Result<Vec<BranchInfo>, GitError> {
    let fmt = "%(refname)%00%(refname:short)%00%(HEAD)%00%(upstream:short)%00%(upstream:track)";
    let out = run(
        repo,
        &[
            "for-each-ref",
            &format!("--format={fmt}"),
            "refs/heads",
            "refs/remotes",
        ],
    )?;
    let mut branches = Vec::new();
    for line in out.lines() {
        if line.is_empty() {
            continue;
        }
        let mut it = line.split('\u{0}');
        let full = it.next().unwrap_or("");
        let short = it.next().unwrap_or("");
        let head = it.next().unwrap_or("");
        let upstream = it.next().unwrap_or("");
        let track = it.next().unwrap_or("");
        if short.is_empty() || short.ends_with("/HEAD") {
            continue;
        }
        let (ahead, behind) = parse_track(track);
        branches.push(BranchInfo {
            name: short.to_string(),
            current: head == "*",
            upstream: (!upstream.is_empty()).then(|| upstream.to_string()),
            ahead,
            behind,
            is_remote: full.starts_with("refs/remotes/"),
        });
    }
    Ok(branches)
}

/// Parse git's `%(upstream:track)` field, e.g. "[ahead 1, behind 2]".
fn parse_track(s: &str) -> (u32, u32) {
    let mut ahead = 0;
    let mut behind = 0;
    let inner = s.trim().trim_start_matches('[').trim_end_matches(']');
    for part in inner.split(',') {
        let part = part.trim();
        if let Some(n) = part.strip_prefix("ahead ") {
            ahead = n.trim().parse().unwrap_or(0);
        } else if let Some(n) = part.strip_prefix("behind ") {
            behind = n.trim().parse().unwrap_or(0);
        }
    }
    (ahead, behind)
}

/// Create a branch (does not switch to it).
pub fn create_branch(repo: &Path, name: &str, base: Option<&str>) -> Result<(), GitError> {
    validate_branch_name(repo, name)?;
    match base {
        Some(b) => {
            reject_flag_like(b)?;
            run(repo, &["branch", name, b])?;
        }
        None => {
            run(repo, &["branch", name])?;
        }
    }
    Ok(())
}

/// Switch the working tree to an existing branch.
pub fn checkout_branch(repo: &Path, name: &str) -> Result<(), GitError> {
    reject_flag_like(name)?;
    run(repo, &["switch", name])?;
    Ok(())
}

/// Create a branch and switch to it (`git switch -c`).
pub fn create_and_checkout(repo: &Path, name: &str, base: Option<&str>) -> Result<(), GitError> {
    validate_branch_name(repo, name)?;
    match base {
        Some(b) => {
            reject_flag_like(b)?;
            run(repo, &["switch", "-c", name, b])?;
        }
        None => {
            run(repo, &["switch", "-c", name])?;
        }
    }
    Ok(())
}

/// Delete a branch. `force` upgrades `-d` to `-D` (delete even if
/// unmerged) and must be opt-in.
pub fn delete_branch(repo: &Path, name: &str, force: bool) -> Result<(), GitError> {
    reject_flag_like(name)?;
    let flag = if force { "-D" } else { "-d" };
    run(repo, &["branch", flag, name])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;
    use tempfile::TempDir;

    fn git(repo: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?} failed");
    }

    /// Temp repo on branch `main` with one commit.
    fn repo() -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        git(p, &["init", "-q", "-b", "main"]);
        git(p, &["config", "user.email", "t@example.com"]);
        git(p, &["config", "user.name", "Test"]);
        fs::write(p.join("a.txt"), "one\n").unwrap();
        git(p, &["add", "a.txt"]);
        git(p, &["commit", "-q", "-m", "init"]);
        dir
    }

    #[test]
    fn create_and_list() {
        let dir = repo();
        let p = dir.path();
        create_branch(p, "feature", None).unwrap();
        let bs = list_branches(p).unwrap();
        let feat = bs.iter().find(|b| b.name == "feature").unwrap();
        assert!(!feat.current && !feat.is_remote);
        let main = bs.iter().find(|b| b.name == "main").unwrap();
        assert!(main.current);
    }

    #[test]
    fn checkout_switches_current() {
        let dir = repo();
        let p = dir.path();
        create_and_checkout(p, "feature", None).unwrap();
        let bs = list_branches(p).unwrap();
        assert!(bs.iter().find(|b| b.name == "feature").unwrap().current);
        assert!(!bs.iter().find(|b| b.name == "main").unwrap().current);
    }

    #[test]
    fn delete_unmerged_requires_force() {
        let dir = repo();
        let p = dir.path();
        create_and_checkout(p, "feature", None).unwrap();
        fs::write(p.join("b.txt"), "x\n").unwrap();
        git(p, &["add", "b.txt"]);
        git(p, &["commit", "-q", "-m", "feat"]);
        checkout_branch(p, "main").unwrap();
        assert!(delete_branch(p, "feature", false).is_err());
        assert!(delete_branch(p, "feature", true).is_ok());
    }

    #[test]
    fn invalid_names_rejected() {
        let dir = repo();
        let p = dir.path();
        assert!(create_branch(p, "--force", None).is_err());
        assert!(create_branch(p, "bad..name", None).is_err());
        assert!(create_branch(p, "", None).is_err());
    }
}
