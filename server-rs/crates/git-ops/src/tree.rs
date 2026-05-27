//! Repo file tree from the committed HEAD tree (`git ls-tree`).
//!
//! One directory level per call so the frontend can lazy-expand large
//! repos; `is_dir` comes from ls-tree's object type so it's exact.

use crate::exec::{run_raw, validate_rel_path, GitError};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct TreeEntry {
    /// Repo-relative path (full, e.g. `src/main.rs`).
    pub path: String,
    pub name: String,
    #[serde(rename = "isDir")]
    pub is_dir: bool,
}

/// List the immediate children of `subdir` (or the repo root) in HEAD.
/// Directories sort before files, then alphabetically.
pub fn file_tree(repo: &Path, subdir: Option<&str>) -> Result<Vec<TreeEntry>, GitError> {
    let pathspec = match subdir {
        Some(d) => {
            validate_rel_path(d)?;
            let d = d.trim_end_matches('/');
            (!d.is_empty()).then(|| format!("{d}/"))
        }
        None => None,
    };
    // `--` before the pathspec so a `subdir` can never be parsed as a flag.
    let mut args: Vec<&str> = vec!["ls-tree", "-z", "HEAD", "--"];
    if let Some(ref p) = pathspec {
        args.push(p.as_str());
    }
    let raw = run_raw(repo, &args)?;

    let mut out = Vec::new();
    for rec in raw.split(|b| *b == 0) {
        if rec.is_empty() {
            continue;
        }
        let s = String::from_utf8_lossy(rec);
        // "<mode> <type> <object>\t<path>"
        let Some((meta, path)) = s.split_once('\t') else {
            continue;
        };
        let is_dir = meta.split_whitespace().nth(1) == Some("tree");
        let name = path.rsplit('/').next().unwrap_or(path).to_string();
        out.push(TreeEntry {
            path: path.to_string(),
            name,
            is_dir,
        });
    }
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.cmp(&b.name)));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;
    use tempfile::TempDir;

    fn git(repo: &Path, args: &[&str]) {
        assert!(Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .status()
            .unwrap()
            .success());
    }

    fn repo() -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        git(p, &["init", "-q", "-b", "main"]);
        git(p, &["config", "user.email", "t@example.com"]);
        git(p, &["config", "user.name", "Test"]);
        fs::create_dir(p.join("dir")).unwrap();
        fs::write(p.join("dir/a.txt"), "x\n").unwrap();
        fs::write(p.join("top.txt"), "y\n").unwrap();
        git(p, &["add", "-A"]);
        git(p, &["commit", "-q", "-m", "init"]);
        dir
    }

    #[test]
    fn root_lists_dir_and_file() {
        let dir = repo();
        let entries = file_tree(dir.path(), None).unwrap();
        let d = entries.iter().find(|e| e.name == "dir").unwrap();
        assert!(d.is_dir && d.path == "dir");
        let f = entries.iter().find(|e| e.name == "top.txt").unwrap();
        assert!(!f.is_dir);
        // Directory sorts first.
        assert_eq!(entries[0].name, "dir");
    }

    #[test]
    fn subdir_lists_children() {
        let dir = repo();
        let entries = file_tree(dir.path(), Some("dir")).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "dir/a.txt");
        assert!(!entries[0].is_dir);
    }

    #[test]
    fn rejects_traversal() {
        let dir = repo();
        assert!(file_tree(dir.path(), Some("../etc")).is_err());
    }
}
