//! Task queue endpoints — submit / list / inspect / cancel / retry.

use super::err;
use agent_start_api::{CreateTaskRequest, TaskBody, TasksBody};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;

use crate::app::Shared;

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub status: Option<String>,
    pub project: Option<String>,
    pub limit: Option<i64>,
}

pub async fn create_task(
    State(app): State<Shared>,
    Json(body): Json<CreateTaskRequest>,
) -> Response {
    match crate::tasks::submit(&app, body).await {
        // 202, not 200: the work has been accepted, not done. A phone
        // that gets a 200 back in 40ms would reasonably expect a PR.
        Ok(row) => (
            StatusCode::ACCEPTED,
            Json(TaskBody {
                task: crate::tasks::to_api(&app, &row),
            }),
        )
            .into_response(),
        Err(e) => err(e.status(), e.to_string()),
    }
}

pub async fn list_tasks(State(app): State<Shared>, Query(q): Query<ListQuery>) -> Response {
    if let Some(status) = q.status.as_deref() {
        if state::TaskStatus::parse(status).is_none() {
            return err(StatusCode::BAD_REQUEST, format!("unknown status: {status}"));
        }
    }
    match state::list_tasks(
        &app.db,
        q.status.as_deref(),
        q.project.as_deref(),
        q.limit.unwrap_or(100),
    )
    .await
    {
        Ok(rows) => Json(TasksBody {
            tasks: rows.iter().map(|r| crate::tasks::to_api(&app, r)).collect(),
        })
        .into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

pub async fn get_task(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    match state::get_task(&app.db, &id).await {
        Ok(Some(row)) => Json(TaskBody {
            task: crate::tasks::to_api(&app, &row),
        })
        .into_response(),
        Ok(None) => err(StatusCode::NOT_FOUND, "task not found"),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

pub async fn cancel_task(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    match crate::tasks::cancel(&app, &id).await {
        Ok(true) => reload(&app, &id).await,
        // Already finished, or never existed. Distinguishing the two
        // matters: "you cannot cancel this" is a different fix from
        // "you have the wrong id".
        Ok(false) => match state::get_task(&app.db, &id).await {
            Ok(Some(_)) => err(StatusCode::CONFLICT, "this task has already finished"),
            Ok(None) => err(StatusCode::NOT_FOUND, "task not found"),
            Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        },
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

pub async fn retry_task(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    match state::retry_task(&app.db, &id).await {
        Ok(true) => reload(&app, &id).await,
        Ok(false) => match state::get_task(&app.db, &id).await {
            Ok(Some(_)) => err(
                StatusCode::CONFLICT,
                "this task has not finished yet; cancel it first",
            ),
            Ok(None) => err(StatusCode::NOT_FOUND, "task not found"),
            Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        },
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

/// Re-read a task after mutating it so the client sees the row that now
/// exists rather than the one it asked us to change.
async fn reload(app: &Shared, id: &str) -> Response {
    match state::get_task(&app.db, id).await {
        Ok(Some(row)) => Json(TaskBody {
            task: crate::tasks::to_api(app, &row),
        })
        .into_response(),
        Ok(None) => err(StatusCode::NOT_FOUND, "task not found"),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}
