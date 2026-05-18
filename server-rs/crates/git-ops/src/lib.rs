//! Thin wrappers around the `git` CLI for worktree management.
//!
//! Mirrors `lib/worktree.ts` from the Node implementation. We shell out
//! to `git` rather than linking libgit2 to keep the dependency surface
//! and binary size small — the operations here (worktree add/remove,
//! branch -D, symbolic-ref) are simple enough that the CLI is fine.

use std::path::{Path, PathBuf};
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

fn run(repo: &Path, args: &[&str]) -> Result<String, GitError> {
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
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub fn is_git_repo(p: &Path) -> bool {
    run(p, &["rev-parse", "--git-dir"]).is_ok()
}

fn default_branch(repo: &Path) -> String {
    match run(repo, &["symbolic-ref", "--short", "HEAD"]) {
        Ok(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                "HEAD".to_string()
            } else {
                trimmed.to_string()
            }
        }
        Err(_) => "HEAD".to_string(),
    }
}

pub struct WorktreeCreated {
    pub worktree_path: PathBuf,
    pub orig_path: PathBuf,
    pub branch: String,
}

pub fn worktree_path_for(session_name: &str) -> PathBuf {
    config_loader::worktree_root().join(session_name)
}

pub fn create_worktree(orig_path: &Path, session_name: &str) -> Result<WorktreeCreated, GitError> {
    if !is_git_repo(orig_path) {
        return Err(GitError::Failed {
            cmd: "git rev-parse --git-dir".into(),
            code: None,
            stderr: format!("{} is not a git repository", orig_path.display()),
        });
    }
    let wt_path = worktree_path_for(session_name);
    if let Some(parent) = wt_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let base = default_branch(orig_path);
    let branch = format!("agent-start/{session_name}");
    run(
        orig_path,
        &[
            "worktree",
            "add",
            "-b",
            &branch,
            wt_path.to_str().ok_or_else(|| GitError::Failed {
                cmd: "validate worktree path".into(),
                code: None,
                stderr: "non-utf8 path".into(),
            })?,
            &base,
        ],
    )?;
    Ok(WorktreeCreated {
        worktree_path: wt_path,
        orig_path: orig_path.to_path_buf(),
        branch,
    })
}

pub fn remove_worktree(
    worktree_path: &Path,
    orig_path: Option<&Path>,
    remove_branch: bool,
) -> Result<(), GitError> {
    let orig: Option<PathBuf> = orig_path.map(Path::to_path_buf).or_else(|| {
        run(worktree_path, &["rev-parse", "--git-common-dir"])
            .ok()
            .map(|s| {
                let trimmed = s.trim();
                Path::new(trimmed)
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| PathBuf::from(trimmed))
            })
    });

    let branch = if remove_branch && orig.is_some() {
        run(worktree_path, &["symbolic-ref", "--short", "HEAD"])
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    } else {
        None
    };

    if let Some(orig) = orig.as_deref() {
        let path_str = worktree_path.to_string_lossy().into_owned();
        let _ = run(orig, &["worktree", "remove", "--force", path_str.as_str()]);
    }
    let _ = std::fs::remove_dir_all(worktree_path);

    if let (Some(orig), Some(branch)) = (orig.as_deref(), branch.as_deref()) {
        if branch.starts_with("agent-start/") {
            let _ = run(orig, &["branch", "-D", branch]);
        }
    }
    Ok(())
}
