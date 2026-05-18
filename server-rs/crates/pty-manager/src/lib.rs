//! In-process PTY multiplexer that replaces tmux.
//!
//! `PtyManager` owns one `PtySession` per `(session name)`, each backed
//! by a `portable_pty` master/child pair. We retain the last ~512 KiB
//! of output in an in-memory ring buffer and broadcast new bytes to any
//! subscribed `Receiver`. On reconnect a client receives the entire
//! buffered history before live tailing.
//!
//! Periodic flushes to SQLite are handled by the host; see
//! `state::append_history`.

use parking_lot::Mutex;
use portable_pty::{CommandBuilder, MasterPty, NativePtySystem, PtySize, PtySystem};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

/// Maximum bytes retained in the live ring buffer per session.
pub const RING_BUFFER_BYTES: usize = 512 * 1024;

/// Capacity of the broadcast channel (number of chunks, each up to 8 KiB).
const BROADCAST_CAP: usize = 256;

#[derive(Debug, Error)]
pub enum PtyError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("pty: {0}")]
    Pty(String),
    #[error("session not found: {0}")]
    NotFound(String),
}

pub struct PtySpawnSpec {
    pub name: String,
    pub cwd: std::path::PathBuf,
    pub shell: String,
    pub command: String,
    pub env: Vec<(String, String)>,
    pub cols: u16,
    pub rows: u16,
}

struct RingBuffer {
    buf: Vec<u8>,
    cap: usize,
}

impl RingBuffer {
    fn new(cap: usize) -> Self {
        Self {
            buf: Vec::with_capacity(cap.min(64 * 1024)),
            cap,
        }
    }
    fn push(&mut self, data: &[u8]) {
        if data.len() >= self.cap {
            self.buf.clear();
            self.buf.extend_from_slice(&data[data.len() - self.cap..]);
            return;
        }
        self.buf.extend_from_slice(data);
        if self.buf.len() > self.cap {
            let excess = self.buf.len() - self.cap;
            self.buf.drain(..excess);
        }
    }
    fn snapshot(&self) -> Vec<u8> {
        self.buf.clone()
    }
}

pub struct PtySession {
    name: String,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    child: Arc<Mutex<Box<dyn portable_pty::Child + Send + Sync>>>,
    ring: Arc<Mutex<RingBuffer>>,
    tx: broadcast::Sender<Vec<u8>>,
    pid: Option<u32>,
    _reader_task: JoinHandle<()>,
}

impl PtySession {
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn subscribe(&self) -> (Vec<u8>, broadcast::Receiver<Vec<u8>>) {
        let snap = self.ring.lock().snapshot();
        (snap, self.tx.subscribe())
    }
    pub fn write(&self, data: &[u8]) -> Result<(), PtyError> {
        let mut writer = self
            .master
            .lock()
            .take_writer()
            .map_err(|e| PtyError::Pty(e.to_string()))?;
        use std::io::Write;
        writer.write_all(data)?;
        writer.flush()?;
        Ok(())
    }
    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), PtyError> {
        self.master
            .lock()
            .resize(PtySize {
                cols,
                rows,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| PtyError::Pty(e.to_string()))?;
        Ok(())
    }
    pub fn kill(&self) {
        let _ = self.child.lock().kill();
    }
    pub fn is_alive(&self) -> bool {
        self.child
            .lock()
            .try_wait()
            .map(|s| s.is_none())
            .unwrap_or(false)
    }
}

#[derive(Default)]
pub struct PtyManager {
    sessions: Mutex<HashMap<String, Arc<PtySession>>>,
}

impl PtyManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn list(&self) -> Vec<String> {
        self.sessions.lock().keys().cloned().collect()
    }

    pub fn get(&self, name: &str) -> Option<Arc<PtySession>> {
        self.sessions.lock().get(name).cloned()
    }

    pub fn has(&self, name: &str) -> bool {
        self.sessions.lock().contains_key(name)
    }

    pub fn remove(&self, name: &str) -> Option<Arc<PtySession>> {
        self.sessions.lock().remove(name)
    }

    pub fn attached_count(&self, name: &str) -> usize {
        self.sessions
            .lock()
            .get(name)
            .map(|s| s.tx.receiver_count())
            .unwrap_or(0)
    }

    pub fn spawn(&self, spec: PtySpawnSpec) -> Result<Arc<PtySession>, PtyError> {
        let PtySpawnSpec {
            name,
            cwd,
            shell,
            command,
            env,
            cols,
            rows,
        } = spec;
        if self.sessions.lock().contains_key(&name) {
            return Err(PtyError::Pty(format!("session already exists: {name}")));
        }
        let pty_system = NativePtySystem::default();
        let pair = pty_system
            .openpty(PtySize {
                cols,
                rows,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| PtyError::Pty(e.to_string()))?;

        let mut cmd = build_command(&shell, &command, &cwd, &env)?;
        cmd.cwd(&cwd);
        for (k, v) in &env {
            cmd.env(k, v);
        }
        if !env.iter().any(|(k, _)| k == "TERM") {
            cmd.env("TERM", "xterm-256color");
        }

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| PtyError::Pty(e.to_string()))?;
        drop(pair.slave);

        let pid = child.process_id();
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| PtyError::Pty(e.to_string()))?;

        let (tx, _rx) = broadcast::channel::<Vec<u8>>(BROADCAST_CAP);
        let ring = Arc::new(Mutex::new(RingBuffer::new(RING_BUFFER_BYTES)));

        let tx_clone = tx.clone();
        let ring_clone = ring.clone();
        let name_clone = name.clone();
        let reader_task = tokio::task::spawn_blocking(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let chunk = buf[..n].to_vec();
                        ring_clone.lock().push(&chunk);
                        let _ = tx_clone.send(chunk);
                    }
                    Err(err) => {
                        tracing::debug!(target: "pty", session = %name_clone, ?err, "pty reader ended");
                        break;
                    }
                }
            }
        });

        let session = Arc::new(PtySession {
            name: name.clone(),
            master: Arc::new(Mutex::new(pair.master)),
            child: Arc::new(Mutex::new(child)),
            ring,
            tx,
            pid,
            _reader_task: reader_task,
        });
        self.sessions.lock().insert(name, session.clone());
        Ok(session)
    }
}

fn build_command(
    shell: &str,
    command: &str,
    cwd: &Path,
    _env: &[(String, String)],
) -> Result<CommandBuilder, PtyError> {
    let mut cmd = if command.is_empty() {
        // Interactive login shell only.
        let mut c = CommandBuilder::new(shell);
        c.arg("-l");
        c
    } else {
        // Run via `<shell> -lc <command>` so PATH/.bashrc are sourced
        // (matching the old tmux behaviour).
        let mut c = CommandBuilder::new(shell);
        c.arg("-lc");
        c.arg(command);
        c
    };
    cmd.cwd(cwd);
    Ok(cmd)
}
