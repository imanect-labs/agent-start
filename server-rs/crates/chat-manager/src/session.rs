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

use crate::driver::codex;
use crate::driver::Driver;
use crate::error::ChatError;
use parking_lot::Mutex;
use std::collections::{HashMap, VecDeque};
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
    /// Which configured chat provider this conversation is talking to.
    /// Changing it is what the composer's picker does.
    pub provider: String,
    /// The provider's stdio protocol (`chat.providers[].driver`).
    pub driver: String,
    /// The agent program to run (from the provider, not the CLI entry).
    pub command: String,
    /// Legacy skip-permissions flag. Chat no longer skips permissions
    /// (#95): it spawns with `--permission-prompt-tool stdio` so AskUserQuestion
    /// and plan approval can surface, auto-allowing every other tool. Kept on
    /// the spec for config compatibility but not added to the command line.
    pub skip_permissions_flag: Option<String>,
    /// Sanitized extra args appended verbatim.
    pub extra_args: String,
    pub env: Vec<(String, String)>,
    /// Initial model (`--model`), or None for the CLI default.
    pub model: Option<String>,
    /// Permission mode (`--permission-mode`), e.g. `"plan"` to make Claude
    /// draft a plan and call ExitPlanMode for approval (#95). `None` = default.
    pub permission_mode: Option<String>,
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
    /// Protocol of the process currently running. Resolved on `start`
    /// so a bad `driver` in config fails loudly there instead of being
    /// papered over with Claude's vocabulary.
    driver: Mutex<Driver>,
    /// Submission counter for request/response protocols.
    submission: AtomicU64,
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
    /// Serializes user-driven lifecycle transitions (`switch_model` /
    /// `revive`) so concurrent control messages from multiple WebSocket
    /// clients can't interleave overlapping respawns.
    lifecycle: tokio::sync::Mutex<()>,
    /// Lossless persistence sink. Every committed envelope is forwarded
    /// here (in addition to the lossy live broadcast) so the host can write
    /// the transcript to SQLite without dropping messages under backpressure.
    commit_tx: Mutex<Option<tokio::sync::mpsc::UnboundedSender<CommitEvent>>>,
    /// In-flight `can_use_tool` requests awaiting a user decision (#95),
    /// keyed by the CLI's `request_id`. Only AskUserQuestion / ExitPlanMode
    /// land here — every other tool is auto-allowed inline.
    pending_perms: Mutex<HashMap<String, PendingPerm>>,
    manager: Weak<crate::manager::ChatManager>,
}

/// A tool-permission request forwarded to the UI, retained so the user's
/// reply can be turned into the matching `control_response`.
struct PendingPerm {
    tool_name: String,
    input: serde_json::Value,
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
            lifecycle: tokio::sync::Mutex::new(()),
            driver: Mutex::new(Driver::parse(&spec.driver).unwrap_or(Driver::ClaudeStreamJson)),
            submission: AtomicU64::new(0),
            spec: Mutex::new(spec),
            proc: Mutex::new(None),
            stdin: tokio::sync::Mutex::new(None),
            tx,
            inflight: Arc::new(Mutex::new(VecDeque::new())),
            commit_tx: Mutex::new(None),
            pending_perms: Mutex::new(HashMap::new()),
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

    /// The provider this conversation is currently talking to.
    pub fn current_provider(&self) -> String {
        self.spec.lock().provider.clone()
    }

    fn driver(&self) -> Driver {
        *self.driver.lock()
    }

    fn next_submission_id(&self) -> String {
        self.submission.fetch_add(1, Ordering::SeqCst).to_string()
    }

    /// The permission mode the process was (re)spawned with (`"plan"` etc.),
    /// or `None` for the default. Surfaced on every `chat_status` so the UI
    /// toggle survives reconnects / model switches (#95).
    pub fn permission_mode(&self) -> Option<String> {
        self.spec.lock().permission_mode.clone()
    }

    pub fn claude_session_id(&self) -> String {
        self.claude_session_id.lock().clone()
    }

    /// Emit a `chat_status` envelope carrying the current model + permission
    /// mode. Centralized so every status frame keeps the same shape and the
    /// UI never loses the plan-mode toggle on a reconnect or respawn (#95).
    fn inject_status(&self, state: &str) {
        self.inject(
            serde_json::json!({
                "type": "chat_status",
                "state": state,
                "provider": self.current_provider(),
                "model": self.current_model(),
                "permissionMode": self.permission_mode(),
            }),
            false,
        );
    }

