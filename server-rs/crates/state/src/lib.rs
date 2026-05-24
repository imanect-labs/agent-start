//! SQLite-backed persistence for the host: session metadata and PTY
//! scrollback flushed from the in-memory ring buffer.
//!
//! Stored at `~/.agent-start/host.db` (override the base directory
//! with `AGENT_START_HOME`).

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions, SqliteRow};
use sqlx::{Pool, Row, Sqlite};
use std::path::PathBuf;
use std::str::FromStr;

pub type Db = Pool<Sqlite>;

#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("db: {0}")]
    Db(#[from] sqlx::Error),
    #[error("migrate: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRow {
    pub name: String,
    pub created_at_ms: i64,
    pub cli: String,
    pub cwd: String,
    pub command: String,
    pub worktree_path: String,
    pub orig_path: String,
    pub pid: Option<i64>,
    pub status: String,
}

impl SessionRow {
    fn from_row(row: SqliteRow) -> Self {
        Self {
            name: row.get("name"),
            created_at_ms: row.get("created_at_ms"),
            cli: row.get("cli"),
            cwd: row.get("cwd"),
            command: row.get("command"),
            worktree_path: row.get("worktree_path"),
            orig_path: row.get("orig_path"),
            pid: row.try_get("pid").ok(),
            status: row.get("status"),
        }
    }
}

impl From<&SessionRow> for agent_start_api::Session {
    fn from(row: &SessionRow) -> Self {
        Self {
            name: row.name.clone(),
            created_at: row.created_at_ms,
            attached: false, // filled in by the host using its live attach map
            stopped: row.status != "running",
            path: if row.worktree_path.is_empty() {
                row.cwd.clone()
            } else {
                row.worktree_path.clone()
            },
            cli: row.cli.clone(),
            worktree_path: row.worktree_path.clone(),
            orig_path: row.orig_path.clone(),
        }
    }
}

pub fn db_path() -> PathBuf {
    config_loader::host_state_dir().join("host.db")
}

pub async fn open() -> Result<Db, StateError> {
    let path = db_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let url = format!("sqlite://{}", path.display());
    let opts = SqliteConnectOptions::from_str(&url)?
        .create_if_missing(true)
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(opts)
        .await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

pub struct NewSession<'a> {
    pub name: &'a str,
    pub cli: &'a str,
    pub cwd: &'a str,
    pub command: &'a str,
    pub worktree_path: &'a str,
    pub orig_path: &'a str,
    pub pid: Option<i64>,
}

pub async fn insert_session(db: &Db, s: NewSession<'_>) -> Result<(), StateError> {
    let now = Utc::now().timestamp_millis();
    sqlx::query(
        "INSERT INTO sessions (name, created_at_ms, cli, cwd, command, worktree_path, orig_path, pid, status) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'running')",
    )
    .bind(s.name)
    .bind(now)
    .bind(s.cli)
    .bind(s.cwd)
    .bind(s.command)
    .bind(s.worktree_path)
    .bind(s.orig_path)
    .bind(s.pid)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn list_sessions(db: &Db, prefix: &str) -> Result<Vec<SessionRow>, StateError> {
    let like = format!("{prefix}%");
    let rows = sqlx::query(
        "SELECT name, created_at_ms, cli, cwd, command, worktree_path, orig_path, pid, status \
         FROM sessions \
         WHERE status = 'running' AND name LIKE ? \
         ORDER BY created_at_ms DESC",
    )
    .bind(like)
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(SessionRow::from_row).collect())
}

/// Every row regardless of status — used on host boot to rehydrate
/// sessions whose worktree still exists on disk.
pub async fn list_all_sessions(db: &Db) -> Result<Vec<SessionRow>, StateError> {
    let rows = sqlx::query(
        "SELECT name, created_at_ms, cli, cwd, command, worktree_path, orig_path, pid, status \
         FROM sessions ORDER BY created_at_ms DESC",
    )
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(SessionRow::from_row).collect())
}

pub async fn get_session(db: &Db, name: &str) -> Result<Option<SessionRow>, StateError> {
    let row = sqlx::query(
        "SELECT name, created_at_ms, cli, cwd, command, worktree_path, orig_path, pid, status \
         FROM sessions WHERE name = ?",
    )
    .bind(name)
    .fetch_optional(db)
    .await?;
    Ok(row.map(SessionRow::from_row))
}

pub async fn mark_dead(db: &Db, name: &str) -> Result<(), StateError> {
    sqlx::query("UPDATE sessions SET status = 'dead' WHERE name = ?")
        .bind(name)
        .execute(db)
        .await?;
    Ok(())
}

/// Sweep on startup: anything still marked `running` in the DB
/// belongs to a previous boot of `agent-start-host` whose PTYs are
/// gone. Mark them all dead so they stop showing up in
/// `GET /api/sessions` and clients don't try to reconnect to a 404.
pub async fn mark_all_running_dead(db: &Db) -> Result<(), StateError> {
    sqlx::query("UPDATE sessions SET status = 'dead' WHERE status = 'running'")
        .execute(db)
        .await?;
    Ok(())
}

pub async fn delete_session(db: &Db, name: &str) -> Result<(), StateError> {
    sqlx::query("DELETE FROM sessions WHERE name = ?")
        .bind(name)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn append_history(db: &Db, name: &str, seq: i64, chunk: &[u8]) -> Result<(), StateError> {
    sqlx::query("INSERT OR REPLACE INTO pty_history (session_name, seq, chunk) VALUES (?, ?, ?)")
        .bind(name)
        .bind(seq)
        .bind(chunk)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn load_history(db: &Db, name: &str) -> Result<Vec<u8>, StateError> {
    let rows = sqlx::query("SELECT chunk FROM pty_history WHERE session_name = ? ORDER BY seq ASC")
        .bind(name)
        .fetch_all(db)
        .await?;
    let mut out = Vec::new();
    for row in rows {
        let chunk: Vec<u8> = row.get("chunk");
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

pub async fn trim_history(db: &Db, name: &str, keep_last: i64) -> Result<(), StateError> {
    sqlx::query(
        "DELETE FROM pty_history \
         WHERE session_name = ? AND seq <= ( \
           SELECT COALESCE(MAX(seq), 0) - ? FROM pty_history WHERE session_name = ? \
         )",
    )
    .bind(name)
    .bind(keep_last)
    .bind(name)
    .execute(db)
    .await?;
    Ok(())
}
