//! Node registry endpoints.
//!
//! Read-only listing plus the two operator levers Phase 1 needs:
//! cordoning a node (stop giving it work without disturbing what it is
//! already running) and minting a join token for a new one.

use super::err;
use agent_start_api::{
    JoinTokenRequest, JoinTokenResponse, NodeLabel, NodePatch, NodeSummary, NodesBody,
};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use cluster_control::{NodeView, PatchError};
use std::time::Duration;

use crate::app::Shared;

fn to_summary(v: &NodeView) -> NodeSummary {
    NodeSummary {
        id: v.info.id.clone(),
        name: v.info.name.clone(),
        status: v.status.as_str().to_string(),
        connected: v.connected,
        cordoned: v.cordoned,
        is_local: v.info.is_local,
        version: v.info.version.clone(),
        os: v.info.os.clone(),
        arch: v.info.arch.clone(),
        executors: v
            .info
            .executors
            .iter()
            .map(|p| p.as_str().to_string())
            .collect(),
        capacity_cpu_millis: v.info.capacity.cpu_millis,
        capacity_mem_mb: v.info.capacity.mem_mb,
        reserved_cpu_millis: v.reserved.cpu_millis,
        reserved_mem_mb: v.reserved.mem_mb,
        max_sessions: v.max_sessions,
        cpu_util: v.metrics.cpu_util,
        mem_util: v.metrics.mem_util,
        load1: v.metrics.load1,
        labels: v
            .info
            .labels
            .iter()
            .map(|(key, value)| NodeLabel {
                key: key.clone(),
                value: value.clone(),
            })
            .collect(),
        sessions: v.running.clone(),
        cached_projects: v.repo_cache.len(),
        last_heartbeat_ms: v.last_heartbeat_ms,
    }
}

pub async fn list_nodes(State(app): State<Shared>) -> Response {
    let Some(control) = app.cluster.as_ref() else {
        // `--role node` has no registry of its own. Answer with an
        // empty, well-formed body so the UI can render "not clustered"
        // instead of an error banner.
        return Json(NodesBody {
            nodes: Vec::new(),
            clustered: false,
        })
        .into_response();
    };
    let nodes = control.nodes().iter().map(to_summary).collect();
    Json(NodesBody {
        nodes,
        clustered: true,
    })
    .into_response()
}

pub async fn get_node(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    let Some(control) = app.cluster.as_ref() else {
        return err(StatusCode::NOT_FOUND, "this host has no node registry");
    };
    match control.node(&id) {
        Some(view) => Json(to_summary(&view)).into_response(),
        None => err(StatusCode::NOT_FOUND, "unknown node"),
    }
}

pub async fn patch_node(
    State(app): State<Shared>,
    Path(id): Path<String>,
    Json(body): Json<NodePatch>,
) -> Response {
    let Some(control) = app.cluster.as_ref() else {
        return err(StatusCode::NOT_FOUND, "this host has no node registry");
    };
    let labels = body
        .labels
        .map(|l| l.into_iter().map(|e| (e.key, e.value)).collect());
    match control
        .patch_node(&id, labels, body.max_sessions, body.cordoned)
        .await
    {
        Ok(view) => Json(to_summary(&view)).into_response(),
        // A storage failure is ours, not the caller's; reporting it as
        // 404 would send an operator hunting for a node that is right
        // there.
        Err(e @ PatchError::UnknownNode) => err(StatusCode::NOT_FOUND, e.to_string()),
        Err(e @ PatchError::Store(_)) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

pub async fn delete_node(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    let Some(control) = app.cluster.as_ref() else {
        return err(StatusCode::NOT_FOUND, "this host has no node registry");
    };
    if control.local_node_id().as_deref() == Some(id.as_str()) {
        return err(
            StatusCode::CONFLICT,
            "the in-process node cannot be removed; stop the host instead",
        );
    }
    match control.remove_node(&id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

/// Mint a join token. The plaintext is shown exactly once — it is not
/// stored, only its hash is — so the response includes the full command
/// to run on the new machine.
pub async fn create_join_token(
    State(app): State<Shared>,
    body: Option<Json<JoinTokenRequest>>,
) -> Response {
    let Some(control) = app.cluster.as_ref() else {
        return err(StatusCode::NOT_FOUND, "this host has no node registry");
    };
    let body = body.map(|Json(b)| b).unwrap_or_default();
    let ttl_secs = body.ttl_secs.unwrap_or(3600).clamp(60, 30 * 24 * 3600);
    let uses = body.uses.unwrap_or(1).clamp(1, 100);

    match control
        .issue_join_token(Duration::from_secs(ttl_secs), uses)
        .await
    {
        Ok(token) => {
            let expires_at_ms = chrono::Utc::now().timestamp_millis() + (ttl_secs as i64) * 1000;
            let command = format!(
                "agent-start-host --role node --join-url <control-plane-url> --join-token {token}"
            );
            Json(JoinTokenResponse {
                token,
                expires_at_ms,
                uses,
                command,
            })
            .into_response()
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}
