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
