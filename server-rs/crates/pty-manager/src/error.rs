use thiserror::Error;

#[derive(Debug, Error)]
pub enum PtyError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("pty: {0}")]
    Pty(String),
    #[error("session not found: {0}")]
    NotFound(String),
}
