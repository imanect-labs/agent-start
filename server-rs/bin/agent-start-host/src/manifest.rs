//! Write a small runtime manifest describing how the CLI / scripts can
//! talk to the running host (URL + PID). Stored under
//! `$XDG_DATA_HOME/agent-start/runtime/manifest.json`.

use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Serialize)]
struct Manifest<'a> {
    url: String,
    pid: u32,
    addr: &'a str,
    started_at: i64,
}

pub fn manifest_path() -> PathBuf {
    config_loader::host_state_dir()
        .join("runtime")
        .join("manifest.json")
}

pub fn write(addr: &str) -> std::io::Result<()> {
    let path = manifest_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let manifest = Manifest {
        url: format!("http://{addr}"),
        pid: std::process::id(),
        addr,
        started_at: chrono::Utc::now().timestamp(),
    };
    std::fs::write(path, serde_json::to_vec_pretty(&manifest)?)?;
    Ok(())
}
