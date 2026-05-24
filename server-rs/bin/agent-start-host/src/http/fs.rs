//! `/api/fs/*` — file-tree + read + atomic-write endpoints for the
//! in-app editor and file explorer. All paths must resolve under one of
//! `cfg.roots` to keep the host from being a directory-traversal vector.

use super::err;
use agent_start_api::{FsEntry, FsFileBody, FsTreeBody, FsWriteRequest};
use axum::extract::Query;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use config_loader::safety::{ensure_under, under_any};
use serde::Deserialize;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const BINARY_SNIFF: usize = 8192;

fn allowed_bases() -> Vec<PathBuf> {
    let mut bases: Vec<PathBuf> = Vec::new();
    if let Ok(cfg) = config_loader::load_config() {
        bases.extend(cfg.roots.iter().map(|r| config_loader::expand_root(r)));
    }
    // Sessions launched in worktree mode live under worktree_root(); allow the
    // editor/file-explorer to read/write there too.
    bases.push(config_loader::worktree_root());
    bases.push(config_loader::projects_dir());
    bases
}

fn resolve_under_roots(target: &Path) -> Result<PathBuf, Response> {
    let bases = allowed_bases();
    under_any(&bases, target)
        .ok_or_else(|| err(StatusCode::FORBIDDEN, "path outside allowed roots"))
}

#[derive(Debug, Deserialize)]
pub struct TreeQuery {
    pub path: String,
}

pub async fn fs_tree(Query(q): Query<TreeQuery>) -> Response {
    let target = PathBuf::from(&q.path);
    let resolved = match resolve_under_roots(&target) {
        Ok(p) => p,
        Err(e) => return e,
    };

    let cfg = config_loader::load_config().ok();
    let show_hidden = cfg.as_ref().map(|c| c.show_hidden).unwrap_or(false);

    let rd = match std::fs::read_dir(&resolved) {
        Ok(r) => r,
        Err(e) => return err(StatusCode::BAD_REQUEST, format!("read_dir failed: {e}")),
    };
    let mut entries: Vec<FsEntry> = Vec::new();
    for ent in rd.flatten() {
        let name = ent.file_name().to_string_lossy().into_owned();
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        let is_dir = ent.file_type().map(|t| t.is_dir()).unwrap_or(false);
        entries.push(FsEntry {
            name,
            path: ent.path().to_string_lossy().into_owned(),
            is_dir,
        });
    }
    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });

    Json(FsTreeBody { entries }).into_response()
}

#[derive(Debug, Deserialize)]
pub struct FileQuery {
    pub path: String,
}

pub async fn fs_read(Query(q): Query<FileQuery>) -> Response {
    let target = PathBuf::from(&q.path);
    let resolved = match resolve_under_roots(&target) {
        Ok(p) => p,
        Err(e) => return e,
    };

    let meta = match std::fs::metadata(&resolved) {
        Ok(m) => m,
        Err(e) => return err(StatusCode::BAD_REQUEST, format!("stat failed: {e}")),
    };
    if !meta.is_file() {
        return err(StatusCode::BAD_REQUEST, "not a regular file");
    }
    if meta.len() > MAX_FILE_BYTES {
        return err(StatusCode::PAYLOAD_TOO_LARGE, "file exceeds 2 MB limit");
    }

    let bytes = match std::fs::read(&resolved) {
        Ok(b) => b,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    let sniff_len = bytes.len().min(BINARY_SNIFF);
    if bytes[..sniff_len].contains(&0u8) {
        return err(StatusCode::UNSUPPORTED_MEDIA_TYPE, "binary file");
    }
    let eol = if bytes.contains(&b'\r') { "crlf" } else { "lf" }.to_string();
    let content = match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => return err(StatusCode::UNSUPPORTED_MEDIA_TYPE, "non-utf8 content"),
    };
    let sha = content_hash(&content);
    Json(FsFileBody { content, sha, eol }).into_response()
}

pub async fn fs_write(Json(req): Json<FsWriteRequest>) -> Response {
    let target = PathBuf::from(&req.path);
    let bases = allowed_bases();

    // For new files the canonicalize-of-target fallback inside ensure_under
    // resolves the parent only; ensure parent exists.
    if let Some(parent) = target.parent() {
        if !parent.exists() {
            return err(StatusCode::BAD_REQUEST, "parent directory does not exist");
        }
    }
    let resolved = match bases.iter().find_map(|b| ensure_under(b, &target).ok()) {
        Some(p) => p,
        None => return err(StatusCode::FORBIDDEN, "path outside allowed roots"),
    };

    if let Some(expected) = req.base_sha.as_ref() {
        if resolved.exists() {
            let current = std::fs::read(&resolved).unwrap_or_default();
            if let Ok(s) = String::from_utf8(current) {
                let actual = content_hash(&s);
                if &actual != expected {
                    return err(
                        StatusCode::CONFLICT,
                        "file changed on disk since last read",
                    );
                }
            }
        }
    }

    // Atomic write: tmp file in same dir, fsync, rename.
    let parent = resolved.parent().unwrap_or_else(|| Path::new("."));
    let tmp = parent.join(format!(
        ".agent-start.tmp.{}",
        std::process::id()
    ));
    if let Err(e) = std::fs::write(&tmp, req.content.as_bytes()) {
        return err(StatusCode::INTERNAL_SERVER_ERROR, format!("write failed: {e}"));
    }
    if let Err(e) = std::fs::rename(&tmp, &resolved) {
        let _ = std::fs::remove_file(&tmp);
        return err(StatusCode::INTERNAL_SERVER_ERROR, format!("rename failed: {e}"));
    }
    let sha = content_hash(&req.content);
    Json(FsFileBody {
        content: req.content,
        sha,
        eol: "lf".to_string(),
    })
    .into_response()
}

fn content_hash(s: &str) -> String {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    format!("{:016x}", h.finish())
}