    /// Drain pending permission requests, emitting a `chat_permission_resolved`
    /// for each so live clients retire the card and reconnect replay nets the
    /// stale `chat_permission` (which lives in `inflight`) back out (#95).
    fn resolve_all_pending(&self) {
        let ids: Vec<String> = self.pending_perms.lock().drain().map(|(k, _)| k).collect();
        for id in ids {
            self.inject(
                serde_json::json!({"type": "chat_permission_resolved", "request_id": id}),
                false,
            );
        }
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

        let line = match self.driver() {
            Driver::ClaudeStreamJson => {
                // The actual stream-json line claude consumes (full
                // base64 inline).
                let mut content = Vec::new();
                for img in images {
                    content.push(serde_json::json!({
                        "type": "image",
                        "source": {
                            "type": "base64",
                            "media_type": img.media_type,
                            "data": img.data,
                        }
                    }));
                }
                if !text.is_empty() {
                    content.push(serde_json::json!({"type": "text", "text": text}));
                }
                serde_json::json!({
                    "type": "user",
                    "message": {"role": "user", "content": content}
                })
                .to_string()
            }
            Driver::CodexProto => {
                // Say what could not be sent, in the transcript rather
                // than in the prompt: text appended to the prompt would
                // reach the model and change its answer.
                if let Some(notice) = codex::dropped_images_notice(images.len()) {
                    self.inject(
                        serde_json::json!({"type": "chat_notice", "message": notice}),
                        false,
                    );
                }
                codex::user_submission(&self.next_submission_id(), text)
            }
        };

        self.write_line(&line).await
    }

    /// Best-effort interrupt of the in-flight turn (decision 12).
    pub async fn interrupt(&self) -> Result<(), ChatError> {
        let line = match self.driver() {
            Driver::ClaudeStreamJson => serde_json::json!({
                "type": "control_request",
                "request_id": uuid::Uuid::new_v4().to_string(),
                "request": {"subtype": "interrupt"}
            })
            .to_string(),
            Driver::CodexProto => codex::interrupt_submission(&self.next_submission_id()),
        };
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
        let _guard = self.lifecycle.lock().await;
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
        self.inject_status("switching");
        self.restart_after_switch().await?;
        self.inject_status("running");
        Ok(())
    }

    /// Point the conversation at a different agent.
    ///
    /// Unlike a model switch this cannot continue the conversation:
    /// another provider has never seen it and holds no session to resume.
    /// The transcript above stays on screen — it is the user's record of
    /// what happened — but the new agent starts without that context, and
    /// the user is told so rather than left to discover it by asking a
    /// follow-up question that lands on a blank slate.
    pub async fn switch_provider(
        self: &Arc<Self>,
        provider: &str,
        command: &str,
        driver: &str,
        model: Option<&str>,
    ) -> Result<(), ChatError> {
        let driver = Driver::parse(driver)?;
        if command.trim().is_empty() {
            return Err(ChatError::Invalid(format!(
                "provider `{provider}` has no command configured"
            )));
        }
        if let Some(m) = model {
            validate_token(m)?;
        }
        let _guard = self.lifecycle.lock().await;
        let same_provider = self.current_provider() == provider;
        {
            let mut spec = self.spec.lock();
            spec.provider = provider.to_string();
            spec.driver = driver.as_str().to_string();
            spec.command = command.to_string();
            spec.model = model.map(str::to_string);
            // A session id belongs to the agent that issued it.
            spec.resume = None;
            spec.start_seq = self.seq.load(Ordering::SeqCst);
        }
        *self.model.lock() = model.map(str::to_string);
        *self.claude_session_id.lock() = String::new();
        // The in-flight buffer holds the deltas of a turn the *previous*
        // agent was part-way through. Replaying those to a client that
        // reconnects after the switch would show partial text from a
        // conversation that no longer exists.
        self.inflight.lock().clear();
        self.kill();
        self.inject_status("switching");
        self.restart_after_switch().await?;
        if !same_provider {
            self.inject(
                serde_json::json!({
                    "type": "chat_notice",
                    "message": format!(
                        "エージェントを {provider} に切り替えました。ここまでの会話は引き継がれません。"
                    ),
                }),
                false,
            );
        }
        self.inject_status("running");
        Ok(())
    }

