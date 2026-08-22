//! Phase 1 acceptance test: two nodes, automatic placement, and a
//! terminal that works no matter which node won.
//!
//! Both nodes are real `NodeRuntime`s over loopback links — the same
//! objects `--role all` and `--role node` run — so this covers
//! registration, scheduling, PTY spawn and the relay end to end. Only
//! the transport is shortened: swapping in a WebSocket adds a JSON
//! round-trip and nothing else, and `cluster-proto` tests that
//! separately.

use cluster_control::{ControlOptions, ControlPlane, Demand, StreamMsg};
use cluster_node::{NodeConfig, NodeRuntime};
use cluster_proto::{AssignSpec, ControlLink, IsolationProfile, ProjectRef, Resources};
use pty_manager::PtyManager;
use std::sync::Arc;
use std::time::Duration;

/// `AGENT_START_HOME` is process-global and these tests run in
/// parallel, so they share one home *and* one connection pool: opening
/// the same SQLite file from several tests at once races the migration
/// runner. Tests stay independent through distinct node names and
/// project ids instead.
static HOME: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
static DB: tokio::sync::OnceCell<state::Db> = tokio::sync::OnceCell::const_new();

/// Spin up a control plane against the shared throwaway database.
async fn control_plane() -> Arc<ControlPlane> {
    let db = DB
        .get_or_init(|| async {
            HOME.get_or_init(|| {
                let dir = tempfile::tempdir().expect("temp home");
                std::env::set_var("AGENT_START_HOME", dir.path());
                dir
            });
            state::open().await.expect("open db")
        })
        .await
        .clone();
    ControlPlane::new(
        db,
        ControlOptions {
            assign_timeout: Duration::from_secs(30),
            ..Default::default()
        },
    )
}

/// Attach a node with its own PTY manager. `local` mirrors the real
/// distinction: the in-process node shares the control plane's
/// filesystem, a remote one does not.
struct Attached {
    pty: Arc<PtyManager>,
    /// Aborting this drops the node's end of the link, which is what a
    /// crashed or unplugged machine looks like to the control plane.
    node_task: tokio::task::JoinHandle<()>,
}

fn attach_node(
    control: &Arc<ControlPlane>,
    name: &str,
    max_sessions: u32,
    local: bool,
) -> Arc<PtyManager> {
    attach(control, name, max_sessions, local).pty
}

fn attach(control: &Arc<ControlPlane>, name: &str, max_sessions: u32, local: bool) -> Attached {
    let pty = Arc::new(PtyManager::new());
    let (mut control_link, node_link) = cluster_proto::loopback();
    control_link = ControlLink {
        local,
        ..control_link
    };
    let runtime = NodeRuntime::new(
        NodeConfig {
            name: name.to_string(),
            max_sessions,
            labels: Vec::new(),
            executor: "process".into(),
            identity_path: None,
        },
        pty.clone(),
    );
    tokio::spawn(control.clone().accept(control_link));
    let node_task = tokio::spawn(async move {
        let _ = runtime.run(node_link, String::new(), None).await;
    });
    Attached { pty, node_task }
}

/// Build a project a node can actually obtain: a working clone for the
/// local node, and a bare repository standing in for `origin` so a
/// remote node has something real to mirror.
fn seed_repo(root: &std::path::Path) -> (std::path::PathBuf, String) {
    let bare = root.join("origin.git");
    let work = root.join("work");
    git(
        root,
        &["init", "--bare", "-b", "main", bare.to_str().unwrap()],
    );
    git(root, &["init", "-b", "main", work.to_str().unwrap()]);
    git(&work, &["config", "user.email", "test@example.invalid"]);
    git(&work, &["config", "user.name", "test"]);
    std::fs::write(work.join("README.md"), "demo\n").unwrap();
    git(&work, &["add", "-A"]);
    git(&work, &["commit", "-qm", "init"]);
    git(&work, &["remote", "add", "origin", bare.to_str().unwrap()]);
    git(&work, &["push", "-q", "origin", "HEAD:main"]);
    (work, bare.to_string_lossy().into_owned())
}

