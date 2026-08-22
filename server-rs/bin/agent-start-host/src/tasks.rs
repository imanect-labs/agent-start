//! The task queue's moving parts: claim, run, finish.
//!
//! A task is "do this to that repository" submitted from a phone and
//! answered, minutes later, with a pull request link. Between those two
//! moments it is an ordinary session — the same worktree, the same
//! scheduler, the same terminal you can attach to and watch — which is
//! what makes a queued run debuggable instead of a black box.
//!
//! The loop is deliberately dull:
//!
//! 1. leases that ran out go back on the queue (the node never answered);
//! 2. one pending task is claimed and launched as a headless session;
//! 3. when that session exits, its outcome decides what happens —
//!    a clean exit commits, pushes and opens a PR on the node that ran
//!    it; anything else fails or retries.
//!
//! Everything the loop needs to survive a restart is in SQLite, so a
//! host that dies mid-task leaves rows that say so rather than work that
//! silently evaporated.

use agent_start_api::{CreateTaskRequest, TaskSummary};
use cluster_control::SessionOutcome;
use cluster_proto::FinalizeSpec;
use state::{TaskResult, TaskRow, TaskStatus};
use std::time::Duration;

use crate::app::Shared;
use crate::launch::{LaunchError, LaunchRequest, MAX_PROMPT_CHARS};

/// How long a claimed task may sit before the claim is assumed dead.
/// Comfortably longer than a cold mirror clone, which is the slowest
/// honest part of starting a session.
const LEASE_MS: i64 = 10 * 60 * 1000;

/// How often the queue is looked at. A task queue is not a hot path;
/// this is about promptness, not throughput.
const TICK: Duration = Duration::from_secs(3);

/// Validate a submission and put it on the queue. Returns the new row.
pub async fn submit(app: &Shared, req: CreateTaskRequest) -> Result<TaskRow, LaunchError> {
    let prompt = req.prompt.trim();
    if prompt.is_empty() {
        return Err(LaunchError::BadRequest("prompt is required".into()));
    }
    let prompt: String = prompt.chars().take(MAX_PROMPT_CHARS).collect();

    let cfg = config_loader::load_config().map_err(|e| LaunchError::Internal(e.to_string()))?;
    let project = std::path::PathBuf::from(&req.project_path);
    if req.project_path.is_empty() || !config_loader::is_path_under_roots(&cfg, &project) {
        return Err(LaunchError::BadRequest(
            "projectPath is missing or outside configured roots".into(),
        ));
    }
    // A task always works on a branch of its own, which means a git
    // repository. Saying so now beats failing at assign time, minutes
    // later, on a node.
    if !git_ops::is_git_repo(&project) {
        return Err(LaunchError::BadRequest(
            "tasks need a git repository: this project is not one".into(),
        ));
    }

    let agent = req
        .agent
        .clone()
        .filter(|a| !a.is_empty())
        .unwrap_or_else(|| cfg.default_cli.clone());
    let cli = cfg
        .clis
        .get(&agent)
        .ok_or_else(|| LaunchError::BadRequest(format!("unknown agent: {agent}")))?;
    if cli.is_chat() {
        return Err(LaunchError::BadRequest(
            "chat agents cannot run a queued task; pick a terminal agent".into(),
        ));
    }
    if cli.command.trim().is_empty() {
        return Err(LaunchError::BadRequest(
            "the bare shell cannot run a queued task; pick an agent".into(),
        ));
    }
    // Node selectors are validated here rather than at claim time: a
    // typo should be a 400 on submission, not a task that fails to
    // schedule ten minutes later for reasons the user cannot see.
    let selector = req.node_selector.clone().unwrap_or_default();
    for label in &selector {
        crate::cluster::parse_label(label).map_err(LaunchError::BadRequest)?;
    }

    let row = state::NewTask {
        id: uuid::Uuid::new_v4().to_string(),
        project_path: req.project_path.clone(),
        project_id: workspace_manager::project_id(&project),
        title: crate::sessions::summarize_title(&prompt),
        prompt,
        agent,
        base_branch: req.base_branch.clone().unwrap_or_default(),
        priority: req.priority.unwrap_or(0),
        max_attempts: req.max_attempts.unwrap_or(3).clamp(1, 10),
        requests_cpu_millis: i64::from(req.cpu_millis.unwrap_or(0)),
        requests_mem_mb: i64::from(req.mem_mb.unwrap_or(0)),
        isolation: req.isolation.clone().unwrap_or_else(|| "process".into()),
        label_selector: selector.join(","),
        create_pr: req.create_pr.unwrap_or(true),
        draft_pr: req.draft_pr.unwrap_or(true),
    };
    state::insert_task(&app.db, &row)
        .await
        .map_err(|e| LaunchError::Internal(e.to_string()))?;
    tracing::info!(task = %row.id, agent = %row.agent, "task queued");

    state::get_task(&app.db, &row.id)
        .await
        .map_err(|e| LaunchError::Internal(e.to_string()))?
        .ok_or_else(|| LaunchError::Internal("the task vanished immediately after insert".into()))
}

