//! Config & preferences loader for agent-start.
//!
//! All state lives under `~/.agent-start/`; override with
//! `AGENT_START_HOME=<path>` (or finer-grained `AGENT_START_CONFIG`,
//! `AGENT_START_PREFS`, `AGENT_START_WORKTREE_ROOT`). The schema mirrors
//! the Node `lib/config.ts` / `lib/preferences.ts` it replaced —
//! `migrate_legacy_layout()` relocates files from the previous XDG
//! layout on first boot, and `claudeCommand` is rewritten into
//! `clis.claude.command` automatically.

mod config;
mod error;
mod io;
mod migrate;
mod paths;
mod preferences;

pub use config::{is_path_under_roots, load_config, CliConfig, Config};
pub use error::ConfigError;
pub use migrate::migrate_legacy_layout;
pub use paths::{
    agent_start_home, config_path, expand_root, host_state_dir, preferences_path, worktree_root,
};
pub use preferences::{
    build_launch_command, load_preferences, sanitize_extra_args, save_preferences, Preferences,
};

#[cfg(test)]
pub use config::clear_cache;
