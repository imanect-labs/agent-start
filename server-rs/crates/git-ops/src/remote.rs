//! Remote sync: fetch / pull / push.
//!
//! These are network-bound and may block on credential negotiation, so
//! callers MUST invoke them under `tokio::task::spawn_blocking`. They use
//! [`run_full`](crate::exec) to surface git's progress/summary (written to
//! stderr even on success) and inherit its `GIT_TERMINAL_PROMPT=0` so a
//! missing credential fails fast instead of hanging on a prompt.
//!
//! `push` never passes `--force`; `pull` is always `--ff-only` so the UI
//! can't leave the repo mid-merge with conflicts the user can't resolve
//! here.

use crate::exec::{reject_flag_like, run_full, GitError};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct SyncResult {
    pub stdout: String,
    pub stderr: String,
}

pub fn fetch(repo: &Path, remote: Option<&str>) -> Result<SyncResult, GitError> {
    let mut args = vec!["fetch"];
    match remote {
        Some(r) => {
            reject_flag_like(r)?;
            args.push(r);
        }
        None => args.push("--all"),
    }
    let (stdout, stderr) = run_full(repo, &args)?;
    Ok(SyncResult { stdout, stderr })
}

pub fn pull(
    repo: &Path,
    remote: Option<&str>,
    branch: Option<&str>,
) -> Result<SyncResult, GitError> {
    require_remote_for_branch(remote, branch)?;
    let mut args = vec!["pull", "--ff-only"];
    if let Some(r) = remote {
        reject_flag_like(r)?;
        args.push(r);
        if let Some(b) = branch {
            reject_flag_like(b)?;
            args.push(b);
        }
    }
    let (stdout, stderr) = run_full(repo, &args)?;
    Ok(SyncResult { stdout, stderr })
}

/// `git` only accepts `<branch>` as a positional after `<remote>`; reject
/// a branch supplied without one rather than silently dropping it.
fn require_remote_for_branch(remote: Option<&str>, branch: Option<&str>) -> Result<(), GitError> {
    if branch.is_some() && remote.is_none() {
        return Err(GitError::Failed {
            cmd: "validate sync args".into(),
            code: None,
            stderr: "a branch was given without a remote".into(),
        });
    }
    Ok(())
}

pub fn push(
    repo: &Path,
    remote: Option<&str>,
    branch: Option<&str>,
    set_upstream: bool,
) -> Result<SyncResult, GitError> {
    require_remote_for_branch(remote, branch)?;
    let mut args = vec!["push"];
    if set_upstream {
        args.push("-u");
    }
    if let Some(r) = remote {
        reject_flag_like(r)?;
        args.push(r);
        if let Some(b) = branch {
            reject_flag_like(b)?;
            args.push(b);
        }
    }
    let (stdout, stderr) = run_full(repo, &args)?;
    Ok(SyncResult { stdout, stderr })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use std::process::Command;

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

    fn init_repo(p: &Path) {
        git(p, &["init", "-q", "-b", "main"]);
        git(p, &["config", "user.email", "t@example.com"]);
        git(p, &["config", "user.name", "Test"]);
        fs::write(p.join("a.txt"), "one\n").unwrap();
        git(p, &["add", "a.txt"]);
        git(p, &["commit", "-q", "-m", "init"]);
    }

    #[test]
    fn push_fetch_pull_roundtrip_offline() {
        // Bare "remote".
        let bare = tempfile::tempdir().unwrap();
        git(bare.path(), &["init", "--bare", "-q", "-b", "main"]);
        let url = bare.path().to_str().unwrap();

        // Repo A: push main up.
        let a = tempfile::tempdir().unwrap();
        init_repo(a.path());
        git(a.path(), &["remote", "add", "origin", url]);
        push(a.path(), Some("origin"), Some("main"), true).unwrap();
        assert!(run_full(bare.path(), &["rev-parse", "main"]).is_ok());

        // Repo B: clone, add a commit, push.
        let b = tempfile::tempdir().unwrap();
        let bp = b.path().join("clone");
        assert!(Command::new("git")
            .args(["clone", "-q", url])
            .arg(&bp)
            .status()
            .unwrap()
            .success());
        git(&bp, &["config", "user.email", "b@example.com"]);
        git(&bp, &["config", "user.name", "B"]);
        fs::write(bp.join("c.txt"), "two\n").unwrap();
        git(&bp, &["add", "c.txt"]);
        git(&bp, &["commit", "-q", "-m", "from-b"]);
        push(&bp, Some("origin"), Some("main"), false).unwrap();

        // Repo A: fetch + ff-only pull sees B's commit.
        fetch(a.path(), Some("origin")).unwrap();
        pull(a.path(), Some("origin"), Some("main")).unwrap();
        assert!(a.path().join("c.txt").exists());
    }

    #[test]
    fn branch_without_remote_is_rejected() {
        let a = tempfile::tempdir().unwrap();
        init_repo(a.path());
        assert!(push(a.path(), None, Some("main"), false).is_err());
        assert!(pull(a.path(), None, Some("main")).is_err());
    }
}
