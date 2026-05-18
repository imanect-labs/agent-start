//! Internal JSON file helpers shared by `config` and `preferences`.

use crate::error::ConfigError;
use serde::Serialize;
use std::path::Path;

pub(crate) fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}
