//! Placement: which node should run this session?
//!
//! Two stages, in the shape kube-scheduler made standard. **Filters**
//! are hard constraints — a node that fails one cannot run the work at
//! all. **Scoring** ranks whatever survives, so the answer degrades
//! gracefully as a cluster fills up instead of failing.
//!
//! The weights below encode one opinion worth stating out loud: cache
//! affinity matters. Nodes keep a bare mirror per project, so the first
//! session for a project on a cold node pays a full clone. Pulling
//! repeat work toward a warm node is worth roughly as much as a fifth
//! of the load signal — enough to matter, not enough to pile everything
//! onto one machine while its neighbours idle.

use cluster_proto::{IsolationProfile, Resources};

/// Weights sum to 1.0; see `docs/multinode-cloud-design.ja.md` §2.3.
///
/// There is deliberately no label term. Labels are a *filter*: `admit`
/// drops every node that does not match the whole selector, so each
/// survivor matches equally and any label score would add the same
/// constant to all of them — a weight that reads as meaningful in the
/// table and changes no ranking at all.
const W_CPU_RESERVED: f32 = 0.35;
const W_CPU_OBSERVED: f32 = 0.25;
const W_MEM_OBSERVED: f32 = 0.20;
const W_CACHE_HIT: f32 = 0.20;

/// How long a freshly placed session suppresses a node's observed-load
/// score. Utilization takes a few seconds to reflect a new agent, and
/// without this every request in a burst sees the same idle reading and
/// piles onto the same node.
pub const WARMUP_PENALTY_SECS: u64 = 20;
/// Size of that penalty, in units of the observed-load score.
const WARMUP_PENALTY: f32 = 0.5;

/// What a session needs from a node.
#[derive(Debug, Clone)]
pub struct Demand {
    pub requests: Resources,
    pub isolation: IsolationProfile,
    /// Every pair must match a node label for it to be eligible.
    pub label_selector: Vec<(String, String)>,
    /// Mirror-cache key; a node already holding it scores higher.
    pub project_id: String,
    /// Explicit node id from the caller. Scoring is skipped, but the
    /// hard filters still apply — pinning to a node that cannot run the
    /// work should fail loudly, not quietly place it elsewhere.
    pub pinned_node: Option<String>,
    /// The project exists only on the control plane's filesystem (no
    /// origin remote), so only the in-process node can reach it.
    pub local_only: bool,
}

/// The scheduler's view of one node. Deliberately a plain snapshot: the
/// caller takes the locks, this module does arithmetic.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub id: String,
    pub name: String,
    pub is_local: bool,
    pub ready: bool,
    pub cordoned: bool,
    pub max_sessions: u32,
    pub running: u32,
    pub capacity: Resources,
    pub reserved: Resources,
    pub cpu_util: f32,
    pub mem_util: f32,
    pub profiles: Vec<IsolationProfile>,
    pub labels: Vec<(String, String)>,
    pub has_repo_cache: bool,
    /// Seconds since this node was last given work, if ever.
    pub secs_since_assign: Option<u64>,
}

/// Why nothing could be placed. Carried back to the user verbatim —
/// "no node available" with no reason is the worst possible answer when
/// a cluster is misconfigured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoFit {
    NoNodes,
    AllBusy,
    NoIsolation(IsolationProfile),
    NoLabels,
    TooLarge,
    LocalOnly,
    UnknownNode(String),
}

impl std::fmt::Display for NoFit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoNodes => write!(f, "no nodes are connected"),
            Self::AllBusy => write!(f, "every node is at its session limit or cordoned"),
            Self::NoIsolation(p) => write!(
                f,
                "no node provides `{}` isolation",
                p.as_str()
            ),
            Self::NoLabels => write!(f, "no node matches the requested labels"),
            Self::TooLarge => write!(
                f,
                "no node has enough unreserved cpu/memory for this request"
            ),
            Self::LocalOnly => write!(
                f,
                "this project has no origin remote, so it can only run on the local node — and the local node is unavailable"
            ),
            Self::UnknownNode(id) => {
                write!(f, "node `{id}` is not connected or cannot run this session")
            }
        }
    }
}

/// Pick the best node, or explain why none fits.
///
/// Reports the *most specific* reason: if candidates were eliminated by
/// several filters, the narrowest one is what the operator needs to
/// hear, so we track eliminations in filter order.
pub fn select<'a>(candidates: &'a [Candidate], demand: &Demand) -> Result<&'a Candidate, NoFit> {
    if candidates.is_empty() {
        return Err(NoFit::NoNodes);
    }

    let ready: Vec<&Candidate> = candidates.iter().filter(|c| c.ready).collect();
    if ready.is_empty() {
        return Err(NoFit::NoNodes);
    }

    if let Some(pinned) = &demand.pinned_node {
        let only: Vec<&Candidate> = ready.iter().copied().filter(|c| &c.id == pinned).collect();
        if only.is_empty() {
            return Err(NoFit::UnknownNode(pinned.clone()));
        }
        let fits = admit(&only, demand)?;
        return best(&fits, demand).ok_or_else(|| NoFit::UnknownNode(pinned.clone()));
    }

    if demand.local_only {
        let local: Vec<&Candidate> = ready.iter().copied().filter(|c| c.is_local).collect();
        if local.is_empty() {
            return Err(NoFit::LocalOnly);
        }
        return best(&admit(&local, demand)?, demand).ok_or(NoFit::AllBusy);
    }

    let fits = admit(&ready, demand)?;
    best(&fits, demand).ok_or(NoFit::AllBusy)
}

