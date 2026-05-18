//! Per-session window endpoints. A "window" is a single PTY inside a
//! session; each browser tab maps to one. The first window (index 0)
//! is created by `POST /api/sessions` and is bound to the session
//! lifetime — closing it kills the session.

use super::err;
use agent_start_api::{NewWindowResponse, WindowInfo, WindowsBody};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use pty_manager::PtySpawnSpec;
use serde_json::json;
use std::path::PathBuf;

use crate::app::Shared;

pub async fn list_windows(State(app): State<Shared>, Path(name): Path<String>) -> Response {
    if let Some(resp) = guard_session_name(&name) {
        return *resp;
    }
    let windows = app
        .pty
        .windows_for(&name)
        .into_iter()
        .map(|index| WindowInfo { index })
        .collect();
    Json(WindowsBody { windows }).into_response()
}

pub async fn create_window(State(app): State<Shared>, Path(name): Path<String>) -> Response {
    if let Some(resp) = guard_session_name(&name) {
        return *resp;
    }
    let dir = match app.sessions.read().get(&name).cloned() {
        Some(d) => d,
        None => return err(StatusCode::NOT_FOUND, "session not found"),
    };
    if !app.pty.has_window(&name, 0) {
        return err(StatusCode::NOT_FOUND, "session has no live window 0");
    }
    let cfg = match config_loader::load_config() {
        Ok(c) => c,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    let index = app.pty.next_window_index(&name);
    let cwd = PathBuf::from(if dir.worktree_path.is_empty() {
        dir.cwd.clone()
    } else {
        dir.worktree_path.clone()
    });
    let env = vec![
        (
            "AGENT_START_ROOT_PATH".to_string(),
            if dir.orig_path.is_empty() {
                dir.cwd.clone()
            } else {
                dir.orig_path.clone()
            },
        ),
        ("AGENT_START_WORKSPACE_NAME".to_string(), name.clone()),
        (
            "AGENT_START_WORKSPACE_PATH".to_string(),
            cwd.to_string_lossy().into_owned(),
        ),
        ("TERM".to_string(), "xterm-256color".into()),
    ];
    let spec = PtySpawnSpec {
        name: name.clone(),
        window: index,
        cwd,
        shell: cfg.shell.clone(),
        // Auxiliary windows always land at a plain interactive shell —
        // the agent CLI itself only runs in window 0.
        command: String::new(),
        env,
        cols: 80,
        rows: 24,
    };
    match app.pty.spawn(spec) {
        Ok(_) => Json(NewWindowResponse { index }).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

pub async fn delete_window(
    State(app): State<Shared>,
    Path((name, index)): Path<(String, String)>,
) -> Response {
    if let Some(resp) = guard_session_name(&name) {
        return *resp;
    }
    let Ok(window) = index.parse::<u32>() else {
        return err(StatusCode::BAD_REQUEST, "invalid window index");
    };
    if window == 0 {
        return err(
            StatusCode::BAD_REQUEST,
            "window 0 is the session's primary window; DELETE /api/sessions/<name> to end the session",
        );
    }
    match app.pty.remove_window(&name, window) {
        Some(w) => {
            w.kill();
            Json(json!({"ok": true})).into_response()
        }
        None => err(StatusCode::NOT_FOUND, "window not found"),
    }
}

fn guard_session_name(name: &str) -> Option<Box<Response>> {
    let cfg = match config_loader::load_config() {
        Ok(c) => c,
        Err(e) => {
            return Some(Box::new(err(
                StatusCode::INTERNAL_SERVER_ERROR,
                e.to_string(),
            )))
        }
    };
    if !workspace_manager::is_valid_session_name(name) || !name.starts_with(&cfg.session_prefix) {
        return Some(Box::new(err(
            StatusCode::BAD_REQUEST,
            "invalid session name",
        )));
    }
    None
}
