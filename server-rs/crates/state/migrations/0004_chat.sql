-- Chat-mode (#34) persistence. A chat session is an ordinary `sessions`
-- row whose backing CLI runs `claude` headless in stream-json mode rather
-- than a PTY. We persist the Claude `session_id` so a host restart / crash
-- can `--resume` the same conversation, plus the logical message log so the
-- transcript can be replayed read-only before the process is revived.

-- Claude's own resumable conversation id (from `system:init.session_id`).
-- Empty string = not yet known / not a chat session.
ALTER TABLE sessions ADD COLUMN claude_session_id TEXT NOT NULL DEFAULT '';

-- One row per logical message (user / assistant / result). `content_json`
-- holds the normalized block array we render in the UI; token-level
-- `stream_event` deltas are NOT stored (they are reconstructed live).
CREATE TABLE IF NOT EXISTS chat_messages (
    session_name  TEXT NOT NULL,
    seq           INTEGER NOT NULL,
    role          TEXT NOT NULL,
    content_json  TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    PRIMARY KEY (session_name, seq),
    FOREIGN KEY (session_name) REFERENCES sessions(name) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_chat_messages_session ON chat_messages(session_name);
