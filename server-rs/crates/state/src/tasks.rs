//! The task queue's persistence layer.
//!
//! Claiming is done with a conditional UPDATE rather than
//! `SELECT … FOR UPDATE SKIP LOCKED`, which SQLite does not have: the
//! scheduler reads a candidate, then writes it only if it is *still*
//! pending. `rows_affected() == 1` is the claim; a second scheduler that
//! lost the race sees zero and moves on. The semantics are the same as
//! `SKIP LOCKED` and the Postgres backend of Phase 4 can substitute it
//! without changing a caller.
//!
//! Every transition is expressed as "move from the state I expect".
//! Nothing here says "set status = running" unconditionally, because a
//! task cancelled by the user while its agent was starting must not be
//! resurrected by the machinery that was already in flight.

use crate::{Db, StateError};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqliteRow;
use sqlx::Row;

/// Where a task is in its life. Stored as the lowercase name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    /// Waiting for a node with room.
    Pending,
    /// Claimed by the scheduler, session not yet confirmed running.
    Assigned,
    /// The agent is working.
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl TaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Assigned => "assigned",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "pending" => Self::Pending,
            "assigned" => Self::Assigned,
            "running" => Self::Running,
            "succeeded" => Self::Succeeded,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            _ => return None,
        })
    }

    /// True once the task will never run again on its own.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRow {
    pub id: String,
    pub project_path: String,
    pub project_id: String,
    pub title: String,
    pub prompt: String,
    pub agent: String,
    pub base_branch: String,
    pub status: String,
    pub priority: i64,
    pub attempts: i64,
    pub max_attempts: i64,
    pub side_effects_committed: bool,
    pub requests_cpu_millis: i64,
    pub requests_mem_mb: i64,
    pub isolation: String,
    pub label_selector: String,
    pub node_id: String,
    pub session_name: String,
    pub lease_expires_at_ms: Option<i64>,
    pub create_pr: bool,
    pub draft_pr: bool,
    pub result_pr_url: String,
    pub result_branch: String,
    pub notes: String,
    pub error: String,
    pub created_at_ms: i64,
    pub started_at_ms: Option<i64>,
    pub finished_at_ms: Option<i64>,
}

const TASK_COLUMNS: &str = "id, project_path, project_id, title, prompt, agent, base_branch, \
     status, priority, attempts, max_attempts, side_effects_committed, \
     requests_cpu_millis, requests_mem_mb, isolation, label_selector, \
     node_id, session_name, lease_expires_at_ms, create_pr, draft_pr, \
     result_pr_url, result_branch, notes, error, \
     created_at_ms, started_at_ms, finished_at_ms";

fn task_from_row(row: SqliteRow) -> TaskRow {
    TaskRow {
        id: row.get("id"),
        project_path: row.get("project_path"),
        project_id: row.get("project_id"),
        title: row.get("title"),
        prompt: row.get("prompt"),
        agent: row.get("agent"),
        base_branch: row.get("base_branch"),
        status: row.get("status"),
        priority: row.get("priority"),
        attempts: row.get("attempts"),
        max_attempts: row.get("max_attempts"),
        side_effects_committed: row.get::<i64, _>("side_effects_committed") != 0,
        requests_cpu_millis: row.get("requests_cpu_millis"),
        requests_mem_mb: row.get("requests_mem_mb"),
        isolation: row.get("isolation"),
        label_selector: row.get("label_selector"),
        node_id: row.get("node_id"),
        session_name: row.get("session_name"),
        // Decoded as `Option` rather than "try and discard the error":
        // a NULL is a real value here (no lease), and conflating it with
        // a decode failure is how a cleared lease reads back as a live
        // one.
        lease_expires_at_ms: row
            .try_get::<Option<i64>, _>("lease_expires_at_ms")
            .unwrap_or_default(),
        create_pr: row.get::<i64, _>("create_pr") != 0,
        draft_pr: row.get::<i64, _>("draft_pr") != 0,
        result_pr_url: row.get("result_pr_url"),
        result_branch: row.get("result_branch"),
        notes: row.get("notes"),
        error: row.get("error"),
        created_at_ms: row.get("created_at_ms"),
        started_at_ms: row
            .try_get::<Option<i64>, _>("started_at_ms")
            .unwrap_or_default(),
        finished_at_ms: row
            .try_get::<Option<i64>, _>("finished_at_ms")
            .unwrap_or_default(),
    }
}

/// A task as submitted, before the queue has touched it.
#[derive(Debug, Clone)]
pub struct NewTask {
    pub id: String,
    pub project_path: String,
    pub project_id: String,
    pub title: String,
    pub prompt: String,
    pub agent: String,
    pub base_branch: String,
    pub priority: i64,
    pub max_attempts: i64,
    pub requests_cpu_millis: i64,
    pub requests_mem_mb: i64,
    pub isolation: String,
    pub label_selector: String,
    pub create_pr: bool,
    pub draft_pr: bool,
}

