-- One snapshot row per (session, window). Holds the most recent PTY
-- ring buffer so that on host restart a stopped session can replay
-- its scrollback to the client.
CREATE TABLE IF NOT EXISTS pty_snapshot (
    session_name TEXT NOT NULL,
    window       INTEGER NOT NULL,
    saved_at_ms  INTEGER NOT NULL,
    chunk        BLOB NOT NULL,
    PRIMARY KEY (session_name, window)
);
