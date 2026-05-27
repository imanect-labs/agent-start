//! User preferences (`preferences.json`) — the per-session CLI flags
//! the launch sheet writes. Loaded lazily; defaults are derived from
//! the active `Config`.

use crate::config::{load_config, CliConfig, Config};
use crate::error::ConfigError;
use crate::io::write_json;
use crate::paths::preferences_path;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preferences {
    pub cli: String,
    #[serde(rename = "skipPermissions")]
    pub skip_permissions: bool,
    #[serde(rename = "extraArgs")]
    pub extra_args: String,
    /// Whether the launch sheet's "create git worktree" toggle is on
    /// by default. Defaults to `true`.
    #[serde(rename = "createWorktree", default = "yes")]
    pub create_worktree: bool,
    /// When true, the "GUI" tab opens noVNC in a new browser window
    /// (full-screen) instead of embedding it as an in-app iframe tab.
    #[serde(rename = "guiOpenInNewTab", default)]
    pub gui_open_in_new_tab: bool,
}

fn yes() -> bool {
    true
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
            create_worktree: true,
            gui_open_in_new_tab: false,
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

/// Whitelist of characters allowed in user-supplied `extraArgs`. Kept
/// tight on purpose: this string is concatenated into a shell command.
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

/// Build the shell-quoted command line we hand to `<shell> -lc '...'`.
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
