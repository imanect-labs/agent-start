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

use crate::driver::{self, AgentDriver, ModelSwitch};
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
    driver: Mutex<Arc<dyn AgentDriver>>,
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
            driver: Mutex::new(
                driver::resolve(&spec.driver)
                    .unwrap_or_else(|_| driver::resolve("").expect("the built-in driver resolves")),
            ),
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

    fn driver(&self) -> Arc<dyn AgentDriver> {
        self.driver.lock().clone()
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
        // Held until the line is written. The wire format is the current
        // driver's, and a switch part-way through would hand a
        // Claude-shaped line to the codex process that replaced it.
        let _guard = self.lifecycle.lock().await;
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

        let turn = self.driver().user_turn(text, images);
        for notice in turn.notices {
            self.inject(
                serde_json::json!({"type": "chat_notice", "message": notice}),
                false,
            );
        }
        for line in &turn.writes {
            self.write_line(line).await?;
        }
        Ok(())
    }

    /// Best-effort interrupt of the in-flight turn (decision 12).
    pub async fn interrupt(&self) -> Result<(), ChatError> {
        // As in `send_user_message`: pick the format and write it without
        // letting a switch replace the process in between.
        let _guard = self.lifecycle.lock().await;
        match self.driver().interrupt() {
            Some(line) => self.write_line(&line).await,
            // A protocol with no interrupt: stopping means killing, which
            // is the user's other button.
            None => Ok(()),
        }
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

    /// Point the conversation at a different model of the same agent.
    ///
    /// How that happens is the driver's to say: Claude carries `--model`
    /// on its command line, so it respawns with `--resume <current id>`
    /// and the conversation continues; an agent that sets the model
    /// inside the session says so with `ModelSwitch::InSession` and keeps
    /// the process. Subscribers stay attached either way — the broadcast
    /// channel and buffers outlive a respawn.
    pub async fn switch_model(self: &Arc<Self>, model: &str) -> Result<(), ChatError> {
        validate_token(model)?;
        let _guard = self.lifecycle.lock().await;
        let driver = self.driver();

        if let ModelSwitch::InSession(lines) = driver.model_switch(model) {
            self.spec.lock().model = Some(model.to_string());
            *self.model.lock() = Some(model.to_string());
            for line in &lines {
                self.write_line(line).await?;
            }
            self.inject_status("running");
            return Ok(());
        }

        let sid = self.claude_session_id();
        // Same reasoning as `revive`: only a driver that has a resume
        // gets one. Without this the doc comment above would be a lie —
        // the respawn would start a conversation that has never heard of
        // anything said so far.
        let resumable = driver.supports_resume();
        {
            let mut spec = self.spec.lock();
            spec.model = Some(model.to_string());
            if resumable && !sid.is_empty() {
                spec.resume = Some(sid);
            }
        }
        *self.model.lock() = Some(model.to_string());
        self.kill();
        self.inject_status("switching");
        self.restart_after_switch().await?;
        if !resumable {
            self.inject(
                serde_json::json!({
                    "type": "chat_notice",
                    "message": format!(
                        "{} はモデルを変えると新しい会話になります。ここまでの内容は引き継がれません。",
                        self.current_provider()
                    ),
                }),
                false,
            );
        }
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
        let driver = driver::resolve(driver)?;
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
            spec.driver = driver.name().to_string();
            spec.command = command.to_string();
            spec.model = model.map(str::to_string);
            // Permission modes are Claude's. Carried across a switch they
            // would keep showing on `chat_status` while the new command
            // line never mentions them — and `set_permission_mode` refuses
            // to touch a non-Claude session, so the toggle would be stuck
            // on with no way back.
            if !driver.supports_permission_mode() {
                spec.permission_mode = None;
            }
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
        let driver = driver::resolve(&spec.driver)?;
        // Retire the previous generation *before* the shared driver changes
        // under it. A reader still draining its own process must never see
        // itself as current once the vocabulary has moved on, not even for
        // the instant between these two lines.
        let my_gen = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        *self.driver.lock() = driver.clone();
        let cmdline = build_cmdline(driver.as_ref(), &spec)?;
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
        let reader_driver = driver.clone();
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
                            session
                                .handle_stdout_line(&line, my_gen, reader_driver.as_ref())
                                .await;
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

        // Whatever the protocol needs said before the first user turn
        // (Claude's `initialize`, without which it never routes
        // `can_use_tool` over stdio). A protocol with no handshake
        // returns nothing.
        for line in driver.handshake(my_gen) {
            if let Err(e) = self.write_line(&line).await {
                tracing::warn!(session = %self.name, error = %e, "failed to send the chat handshake");
            }
        }
        Ok(())
    }

    /// Hand one stdout line to the driver, then act on what it says.
    ///
    /// `my_gen` and `driver` are the reader's own, fixed when it was
    /// spawned. Reading the session's current driver here instead would
    /// mean parsing this process's output in whatever vocabulary happens
    /// to be installed by the time the line is handled.
    async fn handle_stdout_line(&self, line: &str, my_gen: u64, driver: &dyn AgentDriver) {
        if !self.is_current_generation(my_gen) {
            tracing::trace!(target: "chat", session = %self.name, "stale stdout dropped: {line}");
            return;
        }
        let out = driver.on_line(line);
        if out.events.is_empty() && out.writes.is_empty() {
            tracing::trace!(target: "chat", session = %self.name, "event dropped: {line}");
        }
        for event in out.events {
            self.handle_event(event).await;
        }
        // Written here rather than inside the driver, and only while the
        // process that asked is still the one running: an answer sent to
        // its successor answers a question that one never asked.
        for reply in out.writes {
            if !self.is_current_generation(my_gen) {
                return;
            }
            if let Err(e) = self.write_line(&reply).await {
                tracing::warn!(session = %self.name, error = %e, "failed to answer the agent");
            }
        }
    }

    /// Whether the process a reader was spawned for is still the one this
    /// session is running. `kill` aborts the reader, but an abort only
    /// lands at the next await point, so output already buffered can
    /// still arrive after a switch has replaced the process.
    fn is_current_generation(&self, my_gen: u64) -> bool {
        self.generation.load(Ordering::SeqCst) == my_gen
    }

    /// Handle one envelope, already in Claude's stream-json vocabulary.
    ///
    /// Everything protocol-specific has been dealt with by the driver;
    /// what is left is rendering, persistence and the bits of session
    /// state an envelope carries.
    async fn handle_event(&self, value: serde_json::Value) {
        let ty = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
        match ty {
            // A tool whose answer is the user's (#95). Remembered so the
            // reply can be matched to the request it belongs to, then
            // shown as a card.
            "chat_permission" => {
                let request_id = value
                    .get("request_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let tool_name = value
                    .get("tool")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let input = value
                    .get("input")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({}));
                self.pending_perms
                    .lock()
                    .insert(request_id, PendingPerm { tool_name, input });
                self.inject(value, false);
            }
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
        let line = self.driver().permission_reply(&request_id, response);
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
        // same lock, so a check made outside could pass on one agent and
        // then act on the process that replaced it.
        if !self.driver().supports_permission_mode() {
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
        // Retire the reader here, not later in `start`. Between the two
        // its process is dead but its generation is still current, and
        // `abort` does not reach a line already buffered: that line would
        // register a permission card just after `resolve_all_pending`
        // below has cleared them, leaving the user a live-looking prompt
        // whose answer `respond_permission` writes to whichever process
        // has taken this one's place.
        self.generation.fetch_add(1, Ordering::SeqCst);
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

/// The command line for a provider, once the driver has had its say.
///
/// The empty-command check lives here rather than in each driver: a
/// provider with nothing to run is a configuration mistake, not a
/// protocol detail.
fn build_cmdline(driver: &dyn AgentDriver, spec: &ChatSpawnSpec) -> Result<String, ChatError> {
    if spec.command.trim().is_empty() {
        return Err(ChatError::Invalid(format!(
            "provider `{}` has no command to run",
            spec.provider
        )));
    }
    driver.command_line(spec)
}

/// Model names and session ids are placed on the shell command line, so we
/// constrain them to an unambiguous, shell-safe charset.
pub(crate) fn validate_token(s: &str) -> Result<(), ChatError> {
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

    /// A second driver, so the tests can tell "the reader's protocol"
    /// apart from "the session's protocol". It also keeps the trait
    /// honest: an abstraction with exactly one implementation is only a
    /// guess about where the seams are.
    #[derive(Debug, Default)]
    struct TestDriver {
        /// Whether this agent changes model without being restarted.
        model_in_session: bool,
    }

    /// What `TestDriver` reads. Claude would find no `type` here and pass
    /// it to the browser as raw JSON, which is how the two are told apart.
    const OTHER_LINE: &str = r#"{"said":"hi"}"#;

    impl AgentDriver for TestDriver {
        fn name(&self) -> &'static str {
            "test-driver"
        }
        fn command_line(&self, _spec: &ChatSpawnSpec) -> Result<String, ChatError> {
            Ok("true".into())
        }
        fn handshake(&self, _generation: u64) -> Vec<String> {
            vec![]
        }
        fn user_turn(&self, text: &str, _images: &[ChatImage]) -> crate::driver::UserTurn {
            crate::driver::UserTurn {
                writes: vec![text.to_string()],
                notices: vec![],
            }
        }
        fn interrupt(&self) -> Option<String> {
            None
        }
        fn on_line(&self, line: &str) -> crate::driver::DriverOutput {
            let v: serde_json::Value = serde_json::from_str(line).unwrap_or_default();
            let said = v.get("said").and_then(|s| s.as_str()).unwrap_or("");
            crate::driver::DriverOutput::event(serde_json::json!({
                "type": "assistant",
                "message": {"role": "assistant", "content": [{"type": "text", "text": said}]},
            }))
        }
        fn permission_reply(&self, _request_id: &str, _response: serde_json::Value) -> String {
            String::new()
        }
        fn supports_resume(&self) -> bool {
            false
        }
        fn supports_permission_mode(&self) -> bool {
            false
        }
        fn model_switch(&self, _model: &str) -> ModelSwitch {
            if self.model_in_session {
                // No lines: a driver with something to send would need a
                // live process, and what this exercises is the branch, not
                // the wire.
                ModelSwitch::InSession(vec![])
            } else {
                ModelSwitch::Respawn
            }
        }
    }

    /// `kill` aborts a reader, but the abort lands at its next await —
    /// output already buffered can still arrive after a switch. It must
    /// not land in the transcript of the conversation that replaced it.
    #[tokio::test]
    async fn output_from_a_replaced_process_is_dropped() {
        let session = ChatSession::create(base_spec(), Weak::new());
        let (_, mut rx) = session.subscribe();

        // Generation 0 is the session's own, so this reader is current.
        session
            .handle_stdout_line(OTHER_LINE, 0, &TestDriver::default())
            .await;
        let first: serde_json::Value =
            serde_json::from_str(&rx.try_recv().expect("current output was dropped")).unwrap();
        assert_eq!(first["type"], "assistant");

        // A respawn bumps the generation out from under the old reader.
        session.generation.fetch_add(1, Ordering::SeqCst);
        session
            .handle_stdout_line(OTHER_LINE, 0, &TestDriver::default())
            .await;
        assert!(
            rx.try_recv().is_err(),
            "a replaced process still reached subscribers"
        );
    }

    /// The gap between `kill` and the respawn: the old process is dead but
    /// nothing has started yet. A buffered `can_use_tool` arriving here
    /// must not leave a card behind — `kill` has already retired the
    /// pending ones, and answering this one would write a response to the
    /// process that replaces it, for a request it never made.
    #[tokio::test]
    async fn a_killed_reader_cannot_leave_a_permission_card_behind() {
        const ASK: &str = r#"{"type":"control_request","request_id":"req-9","request":{"subtype":"can_use_tool","tool_name":"AskUserQuestion","input":{}}}"#;
        let session = ChatSession::create(base_spec(), Weak::new());
        let (_, mut rx) = session.subscribe();
        let claude = driver::resolve("claude-stream-json").unwrap();

        session.kill();
        session.handle_stdout_line(ASK, 0, claude.as_ref()).await;

        assert!(
            session.pending_perms.lock().is_empty(),
            "a dead process left a permission card the user could answer"
        );
        assert!(rx.try_recv().is_err(), "the card reached the browser");
    }

    /// A line is read in the vocabulary of the process that wrote it, not
    /// whichever one the session has since been pointed at.
    #[tokio::test]
    async fn a_reader_parses_with_its_own_vocabulary() {
        let session = ChatSession::create(base_spec(), Weak::new());
        let (_, mut rx) = session.subscribe();
        // The session already speaks Claude; the reader does not.
        assert_eq!(session.driver().name(), "claude-stream-json");

        session
            .handle_stdout_line(OTHER_LINE, 0, &TestDriver::default())
            .await;

        let got: serde_json::Value =
            serde_json::from_str(&rx.try_recv().expect("output was dropped")).unwrap();
        // Read as Claude this line has no `type`, so it would fall through
        // to the catch-all and reach the browser as raw JSON.
        assert_eq!(got["type"], "assistant");
        assert_eq!(got["message"]["content"][0]["text"], "hi");
    }

    /// An interactive tool becomes a card the user can answer; the card's
    /// arrival is what registers it, so a driver that never raises one
    /// cannot leave the session holding a pending permission.
    #[tokio::test]
    async fn a_permission_card_is_registered_when_it_reaches_the_session() {
        const ASK: &str = r#"{"type":"control_request","request_id":"req-3","request":{"subtype":"can_use_tool","tool_name":"AskUserQuestion","input":{"q":1}}}"#;
        let session = ChatSession::create(base_spec(), Weak::new());
        let (_, mut rx) = session.subscribe();
        let claude = driver::resolve("claude-stream-json").unwrap();

        session.handle_stdout_line(ASK, 0, claude.as_ref()).await;

        assert!(session.pending_perms.lock().contains_key("req-3"));
        let card: serde_json::Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
        assert_eq!(card["type"], "chat_permission");
    }

    /// `InSession` keeps the process. Claude respawns for `--model`, so
    /// nothing exercises this branch until a second driver arrives — and
    /// by then a mistake here would look like a lost conversation.
    #[tokio::test]
    async fn a_model_switch_in_session_does_not_replace_the_process() {
        let session = ChatSession::create(base_spec(), Weak::new());
        *session.driver.lock() = Arc::new(TestDriver {
            model_in_session: true,
        });
        let before = session.generation.load(Ordering::SeqCst);

        session.switch_model("gpt-5").await.unwrap();

        assert_eq!(session.current_model().as_deref(), Some("gpt-5"));
        assert_eq!(session.spec.lock().model.as_deref(), Some("gpt-5"));
        // `kill` retires the generation, so an unchanged one is the proof
        // that no respawn happened.
        assert_eq!(
            session.generation.load(Ordering::SeqCst),
            before,
            "the process was replaced for an in-session switch"
        );
    }

    #[test]
    fn an_unknown_driver_is_an_error_not_a_silent_fallback() {
        // Driving `gemini` with Claude's protocol produces a process that
        // starts, says nothing, and looks like a hang.
        assert!(driver::resolve("gemini-cli").is_err());
        assert!(driver::resolve("claude-stream-json").is_ok());
        assert!(driver::resolve("").is_ok(), "the default must resolve");
    }

    #[test]
    fn a_provider_with_no_command_is_a_configuration_error() {
        let mut s = base_spec();
        s.command = "  ".into();
        let claude = driver::resolve("").unwrap();
        assert!(build_cmdline(claude.as_ref(), &s).is_err());
    }

    #[test]
    fn validate_token_ok_and_bad() {
        assert!(validate_token("claude-opus-4-1.x").is_ok());
        assert!(validate_token("a b").is_err());
        assert!(validate_token("$(x)").is_err());
        assert!(validate_token("").is_err());
    }
}
