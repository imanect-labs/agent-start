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
    write_private(path, json.as_bytes())
}

/// Write owner-only from the first byte. Creating the file and then
/// tightening it leaves the token world-readable in between, which is
/// exactly the window a local attacker needs.
#[cfg(unix)]
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    // `mode` only applies when the file is created, so an existing file
    // keeps whatever mode it had; re-assert it.
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    file.write_all(bytes)
}

#[cfg(not(unix))]
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, bytes)
}

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

    #[cfg(unix)]
    #[test]
    fn the_identity_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node-identity.json");
        // Pre-create it world-readable: rewriting must not leave it so.
        std::fs::write(&path, "{}").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        save_identity(
            &path,
            &Identity {
                node_id: "n-1".into(),
                token: "secret".into(),
            },
        )
        .unwrap();

        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "identity file is readable by others: {mode:o}");
    }
}