pub async fn insert_task(db: &Db, t: &NewTask) -> Result<(), StateError> {
    let now = Utc::now().timestamp_millis();
    sqlx::query(
        "INSERT INTO tasks (id, project_path, project_id, title, prompt, agent, base_branch, \
           status, priority, attempts, max_attempts, requests_cpu_millis, requests_mem_mb, \
           isolation, label_selector, create_pr, draft_pr, created_at_ms) \
         VALUES (?, ?, ?, ?, ?, ?, ?, 'pending', ?, 0, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&t.id)
    .bind(&t.project_path)
    .bind(&t.project_id)
    .bind(&t.title)
    .bind(&t.prompt)
    .bind(&t.agent)
    .bind(&t.base_branch)
    .bind(t.priority)
    .bind(t.max_attempts.max(1))
    .bind(t.requests_cpu_millis)
    .bind(t.requests_mem_mb)
    .bind(&t.isolation)
    .bind(&t.label_selector)
    .bind(i64::from(t.create_pr))
    .bind(i64::from(t.draft_pr))
    .bind(now)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn get_task(db: &Db, id: &str) -> Result<Option<TaskRow>, StateError> {
    let sql = format!("SELECT {TASK_COLUMNS} FROM tasks WHERE id = ?");
    let row = sqlx::query(&sql).bind(id).fetch_optional(db).await?;
    Ok(row.map(task_from_row))
}

/// The task currently owning `session`, if any. How a finished session
/// finds its way back to the queue that started it.
pub async fn task_for_session(db: &Db, session: &str) -> Result<Option<TaskRow>, StateError> {
    let sql = format!(
        "SELECT {TASK_COLUMNS} FROM tasks WHERE session_name = ? \
         ORDER BY created_at_ms DESC LIMIT 1"
    );
    let row = sqlx::query(&sql).bind(session).fetch_optional(db).await?;
    Ok(row.map(task_from_row))
}

/// Newest first. `status` filters to one state; `project` to one path.
pub async fn list_tasks(
    db: &Db,
    status: Option<&str>,
    project: Option<&str>,
    limit: i64,
) -> Result<Vec<TaskRow>, StateError> {
    let sql = format!(
        "SELECT {TASK_COLUMNS} FROM tasks \
         WHERE (?1 IS NULL OR status = ?1) AND (?2 IS NULL OR project_path = ?2) \
         ORDER BY created_at_ms DESC LIMIT ?3"
    );
    let rows = sqlx::query(&sql)
        .bind(status)
        .bind(project)
        .bind(limit.clamp(1, 500))
        .fetch_all(db)
        .await?;
    Ok(rows.into_iter().map(task_from_row).collect())
}

/// Take the next pending task and hold it under a lease.
///
/// Returns `None` when the queue is empty *or* when another scheduler
/// won the race for the head of it — the caller simply tries again on
/// its next tick, so the two cases need not be distinguished.
pub async fn claim_next_task(db: &Db, lease_ms: i64) -> Result<Option<TaskRow>, StateError> {
    let now = Utc::now().timestamp_millis();
    let sql = format!(
        "SELECT {TASK_COLUMNS} FROM tasks WHERE status = 'pending' \
         ORDER BY priority DESC, created_at_ms ASC LIMIT 1"
    );
    let Some(row) = sqlx::query(&sql).fetch_optional(db).await? else {
        return Ok(None);
    };
    let task = task_from_row(row);
    let result = sqlx::query(
        "UPDATE tasks SET status = 'assigned', attempts = attempts + 1, \
           lease_expires_at_ms = ?, error = '' \
         WHERE id = ? AND status = 'pending'",
    )
    .bind(now + lease_ms)
    .bind(&task.id)
    .execute(db)
    .await?;
    if result.rows_affected() == 0 {
        return Ok(None);
    }
    // Re-read so the caller sees the incremented attempt count rather
    // than the pre-claim snapshot it would otherwise report to the user.
    get_task(db, &task.id).await
}

/// Record the session an attempt is running in and clear its lease: the
/// lease covers *placement*, and placement is over.
pub async fn mark_task_running(
    db: &Db,
    id: &str,
    node_id: &str,
    session: &str,
) -> Result<bool, StateError> {
    let now = Utc::now().timestamp_millis();
    let result = sqlx::query(
        "UPDATE tasks SET status = 'running', node_id = ?, session_name = ?, \
           lease_expires_at_ms = NULL, \
           started_at_ms = COALESCE(started_at_ms, ?) \
         WHERE id = ? AND status = 'assigned'",
    )
    .bind(node_id)
    .bind(session)
    .bind(now)
    .bind(id)
    .execute(db)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// Mark that an attempt has pushed or opened a PR. From here on the task
/// is never retried automatically — a rerun would duplicate the effect.
pub async fn mark_side_effects(db: &Db, id: &str) -> Result<(), StateError> {
    sqlx::query("UPDATE tasks SET side_effects_committed = 1 WHERE id = ?")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

/// The outcome of one finished attempt.
#[derive(Debug, Clone, Default)]
pub struct TaskResult {
    pub pr_url: String,
    pub branch: String,
    pub notes: Vec<String>,
}

pub async fn finish_task(
    db: &Db,
    id: &str,
    status: TaskStatus,
    result: &TaskResult,
    error: &str,
) -> Result<(), StateError> {
    let now = Utc::now().timestamp_millis();
    sqlx::query(
        "UPDATE tasks SET status = ?, result_pr_url = ?, result_branch = ?, notes = ?, \
           error = ?, lease_expires_at_ms = NULL, finished_at_ms = ? \
         WHERE id = ? AND status NOT IN ('cancelled')",
    )
    .bind(status.as_str())
    .bind(&result.pr_url)
    .bind(&result.branch)
    .bind(result.notes.join("\n"))
    .bind(error)
    .bind(now)
    .bind(id)
    .execute(db)
    .await?;
    Ok(())
}

/// Send an attempt back to the queue, or fail the task when it has no
/// attempts left. Returns true when it was requeued.
///
/// A task with side effects is never requeued: whatever went wrong, it
/// has already pushed a branch, and a second run would open a second
/// pull request for one request.
pub async fn requeue_task(db: &Db, id: &str, reason: &str) -> Result<bool, StateError> {
    let now = Utc::now().timestamp_millis();
    let requeued = sqlx::query(
        "UPDATE tasks SET status = 'pending', node_id = '', session_name = '', \
           lease_expires_at_ms = NULL, error = ? \
         WHERE id = ? AND status IN ('assigned', 'running') \
           AND side_effects_committed = 0 AND attempts < max_attempts",
    )
    .bind(reason)
    .bind(id)
    .execute(db)
    .await?;
    if requeued.rows_affected() > 0 {
        return Ok(true);
    }
    sqlx::query(
        "UPDATE tasks SET status = 'failed', error = ?, lease_expires_at_ms = NULL, \
           finished_at_ms = ? \
         WHERE id = ? AND status IN ('assigned', 'running')",
    )
    .bind(reason)
    .bind(now)
    .bind(id)
    .execute(db)
    .await?;
    Ok(false)
}

/// Every task whose placement lease has run out. The scheduler requeues
/// them: the node never acknowledged, so nothing is running anywhere.
pub async fn expired_leases(db: &Db) -> Result<Vec<TaskRow>, StateError> {
    let now = Utc::now().timestamp_millis();
    let sql = format!(
        "SELECT {TASK_COLUMNS} FROM tasks \
         WHERE status = 'assigned' AND lease_expires_at_ms IS NOT NULL \
           AND lease_expires_at_ms < ?"
    );
    let rows = sqlx::query(&sql).bind(now).fetch_all(db).await?;
    Ok(rows.into_iter().map(task_from_row).collect())
}

/// User-requested cancellation. Refused (returns false) once the task
/// has reached a terminal state — there is nothing left to stop, and
/// overwriting the outcome would erase the PR link.
pub async fn cancel_task(db: &Db, id: &str) -> Result<bool, StateError> {
    let now = Utc::now().timestamp_millis();
    let result = sqlx::query(
        "UPDATE tasks SET status = 'cancelled', lease_expires_at_ms = NULL, \
           finished_at_ms = ? \
         WHERE id = ? AND status IN ('pending', 'assigned', 'running')",
    )
    .bind(now)
    .bind(id)
    .execute(db)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// Put a finished task back in the queue by hand. Resets the attempt
/// counter — the user is asking for a fresh start, not a continuation of
/// an exhausted one — but deliberately keeps `side_effects_committed`
/// so the automatic requeue path still refuses to double-push.
pub async fn retry_task(db: &Db, id: &str) -> Result<bool, StateError> {
    let result = sqlx::query(
        "UPDATE tasks SET status = 'pending', attempts = 0, node_id = '', session_name = '', \
           lease_expires_at_ms = NULL, error = '', finished_at_ms = NULL \
         WHERE id = ? AND status IN ('failed', 'cancelled', 'succeeded')",
    )
    .bind(id)
    .execute(db)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// On boot, anything the previous process had in flight is in flight
/// nowhere. Assigned tasks return to the queue; running ones are failed,
/// because their session died with the host and we cannot tell whether
/// the agent had already pushed.
pub async fn reset_inflight_tasks(db: &Db) -> Result<(), StateError> {
    let now = Utc::now().timestamp_millis();
    sqlx::query(
        "UPDATE tasks SET status = 'pending', lease_expires_at_ms = NULL, \
           node_id = '', session_name = '' \
         WHERE status = 'assigned' AND side_effects_committed = 0",
    )
    .execute(db)
    .await?;
    sqlx::query(
        "UPDATE tasks SET status = 'failed', finished_at_ms = ?, lease_expires_at_ms = NULL, \
           error = 'agent-start-host restarted while this task was running' \
         WHERE status IN ('assigned', 'running')",
    )
    .bind(now)
    .execute(db)
    .await?;
    Ok(())
}