/// Apply the hard filters, preserving the reason the last one removed
/// everything.
fn admit<'a>(nodes: &[&'a Candidate], demand: &Demand) -> Result<Vec<&'a Candidate>, NoFit> {
    let by_isolation: Vec<&Candidate> = nodes
        .iter()
        .copied()
        .filter(|c| c.profiles.iter().any(|p| demand.isolation.satisfied_by(*p)))
        .collect();
    if by_isolation.is_empty() {
        return Err(NoFit::NoIsolation(demand.isolation));
    }

    let by_labels: Vec<&Candidate> = by_isolation
        .into_iter()
        .filter(|c| {
            demand
                .label_selector
                .iter()
                .all(|(k, v)| c.labels.iter().any(|(ck, cv)| ck == k && cv == v))
        })
        .collect();
    if by_labels.is_empty() {
        return Err(NoFit::NoLabels);
    }

    let by_size: Vec<&Candidate> = by_labels
        .into_iter()
        .filter(|c| {
            // Compare against total capacity, not what is free right
            // now: a request no node could *ever* satisfy is a
            // different problem from a cluster that is merely full.
            c.capacity.cpu_millis >= demand.requests.cpu_millis
                && c.capacity.mem_mb >= demand.requests.mem_mb
        })
        .collect();
    if by_size.is_empty() {
        return Err(NoFit::TooLarge);
    }

    Ok(by_size
        .into_iter()
        .filter(|c| has_room(c, demand))
        .collect())
}

fn has_room(c: &Candidate, demand: &Demand) -> bool {
    if c.cordoned {
        return false;
    }
    if c.max_sessions > 0 && c.running >= c.max_sessions {
        return false;
    }
    let free_cpu = c.capacity.cpu_millis.saturating_sub(c.reserved.cpu_millis);
    let free_mem = c.capacity.mem_mb.saturating_sub(c.reserved.mem_mb);
    free_cpu >= demand.requests.cpu_millis && free_mem >= demand.requests.mem_mb
}

fn best<'a>(fits: &[&'a Candidate], demand: &Demand) -> Option<&'a Candidate> {
    fits.iter()
        .copied()
        .map(|c| (score(c, demand), c))
        .max_by(|a, b| {
            a.0.partial_cmp(&b.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                // Ties break on name so placement is deterministic and
                // tests do not depend on hash order.
                .then_with(|| b.1.name.cmp(&a.1.name))
        })
        .map(|(_, c)| c)
}

