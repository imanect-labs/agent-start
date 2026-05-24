use anyhow::Result;
use axum::http::{header, StatusCode, Uri};
use axum::response::IntoResponse;
use axum::routing::{any, delete, get, put};
use axum::Router;
use parking_lot::RwLock;
use pty_manager::PtyManager;
use state::Db;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

use crate::manifest;
use crate::sessions::SessionDirectory;

pub struct AppState {
    pub db: Db,
    pub pty: Arc<PtyManager>,
    /// In-memory mirror of session metadata. Persisted on insert via SQLite.
    pub sessions: Arc<RwLock<HashMap<String, SessionDirectory>>>,
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
    let sessions = Arc::new(RwLock::new(HashMap::new()));

    // PTYs are in-process state; they cannot survive a host restart.
    // Any rows still flagged `running` in SQLite from a previous boot
    // are zombies — their `agent-start-host` is gone and reconnecting
    // to them just 404s. Mark them dead so `GET /api/sessions` shows
    // a clean slate. Worktree dirs on disk are preserved either way.
    if let Err(e) = state::mark_all_running_dead(&db).await {
        tracing::warn!(error = %e, "failed to mark prior running sessions dead");
    }

    let app_state: Shared = Arc::new(AppState { db, pty, sessions });

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
                    if let Err(e) = state::mark_dead(&state.db, &name).await {
                        tracing::warn!(error = %e, session = %name, "failed to mark dead");
                    }
                });
            }));
    }

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let api_router = Router::new()
        // Versioned (Rust-native) endpoints.
        .route("/v1/health", get(crate::http::health))
        .route("/v1/version", get(crate::http::version))
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
        .route("/api/projects/:name", delete(crate::http::delete_project))
        .route("/api/sessions", get(crate::http::list_sessions))
        .route(
            "/api/sessions",
            axum::routing::post(crate::http::start_session),
        )
        .route("/api/sessions/:name", delete(crate::http::delete_session))
        .route(
            "/api/sessions/:name/windows",
            get(crate::http::list_windows).post(crate::http::create_window),
        )
        .route(
            "/api/sessions/:name/windows/:index",
            delete(crate::http::delete_window),
        )
        .route("/api/fs/tree", get(crate::http::fs_tree))
        .route(
            "/api/fs/file",
            get(crate::http::fs_read).put(crate::http::fs_write),
        )
        .route("/api/git/status", get(crate::http::git_status))
        .route("/api/git/diff", get(crate::http::git_diff))
        // WebSocket — same URL the UI already uses.
        .route("/ws/terminal", get(crate::ws::ws_terminal))
        .with_state(app_state.clone());

    // SPA wiring. API routes already registered above take precedence.
    // For everything else:
    //   * `/assets/*` -> ServeDir on `dist/assets` (hashed JS/CSS chunks).
    //   * any other path -> read `dist/index.html` and return it with 200
    //     so TanStack Router can pick up the client-side route after
    //     hydration. We avoid `ServeDir::not_found_service` because
    //     tower-http hard-codes the response status to 404 for that
    //     code path, which breaks SEO / 404-rate monitors.
    //
    // The fallback explicitly 404s API/WS prefix typos rather than
    // serving them HTML — otherwise `/api/typo` would return 200 with
    // index.html and silently mask broken client integrations.
    let router = if let Some(dist) = frontend_dist {
        let index_path = dist.join("index.html");
        let assets_dir = dist.join("assets");
        tracing::info!(path = %dist.display(), "serving front-end SPA from dist");

        let serve_index = any(move |uri: Uri| {
            let index_path = index_path.clone();
            async move {
                let path = uri.path();
                if path.starts_with("/api/") || path.starts_with("/v1/") || path.starts_with("/ws/")
                {
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
        api_router
    };

    let router = router.layer(cors).layer(TraceLayer::new_for_http());

    let addr = format!("{bind}:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("agent-start-host listening on http://{addr}");

    manifest::write(&addr).ok();

    axum::serve(listener, router.into_make_service()).await?;
    Ok(())
}
