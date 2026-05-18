use agent_start_api::{HealthBody, VersionBody};
use axum::Json;

pub async fn health() -> Json<HealthBody> {
    Json(HealthBody { ok: true })
}

pub async fn version() -> Json<VersionBody> {
    Json(VersionBody {
        name: "agent-start-host".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    })
}