pub fn score(c: &Candidate, _demand: &Demand) -> f32 {
    let cpu_reserved_ratio = if c.capacity.cpu_millis == 0 {
        1.0
    } else {
        (c.reserved.cpu_millis as f32 / c.capacity.cpu_millis as f32).clamp(0.0, 1.0)
    };

    let warm = match c.secs_since_assign {
        Some(s) if s < WARMUP_PENALTY_SECS => WARMUP_PENALTY,
        _ => 0.0,
    };
    let observed_cpu = (1.0 - c.cpu_util - warm).clamp(0.0, 1.0);

    W_CPU_RESERVED * (1.0 - cpu_reserved_ratio)
        + W_CPU_OBSERVED * observed_cpu
        + W_MEM_OBSERVED * (1.0 - c.mem_util).clamp(0.0, 1.0)
        + W_CACHE_HIT * if c.has_repo_cache { 1.0 } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(name: &str) -> Candidate {
        Candidate {
            id: name.into(),
            name: name.into(),
            is_local: false,
            ready: true,
            cordoned: false,
            max_sessions: 4,
            running: 0,
            capacity: Resources {
                cpu_millis: 8000,
                mem_mb: 16384,
            },
            reserved: Resources::ZERO,
            cpu_util: 0.0,
            mem_util: 0.0,
            profiles: vec![IsolationProfile::Process],
            labels: Vec::new(),
            has_repo_cache: false,
            secs_since_assign: None,
        }
    }

    fn demand() -> Demand {
        Demand {
            requests: Resources::default_request(),
            isolation: IsolationProfile::Process,
            label_selector: Vec::new(),
            project_id: "demo".into(),
            pinned_node: None,
            local_only: false,
        }
    }

    #[test]
    fn the_less_loaded_node_wins() {
        let mut busy = node("busy");
        busy.cpu_util = 0.9;
        busy.reserved = Resources {
            cpu_millis: 6000,
            mem_mb: 8192,
        };
        let idle = node("idle");
        let nodes = vec![busy, idle];
        assert_eq!(select(&nodes, &demand()).unwrap().name, "idle");
    }

    #[test]
    fn a_warm_repo_cache_breaks_a_tie() {
        let cold = node("cold");
        let mut warm = node("warm");
        warm.has_repo_cache = true;
        let nodes = vec![cold, warm];
        assert_eq!(select(&nodes, &demand()).unwrap().name, "warm");
    }

    #[test]
    fn cache_affinity_does_not_outweigh_a_full_node() {
        // A warm node that is nearly booked should still lose to a cold
        // idle one — otherwise every session for a project stacks up on
        // whichever machine happened to clone it first.
        let mut warm = node("warm");
        warm.has_repo_cache = true;
        warm.cpu_util = 0.95;
        warm.reserved = Resources {
            cpu_millis: 7000,
            mem_mb: 12288,
        };
        let cold = node("cold");
        let nodes = vec![warm, cold];
        assert_eq!(select(&nodes, &demand()).unwrap().name, "cold");
    }

    #[test]
    fn a_node_at_its_session_cap_is_skipped() {
        let mut full = node("full");
        full.running = 4;
        let free = node("free");
        let nodes = vec![full, free];
        assert_eq!(select(&nodes, &demand()).unwrap().name, "free");
    }

    #[test]
    fn cordoned_nodes_never_receive_work() {
        let mut only = node("only");
        only.cordoned = true;
        assert_eq!(select(&[only], &demand()).unwrap_err(), NoFit::AllBusy);
    }

    #[test]
    fn isolation_is_a_hard_filter_with_a_specific_error() {
        let mut d = demand();
        d.isolation = IsolationProfile::MicroVm;
        let err = select(&[node("plain")], &d).unwrap_err();
        assert_eq!(err, NoFit::NoIsolation(IsolationProfile::MicroVm));
        assert!(err.to_string().contains("microvm"));
    }

    #[test]
    fn labels_must_all_match() {
        let mut gpu = node("gpu");
        gpu.labels = vec![("gpu".into(), "true".into())];
        let plain = node("plain");
        let mut d = demand();
        d.label_selector = vec![("gpu".into(), "true".into())];
        assert_eq!(select(&[plain.clone(), gpu], &d).unwrap().name, "gpu");
        assert_eq!(select(&[plain], &d).unwrap_err(), NoFit::NoLabels);
    }

    #[test]
    fn an_oversized_request_is_distinguished_from_a_busy_cluster() {
        let mut d = demand();
        d.requests = Resources {
            cpu_millis: 64_000,
            mem_mb: 999_999,
        };
        assert_eq!(select(&[node("small")], &d).unwrap_err(), NoFit::TooLarge);
    }

    #[test]
    fn projects_without_an_origin_stay_on_the_local_node() {
        let mut d = demand();
        d.local_only = true;
        let remote = node("remote");
        let mut local = node("local");
        local.is_local = true;
        // Even though `remote` looks identical, it cannot see the files.
        assert_eq!(select(&[remote.clone(), local], &d).unwrap().name, "local");
        assert_eq!(select(&[remote], &d).unwrap_err(), NoFit::LocalOnly);
    }

    #[test]
    fn memory_pressure_moves_work_away() {
        let mut tight = node("tight");
        tight.mem_util = 0.95;
        let roomy = node("roomy");
        assert_eq!(select(&[tight, roomy], &demand()).unwrap().name, "roomy");
    }

    #[test]
    fn labels_filter_but_do_not_score() {
        // Both match the selector, so the selector must not tip the
        // scales — only load may.
        let mut busy = node("busy");
        busy.labels = vec![("gpu".into(), "true".into())];
        busy.cpu_util = 0.9;
        let mut idle = node("idle");
        idle.labels = vec![("gpu".into(), "true".into())];
        let mut d = demand();
        d.label_selector = vec![("gpu".into(), "true".into())];
        assert_eq!(select(&[busy, idle], &d).unwrap().name, "idle");
    }

    #[test]
    fn a_just_assigned_node_is_penalized_so_bursts_spread() {
        // Both look idle because utilization has not caught up yet;
        // only the warm-up penalty tells them apart.
        let mut just_used = node("just-used");
        just_used.secs_since_assign = Some(1);
        let untouched = node("untouched");
        let nodes = vec![just_used, untouched];
        assert_eq!(select(&nodes, &demand()).unwrap().name, "untouched");
    }

    #[test]
    fn pinning_overrides_scoring_but_not_the_filters() {
        let mut busy = node("busy");
        busy.cpu_util = 0.99;
        let idle = node("idle");
        let mut d = demand();
        d.pinned_node = Some("busy".into());
        assert_eq!(select(&[busy.clone(), idle], &d).unwrap().name, "busy");

        // …and a pinned node that cannot take the work fails loudly.
        busy.cordoned = true;
        assert_eq!(
            select(&[busy], &d).unwrap_err(),
            NoFit::UnknownNode("busy".into())
        );
    }

    #[test]
    fn disconnected_nodes_are_not_candidates() {
        let mut down = node("down");
        down.ready = false;
        assert_eq!(select(&[down], &demand()).unwrap_err(), NoFit::NoNodes);
    }
}