    /// Revive a dead conversation in place (after crash / host restart),
    /// resuming the same Claude session id if known. If the previous revive
    /// died before its `system:init`, the resume id is stale — fall back to
    /// a fresh conversation and tell the user (U5).
    pub async fn revive(self: &Arc<Self>) -> Result<(), ChatError> {
        let _guard = self.lifecycle.lock().await;
        // Another concurrent revive may have already brought it back.
        if self.is_alive() {
            return Ok(());
        }
        let fallback = self.resume_suspect.swap(false, Ordering::SeqCst);
        let sid = self.claude_session_id();
        // Only where the protocol actually has a resume. Setting it on a
        // driver that ignores it would leave a stale id on the spec, and
        // the next reader of that field would believe the conversation
        // could be continued when it cannot.
        let resumable = self.driver().supports_resume();
        {
            let mut spec = self.spec.lock();
            if fallback || !resumable {
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
        self.inject_status("running");
        Ok(())
    }

    /// Bring the process back up after a deliberate respawn, reporting
    /// `dead` if it will not start.
    ///
    /// Every switch announces `switching` before killing the old
    /// process. If the new one then fails to spawn there is no reader to
    /// notice its end, so nothing would ever correct that status and the
    /// UI would sit on "切り替え中…" forever.
    async fn restart_after_switch(self: &Arc<Self>) -> Result<(), ChatError> {
        if let Err(e) = self.start().await {
            self.inject_status("dead");
            return Err(e);
        }
        Ok(())
    }

    /// Spawn (or respawn) the child process and its stdout reader.
    pub async fn start(self: &Arc<Self>) -> Result<(), ChatError> {
        let spec = self.spec.lock().clone();
        let driver = Driver::parse(&spec.driver)?;
        *self.driver.lock() = driver;
        let cmdline = build_cmdline(driver, &spec)?;
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
                            session.handle_stdout_line(&line).await;
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

        // Interactive permissions handshake (#95): the CLI only starts routing
        // `can_use_tool` over stdio after it receives an `initialize` control
        // request. stdin is read in order, so sending it before the first user
        // turn is enough — no need to await the reply. Claude-only: the codex
        // protocol has no equivalent and would reject the line.
        if driver == Driver::ClaudeStreamJson {
            let init = serde_json::json!({
                "type": "control_request",
                "request_id": format!("init-{}", my_gen),
                "request": {"subtype": "initialize", "hooks": {}}
            })
            .to_string();
            if let Err(e) = self.write_line(&init).await {
                tracing::warn!(session = %self.name, error = %e, "failed to send chat initialize handshake");
            }
        }
        Ok(())
    }

    /// Parse one stdout JSON line, classify it, and broadcast an envelope.
    ///
    /// Providers other than Claude are translated into Claude's
    /// vocabulary first: that is the shape the browser renders, so the
    /// alternative would be a second renderer per agent.
    async fn handle_stdout_line(&self, line: &str) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            tracing::debug!(target: "chat", session = %self.name, "non-JSON stdout: {line}");
            return;
        };
        match self.driver() {
            Driver::ClaudeStreamJson => self.handle_event(value).await,
            Driver::CodexProto => {
                let events = codex::translate(&value);
                if events.is_empty() {
                    tracing::trace!(target: "chat", session = %self.name, "codex event dropped: {line}");
                }
                for event in events {
                    self.handle_event(event).await;
                }
            }
        }
    }

