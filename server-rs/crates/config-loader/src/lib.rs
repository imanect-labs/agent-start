//! XDG-compliant config & preferences loader for agent-start.
//!
//! Mirrors the schema and migration behaviour of the Node `lib/config.ts`
//! and `lib/preferences.ts` so the existing `config.json` and
//! `preferences.json` on disk continue to work.

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("config io: {0}")]
    Io(#[from] std::io::Error),
    #[error("config parse: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("config invalid: {0}")]
    Invalid(String),
}

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

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

fn xdg_config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"))
}

fn xdg_cache_home() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".cache"))
}

pub fn config_path() -> PathBuf {
    if let Some(p) = std::env::var_os("AGENT_START_CONFIG") {
        return PathBuf::from(p);
    }
    xdg_config_home().join("agent-start").join("config.json")
}

pub fn preferences_path() -> PathBuf {
    if let Some(p) = std::env::var_os("AGENT_START_PREFS") {
        return PathBuf::from(p);
    }
    xdg_config_home()
        .join("agent-start")
        .join("preferences.json")
}

pub fn worktree_root() -> PathBuf {
    if let Some(p) = std::env::var_os("AGENT_START_WORKTREE_ROOT") {
        return PathBuf::from(p);
    }
    xdg_cache_home().join("agent-start").join("worktrees")
}

pub fn host_state_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".local").join("share"))
        .join("agent-start")
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
            roots: vec![home().join("dev").to_string_lossy().into_owned()],
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

pub fn clear_cache() {
    *cache().write() = None;
}

/// Load (and migrate if necessary) the on-disk config, creating it from
/// defaults if the file does not yet exist.
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
    let migrated = migrate_legacy(&mut value);

    let defaults = Config::default();
    let mut defaults_value = serde_json::to_value(&defaults)?;
    // Shallow merge user-provided keys over defaults.
    if let (Some(d), Some(u)) = (defaults_value.as_object_mut(), value.as_object()) {
        for (k, v) in u {
            d.insert(k.clone(), v.clone());
        }
    }
    // For `clis`, merge per-key over the default presets so users only need
    // to override individual entries.
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

fn migrate_legacy(value: &mut serde_json::Value) -> bool {
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

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let buf = serde_json::to_vec_pretty(value)?;
    std::fs::write(path, buf)?;
    Ok(())
}

/// Mirror of `lib/preferences.ts`. Returns defaults if the file is absent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preferences {
    pub cli: String,
    #[serde(rename = "skipPermissions")]
    pub skip_permissions: bool,
    #[serde(rename = "extraArgs")]
    pub extra_args: String,
}

impl Preferences {
    pub fn defaults(cfg: &Config) -> Self {
        Self {
            cli: if cfg.default_cli.is_empty() {
                "claude".to_string()
            } else {
                cfg.default_cli.clone()
            },
            skip_permissions: true,
            extra_args: String::new(),
        }
    }
}

pub fn load_preferences() -> Result<Preferences, ConfigError> {
    let cfg = load_config()?;
    let defaults = Preferences::defaults(&cfg);
    let path = preferences_path();
    match std::fs::read_to_string(&path) {
        Ok(raw) => {
            let mut value: serde_json::Value = serde_json::from_str(&raw)?;
            // legacy: dangerouslySkipPermissions → skipPermissions
            if let Some(obj) = value.as_object_mut() {
                if !obj.contains_key("skipPermissions") {
                    if let Some(legacy) = obj.remove("dangerouslySkipPermissions") {
                        obj.insert("skipPermissions".into(), legacy);
                    }
                }
            }
            let mut defaults_value = serde_json::to_value(&defaults)?;
            if let (Some(d), Some(u)) = (defaults_value.as_object_mut(), value.as_object()) {
                for (k, v) in u {
                    d.insert(k.clone(), v.clone());
                }
            }
            Ok(serde_json::from_value(defaults_value)?)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(defaults),
        Err(err) => Err(err.into()),
    }
}

pub fn save_preferences(prefs: &Preferences) -> Result<(), ConfigError> {
    write_json(&preferences_path(), prefs)
}

const EXTRA_ARGS_ALLOWED: &str =
    "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_-./= ";

pub fn sanitize_extra_args(input: &str) -> Result<String, ConfigError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    if trimmed.chars().any(|c| !EXTRA_ARGS_ALLOWED.contains(c)) {
        return Err(ConfigError::Invalid(
            "extraArgs contains unsupported characters. Allowed: letters, digits, space, _ - . / ="
                .into(),
        ));
    }
    Ok(trimmed.to_string())
}

pub fn build_launch_command(
    cli: &CliConfig,
    skip_permissions: bool,
    extra_args: &str,
) -> Result<String, ConfigError> {
    if cli.command.is_empty() {
        return Ok(String::new());
    }
    let mut parts: Vec<String> = vec![cli.command.clone()];
    if skip_permissions {
        if let Some(flag) = &cli.skip_permissions_flag {
            parts.push(flag.clone());
        }
    }
    let extra = sanitize_extra_args(extra_args)?;
    if !extra.is_empty() {
        parts.push(extra);
    }
    Ok(parts.join(" "))
}

/// True when `target` resolves under any of `cfg.roots`.
pub fn is_path_under_roots(cfg: &Config, target: &Path) -> bool {
    let Ok(resolved) =
        std::fs::canonicalize(target).or_else(|_| Ok::<_, std::io::Error>(target.to_path_buf()))
    else {
        return false;
    };
    for root in &cfg.roots {
        let root = std::path::PathBuf::from(shellexpand_home(root));
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

fn shellexpand_home(p: &str) -> String {
    if let Some(rest) = p.strip_prefix("~/") {
        home().join(rest).to_string_lossy().into_owned()
    } else if p == "~" {
        home().to_string_lossy().into_owned()
    } else {
        p.to_string()
    }
}

pub fn expand_root(root: &str) -> PathBuf {
    PathBuf::from(shellexpand_home(root))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_ok() {
        assert_eq!(
            sanitize_extra_args("  --model opus  ").unwrap(),
            "--model opus"
        );
    }

    #[test]
    fn sanitize_rejects() {
        assert!(sanitize_extra_args("rm -rf;ls").is_err());
        assert!(sanitize_extra_args("$(whoami)").is_err());
    }

    #[test]
    fn build_command_empty_for_shell() {
        let c = CliConfig {
            command: String::new(),
            skip_permissions_flag: None,
            label: None,
        };
        assert_eq!(build_launch_command(&c, true, "anything").unwrap(), "");
    }

    #[test]
    fn build_command_skip_flag() {
        let c = CliConfig {
            command: "claude".into(),
            skip_permissions_flag: Some("--dangerously-skip-permissions".into()),
            label: None,
        };
        assert_eq!(
            build_launch_command(&c, true, "--model opus").unwrap(),
            "claude --dangerously-skip-permissions --model opus"
        );
        assert_eq!(build_launch_command(&c, false, "").unwrap(), "claude");
    }
}
