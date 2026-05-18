//! HTTP handlers. Each submodule owns one slice of the surface; this
//! file just wires them together and exposes the shared `err()` helper.

use agent_start_api::ErrorBody;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

mod config;
mod meta;
mod preferences;
mod projects;
mod sessions;

pub use config::get_config;
pub use meta::{health, version};
pub use preferences::{get_preferences, put_preferences};
pub use projects::list_projects;
pub use sessions::{delete_session, list_sessions, start_session};

/// Render an `{ "error": "<msg>" }` JSON body with the given status code.
pub(crate) fn err(status: StatusCode, msg: impl Into<String>) -> Response {
    (status, Json(ErrorBody::new(msg.into()))).into_response()
}
