//! `POST /api/sessions/:name/novnc` — ensure an Xvnc + websockify pair
//! is running for this session and return the URL the browser should
//! load to view the desktop. `DELETE` stops both children. HTTP/WS
//! forwarding lives in `novnc_proxy`.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

use super::err;
use crate::app::Shared;

#[derive(Serialize)]
pub struct OpenResponse {
    pub url: String,
    pub ws_port: u16,
    pub display: u32,
}

pub async fn open_novnc(State(app): State<Shared>, Path(name): Path<String>) -> Response {
    // Confirm the session exists so we don't spawn an orphan backend for
    // a typo'd name.
    if !app.sessions.read().contains_key(&name) {
        return err(StatusCode::NOT_FOUND, "session not found");
    }

    let instance = match app.novnc.ensure(&name).await {
        Ok(i) => i,
        Err(e @ novnc_manager::NovncError::XvncNotInstalled) => {
            return err(StatusCode::FAILED_DEPENDENCY, e.to_string());
        }
        Err(e @ novnc_manager::NovncError::WebsockifyNotInstalled) => {
            return err(StatusCode::FAILED_DEPENDENCY, e.to_string());
        }
        Err(e @ novnc_manager::NovncError::NovncDirNotFound) => {
            return err(StatusCode::FAILED_DEPENDENCY, e.to_string());
        }
        Err(e) => {
            tracing::warn!(error = %e, session = %name, "noVNC spawn failed");
            return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
        }
    };

    // `path=websockify` tells noVNC's vnc.html which sub-path under the
    // page origin to upgrade the WebSocket on — matches what websockify
    // exposes when invoked with a target host:port positional argument.
    let url = format!(
        "/vnc/{name}/vnc.html?path=vnc/{name}/websockify&autoconnect=1&resize=scale"
    );
    Json(OpenResponse {
        url,
        ws_port: instance.ws_port(),
        display: instance.display(),
    })
    .into_response()
}

pub async fn close_novnc(State(app): State<Shared>, Path(name): Path<String>) -> Response {
    app.novnc.kill(&name).await;
    (StatusCode::NO_CONTENT, ()).into_response()
}
