//! `agent-start-host update` — re-runs the official installer to upgrade the
//! binary in place. If a user-level service (systemd-user / launchd) is
//! already registered, it is re-registered and restarted so the new binary
//! takes effect without a manual restart.

use anyhow::{anyhow, Context, Result};
use clap::Args;
use std::path::PathBuf;
use std::process::{Command, Stdio};

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
    println!("    current:   {}", env!("CARGO_PKG_VERSION"));
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

    // Spawn the downloader and bash as two separate processes and pipe the
    // installer script over stdin. This avoids interpolating `url` into a
    // shell string, which would be unsafe if --url / env override ever
    // contained shell metacharacters.
    let mut dl_cmd = Command::new(downloader.exe);
    dl_cmd
        .args(downloader.args)
        .arg(&url)
        .stdout(Stdio::piped());
    let mut dl_child = dl_cmd
        .spawn()
        .with_context(|| format!("failed to spawn {}", downloader.exe))?;
    let dl_stdout = dl_child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("failed to capture {} stdout", downloader.exe))?;

    let mut bash_cmd = Command::new("bash");
    bash_cmd.arg("-s").stdin(Stdio::from(dl_stdout));
    if let Some(v) = args.version {
        bash_cmd.env("AGENT_START_VERSION", v);
    }
    if service_mode {
        bash_cmd.env("AGENT_START_SERVICE", "1");
    }

    let bash_status = bash_cmd
        .status()
        .context("failed to spawn bash to run installer")?;
    let dl_status = dl_child
        .wait()
        .with_context(|| format!("failed to wait for {}", downloader.exe))?;

    if !dl_status.success() {
        return Err(anyhow!(
            "downloader {} exited with status {} — your existing install was not changed",
            downloader.exe,
            dl_status
        ));
    }
    if !bash_status.success() {
        return Err(anyhow!(
            "installer exited with status {} — re-run with --no-service or fix the reported error and try again",
            bash_status
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

struct Downloader {
    exe: &'static str,
    args: &'static [&'static str],
}

fn pick_downloader() -> Option<Downloader> {
    if which("curl") {
        Some(Downloader {
            exe: "curl",
            args: &["-fsSL"],
        })
    } else if which("wget") {
        Some(Downloader {
            exe: "wget",
            args: &["-qO-"],
        })
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