/// Start the background loop that drains the queue.
pub fn spawn_runner(app: Shared) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(TICK);
        loop {
            tick.tick().await;
            if let Err(e) = drain(&app).await {
                tracing::warn!(error = %e, "task queue tick failed");
            }
        }
    });
}

async fn drain(app: &Shared) -> Result<(), state::StateError> {
    // A lease that ran out means the node never acknowledged: nothing is
    // running anywhere, so the task can go back without risking a second
    // agent on the same work.
    for stale in state::expired_leases(&app.db).await? {
        let requeued =
            state::requeue_task(&app.db, &stale.id, "no node accepted this task in time").await?;
        tracing::warn!(task = %stale.id, requeued, "task lease expired");
    }

    // One at a time. The scheduler is what decides whether there is room
    // for the session; claiming a batch here would only queue work up
    // behind a placement that may not be possible yet.
    let Some(task) = state::claim_next_task(&app.db, LEASE_MS).await? else {
        return Ok(());
    };
    start_claimed(app, task).await;
    Ok(())
}

/// Launch a claimed task as a headless session.
async fn start_claimed(app: &Shared, task: TaskRow) {
    let request = LaunchRequest {
        base: agent_start_api::StartSessionRequest {
            project_path: task.project_path.clone(),
            cli: Some(task.agent.clone()),
            // A queued agent has nobody to answer a permission prompt.
            skip_permissions: Some(true),
            extra_args: Some(String::new()),
            // Always its own worktree: a task must never mutate the
            // user's checkout, and the branch is what gets pushed.
            create_worktree: Some(true),
            prompt: Some(task.prompt.clone()),
            cpu_millis: u32::try_from(task.requests_cpu_millis)
                .ok()
                .filter(|v| *v > 0),
            mem_mb: u32::try_from(task.requests_mem_mb).ok().filter(|v| *v > 0),
            isolation: Some(task.isolation.clone()),
            node_selector: Some(
                task.label_selector
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect(),
            ),
            node_id: None,
        },
        headless: true,
    };

    match crate::launch::launch(app, request).await {
        Ok(launched) => {
            let promoted =
                state::mark_task_running(&app.db, &task.id, &launched.node_id, &launched.name)
                    .await
                    .unwrap_or(false);
            if !promoted {
                // The user cancelled between the claim and the launch.
                // Tear the session down rather than leave an orphan
                // agent working on something nobody wants.
                tracing::info!(task = %task.id, session = %launched.name, "task was cancelled while starting; stopping its session");
                stop_session(app, &launched.name).await;
                return;
            }
            tracing::info!(
                task = %task.id,
                session = %launched.name,
                node = %launched.node_name,
                "task running"
            );
        }
        Err(e) => {
            // A bad request will fail identically on every retry, so it
            // is a failure now rather than three times over.
            let permanent = matches!(e, LaunchError::BadRequest(_));
            if permanent {
                let _ = state::finish_task(
                    &app.db,
                    &task.id,
                    TaskStatus::Failed,
                    &TaskResult::default(),
                    &e.to_string(),
                )
                .await;
                tracing::warn!(task = %task.id, error = %e, "task rejected");
            } else {
                let requeued = state::requeue_task(&app.db, &task.id, &e.to_string())
                    .await
                    .unwrap_or(false);
                tracing::warn!(task = %task.id, error = %e, requeued, "task could not start");
            }
        }
    }
}

/// React to a session ending. Called from the control plane's exit hook
/// for every session; returns immediately for those no task owns.
pub async fn on_session_ended(app: &Shared, session: &str, outcome: SessionOutcome) {
    let task = match state::task_for_session(&app.db, session).await {
        Ok(Some(t)) => t,
        Ok(None) => return,
        Err(e) => {
            tracing::warn!(error = %e, session = %session, "task lookup failed");
            return;
        }
    };
    if TaskStatus::parse(&task.status).is_some_and(TaskStatus::is_terminal) {
        return;
    }

    match &outcome {
        SessionOutcome::Exited { code: Some(0) } => finish_successfully(app, &task).await,
        SessionOutcome::Exited { code } => {
            let detail = match code {
                Some(c) => format!("エージェントは終了コード {c} で終了しました"),
                // Not the same as zero: we never learned the status, so
                // treating it as success would open PRs for crashes.
                None => "エージェントの終了コードを取得できませんでした".to_string(),
            };
            fail_or_retry(app, &task, &detail).await;
        }
        SessionOutcome::Failed { error } => fail_or_retry(app, &task, error).await,
        SessionOutcome::Lost => {
            fail_or_retry(app, &task, "実行中のノードと接続できなくなりました").await
        }
    }
}

