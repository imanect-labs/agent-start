//! Working-tree mutations: stage, unstage, commit, discard.
//!
//! All path arguments are repo-relative and validated against traversal
//! before they reach the `git` command line, and every pathspec is
//! preceded by `--` so a path can never be parsed as a flag/refspec.

use crate::exec::{run, validate_rel_path, GitError};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct CommitResult {
    pub sha: String,
    pub summary: String,
}

fn validate_paths(paths: &[String]) -> Result<(), GitError> {
    for p in paths {
        validate_rel_path(p)?;
    }
    Ok(())
}

/// Stage the given repo-relative paths. With an empty list, stages every
/// change including untracked files (`git add -A`).
pub fn stage(repo: &Path, paths: &[String]) -> Result<(), GitError> {
    validate_paths(paths)?;
    if paths.is_empty() {
        run(repo, &["add", "-A"])?;
    } else {
        let mut args: Vec<&str> = vec!["add", "--"];
        args.extend(paths.iter().map(String::as_str));
        run(repo, &args)?;
    }
    Ok(())
}

/// Unstage the given paths (move them back to the working tree). An empty
/// list unstages everything via `git reset`.
pub fn unstage(repo: &Path, paths: &[String]) -> Result<(), GitError> {
    validate_paths(paths)?;
    if paths.is_empty() {
        run(repo, &["reset", "-q"])?;
    } else {
        let mut args: Vec<&str> = vec!["restore", "--staged", "--"];
        args.extend(paths.iter().map(String::as_str));
        run(repo, &args)?;
    }
    Ok(())
}

/// Create a commit from the staged index. Returns the new commit's full
/// SHA and subject line. Rejects an empty message before invoking git.
pub fn commit(repo: &Path, message: &str) -> Result<CommitResult, GitError> {
    if message.trim().is_empty() {
        return Err(GitError::Failed {
            cmd: "git commit".into(),
            code: None,
            stderr: "empty commit message".into(),
        });
    }
    run(repo, &["commit", "-m", message])?;
    let sha = run(repo, &["rev-parse", "HEAD"])?.trim().to_string();
    let summary = run(repo, &["log", "-1", "--pretty=%s"])?.trim().to_string();
    Ok(CommitResult { sha, summary })
}

fn is_tracked(repo: &Path, path: &str) -> bool {
    run(repo, &["ls-files", "--error-unmatch", "--", path]).is_ok()
}

/// Discard local changes for the given paths. Tracked files are reset to
/// HEAD (index + working tree); untracked files/dirs are deleted with a
/// scoped `git clean`. An empty list is rejected so this can never wipe
/// the whole tree.
pub fn discard(repo: &Path, paths: &[String]) -> Result<(), GitError> {
    if paths.is_empty() {
        return Err(GitError::Failed {
            cmd: "git discard".into(),
            code: None,
            stderr: "no paths given".into(),
        });
    }
    validate_paths(paths)?;

    let mut tracked: Vec<&str> = Vec::new();
    let mut untracked: Vec<&str> = Vec::new();
    for p in paths {
        if is_tracked(repo, p) {
            tracked.push(p);
        } else {
            untracked.push(p);
        }
    }

    if !tracked.is_empty() {
        let mut args: Vec<&str> = vec!["restore", "--source=HEAD", "--staged", "--worktree", "--"];
        args.extend(tracked.iter().copied());
        run(repo, &args)?;
    }
    if !untracked.is_empty() {
        // `-d` lets us remove untracked directories too, but the explicit
        // pathspec after `--` keeps this scoped — never a bare repo clean.
        let mut args: Vec<&str> = vec!["clean", "-f", "-d", "--"];
        args.extend(untracked.iter().copied());
        run(repo, &args)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;
    use tempfile::TempDir;

    /// A temp repo with one committed file (`a.txt` = "one\n").
    fn repo() -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        git(p, &["init", "-q"]);
        git(p, &["config", "user.email", "t@example.com"]);
        git(p, &["config", "user.name", "Test"]);
        fs::write(p.join("a.txt"), "one\n").unwrap();
        git(p, &["add", "a.txt"]);
        git(p, &["commit", "-q", "-m", "init"]);
        dir
    }

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

    #[test]
    fn stage_then_unstage() {
        let dir = repo();
        let p = dir.path();
        fs::write(p.join("a.txt"), "two\n").unwrap();
        stage(p, &["a.txt".into()]).unwrap();
        let staged = run(p, &["diff", "--cached", "--name-only"]).unwrap();
        assert!(staged.contains("a.txt"));
        unstage(p, &["a.txt".into()]).unwrap();
        let staged = run(p, &["diff", "--cached", "--name-only"]).unwrap();
        assert!(!staged.contains("a.txt"));
    }

    #[test]
    fn commit_returns_head_sha() {
        let dir = repo();
        let p = dir.path();
        fs::write(p.join("a.txt"), "two\n").unwrap();
        stage(p, &["a.txt".into()]).unwrap();
        let res = commit(p, "second").unwrap();
        let head = run(p, &["rev-parse", "HEAD"]).unwrap();
        assert_eq!(res.sha, head.trim());
        assert_eq!(res.summary, "second");
    }

    #[test]
    fn empty_message_rejected() {
        let dir = repo();
        assert!(commit(dir.path(), "   ").is_err());
    }

    #[test]
    fn discard_restores_tracked_and_removes_untracked() {
        let dir = repo();
        let p = dir.path();
        fs::write(p.join("a.txt"), "dirty\n").unwrap();
        fs::write(p.join("new.txt"), "x\n").unwrap();
        discard(p, &["a.txt".into(), "new.txt".into()]).unwrap();
        assert_eq!(fs::read_to_string(p.join("a.txt")).unwrap(), "one\n");
        assert!(!p.join("new.txt").exists());
    }

    #[test]
    fn rejects_path_traversal() {
        let dir = repo();
        assert!(stage(dir.path(), &["../escape".into()]).is_err());
        assert!(stage(dir.path(), &["/etc/passwd".into()]).is_err());
    }

    #[test]
    fn empty_discard_rejected() {
        let dir = repo();
        assert!(discard(dir.path(), &[]).is_err());
    }
}
