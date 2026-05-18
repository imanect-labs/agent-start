use super::err;
use agent_start_api::{CliInfo, ConfigBody, ConfigPaths};
use axum::http::StatusCode;
use axum::response::Response;
use axum::{response::IntoResponse, Json};

pub async fn get_config() -> Response {
    let cfg = match config_loader::load_config() {
        Ok(c) => c,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    let clis = cfg
        .clis
        .iter()
        .map(|(key, conf)| CliInfo {
            key: key.clone(),
            label: conf.label.clone().unwrap_or_else(|| key.clone()),
            command: conf.command.clone(),
            has_skip_flag: conf.skip_permissions_flag.is_some(),
            skip_flag: conf.skip_permissions_flag.clone().unwrap_or_default(),
        })
        .collect();
    let paths = ConfigPaths {
        config: config_loader::config_path().to_string_lossy().into_owned(),
        preferences: config_loader::preferences_path()
            .to_string_lossy()
            .into_owned(),
        worktree_root: config_loader::worktree_root()
            .to_string_lossy()
            .into_owned(),
    };
    Json(ConfigBody {
        clis,
        default_cli: cfg.default_cli,
        session_prefix: cfg.session_prefix,
        roots: cfg.roots,
        shell: cfg.shell,
        show_hidden: cfg.show_hidden,
        git_only: cfg.git_only,
        paths,
    })
    .into_response()
}
