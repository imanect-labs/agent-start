//! The task queue's contract: one claim per task, leases that expire
//! back into the queue, and side effects that stop a retry.
//!
//! Each test gets its own database. The queue is global by design —
//! `claim_next_task` takes the head of the whole table — so tests
//! sharing one file would steal each other's work and fail for reasons
//! unrelated to what they are checking.

use state::{NewTask, TaskStatus};

/// A throwaway database, kept alive by its `TempDir` for the test's
/// duration.
async fn db() -> (state::Db, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    let db = state::open_at(&dir.path().join("host.db"))
        .await
        .expect("open db");
    (db, dir)
}

fn task(id: &str) -> NewTask {
    NewTask {
        id: id.to_string(),
        project_path: format!("/projects/{id}"),
        project_id: format!("proj-{id}"),
        title: "テスト".into(),
        prompt: "do the thing".into(),
        agent: "claude".into(),
        base_branch: String::new(),
        priority: 0,
        max_attempts: 2,
        requests_cpu_millis: 1000,
        requests_mem_mb: 2048,
        isolation: "process".into(),
        label_selector: String::new(),
        create_pr: true,
        draft_pr: true,
    }
}

#[tokio::test]
async fn a_task_is_claimed_exactly_once() {
    let (db, _home) = db().await;
    state::insert_task(&db, &task("claim-once")).await.unwrap();

    let first = state::claim_next_task(&db, 30_000).await.unwrap();
    let claimed = first.expect("the queue had a task");
    assert_eq!(claimed.id, "claim-once");
    assert_eq!(claimed.status, TaskStatus::Assigned.as_str());
    assert_eq!(claimed.attempts, 1, "the attempt was counted");

    // Nothing else is pending, so a second scheduler gets nothing —
    // it must not hand the same task out twice.
    let second = state::claim_next_task(&db, 30_000).await.unwrap();
    assert!(second.is_none(), "the same task was claimed twice");
}

#[tokio::test]
async fn priority_then_arrival_decides_the_head_of_the_queue() {
    let (db, _home) = db().await;
    let mut low = task("order-low");
    low.priority = 0;
    let mut high = task("order-high");
    high.priority = 5;
    // Insert the low-priority one first: arrival order must not win.
    state::insert_task(&db, &low).await.unwrap();
    state::insert_task(&db, &high).await.unwrap();

    let first = state::claim_next_task(&db, 30_000).await.unwrap().unwrap();
    assert_eq!(first.id, "order-high");
    let second = state::claim_next_task(&db, 30_000).await.unwrap().unwrap();
    assert_eq!(second.id, "order-low");
}

#[tokio::test]
async fn an_expired_lease_returns_the_task_to_the_queue() {
    let (db, _home) = db().await;
    state::insert_task(&db, &task("lease")).await.unwrap();
    // A lease already in the past: the node never acknowledged.
    state::claim_next_task(&db, -1).await.unwrap().unwrap();

    let expired = state::expired_leases(&db).await.unwrap();
    assert!(
        expired.iter().any(|t| t.id == "lease"),
        "the stale claim was not noticed"
    );

    assert!(state::requeue_task(&db, "lease", "lease expired")
        .await
        .unwrap());
    let back = state::get_task(&db, "lease").await.unwrap().unwrap();
    assert_eq!(back.status, TaskStatus::Pending.as_str());
    assert!(back.session_name.is_empty(), "stale session still attached");
}

#[tokio::test]
async fn a_task_out_of_attempts_fails_instead_of_looping() {
    let (db, _home) = db().await;
    let mut t = task("attempts");
    t.max_attempts = 1;
    state::insert_task(&db, &t).await.unwrap();

    state::claim_next_task(&db, 30_000).await.unwrap().unwrap();
    let requeued = state::requeue_task(&db, "attempts", "node lost")
        .await
        .unwrap();
    assert!(!requeued, "retried past its own limit");

    let row = state::get_task(&db, "attempts").await.unwrap().unwrap();
    assert_eq!(row.status, TaskStatus::Failed.as_str());
    assert_eq!(row.error, "node lost");
}

#[tokio::test]
async fn a_task_that_already_pushed_is_never_retried_automatically() {
    let (db, _home) = db().await;
    state::insert_task(&db, &task("side-effects"))
        .await
        .unwrap();
    state::claim_next_task(&db, 30_000).await.unwrap().unwrap();
    state::mark_task_running(&db, "side-effects", "node-1", "cc-x")
        .await
        .unwrap();
    state::mark_side_effects(&db, "side-effects").await.unwrap();

    // Plenty of attempts left, but a second run would open a second PR.
    let requeued = state::requeue_task(&db, "side-effects", "node lost")
        .await
        .unwrap();
    assert!(!requeued, "requeued a task that had already pushed");
    let row = state::get_task(&db, "side-effects").await.unwrap().unwrap();
    assert_eq!(row.status, TaskStatus::Failed.as_str());
}

#[tokio::test]
async fn cancelling_a_finished_task_does_not_erase_its_result() {
    let (db, _home) = db().await;
    state::insert_task(&db, &task("done")).await.unwrap();
    state::claim_next_task(&db, 30_000).await.unwrap().unwrap();
    state::mark_task_running(&db, "done", "node-1", "cc-done")
        .await
        .unwrap();
    let result = state::TaskResult {
        pr_url: "https://example.com/pr/1".into(),
        branch: "cc-done".into(),
        notes: vec![],
    };
    state::finish_task(&db, "done", TaskStatus::Succeeded, &result, "")
        .await
        .unwrap();

    assert!(
        !state::cancel_task(&db, "done").await.unwrap(),
        "cancelled a task that had already finished"
    );
    let row = state::get_task(&db, "done").await.unwrap().unwrap();
    assert_eq!(row.status, TaskStatus::Succeeded.as_str());
    assert_eq!(row.result_pr_url, "https://example.com/pr/1");
}

#[tokio::test]
async fn a_running_session_can_be_traced_back_to_its_task() {
    let (db, _home) = db().await;
    state::insert_task(&db, &task("trace")).await.unwrap();
    state::claim_next_task(&db, 30_000).await.unwrap().unwrap();
    assert!(
        state::mark_task_running(&db, "trace", "node-7", "cc-trace-1")
            .await
            .unwrap()
    );

    let found = state::task_for_session(&db, "cc-trace-1")
        .await
        .unwrap()
        .expect("session had no task");
    assert_eq!(found.id, "trace");
    assert_eq!(found.node_id, "node-7");
    assert!(
        found.lease_expires_at_ms.is_none(),
        "lease outlived placement"
    );
}

#[tokio::test]
async fn a_cancelled_task_cannot_be_marked_running_by_work_already_in_flight() {
    let (db, _home) = db().await;
    state::insert_task(&db, &task("raced")).await.unwrap();
    state::claim_next_task(&db, 30_000).await.unwrap().unwrap();
    assert!(state::cancel_task(&db, "raced").await.unwrap());

    // The session that was already starting reports in afterwards.
    let promoted = state::mark_task_running(&db, "raced", "node-1", "cc-raced")
        .await
        .unwrap();
    assert!(!promoted, "a cancelled task was resurrected");
    let row = state::get_task(&db, "raced").await.unwrap().unwrap();
    assert_eq!(row.status, TaskStatus::Cancelled.as_str());
}
