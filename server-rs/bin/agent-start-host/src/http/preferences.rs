use super::err;
use agent_start_api::{Preferences, PreferencesBody, PreferencesPatch};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

pub async fn get_preferences() -> Response {
    match config_loader::load_preferences() {
        Ok(p) => Json(PreferencesBody {
            preferences: Preferences {
                cli: p.cli,
                skip_permissions: p.skip_permissions,
                extra_args: p.extra_args,
            },
        })
        .into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

pub async fn put_preferences(Json(body): Json<PreferencesPatch>) -> Response {
    let cfg = match config_loader::load_config() {
        Ok(c) => c,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    let mut current = match config_loader::load_preferences() {
        Ok(c) => c,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    if let Some(cli) = body.cli {
        if !cfg.clis.contains_key(&cli) {
            return err(StatusCode::BAD_REQUEST, format!("unknown cli: {cli}"));
        }
        current.cli = cli;
    }
    if let Some(skip) = body.skip_permissions {
        current.skip_permissions = skip;
    }
    if let Some(extra) = body.extra_args {
        match config_loader::sanitize_extra_args(&extra) {
            Ok(v) => current.extra_args = v,
            Err(e) => return err(StatusCode::BAD_REQUEST, e.to_string()),
        }
    }
    if let Err(e) = config_loader::save_preferences(&current) {
        return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    }
    Json(PreferencesBody {
        preferences: Preferences {
            cli: current.cli,
            skip_permissions: current.skip_permissions,
            extra_args: current.extra_args,
        },
    })
    .into_response()
}
