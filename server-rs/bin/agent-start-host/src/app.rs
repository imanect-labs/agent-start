use anyhow::Result;
use axum::routing::{delete, get, put};
use axum::Router;
use parking_lot::RwLock;
use pty_manager::PtyManager;
use state::Db;
use std::collections::HashMap;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
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

pub async fn run(bind: String, port: u16) -> Result<()> {
    let db = state::open().await?;
    let pty = Arc::new(PtyManager::new());
    let sessions = Arc::new(RwLock::new(HashMap::new()));

    // Hydrate in-memory directory from the DB so existing sessions show up
    // even after the host restarts (their PTYs are gone, but the metadata
    // remains for display until the user clears them).
    if let Ok(rows) = state::list_sessions(&db, "").await {
        let mut map = sessions.write();
        for row in rows {
            map.insert(row.name.clone(), SessionDirectory::from_row(&row));
        }
    }

    let app_state: Shared = Arc::new(AppState { db, pty, sessions });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let router = Router::new()
        // Versioned (Rust-native) endpoints.
        .route("/v1/health", get(crate::http::health))
        .route("/v1/version", get(crate::http::version))
        // Legacy `/api/*` surface preserved so the existing Next.js UI can
        // proxy through `next.config.mjs` rewrites without changes.
        .route("/api/config", get(crate::http::get_config))
        .route("/api/preferences", get(crate::http::get_preferences))
        .route("/api/preferences", put(crate::http::put_preferences))
        .route("/api/projects", get(crate::http::list_projects))
        .route("/api/sessions", get(crate::http::list_sessions))
        .route(
            "/api/sessions",
            axum::routing::post(crate::http::start_session),
        )
        .route("/api/sessions/:name", delete(crate::http::delete_session))
        // WebSocket — same URL the UI already uses.
        .route("/ws/terminal", get(crate::ws::ws_terminal))
        .with_state(app_state.clone())
        .layer(cors)
        .layer(TraceLayer::new_for_http());

    let addr = format!("{bind}:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("agent-start-host listening on http://{addr}");

    manifest::write(&addr).ok();

    axum::serve(listener, router.into_make_service()).await?;
    Ok(())
}
