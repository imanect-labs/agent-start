//! CLI tests for `agent-start`.
//!
//! The argument-parsing tests run fully offline. The end-to-end test spawns a
//! real `agent-start-host` and drives it over HTTP; it is gated on the host
//! binary having been built (it skips cleanly otherwise, e.g. when running
//! `cargo test -p agent-start` without building the host first).

use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

fn bin() -> Command {
    Command::cargo_bin("agent-start").expect("agent-start binary should be built")
}

#[test]
fn version_flag_prints_semver() {
    bin()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn version_subcommand_prints_name() {
    bin()
        .arg("version")
        .assert()
        .success()
        .stdout(predicate::str::starts_with("agent-start "));
}

#[test]
fn help_lists_core_subcommands() {
    bin()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("project").and(predicate::str::contains("session")));
}

#[test]
fn unknown_subcommand_fails() {
    bin().arg("definitely-not-a-command").assert().failure();
}

#[test]
fn project_add_requires_a_source() {
    // No --clone / --import: must error before touching the network.
    bin()
        .args(["--url", "http://127.0.0.1:1", "project", "add"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--clone").or(predicate::str::contains("--import")));
}

/// Resolve the path to the sibling `agent-start-host` binary in the same
/// target dir, if it has been built.
fn host_bin() -> Option<PathBuf> {
    let p = assert_cmd::cargo::cargo_bin("agent-start-host");
    p.exists().then_some(p)
}

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    listener.local_addr().unwrap().port()
}

#[test]
fn e2e_status_and_project_list_against_live_host() {
    let Some(host) = host_bin() else {
        eprintln!("skipping e2e: agent-start-host not built");
        return;
    };

    // Isolate all on-disk state (config, projects, runtime manifest) into a
    // temp HOME so the test never touches the developer's real ~/.agent-start.
    let tmp = std::env::temp_dir().join(format!("agent-start-e2e-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();

    let port = free_port();
    let url = format!("http://127.0.0.1:{port}");

    let mut child = Command::new(&host)
        .args(["--bind", "127.0.0.1", "--port", &port.to_string()])
        .env("HOME", &tmp)
        .env("AGENT_START_HOME", &tmp)
        .env("XDG_DATA_HOME", tmp.join("data"))
        .env("XDG_CONFIG_HOME", tmp.join("config"))
        .spawn()
        .expect("spawn agent-start-host");

    // Poll `status` until the host answers or we time out.
    let mut up = false;
    for _ in 0..50 {
        let ok = bin()
            .args(["--url", &url, "status"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if ok {
            up = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    let result = std::panic::catch_unwind(|| {
        assert!(up, "host did not become reachable at {url}");

        bin()
            .args(["--url", &url, "status", "--json"])
            .assert()
            .success()
            .stdout(predicate::str::contains("\"version\""));

        bin()
            .args(["--url", &url, "project", "list", "--json"])
            .assert()
            .success()
            .stdout(predicate::str::starts_with("{"));
    });

    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&tmp);

    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}
