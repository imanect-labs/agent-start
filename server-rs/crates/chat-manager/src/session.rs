//! One headless `claude` conversation driven over stream-json.
//!
//! Unlike a PTY session this has no terminal: we pipe newline-delimited
//! JSON to the child's stdin and parse newline-delimited JSON from its
//! stdout. Each parsed event is wrapped in an *envelope* and broadcast to
//! every subscribed WebSocket. The browser renders the envelopes directly
//! (decision 3: server passes events through, it does not normalize them).
//!
//! The process stays alive across turns (verified: one `claude -p` reads
//! many stdin messages, emitting one assistant turn each). We only respawn
//! to switch model or to revive a crashed/restarted conversation, both via
//! `--resume <session_id>` which continues the same conversation without
//! re-emitting history.

use crate::error::ChatError;
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::process::Stdio;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

/// Capacity (in envelopes) of the live broadcast channel.
const BROADCAST_CAP: usize = 512;
/// Cap on the in-flight replay buffer (envelopes since the last turn end).
/// A single turn with heavy token streaming can emit thousands of
/// `stream_event`s; we keep a bounded tail so a mid-generation reconnect
/// still shows recent activity without unbounded memory growth.
const INFLIGHT_CAP: usize = 4096;

/// Inputs needed to (re)spawn the underlying `claude` process.
#[derive(Clone)]
pub struct ChatSpawnSpec {
    pub name: String,
    pub cwd: std::path::PathBuf,
    pub shell: String,
    /// The `claude` program/command (from `CliConfig.command`).
    pub command: String,
    /// Skip-permissions flag — chat mode requires it (decision 7).
    pub skip_permissions_flag: Option<String>,
    /// Sanitized extra args appended verbatim.
    pub extra_args: String,
    pub env: Vec<(String, String)>,
    /// Initial model (`--model`), or None for the CLI default.
    pub model: Option<String>,
    /// Resume an existing Claude conversation (`--resume <id>`).
    pub resume: Option<String>,
    /// First `_seq` to assign to committed messages — seeded from SQLite
    /// so seqs stay monotonic across restarts.
    pub start_seq: i64,
}

/// The mutable per-process handles, replaced wholesale on respawn.
struct Proc {
    child: Child,
    reader: JoinHandle<()>,
}

pub struct ChatSession {
    name: String,
    spec: Mutex<ChatSpawnSpec>,
    proc: Mutex<Option<Proc>>,
    /// The child's stdin, kept in an async mutex so writes can `.await`
    /// without blocking the synchronous `proc` lock. `None` when no process
    /// is running.
    stdin: tokio::sync::Mutex<Option<ChildStdin>>,
    tx: broadcast::Sender<String>,
    inflight: Arc<Mutex<VecDeque<String>>>,
    seq: Arc<AtomicI64>,
    /// Current model, synced from `system:init` and from `switch_model`.
    model: Arc<Mutex<Option<String>>>,
    /// Claude's resumable conversation id, captured from `system:init`.
    claude_session_id: Arc<Mutex<String>>,
    /// Bumped on every `start()`. A reader task only reports its EOF as a
    /// crash if it is still the current generation (otherwise a respawn
    /// already replaced it).
    generation: AtomicU64,
    /// Whether a `system:init` arrived since the last `start()`. If a
    /// `--resume` process dies before its init, the resume id is stale —
    /// the next revive falls back to a fresh conversation (U5).
    saw_init: std::sync::atomic::AtomicBool,
    /// Set when a resumed process died before producing `system:init`.
    resume_suspect: std::sync::atomic::AtomicBool,
    /// Lossless persistence sink. Every committed envelope is forwarded
    /// here (in addition to the lossy live broadcast) so the host can write
    /// the transcript to SQLite without dropping messages under backpressure.
    commit_tx: Mutex<Option<tokio::sync::mpsc::UnboundedSender<CommitEvent>>>,
    manager: Weak<crate::manager::ChatManager>,
}

/// A committed (persistable) chat message, delivered to the host's
/// persistence task. `json` is the full envelope (already carrying `_seq`).
#[derive(Clone)]
pub struct CommitEvent {
    pub seq: i64,
    pub role: String,
    pub json: String,
}

impl ChatSession {
    pub(crate) fn create(
        spec: ChatSpawnSpec,
        manager: Weak<crate::manager::ChatManager>,
    ) -> Arc<Self> {
        let (tx, _rx) = broadcast::channel(BROADCAST_CAP);
        Arc::new(Self {
            name: spec.name.clone(),
            seq: Arc::new(AtomicI64::new(spec.start_seq)),
            model: Arc::new(Mutex::new(spec.model.clone())),
            claude_session_id: Arc::new(Mutex::new(spec.resume.clone().unwrap_or_default())),
            generation: AtomicU64::new(0),
            saw_init: std::sync::atomic::AtomicBool::new(false),
            resume_suspect: std::sync::atomic::AtomicBool::new(false),
            spec: Mutex::new(spec),
            proc: Mutex::new(None),
            stdin: tokio::sync::Mutex::new(None),
            tx,
            inflight: Arc::new(Mutex::new(VecDeque::new())),
            commit_tx: Mutex::new(None),
            manager,
        })
    }

