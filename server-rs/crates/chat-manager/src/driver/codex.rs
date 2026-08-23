//! Translating `codex proto` into the envelope shapes the UI renders.
//!
//! **Unverified against a real binary.** This is written against the
//! published `codex proto` submission/event vocabulary; the `codex` CLI
//! is not installed in the environment this was developed in, so nothing
//! here has been exercised end to end. The provider ships marked
//! `experimental` for that reason, and if the protocol has moved this
//! file is the only place that needs to change — everything upstream of
//! it speaks Claude's vocabulary.
//!
//! Codex speaks a request/response protocol: each stdin line is a
//! *submission* with an id, each stdout line an *event* tagged with the
//! submission it belongs to. The renderer, meanwhile, expects Claude's
//! stream-json: partial text as `stream_event` deltas, a committed
//! `assistant` message per turn, `user` messages carrying `tool_result`
//! blocks, and a `result` marking the turn's end. So:
//!
//! | codex event              | rendered as                                |
//! | ------------------------ | ------------------------------------------ |
//! | `session_configured`     | `system:init` (session id + model)          |
//! | `agent_message_delta`    | `stream_event` / `text_delta`               |
//! | `agent_reasoning_delta`  | `stream_event` / `thinking_delta`           |
//! | `agent_message`          | committed `assistant` text block            |
//! | `agent_reasoning`        | committed `assistant` thinking block        |
//! | `exec_command_begin`     | committed `assistant` `tool_use` block      |
//! | `exec_command_end`       | committed `user` `tool_result` block        |
//! | `task_complete`          | `result`                                    |
//! | `error` / `stream_error` | `chat_error`                                |

use serde_json::{json, Value};

/// Turn one codex event into zero or more Claude-shaped envelopes.
///
/// Stateless: codex tags a command's output with the same `call_id` it
/// announced, and the UI already pairs `tool_use` with `tool_result` by
/// that id, so there is nothing to remember between lines.
pub fn translate(event: &Value) -> Vec<Value> {
    let msg = event.get("msg").unwrap_or(event);
    let ty = msg.get("type").and_then(Value::as_str).unwrap_or("");
    match ty {
        "session_configured" => vec![json!({
            "type": "system",
            "subtype": "init",
            "session_id": msg.get("session_id").and_then(Value::as_str).unwrap_or(""),
            "model": msg.get("model").and_then(Value::as_str).unwrap_or(""),
        })],

        "agent_message_delta" => vec![delta_event(
            "text_delta",
            "text",
            msg.get("delta").and_then(Value::as_str).unwrap_or(""),
        )],
        "agent_reasoning_delta" => vec![delta_event(
            "thinking_delta",
            "thinking",
            msg.get("delta").and_then(Value::as_str).unwrap_or(""),
        )],

        "agent_message" => {
            let text = msg.get("message").and_then(Value::as_str).unwrap_or("");
            vec![assistant(vec![json!({"type": "text", "text": text})])]
        }
        "agent_reasoning" => {
            let text = msg.get("text").and_then(Value::as_str).unwrap_or("");
            vec![assistant(vec![
                json!({"type": "thinking", "thinking": text}),
            ])]
        }

        "exec_command_begin" => {
            let call_id = call_id(msg);
            let command = command_line(msg);
            vec![assistant(vec![json!({
                "type": "tool_use",
                "id": call_id,
                "name": "Bash",
                "input": {"command": command},
            })])]
        }
        "exec_command_end" => {
            let call_id = call_id(msg);
            let exit = msg.get("exit_code").and_then(Value::as_i64);
            let output = msg
                .get("aggregated_output")
                .or_else(|| msg.get("stdout"))
                .and_then(Value::as_str)
                .unwrap_or("");
            vec![json!({
                "type": "user",
                "message": {"role": "user", "content": [{
                    "type": "tool_result",
                    "tool_use_id": call_id,
                    "content": output,
                    "is_error": exit.map(|c| c != 0).unwrap_or(false),
                }]},
            })]
        }

        // The turn is over. `result` is also what clears the in-flight
        // replay buffer upstream, so it must not be swallowed.
        "task_complete" => vec![json!({"type": "result", "subtype": "success"})],

        "error" | "stream_error" => {
            let text = msg
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("codex reported an error");
            vec![json!({"type": "chat_error", "message": text})]
        }

        // Anything else is codex housekeeping (token counts, task
        // lifecycle chatter). Dropping it keeps the transcript readable;
        // it is logged upstream at debug level.
        _ => Vec::new(),
    }
}

