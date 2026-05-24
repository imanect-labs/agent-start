//! `POST /api/projects/clone` / `POST /api/projects/import` /
//! `DELETE /api/projects/:name` — long-running clone/import use an
//! optimistic `*.partial` directory + `/api/projects` polling; the
//! handler returns `202 Accepted` immediately and spawns a background
//! task that promotes `<name>.partial` → `<name>` on completion or
//! writes `<name>.error` on failure.

use super::err;
use agent_start_api::{CloneRequest, ImportRequest, ProjectOpResponse};
use axum::extract::Path as AxPath;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use config_loader::safety::ensure_under;
use std::path::{Path, PathBuf};

fn projects_root() -> PathBuf {
    config_loader::projects_dir()
}

fn slugify(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
            out.push(ch);
        } else if ch == ' ' || ch == '/' {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches(|c: char| c == '.' || c == '-' || c == '_');
    if trimmed.is_empty() {
        "project".to_string()
    } else {
        trimmed.to_string()
    }
}

fn derive_name_from_url(url: &str) -> String {
    let last = url
        .trim_end_matches('/')
        .rsplit_once('/')
        .map(|(_, t)| t)
        .unwrap_or(url);
    let stem = last.strip_suffix(".git").unwrap_or(last);
    slugify(stem)
}

fn unique_name(root: &Path, base: &str) -> String {
    let mut candidate = base.to_string();
    let mut n = 2u32;
    while root.join(&candidate).exists()
        || root.join(format!("{}.partial", &candidate)).exists()
        || root.join(format!("{}.error", &candidate)).exists()
    {
        candidate = format!("{}-{}", base, n);
        n += 1;
        if n > 999 {
            break;
        }
    }
    candidate
}

fn write_error_marker(root: &Path, name: &str, msg: &str) {
    let path = root.join(format!("{}.error", name));
    let _ = std::fs::write(&path, msg);
    let _ = std::fs::remove_dir_all(root.join(format!("{}.partial", name)));
}

pub async fn clone_project(Json(req): Json<CloneRequest>) -> Response {
    let root = projects_root();
    if let Err(e) = std::fs::create_dir_all(&root) {
        return err(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("create projects dir: {e}"),
        );
    }

    let base = req
        .name
        .as_deref()
        .map(slugify)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| derive_name_from_url(&req.url));
    let name = unique_name(&root, &base);
    let partial = root.join(format!("{}.partial", &name));

    // Reserve the partial path before spawning so concurrent calls don't
    // race on the same name.
    if let Err(e) = std::fs::create_dir_all(&partial) {
        return err(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("reserve dir: {e}"),
        );
    }
    let _ = std::fs::write(partial.join(".agent-start-kind"), "clone");

    let url = req.url.clone();
    let root_clone = root.clone();
    let name_for_task = name.clone();
    tokio::spawn(async move {
        // git clone writes into an empty target; the partial dir holds a
        // marker file plus an empty `repo/` we clone into.
        let target = root_clone
            .join(format!("{}.partial", &name_for_task))
            .join("repo");
        let res = tokio::task::spawn_blocking(move || git_ops::clone(&url, &target)).await;
        let outcome = match res {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(e.to_string()),
            Err(e) => Err(format!("join error: {e}")),
        };
        match outcome {
            Ok(()) => {
                let from = root_clone
                    .join(format!("{}.partial", &name_for_task))
                    .join("repo");
                let to = root_clone.join(&name_for_task);
                if let Err(e) = std::fs::rename(&from, &to) {
                    write_error_marker(&root_clone, &name_for_task, &format!("rename: {e}"));
                    return;
                }
                let _ =
                    std::fs::remove_dir_all(root_clone.join(format!("{}.partial", &name_for_task)));
            }
            Err(msg) => write_error_marker(&root_clone, &name_for_task, &msg),
        }
    });

    let path = root.join(&name).to_string_lossy().into_owned();
    (StatusCode::ACCEPTED, Json(ProjectOpResponse { name, path })).into_response()
}

