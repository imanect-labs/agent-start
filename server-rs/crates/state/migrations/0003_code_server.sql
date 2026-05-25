CREATE TABLE IF NOT EXISTS code_server_instances (
    session_name   TEXT PRIMARY KEY,
    port           INTEGER NOT NULL,
    pid            INTEGER NOT NULL,
    started_at_ms  INTEGER NOT NULL,
    FOREIGN KEY (session_name) REFERENCES sessions(name) ON DELETE CASCADE
);
