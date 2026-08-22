//! Session lifecycle endpoints — list / start / restart / delete.
//!
//! The work of starting a session lives in `crate::launch`, which the
//! task queue also uses. What is left here is the HTTP shape of it:
//! decoding the request, and turning a launch failure into a status
//! code.

use super::err;
use agent_start_api::{
    DeleteSessionResponse, SessionsBody, StartSessionRequest, StartSessionResponse,
};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use pty_manager::PtySpawnSpec;
use serde::Deserialize;
use std::path::PathBuf;

use crate::app::Shared;
use crate::launch::{launch, LaunchRequest};

pub async fn list_sessions(State(app): State<Shared>) -> Response {
    let mut out = {
        let dirs = app.sessions.read();
        let mut out = Vec::with_capacity(dirs.len());
        for d in dirs.values() {
            // A relayed session has no local PTY, so the local attach
            // count is only meaningful for sessions on this host.
            let attached = app.pty.attached_count(&d.name) > 0;
            out.push(d.to_api(attached));
        }
        out
    };
    // Resolve node ids to names for the UI badge. Nodes that have since
    // been removed keep their id, which is still better than nothing.
    if let Some(control) = app.cluster.as_ref() {
        let local = control.local_node_id();
        for s in &mut out {
            if s.node_id.is_empty() || Some(&s.node_id) == local.as_ref() {
                continue;
            }
            if let Some(view) = control.node(&s.node_id) {
                s.node_name = view.info.name;
            }
        }
    }
    out.sort_by_key(|s| std::cmp::Reverse(s.created_at));
    Json(SessionsBody { sessions: out }).into_response()
}

#[derive(Debug, Deserialize)]
pub struct DeleteQuery {
    #[serde(rename = "deleteWorktree")]
    pub delete_worktree: Option<String>,
}

pub async fn start_session(
    State(app): State<Shared>,
    Json(body): Json<StartSessionRequest>,
) -> Response {
    match launch(&app, LaunchRequest::interactive(body)).await {
        Ok(l) => Json(StartSessionResponse {
            name: l.name,
            command: l.command,
            cli: l.cli,
            cwd: l.cwd,
            worktree_path: l.worktree_path,
            node_id: l.node_id,
            node_name: l.node_name,
        })
        .into_response(),
        Err(e) => err(crate::http::tasks::status_for(&e), e.to_string()),
    }
}

