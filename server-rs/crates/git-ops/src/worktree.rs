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

/// The remote's default branch (e.g. `main`), resolved via
/// `refs/remotes/origin/HEAD`. Returns `None` when no `origin` remote
/// is configured or its HEAD symref hasn't been set.
fn remote_default_branch(repo: &Path) -> Option<String> {
    let out = run(
        repo,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    )
    .ok()?;
    let trimmed = out.trim();
    trimmed.strip_prefix("origin/").map(|s| s.to_string())
}

/// Pick the base commit for a new worktree, as a resolved SHA.
///
/// Returning a name would be a trap. `git worktree add -b <new> <path>
/// <name>` does not always create `<new>`: when `<name>` exists only as
/// a remote-tracking branch, git's DWIM creates a local branch called
/// `<name>` instead, and the *next* session for the same repo then dies
/// with "already used by worktree". A bare mirror can also carry a HEAD
/// pointing at a ref it does not have, which fails outright with
/// "invalid reference". A SHA cannot be reinterpreted either way.
///
/// Preference order: the remote's default branch (fetched first, so we
/// branch off the latest upstream commit), then the repo's own HEAD,
/// then any branch it has — the last covers a mirror whose HEAD is
/// stale or unborn.
fn resolve_base(repo: &Path) -> Result<String, GitError> {
    let mut candidates: Vec<String> = Vec::new();
    if let Some(branch) = remote_default_branch(repo) {
        // Best-effort fetch; if it fails (offline, auth, etc.) we still
        // branch off whatever the local `origin/<branch>` ref points at.
        let _ = run(repo, &["fetch", "origin", &branch]);
        candidates.push(format!("refs/remotes/origin/{branch}"));
    }
    candidates.push("HEAD".to_string());
    if let Ok(out) = run(
        repo,
        &[
            "for-each-ref",
            "--count=1",
            "--format=%(refname)",
            "refs/heads/",
        ],
    ) {
        let first = out.trim();
        if !first.is_empty() {
            candidates.push(first.to_string());
        }
    }

    for candidate in &candidates {
        let spec = format!("{candidate}^{{commit}}");
        if let Ok(sha) = run(repo, &["rev-parse", "--verify", "--quiet", spec.as_str()]) {
            let sha = sha.trim().to_string();
            if !sha.is_empty() {
                return Ok(sha);
            }
        }
    }
    Err(GitError::Failed {
        cmd: "resolve worktree base".into(),
        code: None,
        stderr: format!(
            "{} has no commit to branch from (tried: {})",
            repo.display(),
            candidates.join(", ")
        ),
    })
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
    let base = resolve_base(orig_path)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(cwd: &Path, args: &[&str]) {
        let out = Command::new("git")
            .current_dir(cwd)
            .args(args)
            .output()
            .expect("run git");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn seed(dir: &Path) {
        git(dir, &["init", "-q", "."]);
        git(dir, &["config", "user.email", "t@example.invalid"]);
        git(dir, &["config", "user.name", "t"]);
        std::fs::write(dir.join("f"), "x").unwrap();
        git(dir, &["add", "-A"]);
        git(dir, &["commit", "-qm", "init"]);
    }

    /// The repository's own object id for `HEAD`, whatever hash
    /// algorithm it uses — asserting 40 characters would fail on a
    /// SHA-256 repository.
    fn head_oid(dir: &Path) -> String {
        let out = Command::new("git")
            .current_dir(dir)
            .args(["rev-parse", "--verify", "HEAD^{commit}"])
            .output()
            .expect("run git");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    #[test]
    fn base_resolves_to_a_commit_sha_not_a_name() {
        let dir = tempfile::tempdir().unwrap();
        seed(dir.path());
        let base = resolve_base(dir.path()).unwrap();
        assert_eq!(base, head_oid(dir.path()));
        assert!(base.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn base_survives_a_head_pointing_at_a_branch_that_does_not_exist() {
        // A bare mirror can carry a HEAD whose ref was never fetched;
        // naming it as the base fails with "invalid reference".
        let dir = tempfile::tempdir().unwrap();
        seed(dir.path());
        git(
            dir.path(),
            &["symbolic-ref", "HEAD", "refs/heads/nonexistent"],
        );
        let base = resolve_base(dir.path()).expect("should fall back to an existing branch");
        assert_eq!(base.len(), 40);
    }

    #[test]
    fn a_repo_with_no_commits_reports_a_clear_error() {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q", "."]);
        let err = resolve_base(dir.path()).unwrap_err().to_string();
        assert!(err.contains("no commit to branch from"), "unhelpful: {err}");
    }
}
