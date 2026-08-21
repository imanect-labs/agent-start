//! Persistence for the node registry, its samples, and join tokens.
//!
//! The control plane keeps the authoritative view of live nodes in
//! memory (it holds their connections), so these rows exist for what
//! memory cannot answer: what a node was called and how it was
//! configured before the control plane last restarted, and whether a
//! presented credential is real.

use crate::{Db, StateError};
use chrono::Utc;
use sqlx::Row;

#[derive(Debug, Clone)]
pub struct NodeRow {
    pub id: String,
    pub name: String,
    pub token_hash: String,
    pub status: String,
    pub version: String,
    pub os: String,
    pub arch: String,
    /// Comma-separated isolation profiles.
    pub executors: String,
    pub capacity_cpu_millis: i64,
    pub capacity_mem_mb: i64,
    pub max_sessions: i64,
    /// JSON object of scheduling labels.
    pub labels: String,
    pub cordoned: bool,
    pub is_local: bool,
    pub last_heartbeat_ms: i64,
    pub created_at_ms: i64,
}

fn node_from_row(row: sqlx::sqlite::SqliteRow) -> NodeRow {
    NodeRow {
        id: row.get("id"),
        name: row.get("name"),
        token_hash: row.get("token_hash"),
        status: row.get("status"),
        version: row.get("version"),
        os: row.get("os"),
        arch: row.get("arch"),
        executors: row.get("executors"),
        capacity_cpu_millis: row.get("capacity_cpu_millis"),
        capacity_mem_mb: row.get("capacity_mem_mb"),
        max_sessions: row.get("max_sessions"),
        labels: row.get("labels"),
        cordoned: row.get::<i64, _>("cordoned") != 0,
        is_local: row.get::<i64, _>("is_local") != 0,
        last_heartbeat_ms: row.get("last_heartbeat_ms"),
        created_at_ms: row.get("created_at_ms"),
    }
}

const NODE_COLUMNS: &str = "id, name, token_hash, status, version, os, arch, executors, \
     capacity_cpu_millis, capacity_mem_mb, max_sessions, labels, cordoned, is_local, \
     last_heartbeat_ms, created_at_ms";