    /// Install the host's persistence sink. Committed envelopes emitted
    /// after this call are forwarded to `tx` losslessly.
    pub fn set_commit_sink(&self, tx: tokio::sync::mpsc::UnboundedSender<CommitEvent>) {
        *self.commit_tx.lock() = Some(tx);
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn current_model(&self) -> Option<String> {
        self.model.lock().clone()
    }

    pub fn claude_session_id(&self) -> String {
        self.claude_session_id.lock().clone()
    }

    /// Snapshot the in-flight buffer plus a live receiver, taken together so
    /// a reconnecting client cannot miss an event between the two.
    pub fn subscribe(&self) -> (Vec<String>, broadcast::Receiver<String>) {
        let inflight = self.inflight.lock();
        let rx = self.tx.subscribe();
        (inflight.iter().cloned().collect(), rx)
    }

    pub fn is_alive(&self) -> bool {
        self.proc.lock().is_some()
    }

    /// Inject a host-synthesized envelope (user input, status, error) into
    /// the same ordered stream the browser and persistence task consume.
    /// `committed` envelopes get a monotonic `_seq` and are persisted.
    pub fn inject(&self, mut value: serde_json::Value, committed: bool) {
        let mut commit: Option<CommitEvent> = None;
        if committed {
            let s = self.seq.fetch_add(1, Ordering::SeqCst);
            let role = value
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if let Some(obj) = value.as_object_mut() {
                obj.insert("_seq".into(), serde_json::json!(s));
            }
            commit = Some(CommitEvent {
                seq: s,
                role,
                json: value.to_string(),
            });
        }
        let line = value.to_string();
        if let Some(ev) = commit {
            if let Some(tx) = self.commit_tx.lock().as_ref() {
                let _ = tx.send(ev);
            }
        }
        self.push(line);
    }

    fn push(&self, line: String) {
        {
            let mut buf = self.inflight.lock();
            buf.push_back(line.clone());
            while buf.len() > INFLIGHT_CAP {
                buf.pop_front();
            }
        }
        let _ = self.tx.send(line);
    }

    /// Send a user turn: emit the synthesized `user_input` envelope (so the
    /// browser and persistence both see it in order) then write the
    /// stream-json line to the child's stdin.
    pub async fn send_user_message(
        &self,
        text: &str,
        images: &[ChatImage],
    ) -> Result<(), ChatError> {
        // Envelope the UI/persistence render (carries our own content shape).
        let mut content = Vec::new();
        if !text.is_empty() {
            content.push(serde_json::json!({"type": "text", "text": text}));
        }
        for img in images {
            content.push(serde_json::json!({
                "type": "image",
                "media_type": img.media_type,
                "thumb": img.thumb,
            }));
        }
        self.inject(
            serde_json::json!({"type": "user_input", "content": content}),
            true,
        );

        // The actual stream-json line claude consumes (full base64 inline).
        let mut claude_content = Vec::new();
        for img in images {
            claude_content.push(serde_json::json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": img.media_type,
                    "data": img.data,
                }
            }));
        }
        if !text.is_empty() {
            claude_content.push(serde_json::json!({"type": "text", "text": text}));
        }
        let line = serde_json::json!({
            "type": "user",
            "message": {"role": "user", "content": claude_content}
        })
        .to_string();

        self.write_line(&line).await
    }

    /// Best-effort interrupt of the in-flight turn (decision 12).
    pub async fn interrupt(&self) -> Result<(), ChatError> {
        let line = serde_json::json!({
            "type": "control_request",
            "request_id": uuid::Uuid::new_v4().to_string(),
            "request": {"subtype": "interrupt"}
        })
        .to_string();
        self.write_line(&line).await
    }

    async fn write_line(&self, line: &str) -> Result<(), ChatError> {
        let bytes = format!("{line}\n").into_bytes();
        let mut guard = self.stdin.lock().await;
        let stdin = guard
            .as_mut()
            .ok_or_else(|| ChatError::Closed("process not running".into()))?;
        stdin
            .write_all(&bytes)
            .await
            .map_err(|e| ChatError::Closed(e.to_string()))?;
        stdin
            .flush()
            .await
            .map_err(|e| ChatError::Closed(e.to_string()))
    }

    /// Switch model by respawning with `--resume <current id> --model <new>`.
    /// The conversation continues; subscribers stay attached because the
    /// broadcast channel and buffers are preserved across the respawn.
    pub async fn switch_model(self: &Arc<Self>, model: &str) -> Result<(), ChatError> {
        validate_token(model)?;
        let sid = self.claude_session_id();
        {
            let mut spec = self.spec.lock();
            spec.model = Some(model.to_string());
            if !sid.is_empty() {
                spec.resume = Some(sid);
            }
        }
        *self.model.lock() = Some(model.to_string());
        self.kill();
        self.inject(
            serde_json::json!({"type": "chat_status", "state": "switching", "model": model}),
            false,
        );
        self.start().await?;
        self.inject(
            serde_json::json!({"type": "chat_status", "state": "running", "model": model}),
            false,
        );
        Ok(())
    }

    /// Revive a dead conversation in place (after crash / host restart),
    /// resuming the same Claude session id if known. If the previous revive
    /// died before its `system:init`, the resume id is stale — fall back to
    /// a fresh conversation and tell the user (U5).
    pub async fn revive(self: &Arc<Self>) -> Result<(), ChatError> {
        let fallback = self.resume_suspect.swap(false, Ordering::SeqCst);
        let sid = self.claude_session_id();
        {
            let mut spec = self.spec.lock();
            if fallback {
                spec.resume = None;
            } else if !sid.is_empty() {
                spec.resume = Some(sid);
            }
            spec.start_seq = self.seq.load(Ordering::SeqCst);
        }
        if fallback {
            self.inject(
                serde_json::json!({
                    "type": "chat_error",
                    "message": "前回の会話を再開できなかったため、新しい会話を開始します。",
                }),
                false,
            );
        }
        self.start().await?;
        let model = self.current_model();
        self.inject(
            serde_json::json!({"type": "chat_status", "state": "running", "model": model}),
            false,
        );
        Ok(())
    }

    /// Spawn (or respawn) the child process and its stdout reader.
    pub async fn start(self: &Arc<Self>) -> Result<(), ChatError> {
        let spec = self.spec.lock().clone();
        let cmdline = build_cmdline(&spec)?;
        tracing::info!(session = %self.name, cmd = %cmdline, "spawning chat process");

        let mut command = tokio::process::Command::new(&spec.shell);
        command
            .arg("-lc")
            .arg(format!("exec {cmdline}"))
            .current_dir(&spec.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        for (k, v) in &spec.env {
            command.env(k, v);
        }

        let mut child = command
            .spawn()
            .map_err(|e| ChatError::Spawn(e.to_string()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| ChatError::Spawn("no stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ChatError::Spawn("no stdout".into()))?;
        let stderr = child.stderr.take();

        // Drain stderr to the log so CLI startup errors are visible.
        if let Some(stderr) = stderr {
            let name = self.name.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::debug!(target: "chat", session = %name, "stderr: {line}");
                }
            });
        }

        self.saw_init.store(false, Ordering::SeqCst);
        let my_gen = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let weak: Weak<ChatSession> = Arc::downgrade(self);
        let name = self.name.clone();
        let reader = tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => {
                        if line.trim().is_empty() {
                            continue;
                        }
                        if let Some(session) = weak.upgrade() {
                            session.handle_stdout_line(&line);
                        } else {
                            break;
                        }
                    }
                    Ok(None) => break, // EOF: child exited.
                    Err(e) => {
                        tracing::debug!(target: "chat", session = %name, "stdout read error: {e}");
                        break;
                    }
                }
            }
            // Process ended. Report a crash only if this is still the
            // current generation (a respawn would have bumped it).
            if let Some(session) = weak.upgrade() {
                session.on_reader_end(my_gen);
            }
        });

        *self.stdin.lock().await = Some(stdin);
        *self.proc.lock() = Some(Proc { child, reader });
        Ok(())
    }

    /// Parse one stdout JSON line, classify it, and broadcast an envelope.
    fn handle_stdout_line(&self, line: &str) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            tracing::debug!(target: "chat", session = %self.name, "non-JSON stdout: {line}");
            return;
        };
        let ty = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
        match ty {
            // Housekeeping — dropped (decision 3).
            "rate_limit_event" => {}
            "system" => {
                // Capture session id + model from init for resume/switch.
                if value.get("subtype").and_then(|v| v.as_str()) == Some("init") {
                    self.saw_init.store(true, Ordering::SeqCst);
                    if let Some(sid) = value.get("session_id").and_then(|v| v.as_str()) {
                        *self.claude_session_id.lock() = sid.to_string();
                    }
                    if let Some(m) = value.get("model").and_then(|v| v.as_str()) {
                        *self.model.lock() = Some(m.to_string());
                    }
                }
                self.inject(value, false);
            }
            // Committed logical messages, persisted with a `_seq` (decision 6:
            // user / assistant / result). The frontend renders user/assistant
            // and treats `result` as a turn-end marker (cost hidden, dec. 13).
            "assistant" | "user" | "result" => {
                let is_result = ty == "result";
                self.inject(value, true);
                if is_result {
                    // Trim the in-flight buffer at the turn boundary so a later
                    // reconnect doesn't replay a finished turn's token deltas.
                    self.inflight.lock().clear();
                }
            }
            _ => self.inject(value, false),
        }
    }

    fn on_reader_end(&self, my_gen: u64) {
        // A respawn bumps the generation; if ours is stale this EOF belongs
        // to a process we already replaced, so ignore it.
        if self.generation.load(Ordering::SeqCst) != my_gen {
            return;
        }
        *self.proc.lock() = None;
        // A resumed process that died before its `system:init` means the
        // resume id is stale; the next revive starts a fresh conversation.
        if !self.saw_init.load(Ordering::SeqCst) && self.spec.lock().resume.is_some() {
            self.resume_suspect.store(true, Ordering::SeqCst);
        }
        self.inject(
            serde_json::json!({"type": "chat_status", "state": "dead"}),
            false,
        );
        if let Some(mgr) = self.manager.upgrade() {
            mgr.fire_exit(&self.name);
        }
    }

    /// Kill the underlying process (graceful stop is just dropping stdin,
    /// but an explicit kill is used for model switch / session delete).
    pub fn kill(&self) {
        if let Some(mut proc) = self.proc.lock().take() {
            proc.reader.abort();
            let _ = proc.child.start_kill();
        }
    }
}

