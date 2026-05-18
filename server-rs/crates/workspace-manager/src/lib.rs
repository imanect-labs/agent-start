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

/// `<prefix><slug>-<unix-seconds>`
pub fn session_name(prefix: &str, project_name: &str) -> String {
    let slug: String = slugify(project_name).chars().take(32).collect();
    let ts = chrono::Utc::now().timestamp();
    format!("{prefix}{slug}-{ts}")
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
}
