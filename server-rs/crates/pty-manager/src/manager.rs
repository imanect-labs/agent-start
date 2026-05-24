//! `PtyManager` — the multiplexer keyed by `(session_name, window)`.
//!
//! Each agent-start session has at least one window (`window = 0`),
//! the one started by `POST /api/sessions`. Additional windows are
//! created by `POST /api/sessions/<name>/windows`; the manager
//! allocates the next free index per session.

use crate::error::PtyError;
use crate::ring::RingBuffer;
use crate::session::PtySession;
use parking_lot::Mutex;
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Weak};
use tokio::sync::broadcast;

/// Maximum bytes retained in the per-window ring buffer.
pub const RING_BUFFER_BYTES: usize = 512 * 1024;

/// Capacity of the broadcast channel (number of chunks, each up to 8 KiB).
const BROADCAST_CAP: usize = 256;

pub struct PtySpawnSpec {
    pub name: String,
    pub window: u32,
    pub cwd: std::path::PathBuf,
    pub shell: String,
    pub command: String,
    pub env: Vec<(String, String)>,
    pub cols: u16,
    pub rows: u16,
}

/// Fired exactly once when a child process exits — by user `exit`,
/// signal, or `kill()`. The host uses it to mark the session/window
/// dead in SQLite and drop the in-memory directory entry.
pub type ExitHook = Arc<dyn Fn(&str, u32) + Send + Sync>;

#[derive(Default)]
pub struct PtyManager {
    sessions: Mutex<HashMap<(String, u32), Arc<PtySession>>>,
    on_exit: Mutex<Option<ExitHook>>,
}

impl PtyManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_exit_hook(&self, hook: ExitHook) {
        *self.on_exit.lock() = Some(hook);
    }

    pub fn get(&self, name: &str, window: u32) -> Option<Arc<PtySession>> {
        self.sessions
            .lock()
            .get(&(name.to_string(), window))
            .cloned()
    }

    /// All window indices for the given session, sorted ascending.
    pub fn windows_for(&self, name: &str) -> Vec<u32> {
        let map = self.sessions.lock();
        let mut out: Vec<u32> = map
            .keys()
            .filter(|(n, _)| n == name)
            .map(|(_, w)| *w)
            .collect();
        out.sort_unstable();
        out
    }

    pub fn next_window_index(&self, name: &str) -> u32 {
        self.windows_for(name)
            .last()
            .copied()
            .map(|n| n + 1)
            .unwrap_or(0)
    }

    pub fn has_window(&self, name: &str, window: u32) -> bool {
        self.sessions
            .lock()
            .contains_key(&(name.to_string(), window))
    }

    /// Drop and return a single window's PTY (does not kill).
    pub fn remove_window(&self, name: &str, window: u32) -> Option<Arc<PtySession>> {
        self.sessions.lock().remove(&(name.to_string(), window))
    }

    /// Drop and return every window for a session (does not kill them
    /// — the caller is expected to call `kill()` on each).
    pub fn remove_session(&self, name: &str) -> Vec<Arc<PtySession>> {
        let mut map = self.sessions.lock();
        let keys: Vec<(String, u32)> = map.keys().filter(|(n, _)| n == name).cloned().collect();
        keys.into_iter().filter_map(|k| map.remove(&k)).collect()
    }

    /// Snapshot every live (session, window)'s ring buffer. Used by the
    /// host's periodic flusher to persist scrollback to SQLite so it
    /// can be replayed after a restart.
    pub fn snapshot_all(&self) -> Vec<(String, u32, Vec<u8>)> {
        let map = self.sessions.lock();
        map.iter()
            .map(|((n, w), s)| (n.clone(), *w, s.subscribe().0))
            .collect()
    }

    /// Sum of WebSocket receivers across every window of the session.
    pub fn attached_count(&self, name: &str) -> usize {
        let map = self.sessions.lock();
        map.iter()
            .filter(|((n, _), _)| n == name)
            .map(|(_, s)| s.attached_count())
            .sum()
    }

    pub fn spawn(self: &Arc<Self>, spec: PtySpawnSpec) -> Result<Arc<PtySession>, PtyError> {
        let PtySpawnSpec {
            name,
            window,
            cwd,
            shell,
            command,
            env,
            cols,
            rows,
        } = spec;
        let key = (name.clone(), window);
        if self.sessions.lock().contains_key(&key) {
            return Err(PtyError::Pty(format!(
                "session already exists: {name}#{window}"
            )));
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

        let mut cmd = build_command(&shell, &command, &cwd);
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
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| PtyError::Pty(e.to_string()))?;

        let (tx, _rx) = broadcast::channel::<Vec<u8>>(BROADCAST_CAP);
        let ring = Arc::new(Mutex::new(RingBuffer::new(RING_BUFFER_BYTES)));

        let tx_clone = tx.clone();
        let ring_clone = ring.clone();
        let manager_weak: Weak<PtyManager> = Arc::downgrade(self);
        let child_arc = Arc::new(Mutex::new(child));
        let child_for_exit = child_arc.clone();
        let exit_key = key.clone();
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
                        tracing::debug!(
                            target: "pty",
                            session = %exit_key.0,
                            window = exit_key.1,
                            ?err,
                            "pty reader ended"
                        );
                        break;
                    }
                }
            }
            // Reader EOF / error means the slave has been closed —
            // either the child exited or we were killed. Reap to
            // surface its status and trigger cleanup.
            let _ = child_for_exit.lock().wait();
            if let Some(mgr) = manager_weak.upgrade() {
                mgr.sessions.lock().remove(&exit_key);
                let hook = mgr.on_exit.lock().clone();
                if let Some(hook) = hook {
                    hook(&exit_key.0, exit_key.1);
                }
            }
        });

        let session = Arc::new(PtySession {
            name: name.clone(),
            window,
            master: Arc::new(Mutex::new(pair.master)),
            writer: Mutex::new(writer),
            child: child_arc,
            ring,
            tx,
            pid,
            _reader_task: reader_task,
        });
        self.sessions.lock().insert(key, session.clone());
        Ok(session)
    }
}

fn build_command(shell: &str, command: &str, cwd: &Path) -> CommandBuilder {
    let mut cmd = if command.is_empty() {
        // Interactive login shell only.
        let mut c = CommandBuilder::new(shell);
        c.arg("-l");
        c
    } else {
        // Run via `<shell> -lc <command>` so PATH/.bashrc are sourced.
        let mut c = CommandBuilder::new(shell);
        c.arg("-lc");
        c.arg(command);
        c
    };
    cmd.cwd(cwd);
    cmd
}
