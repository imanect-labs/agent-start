//! One PTY-backed window: a single `portable_pty` master/child pair
//! plus its ring buffer, broadcast tx, and writer.

use crate::error::PtyError;
use crate::ring::RingBuffer;
use parking_lot::Mutex;
use portable_pty::{MasterPty, PtySize};
use std::io::Write;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

pub struct PtySession {
    pub(crate) name: String,
    pub(crate) window: u32,
    pub(crate) master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    /// Long-lived PTY writer.
    ///
    /// `MasterPty::take_writer()` may only be called once for the
    /// lifetime of the master, and dropping the returned `Write` sends
    /// EOF to the slave. We therefore take it exactly once at spawn
    /// time and hold it for the life of the session.
    pub(crate) writer: Mutex<Box<dyn Write + Send>>,
    pub(crate) child: Arc<Mutex<Box<dyn portable_pty::Child + Send + Sync>>>,
    pub(crate) ring: Arc<Mutex<RingBuffer>>,
    pub(crate) tx: broadcast::Sender<Vec<u8>>,
    pub(crate) pid: Option<u32>,
    pub(crate) _reader_task: JoinHandle<()>,
}

impl PtySession {
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn window(&self) -> u32 {
        self.window
    }
    pub fn subscribe(&self) -> (Vec<u8>, broadcast::Receiver<Vec<u8>>) {
        let snap = self.ring.lock().snapshot();
        (snap, self.tx.subscribe())
    }
    pub fn write(&self, data: &[u8]) -> Result<(), PtyError> {
        let mut writer = self.writer.lock();
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
    pub fn attached_count(&self) -> usize {
        self.tx.receiver_count()
    }
}
