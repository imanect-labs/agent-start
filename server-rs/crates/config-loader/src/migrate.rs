//! One-time migration from the old XDG layout
//! (`~/.config/agent-start/`, `~/.local/share/agent-start/`,
//! `~/.cache/agent-start/worktrees/`) into the unified
//! `~/.agent-start/` directory introduced in mid-2026.

use crate::error::ConfigError;
use crate::paths::{agent_start_home, home};
use std::path::PathBuf;

/// True when any path-override env var is set; in that case we leave
/// the user's explicit layout alone.
fn user_overrode_paths() -> bool {
    [
        "AGENT_START_CONFIG",
        "AGENT_START_PREFS",
        "AGENT_START_HOME",
    ]
    .iter()
    .any(|k| std::env::var_os(k).is_some())
}

fn xdg_config_agent_start() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"))
        .join("agent-start")
}

fn xdg_data_agent_start() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".local").join("share"))
        .join("agent-start")
}

/// Relocate legacy files into `~/.agent-start/`. Idempotent and best-effort:
/// skips any file already present at the destination, ignores missing
/// sources, and never relocates existing git worktrees (`git worktree`
/// records absolute paths).
pub fn migrate_legacy_layout() -> Result<(), ConfigError> {
    if user_overrode_paths() {
        return Ok(());
    }
    let dest = agent_start_home();
    let xdg_config = xdg_config_agent_start();
    let xdg_data = xdg_data_agent_start();

    let moves: [(PathBuf, PathBuf); 5] = [
        (xdg_config.join("config.json"), dest.join("config.json")),
        (
            xdg_config.join("preferences.json"),
            dest.join("preferences.json"),
        ),
        (xdg_data.join("host.db"), dest.join("host.db")),
        (xdg_data.join("host.db-wal"), dest.join("host.db-wal")),
        (xdg_data.join("host.db-shm"), dest.join("host.db-shm")),
    ];

    let need_migrate = moves.iter().any(|(src, dst)| src.exists() && !dst.exists());
    let old_runtime = xdg_data.join("runtime").join("manifest.json");
    let new_runtime = dest.join("runtime").join("manifest.json");
    let need_runtime = old_runtime.exists() && !new_runtime.exists();
    if !need_migrate && !need_runtime {
        return Ok(());
    }
    std::fs::create_dir_all(&dest)?;

    for (src, dst) in moves {
        relocate(&src, &dst)?;
    }
    if need_runtime {
        if let Some(p) = new_runtime.parent() {
            std::fs::create_dir_all(p)?;
        }
        relocate(&old_runtime, &new_runtime)?;
    }
    Ok(())
}

fn relocate(src: &std::path::Path, dst: &std::path::Path) -> Result<(), ConfigError> {
    if !src.exists() || dst.exists() {
        return Ok(());
    }
    tracing::info!(from = %src.display(), to = %dst.display(), "migrating legacy file");
    // `rename` is atomic within a single filesystem; copy+remove is the
    // fallback when src/dst straddle filesystems (e.g. `~/.cache` on a
    // tmpfs while `~/.agent-start` is on the home volume).
    if std::fs::rename(src, dst).is_err() {
        std::fs::copy(src, dst)?;
        let _ = std::fs::remove_file(src);
    }
    Ok(())
}
