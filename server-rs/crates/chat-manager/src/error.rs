use std::fmt;

/// Errors surfaced by the chat manager. Kept small and `String`-backed
/// because callers (the host's WS handler) turn them straight into log
/// lines or `chat_error` envelopes.
#[derive(Debug)]
pub enum ChatError {
    /// The CLI subprocess could not be spawned.
    Spawn(String),
    /// Writing to the child's stdin failed — usually because the process
    /// already exited. The host treats this as "needs revive".
    Closed(String),
    /// A model / session id failed validation before being placed on the
    /// command line.
    Invalid(String),
}

impl fmt::Display for ChatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChatError::Spawn(m) => write!(f, "spawn: {m}"),
            ChatError::Closed(m) => write!(f, "closed: {m}"),
            ChatError::Invalid(m) => write!(f, "invalid: {m}"),
        }
    }
}

impl std::error::Error for ChatError {}
