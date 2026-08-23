-- The task queue: "do this to that repository" submitted from a phone,
-- run on whichever node has room, and delivered back as a pull request.
--
-- Tasks outlive the sessions that execute them. A task that is retried
-- keeps its identity and its history while its `session_name` moves to
-- the new attempt, which is what makes "it ran twice on two machines"
-- something the UI can show rather than something it has to infer.

CREATE TABLE IF NOT EXISTS tasks (
    id                     TEXT PRIMARY KEY,
    -- Project as the submitter named it, plus the derived id the
    -- scheduler matches mirror caches on.
    project_path           TEXT NOT NULL,
    project_id             TEXT NOT NULL DEFAULT '',
    -- Short human label, derived from the prompt at submission.
    title                  TEXT NOT NULL DEFAULT '',
    prompt                 TEXT NOT NULL,
    -- Key into `config.clis`.
    agent                  TEXT NOT NULL,
    -- Branch the worktree is cut from and the PR targets. Empty means
    -- the repository default.
    base_branch            TEXT NOT NULL DEFAULT '',

    -- pending | assigned | running | succeeded | failed | cancelled
    status                 TEXT NOT NULL DEFAULT 'pending',
    priority               INTEGER NOT NULL DEFAULT 0,
    attempts               INTEGER NOT NULL DEFAULT 0,
    max_attempts           INTEGER NOT NULL DEFAULT 3,
    -- Set once the run has pushed or opened a PR. A task with side
    -- effects is never retried automatically: a second attempt would
    -- open a second pull request for the same request.
    side_effects_committed INTEGER NOT NULL DEFAULT 0,

    requests_cpu_millis    INTEGER NOT NULL DEFAULT 0,
    requests_mem_mb        INTEGER NOT NULL DEFAULT 0,
    isolation              TEXT NOT NULL DEFAULT 'process',
    -- Comma-separated `key=value` node selectors.
    label_selector         TEXT NOT NULL DEFAULT '',

    -- Current (or last) attempt.
    node_id                TEXT NOT NULL DEFAULT '',
    session_name           TEXT NOT NULL DEFAULT '',
    -- While a task is claimed but not yet running, this is when the
    -- claim goes stale and the task returns to the queue.
    lease_expires_at_ms    INTEGER,

    create_pr              INTEGER NOT NULL DEFAULT 1,
    draft_pr               INTEGER NOT NULL DEFAULT 1,
    result_pr_url          TEXT NOT NULL DEFAULT '',
    result_branch          TEXT NOT NULL DEFAULT '',
    -- Non-fatal explanations from the finalize step, newline separated
    -- ("the agent changed nothing", "gh is not installed").
    notes                  TEXT NOT NULL DEFAULT '',
    error                  TEXT NOT NULL DEFAULT '',

    created_at_ms          INTEGER NOT NULL,
    started_at_ms          INTEGER,
    finished_at_ms         INTEGER
);

-- The queue read: pending work in priority then arrival order.
CREATE INDEX IF NOT EXISTS idx_tasks_queue
    ON tasks(status, priority DESC, created_at_ms ASC);

-- Reverse lookup from a finished session back to the task that owns it.
CREATE INDEX IF NOT EXISTS idx_tasks_session ON tasks(session_name);
