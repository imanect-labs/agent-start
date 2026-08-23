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

/// Build the command line for a *headless* run: the CLI is handed the
/// prompt, does the work, and exits.
///
/// This is what a queued task needs. `build_launch_command` produces an
/// interactive session, and appending a prompt to that leaves most
/// agents sitting at a REPL waiting for a human who is not there —
/// which a task queue experiences as every task timing out.
///
/// The prompt is single-quoted, so it reaches the CLI as exactly one
/// argument no matter what the user typed.
pub fn build_headless_command(
    cli: &CliConfig,
    skip_permissions: bool,
    extra_args: &str,
    prompt: &str,
) -> Result<String, ConfigError> {
    if cli.command.is_empty() {
        return Err(ConfigError::Invalid(
            "this CLI has no command to run headlessly".into(),
        ));
    }
    let mut parts: Vec<String> = vec![cli.command.clone()];
    // A subcommand (`codex exec`) has to come before its flags, so the
    // prompt argument is placed first and the prompt itself last.
    if let Some(arg) = &cli.prompt_arg {
        parts.push(arg.clone());
    }
    if skip_permissions {
        if let Some(flag) = &cli.skip_permissions_flag {
            parts.push(flag.clone());
        }
    }
    let extra = sanitize_extra_args(extra_args)?;
    if !extra.is_empty() {
        parts.push(extra);
    }
    parts.push(shell_quote(prompt));
    Ok(parts.join(" "))
}

/// Wrap `s` in single quotes for safe inclusion in a `sh -lc <cmd>`
/// string, escaping embedded single quotes as `'\''`. Everything inside
/// stays literal, so a prompt containing `$(…)` or `;` is text rather
/// than a command.
pub fn shell_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claude() -> CliConfig {
        CliConfig {
            command: "claude".into(),
            skip_permissions_flag: Some("--dangerously-skip-permissions".into()),
            label: None,
            mode: None,
            prompt_arg: Some("-p".into()),
        }
    }

    #[test]
    fn headless_command_puts_the_prompt_behind_its_flag() {
        let c = build_headless_command(&claude(), true, "", "fix the login bug").unwrap();
        assert_eq!(
            c,
            "claude -p --dangerously-skip-permissions 'fix the login bug'"
        );
    }

    #[test]
    fn a_cli_without_a_prompt_flag_gets_a_positional_prompt() {
        let mut c = claude();
        c.prompt_arg = None;
        assert_eq!(
            build_headless_command(&c, false, "", "do it").unwrap(),
            "claude 'do it'"
        );
    }

    #[test]
    fn a_prompt_cannot_break_out_of_its_quotes() {
        let c = build_headless_command(&claude(), false, "", "$(rm -rf /); `id` 'x'").unwrap();
        let prompt = c.strip_prefix("claude -p ").unwrap();
        assert!(prompt.starts_with('\'') && prompt.ends_with('\''));
        assert!(prompt.contains("$(rm -rf /)"), "prompt text was mangled");
        // The embedded quote is escaped, never left to terminate the span.
        assert!(prompt.contains("'\\''x'\\''"));
    }

    #[test]
    fn the_bare_shell_cannot_run_headlessly() {
        let c = CliConfig {
            command: String::new(),
            skip_permissions_flag: None,
            label: None,
            mode: None,
            prompt_arg: None,
        };
        assert!(build_headless_command(&c, false, "", "hi").is_err());
    }

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
            mode: None,
            prompt_arg: None,
        };
        assert_eq!(build_launch_command(&c, true, "anything").unwrap(), "");
    }

    #[test]
    fn build_command_skip_flag() {
        let c = CliConfig {
            command: "claude".into(),
            skip_permissions_flag: Some("--dangerously-skip-permissions".into()),
            label: None,
            mode: None,
            prompt_arg: None,
        };
        assert_eq!(
            build_launch_command(&c, true, "--model opus").unwrap(),
            "claude --dangerously-skip-permissions --model opus"
        );
        assert_eq!(build_launch_command(&c, false, "").unwrap(), "claude");
    }
}
