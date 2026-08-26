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
    /// Launch mode: `"pty"` (default — a terminal window) or `"chat"`
    /// (headless stream-json, rendered as a ChatTab). Absent = pty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// Flag (or subcommand) that makes this CLI take a prompt, do the
    /// work, and exit — `-p` for `claude`, `exec` for `codex`. Queued
    /// tasks need it: an interactive REPL waiting for input would sit
    /// there until its lease expired. Absent means the CLI is given the
    /// prompt as a bare positional argument.
    #[serde(rename = "promptArg", default, skip_serializing_if = "Option::is_none")]
    pub prompt_arg: Option<String>,
}

impl CliConfig {
    /// True when this CLI launches the headless chat experience (#34)
    /// rather than a PTY-backed terminal.
    pub fn is_chat(&self) -> bool {
        self.mode.as_deref() == Some("chat")
    }
}

/// One selectable model for chat mode (decision 8). `id` is passed to
/// `claude --model`; `label` is the human-facing name in the picker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatModel {
    pub id: String,
    pub label: String,
}

/// One agent a chat can talk to.
///
/// A chat is not bound to a provider at launch: the composer picks
/// `provider/model` the way paseo's `claude/opus-4.6` does, and
/// switching either one respawns the conversation underneath. That is
/// why the command lives here rather than on a per-provider `clis`
/// entry — there is exactly one "Chat" launcher, not one per agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatProvider {
    pub id: String,
    pub label: String,
    /// Program to run (resolved through the login shell's PATH).
    pub command: String,
    /// Which stdio protocol the command speaks. See
    /// `chat_manager::AgentDriver` and `chat_manager::driver::resolve`
    /// for the implemented set; an unknown value is refused at spawn time
    /// with the name in the message rather than silently falling back to
    /// a protocol the CLI does not speak.
    pub driver: String,
    pub models: Vec<ChatModel>,
    #[serde(rename = "defaultModel", skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    /// Shown in the picker with a warning badge. For drivers written
    /// against a published protocol but not yet exercised against a real
    /// binary here.
    #[serde(default)]
    pub experimental: bool,
}

/// Chat-mode configuration (#34): which agents are selectable, and which
/// one a new conversation starts on.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatConfig {
    #[serde(default)]
    pub providers: Vec<ChatProvider>,
    /// Pre-provider model list (#34). Kept so an existing config keeps
    /// working; `backfill_providers` folds it into the Claude provider.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<ChatModel>,
    #[serde(
        rename = "defaultProvider",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub default_provider: Option<String>,
    #[serde(rename = "defaultModel", skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
}

impl Default for ChatConfig {
    fn default() -> Self {
        ChatConfig {
            providers: vec![
                ChatProvider {
                    id: "claude".into(),
                    label: "Claude Code".into(),
                    command: "claude".into(),
                    driver: "claude-stream-json".into(),
                    models: vec![
                        ChatModel {
                            id: "opus".into(),
                            label: "Opus".into(),
                        },
                        ChatModel {
                            id: "sonnet".into(),
                            label: "Sonnet".into(),
                        },
                        ChatModel {
                            id: "haiku".into(),
                            label: "Haiku".into(),
                        },
                    ],
                    default_model: None,
                    experimental: false,
                },
                // Codex is deliberately absent. It shipped here against
                // `codex proto`, a subcommand the CLI no longer has, so
                // picking it produced a process that could never speak.
                // The protocol it actually wants (`codex app-server`,
                // JSON-RPC) is recorded in `docs/references.ja.md`; a
                // provider comes back when there is a driver for it.
            ],
            models: Vec::new(),
            default_provider: None,
            default_model: None,
        }
    }
}

impl ChatConfig {
    /// The provider a new conversation starts on: the configured
    /// default, or the first one listed.
    pub fn default_provider(&self) -> Option<&ChatProvider> {
        self.default_provider
            .as_deref()
            .and_then(|id| self.provider(id))
            .or_else(|| self.providers.first())
    }

    pub fn provider(&self, id: &str) -> Option<&ChatProvider> {
        self.providers.iter().find(|p| p.id == id)
    }