fn delta_event(delta_type: &str, field: &str, text: &str) -> Value {
    json!({
        "type": "stream_event",
        "event": {
            "type": "content_block_delta",
            "delta": {"type": delta_type, field: text},
        },
    })
}

fn assistant(content: Vec<Value>) -> Value {
    json!({"type": "assistant", "message": {"role": "assistant", "content": content}})
}

fn call_id(msg: &Value) -> String {
    msg.get("call_id")
        .or_else(|| msg.get("id"))
        .and_then(Value::as_str)
        .unwrap_or("codex-call")
        .to_string()
}

/// Codex reports a command as an argv array; the UI shows one line.
fn command_line(msg: &Value) -> String {
    match msg.get("command") {
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" "),
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    }
}

/// One submission line for a user turn.
///
/// Images are not sent: codex's proto item vocabulary for inline images
/// is not settled here. The user is told so through a `chat_notice` (see
/// `dropped_images_notice`) rather than by appending a note to the
/// prompt — text added here would reach the model and change its answer.
pub fn user_submission(id: &str, text: &str) -> String {
    json!({
        "id": id,
        "op": {"type": "user_input", "items": [{"type": "text", "text": text}]},
    })
    .to_string()
}

/// Message for the attachments this driver cannot carry, or `None` when
/// the turn had none.
pub fn dropped_images_notice(image_count: usize) -> Option<String> {
    (image_count > 0).then(|| {
        format!("画像 {image_count} 件は Codex プロバイダでは送信されません（テキストのみ送信しました）。")
    })
}

pub fn interrupt_submission(id: &str) -> String {
    json!({"id": id, "op": {"type": "interrupt"}}).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_streamed_message_becomes_a_text_delta() {
        let out =
            translate(&json!({"id": "1", "msg": {"type": "agent_message_delta", "delta": "hel"}}));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["type"], "stream_event");
        assert_eq!(out[0]["event"]["delta"]["type"], "text_delta");
        assert_eq!(out[0]["event"]["delta"]["text"], "hel");
    }

    #[test]
    fn a_command_and_its_output_are_paired_by_call_id() {
        let begin = translate(&json!({"msg": {
            "type": "exec_command_begin",
            "call_id": "c1",
            "command": ["cargo", "test"],
        }}));
        assert_eq!(begin[0]["message"]["content"][0]["type"], "tool_use");
        assert_eq!(begin[0]["message"]["content"][0]["id"], "c1");
        assert_eq!(
            begin[0]["message"]["content"][0]["input"]["command"],
            "cargo test"
        );

        let end = translate(&json!({"msg": {
            "type": "exec_command_end",
            "call_id": "c1",
            "exit_code": 1,
            "aggregated_output": "boom",
        }}));
        let block = &end[0]["message"]["content"][0];
        assert_eq!(block["tool_use_id"], "c1");
        assert_eq!(block["content"], "boom");
        assert_eq!(block["is_error"], true, "a non-zero exit is an error");
    }

    #[test]
    fn the_end_of_a_turn_is_reported_as_a_result() {
        let out = translate(&json!({"msg": {"type": "task_complete"}}));
        assert_eq!(out[0]["type"], "result");
    }

    #[test]
    fn housekeeping_events_are_dropped_rather_than_rendered() {
        assert!(translate(&json!({"msg": {"type": "token_count"}})).is_empty());
    }

    #[test]
    fn a_dropped_image_is_reported_without_touching_the_prompt() {
        let line = user_submission("3", "look at this");
        let v: Value = serde_json::from_str(&line).unwrap();
        // The model sees exactly what the user typed.
        assert_eq!(v["op"]["items"][0]["text"], "look at this");
        let notice = dropped_images_notice(2).expect("the user is told");
        assert!(notice.contains("画像 2 件"), "unhelpful notice: {notice}");
        assert!(dropped_images_notice(0).is_none());
    }
}
