//! Thin wrappers around the `git` CLI.
//!
//! Mirrors what the old Node `lib/worktree.ts` + `lib/git.ts` shipped.
//! We shell out to `git` rather than linking libgit2 to keep the
//! dependency surface and binary size small — the operations we need
//! (worktree, porcelain status, single-file diff) are simple enough
//! that the CLI is fine.

mod branch;
mod commit;
mod diff;
mod exec;
mod github;
mod log;
mod remote;
mod status;
mod tree;
mod worktree;

pub use branch::{
    checkout_branch, create_and_checkout, create_branch, delete_branch, list_branches, BranchInfo,
};
pub use commit::{commit, discard, stage, unstage, CommitResult};
pub use diff::{git_diff, DiffMode, GitDiff};
pub use exec::{clone, is_git_repo, GitError};
pub use github::{list_issues, view_issue, IssueDetail, IssueSummary};
pub use log::{log_graph, CommitNode};
pub use remote::{fetch, pull, push, SyncResult};
pub use status::{git_status, GitFile, GitStatus};
pub use tree::{file_tree, TreeEntry};
pub use worktree::{create_worktree, remove_worktree, worktree_path_for, WorktreeCreated};
