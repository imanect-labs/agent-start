//! Top-level `config.json` schema, cached loader, and `claudeCommand`
//! → `clis.claude` legacy migration.

use crate::error::ConfigError;
use crate::io::write_json;
use crate::paths::{self, config_path};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliConfig {
    pub command: String,
    #[serde(
        rename = "skipPermissionsFlag",
        skip_serializing_if = "Option::is_none"
    )]
    pub skip_permissions_flag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub roots: Vec<String>,
    #[serde(rename = "sessionPrefix")]
    pub session_prefix: String,
    pub shell: String,
    #[serde(rename = "showHidden")]
    pub show_hidden: bool,
    #[serde(rename = "gitOnly")]
    pub git_only: bool,
    pub clis: BTreeMap<String, CliConfig>,
    #[serde(rename = "defaultCli")]
    pub default_cli: String,
}

impl Default for Config {
    fn default() -> Self {
        let mut clis = BTreeMap::new();
        clis.insert(
            "claude".to_string(),
            CliConfig {
                command: "claude".to_string(),
                skip_permissions_flag: Some("--dangerously-skip-permissions".to_string()),
                label: Some("Claude Code".to_string()),
            },
        );
        clis.insert(
            "codex".to_string(),
            CliConfig {
                command: "codex".to_string(),
                skip_permissions_flag: Some("--full-auto".to_string()),
                label: Some("Codex CLI".to_string()),
            },
        );
        clis.insert(
            "shell".to_string(),
            CliConfig {
                command: String::new(),
                skip_permissions_flag: None,
                label: Some("Terminal".to_string()),
            },
        );
        Config {
            roots: vec![paths::home().join("dev").to_string_lossy().into_owned()],
            session_prefix: "cc-".to_string(),
            shell: "/bin/bash".to_string(),
            show_hidden: false,
            git_only: false,
            clis,
            default_cli: "claude".to_string(),
        }
    }
}

static CACHE: OnceLock<RwLock<Option<Config>>> = OnceLock::new();

fn cache() -> &'static RwLock<Option<Config>> {
    CACHE.get_or_init(|| RwLock::new(None))
}

/// Drop the cached `Config` so the next `load_config()` re-reads the file.
/// Test-only; production code reads through the cache.
#[cfg(test)]
pub fn clear_cache() {
    *cache().write() = None;
}

/// Load (and migrate if necessary) the on-disk config, creating it from
/// defaults if the file does not yet exist. Cached after first call.
pub fn load_config() -> Result<Config, ConfigError> {
    if let Some(c) = cache().read().clone() {
        return Ok(c);
    }
    let path = config_path();
    let cfg = match std::fs::read_to_string(&path) {
        Ok(raw) => merge_with_defaults(&raw, &path)?,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            let default = Config::default();
            write_json(&path, &default)?;
            default
        }
        Err(err) => return Err(err.into()),
    };
    *cache().write() = Some(cfg.clone());
    Ok(cfg)
}

fn merge_with_defaults(raw: &str, path: &Path) -> Result<Config, ConfigError> {
    let mut value: serde_json::Value = serde_json::from_str(raw)?;
    let migrated = migrate_legacy_claude_command(&mut value);

    let mut defaults_value = serde_json::to_value(Config::default())?;
    if let (Some(d), Some(u)) = (defaults_value.as_object_mut(), value.as_object()) {
        for (k, v) in u {
            d.insert(k.clone(), v.clone());
        }
    }
    // Per-key merge for `clis` so users only need to override individual entries.
    if let Some(user_clis) = value.get("clis").and_then(|v| v.as_object()) {
        if let Some(merged_clis) = defaults_value
            .get_mut("clis")
            .and_then(|v| v.as_object_mut())
        {
            for (k, v) in user_clis {
                merged_clis.insert(k.clone(), v.clone());
            }
        }
    }
    let cfg: Config = serde_json::from_value(defaults_value)?;
    if migrated {
        write_json(path, &cfg)?;
    }
    Ok(cfg)
}

/// Old config files used a top-level `claudeCommand` string. Lift it
/// into `clis.claude.command` and signal that the file needs rewriting.
fn migrate_legacy_claude_command(value: &mut serde_json::Value) -> bool {
    let Some(map) = value.as_object_mut() else {
        return false;
    };
    let Some(legacy) = map.remove("claudeCommand") else {
        return false;
    };
    let Some(legacy_cmd) = legacy.as_str().map(str::to_owned) else {
        return false;
    };
    let clis = map
        .entry("clis".to_string())
        .or_insert_with(|| serde_json::Value::Object(Default::default()));
    let Some(clis_obj) = clis.as_object_mut() else {
        return false;
    };
    let claude = clis_obj
        .entry("claude".to_string())
        .or_insert_with(|| serde_json::json!({"command": legacy_cmd.clone()}));
    if let Some(c) = claude.as_object_mut() {
        c.insert("command".into(), serde_json::Value::String(legacy_cmd));
    }
    true
}

/// True when `target` resolves under any of `cfg.roots` (with `~` expanded).
pub fn is_path_under_roots(cfg: &Config, target: &Path) -> bool {
    let Ok(resolved) =
        std::fs::canonicalize(target).or_else(|_| Ok::<_, std::io::Error>(target.to_path_buf()))
    else {
        return false;
    };
    for root in &cfg.roots {
        let root = paths::expand_root(root);
        let Ok(root_canon) =
            std::fs::canonicalize(&root).or_else(|_| Ok::<_, std::io::Error>(root.clone()))
        else {
            continue;
        };
        if resolved == root_canon || resolved.starts_with(&root_canon) {
            return true;
        }
    }
    false
}
