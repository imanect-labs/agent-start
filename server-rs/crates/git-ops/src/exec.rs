//! Shared `git` subprocess plumbing.

use std::path::Path;
use std::process::Command;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GitError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("git {cmd} failed (exit {code:?}): {stderr}")]
    Failed {
        cmd: String,
        code: Option<i32>,
        stderr: String,
    },
}

pub(crate) fn run(repo: &Path, args: &[&str]) -> Result<String, GitError> {
    let raw = run_raw(repo, args)?;
    Ok(String::from_utf8_lossy(&raw).into_owned())
}

/// Returns raw stdout bytes — needed for porcelain `-z` output where
/// NUL is the field separator.
pub(crate) fn run_raw(repo: &Path, args: &[&str]) -> Result<Vec<u8>, GitError> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(repo);
    for a in args {
        cmd.arg(a);
    }
    let output = cmd.output()?;
    if !output.status.success() {
        return Err(GitError::Failed {
            cmd: format!("git {}", args.join(" ")),
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(output.stdout)
}

/// Like [`run`] but returns both stdout and stderr on success.
///
/// Network operations (push/pull/fetch) write their human-readable
/// summary to stderr even when they succeed, so callers that want to
/// surface that output need both streams. `GIT_TERMINAL_PROMPT=0` is set
/// so a missing credential fails fast instead of blocking forever on an
/// interactive prompt — callers run this under `spawn_blocking` anyway.
pub(crate) fn run_full(repo: &Path, args: &[&str]) -> Result<(String, String), GitError> {
    let mut cmd = Command::new("git");
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    cmd.arg("-C").arg(repo);
    for a in args {
        cmd.arg(a);
    }
    let output = cmd.output()?;
    if !output.status.success() {
        return Err(GitError::Failed {
            cmd: format!("git {}", args.join(" ")),
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok((
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
}

/// Reject path arguments that could escape the repo or be parsed as a
/// flag: absolute paths and any `..` segment. Mirrors the inline check
/// the `git_diff` HTTP handler already performs.
pub(crate) fn validate_rel_path(p: &str) -> Result<(), GitError> {
    if p.starts_with('/') || p.split('/').any(|seg| seg == "..") {
        return Err(GitError::Failed {
            cmd: "validate path".into(),
            code: None,
            stderr: format!("invalid path: {p}"),
        });
    }
    Ok(())
}

/// Reject an argument that is empty or could be parsed as a flag (starts
/// with `-`). Used for ref/remote/branch names that flow onto the command
/// line where `--` separation isn't available (e.g. `git branch <name>`).
pub(crate) fn reject_flag_like(s: &str) -> Result<(), GitError> {
    if s.is_empty() || s.starts_with('-') {
        return Err(GitError::Failed {
            cmd: "validate argument".into(),
            code: None,
            stderr: format!("invalid argument: {s}"),
        });
    }
    Ok(())
}

pub fn is_git_repo(p: &Path) -> bool {
    run(p, &["rev-parse", "--git-dir"]).is_ok()
}

/// Ensure `dest` holds an up-to-date bare mirror of `url`.
///
/// This is the node-local repository cache behind multi-node
/// scheduling: the first session for a project on a given node pays for
/// a mirror clone, every later one only pays for a fetch, and worktrees
/// are cut straight from the mirror. A failed refresh of an existing
/// mirror is not fatal — branching off slightly stale refs beats
/// refusing to start a session because the node is briefly offline.
pub fn ensure_mirror(url: &str, dest: &Path) -> Result<(), GitError> {
    if dest.join("HEAD").exists() {
        if let Err(e) = run(dest, &["fetch", "--prune", "origin"]) {
            tracing::warn!(error = %e, mirror = %dest.display(), "mirror refresh failed; using cached refs");
        }
        return Ok(());
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // A partial directory from an interrupted clone would make `git
    // clone` fail forever; clear it first.
    if dest.exists() {
        std::fs::remove_dir_all(dest)?;
    }
    let mut cmd = Command::new("git");
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    cmd.arg("clone").arg("--mirror").arg(url).arg(dest);
    let output = cmd.output()?;
    if !output.status.success() {
        // Leave nothing half-cloned behind for the next attempt.
        let _ = std::fs::remove_dir_all(dest);
        return Err(GitError::Failed {
            cmd: format!("git clone --mirror {} {}", url, dest.display()),
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(())
}

/// URL of the repository's `origin` remote, if it has one. A project
/// without an origin cannot be reproduced on another node, so the
/// scheduler keeps its sessions on the host that holds the files.
pub fn origin_url(repo: &Path) -> Option<String> {
    let out = run(repo, &["remote", "get-url", "origin"]).ok()?;
    let trimmed = out.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Run `git clone <url> <dest>` from the current working directory.
/// Blocks until the clone completes; callers should `tokio::task::spawn_blocking`.
pub fn clone(url: &str, dest: &Path) -> Result<(), GitError> {
    let mut cmd = Command::new("git");
    cmd.arg("clone").arg(url).arg(dest);
    let output = cmd.output()?;
    if !output.status.success() {
        return Err(GitError::Failed {
            cmd: format!("git clone {} {}", url, dest.display()),
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(())
}
