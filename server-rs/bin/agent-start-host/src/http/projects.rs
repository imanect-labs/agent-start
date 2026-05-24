use super::err;
use agent_start_api::{PendingProject, ProjectsBody};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

pub async fn list_projects() -> Response {
    let cfg = match config_loader::load_config() {
        Ok(c) => c,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    let projects = match workspace_manager::list_projects(&cfg) {
        Ok(p) => p,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    let pending = list_pending(&config_loader::projects_dir());
    Json(ProjectsBody { projects, pending }).into_response()
}

/// Scan `<projects>/*.partial` and `*.error` to surface in-progress and
/// failed clone/import jobs to the sidebar. Restart-safe: no in-memory state.
fn list_pending(projects_dir: &std::path::Path) -> Vec<PendingProject> {
    let Ok(rd) = std::fs::read_dir(projects_dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if let Some(stem) = name.strip_suffix(".partial") {
            let kind = std::fs::read_to_string(path.join(".agent-start-kind"))
                .unwrap_or_else(|_| "import".to_string())
                .trim()
                .to_string();
            out.push(PendingProject {
                name: stem.to_string(),
                path: path.to_string_lossy().into_owned(),
                kind,
                error: None,
            });
        } else if let Some(stem) = name.strip_suffix(".error") {
            let err_msg = std::fs::read_to_string(&path).unwrap_or_default();
            out.push(PendingProject {
                name: stem.to_string(),
                path: path.to_string_lossy().into_owned(),
                kind: "error".to_string(),
                error: Some(err_msg),
            });
        }
    }
    out
}
