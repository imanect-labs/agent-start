//! Project / session-name helpers shared between the host and the CLI.

use std::path::Path;

/// Slug a project name into a tmux-style identifier.
pub fn slugify(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut last_dash = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "project".to_string()
    } else {
        trimmed
    }
}

/// Stable identifier for a project, derived from its path on the
/// control plane. It names the node-local mirror cache directory, so it
/// must be filesystem-safe and stable across restarts — but it never
/// needs to agree between two different control planes.
pub fn project_id(path: &Path) -> String {
    // FNV-1a over the full path disambiguates same-named projects under
    // different roots without pulling in a hash crate.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in path.to_string_lossy().as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "project".into());
    let slug: String = slugify(&name).chars().take(32).collect();
    format!("{slug}-{hash:016x}")
}

/// Environment handed to a freshly spawned session process (PTY or
/// chat), so the agent CLI can find its own workspace. Lives here
/// because both the HTTP layer and the node runtime spawn agents and
/// the two must agree on the variable names.
pub fn launch_env(orig: &Path, name: &str, cwd: &Path) -> Vec<(String, String)> {
    vec![
        (
            "AGENT_START_ROOT_PATH".into(),
            orig.to_string_lossy().into_owned(),
        ),
        ("AGENT_START_WORKSPACE_NAME".into(), name.to_string()),
        (
            "AGENT_START_WORKSPACE_PATH".into(),
            cwd.to_string_lossy().into_owned(),
        ),
        ("TERM".into(), "xterm-256color".into()),
    ]
}

/// `<prefix><slug>-<unix-seconds><suffix>`
///
/// The seconds alone are not enough: a scheduler placing a burst of
/// sessions across a cluster easily creates several within the same
/// second, and a collision means two sessions fighting over one
/// worktree, one branch and one primary key.
pub fn session_name(prefix: &str, project_name: &str) -> String {
    let slug: String = slugify(project_name).chars().take(32).collect();
    let ts = chrono::Utc::now().timestamp();
    format!("{prefix}{slug}-{ts}{}", random_suffix())
}

/// Eight lowercase base-36 characters (~2.8e12 values) from the OS RNG.
///
/// Eight rather than four: at four the space is only 1.7M, which sounds
/// ample until you run the birthday bound — 500 names collide about 7%
/// of the time. Eight puts that at ~4e-8.
fn random_suffix() -> String {
    const ALPHABET: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    const LEN: usize = 8;
    // Consume the UUID as one integer rather than one byte per
    // character: `byte % 36` would over-represent the first four
    // letters, and 36^8 is a vanishing fraction of a v4 UUID's 122 bits.
    let mut n = u128::from_le_bytes(uuid::Uuid::new_v4().into_bytes());
    let mut out = String::with_capacity(LEN);
    for _ in 0..LEN {
        out.push(ALPHABET[(n % ALPHABET.len() as u128) as usize] as char);
        n /= ALPHABET.len() as u128;
    }
    out
}

const SESSION_NAME_ALLOWED: &str =
    "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_-";

pub fn is_valid_session_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| SESSION_NAME_ALLOWED.contains(c))
}

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Scan configured `roots` for project directories. Mirrors `lib/projects.ts`.
pub fn list_projects(
    cfg: &config_loader::Config,
) -> Result<Vec<agent_start_api::Project>, ScanError> {
    let mut out = Vec::new();
    for root in &cfg.roots {
        let root_path = config_loader::expand_root(root);
        let Ok(entries) = std::fs::read_dir(&root_path) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if !cfg.show_hidden && name.starts_with('.') {
                continue;
            }
            let full = entry.path();
            let is_git = has_git_dir(&full);
            if cfg.git_only && !is_git {
                continue;
            }
            let mtime_ms = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            out.push(agent_start_api::Project {
                name,
                path: full.to_string_lossy().into_owned(),
                root: root_path.to_string_lossy().into_owned(),
                mtime_ms,
                is_git,
            });
        }
    }
    out.sort_by_key(|p| std::cmp::Reverse(p.mtime_ms));
    Ok(out)
}

