CREATE TABLE IF NOT EXISTS sessions (
    name           TEXT PRIMARY KEY,
    created_at_ms  INTEGER NOT NULL,
    cli            TEXT NOT NULL,
    cwd            TEXT NOT NULL,
    command        TEXT NOT NULL,
    worktree_path  TEXT NOT NULL DEFAULT '',
    orig_path      TEXT NOT NULL DEFAULT '',
    pid            INTEGER,
    status         TEXT NOT NULL DEFAULT 'running'
);

CREATE TABLE IF NOT EXISTS pty_history (
    session_name TEXT NOT NULL,
    seq          INTEGER NOT NULL,
    chunk        BLOB NOT NULL,
    PRIMARY KEY (session_name, seq),
    FOREIGN KEY (session_name) REFERENCES sessions(name) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_pty_history_session ON pty_history(session_name);
