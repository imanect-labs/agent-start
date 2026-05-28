//! In-process manager for headless `claude` chat conversations (#34).
//!
//! Sibling to `pty-manager`: where that drives a terminal, this drives
//! `claude -p` in stream-json mode, broadcasting parsed event envelopes to
//! subscribed WebSockets. See `session::ChatSession` for the protocol and
//! `docs/chat-ui-plan.md` for the design decisions behind it.

mod error;
mod manager;
mod session;

pub use error::ChatError;
pub use manager::{ChatExitHook, ChatManager};
pub use session::{ChatImage, ChatSession, ChatSpawnSpec, CommitEvent};
