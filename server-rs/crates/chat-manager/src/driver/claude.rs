//! `claude -p --input-format stream-json --output-format stream-json`.
//!
//! The reference driver, and the one whose vocabulary everything else
//! translates into. What lives here is only what is true of *this* wire
//! protocol; how a message is rendered, persisted or replayed is the
//! session's business.

use super::{AgentDriver, DriverOutput, ModelSwitch, UserTurn};
use crate::error::ChatError;
use crate::session::{validate_token, ChatImage, ChatSpawnSpec};
use serde_json::json;

/// Tools whose answer is the user's, not ours. Everything else is
/// auto-allowed so the UX matches the old skip-permissions path while
/// questions and plans still surface (#95).
const INTERACTIVE_TOOLS: [&str; 2] = ["AskUserQuestion", "ExitPlanMode"];

#[derive(Debug, Default)]
pub struct ClaudeStreamJson;

impl AgentDriver for ClaudeStreamJson {
    fn name(&self) -> &'static str {
        "claude-stream-json"
    }

    fn command_line(&self, spec: &ChatSpawnSpec) -> Result<String, ChatError> {
        let mut parts: Vec<String> = vec![
            spec.command.clone(),
            "-p".into(),
            "--input-format".into(),
            "stream-json".into(),
            "--output-format".into(),
            "stream-json".into(),
            "--verbose".into(),
            "--include-partial-messages".into(),
            // Interactive permissions (#95): route tool approvals over
            // stdio so AskUserQuestion / ExitPlanMode reach the UI.
            // Requires the `initialize` handshake below — without it the
            // CLI never emits `can_use_tool`.
            "--permission-prompt-tool".into(),
            "stdio".into(),
        ];
        if let Some(mode) = &spec.permission_mode {
            validate_token(mode)?;
            parts.push("--permission-mode".into());
            parts.push(mode.clone());
        }
        if let Some(model) = &spec.model {
            validate_token(model)?;
            parts.push("--model".into());
            parts.push(model.clone());
        }
        if let Some(resume) = &spec.resume {
            validate_token(resume)?;
            parts.push("--resume".into());
            parts.push(resume.clone());
        }
        // `spec.skip_permissions_flag` is deliberately not used: #95
        // replaced skipping permissions with the stdio prompt tool above.
        let extra = spec.extra_args.trim();
        if !extra.is_empty() {
            parts.push(extra.to_string());
        }
        Ok(parts.join(" "))
    }

    fn handshake(&self, generation: u64) -> Vec<String> {
        // stdin is read in order, so sending this before the first user
        // turn is enough — no need to await the reply.
        vec![json!({
            "type": "control_request",
            "request_id": format!("init-{generation}"),
            "request": {"subtype": "initialize", "hooks": {}},
        })
        .to_string()]
    }

    fn user_turn(&self, text: &str, images: &[ChatImage]) -> UserTurn {
        let mut content = Vec::new();
        for img in images {
            content.push(json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": img.media_type,
                    "data": img.data,
                }
            }));
        }
        if !text.is_empty() {
            content.push(json!({"type": "text", "text": text}));
        }
        UserTurn {
            writes: vec![json!({
                "type": "user",
                "message": {"role": "user", "content": content},
            })
            .to_string()],
            notices: vec![],
        }
    }

    fn interrupt(&self) -> Option<String> {
        Some(
            json!({
                "type": "control_request",
                "request_id": uuid::Uuid::new_v4().to_string(),
                "request": {"subtype": "interrupt"},
            })
            .to_string(),
        )
    }

    fn on_line(&self, line: &str) -> DriverOutput {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            tracing::debug!(target: "chat", "non-JSON stdout: {line}");
            return DriverOutput::default();
        };
        match value.get("type").and_then(|v| v.as_str()).unwrap_or("") {
            // Housekeeping — dropped (decision 3).
            "rate_limit_event" => DriverOutput::default(),
            // Replies to our own control requests (initialize / allow).
            // Internal bookkeeping — never shown to the browser.
            "control_response" => DriverOutput::default(),
            "control_request" => {
                let subtype = value
                    .get("request")
                    .and_then(|r| r.get("subtype"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if subtype == "can_use_tool" {
                    can_use_tool(&value)
                } else {
                    DriverOutput::default()
                }
            }
            _ => DriverOutput::event(value),
        }
    }

    fn permission_reply(&self, request_id: &str, response: serde_json::Value) -> String {
        control_response_line(request_id, response)
    }

    fn supports_resume(&self) -> bool {
        true
    }

    fn supports_permission_mode(&self) -> bool {
        true
    }

    fn model_switch(&self, _model: &str) -> ModelSwitch {
        // `--model` is a command-line flag, so the conversation is
        // continued with `--resume` on a fresh process.
        ModelSwitch::Respawn
    }
}

