use agent_start_api::*;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use config_loader as cfg;
use pty_manager::PtySpawnSpec;
use serde::Deserialize;
use std::path::PathBuf;
use workspace_manager as wm;

use crate::app::Shared;
use crate::sessions::SessionDirectory;

pub async fn health() -> Json<HealthBody> {
    Json(HealthBody { ok: true })
}

pub async fn version() -> Json<VersionBody> {
    Json(VersionBody {
        name: "agent-start-host".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    })
}

fn err(status: StatusCode, msg: impl Into<String>) -> Response {
    (status, Json(ErrorBody::new(msg.into()))).into_response()
}

pub async fn get_config() -> Response {
    match cfg::load_config() {
        Ok(c) => {
            let clis: Vec<CliInfo> = c
                .clis
                .iter()
                .map(|(key, conf)| CliInfo {
                    key: key.clone(),
                    label: conf.label.clone().unwrap_or_else(|| key.clone()),
                    command: conf.command.clone(),
                    has_skip_flag: conf.skip_permissions_flag.is_some(),
                    skip_flag: conf.skip_permissions_flag.clone().unwrap_or_default(),
                })
                .collect();
            Json(ConfigBody {
                clis,
                default_cli: c.default_cli,
                session_prefix: c.session_prefix,
            })
            .into_response()
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

pub async fn get_preferences() -> Response {
    match cfg::load_preferences() {
        Ok(p) => Json(PreferencesBody {
            preferences: Preferences {
                cli: p.cli,
                skip_permissions: p.skip_permissions,
                extra_args: p.extra_args,
            },
        })
        .into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

pub async fn put_preferences(Json(body): Json<PreferencesPatch>) -> Response {
    let config = match cfg::load_config() {
        Ok(c) => c,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    let mut current = match cfg::load_preferences() {
        Ok(c) => c,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    if let Some(cli) = body.cli {
        if !config.clis.contains_key(&cli) {
            return err(StatusCode::BAD_REQUEST, format!("unknown cli: {cli}"));
        }
        current.cli = cli;
    }
    if let Some(skip) = body.skip_permissions {
        current.skip_permissions = skip;
    }
    if let Some(extra) = body.extra_args {
        match cfg::sanitize_extra_args(&extra) {
            Ok(v) => current.extra_args = v,
            Err(e) => return err(StatusCode::BAD_REQUEST, e.to_string()),
        }
    }
    if let Err(e) = cfg::save_preferences(&current) {
        return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    }
    Json(PreferencesBody {
        preferences: Preferences {
            cli: current.cli,
            skip_permissions: current.skip_permissions,
            extra_args: current.extra_args,
        },
    })
    .into_response()
}

pub async fn list_projects() -> Response {
    let config = match cfg::load_config() {
        Ok(c) => c,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    match wm::list_projects(&config) {
        Ok(projects) => Json(ProjectsBody { projects }).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
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

pub async fn start_session(
    State(app): State<Shared>,
    Json(body): Json<StartSessionRequest>,
) -> Response {
    if body.project_path.is_empty() {
        return err(StatusCode::BAD_REQUEST, "projectPath is required");
    }
    let config = match cfg::load_config() {
        Ok(c) => c,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    let prefs = match cfg::load_preferences() {
        Ok(p) => p,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    let resolved = PathBuf::from(&body.project_path);
    if !cfg::is_path_under_roots(&config, &resolved) {
        return err(
            StatusCode::BAD_REQUEST,
            "projectPath is outside configured roots",
        );
    }

    let cli_key = body.cli.clone().unwrap_or_else(|| {
        if !prefs.cli.is_empty() {
            prefs.cli.clone()
        } else {
            config.default_cli.clone()
        }
    });
    let Some(cli_conf) = config.clis.get(&cli_key) else {
        return err(StatusCode::BAD_REQUEST, format!("unknown cli: {cli_key}"));
    };

    let skip = body.skip_permissions.unwrap_or(prefs.skip_permissions);
    let extra_raw = body
        .extra_args
        .clone()
        .unwrap_or_else(|| prefs.extra_args.clone());
    let extra = match cfg::sanitize_extra_args(&extra_raw) {
        Ok(v) => v,
        Err(e) => return err(StatusCode::BAD_REQUEST, e.to_string()),
    };

    let create_wt = body.create_worktree.unwrap_or(false);
    if create_wt && !git_ops::is_git_repo(&resolved) {
        return err(
            StatusCode::BAD_REQUEST,
            "createWorktree requested but project is not a git repository",
        );
    }

    let command = match cfg::build_launch_command(cli_conf, skip, &extra) {
        Ok(c) => c,
        Err(e) => return err(StatusCode::BAD_REQUEST, e.to_string()),
    };
    let base_name = resolved
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "project".into());
    let name = wm::session_name(&config.session_prefix, &base_name);

    let mut cwd: PathBuf = resolved.clone();
    let mut worktree_path: Option<PathBuf> = None;
    if create_wt {
        match git_ops::create_worktree(&resolved, &name) {
            Ok(wt) => {
                cwd = wt.worktree_path.clone();
                worktree_path = Some(wt.worktree_path);
            }
            Err(e) => {
                return err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("worktree creation failed: {e}"),
                );
            }
        }
    }

    if cli_key == "claude" {
        let _ = wm::mark_claude_trusted(&cwd);
    }

    let env = vec![
        (
            "AGENT_START_ROOT_PATH".to_string(),
            resolved.to_string_lossy().into_owned(),
        ),
        ("AGENT_START_WORKSPACE_NAME".to_string(), name.clone()),
        (
            "AGENT_START_WORKSPACE_PATH".to_string(),
            cwd.to_string_lossy().into_owned(),
        ),
        ("TERM".to_string(), "xterm-256color".to_string()),
    ];

    let spec = PtySpawnSpec {
        name: name.clone(),
        cwd: cwd.clone(),
        shell: config.shell.clone(),
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
            worktree_path: wt_str.clone(),
            orig_path: orig_str.clone(),
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

#[derive(Debug, Deserialize)]
pub struct DeleteQuery {
    #[serde(rename = "deleteWorktree")]
    pub delete_worktree: Option<String>,
}

pub async fn delete_session(
    State(app): State<Shared>,
    Path(name): Path<String>,
    Query(q): Query<DeleteQuery>,
) -> Response {
    let config = match cfg::load_config() {
        Ok(c) => c,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    if !wm::is_valid_session_name(&name) || !name.starts_with(&config.session_prefix) {
        return err(StatusCode::BAD_REQUEST, "invalid session name");
    }
    let delete_wt = q.delete_worktree.as_deref() == Some("1");

    let dir = app.sessions.read().get(&name).cloned();

    if let Some(session) = app.pty.remove(&name) {
        session.kill();
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