    /// Handle one event already in Claude's stream-json vocabulary.
    async fn handle_event(&self, value: serde_json::Value) {
        let ty = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
        match ty {
            // Housekeeping — dropped (decision 3).
            "rate_limit_event" => {}
            // Tool-permission request (#95): auto-allow everything except the
            // two interactive tools, which we forward to the UI and answer
            // when the user replies.
            "control_request" => {
                let subtype = value
                    .get("request")
                    .and_then(|r| r.get("subtype"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if subtype == "can_use_tool" {
                    self.handle_can_use_tool(&value).await;
                }
            }
            // Replies to our own control requests (initialize / allow). Internal
            // bookkeeping — never shown to the browser.
            "control_response" => {}
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

    /// Classify a `can_use_tool` request: auto-allow ordinary tools inline,
    /// or forward AskUserQuestion / ExitPlanMode to the UI for a decision.
    async fn handle_can_use_tool(&self, value: &serde_json::Value) {
        let request_id = value
            .get("request_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if request_id.is_empty() {
            return;
        }
        let req = value.get("request");
        let tool_name = req
            .and_then(|r| r.get("tool_name"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let input = req
            .and_then(|r| r.get("input"))
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));

        match tool_name.as_str() {
            "AskUserQuestion" | "ExitPlanMode" => {
                self.pending_perms.lock().insert(
                    request_id.clone(),
                    PendingPerm {
                        tool_name: tool_name.clone(),
                        input: input.clone(),
                    },
                );
                self.inject(
                    serde_json::json!({
                        "type": "chat_permission",
                        "request_id": request_id,
                        "tool": tool_name,
                        "input": input,
                    }),
                    false,
                );
            }
            _ => {
                let line = control_response_line(
                    &request_id,
                    serde_json::json!({"behavior": "allow", "updatedInput": input}),
                );
                if let Err(e) = self.write_line(&line).await {
                    tracing::warn!(session = %self.name, error = %e, "failed to auto-allow tool");
                }
            }
        }
    }

    /// Resolve a pending AskUserQuestion / ExitPlanMode request with the
    /// user's decision (#95). A missing `request_id` (already answered or a
    /// stale reconnect double-submit) is a no-op.
    pub async fn respond_permission(
        &self,
        request_id: String,
        allow: bool,
        answers: Option<serde_json::Value>,
        message: Option<String>,
    ) -> Result<(), ChatError> {
        let Some(pending) = self.pending_perms.lock().remove(&request_id) else {
            return Ok(());
        };
        let response = if allow {
            let mut updated_input = pending.input;
            if pending.tool_name == "AskUserQuestion" {
                if let (Some(obj), Some(answers)) = (updated_input.as_object_mut(), answers) {
                    obj.insert("answers".into(), answers);
                }
            }
            serde_json::json!({"behavior": "allow", "updatedInput": updated_input})
        } else {
            serde_json::json!({
                "behavior": "deny",
                "message": message.unwrap_or_else(|| "ユーザーが拒否しました。".into()),
            })
        };
        let line = control_response_line(&request_id, response);
        // Tell any reconnecting client to retire the card it replayed.
        self.inject(
            serde_json::json!({"type": "chat_permission_resolved", "request_id": request_id}),
            false,
        );
        self.write_line(&line).await
    }

    /// Switch the permission mode (e.g. toggle plan mode) by respawning with
    /// `--resume <id> --permission-mode <mode>` (#95). Mirrors `switch_model`.
    ///
    /// Claude only. Accepting it for another driver would store a mode
    /// its command line never carries, and then report it on every
    /// `chat_status` — the toggle would look enabled while the agent had
    /// never heard of it.
    pub async fn set_permission_mode(
        self: &Arc<Self>,
        mode: Option<&str>,
    ) -> Result<(), ChatError> {
        if let Some(m) = mode {
            validate_token(m)?;
        }
        let _guard = self.lifecycle.lock().await;
        // Under the lock, not before it: `switch_provider` holds this
        // same lock, so a check made outside could pass on Claude and
        // then act on the codex process that replaced it.
        if self.driver() != Driver::ClaudeStreamJson {
            return Err(ChatError::Invalid(format!(
                "`{}` は権限モードをサポートしていません",
                self.current_provider()
            )));
        }
        let sid = self.claude_session_id();
        {
            let mut spec = self.spec.lock();
            spec.permission_mode = mode.map(|m| m.to_string());
            if !sid.is_empty() {
                spec.resume = Some(sid);
            }
            spec.start_seq = self.seq.load(Ordering::SeqCst);
        }
        self.kill();
        self.inject_status("switching");
        self.restart_after_switch().await?;
        self.inject_status("running");
        Ok(())
    }

    fn on_reader_end(&self, my_gen: u64) {
        // A respawn bumps the generation; if ours is stale this EOF belongs
        // to a process we already replaced, so ignore it.
        if self.generation.load(Ordering::SeqCst) != my_gen {
            return;
        }
        *self.proc.lock() = None;
        // Drop the dead stdin so a write can't target a stale pipe before a
        // revive swaps in a fresh one.
        if let Ok(mut stdin) = self.stdin.try_lock() {
            *stdin = None;
        }
        // Any unanswered permission requests died with the process; retire them
        // (this also scrubs the replayed `chat_permission` cards from inflight)
        // so a revive doesn't try to answer a stale request_id.
        self.resolve_all_pending();
        // A resumed process that died before its `system:init` means the
        // resume id is stale; the next revive starts a fresh conversation.
        if !self.saw_init.load(Ordering::SeqCst) && self.spec.lock().resume.is_some() {
            self.resume_suspect.store(true, Ordering::SeqCst);
        }
        self.inject_status("dead");
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
        if let Ok(mut stdin) = self.stdin.try_lock() {
            *stdin = None;
        }
        // Retire any pending permission cards (and scrub their inflight replay)
        // so a respawn doesn't leave stale, un-answerable prompts (#95).
        self.resolve_all_pending();
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

/// Build the stream-json line that answers a `can_use_tool` request. `inner`
/// is the `{behavior, ...}` decision object (#95).
fn control_response_line(request_id: &str, inner: serde_json::Value) -> String {
    serde_json::json!({
        "type": "control_response",
        "response": {
            "subtype": "success",
            "request_id": request_id,
            "response": inner,
        }
    })
    .to_string()
}

fn build_cmdline(driver: Driver, spec: &ChatSpawnSpec) -> Result<String, ChatError> {
    if spec.command.trim().is_empty() {
        return Err(ChatError::Invalid(format!(
            "provider `{}` has no command to run",
            spec.provider
        )));
    }
    match driver {
        Driver::ClaudeStreamJson => claude_cmdline(spec),
        Driver::CodexProto => codex_cmdline(spec),
    }
}

/// `codex proto`: newline-delimited submissions in, events out.
///
/// Flag placement here is the part most likely to need adjusting if the
/// codex CLI moves — see the note at the top of `driver/codex.rs`.
fn codex_cmdline(spec: &ChatSpawnSpec) -> Result<String, ChatError> {
    let mut parts: Vec<String> = vec![spec.command.clone(), "proto".into()];
    if let Some(model) = &spec.model {
        validate_token(model)?;
        parts.push("--model".into());
        parts.push(model.clone());
    }
    let extra = spec.extra_args.trim();
    if !extra.is_empty() {
        parts.push(extra.to_string());
    }
    Ok(parts.join(" "))
}

fn claude_cmdline(spec: &ChatSpawnSpec) -> Result<String, ChatError> {
    let mut parts: Vec<String> = vec![
        spec.command.clone(),
        "-p".into(),
        "--input-format".into(),
        "stream-json".into(),
        "--output-format".into(),
        "stream-json".into(),
        "--verbose".into(),
        "--include-partial-messages".into(),
        // Interactive permissions (#95): route tool approvals over stdio so
        // AskUserQuestion / ExitPlanMode reach the UI. Every other tool is
        // auto-allowed by the reader, so the UX matches the old skip-permissions
        // path while letting questions and plans surface. Requires the
        // `initialize` handshake we send on spawn — without it the CLI never
        // emits `can_use_tool`.
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
        let c = build_cmdline(Driver::ClaudeStreamJson, &base_spec()).unwrap();
        assert!(c.starts_with("claude -p --input-format stream-json"));
        assert!(c.contains("--include-partial-messages"));
        // #95: chat is interactive, not skip-permissions.
        assert!(c.contains("--permission-prompt-tool stdio"));
        assert!(!c.contains("--dangerously-skip-permissions"));
    }

    #[test]
    fn cmdline_model_and_resume() {
        let mut s = base_spec();
        s.model = Some("opus".into());
        s.resume = Some("abc-123".into());
        let c = build_cmdline(Driver::ClaudeStreamJson, &s).unwrap();
        assert!(c.contains("--model opus"));
        assert!(c.contains("--resume abc-123"));
    }

    #[test]
    fn cmdline_plan_mode() {
        let mut s = base_spec();
        s.permission_mode = Some("plan".into());
        let c = build_cmdline(Driver::ClaudeStreamJson, &s).unwrap();
        assert!(c.contains("--permission-mode plan"));
    }

    #[test]
    fn control_response_shape() {
        let line = control_response_line("req-1", serde_json::json!({"behavior": "allow", "x": 1}));
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["type"], "control_response");
        assert_eq!(v["response"]["subtype"], "success");
        assert_eq!(v["response"]["request_id"], "req-1");
        assert_eq!(v["response"]["response"]["behavior"], "allow");
    }

    #[test]
    fn rejects_injection_in_model() {
        let mut s = base_spec();
        s.model = Some("opus; rm -rf /".into());
        assert!(build_cmdline(Driver::ClaudeStreamJson, &s).is_err());
    }

    #[test]
    fn validate_token_ok_and_bad() {
        assert!(validate_token("claude-opus-4-1.x").is_ok());
        assert!(validate_token("a b").is_err());
        assert!(validate_token("$(x)").is_err());
        assert!(validate_token("").is_err());
    }
}