fn has_git_dir(p: &Path) -> bool {
    std::fs::metadata(p.join(".git"))
        .map(|m| m.is_dir() || m.is_file())
        .unwrap_or(false)
}

/// Pre-accept the Claude Code workspace trust dialog by editing `~/.claude.json`.
pub fn mark_claude_trusted(dir: &Path) -> std::io::Result<()> {
    let home = dirs::home_dir().ok_or_else(|| std::io::Error::other("no home"))?;
    let path = home.join(".claude.json");
    let mut cfg: serde_json::Value = match std::fs::read_to_string(&path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_else(|_| serde_json::json!({})),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
        Err(err) => return Err(err),
    };
    let target = dir.to_string_lossy().into_owned();
    let projects = cfg
        .as_object_mut()
        .ok_or_else(|| std::io::Error::other("invalid claude config"))?
        .entry("projects".to_string())
        .or_insert_with(|| serde_json::json!({}));
    let projects = projects
        .as_object_mut()
        .ok_or_else(|| std::io::Error::other("invalid projects map"))?;
    let entry = projects
        .entry(target)
        .or_insert_with(|| serde_json::json!({}));
    if entry
        .get("hasTrustDialogAccepted")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return Ok(());
    }
    let new_entry = serde_json::json!({
        "allowedTools": [],
        "mcpContextUris": [],
        "mcpServers": {},
        "enabledMcpjsonServers": [],
        "disabledMcpjsonServers": [],
        "hasTrustDialogAccepted": true,
    });
    if let Some(obj) = entry.as_object_mut() {
        if let Some(new_obj) = new_entry.as_object() {
            for (k, v) in new_obj {
                obj.entry(k.clone()).or_insert_with(|| v.clone());
            }
        }
        obj.insert(
            "hasTrustDialogAccepted".into(),
            serde_json::Value::Bool(true),
        );
    } else {
        *entry = new_entry;
    }

    let tmp = path.with_extension(format!("agent-start.tmp.{}", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec_pretty(&cfg).unwrap_or_default())?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("Hello World!!"), "hello-world");
        assert_eq!(slugify("AbcDEF"), "abcdef");
        assert_eq!(slugify("..."), "project");
    }

    #[test]
    fn session_name_valid() {
        let n = session_name("cc-", "my project");
        assert!(n.starts_with("cc-my-project-"));
        assert!(is_valid_session_name(&n));
    }

    #[test]
    fn session_names_do_not_collide_within_one_second() {
        // A cluster places bursts of sessions; second-resolution names
        // alone would hand two of them the same worktree and branch.
        // 2000 draws from 36^8 collide with probability ~7e-7, so this
        // is a real assertion rather than a coin flip.
        const N: usize = 2000;
        let names: std::collections::HashSet<String> =
            (0..N).map(|_| session_name("cc-", "demo")).collect();
        assert_eq!(names.len(), N, "duplicate session names generated");
    }

    #[test]
    fn the_random_suffix_uses_its_whole_alphabet() {
        // A modulo-per-byte suffix would never emit the tail of the
        // alphabet evenly; sample enough to see the whole range.
        let seen: std::collections::HashSet<char> = (0..2000)
            .flat_map(|_| random_suffix().chars().collect::<Vec<_>>())
            .collect();
        assert_eq!(seen.len(), 36, "suffix alphabet is skewed: {seen:?}");
        assert!(random_suffix().len() == 8);
    }

    #[test]
    fn project_id_separates_same_named_projects_under_different_roots() {
        let a = project_id(Path::new("/srv/work/api"));
        let b = project_id(Path::new("/home/me/api"));
        assert_ne!(a, b);
        assert!(a.starts_with("api-"), "unreadable id: {a}");
        // Stable across calls — it names a cache directory.
        assert_eq!(a, project_id(Path::new("/srv/work/api")));
    }
}
