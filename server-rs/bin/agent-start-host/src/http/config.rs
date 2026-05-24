use super::err;
use agent_start_api::{CliInfo, ConfigBody, ConfigPatch, ConfigPaths};
use axum::http::StatusCode;
use axum::response::Response;
use axum::{response::IntoResponse, Json};
use config_loader::CliConfig;
use std::collections::BTreeMap;

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

pub async fn put_config(Json(patch): Json<ConfigPatch>) -> Response {
    let mut cfg = match config_loader::load_config() {
        Ok(c) => c,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    if let Some(v) = patch.roots {
        cfg.roots = v;
    }
    if let Some(v) = patch.session_prefix {
        cfg.session_prefix = v;
    }
    if let Some(v) = patch.shell {
        cfg.shell = v;
    }
    if let Some(v) = patch.show_hidden {
        cfg.show_hidden = v;
    }
    if let Some(v) = patch.git_only {
        cfg.git_only = v;
    }
    if let Some(v) = patch.default_cli {
        cfg.default_cli = v;
    }
    if let Some(clis_patch) = patch.clis {
        let mut next: BTreeMap<String, CliConfig> = BTreeMap::new();
        for (key, c) in clis_patch {
            next.insert(
                key,
                CliConfig {
                    command: c.command,
                    skip_permissions_flag: c.skip_permissions_flag,
                    label: c.label,
                },
            );
        }
        cfg.clis = next;
    }

    if cfg.roots.is_empty() {
        return err(StatusCode::BAD_REQUEST, "roots must not be empty");
    }
    if !cfg.clis.contains_key(&cfg.default_cli) {
        return err(
            StatusCode::BAD_REQUEST,
            format!("defaultCli '{}' not present in clis", cfg.default_cli),
        );
    }
    if cfg.shell.trim().is_empty() {
        return err(StatusCode::BAD_REQUEST, "shell must not be empty");
    }

    if let Err(e) = config_loader::save_config(&cfg) {
        return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    }
    get_config().await
}
