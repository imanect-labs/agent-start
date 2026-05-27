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