/// Insert or refresh a node's registration. `labels`, `cordoned` and
/// `max_sessions` are deliberately *not* overwritten on conflict: those
/// are operator decisions made through the API, and a node reconnecting
/// must not silently undo a cordon.
pub async fn upsert_node(db: &Db, n: &NodeRow) -> Result<(), StateError> {
    sqlx::query(
        "INSERT INTO nodes (id, name, token_hash, status, version, os, arch, executors, \
           capacity_cpu_millis, capacity_mem_mb, max_sessions, labels, cordoned, is_local, \
           last_heartbeat_ms, created_at_ms) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT(id) DO UPDATE SET \
           name = excluded.name, \
           token_hash = excluded.token_hash, \
           status = excluded.status, \
           version = excluded.version, \
           os = excluded.os, \
           arch = excluded.arch, \
           executors = excluded.executors, \
           capacity_cpu_millis = excluded.capacity_cpu_millis, \
           capacity_mem_mb = excluded.capacity_mem_mb, \
           is_local = excluded.is_local, \
           last_heartbeat_ms = excluded.last_heartbeat_ms",
    )
    .bind(&n.id)
    .bind(&n.name)
    .bind(&n.token_hash)
    .bind(&n.status)
    .bind(&n.version)
    .bind(&n.os)
    .bind(&n.arch)
    .bind(&n.executors)
    .bind(n.capacity_cpu_millis)
    .bind(n.capacity_mem_mb)
    .bind(n.max_sessions)
    .bind(&n.labels)
    .bind(i64::from(n.cordoned))
    .bind(i64::from(n.is_local))
    .bind(n.last_heartbeat_ms)
    .bind(n.created_at_ms)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn list_nodes(db: &Db) -> Result<Vec<NodeRow>, StateError> {
    let sql = format!("SELECT {NODE_COLUMNS} FROM nodes ORDER BY created_at_ms ASC");
    let rows = sqlx::query(&sql).fetch_all(db).await?;
    Ok(rows.into_iter().map(node_from_row).collect())
}

pub async fn get_node(db: &Db, id: &str) -> Result<Option<NodeRow>, StateError> {
    let sql = format!("SELECT {NODE_COLUMNS} FROM nodes WHERE id = ?");
    let row = sqlx::query(&sql).bind(id).fetch_optional(db).await?;
    Ok(row.map(node_from_row))
}

pub async fn get_node_by_name(db: &Db, name: &str) -> Result<Option<NodeRow>, StateError> {
    let sql = format!("SELECT {NODE_COLUMNS} FROM nodes WHERE name = ?");
    let row = sqlx::query(&sql).bind(name).fetch_optional(db).await?;
    Ok(row.map(node_from_row))
}

pub async fn set_node_status(
    db: &Db,
    id: &str,
    status: &str,
    last_heartbeat_ms: i64,
) -> Result<(), StateError> {
    sqlx::query("UPDATE nodes SET status = ?, last_heartbeat_ms = ? WHERE id = ?")
        .bind(status)
        .bind(last_heartbeat_ms)
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

/// Apply the operator-owned fields. Each is optional so a PATCH that
/// only cordons a node does not have to restate its labels.
pub async fn update_node_settings(
    db: &Db,
    id: &str,
    labels: Option<&str>,
    max_sessions: Option<i64>,
    cordoned: Option<bool>,
) -> Result<(), StateError> {
    if let Some(labels) = labels {
        sqlx::query("UPDATE nodes SET labels = ? WHERE id = ?")
            .bind(labels)
            .bind(id)
            .execute(db)
            .await?;
    }
    if let Some(max) = max_sessions {
        sqlx::query("UPDATE nodes SET max_sessions = ? WHERE id = ?")
            .bind(max)
            .bind(id)
            .execute(db)
            .await?;
    }
    if let Some(cordoned) = cordoned {
        sqlx::query("UPDATE nodes SET cordoned = ? WHERE id = ?")
            .bind(i64::from(cordoned))
            .bind(id)
            .execute(db)
            .await?;
    }
    Ok(())
}

pub async fn delete_node(db: &Db, id: &str) -> Result<(), StateError> {
    sqlx::query("DELETE FROM nodes WHERE id = ?")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

/// Mark every node not-ready. Run at boot: the control plane has no
/// connections yet, so claiming otherwise would let the scheduler
/// place work on a node that cannot hear it.
pub async fn mark_all_nodes_notready(db: &Db) -> Result<(), StateError> {
    sqlx::query("UPDATE nodes SET status = 'notready'")
        .execute(db)
        .await?;
    Ok(())
}

pub async fn record_node_metrics(
    db: &Db,
    node_id: &str,
    cpu_util: f64,
    mem_util: f64,
    load1: f64,
    running: i64,
) -> Result<(), StateError> {
    let now = Utc::now().timestamp_millis();
    sqlx::query(
        "INSERT OR REPLACE INTO node_metrics (node_id, at_ms, cpu_util, mem_util, load1, running) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(node_id)
    .bind(now)
    .bind(cpu_util)
    .bind(mem_util)
    .bind(load1)
    .bind(running)
    .execute(db)
    .await?;
    // Keep the table bounded without a separate sweeper: each write
    // drops everything but the newest `METRICS_KEEP` samples.
    sqlx::query(
        "DELETE FROM node_metrics WHERE node_id = ? AND at_ms NOT IN ( \
           SELECT at_ms FROM node_metrics WHERE node_id = ? ORDER BY at_ms DESC LIMIT ? \
         )",
    )
    .bind(node_id)
    .bind(node_id)
    .bind(METRICS_KEEP)
    .execute(db)
    .await?;
    Ok(())
}

/// Samples retained per node — 60 heartbeats, i.e. ten minutes at the
/// default interval.
const METRICS_KEEP: i64 = 60;

#[derive(Debug, Clone, Copy)]
pub struct MetricSample {
    pub at_ms: i64,
    pub cpu_util: f64,
    pub mem_util: f64,
    pub load1: f64,
    pub running: i64,
}

pub async fn node_metrics_history(
    db: &Db,
    node_id: &str,
    limit: i64,
) -> Result<Vec<MetricSample>, StateError> {
    let rows = sqlx::query(
        "SELECT at_ms, cpu_util, mem_util, load1, running FROM node_metrics \
         WHERE node_id = ? ORDER BY at_ms DESC LIMIT ?",
    )
    .bind(node_id)
    .bind(limit)
    .fetch_all(db)
    .await?;
    let mut out: Vec<MetricSample> = rows
        .into_iter()
        .map(|r| MetricSample {
            at_ms: r.get("at_ms"),
            cpu_util: r.get("cpu_util"),
            mem_util: r.get("mem_util"),
            load1: r.get("load1"),
            running: r.get("running"),
        })
        .collect();
    out.reverse(); // oldest first, ready to plot
    Ok(out)
}

/// Replace a node's cached-project set with what it just reported.
pub async fn replace_repo_cache(
    db: &Db,
    node_id: &str,
    project_ids: &[String],
) -> Result<(), StateError> {
    let now = Utc::now().timestamp_millis();
    let mut tx = db.begin().await?;
    sqlx::query("DELETE FROM node_repo_cache WHERE node_id = ?")
        .bind(node_id)
        .execute(&mut *tx)
        .await?;
    for pid in project_ids {
        sqlx::query(
            "INSERT OR REPLACE INTO node_repo_cache (node_id, project_id, last_seen_ms) \
             VALUES (?, ?, ?)",
        )
        .bind(node_id)
        .bind(pid)
        .bind(now)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn create_join_token(
    db: &Db,
    id: &str,
    token_hash: &str,
    expires_at_ms: i64,
    uses: i64,
) -> Result<(), StateError> {
    let now = Utc::now().timestamp_millis();
    sqlx::query(
        "INSERT INTO join_tokens (id, token_hash, expires_at_ms, uses_left, created_at_ms) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(token_hash)
    .bind(expires_at_ms)
    .bind(uses)
    .bind(now)
    .execute(db)
    .await?;
    Ok(())
}

/// Spend one use of a join token. Returns false when the token is
/// unknown, expired, or exhausted — the three cases are deliberately
/// indistinguishable to the caller so a prober learns nothing.
pub async fn consume_join_token(db: &Db, token_hash: &str) -> Result<bool, StateError> {
    let now = Utc::now().timestamp_millis();
    let result = sqlx::query(
        "UPDATE join_tokens SET uses_left = uses_left - 1 \
         WHERE token_hash = ? AND uses_left > 0 AND expires_at_ms > ?",
    )
    .bind(token_hash)
    .bind(now)
    .execute(db)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn purge_spent_join_tokens(db: &Db) -> Result<(), StateError> {
    let now = Utc::now().timestamp_millis();
    sqlx::query("DELETE FROM join_tokens WHERE uses_left <= 0 OR expires_at_ms <= ?")
        .bind(now)
        .execute(db)
        .await?;
    Ok(())
}

/// Record which node a session landed on. An empty id means the local
/// node, which is what every pre-cluster row already says.
pub async fn set_session_node(db: &Db, session: &str, node_id: &str) -> Result<(), StateError> {
    sqlx::query("UPDATE sessions SET node_id = ? WHERE name = ?")
        .bind(node_id)
        .bind(session)
        .execute(db)
        .await?;
    Ok(())
}
