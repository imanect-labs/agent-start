use anyhow::Result;
use axum::http::{header, StatusCode, Uri};
use axum::response::IntoResponse;
use axum::routing::{any, delete, get, post, put};
use axum::Router;
use chat_manager::ChatManager;
use code_server_manager::CodeServerManager;
use novnc_manager::NovncManager;
use parking_lot::RwLock;
use pty_manager::PtyManager;
use rust_embed::RustEmbed;
use state::Db;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

/// SPA assets compiled into the host binary. Source is `front/dist/`, populated
/// by `vp build` (or by the build.rs placeholder when missing).
#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../../../front/dist"]
struct FrontAssets;

use crate::manifest;
use crate::sessions::SessionDirectory;

pub struct AppState {
    pub db: Db,
    pub pty: Arc<PtyManager>,
    pub chat: Arc<ChatManager>,
    pub code_server: Arc<CodeServerManager>,
    pub novnc: Arc<NovncManager>,
    /// In-memory mirror of session metadata. Persisted on insert via SQLite.
    pub sessions: Arc<RwLock<HashMap<String, SessionDirectory>>>,
    /// Cached result of the last GitHub Releases check, with the instant it
    /// was taken. Lets `/v1/update-check` answer cheaply (and avoid hammering
    /// the GitHub API) when polled by the UI/CLI. See `http::meta`.
    pub update_cache: RwLock<Option<(std::time::Instant, agent_start_api::UpdateCheckBody)>>,
}

pub type Shared = Arc<AppState>;

