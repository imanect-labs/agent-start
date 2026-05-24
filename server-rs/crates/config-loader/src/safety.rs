//! Path-safety helpers used by all file-system facing HTTP handlers.
//!
//! `ensure_under` canonicalizes `base` plus the parent of `candidate` and
//! verifies the result stays under `base`. Resolving the parent (rather
//! than `candidate` itself) lets callers validate paths that do not yet
//! exist — important for write/create endpoints.

use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum SafetyError {
    OutsideBase,
    BadInput(String),
}

impl std::fmt::Display for SafetyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SafetyError::OutsideBase => write!(f, "path is outside the allowed base directory"),
            SafetyError::BadInput(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for SafetyError {}

/// Resolve `candidate` and confirm it stays under `base`.
pub fn ensure_under(base: &Path, candidate: &Path) -> Result<PathBuf, SafetyError> {
    let base_canon = std::fs::canonicalize(base)
        .map_err(|e| SafetyError::BadInput(format!("base canonicalize failed: {e}")))?;

    let resolved = if candidate.exists() {
        std::fs::canonicalize(candidate)
            .map_err(|e| SafetyError::BadInput(format!("canonicalize failed: {e}")))?
    } else {
        let parent = candidate
            .parent()
            .ok_or_else(|| SafetyError::BadInput("missing parent".to_string()))?;
        let file = candidate
            .file_name()
            .ok_or_else(|| SafetyError::BadInput("missing file name".to_string()))?;
        let parent_canon = std::fs::canonicalize(parent)
            .map_err(|e| SafetyError::BadInput(format!("parent canonicalize failed: {e}")))?;
        parent_canon.join(file)
    };

    if resolved == base_canon || resolved.starts_with(&base_canon) {
        Ok(resolved)
    } else {
        Err(SafetyError::OutsideBase)
    }
}

/// True if `candidate` resolves under any of `bases`.
pub fn under_any(bases: &[PathBuf], candidate: &Path) -> Option<PathBuf> {
    for b in bases {
        if let Ok(p) = ensure_under(b, candidate) {
            return Some(p);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn allows_inside_base() {
        let dir = tempdir().unwrap();
        let base = dir.path();
        let inner = base.join("a.txt");
        fs::write(&inner, "x").unwrap();
        let resolved = ensure_under(base, &inner).unwrap();
        assert!(resolved.starts_with(base));
    }

    #[test]
    fn rejects_traversal() {
        let dir = tempdir().unwrap();
        let inside = dir.path().join("sub");
        fs::create_dir(&inside).unwrap();
        let outside = dir.path().parent().unwrap().join("escape");
        let err = ensure_under(&inside, &outside).unwrap_err();
        assert!(matches!(
            err,
            SafetyError::OutsideBase | SafetyError::BadInput(_)
        ));
    }
}
