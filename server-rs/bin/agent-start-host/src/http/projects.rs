use super::err;
use agent_start_api::ProjectsBody;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

pub async fn list_projects() -> Response {
    let cfg = match config_loader::load_config() {
        Ok(c) => c,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    match workspace_manager::list_projects(&cfg) {
        Ok(projects) => Json(ProjectsBody { projects }).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}