pub async fn import_project(Json(req): Json<ImportRequest>) -> Response {
    let src = PathBuf::from(&req.src);
    let canon = match std::fs::canonicalize(&src) {
        Ok(p) => p,
        Err(e) => return err(StatusCode::BAD_REQUEST, format!("source canonicalize: {e}")),
    };
    if !canon.is_dir() {
        return err(StatusCode::BAD_REQUEST, "source is not a directory");
    }

    let root = projects_root();
    if let Err(e) = std::fs::create_dir_all(&root) {
        return err(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("create projects dir: {e}"),
        );
    }

    let base = req
        .name
        .as_deref()
        .map(slugify)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            slugify(
                canon
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "project".to_string())
                    .as_str(),
            )
        });
    let name = unique_name(&root, &base);
    let partial = root.join(format!("{}.partial", &name));
    if let Err(e) = std::fs::create_dir_all(&partial) {
        return err(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("reserve dir: {e}"),
        );
    }
    let _ = std::fs::write(partial.join(".agent-start-kind"), "import");

    let root_clone = root.clone();
    let name_for_task = name.clone();
    let name_for_marker = name.clone();
    let src_for_task = canon.clone();
    tokio::spawn(async move {
        let res = tokio::task::spawn_blocking(move || {
            let target = root_clone
                .join(format!("{}.partial", &name_for_task))
                .join("data");
            copy_dir_recursive(&src_for_task, &target)?;
            std::fs::rename(&target, root_clone.join(&name_for_task))?;
            let _ = std::fs::remove_dir_all(root_clone.join(format!("{}.partial", &name_for_task)));
            Ok::<_, std::io::Error>(())
        })
        .await;
        if let Ok(Err(e)) = res {
            // Marker must be keyed on the real project name so the
            // sidebar's `<name>.partial` row flips to `<name>.error`
            // and the partial dir gets cleaned up. A literal "_import"
            // would leak the partial and never surface the failure.
            let root_for_marker = config_loader::projects_dir();
            write_error_marker(&root_for_marker, &name_for_marker, &e.to_string());
        }
    });

    let path = root.join(&name).to_string_lossy().into_owned();
    (StatusCode::ACCEPTED, Json(ProjectOpResponse { name, path })).into_response()
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else if ty.is_symlink() {
            // skip symlinks to avoid follow-loops; could re-create later.
            continue;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

pub async fn delete_project(AxPath(name): AxPath<String>) -> Response {
    let root = projects_root();
    let target = root.join(&name);
    let resolved = match ensure_under(&root, &target) {
        Ok(p) => p,
        Err(_) => return err(StatusCode::FORBIDDEN, "project name escapes projects root"),
    };
    if !resolved.exists() {
        // Also accept deletion of stale `.partial` / `.error` markers.
        let partial = root.join(format!("{}.partial", &name));
        let err_marker = root.join(format!("{}.error", &name));
        let mut removed = false;
        if partial.exists() {
            let _ = std::fs::remove_dir_all(&partial);
            removed = true;
        }
        if err_marker.exists() {
            let _ = std::fs::remove_file(&err_marker);
            removed = true;
        }
        if removed {
            return Json(serde_json::json!({"ok": true})).into_response();
        }
        return err(StatusCode::NOT_FOUND, "project not found");
    }
    if let Err(e) = std::fs::remove_dir_all(&resolved) {
        return err(StatusCode::INTERNAL_SERVER_ERROR, format!("remove: {e}"));
    }
    // Best-effort cleanup of sibling markers.
    let _ = std::fs::remove_file(root.join(format!("{}.error", &name)));
    let _ = std::fs::remove_dir_all(root.join(format!("{}.partial", &name)));

    Json(serde_json::json!({"ok": true})).into_response()
}
