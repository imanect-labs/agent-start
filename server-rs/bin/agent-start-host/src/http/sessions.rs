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
        is_chat,
        extra,
        prompt,
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

    // Both the PTY `claude` and the chat `claude` run the same binary, so
    // mark the worktree Claude-trusted for either.
    if cli_key == "claude" || is_chat {
        let _ = workspace_manager::mark_claude_trusted(&cwd);
    }

    if is_chat {
        return start_chat_session(
            &app,
            StartChatArgs {
                cfg: &cfg,
                name: &name,
                cli_key: &cli_key,
                command: &command,
                resolved: &resolved,
                cwd: &cwd,
                worktree_path: worktree_path.as_deref(),
                extra: &extra,
                prompt: prompt.as_deref(),
            },
        )
        .await;
    }

    let env = crate::sessions::launch_env(&resolved, &name, &cwd);
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
            history: Vec::new(),
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

/// Inputs for `start_chat_session`, grouped to keep the arg list sane.
struct StartChatArgs<'a> {
    cfg: &'a config_loader::Config,
    name: &'a str,
    cli_key: &'a str,
    command: &'a str,
    resolved: &'a StdPath,
    cwd: &'a StdPath,
    worktree_path: Option<&'a StdPath>,
    extra: &'a str,
    prompt: Option<&'a str>,
}

/// Spawn a headless chat conversation (#34) instead of a PTY. Mirrors the
/// PTY path: persist the session row + in-memory directory, roll back the
/// worktree if the process fails to start. Chat always runs with
/// skip-permissions (decision 7).
async fn start_chat_session(app: &Shared, args: StartChatArgs<'_>) -> Response {
    let StartChatArgs {
        cfg,
        name,
        cli_key,
        command,
        resolved,
        cwd,
        worktree_path,
        extra,
        prompt,
    } = args;

    let cli_conf = match cfg.clis.get(cli_key) {
        Some(c) => c,
        None => return err(StatusCode::BAD_REQUEST, format!("unknown cli: {cli_key}")),
    };
    let env = crate::sessions::launch_env(resolved, name, cwd);
    let spec = chat_manager::ChatSpawnSpec {
        name: name.to_string(),
        cwd: cwd.to_path_buf(),
        shell: cfg.shell.clone(),
        command: cli_conf.command.clone(),
        skip_permissions_flag: cli_conf.skip_permissions_flag.clone(),
        extra_args: extra.to_string(),
        env,
        model: cfg.chat.default_model.clone(),
        resume: None,
        start_seq: 0,
    };

    let session = app.chat.insert_dormant(spec);
    crate::chat::attach_persistence(app.clone(), session.clone());
    if let Err(e) = session.start().await {
        app.chat.remove(name);
        if let Some(wt) = worktree_path {
            let _ = git_ops::remove_worktree(wt, Some(resolved), true);
        }
        return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    }

    // Forward an initial prompt as the first user turn (e.g. launching
    // from a GitHub issue).
    if let Some(prompt) = prompt {
        if let Err(e) = session.send_user_message(prompt, &[]).await {
            tracing::warn!(error = %e, session = %name, "failed to send initial chat prompt");
        }
    }

    let wt_str = worktree_path
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
            name,
            cli: cli_key,
            cwd: &cwd.to_string_lossy(),
            command,
            worktree_path: &wt_str,
            orig_path: &orig_str,
            pid: None,
        },
    )
    .await
    {
        tracing::warn!(error = %e, "failed to persist chat session metadata");
    }

    app.sessions.write().insert(
        name.to_string(),
        SessionDirectory {
            name: name.to_string(),
            created_at_ms: chrono::Utc::now().timestamp_millis(),
            cli: cli_key.to_string(),
            cwd: cwd.to_string_lossy().into_owned(),
            worktree_path: wt_str,
            orig_path: orig_str,
            live: true,
            history: Vec::new(),
        },
    );

    Json(StartSessionResponse {
        name: name.to_string(),
        command: command.to_string(),
        cli: cli_key.to_string(),
        cwd: cwd.to_string_lossy().into_owned(),
        worktree_path: worktree_path.map(|p| p.to_string_lossy().into_owned()),
    })
    .into_response()
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
    let _ = state::delete_pty_snapshots(&app.db, &name).await;

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
    /// True when the selected CLI runs in headless chat mode (#34).
    is_chat: bool,
    /// Sanitized extra args (without the appended prompt).
    extra: String,
    /// Initial prompt (trimmed + capped), if any.
    prompt: Option<String>,
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

    let is_chat = cli_conf.is_chat();

    // Capped initial prompt, shared by both launch paths.
    let prompt = body.prompt.as_deref().and_then(|p| {
        let trimmed = p.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.chars().take(MAX_PROMPT_CHARS).collect::<String>())
        }
    });

    // Chat mode never builds a PTY command line; it spawns the headless
    // process from its components in `start_session`. The stored `command`
    // is just a human-readable descriptor.
    let command = if is_chat {
        format!("{} (chat)", cli_conf.command)
    } else {
        let mut command = config_loader::build_launch_command(cli_conf, skip, &extra)
            .map_err(|e| bad(e.to_string()))?;
        // An initial prompt (e.g. launching from a GitHub issue) is handed
        // to the agent CLI as a positional argument. Skip it for the
        // bare-shell CLI (empty command) which has no prompt argument.
        if let Some(prompt) = &prompt {
            if !command.is_empty() {
                command.push(' ');
                command.push_str(&shell_single_quote(prompt));
            }
        }
        command
    };

    Ok(Prepared {
        cfg,
        cli_key,
        command,
        resolved,
        create_wt,
        is_chat,
        extra,
        prompt,
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

/// Upper bound on the initial-prompt length we forward to the CLI. Issue
/// bodies can be long; this keeps the spawned command line well within
/// `ARG_MAX` while preserving enough context to be useful.
const MAX_PROMPT_CHARS: usize = 8000;

/// Wrap `s` in single quotes for safe inclusion in the `bash -lc <cmd>`
/// command string, escaping embedded single quotes as `'\''`. The result
/// is passed verbatim to the CLI as one positional argument.
fn shell_single_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

#[cfg(test)]
mod tests {
    use super::shell_single_quote;

    #[test]
    fn quotes_plain_text() {
        assert_eq!(shell_single_quote("hello world"), "'hello world'");
    }

    #[test]
    fn escapes_embedded_single_quotes() {
        // bash sees: 'it'\''s' -> concatenated literal `it's`
        assert_eq!(shell_single_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn neutralizes_shell_metacharacters() {
        let q = shell_single_quote("$(rm -rf /); `whoami` && echo \"x\"");
        // Everything stays inside one quoted span (no unescaped quote breaks out).
        assert!(q.starts_with('\''));
        assert!(q.ends_with('\''));
        assert!(q.contains("$(rm -rf /)"));
    }
}
