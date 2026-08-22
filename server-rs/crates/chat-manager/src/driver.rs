//! Which stdio protocol a chat provider speaks.
//!
//! A chat is not tied to one agent: the composer picks `provider/model`
//! and the conversation respawns underneath. What changes between
//! providers is the wire protocol on the child's stdin and stdout, so
//! that — and only that — is what a `Driver` abstracts.
//!
//! Claude's stream-json vocabulary is the **internal** one: the browser
//! renders it directly (decision 3 in `docs/chat-ui-plan.md`), so a
//! second driver's job is to translate its own events into those shapes
//! rather than to invent a third format nothing renders.

use crate::error::ChatError;

pub mod codex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Driver {
    /// `claude -p --input-format stream-json --output-format stream-json`.
    ClaudeStreamJson,
    /// `codex proto` — newline-delimited submissions in, events out.
    CodexProto,
}

impl Driver {
    /// Resolve a configured driver name.
    ///
    /// An unknown name is an error rather than a fallback: silently
    /// driving `gemini` with Claude's protocol produces a process that
    /// starts, says nothing, and looks like a hang.
    pub fn parse(name: &str) -> Result<Self, ChatError> {
        match name.trim() {
            "claude-stream-json" | "" => Ok(Self::ClaudeStreamJson),
            "codex-proto" => Ok(Self::CodexProto),
            other => Err(ChatError::Invalid(format!(
                "unknown chat driver `{other}` (expected `claude-stream-json` or `codex-proto`)"
            ))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeStreamJson => "claude-stream-json",
            Self::CodexProto => "codex-proto",
        }
    }

    /// Whether this driver can continue an earlier conversation by id.
    /// Claude has `--resume`; the codex proto session is per-process.
    pub fn supports_resume(self) -> bool {
        matches!(self, Self::ClaudeStreamJson)
    }
}
