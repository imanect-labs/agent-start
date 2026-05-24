//! Session lifecycle endpoints — list / start / delete.
//!
//! `start_session` is the heaviest path in the host: it validates the
//! request, optionally creates a git worktree, marks the directory
//! Claude-trusted, spawns the PTY, and persists metadata to SQLite +
//! the in-memory directory. Worktree creation is rolled back if the
//! PTY spawn fails so we don't leak branches.

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
use std::path::{Path as StdPath, PathBuf};

use crate::app::Shared;
use crate::sessions::SessionDirectory;

/// Helper-return type. Boxing the `Response` keeps the
/// `clippy::result_large_err` lint happy (axum responses are ~128 B
/// and easily dwarf the `Ok` variant of these helpers).
type Erred<T> = Result<T, Box<Response>>;

fn bad(msg: impl Into<String>) -> Box<Response> {
    Box::new(err(StatusCode::BAD_REQUEST, msg))
}

fn boom(msg: impl Into<String>) -> Box<Response> {
    Box::new(err(StatusCode::INTERNAL_SERVER_ERROR, msg))
}

pub async fn list_sessions(State(app): State<Shared>) -> Response {
    let dirs = app.sessions.read();
    let mut out = Vec::with_capacity(dirs.len());
    for d in dirs.values() {
        let attached = app.pty.attached_count(&d.name) > 0;
        out.push(d.to_api(attached));
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
    let prepared = match prepare_start(&body) {
        Ok(p) => p,
        Err(resp) => return *resp,
    };

    let Prepared {
        cfg,
        cli_key,
        command,
        resolved,
        create_wt,
    } = prepared;

    let base_name = resolved
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "project".into());
    let name = workspace_manager::session_name(&cfg.session_prefix, &base_name);

    let (cwd, worktree_path) = match maybe_create_worktree(&resolved, &name, create_wt) {
        Ok(v) => v,
        Err(resp) => return *resp,
    };

    if cli_key == "claude" {
        let _ = workspace_manager::mark_claude_trusted(&cwd);
    }

    let env = launch_env(&resolved, &name, &cwd);
    let spec = PtySpawnSpec {
        name: name.clone(),
        window: 0,
        cwd: cwd.clone(),
        shell: cfg.shell.clone(),
        command: command.clone(),
        env,
        cols: 80,
        rows: 24,
    };

    let session = match app.pty.spawn(spec) {
        Ok(s) => s,
        Err(e) => {
            if let Some(wt) = worktree_path.as_deref() {
                let _ = git_ops::remove_worktree(wt, Some(&resolved), true);
            }
            return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
        }
    };

    let wt_str = worktree_path
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let orig_str = if worktree_path.is_some() {
        resolved.to_string_lossy().into_owned()
    } else {
        String::new()
    };

    if let Err(e) = state::insert_session(
        &app.db,
        state::NewSession {
            name: &name,
            cli: &cli_key,
            cwd: &cwd.to_string_lossy(),
            command: &command,
            worktree_path: &wt_str,
            orig_path: &orig_str,
            pid: session.pid().map(|v| v as i64),
        },
    )
    .await
    {
        tracing::warn!(error = %e, "failed to persist session metadata");
    }

    app.sessions.write().insert(
        name.clone(),
        SessionDirectory {
            name: name.clone(),
            created_at_ms: chrono::Utc::now().timestamp_millis(),
            cli: cli_key.clone(),
            cwd: cwd.to_string_lossy().into_owned(),
            worktree_path: wt_str,
            orig_path: orig_str,
            live: true,
        },
    );

    Json(StartSessionResponse {
        name,
        command,
        cli: cli_key,
        cwd: cwd.to_string_lossy().into_owned(),
        worktree_path: worktree_path.map(|p| p.to_string_lossy().into_owned()),
    })
    .into_response()
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

    for window in app.pty.remove_session(&name) {
        window.kill();
    }
    if let Err(e) = state::mark_dead(&app.db, &name).await {
        tracing::warn!(error = %e, "failed to mark session dead");
    }
    app.sessions.write().remove(&name);

    let mut worktree_removed = false;
    let mut worktree_error: Option<String> = None;
    if delete_wt {
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

    Json(DeleteSessionResponse {
        ok: true,
        worktree_removed,
        worktree_error,
    })
    .into_response()
}

/// Resolved + validated inputs needed to spawn a session.
struct Prepared {
    cfg: config_loader::Config,
    cli_key: String,
    command: String,
    resolved: PathBuf,
    create_wt: bool,
}

fn prepare_start(body: &StartSessionRequest) -> Erred<Prepared> {
    if body.project_path.is_empty() {
        return Err(bad("projectPath is required"));
    }
    let cfg = config_loader::load_config().map_err(|e| boom(e.to_string()))?;
    let prefs = config_loader::load_preferences().map_err(|e| boom(e.to_string()))?;

    let resolved = PathBuf::from(&body.project_path);
    if !config_loader::is_path_under_roots(&cfg, &resolved) {
        return Err(bad("projectPath is outside configured roots"));
    }

    let cli_key = body.cli.clone().unwrap_or_else(|| {
        if !prefs.cli.is_empty() {
            prefs.cli.clone()
        } else {
            cfg.default_cli.clone()
        }
    });
    let cli_conf = cfg
        .clis
        .get(&cli_key)
        .ok_or_else(|| bad(format!("unknown cli: {cli_key}")))?;

    let skip = body.skip_permissions.unwrap_or(prefs.skip_permissions);
    let extra_raw = body
        .extra_args
        .clone()
        .unwrap_or_else(|| prefs.extra_args.clone());
    let extra = config_loader::sanitize_extra_args(&extra_raw).map_err(|e| bad(e.to_string()))?;

    let create_wt = body.create_worktree.unwrap_or(false);
    if create_wt && !git_ops::is_git_repo(&resolved) {
        return Err(bad(
            "createWorktree requested but project is not a git repository",
        ));
    }

    let command = config_loader::build_launch_command(cli_conf, skip, &extra)
        .map_err(|e| bad(e.to_string()))?;

    Ok(Prepared {
        cfg,
        cli_key,
        command,
        resolved,
        create_wt,
    })
}

fn maybe_create_worktree(
    orig_path: &StdPath,
    session_name: &str,
    create: bool,
) -> Erred<(PathBuf, Option<PathBuf>)> {
    if !create {
        return Ok((orig_path.to_path_buf(), None));
    }
    match git_ops::create_worktree(orig_path, session_name) {
        Ok(wt) => Ok((wt.worktree_path.clone(), Some(wt.worktree_path))),
        Err(e) => Err(boom(format!("worktree creation failed: {e}"))),
    }
}

fn launch_env(orig: &StdPath, name: &str, cwd: &StdPath) -> Vec<(String, String)> {
    vec![
        (
            "AGENT_START_ROOT_PATH".into(),
            orig.to_string_lossy().into_owned(),
        ),
        ("AGENT_START_WORKSPACE_NAME".into(), name.to_string()),
        (
            "AGENT_START_WORKSPACE_PATH".into(),
            cwd.to_string_lossy().into_owned(),
        ),
        ("TERM".into(), "xterm-256color".into()),
    ]
}
