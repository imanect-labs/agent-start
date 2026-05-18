//! Fixed-size scrollback buffer kept in memory per PTY. Older bytes are
//! dropped from the front as new bytes arrive. The host can flush
//! periodically to SQLite (`state::append_history`); we deliberately
//! do not hold the entire session output in memory.

pub(crate) struct RingBuffer {
    buf: Vec<u8>,
    cap: usize,
}

impl RingBuffer {
    pub(crate) fn new(cap: usize) -> Self {
        Self {
            buf: Vec::with_capacity(cap.min(64 * 1024)),
            cap,
        }
    }
    pub(crate) fn push(&mut self, data: &[u8]) {
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
    pub(crate) fn snapshot(&self) -> Vec<u8> {
        self.buf.clone()
    }
}
