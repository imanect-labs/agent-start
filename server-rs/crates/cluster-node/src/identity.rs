//! Persisted node identity.
//!
//! A join token is a one-shot credential; what the node keeps is the
//! long-lived token the control plane hands back in `Welcome`. Storing
//! it means a node survives a restart as the *same* node — its labels,
//! its history and its cache affinity all stay attached — instead of
//! reappearing as a stranger next to its own stale row.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    #[serde(rename = "nodeId")]
    pub node_id: String,
    pub token: String,
}

pub fn load_identity(path: &Path) -> Option<Identity> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

pub fn save_identity(path: &Path, id: &Identity) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(id).map_err(std::io::Error::other)?;
    std::fs::write(path, json)?;
    restrict_permissions(path);
    Ok(())
}

/// The file holds a cluster credential, so keep it owner-only. Failure
/// is logged rather than fatal: a filesystem without Unix modes (or a
/// Windows node) should still be able to register.
#[cfg(unix)]
fn restrict_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Err(e) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
        tracing::warn!(error = %e, path = %path.display(), "could not restrict identity file mode");
    }
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_round_trips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node-identity.json");
        assert!(load_identity(&path).is_none());
        save_identity(
            &path,
            &Identity {
                node_id: "n-1".into(),
                token: "secret".into(),
            },
        )
        .unwrap();
        let back = load_identity(&path).unwrap();
        assert_eq!(back.node_id, "n-1");
        assert_eq!(back.token, "secret");
    }
}