pub async fn run(bind: String, port: u16, frontend_dist: Option<PathBuf>) -> Result<()> {
    // Move legacy XDG files into ~/.agent-start/ on first boot of a new
    // build. No-op when the env overrides are set or files already exist
    // at the destination.
    if let Err(e) = config_loader::migrate_legacy_layout() {
        tracing::warn!(error = %e, "legacy layout migration encountered an error (continuing)");
    }

    // Ensure the default projects directory exists before any config load
    // so first-run users immediately see a valid roots entry.
    let projects = config_loader::projects_dir();
    if let Err(e) = std::fs::create_dir_all(&projects) {
        tracing::warn!(error = %e, path = %projects.display(), "failed to create projects dir");
    }
    // If existing config has no projects-dir entry in `roots`, add it and
    // persist. Keeps user-customized additional roots intact.
    if let Ok(mut cfg) = config_loader::load_config() {
        let projects_str = projects.to_string_lossy().into_owned();
        let has = cfg.roots.iter().any(|r| {
            let p = config_loader::expand_root(r);
            p == projects || p.to_string_lossy() == projects_str || r.as_str() == projects_str
        });
        if !has {
            cfg.roots.insert(0, projects_str);
            if let Err(e) = config_loader::save_config(&cfg) {
                tracing::warn!(error = %e, "failed to persist projects-dir root");
            }
        }
    }

    let db = state::open().await?;
    let pty = Arc::new(PtyManager::new());
    let chat = Arc::new(ChatManager::new());
    let code_server = Arc::new(CodeServerManager::new());
    let novnc = Arc::new(NovncManager::new());
    let sessions = Arc::new(RwLock::new(HashMap::new()));

    // Stale rows from the previous boot point at dead children/ports.
    // Wipe them now so the proxy never tries to forward to a phantom
    // port; live entries are recreated as users click "VSCode で開く".
    if let Err(e) = state::clear_code_server(&db).await {
        tracing::warn!(error = %e, "failed to clear stale code-server rows");
    }

    // PTYs are in-process state; they cannot survive a host restart.
    // Any rows still flagged `running` in SQLite from a previous boot
    // are zombies — their `agent-start-host` is gone and reconnecting
    // to them just 404s. Mark them dead so `GET /api/sessions` shows
    // a clean slate. Worktree dirs on disk are preserved either way.
    if let Err(e) = state::mark_all_running_dead(&db).await {
        tracing::warn!(error = %e, "failed to mark prior running sessions dead");
    }

    // Rehydrate every session row whose backing directory (worktree or
    // cwd) is still on disk. The PTY is gone but the session should
    // stay visible (marked "stopped") so the user keeps all their open
    // tabs and can either delete cleanly or just look at the scrollback.
    if let Ok(rows) = state::list_all_sessions(&db).await {
        let mut map: HashMap<String, SessionDirectory> = HashMap::new();
        for row in rows {
            let probe = if !row.worktree_path.is_empty() {
                row.worktree_path.as_str()
            } else if !row.cwd.is_empty() {
                row.cwd.as_str()
            } else {
                continue;
            };
            if !std::path::Path::new(probe).exists() {
                continue;
            }
            let history = match state::load_pty_snapshot(&db, &row.name, 0).await {
                Ok(Some(h)) => {
                    tracing::info!(session = %row.name, bytes = h.len(), "rehydrated snapshot");
                    h
                }
                Ok(None) => {
                    tracing::info!(session = %row.name, "no snapshot in db");
                    Vec::new()
                }
                Err(e) => {
                    tracing::warn!(error = %e, session = %row.name, "failed to load snapshot");
                    Vec::new()
                }
            };
            map.insert(
                row.name.clone(),
                SessionDirectory {
                    name: row.name,
                    created_at_ms: row.created_at_ms,
                    cli: row.cli,
                    cwd: row.cwd,
                    worktree_path: row.worktree_path,
                    orig_path: row.orig_path,
                    live: false,
                    history,
                },
            );
        }
        if !map.is_empty() {
            tracing::info!(count = map.len(), "rehydrated stopped sessions from disk");
            *sessions.write() = map;
        }
    }

    let app_state: Shared = Arc::new(AppState {
        db,
        pty,
        chat,
        code_server,
        novnc,
        sessions,
        update_cache: RwLock::new(None),
    });

    // When a child process exits on its own (user types `exit`, the
    // agent finishes, etc.) drop the in-memory entry and mark the row
    // dead in SQLite so `GET /api/sessions` stops listing it. Only
    // window 0 represents the session itself; auxiliary windows just
    // vanish from `/windows` without ending the session.
    {
        let state_for_hook = app_state.clone();
        app_state
            .pty
            .set_exit_hook(Arc::new(move |name: &str, window: u32| {
                if window != 0 {
                    return;
                }
                let state = state_for_hook.clone();
                let name = name.to_string();
                tokio::spawn(async move {
                    state.sessions.write().remove(&name);
                    // Kill any noVNC backend tied to this session so Xvnc
                    // and websockify don't linger as zombies after the
                    // PTY they were paired with dies.
                    state.novnc.kill(&name).await;
                    if let Err(e) = state::mark_dead(&state.db, &name).await {
                        tracing::warn!(error = %e, session = %name, "failed to mark dead");
                    }
                });
            }));
    }

    // Chat conversations (#34) that crash or exit unexpectedly stay
    // visible as `dead`: the transcript is browsable and the next
    // `user_message` revives them via `--resume` (decision 12). Unlike the
    // PTY hook we do NOT remove the directory entry.
    {
        let state_for_chat_hook = app_state.clone();
        app_state.chat.set_exit_hook(Arc::new(move |name: &str| {
            let state = state_for_chat_hook.clone();
            let name = name.to_string();
            tokio::spawn(async move {
                if let Some(d) = state.sessions.write().get_mut(&name) {
                    d.live = false;
                }
                if let Err(e) = state::mark_dead(&state.db, &name).await {
                    tracing::warn!(error = %e, session = %name, "failed to mark chat dead");
                }
            });
        }));
    }

    // Rehydrate chat sessions as dormant so their transcript is browsable
    // after a restart and the first send can `--resume` the conversation.
    if let Ok(cfg) = config_loader::load_config() {
        if let Ok(rows) = state::list_all_sessions(&app_state.db).await {
            for row in rows {
                let is_chat = cfg.clis.get(&row.cli).map(|c| c.is_chat()).unwrap_or(false);
                if !is_chat {
                    continue;
                }
                // Only rehydrate sessions whose directory survived (those
                // present in the in-memory map populated above).
                if !app_state.sessions.read().contains_key(&row.name) {
                    continue;
                }
                let cli_conf = cfg.clis.get(&row.cli);
                let cwd = if !row.worktree_path.is_empty() {
                    row.worktree_path.clone()
                } else {
                    row.cwd.clone()
                };
                let orig = if row.orig_path.is_empty() {
                    cwd.clone()
                } else {
                    row.orig_path.clone()
                };
                let start_seq = state::next_chat_seq(&app_state.db, &row.name)
                    .await
                    .unwrap_or(0);
                let spec = chat_manager::ChatSpawnSpec {
                    name: row.name.clone(),
                    cwd: std::path::PathBuf::from(&cwd),
                    shell: cfg.shell.clone(),
                    command: cli_conf
                        .map(|c| c.command.clone())
                        .unwrap_or_else(|| "claude".into()),
                    skip_permissions_flag: cli_conf.and_then(|c| c.skip_permissions_flag.clone()),
                    extra_args: String::new(),
                    env: crate::sessions::launch_env(
                        std::path::Path::new(&orig),
                        &row.name,
                        std::path::Path::new(&cwd),
                    ),
                    model: cfg.chat.default_model.clone(),
                    resume: if row.claude_session_id.is_empty() {
                        None
                    } else {
                        Some(row.claude_session_id.clone())
                    },
                    start_seq,
                };
                let session = app_state.chat.insert_dormant(spec);
                crate::chat::attach_persistence(app_state.clone(), session);
            }
        }
    }

    // Periodic flusher: every 5s snapshot every live PTY's ring buffer
    // into SQLite. On a restart `list_all_sessions` + `load_pty_snapshot`
    // reconstructs the scrollback so users see their last terminal state
    // instead of an empty pane.
    {
        let state_for_flush = app_state.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(5));
            tick.tick().await; // skip the immediate first tick
            loop {
                tick.tick().await;
                let snapshots = state_for_flush.pty.snapshot_all();
                for (name, window, bytes) in snapshots {
                    if bytes.is_empty() {
                        continue;
                    }
                    if let Err(e) =
                        state::save_pty_snapshot(&state_for_flush.db, &name, window as i64, &bytes)
                            .await
                    {
                        tracing::debug!(error = %e, session = %name, window, "snapshot flush failed");
                    }
                }
            }
        });
    }

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let api_router = Router::new()
        // Versioned (Rust-native) endpoints.
        .route("/v1/health", get(crate::http::health))
        .route("/v1/version", get(crate::http::version))
        .route("/v1/update-check", get(crate::http::update_check))
        // `/api/*` surface consumed by the Vite+ SPA under `/front/`.
        .route(
            "/api/config",
            get(crate::http::get_config).put(crate::http::put_config),
        )
        .route("/api/preferences", get(crate::http::get_preferences))
        .route("/api/preferences", put(crate::http::put_preferences))
        .route("/api/projects", get(crate::http::list_projects))
        .route(
            "/api/projects/clone",
            axum::routing::post(crate::http::clone_project),
        )
        .route(
            "/api/projects/import",
            axum::routing::post(crate::http::import_project),
        )
        .route("/api/projects/{name}", delete(crate::http::delete_project))
        .route("/api/sessions", get(crate::http::list_sessions))
        .route(
            "/api/sessions",
            axum::routing::post(crate::http::start_session),
        )
        .route("/api/sessions/{name}", delete(crate::http::delete_session))
        .route(
            "/api/sessions/{name}/restart",
            axum::routing::post(crate::http::restart_session),
        )
        .route(
            "/api/sessions/{name}/windows",
            get(crate::http::list_windows).post(crate::http::create_window),
        )
        .route(
            "/api/sessions/{name}/windows/{index}",
            delete(crate::http::delete_window),
        )
        .route("/api/fs/tree", get(crate::http::fs_tree))
        .route(
            "/api/fs/file",
            get(crate::http::fs_read).put(crate::http::fs_write),
        )
        .route("/api/git/status", get(crate::http::git_status))
        .route("/api/git/diff", get(crate::http::git_diff))
        .route("/api/projects/issues", get(crate::http::list_issues))
        .route("/api/projects/issue", get(crate::http::view_issue))
        .route("/api/git/stage", post(crate::http::git_stage))
        .route("/api/git/unstage", post(crate::http::git_unstage))
        .route("/api/git/commit", post(crate::http::git_commit))
        .route("/api/git/discard", post(crate::http::git_discard))
        .route(
            "/api/git/branches",
            get(crate::http::git_branches).post(crate::http::git_create_branch),
        )
        .route(
            "/api/git/branches/delete",
            post(crate::http::git_delete_branch),
        )
        .route("/api/git/checkout", post(crate::http::git_checkout))
        .route("/api/git/fetch", post(crate::http::git_fetch))
        .route("/api/git/pull", post(crate::http::git_pull))
        .route("/api/git/push", post(crate::http::git_push))
        .route("/api/git/log", get(crate::http::git_log))
        .route("/api/git/tree", get(crate::http::git_tree))
        .route(
            "/api/sessions/{name}/code-server",
            axum::routing::post(crate::http::open_code_server)
                .delete(crate::http::close_code_server),
        )
        // Reverse proxy to code-server child process. Captures both the
        // root and any sub-path so VSCode's static assets, API, and WS
        // upgrades all route to the right child.
        .route(
            "/v/{name}",
            any(crate::http::code_server_proxy::proxy_handler),
        )
        .route(
            "/v/{name}/",
            any(crate::http::code_server_proxy::proxy_handler),
        )
        .route(
            "/v/{name}/{*rest}",
            any(crate::http::code_server_proxy::proxy_handler),
        )
        .route(
            "/api/sessions/{name}/novnc",
            axum::routing::post(crate::http::open_novnc).delete(crate::http::close_novnc),
        )
        // Reverse proxy to per-session websockify child. Same shape as
        // the code-server proxy above: root, root-slash, and wildcard.
        .route("/vnc/{name}", any(crate::http::novnc_proxy::proxy_handler))
        .route("/vnc/{name}/", any(crate::http::novnc_proxy::proxy_handler))
        .route(
            "/vnc/{name}/{*rest}",
            any(crate::http::novnc_proxy::proxy_handler),
        )
        // WebSocket — same URL the UI already uses.
        .route("/ws/terminal", get(crate::ws::ws_terminal))
        // Chat-mode WebSocket (#34).
        .route("/ws/chat", get(crate::ws_chat::ws_chat))
        .with_state(app_state.clone());

    // SPA wiring. API routes already registered above take precedence.
    // For everything else:
    //   * `/assets/*` -> hashed JS/CSS chunks.
    //   * any other path -> serve `index.html` with 200 so TanStack Router
    //     can pick up the client-side route after hydration.
    //
    // Source precedence:
    //   1. If `--frontend-dist` is set, serve from that filesystem path
    //      (useful for dev / staging without rebuilding the host).
    //   2. Otherwise serve assets embedded into the host binary at build
    //      time via `rust-embed` (default for distributed binaries).
    //
    // Both branches explicitly 404 API/WS prefix typos rather than
    // serving them HTML — otherwise `/api/typo` would return 200 with
    // index.html and silently mask broken client integrations.
    let router = if let Some(dist) = frontend_dist {
        let index_path = dist.join("index.html");
        let assets_dir = dist.join("assets");
        tracing::info!(path = %dist.display(), "serving front-end SPA from dist (filesystem override)");

        let serve_index = any(move |uri: Uri| {
            let index_path = index_path.clone();
            async move {
                if is_api_path(uri.path()) {
                    return StatusCode::NOT_FOUND.into_response();
                }
                match tokio::fs::read(&index_path).await {
                    Ok(body) => (
                        StatusCode::OK,
                        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                        body,
                    )
                        .into_response(),
                    Err(e) => {
                        tracing::error!(error = %e, path = %index_path.display(), "failed to read index.html");
                        StatusCode::INTERNAL_SERVER_ERROR.into_response()
                    }
                }
            }
        });

        api_router
            .nest_service("/assets", ServeDir::new(&assets_dir))
            .fallback_service(serve_index)
    } else {
        tracing::info!("serving front-end SPA from embedded assets");
        api_router
            .route("/assets/{*path}", get(serve_embedded_asset))
            .fallback(serve_embedded_index)
    };

    let router = router.layer(cors).layer(TraceLayer::new_for_http());

    let addr = format!("{bind}:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("agent-start-host listening on http://{addr}");

    manifest::write(&addr).ok();

    // Spawn a Ctrl-C watcher that flushes snapshots and hard-exits.
    // Graceful axum shutdown blocks indefinitely on live WebSocket
    // connections (the browser holds the terminal sockets open with
    // no FIN), so the user would otherwise have to ^Z the process —
    // which prevents any future periodic flush from running and loses
    // up to the last 5s of terminal output. Hard-exit is fine here
    // because every piece of mutable state worth keeping (sessions,
    // PTY snapshots, scrollback) is in SQLite by the time we return.
    let shutdown_state = app_state.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        tracing::info!("shutdown signal received; killing noVNC children");
        shutdown_state.novnc.kill_all().await;
        tracing::info!("flushing PTY snapshots");
        let snapshots = shutdown_state.pty.snapshot_all();
        tracing::info!(count = snapshots.len(), "snapshot_all collected");
        for (name, window, bytes) in snapshots {
            if bytes.is_empty() {
                tracing::info!(session = %name, window, "skipping empty snapshot");
                continue;
            }
            match state::save_pty_snapshot(&shutdown_state.db, &name, window as i64, &bytes).await {
                Ok(()) => tracing::info!(
                    session = %name,
                    window,
                    bytes = bytes.len(),
                    "snapshot saved"
                ),
                Err(e) => tracing::warn!(
                    error = %e,
                    session = %name,
                    window,
                    "final snapshot flush failed"
                ),
            }
        }
        tracing::info!("flush complete; exiting");
        std::process::exit(0);
    });

    axum::serve(listener, router.into_make_service()).await?;
    Ok(())
}

fn is_api_path(path: &str) -> bool {
    path.starts_with("/api/")
        || path.starts_with("/v1/")
        || path.starts_with("/ws/")
        || path.starts_with("/v/")
        || path.starts_with("/vnc/")
}

async fn serve_embedded_index(uri: Uri) -> axum::response::Response {
    if is_api_path(uri.path()) {
        return StatusCode::NOT_FOUND.into_response();
    }
    match FrontAssets::get("index.html") {
        Some(file) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            file.data.into_owned(),
        )
            .into_response(),
        None => {
            tracing::error!("embedded index.html missing");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn serve_embedded_asset(
    axum::extract::Path(path): axum::extract::Path<String>,
) -> axum::response::Response {
    let full = format!("assets/{path}");
    match FrontAssets::get(&full) {
        Some(file) => {
            let mime = mime_guess::from_path(&full).first_or_octet_stream();
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, mime.as_ref().to_string())],
                file.data.into_owned(),
            )
                .into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
