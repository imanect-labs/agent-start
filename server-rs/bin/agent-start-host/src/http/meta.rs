use agent_start_api::{HealthBody, UpdateCheckBody, VersionBody};
use axum::extract::State;
use axum::Json;
use std::time::{Duration, Instant};

use crate::app::Shared;

pub async fn health() -> Json<HealthBody> {
    Json(HealthBody { ok: true })
}

pub async fn version() -> Json<VersionBody> {
    Json(VersionBody {
        name: "agent-start-host".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    })
}

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/imanect-labs/agent-start/releases/latest";
/// How long a successful check stays fresh before we hit GitHub again.
const CACHE_TTL: Duration = Duration::from_secs(6 * 60 * 60);

/// Report whether a newer `agent-start` release is available on GitHub.
///
/// Best-effort and cached: a successful check is reused for `CACHE_TTL` so
/// the UI/CLI can poll freely. Any failure (offline, rate-limited, parse
/// error) resolves to `available: false` and is *not* cached, so the next
/// poll retries.
pub async fn update_check(State(state): State<Shared>) -> Json<UpdateCheckBody> {
    if let Some((fetched, body)) = state.update_cache.read().clone() {
        if fetched.elapsed() < CACHE_TTL {
            return Json(body);
        }
    }

    let body = match fetch_latest().await {
        Some((latest, html_url)) => {
            let available = is_newer(&latest, CURRENT_VERSION);
            let body = UpdateCheckBody {
                current: CURRENT_VERSION.to_string(),
                latest: Some(latest),
                available,
                html_url: Some(html_url),
            };
            *state.update_cache.write() = Some((Instant::now(), body.clone()));
            body
        }
        None => UpdateCheckBody {
            current: CURRENT_VERSION.to_string(),
            latest: None,
            available: false,
            html_url: None,
        },
    };

    Json(body)
}

/// Fetch the latest release `tag_name` + `html_url` from GitHub. Returns
/// `None` on any error so callers can degrade gracefully.
async fn fetch_latest() -> Option<(String, String)> {
    let resp = reqwest::Client::new()
        .get(LATEST_RELEASE_API)
        // GitHub rejects requests without a User-Agent.
        .header(reqwest::header::USER_AGENT, "agent-start-host")
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let json: serde_json::Value = resp.json().await.ok()?;
    let tag = json.get("tag_name")?.as_str()?.to_string();
    let html_url = json
        .get("html_url")
        .and_then(|v| v.as_str())
        .unwrap_or("https://github.com/imanect-labs/agent-start/releases")
        .to_string();
    Some((tag, html_url))
}

/// Compare two version strings (with optional leading `v`/`V`) by their
/// dot-separated numeric components. Returns true only when `latest` is
/// strictly greater than `current`. If either string has a segment that
/// isn't a plain integer, the parse fails and we decline to flag an update —
/// a malformed tag should never trigger a false "update available".
fn is_newer(latest: &str, current: &str) -> bool {
    let parse = |s: &str| -> Option<Vec<u64>> {
        s.trim_start_matches(['v', 'V'])
            .split('.')
            .map(|p| p.trim().parse::<u64>().ok())
            .collect()
    };
    let (Some(l), Some(c)) = (parse(latest), parse(current)) else {
        return false;
    };
    let n = l.len().max(c.len());
    for i in 0..n {
        let lv = l.get(i).copied().unwrap_or(0);
        let cv = c.get(i).copied().unwrap_or(0);
        if lv != cv {
            return lv > cv;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::is_newer;

    #[test]
    fn detects_newer_versions() {
        assert!(is_newer("v0.2.0", "0.1.0"));
        assert!(is_newer("0.1.1", "v0.1.0"));
        assert!(is_newer("v0.2", "v0.1.5"));
        assert!(is_newer("v1.0.0", "v0.9.9"));
    }

    #[test]
    fn ignores_same_or_older() {
        assert!(!is_newer("v0.1.0", "v0.1.0"));
        assert!(!is_newer("v0.1.0", "v0.2.0"));
        assert!(!is_newer("garbage", "v0.1.0"));
    }
}