/// One inline image attached to a user turn.
#[derive(Clone)]
pub struct ChatImage {
    pub media_type: String,
    /// base64-encoded bytes (no data: prefix).
    pub data: String,
    /// Optional small thumbnail (data URL) for transcript display.
    pub thumb: Option<String>,
}

fn build_cmdline(spec: &ChatSpawnSpec) -> Result<String, ChatError> {
    if spec.command.trim().is_empty() {
        return Err(ChatError::Invalid("empty claude command".into()));
    }
    let mut parts: Vec<String> = vec![
        spec.command.clone(),
        "-p".into(),
        "--input-format".into(),
        "stream-json".into(),
        "--output-format".into(),
        "stream-json".into(),
        "--verbose".into(),
        "--include-partial-messages".into(),
    ];
    if let Some(flag) = &spec.skip_permissions_flag {
        parts.push(flag.clone());
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
    let extra = spec.extra_args.trim();
    if !extra.is_empty() {
        parts.push(extra.to_string());
    }
    Ok(parts.join(" "))
}

/// Model names and session ids are placed on the shell command line, so we
/// constrain them to an unambiguous, shell-safe charset.
fn validate_token(s: &str) -> Result<(), ChatError> {
    if s.is_empty() || s.len() > 128 {
        return Err(ChatError::Invalid(format!("token length: {s:?}")));
    }
    if s.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        Ok(())
    } else {
        Err(ChatError::Invalid(format!("token charset: {s:?}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_spec() -> ChatSpawnSpec {
        ChatSpawnSpec {
            name: "cc-x".into(),
            cwd: "/tmp".into(),
            shell: "/bin/bash".into(),
            command: "claude".into(),
            skip_permissions_flag: Some("--dangerously-skip-permissions".into()),
            extra_args: String::new(),
            env: vec![],
            model: None,
            resume: None,
            start_seq: 0,
        }
    }

    #[test]
    fn cmdline_minimal() {
        let c = build_cmdline(&base_spec()).unwrap();
        assert!(c.starts_with("claude -p --input-format stream-json"));
        assert!(c.contains("--include-partial-messages"));
        assert!(c.contains("--dangerously-skip-permissions"));
    }

    #[test]
    fn cmdline_model_and_resume() {
        let mut s = base_spec();
        s.model = Some("opus".into());
        s.resume = Some("abc-123".into());
        let c = build_cmdline(&s).unwrap();
        assert!(c.contains("--model opus"));
        assert!(c.contains("--resume abc-123"));
    }

    #[test]
    fn rejects_injection_in_model() {
        let mut s = base_spec();
        s.model = Some("opus; rm -rf /".into());
        assert!(build_cmdline(&s).is_err());
    }

    #[test]
    fn validate_token_ok_and_bad() {
        assert!(validate_token("claude-opus-4-1.x").is_ok());
        assert!(validate_token("a b").is_err());
        assert!(validate_token("$(x)").is_err());
        assert!(validate_token("").is_err());
    }
}
