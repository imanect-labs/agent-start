//! Phase 2 acceptance test: an agent runs on a node, and what it wrote
//! comes back as a pushed branch.
//!
//! This is the half of the task queue that crosses the machine boundary.
//! The queue itself (claim, lease, retry) is exercised against the
//! database in `state`; what cannot be checked there is that a control
//! plane can ask a *node* to turn a worktree into a commit and a push,
//! because the files only exist on that node. So this drives a real
//! `NodeRuntime` over a loopback link: assign a session that writes a
//! file, wait for it to exit, then finalize and look in the bare
//! repository that stands in for `origin`.

use cluster_control::{ControlOptions, ControlPlane, Demand};
use cluster_node::{NodeConfig, NodeRuntime};
use cluster_proto::{
    AssignSpec, ControlLink, FinalizeSpec, IsolationProfile, ProjectRef, Resources,
};
use pty_manager::PtyManager;
use std::sync::Arc;
use std::time::Duration;

async fn control_plane(home: &std::path::Path) -> Arc<ControlPlane> {
    std::env::set_var("AGENT_START_HOME", home);
    let db = state::open_at(&home.join("host.db"))
        .await
        .expect("open db");
    ControlPlane::new(
        db,
        ControlOptions {
            assign_timeout: Duration::from_secs(60),
            finalize_timeout: Duration::from_secs(60),
            ..Default::default()
        },
    )
}

fn attach_node(control: &Arc<ControlPlane>, name: &str) -> Arc<PtyManager> {
    let pty = Arc::new(PtyManager::new());
    let (mut control_link, node_link) = cluster_proto::loopback();
    control_link = ControlLink {
        local: true,
        ..control_link
    };
    let runtime = NodeRuntime::new(
        NodeConfig {
            name: name.to_string(),
            max_sessions: 4,
            labels: Vec::new(),
            executor: "process".into(),
            identity_path: None,
        },
        pty.clone(),
    );
    tokio::spawn(control.clone().accept(control_link));
    tokio::spawn(async move {
        let _ = runtime.run(node_link, String::new(), None).await;
    });
    pty
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

/// A working clone with a bare `origin` behind it, so a push has
/// somewhere real to land.
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

async fn await_node(control: &Arc<ControlPlane>) {
    for _ in 0..200 {
        if !control.nodes().is_empty() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("the node never registered");
}

/// Wait until the session's PTY is gone, i.e. the agent has exited.
async fn await_exit(pty: &Arc<PtyManager>, session: &str) {
    for _ in 0..400 {
        if pty.windows_for(session).is_empty() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("the agent never exited");
}

#[tokio::test]
async fn a_finished_agents_work_comes_back_as_a_pushed_branch() {
    let home = tempfile::tempdir().expect("home");
    let control = control_plane(home.path()).await;
    let pty = attach_node(&control, "finalize-node");
    await_node(&control).await;

    let repo_root = tempfile::tempdir().expect("repo");
    let (work, clone_url) = seed_repo(repo_root.path());
    let project_id = "task-finalize-0001";
    let session = "cc-finalize-0001";

    // Stand-in for an agent: writes a file into its worktree and exits,
    // which is exactly the shape a headless `claude -p '…'` run has.
    let spec = AssignSpec {
        session: session.to_string(),
        cli: "shell".into(),
        command: "printf 'agent was here\\n' > AGENT.md".into(),
        shell: "/bin/bash".into(),
        project: ProjectRef {
            id: project_id.into(),
            name: "demo".into(),
            local_path: work.to_string_lossy().into_owned(),
            clone_url: Some(clone_url),
        },
        // A task always works on its own branch.
        create_worktree: true,
        mark_claude_trusted: false,
        requests: Resources {
            cpu_millis: 500,
            mem_mb: 256,
        },
        isolation: IsolationProfile::Process,
        env: Vec::new(),
    };
    let demand = Demand {
        requests: Resources {
            cpu_millis: 500,
            mem_mb: 256,
        },
        isolation: IsolationProfile::Process,
        label_selector: Vec::new(),
        project_id: project_id.into(),
        pinned_node: None,
        local_only: false,
    };

    let placed = control
        .start_session(spec, demand)
        .await
        .expect("session placed");
    await_exit(&pty, session).await;

    let report = control
        .finalize_session(
            &placed.node_id,
            session,
            FinalizeSpec {
                commit_message: "task: write AGENT.md".into(),
                push: true,
                // `gh` is not part of this test's contract — the branch
                // reaching origin is.
                open_pr: false,
                pr_title: String::new(),
                pr_body: String::new(),
                draft: false,
                base_branch: String::new(),
            },
        )
        .await
        .expect("finalize succeeded");

    assert!(report.committed, "the agent's file was not committed");
    assert!(report.pushed, "the branch never reached origin");
    // `create_worktree` names the branch after the session, under the
    // tool's own namespace.
    assert!(
        report.branch.ends_with(session),
        "pushed the wrong branch: {}",
        report.branch
    );

    // The proof is in the bare repository: the branch exists there, and
    // the file the agent wrote is in it.
    let bare = repo_root.path().join("origin.git");
    let listed = std::process::Command::new("git")
        .current_dir(&bare)
        .args(["show", &format!("{}:AGENT.md", report.branch)])
        .output()
        .expect("git show");
    assert!(
        listed.status.success(),
        "the pushed branch has no AGENT.md: {}",
        String::from_utf8_lossy(&listed.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&listed.stdout).trim(),
        "agent was here"
    );
}

#[tokio::test]
async fn finalizing_a_session_no_node_knows_about_is_an_error() {
    let home = tempfile::tempdir().expect("home");
    let control = control_plane(home.path()).await;
    attach_node(&control, "empty-node");
    await_node(&control).await;
    let node_id = control.nodes()[0].info.id.clone();

    let err = control
        .finalize_session(
            &node_id,
            "cc-never-existed",
            FinalizeSpec {
                commit_message: "nothing".into(),
                push: false,
                open_pr: false,
                pr_title: String::new(),
                pr_body: String::new(),
                draft: false,
                base_branch: String::new(),
            },
        )
        .await
        .expect_err("finalized a session that was never assigned");
    assert!(
        err.contains("not known to this node"),
        "unhelpful error: {err}"
    );
}
