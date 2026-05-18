//! In-process PTY multiplexer that replaces tmux.
//!
//! `PtyManager` owns one `PtySession` per `(session_name, window)`.
//! Each session has at least window 0 (created by `POST /api/sessions`)
//! and may have additional windows for the per-session tab UI.
//!
//! We retain the last ~512 KiB of output per window in an in-memory
//! ring buffer and broadcast new bytes to any subscribed `Receiver`.
//! On reconnect a client receives the entire ring before live tailing.

mod error;
mod manager;
mod ring;
mod session;

pub use error::PtyError;
pub use manager::{ExitHook, PtyManager, PtySpawnSpec, RING_BUFFER_BYTES};
pub use session::PtySession;