fn git(cwd: &std::path::Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("run git");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn spec(
    session: &str,
    project_id: &str,
    project_dir: &std::path::Path,
    clone_url: &str,
) -> AssignSpec {
    AssignSpec {
        session: session.to_string(),
        cli: "shell".into(),
        // Long enough to still be running when we assert; the PTY echoes
        // whatever we type at it regardless of what the child does.
        command: "sleep 30".into(),
        shell: "/bin/bash".into(),
        project: ProjectRef {
            id: project_id.to_string(),
            name: "demo".into(),
            local_path: project_dir.to_string_lossy().into_owned(),
            // Present, so the scheduler is free to use any node — and
            // real, because a node that is not local can only get the
            // source by cloning it.
            clone_url: Some(clone_url.to_string()),
        },
        create_worktree: false,
        mark_claude_trusted: false,
        requests: Resources {
            cpu_millis: 500,
            mem_mb: 256,
        },
        isolation: IsolationProfile::Process,
        env: Vec::new(),
    }
}

fn demand(project_id: &str) -> Demand {
    Demand {
        requests: Resources {
            cpu_millis: 500,
            mem_mb: 256,
        },
        isolation: IsolationProfile::Process,
        label_selector: Vec::new(),
        project_id: project_id.to_string(),
        pinned_node: None,
        local_only: false,
    }
}

/// Cancel every session and wait until the PTYs are actually gone from
/// the nodes.
///
/// Checking the control plane's own bookkeeping would prove nothing:
/// `cancel_session` drops the reservation immediately and only then
/// tells the node. The nodes' `PtyManager`s are the ground truth, and
/// waiting on them also stops a live child from holding the test's
/// blocking reader threads open at shutdown.
async fn drain(control: &Arc<ControlPlane>, nodes: &[Arc<PtyManager>], sessions: &[String]) {
    for name in sessions {
        control.cancel_session(name, false).await;
    }
    for _ in 0..400 {
        let alive: Vec<&String> = sessions
            .iter()
            .filter(|s| nodes.iter().any(|p| !p.windows_for(s).is_empty()))
            .collect();
        if alive.is_empty() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("sessions survived cancel: {sessions:?}");
}

/// Wait for both nodes to finish registering. Registration is a frame
/// round-trip, so it is fast but not synchronous with `attach_node`.
async fn await_nodes(control: &Arc<ControlPlane>, want: usize) {
    for _ in 0..100 {
        if control.nodes().len() >= want {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("nodes never registered: {:?}", control.nodes().len());
}

#[tokio::test]
async fn sessions_spread_across_nodes_and_stay_reachable() {
    let project = tempfile::tempdir().unwrap();
    let (work, origin) = seed_repo(project.path());
    let pid = "spread-project";
    let control = control_plane().await;

    let ptys = vec![
        attach_node(&control, "spread-a", 4, true),
        attach_node(&control, "spread-b", 4, false),
    ];
    await_nodes(&control, 2).await;

    // Four sessions onto two equal, idle nodes. Nothing in the load
    // signal separates them at this timescale — the warm-up penalty on a
    // just-used node is what stops all four piling onto one.
    let mut placed = Vec::new();
    for i in 0..4 {
        let name = format!("cc-demo-{i}");
        let outcome = control
            .start_session(spec(&name, pid, &work, &origin), demand(pid))
            .await
            .unwrap_or_else(|e| panic!("session {i} was not placed: {e}"));
        placed.push((name, outcome));
    }

    let on_a = placed
        .iter()
        .filter(|(_, o)| o.node_name == "spread-a")
        .count();
    let on_b = placed.len() - on_a;
    assert!(
        on_a > 0 && on_b > 0,
        "sessions did not spread: {on_a} on spread-a, {on_b} on spread-b"
    );

    // Every node's own view agrees with the placements it was given.
    for view in control.nodes() {
        let expected = placed
            .iter()
            .filter(|(_, o)| o.node_name == view.info.name)
            .count();
        assert_eq!(
            view.running.len(),
            expected,
            "{} reports {:?}",
            view.info.name,
            view.running
        );
        assert_eq!(view.reserved.cpu_millis, expected as u32 * 500);
    }

    // A session on the non-local node must be reachable through the
    // relay, byte for byte: this is the terminal path a browser takes.
    let (remote_session, _) = placed
        .iter()
        .find(|(_, o)| !o.is_local)
        .expect("at least one session should be on the remote node");

    let mut stream = control
        .open_pty_stream(remote_session, 0)
        .await
        .expect("relay channel");
    let mut rx = stream.take_rx().expect("stream receiver");

    stream.write(b"marker-42\n".to_vec()).await;

    // The PTY echoes typed input, so we should see our marker come back
    // through the node, the link and the relay.
    let echoed = tokio::time::timeout(Duration::from_secs(10), async {
        let mut seen = Vec::new();
        while let Some(msg) = rx.recv().await {
            if let StreamMsg::Data(chunk) = msg {
                seen.extend_from_slice(&chunk);
                if String::from_utf8_lossy(&seen).contains("marker-42") {
                    return true;
                }
            }
        }
        false
    })
    .await
    .expect("relay timed out");
    assert!(echoed, "marker never came back through the relay");

    let names: Vec<String> = placed.iter().map(|(n, _)| n.clone()).collect();
    drain(&control, &ptys, &names).await;
}

#[tokio::test]
async fn a_full_node_pushes_work_to_its_neighbour() {
    let project = tempfile::tempdir().unwrap();
    let (work, origin) = seed_repo(project.path());
    let pid = "cap-project";
    let control = control_plane().await;

    // The first node can host exactly one session; everything else must
    // land on its neighbour even though the two are otherwise identical.
    let ptys = vec![
        attach_node(&control, "cap-a", 1, true),
        attach_node(&control, "cap-b", 8, false),
    ];
    await_nodes(&control, 2).await;

    let mut names = Vec::new();
    for i in 0..3 {
        let name = format!("cc-cap-{i}");
        let outcome = control
            .start_session(spec(&name, pid, &work, &origin), demand(pid))
            .await
            .unwrap_or_else(|e| panic!("session {i} was not placed: {e}"));
        names.push(outcome.node_name);
    }

    assert_eq!(
        names.iter().filter(|n| *n == "cap-a").count(),
        1,
        "cap-a exceeded its session cap: {names:?}"
    );
    assert_eq!(names.iter().filter(|n| *n == "cap-b").count(), 2);

    let names: Vec<String> = (0..3).map(|i| format!("cc-cap-{i}")).collect();
    drain(&control, &ptys, &names).await;
}

#[tokio::test]
async fn a_project_without_an_origin_stays_on_the_local_node() {
    let project = tempfile::tempdir().unwrap();
    let (work, origin) = seed_repo(project.path());
    let pid = "pinned-project";
    let control = control_plane().await;

    let ptys = vec![
        attach_node(&control, "pinned-a", 4, true),
        attach_node(&control, "pinned-b", 4, false),
    ];
    await_nodes(&control, 2).await;

    let mut spec = spec("cc-nolocal-0", pid, &work, &origin);
    spec.project.clone_url = None;
    let mut demand = demand(pid);
    demand.local_only = true;

    // Repeated, because the point is that it never wanders: a node that
    // cannot see the files must not win even when it scores better.
    for _ in 0..3 {
        let mut s = spec.clone();
        s.session = format!("cc-nolocal-{}", uuid_like());
        let session = s.session.clone();
        let outcome = control.start_session(s, demand.clone()).await.unwrap();
        assert_eq!(outcome.node_name, "pinned-a");
        assert!(outcome.is_local);
        drain(&control, &ptys, std::slice::from_ref(&session)).await;
    }
}

/// Two sessions for the same cold project, started at the same moment
/// on the same node. Both have to clone the mirror, and without a
/// per-project lock the second `git clone --mirror` finds the first
/// one's half-written directory, deletes it, and breaks both.
#[tokio::test]
async fn concurrent_sessions_share_one_cold_mirror() {
    let project = tempfile::tempdir().unwrap();
    let (work, origin) = seed_repo(project.path());
    let pid = "cold-mirror-project";
    let control = control_plane().await;

    // One node only, and not local, so both sessions must go through
    // the mirror rather than reading the project in place.
    let ptys = vec![attach_node(&control, "cold-a", 4, false)];
    await_nodes(&control, 1).await;

    let names = ["cc-cold-0".to_string(), "cc-cold-1".to_string()];
    let (first, second) = tokio::join!(
        control.start_session(spec(&names[0], pid, &work, &origin), demand(pid)),
        control.start_session(spec(&names[1], pid, &work, &origin), demand(pid)),
    );
    first.expect("first session");
    second.expect("second session");

    drain(&control, &ptys, &names).await;
}

/// When a node drops off, its sessions must fail loudly rather than
/// hang. The frame channel outlives the connection, so a relay opened
/// against a dead node would otherwise accept the request and then
/// deliver nothing — the worst possible failure for a terminal.
#[tokio::test]
async fn a_terminal_on_a_vanished_node_is_refused_not_left_hanging() {
    let project = tempfile::tempdir().unwrap();
    let (work, origin) = seed_repo(project.path());
    let pid = "vanish-project";
    let control = control_plane().await;

    let node = attach(&control, "vanish-a", 4, false);
    await_nodes(&control, 1).await;

    let name = "cc-vanish-0".to_string();
    control
        .start_session(spec(&name, pid, &work, &origin), demand(pid))
        .await
        .expect("session placed");

    // While the node is healthy the relay opens.
    assert!(control.open_pty_stream(&name, 0).await.is_some());

    // Pull the plug.
    node.node_task.abort();
    for _ in 0..200 {
        if control.nodes().iter().all(|v| !v.connected) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(
        control.nodes().iter().all(|v| !v.connected),
        "control plane never noticed the node had gone"
    );

    assert!(
        control.open_pty_stream(&name, 0).await.is_none(),
        "relay opened against a node that is no longer connected"
    );

    for w in node.pty.remove_session(&name) {
        w.kill();
    }
}

/// Cheap unique suffix; the crate has no rng dependency and the test
/// only needs distinct session names.
fn uuid_like() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    N.fetch_add(1, Ordering::Relaxed).to_string()
}
