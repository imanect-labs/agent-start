//! Single-file `git diff` for the right-hand pane of the diff viewer.
//!
//! Untracked files have no diff target, so we synthesize an all-`+`
//! patch from their contents. Output is capped at `MAX_DIFF_BYTES` to
//! keep huge generated files (lockfiles, blobs that accidentally got
//! tracked) from blowing up the host's memory.

use crate::exec::{run, GitError};
use serde::Serialize;
use std::path::Path;

const MAX_DIFF_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, Copy)]
pub enum DiffMode {
    Worktree,
    Staged,
    Head,
}

impl DiffMode {
    pub fn parse(s: &str) -> Self {
        match s {
            "staged" => DiffMode::Staged,
            "head" => DiffMode::Head,
            _ => DiffMode::Worktree,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct GitDiff {
    pub diff: String,
    pub truncated: bool,
    #[serde(rename = "isUntracked")]
    pub is_untracked: bool,
}

pub fn git_diff(repo: &Path, file: &str, mode: DiffMode) -> Result<GitDiff, GitError> {
    let is_untracked = run(repo, &["ls-files", "--error-unmatch", "--", file]).is_err();

    if is_untracked && !matches!(mode, DiffMode::Staged) {
        return Ok(synthesize_untracked_diff(repo, file));
    }

    let mut args: Vec<&str> = vec!["-c", "color.ui=never", "diff", "--no-color"];
    match mode {
        DiffMode::Staged => args.push("--cached"),
        DiffMode::Head => args.push("HEAD"),
        DiffMode::Worktree => {}
    }
    args.push("--");
    args.push(file);

    let stdout = run(repo, &args)?;
    let bytes = stdout.as_bytes();
    let (diff, truncated) = cap(bytes);
    Ok(GitDiff {
        diff,
        truncated,
        is_untracked,
    })
}

fn synthesize_untracked_diff(repo: &Path, file: &str) -> GitDiff {
    let abs = repo.join(file);
    let raw = match std::fs::read(&abs) {
        Ok(b) => b,
        Err(_) => {
            return GitDiff {
                diff: String::new(),
                truncated: false,
                is_untracked: true,
            };
        }
    };
    let truncated = raw.len() > MAX_DIFF_BYTES;
    let slice = if truncated {
        &raw[..MAX_DIFF_BYTES]
    } else {
        &raw[..]
    };
    let body = String::from_utf8_lossy(slice);
    let mut diff = String::with_capacity(body.len() + file.len() * 2 + 64);
    diff.push_str(&format!("diff --git a/{file} b/{file}\n"));
    diff.push_str("new file (untracked)\n");
    diff.push_str("--- /dev/null\n");
    diff.push_str(&format!("+++ b/{file}\n"));
    for line in body.split('\n') {
        diff.push('+');
        diff.push_str(line);
        diff.push('\n');
    }
    // Strip the trailing extra newline introduced by the split.
    if diff.ends_with('\n') {
        diff.pop();
    }
    GitDiff {
        diff,
        truncated,
        is_untracked: true,
    }
}

fn cap(bytes: &[u8]) -> (String, bool) {
    if bytes.len() <= MAX_DIFF_BYTES {
        return (String::from_utf8_lossy(bytes).into_owned(), false);
    }
    let mut s = String::from_utf8_lossy(&bytes[..MAX_DIFF_BYTES]).into_owned();
    s.push_str("\n\n[…diff truncated…]\n");
    (s, true)
}
