//! Thin wrappers around the `git` CLI.
//!
//! Mirrors what the old Node `lib/worktree.ts` + `lib/git.ts` shipped.
//! We shell out to `git` rather than linking libgit2 to keep the
//! dependency surface and binary size small — the operations we need
//! (worktree, porcelain status, single-file diff) are simple enough
//! that the CLI is fine.

mod diff;
mod exec;
mod status;
mod worktree;

pub use diff::{git_diff, DiffMode, GitDiff};
pub use exec::{is_git_repo, GitError};
pub use status::{git_status, GitFile, GitStatus};
pub use worktree::{create_worktree, remove_worktree, worktree_path_for, WorktreeCreated};