    /// Fold a pre-provider config into the new shape.
    ///
    /// A `chat` block written before providers existed carries only
    /// `models` — those are Claude models, because Claude was the only
    /// agent chat could talk to. Dropping them would silently empty the
    /// model picker for every existing user, so they become the Claude
    /// provider's list instead.
    pub fn backfill_providers(&mut self) {
        if self.providers.is_empty() {
            self.providers = ChatConfig::default().providers;
        }
        // Taken before the early return: a config that set only
        // `defaultModel` and never listed models would otherwise keep it
        // on `ChatConfig`, where nothing reads it any more.
        let legacy_models = std::mem::take(&mut self.models);
        let legacy_default = self.default_model.take();
        if legacy_models.is_empty() && legacy_default.is_none() {
            return;
        }
        if let Some(claude) = self.providers.iter_mut().find(|p| p.id == "claude") {
            if !legacy_models.is_empty() {
                claude.models = legacy_models;
            }
            if legacy_default.is_some() {
                claude.default_model = legacy_default;
            }
        }
    }
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
    #[serde(default)]
    pub chat: ChatConfig,
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
                mode: None,
                prompt_arg: Some("-p".to_string()),
            },
        );
        clis.insert(
            "claude-chat".to_string(),
            CliConfig {
                command: "claude".to_string(),
                skip_permissions_flag: Some("--dangerously-skip-permissions".to_string()),
                label: Some("Chat".to_string()),
                mode: Some("chat".to_string()),
                prompt_arg: Some("-p".to_string()),
            },
        );
        clis.insert(
            "codex".to_string(),
            CliConfig {
                command: "codex".to_string(),
                // `--full-auto` was removed from the codex CLI; passing it
                // is an argument error, which broke both a codex task and
                // a codex terminal whenever skip-permissions was on.
                // Verified against codex-cli 0.149.1 (2026-08).
                skip_permissions_flag: Some(
                    "--dangerously-bypass-approvals-and-sandbox".to_string(),
                ),
                label: Some("Codex CLI".to_string()),
                mode: None,
                prompt_arg: Some("exec".to_string()),
            },
        );
        clis.insert(
            "shell".to_string(),
            CliConfig {
                command: String::new(),
                skip_permissions_flag: None,
                label: Some("Terminal".to_string()),
                mode: None,
                prompt_arg: None,
            },
        );
        Config {
            roots: vec![paths::projects_dir().to_string_lossy().into_owned()],
            session_prefix: "cc-".to_string(),
            shell: "/bin/bash".to_string(),
            show_hidden: false,
            git_only: false,
            clis,
            default_cli: "claude".to_string(),
            chat: ChatConfig::default(),
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

/// Drop the cached `Config`. Call after writing the file so subsequent
/// `load_config()` calls pick up the new contents.
pub fn invalidate_cache() {
    *cache().write() = None;
}

/// Persist `cfg` to `config_path()` (pretty JSON) and invalidate the cache.
pub fn save_config(cfg: &Config) -> Result<(), ConfigError> {
    write_json(&config_path(), cfg)?;
    invalidate_cache();
    Ok(())
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
    // Capture the default `clis` *before* the generic overlay clobbers it, so
    // newly-shipped built-ins (e.g. `claude-chat`) survive even when the
    // user's file already has its own `clis` object.
    let default_clis = defaults_value.get("clis").cloned();
    if let (Some(d), Some(u)) = (defaults_value.as_object_mut(), value.as_object()) {
        for (k, v) in u {
            d.insert(k.clone(), v.clone());
        }
    }
    // Per-key merge for `clis` so users only need to override individual
    // entries: start from the fresh defaults, then overlay the user's keys.
    if let Some(user_clis) = value.get("clis").and_then(|v| v.as_object()) {
        let mut merged = default_clis
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();
        for (k, v) in user_clis {
            merged.insert(k.clone(), v.clone());
        }
        if let Some(obj) = defaults_value.as_object_mut() {
            obj.insert("clis".to_string(), serde_json::Value::Object(merged));
        }
    }
    let mut cfg: Config = serde_json::from_value(defaults_value)?;
    // `chat.providers` gets the same treatment as `clis`, for the same
    // reason: a user who listed providers once should still receive
    // agents shipped later, instead of being frozen at whatever existed
    // the day they edited the file.
    merge_default_providers(&mut cfg.chat);
    cfg.chat.backfill_providers();
    if migrated {
        write_json(path, &cfg)?;
    }
    Ok(cfg)
}

/// Add built-in providers the user's config does not mention, keeping
/// their own definitions (and their order) untouched.
///
/// Matching is by `id`, so overriding one built-in does not cost you the
/// others — and a provider the user invented is never disturbed.
fn merge_default_providers(chat: &mut ChatConfig) {
    merge_providers_from(chat, ChatConfig::default().providers)
}

/// Split out so the merge can be exercised against a built-in set the
/// test controls. Asserting it through `ChatConfig::default()` only works
/// while there happens to be more than one built-in agent, which is not
/// something this behaviour depends on.
fn merge_providers_from(chat: &mut ChatConfig, built_ins: Vec<ChatProvider>) {
    if chat.providers.is_empty() {
        return; // `backfill_providers` installs the whole default set.
    }
    for built_in in built_ins {
        if !chat.providers.iter().any(|p| p.id == built_in.id) {
            chat.providers.push(built_in);
        }
    }
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

#[cfg(test)]
mod merge_tests {
    use super::*;

    #[test]
    fn preserves_new_builtin_cli_when_user_has_clis() {
        // A user config predating `claude-chat` that already customized clis.
        let raw = r#"{
            "roots": ["/x"],
            "clis": {
                "claude": { "command": "claude" },
                "codex": { "command": "codex" }
            }
        }"#;
        let cfg = merge_with_defaults(raw, Path::new("/tmp/does-not-matter.json")).unwrap();
        // The new built-in must survive the merge…
        assert!(cfg.clis.contains_key("claude-chat"), "claude-chat dropped");
        assert!(cfg.clis["claude-chat"].is_chat());
        // …and the user's own entries are still present.
        assert!(cfg.clis.contains_key("claude"));
        assert!(cfg.clis.contains_key("codex"));
        // chat defaults fill in when absent.
        assert!(!cfg.chat.providers.is_empty());
    }

    #[test]
    fn a_pre_provider_chat_block_keeps_its_models() {
        // What a config written before providers existed looks like.
        let raw = r#"{
            "roots": ["/x"],
            "chat": { "models": [{"id": "opus", "label": "Opus"}], "defaultModel": "opus" }
        }"#;
        let cfg = merge_with_defaults(raw, Path::new("/tmp/x.json")).unwrap();
        let claude = cfg.chat.provider("claude").expect("claude provider");
        assert_eq!(claude.models.len(), 1, "the user's model list was dropped");
        assert_eq!(claude.default_model.as_deref(), Some("opus"));
        // …and every built-in agent is still on offer.
        assert_eq!(
            cfg.chat.providers.len(),
            ChatConfig::default().providers.len()
        );
    }

    #[test]
    fn a_config_with_only_a_default_model_keeps_it() {
        // No `models` list, just the default: the early return used to
        // drop this on the floor.
        let raw = r#"{ "roots": ["/x"], "chat": { "defaultModel": "sonnet" } }"#;
        let cfg = merge_with_defaults(raw, Path::new("/tmp/x.json")).unwrap();
        let claude = cfg.chat.provider("claude").expect("claude provider");
        assert_eq!(claude.default_model.as_deref(), Some("sonnet"));
        assert!(!claude.models.is_empty(), "the built-in menu was lost");
    }

    #[test]
    fn a_user_provider_list_still_receives_new_built_ins() {
        let raw = r#"{
            "roots": ["/x"],
            "chat": { "providers": [
              { "id": "claude", "label": "Mine", "command": "my-claude",
                "driver": "claude-stream-json", "models": [] }
            ] }
        }"#;
        let cfg = merge_with_defaults(raw, Path::new("/tmp/x.json")).unwrap();
        // Their override wins…
        assert_eq!(cfg.chat.provider("claude").unwrap().command, "my-claude");
        // Their entry stays first, so the default provider is unchanged.
        assert_eq!(cfg.chat.providers[0].id, "claude");

        // …and an agent added to the built-ins later reaches them, rather
        // than being shadowed by the list they happened to write once.
        let mut chat = cfg.chat.clone();
        let newcomer = ChatProvider {
            id: "newcomer".into(),
            label: "Newcomer".into(),
            command: "newcomer".into(),
            driver: "claude-stream-json".into(),
            models: vec![],
            default_model: None,
            experimental: false,
        };
        merge_providers_from(&mut chat, vec![newcomer]);
        assert!(
            chat.provider("newcomer").is_some(),
            "a new built-in was dropped"
        );
        assert_eq!(
            chat.providers[0].id, "claude",
            "the newcomer stole first place"
        );
    }

    #[test]
    fn the_default_provider_falls_back_to_the_first_listed() {
        let cfg = Config::default();
        assert_eq!(
            cfg.chat.default_provider().map(|p| p.id.as_str()),
            Some("claude")
        );
    }

    #[test]
    fn user_override_of_builtin_wins() {
        let raw = r#"{ "roots": ["/x"], "clis": { "claude": { "command": "my-claude" } } }"#;
        let cfg = merge_with_defaults(raw, Path::new("/tmp/x.json")).unwrap();
        assert_eq!(cfg.clis["claude"].command, "my-claude");
        assert!(cfg.clis.contains_key("claude-chat"));
    }
}