/// POST `/api/sessions/:name/restart` — bring a session that was
/// rehydrated as `stopped` (its previous PTY died with the host) back
/// to life. Reuses the original cli/cwd/command from SQLite so the
/// user's open tabs reconnect to a fresh PTY transparently. Returns
/// 409 if the session is already live, 404 if it's unknown, 410 if
/// the worktree was deleted out from under us.
pub async fn restart_session(State(app): State<Shared>, Path(name): Path<String>) -> Response {
    if !workspace_manager::is_valid_session_name(&name) {
        return err(StatusCode::BAD_REQUEST, "invalid session name");
    }
    let existing = app.sessions.read().get(&name).cloned();
    let Some(dir) = existing else {
        return err(StatusCode::NOT_FOUND, "session not found");
    };
    if dir.live {
        return err(StatusCode::CONFLICT, "session is already running");
    }
    if !is_local_session(&app, &name) {
        // Restart re-spawns a PTY from this host's filesystem, which is
        // meaningless for a worktree that lives on another machine.
        // Cross-node restart arrives with the task queue in Phase 2.
        return err(
            StatusCode::CONFLICT,
            "this session ran on another node; start a new session instead",
        );
    }

    let row = match state::get_session(&app.db, &name).await {
        Ok(Some(r)) => r,
        Ok(None) => return err(StatusCode::NOT_FOUND, "session metadata missing"),
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    let cwd = PathBuf::from(&row.cwd);
    if !cwd.exists() {
        return err(
            StatusCode::GONE,
            format!("cwd no longer exists: {}", cwd.display()),
        );
    }
    let cfg = match config_loader::load_config() {
        Ok(c) => c,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    let orig = if row.orig_path.is_empty() {
        cwd.clone()
    } else {
        PathBuf::from(&row.orig_path)
    };
    let env = crate::sessions::launch_env(&orig, &name, &cwd);
    let spec = PtySpawnSpec {
        name: name.clone(),
        window: 0,
        cwd: cwd.clone(),
        shell: cfg.shell.clone(),
        command: row.command.clone(),
        env,
        cols: 80,
        rows: 24,
    };
    let saved_history = dir.history.clone();
    let session = match app.pty.spawn(spec) {
        Ok(s) => s,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    // Seed the new PTY's ring buffer with the persisted scrollback so
    // the first WS subscriber sees previous-session output above the
    // fresh prompt. Without this, restarting a TUI session (Claude,
    // vim, etc.) flashes the snapshot away the instant the new shell
    // paints — the user reads it as "再開でリセットされた".
    if !saved_history.is_empty() {
        session.seed_history(&saved_history);
        // Separator so the user can see where the previous host's
        // output ended and the new shell began.
        session
            .seed_history(b"\r\n\x1b[2m-- restarted: previous session output above --\x1b[0m\r\n");
    }

    if let Err(e) = state::mark_running(&app.db, &name, session.pid().map(|v| v as i64)).await {
        tracing::warn!(error = %e, "failed to mark session running");
    }
    if let Some(d) = app.sessions.write().get_mut(&name) {
        d.live = true;
        // The seeded ring is now the source of truth for replay; clear
        // the SessionDirectory copy so a future stop doesn't double-feed it.
        d.history.clear();
    }

    Json(StartSessionResponse {
        name: name.clone(),
        command: row.command,
        cli: row.cli,
        cwd: row.cwd,
        worktree_path: if row.worktree_path.is_empty() {
            None
        } else {
            Some(row.worktree_path)
        },
        node_id: row.node_id,
        node_name: String::new(),
    })
    .into_response()
}

/// True when the session's PTY lives in this process. Sessions started
/// before the cluster layer, and everything on the in-process node,
/// answer yes — which is what keeps the single-host paths unchanged.
pub(crate) fn is_local_session(app: &Shared, name: &str) -> bool {
    match app.cluster.as_ref() {
        Some(control) => control.is_local_session(name),
        None => true,
    }
}

pub async fn delete_session(
    State(app): State<Shared>,
    Path(name): Path<String>,
    Query(q): Query<DeleteQuery>,
) -> Response {
    let cfg = match config_loader::load_config() {
        Ok(c) => c,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    if !workspace_manager::is_valid_session_name(&name) || !name.starts_with(&cfg.session_prefix) {
        return err(StatusCode::BAD_REQUEST, "invalid session name");
    }
    let delete_wt = q.delete_worktree.as_deref() == Some("1");

    let dir = app.sessions.read().get(&name).cloned();
    let local = is_local_session(&app, &name);

    // A session on another node is torn down by that node: it owns the
    // PTY and the worktree. Everything below this point is host-local
    // bookkeeping that applies either way.
    let mut remote_cancel_delivered = true;
    if !local {
        if let Some(control) = app.cluster.as_ref() {
            remote_cancel_delivered = control.cancel_session(&name, delete_wt).await;
        }
    }

    for window in app.pty.remove_session(&name) {
        window.kill();
    }
    // Tear down the chat conversation (if this is a chat session). Its
    // transcript rows cascade-delete with the session row below.
    app.chat.remove(&name);
    app.code_server.kill(&name).await;
    let _ = state::delete_code_server(&app.db, &name).await;
    if let Err(e) = state::mark_dead(&app.db, &name).await {
        tracing::warn!(error = %e, "failed to mark session dead");
    }
    app.sessions.write().remove(&name);

    let mut worktree_removed = false;
    let mut worktree_error: Option<String> = None;
    if delete_wt && !local {
        // The node removes the worktree asynchronously and does not
        // acknowledge completion yet (that arrives with the task
        // queue), so all we can honestly report is whether the request
        // reached it at all.
        if remote_cancel_delivered {
            worktree_removed = true;
        } else {
            worktree_error = Some(
                "the node running this session is unreachable; its worktree was left in place"
                    .to_string(),
            );
        }
    }
    if delete_wt && local {
        if let Some(d) = dir.as_ref() {
            if !d.worktree_path.is_empty() {
                let wt = PathBuf::from(&d.worktree_path);
                let orig = if d.orig_path.is_empty() {
                    None
                } else {
                    Some(PathBuf::from(&d.orig_path))
                };
                match git_ops::remove_worktree(&wt, orig.as_deref(), true) {
                    Ok(()) => worktree_removed = true,
                    Err(e) => worktree_error = Some(e.to_string()),
                }
            }
        }
    }
    let _ = state::delete_session(&app.db, &name).await;
    let _ = state::delete_pty_snapshots(&app.db, &name).await;

    Json(DeleteSessionResponse {
        ok: true,
        worktree_removed,
        worktree_error,
    })
    .into_response()
}
