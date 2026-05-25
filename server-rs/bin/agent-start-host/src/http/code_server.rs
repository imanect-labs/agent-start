//! `POST /api/sessions/:name/code-server` — ensure a code-server child
//! is running for this session and return the URL the browser should
//! open. `DELETE` stops it. The actual HTTP/WS forwarding lives in the
//! `code_server_proxy` module.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use std::path::PathBuf;

use super::err;
use crate::app::Shared;

#[derive(Serialize)]
pub struct OpenResponse {
    pub url: String,
    pub port: u16,
}

pub async fn open_code_server(State(app): State<Shared>, Path(name): Path<String>) -> Response {
    let worktree = match resolve_worktree(&app, &name) {
        Some(p) => p,
        None => return err(StatusCode::NOT_FOUND, "session not found"),
    };

    let base = format!("/v/{name}");
    let instance = match app
        .code_server
        .ensure_with_base(&name, &worktree, Some(&base))
        .await
    {
        Ok(i) => i,
        Err(code_server_manager::CodeServerError::NotInstalled) => {
            return err(
                StatusCode::FAILED_DEPENDENCY,
                "code-server not found on PATH (install code-server or set AGENT_START_CODE_SERVER_BIN)",
            );
        }
        Err(e) => {
            tracing::warn!(error = %e, session = %name, "code-server spawn failed");
            return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
        }
    };

    if let Err(e) = state::insert_code_server(
        &app.db,
        &name,
        instance.port() as i64,
        instance.pid() as i64,
    )
    .await
    {
        tracing::warn!(error = %e, session = %name, "failed to persist code-server row");
    }

    // Pass `?folder=` so code-server explicitly opens the worktree on
    // load — without it, the client sometimes restores a different
    // workspace from window state instead of the one we spawned with.
    let folder = urlencode(&worktree.to_string_lossy());
    Json(OpenResponse {
        url: format!("/v/{name}/?folder={folder}"),
        port: instance.port(),
    })
    .into_response()
}

/// Minimal RFC 3986 query-component percent-encoder. Avoids pulling in
/// the `urlencoding` crate just for one call site.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub async fn close_code_server(State(app): State<Shared>, Path(name): Path<String>) -> Response {
    app.code_server.kill(&name).await;
    let _ = state::delete_code_server(&app.db, &name).await;
    (StatusCode::NO_CONTENT, ()).into_response()
}

fn resolve_worktree(app: &Shared, name: &str) -> Option<PathBuf> {
    let dirs = app.sessions.read();
    let d = dirs.get(name)?;
    if !d.worktree_path.is_empty() {
        Some(PathBuf::from(&d.worktree_path))
    } else if !d.cwd.is_empty() {
        Some(PathBuf::from(&d.cwd))
    } else {
        None
    }
}
