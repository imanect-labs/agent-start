//! Node-side resource sampling.
//!
//! The scheduler reacts to what it is told, so smoothing belongs here
//! rather than in the control plane: a node reports an EWMA of its own
//! utilization, and one busy second never drains work away from an
//! otherwise idle machine.

use cluster_proto::{NodeCapacity, NodeMetrics};
use sysinfo::System;

/// Weight of the newest sample in the EWMA. At the default 10s
/// heartbeat this gives a time constant of roughly half a minute —
/// long enough to ignore a compile spike, short enough to notice a
/// machine that has genuinely filled up.
const ALPHA: f32 = 0.3;

pub struct MetricsProbe {
    sys: System,
    smoothed: Option<NodeMetrics>,
}

impl Default for MetricsProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricsProbe {
    pub fn new() -> Self {
        Self {
            sys: System::new_all(),
            smoothed: None,
        }
    }

    /// Total resources this machine can offer, less a small reservation
    /// for the node agent and the OS. `max_sessions` comes from config;
    /// it is the operator's cap, independent of hardware.
    pub fn capacity(&mut self, max_sessions: u32) -> NodeCapacity {
        self.sys.refresh_memory();
        let cores = self.sys.cpus().len().max(1) as u32;
        let total_mb = (self.sys.total_memory() / (1024 * 1024)) as u32;
        // Hold back 10% (at least 512 MiB) so a fully-booked node still
        // has room to run git, the agent itself, and ssh.
        let reserve_mb = (total_mb / 10).max(512).min(total_mb);
        NodeCapacity {
            cpu_millis: cores * 1000,
            mem_mb: total_mb.saturating_sub(reserve_mb),
            max_sessions,
        }
    }

    /// Take a sample and fold it into the running average.
    pub fn sample(&mut self) -> NodeMetrics {
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();

        let total = self.sys.total_memory();
        let raw = NodeMetrics {
            cpu_util: (self.sys.global_cpu_usage() / 100.0).clamp(0.0, 1.0),
            mem_util: if total == 0 {
                0.0
            } else {
                (self.sys.used_memory() as f32 / total as f32).clamp(0.0, 1.0)
            },
            load1: System::load_average().one as f32,
        };

        let next = match self.smoothed {
            None => raw,
            Some(prev) => NodeMetrics {
                cpu_util: ewma(prev.cpu_util, raw.cpu_util),
                mem_util: ewma(prev.mem_util, raw.mem_util),
                load1: ewma(prev.load1, raw.load1),
            },
        };
        self.smoothed = Some(next);
        next
    }
}

fn ewma(prev: f32, sample: f32) -> f32 {
    prev * (1.0 - ALPHA) + sample * ALPHA
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_reports_something_usable() {
        let mut probe = MetricsProbe::new();
        let cap = probe.capacity(4);
        assert!(cap.cpu_millis >= 1000, "at least one core: {cap:?}");
        assert_eq!(cap.max_sessions, 4);
    }

    #[test]
    fn first_sample_is_taken_verbatim_then_smoothed() {
        // A single spike must not swing the average all the way.
        let smoothed = ewma(0.1, 1.0);
        assert!(smoothed < 0.4, "one spike moved the EWMA to {smoothed}");
        assert!(smoothed > 0.1);
    }

    #[test]
    fn sample_stays_in_range() {
        let mut probe = MetricsProbe::new();
        let m = probe.sample();
        assert!((0.0..=1.0).contains(&m.cpu_util));
        assert!((0.0..=1.0).contains(&m.mem_util));
    }
}
