-- Human-readable session title (#34 follow-up). Derived from the initial
-- task prompt (e.g. a GitHub issue) at creation, or from the first chat
-- message for chat-mode sessions, so the sidebar can show what the agent is
-- working on instead of the timestamped session name.
-- Empty string = not yet known.
ALTER TABLE sessions ADD COLUMN title TEXT NOT NULL DEFAULT '';
