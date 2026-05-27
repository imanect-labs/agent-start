//! `agent-start-host update` — re-runs the official installer to upgrade the
//! binary in place. If a user-level service (systemd-user / launchd) is
//! already registered, it is re-registered and restarted so the new binary
//! takes effect without a manual restart.

use anyhow::{anyhow, Context, Result};
use clap::Args;
use std::path::PathBuf;
use std::process::Command;

const DEFAULT_INSTALL_URL: &str = "https://agentstart.imanect.app/install.sh";

#[derive(Debug, Args)]
pub struct UpdateArgs {
    /// Pin to a specific release tag (e.g. v0.2.0). Defaults to latest.
    #[arg(long)]
    version: Option<String>,

    /// Override the installer URL (mostly for testing forks).
    #[arg(long)]
    url: Option<String>,

    /// Force service re-registration even if no existing unit is detected.
    #[arg(long)]
    service: bool,

    /// Skip service re-registration even if a unit is detected.
    #[arg(long, conflicts_with = "service")]
    no_service: bool,
}

pub fn run(args: UpdateArgs) -> Result<()> {
    let url = args
        .url
        .or_else(|| std::env::var("AGENT_START_INSTALL_URL").ok())
        .unwrap_or_else(|| DEFAULT_INSTALL_URL.to_string());

    let detected = detect_service();
    let service_mode = if args.no_service {
        false
    } else {
        args.service || detected.is_some()
    };

    println!("==> updating agent-start-host");
    println!("    installer: {}", url);
    println!(
        "    current:   {}",
        env!("CARGO_PKG_VERSION")
    );
    if let Some(ref v) = args.version {
        println!("    target:    {}", v);
    } else {
        println!("    target:    latest");
    }
    if service_mode {
        match &detected {
            Some(p) => println!("    service:   re-register (found {})", p.display()),
            None => println!("    service:   register (forced)"),
        }
    } else {
        println!("    service:   skip");
    }

    let downloader = pick_downloader().context("need either curl or wget on PATH")?;
    let cmd_line = format!("{downloader} {url} | bash");

    let mut shell = Command::new("bash");
    shell.arg("-c").arg(&cmd_line);
    if let Some(v) = args.version {
        shell.env("AGENT_START_VERSION", v);
    }
    if service_mode {
        shell.env("AGENT_START_SERVICE", "1");
    }

    let status = shell
        .status()
        .with_context(|| format!("failed to spawn shell to run installer ({cmd_line})"))?;

    if !status.success() {
        return Err(anyhow!(
            "installer exited with status {} — your existing install was not changed if the download failed before the install step",
            status
        ));
    }

    println!("==> update complete");
    if service_mode {
        println!("    service was restarted by the installer.");
    } else {
        println!("    restart any running agent-start-host process to pick up the new binary.");
    }
    Ok(())
}

fn pick_downloader() -> Option<&'static str> {
    if which("curl") {
        Some("curl -fsSL")
    } else if which("wget") {
        Some("wget -qO-")
    } else {
        None
    }
}

fn which(name: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {name} >/dev/null 2>&1"))
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Look for an installed user-level service unit. Returns the path if found
/// so we can surface it in the upgrade summary.
fn detect_service() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;

    let systemd = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"))
        .join("systemd/user/agent-start.service");
    if systemd.exists() {
        return Some(systemd);
    }

    let launchd = home.join("Library/LaunchAgents/app.agent-start.plist");
    if launchd.exists() {
        return Some(launchd);
    }

    None
}
