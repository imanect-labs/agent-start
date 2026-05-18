//! `git worktree add` / `git worktree remove` wrappers.
//!
//! The branch name we attach to every worktree is
//! `agent-start/<session>`; on removal that branch is force-deleted
//! along with the worktree, but only when it really starts with
//! `agent-start/` (a safety net so we never blow away a user's branch).

use crate::exec::{is_git_repo, run, GitError};
use std::path::{Path, PathBuf};

pub struct WorktreeCreated {
    pub worktree_path: PathBuf,
    pub orig_path: PathBuf,
    pub branch: String,
}

pub fn worktree_path_for(session_name: &str) -> PathBuf {
    config_loader::worktree_root().join(session_name)
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
    let wt_str = wt_path.to_str().ok_or_else(|| GitError::Failed {
        cmd: "validate worktree path".into(),
        code: None,
        stderr: "non-utf8 path".into(),
    })?;
    run(
        orig_path,
        &["worktree", "add", "-b", &branch, wt_str, &base],
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
