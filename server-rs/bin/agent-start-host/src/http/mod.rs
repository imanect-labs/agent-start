//! HTTP handlers. Each submodule owns one slice of the surface; this
//! file just wires them together and exposes the shared `err()` helper.

use agent_start_api::ErrorBody;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

mod code_server;
pub mod code_server_proxy;
mod config;
mod fs;
mod git;
mod issues;
mod meta;
mod novnc;
pub mod novnc_proxy;
mod preferences;
mod projects;
mod projects_write;
mod sessions;
mod windows;

pub use code_server::{close_code_server, open_code_server};
pub use config::{get_config, put_config};
pub use fs::{fs_read, fs_tree, fs_write};
pub use git::{git_diff, git_status};
pub use issues::{list_issues, view_issue};
pub use meta::{health, version};
pub use novnc::{close_novnc, open_novnc};
pub use preferences::{get_preferences, put_preferences};
pub use projects::list_projects;
pub use projects_write::{clone_project, delete_project, import_project};
pub use sessions::{delete_session, list_sessions, restart_session, start_session};
pub use windows::{create_window, delete_window, list_windows};

/// Render an `{ "error": "<msg>" }` JSON body with the given status code.
pub(crate) fn err(status: StatusCode, msg: impl Into<String>) -> Response {
    (status, Json(ErrorBody::new(msg.into()))).into_response()
}
