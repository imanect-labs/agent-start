//! Turning a finished agent's worktree into a branch, a push, and a PR.
//!
//! This is the tail of a queued task (`docs/multinode-cloud-design.ja.md`
//! §2.7). It always runs on the machine that owns the worktree — the
//! node, not the control plane — because that is the only place the
//! files exist.
//!
//! Every step is allowed to be a no-op and say so. A task whose agent
//! changed nothing is not a failure, and neither is a host without `gh`
//! installed: the branch is still pushed and the user is told why no PR
//! appeared. Only a genuine git failure (a rejected push, a broken
//! repository) is an error, because that is the case where quietly
//! reporting success would leave the user waiting for a PR that is never
//! coming.

use crate::commit;
use crate::exec::{reject_flag_like, run, run_full, GitError};
use crate::status::git_status;
use std::path::Path;
use std::process::Command;

/// What to do with the worktree once the agent has finished.
#[derive(Debug, Clone)]
pub struct FinalizeRequest {
    pub commit_message: String,
    pub push: bool,
    pub open_pr: bool,
    pub pr_title: String,
    pub pr_body: String,
    pub draft: bool,
    /// Base branch for the PR; empty means the repository default.
    pub base_branch: String,
}

/// What actually happened. Each step reports itself so a partial result
/// stays legible instead of collapsing into one boolean.
#[derive(Debug, Clone, Default)]
pub struct FinalizeReport {
    pub committed: bool,
    pub sha: String,
    pub branch: String,
    pub pushed: bool,
    pub pr_url: String,
    pub notes: Vec<String>,
}

/// The branch currently checked out in `repo`, or `None` on a detached
/// HEAD (which a task should never produce, but a user poking at the
/// worktree can).
pub fn current_branch(repo: &Path) -> Option<String> {
    let out = run(repo, &["symbolic-ref", "--quiet", "--short", "HEAD"]).ok()?;
    let name = out.trim().to_string();
    (!name.is_empty()).then_some(name)
}

/// True when the worktree has anything to commit, staged or not,
/// tracked or not.
pub fn has_changes(repo: &Path) -> Result<bool, GitError> {
    let status = git_status(repo)?;
    Ok(!status.files.is_empty())
}

/// Commit whatever the agent left behind, push the branch, and open a
/// pull request — as far as each is asked for and possible.
///
/// Callers must run this under `spawn_blocking`: push and `gh` are
/// network-bound.
pub fn finalize(repo: &Path, req: &FinalizeRequest) -> Result<FinalizeReport, GitError> {
    let mut report = FinalizeReport::default();

    let branch = current_branch(repo).ok_or_else(|| GitError::Failed {
        cmd: "finalize".into(),
        code: None,
        stderr: "the worktree has a detached HEAD; there is no branch to push".into(),
    })?;
    reject_flag_like(&branch)?;
    report.branch = branch.clone();

    if has_changes(repo)? {
        commit::stage(repo, &[])?;
        let result = commit::commit(repo, &req.commit_message)?;
        report.committed = true;
        report.sha = result.sha;
    } else {
        report.notes.push(
            "エージェントは変更を残しませんでした（コミットするものがありません）".to_string(),
        );
    }

    if !req.push {
        return Ok(report);
    }

    // Nothing local means nothing to push and nothing to open a PR
    // against — reporting otherwise would send the user to an empty
    // branch.
    if !report.committed && !branch_has_commits_beyond_upstream(repo, &branch) {
        report
            .notes
            .push("push できる新しいコミットがありません".to_string());
        return Ok(report);
    }

    run_full(repo, &["push", "-u", "origin", &branch])?;
    report.pushed = true;

    if !req.open_pr {
        return Ok(report);
    }

    match open_pr(repo, &branch, req) {
        Ok(url) => report.pr_url = url,
        Err(note) => report.notes.push(note),
    }
    Ok(report)
}

/// True when the branch has commits its upstream does not. A branch with
/// no upstream at all counts: it has never been pushed, so everything on
/// it is new.
fn branch_has_commits_beyond_upstream(repo: &Path, branch: &str) -> bool {
    let upstream = format!("{branch}@{{upstream}}");
    match run(
        repo,
        &["rev-list", "--count", &format!("{upstream}..{branch}")],
    ) {
        Ok(out) => out.trim().parse::<u64>().unwrap_or(0) > 0,
        // No upstream configured — the branch is entirely new.
        Err(_) => true,
    }
}

