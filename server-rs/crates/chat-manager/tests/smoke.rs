//! End-to-end smoke test against the real `claude` CLI.
//!
//! Ignored by default (needs `claude` on PATH + network). Run with:
//!   cargo test -p chat-manager --test smoke -- --ignored --nocapture

use chat_manager::{ChatManager, ChatSpawnSpec};
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
#[ignore]
async fn chat_roundtrip() {
    let mgr = Arc::new(ChatManager::new());
    let spec = ChatSpawnSpec {
        name: "cc-smoke".into(),
        cwd: std::env::temp_dir(),
        shell: "/bin/bash".into(),
        command: "claude".into(),
        skip_permissions_flag: Some("--dangerously-skip-permissions".into()),
        extra_args: String::new(),
        env: vec![],
        model: Some("haiku".into()),
        resume: None,
        start_seq: 0,
    };
    let session = mgr.spawn(spec).await.expect("spawn");
    let (_inflight, mut rx) = session.subscribe();

    session
        .send_user_message("reply with exactly the word PONG", &[])
        .await
        .expect("send");

    let mut saw_user_input = false;
    let mut saw_assistant = false;
    let mut saw_result = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(90);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(30), rx.recv()).await {
            Ok(Ok(line)) => {
                let v: serde_json::Value = serde_json::from_str(&line).unwrap();
                match v.get("type").and_then(|t| t.as_str()) {
                    Some("user_input") => {
                        assert!(v.get("_seq").is_some(), "user_input must carry _seq");
                        saw_user_input = true;
                    }
                    Some("assistant") => {
                        assert!(v.get("_seq").is_some(), "assistant must carry _seq");
                        saw_assistant = true;
                    }
                    Some("result") => {
                        saw_result = true;
                        break;
                    }
                    _ => {}
                }
            }
            _ => break,
        }
    }
    assert!(saw_user_input, "expected synthesized user_input envelope");
    assert!(saw_assistant, "expected at least one assistant envelope");
    assert!(saw_result, "expected a result envelope");
    assert!(
        !session.claude_session_id().is_empty(),
        "claude session id should be captured from system:init"
    );
    mgr.remove("cc-smoke");
}