async fn fail_or_retry(app: &Shared, task: &TaskRow, reason: &str) {
    match state::requeue_task(&app.db, &task.id, reason).await {
        Ok(true) => tracing::info!(task = %task.id, reason, "task requeued"),
        Ok(false) => tracing::warn!(task = %task.id, reason, "task failed"),
        Err(e) => tracing::warn!(task = %task.id, error = %e, "failed to record task outcome"),
    }
}

/// The agent exited cleanly: commit what it produced, push the branch,
/// and open the pull request — all on the node that holds the worktree.
async fn finish_successfully(app: &Shared, task: &TaskRow) {
    let Some(control) = app.cluster.as_ref() else {
        let _ = state::finish_task(
            &app.db,
            &task.id,
            TaskStatus::Failed,
            &TaskResult::default(),
            "this host runs no scheduler, so the task's branch cannot be finalized",
        )
        .await;
        return;
    };

    let title = if task.title.is_empty() {
        format!("task {}", &task.id[..8.min(task.id.len())])
    } else {
        task.title.clone()
    };
    let spec = FinalizeSpec {
        commit_message: format!("{title}\n\n{}", task.prompt),
        push: true,
        open_pr: task.create_pr,
        pr_title: title,
        pr_body: pr_body(task),
        draft: task.draft_pr,
        base_branch: task.base_branch.clone(),
    };

    // From here on the run may leave a branch behind, so it must never
    // be retried automatically — a second attempt would push again and
    // open a second pull request for one request. Recorded *before* the
    // call, because a finalize that pushes and then loses the connection
    // has still pushed.
    if let Err(e) = state::mark_side_effects(&app.db, &task.id).await {
        tracing::warn!(task = %task.id, error = %e, "failed to record task side effects");
    }

    match control
        .finalize_session(&task.node_id, &task.session_name, spec)
        .await
    {
        Ok(ok) => {
            let result = TaskResult {
                pr_url: ok.pr_url.clone(),
                branch: ok.branch.clone(),
                notes: ok.notes.clone(),
            };
            let _ = state::finish_task(&app.db, &task.id, TaskStatus::Succeeded, &result, "").await;
            tracing::info!(
                task = %task.id,
                pr = %ok.pr_url,
                branch = %ok.branch,
                committed = ok.committed,
                pushed = ok.pushed,
                "task finished"
            );
        }
        Err(e) => {
            let _ = state::finish_task(
                &app.db,
                &task.id,
                TaskStatus::Failed,
                &TaskResult::default(),
                &format!("エージェントは完了しましたが、成果を PR にできませんでした: {e}"),
            )
            .await;
            tracing::warn!(task = %task.id, error = %e, "finalize failed");
        }
    }
}

fn pr_body(task: &TaskRow) -> String {
    format!(
        "agent-start のタスクとして `{}` が作業しました。\n\n## 依頼内容\n\n{}\n",
        task.agent, task.prompt
    )
}

/// Cancel a task and stop whatever it has running.
pub async fn cancel(app: &Shared, id: &str) -> Result<bool, state::StateError> {
    let task = state::get_task(&app.db, id).await?;
    let Some(task) = task else { return Ok(false) };
    let cancelled = state::cancel_task(&app.db, id).await?;
    if cancelled && !task.session_name.is_empty() {
        stop_session(app, &task.session_name).await;
    }
    Ok(cancelled)
}

/// Tear down a task's session wherever it is running.
async fn stop_session(app: &Shared, session: &str) {
    if let Some(control) = app.cluster.as_ref() {
        control.cancel_session(session, false).await;
    }
    for window in app.pty.remove_session(session) {
        window.kill();
    }
}

/// Render a row for the API, resolving the node id to a name so the UI
/// can say *where* a task ran without a second request.
pub fn to_api(app: &Shared, row: &TaskRow) -> TaskSummary {
    let node_name = app
        .cluster
        .as_ref()
        .filter(|c| c.local_node_id().as_deref() != Some(row.node_id.as_str()))
        .and_then(|c| c.node(&row.node_id))
        .map(|v| v.info.name)
        .unwrap_or_default();
    TaskSummary {
        id: row.id.clone(),
        title: row.title.clone(),
        prompt: row.prompt.clone(),
        project_path: row.project_path.clone(),
        agent: row.agent.clone(),
        status: row.status.clone(),
        attempts: row.attempts,
        max_attempts: row.max_attempts,
        base_branch: row.base_branch.clone(),
        node_id: row.node_id.clone(),
        node_name,
        session_name: row.session_name.clone(),
        pr_url: row.result_pr_url.clone(),
        branch: row.result_branch.clone(),
        notes: row
            .notes
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(str::to_string)
            .collect(),
        error: row.error.clone(),
        created_at: row.created_at_ms,
        started_at: row.started_at_ms,
        finished_at: row.finished_at_ms,
    }
}
