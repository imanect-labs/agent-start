-- Multi-node scheduling state.
--
-- Nodes survive restarts of the control plane: a node that reconnects
-- with its stored identity keeps its labels, its cap and its history
-- instead of appearing as a second row for the same machine.

CREATE TABLE IF NOT EXISTS nodes (
    id                  TEXT PRIMARY KEY,
    name                TEXT NOT NULL,
    -- SHA-256 of the node's long-lived token. Never the token itself.
    token_hash          TEXT NOT NULL DEFAULT '',
    status              TEXT NOT NULL DEFAULT 'notready',
    version             TEXT NOT NULL DEFAULT '',
    os                  TEXT NOT NULL DEFAULT '',
    arch                TEXT NOT NULL DEFAULT '',
    -- Comma-separated isolation profiles the node's executor provides.
    executors           TEXT NOT NULL DEFAULT 'process',
    capacity_cpu_millis INTEGER NOT NULL DEFAULT 0,
    capacity_mem_mb     INTEGER NOT NULL DEFAULT 0,
    max_sessions        INTEGER NOT NULL DEFAULT 0,
    -- JSON object of scheduling labels.
    labels              TEXT NOT NULL DEFAULT '{}',
    cordoned            INTEGER NOT NULL DEFAULT 0,
    -- The in-process node of `--role all`. Only it shares a filesystem
    -- with the control plane, so projects without an origin remote can
    -- run nowhere else.
    is_local            INTEGER NOT NULL DEFAULT 0,
    last_heartbeat_ms   INTEGER NOT NULL DEFAULT 0,
    created_at_ms       INTEGER NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_nodes_name ON nodes(name);

-- Rolling utilization samples, one per heartbeat. Trimmed to the last
-- few minutes per node; this is a sparkline, not a metrics store.
CREATE TABLE IF NOT EXISTS node_metrics (
    node_id   TEXT NOT NULL,
    at_ms     INTEGER NOT NULL,
    cpu_util  REAL NOT NULL,
    mem_util  REAL NOT NULL,
    load1     REAL NOT NULL,
    running   INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (node_id, at_ms),
    FOREIGN KEY (node_id) REFERENCES nodes(id) ON DELETE CASCADE
);

-- Which node already holds a mirror of which project. Feeds the
-- scheduler's cache-affinity score so repeat work on a project lands
-- where the clone already is.
CREATE TABLE IF NOT EXISTS node_repo_cache (
    node_id      TEXT NOT NULL,
    project_id   TEXT NOT NULL,
    last_seen_ms INTEGER NOT NULL,
    PRIMARY KEY (node_id, project_id),
    FOREIGN KEY (node_id) REFERENCES nodes(id) ON DELETE CASCADE
);

-- One-shot (or limited-use) credentials for joining the cluster.
CREATE TABLE IF NOT EXISTS join_tokens (
    id            TEXT PRIMARY KEY,
    token_hash    TEXT NOT NULL UNIQUE,
    expires_at_ms INTEGER NOT NULL,
    uses_left     INTEGER NOT NULL,
    created_at_ms INTEGER NOT NULL
);

-- Empty means "the local node", which keeps every pre-cluster row valid.
ALTER TABLE sessions ADD COLUMN node_id TEXT NOT NULL DEFAULT '';
