//! Which stdio protocol a chat provider speaks.
//!
//! A chat is not tied to one agent: the composer picks `provider/model`
//! and the conversation respawns underneath. What changes between
//! providers is the wire protocol on the child's stdin and stdout, so
//! that — and only that — is what a driver abstracts.
//!
//! Claude's stream-json vocabulary is the **internal** one: the browser
//! renders it directly (decision 3 in `docs/chat-ui-plan.md`), so a
//! second driver's job is to translate its own events into those shapes
//! rather than to invent a third format nothing renders.
//!
//! # Why a trait, with one implementation
//!
//! The first attempt at a second agent shipped as a `match` on an enum,
//! with the protocol smeared across `session.rs` — a command-line builder
//! here, a send arm there, a stdout arm somewhere else. It was written
//! against `codex proto`, a subcommand that no longer exists, and the
//! shape of the enum hid how much of Claude's design had been assumed
//! along the way. Two of those assumptions are worth naming, because
//! they are what this trait exists to remove:
//!
//! - **A turn is not always a line you write.** `handle_can_use_tool`
//!   answered the agent by reaching for the process's stdin from inside
//!   event handling, which is how a reader for a process that had already
//!   been replaced could write to its successor (#128, #129). Here a line
//!   read produces [`DriverOutput`], and what to write back is *data* the
//!   session sends from one place, after checking that the process it
//!   came from is still the one running.
//! - **Changing model is not always a respawn.** Claude carries `--model`
//!   and `--resume` on its command line, so `switch_model` kills and
//!   restarts. Other agents set it inside the session; [`ModelSwitch`]
//!   says which, instead of the caller assuming.
//!
//! # Mapping a request/response protocol onto this
//!
//! The next driver is expected to be `codex app-server`: JSON-RPC over
//! the same stdio, with `session/prompt` and `thread/resume` as methods
//! and `item/*/requestApproval` arriving as **requests from the agent**
//! that we must answer. See `docs/references.ja.md` for the method list
//! observed in paseo, which drives codex that way.
//!
//! That protocol fits the seams here — a request from the agent is a line
//! in, its answer a line out, and `permission_reply` already names that
//! round trip — but it needs one thing this file deliberately does not
//! provide: correlation of a request id we send with the response that
//! comes back later. That belongs in the driver that needs it, built
//! against a binary someone can actually run. Writing it here, with no
//! implementation to check it against, is how the last one went wrong.

use crate::error::ChatError;
use crate::session::{ChatImage, ChatSpawnSpec};
use std::sync::Arc;

pub mod claude;

/// What one line of the agent's stdout amounts to.
#[derive(Debug, Default)]
pub struct DriverOutput {
    /// Envelopes in Claude's stream-json vocabulary, for the browser and
    /// for persistence. Empty means the line was housekeeping.
    pub events: Vec<serde_json::Value>,
    /// Lines to write back to the *same* process. The session sends
    /// these; a driver never holds the child's stdin.
    pub writes: Vec<String>,
}

impl DriverOutput {
    pub fn event(value: serde_json::Value) -> Self {
        Self {
            events: vec![value],
            ..Self::default()
        }
    }

    pub fn write(line: String) -> Self {
        Self {
            writes: vec![line],
            ..Self::default()
        }
    }
}

/// A user's turn: what to send, and anything the user should be told
/// about what could not be sent.
#[derive(Debug, Default)]
pub struct UserTurn {
    pub writes: Vec<String>,
    /// Shown in the transcript, never appended to the prompt — text added
    /// to a prompt reaches the model and changes its answer.
    pub notices: Vec<String>,
}

/// How an agent changes model.
#[derive(Debug)]
pub enum ModelSwitch {
    /// Kill and respawn; the model lives on the command line.
    Respawn,
    /// Send these lines to the running process.
    #[allow(dead_code)] // The first driver that needs it is codex.
    InSession(Vec<String>),
}

/// One agent's stdio protocol.
///
/// Object-safe on purpose: a session holds `Arc<dyn AgentDriver>` and a
/// reader keeps its own clone, so a line is always interpreted in the
/// vocabulary of the process that produced it rather than whichever
/// driver the session has since been pointed at.
///
/// Every method takes `&self`. A driver that must count something (a
/// submission id, an outstanding request) owns that state internally.
pub trait AgentDriver: Send + Sync + std::fmt::Debug {
    /// The configured name, as it appears in `chat.providers[].driver`.
    fn name(&self) -> &'static str;

    /// The shell command line that starts the agent.
    fn command_line(&self, spec: &ChatSpawnSpec) -> Result<String, ChatError>;

    /// Lines to write immediately after spawn. `generation` distinguishes
    /// one process's handshake from the next one's in the agent's logs.
    fn handshake(&self, generation: u64) -> Vec<String>;

    /// Turn a user's message into what goes on the wire.
    fn user_turn(&self, text: &str, images: &[ChatImage]) -> UserTurn;

    /// Best-effort interrupt of the in-flight turn, where the protocol
    /// has one.
    fn interrupt(&self) -> Option<String>;

    /// Interpret one line of stdout.
    fn on_line(&self, line: &str) -> DriverOutput;

    /// The user's answer to a permission card this driver raised.
    fn permission_reply(&self, request_id: &str, response: serde_json::Value) -> String;

    /// Whether this driver can continue an earlier conversation by id.
    fn supports_resume(&self) -> bool;

    /// Whether a permission mode (Claude's plan mode) means anything
    /// here. Accepting one for a driver that drops it would report a mode
    /// on every `chat_status` that the agent had never heard of.
    fn supports_permission_mode(&self) -> bool;

    /// How this agent changes model.
    fn model_switch(&self, model: &str) -> ModelSwitch;
}

/// Resolve a configured driver name.
///
/// An unknown name is an error rather than a fallback: silently driving
/// `gemini` with Claude's protocol produces a process that starts, says
/// nothing, and looks like a hang.
pub fn resolve(name: &str) -> Result<Arc<dyn AgentDriver>, ChatError> {
    match name.trim() {
        "claude-stream-json" | "" => Ok(Arc::new(claude::ClaudeStreamJson)),
        other => Err(ChatError::Invalid(format!(
            "unknown chat driver `{other}` (expected `claude-stream-json`)"
        ))),
    }
}