/// Classify a `can_use_tool` request: the two interactive tools become a
/// permission card for the user, everything else is allowed on the spot.
///
/// The allow is *returned*, not written. The session writes it, and only
/// after confirming the process that asked is still the one running — an
/// answer sent to a successor answers a question it never asked.
fn can_use_tool(value: &serde_json::Value) -> DriverOutput {
    let request_id = value
        .get("request_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if request_id.is_empty() {
        return DriverOutput::default();
    }
    let req = value.get("request");
    let tool_name = req
        .and_then(|r| r.get("tool_name"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let input = req
        .and_then(|r| r.get("input"))
        .cloned()
        .unwrap_or_else(|| json!({}));

    if INTERACTIVE_TOOLS.contains(&tool_name) {
        DriverOutput::event(json!({
            "type": "chat_permission",
            "request_id": request_id,
            "tool": tool_name,
            "input": input,
        }))
    } else {
        DriverOutput::write(control_response_line(
            request_id,
            json!({"behavior": "allow", "updatedInput": input}),
        ))
    }
}

/// The envelope `claude` expects for an answer to one of its control
/// requests.
fn control_response_line(request_id: &str, response: serde_json::Value) -> String {
    json!({
        "type": "control_response",
        "response": {
            "subtype": "success",
            "request_id": request_id,
            "response": response,
        },
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> ChatSpawnSpec {
        ChatSpawnSpec {
            name: "cc-x".into(),
            cwd: "/tmp".into(),
            shell: "/bin/bash".into(),
            provider: "claude".into(),
            driver: "claude-stream-json".into(),
            command: "claude".into(),
            skip_permissions_flag: Some("--dangerously-skip-permissions".into()),
            extra_args: String::new(),
            env: vec![],
            model: None,
            permission_mode: None,
            resume: None,
            start_seq: 0,
        }
    }

    #[test]
    fn cmdline_minimal() {
        let c = ClaudeStreamJson.command_line(&spec()).unwrap();
        assert!(c.starts_with("claude -p --input-format stream-json"));
        assert!(c.contains("--include-partial-messages"));
        // #95: chat is interactive, not skip-permissions.
        assert!(c.contains("--permission-prompt-tool stdio"));
        assert!(!c.contains("--dangerously-skip-permissions"));
    }

    #[test]
    fn cmdline_model_resume_and_plan_mode() {
        let mut s = spec();
        s.model = Some("opus".into());
        s.resume = Some("abc-123".into());
        s.permission_mode = Some("plan".into());
        let c = ClaudeStreamJson.command_line(&s).unwrap();
        assert!(c.contains("--model opus"));
        assert!(c.contains("--resume abc-123"));
        assert!(c.contains("--permission-mode plan"));
    }

    #[test]
    fn rejects_injection_in_model() {
        let mut s = spec();
        s.model = Some("opus; rm -rf /".into());
        assert!(ClaudeStreamJson.command_line(&s).is_err());
    }

    /// The auto-allow leaves as data. It used to be written from inside
    /// event handling, which is how a retired reader could answer on the
    /// successor's stdin.
    #[test]
    fn an_ordinary_tool_is_allowed_by_a_returned_line_not_a_write() {
        let line = json!({
            "type": "control_request",
            "request_id": "req-1",
            "request": {"subtype": "can_use_tool", "tool_name": "Bash", "input": {"command": "ls"}},
        })
        .to_string();

        let out = ClaudeStreamJson.on_line(&line);
        assert!(out.events.is_empty(), "the browser was shown a Bash call");
        assert_eq!(out.writes.len(), 1);
        let v: serde_json::Value = serde_json::from_str(&out.writes[0]).unwrap();
        assert_eq!(v["type"], "control_response");
        assert_eq!(v["response"]["request_id"], "req-1");
        assert_eq!(v["response"]["response"]["behavior"], "allow");
    }

    #[test]
    fn an_interactive_tool_becomes_a_card_and_answers_nothing() {
        let line = json!({
            "type": "control_request",
            "request_id": "req-2",
            "request": {"subtype": "can_use_tool", "tool_name": "ExitPlanMode", "input": {}},
        })
        .to_string();

        let out = ClaudeStreamJson.on_line(&line);
        assert!(out.writes.is_empty(), "answered for the user");
        assert_eq!(out.events.len(), 1);
        assert_eq!(out.events[0]["type"], "chat_permission");
        assert_eq!(out.events[0]["request_id"], "req-2");
    }

    #[test]
    fn housekeeping_is_dropped_and_everything_else_passes_through() {
        for ty in ["rate_limit_event", "control_response"] {
            let out = ClaudeStreamJson.on_line(&json!({"type": ty}).to_string());
            assert!(
                out.events.is_empty() && out.writes.is_empty(),
                "{ty} leaked"
            );
        }
        let out = ClaudeStreamJson.on_line(&json!({"type": "assistant"}).to_string());
        assert_eq!(out.events.len(), 1);
    }

    #[test]
    fn a_non_json_line_is_not_a_panic() {
        let out = ClaudeStreamJson.on_line("Welcome to claude!");
        assert!(out.events.is_empty() && out.writes.is_empty());
    }
}
