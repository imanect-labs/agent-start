//! In-process manager for headless `claude` chat conversations (#34).
//!
//! Sibling to `pty-manager`: where that drives a terminal, this drives
//! `claude -p` in stream-json mode, broadcasting parsed event envelopes to
//! subscribed WebSockets. See `session::ChatSession` for the protocol and
//! `docs/chat-ui-plan.md` for the design decisions behind it.

mod driver;
mod error;
mod manager;
mod session;

// Every type on `AgentDriver`'s signature, so the trait can be
// implemented from outside this crate. `driver` itself stays private:
// the trait and its vocabulary are the seam, the module layout is not.
pub use driver::{AgentDriver, DriverOutput, ModelSwitch, UserTurn};
pub use error::ChatError;
pub use manager::{ChatExitHook, ChatManager};
pub use session::{ChatImage, ChatSession, ChatSpawnSpec, CommitEvent};