/// Open a pull request with `gh`. Returns the PR URL, or a
/// human-readable note explaining why there is none. A missing `gh`, or
/// an unauthenticated one, is a note rather than an error: the work is
/// pushed and the user can open the PR themselves.
fn open_pr(repo: &Path, branch: &str, req: &FinalizeRequest) -> Result<String, String> {
    let title = if req.pr_title.trim().is_empty() {
        branch.to_string()
    } else {
        req.pr_title.clone()
    };
    let mut args: Vec<String> = vec![
        "pr".into(),
        "create".into(),
        "--head".into(),
        branch.into(),
        "--title".into(),
        title,
        "--body".into(),
        req.pr_body.clone(),
    ];
    if req.draft {
        args.push("--draft".into());
    }
    let base = req.base_branch.trim();
    if !base.is_empty() {
        if base.starts_with('-') {
            return Err(format!("base ブランチ名が不正です: {base}"));
        }
        args.push("--base".into());
        args.push(base.into());
    }

    let output = Command::new("gh")
        .current_dir(repo)
        .args(&args)
        // Closed, not inherited: an unauthenticated `gh` prompts, and a
        // prompt with nowhere to read from would block this thread for
        // as long as the node lives.
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| {
            format!("gh が実行できないため PR は作成していません（ブランチは push 済み）: {e}")
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!(
            "gh pr create に失敗したため PR は作成していません（ブランチは push 済み）: {stderr}"
        ));
    }
    // `gh pr create` prints the PR URL on stdout, sometimes preceded by
    // progress chatter — take the last line that looks like one.
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| l.starts_with("https://"))
        .map(str::to_string)
        .ok_or_else(|| "PR は作成されましたが URL を読み取れませんでした".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

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

    fn request() -> FinalizeRequest {
        FinalizeRequest {
            commit_message: "task: work".into(),
            push: false,
            open_pr: false,
            pr_title: String::new(),
            pr_body: String::new(),
            draft: false,
            base_branch: String::new(),
        }
    }

    #[test]
    fn commits_whatever_the_agent_left_behind() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        fs::write(dir.path().join("new.txt"), "from the agent\n").unwrap();

        let report = finalize(dir.path(), &request()).unwrap();
        assert!(report.committed);
        assert!(!report.sha.is_empty());
        assert_eq!(report.branch, "main");
        assert!(!has_changes(dir.path()).unwrap(), "worktree still dirty");
    }

    #[test]
    fn an_agent_that_changed_nothing_is_not_a_failure() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());

        let report = finalize(dir.path(), &request()).unwrap();
        assert!(!report.committed);
        assert!(!report.notes.is_empty(), "the user is told nothing changed");
    }

    #[test]
    fn pushes_the_branch_to_origin() {
        let bare = tempfile::tempdir().unwrap();
        git(bare.path(), &["init", "--bare", "-q", "-b", "main"]);
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        git(
            dir.path(),
            &["remote", "add", "origin", bare.path().to_str().unwrap()],
        );
        fs::write(dir.path().join("new.txt"), "work\n").unwrap();

        let mut req = request();
        req.push = true;
        let report = finalize(dir.path(), &req).unwrap();
        assert!(report.committed && report.pushed);
        assert!(run(bare.path(), &["rev-parse", "main"]).is_ok());
    }

    #[test]
    fn a_clean_worktree_with_nothing_new_is_not_pushed() {
        let bare = tempfile::tempdir().unwrap();
        git(bare.path(), &["init", "--bare", "-q", "-b", "main"]);
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        git(
            dir.path(),
            &["remote", "add", "origin", bare.path().to_str().unwrap()],
        );
        git(dir.path(), &["push", "-q", "-u", "origin", "main"]);

        let mut req = request();
        req.push = true;
        let report = finalize(dir.path(), &req).unwrap();
        assert!(!report.committed);
        assert!(!report.pushed, "pushed an empty branch");
    }

    /// A `gh` of our own, first on `PATH`.
    ///
    /// The real one is never invoked from a test: on a CI runner it is
    /// installed *and* authenticated, so `gh pr create` would try to open
    /// an actual pull request. This stub records the argv it was given and
    /// decides what to do from the repository it was run in, which is how
    /// two tests share one `PATH` — process-global, set once for the whole
    /// binary.
    fn stub_gh() {
        static DIR: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
        DIR.get_or_init(|| {
            let dir = tempfile::tempdir().expect("stub dir");
            let gh = dir.path().join("gh");
            fs::write(
                &gh,
                r#"#!/bin/sh
for a in "$@"; do printf '%s\n' "$a"; done > .gh-argv
if [ -f .gh-fail ]; then
  echo "gh: To get started with GitHub CLI, please run: gh auth login" >&2
  exit 4
fi
# The real one prints progress before the URL; the parser must skip it.
echo "Creating pull request for the branch into main"
echo "https://github.com/example/repo/pull/42"
"#,
            )
            .unwrap();
            let mut perm = fs::metadata(&gh).unwrap().permissions();
            std::os::unix::fs::PermissionsExt::set_mode(&mut perm, 0o755);
            fs::set_permissions(&gh, perm).unwrap();
            let path = std::env::var("PATH").unwrap_or_default();
            std::env::set_var("PATH", format!("{}:{path}", dir.path().display()));
            dir
        });
    }

    /// A repo with an `origin` to push to, ready for `finalize`.
    fn repo_with_origin() -> (tempfile::TempDir, tempfile::TempDir) {
        let bare = tempfile::tempdir().unwrap();
        git(bare.path(), &["init", "--bare", "-q", "-b", "main"]);
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        git(
            dir.path(),
            &["remote", "add", "origin", bare.path().to_str().unwrap()],
        );
        fs::write(dir.path().join("new.txt"), "work\n").unwrap();
        (dir, bare)
    }

    fn gh_argv(repo: &Path) -> Vec<String> {
        fs::read_to_string(repo.join(".gh-argv"))
            .expect("gh was never invoked")
            .lines()
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn opens_a_pull_request_and_reports_its_url() {
        stub_gh();
        let (dir, _bare) = repo_with_origin();
        git(dir.path(), &["checkout", "-q", "-b", "agent-start/cc-1"]);

        let mut req = request();
        req.push = true;
        req.open_pr = true;
        req.pr_title = "ログイン検証を直す".into();
        req.pr_body = "本文".into();
        req.draft = true;
        req.base_branch = "main".into();
        let report = finalize(dir.path(), &req).unwrap();

        assert!(report.pushed);
        assert_eq!(report.pr_url, "https://github.com/example/repo/pull/42");

        // The branch is named explicitly: `gh` would otherwise infer it
        // from the checkout, and a worktree's HEAD is not what we mean.
        let argv = gh_argv(dir.path());
        let pair = |flag: &str| {
            argv.iter()
                .position(|a| a == flag)
                .map(|i| argv[i + 1].clone())
        };
        assert_eq!(argv[0], "pr");
        assert_eq!(argv[1], "create");
        assert_eq!(pair("--head").as_deref(), Some("agent-start/cc-1"));
        assert_eq!(pair("--title").as_deref(), Some("ログイン検証を直す"));
        assert_eq!(pair("--body").as_deref(), Some("本文"));
        assert_eq!(pair("--base").as_deref(), Some("main"));
        assert!(argv.iter().any(|a| a == "--draft"));
    }

    /// The branch is pushed either way; a `gh` that cannot authenticate
    /// costs the user a PR, not their work.
    #[test]
    fn an_unauthenticated_gh_is_a_note_not_a_failure() {
        stub_gh();
        let (dir, _bare) = repo_with_origin();
        fs::write(dir.path().join(".gh-fail"), "").unwrap();

        let mut req = request();
        req.push = true;
        req.open_pr = true;
        let report = finalize(dir.path(), &req).unwrap();

        assert!(report.pushed, "the push was lost with the PR");
        assert!(report.pr_url.is_empty());
        let note = report.notes.join(" / ");
        assert!(
            note.contains("push 済み") && note.contains("gh auth login"),
            "the note says neither what failed nor that the work survived: {note}"
        );
    }

    /// An empty title would make `gh` read one from an editor it has no
    /// terminal for. The branch name is a poor title but a real one.
    #[test]
    fn a_task_without_a_title_still_gets_one() {
        stub_gh();
        let (dir, _bare) = repo_with_origin();
        git(dir.path(), &["checkout", "-q", "-b", "agent-start/cc-2"]);

        let mut req = request();
        req.push = true;
        req.open_pr = true;
        let report = finalize(dir.path(), &req).unwrap();

        assert!(!report.pr_url.is_empty());
        let argv = gh_argv(dir.path());
        let title = argv
            .iter()
            .position(|a| a == "--title")
            .map(|i| argv[i + 1].clone());
        assert_eq!(title.as_deref(), Some("agent-start/cc-2"));
    }

    #[test]
    fn a_detached_head_is_an_error_not_a_silent_skip() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        let sha = run(dir.path(), &["rev-parse", "HEAD"]).unwrap();
        git(dir.path(), &["checkout", "-q", sha.trim()]);

        let err = finalize(dir.path(), &request()).unwrap_err().to_string();
        assert!(err.contains("detached"), "unhelpful error: {err}");
    }
}
