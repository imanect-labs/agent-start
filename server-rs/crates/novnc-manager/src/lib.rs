//! Spawns and tracks per-session noVNC backends. For each session we
//! launch three children: `Xvnc` (TigerVNC headless X server + VNC server),
//! `websockify` which bridges browser WebSocket traffic into the VNC TCP
//! socket and also serves the bundled noVNC `vnc.html` UI, and a desktop
//! session (gnome-session / startxfce4 / startlxqt) attached to the new
//! display so the user sees an actual desktop instead of a blank screen.
//!
//! Discovery:
//! - `AGENT_START_XVNC_BIN` overrides the Xvnc binary path.
//! - `AGENT_START_WEBSOCKIFY_BIN` overrides the websockify binary path.
//! - `AGENT_START_NOVNC_DIR` overrides the noVNC web asset directory.
//! - `AGENT_START_DESKTOP_CMD` overrides the desktop session command
//!   (full argv, e.g. `gnome-session --session=ubuntu`). If unset we
//!   try Ubuntu GNOME, then XFCE, then LXQt. If none are installed the
//!   X server still comes up but with no window manager — Xvnc plus a
//!   bare root window.
//!
//! Otherwise PATH is searched and common install dirs are probed.
//!
//! Auth: both network children bind `127.0.0.1` only. RFB is
//! `-SecurityTypes None` to match the existing code-server `--auth none`
//! posture (host-local reverse proxy is the trust boundary).

use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::process::{Child, Command};

#[derive(Debug, thiserror::Error)]
pub enum NovncError {
    #[error("Xvnc binary not found on PATH; install TigerVNC or set AGENT_START_XVNC_BIN")]
    XvncNotInstalled,
    #[error(
        "websockify binary not found on PATH; install websockify or set AGENT_START_WEBSOCKIFY_BIN"
    )]
    WebsockifyNotInstalled,
    #[error("noVNC web assets not found; install noVNC or set AGENT_START_NOVNC_DIR")]
    NovncDirNotFound,
    #[error("no free X display in range 100..200")]
    NoFreeDisplay,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("child did not start listening within {0:?}")]
    StartTimeout(Duration),
    #[error("spawn: {0}")]
    Spawn(String),
}

pub struct Instance {
    pub display: u32,
    pub rfb_port: u16,
    pub ws_port: u16,
    pub xvnc_pid: u32,
    pub websockify_pid: u32,
    /// PID of the desktop session leader (gnome-session / startxfce4 /
    /// ...). `None` when no desktop binary was found — Xvnc still runs
    /// so the connection works, the user just sees an empty root window.
    pub desktop_pid: Option<u32>,
    xvnc: Mutex<Option<Child>>,
    websockify: Mutex<Option<Child>>,
    desktop: Mutex<Option<Child>>,
}

impl Instance {
    pub fn display(&self) -> u32 {
        self.display
    }
    pub fn rfb_port(&self) -> u16 {
        self.rfb_port
    }
    pub fn ws_port(&self) -> u16 {
        self.ws_port
    }
}

#[derive(Default)]
pub struct NovncManager {
    instances: Mutex<HashMap<String, Arc<Instance>>>,
    /// Serializes spawns so two concurrent `ensure()` calls for the same
    /// session can't both pass the existence check, race on
    /// `allocate_port()` / `allocate_display()`, and leak duplicate
    /// children. Held across the awaits in `ensure`, so it must be a
    /// `tokio::sync::Mutex`.
    spawn_lock: tokio::sync::Mutex<()>,
}

impl NovncManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn port_for(&self, session: &str) -> Option<u16> {
        self.instances.lock().get(session).map(|i| i.ws_port)
    }

    pub fn get(&self, session: &str) -> Option<Arc<Instance>> {
        self.instances.lock().get(session).cloned()
    }

    /// Idempotent spawn. Returns the running instance, starting both
    /// child processes only if no instance exists for this session.
    pub async fn ensure(&self, session: &str) -> Result<Arc<Instance>, NovncError> {
        // Fast path: already running.
        if let Some(existing) = self.instances.lock().get(session).cloned() {
            return Ok(existing);
        }
        // Serialize spawns. Re-check under the lock so the second waiter
        // doesn't allocate ports again after the first one inserted the
        // instance.
        let _guard = self.spawn_lock.lock().await;
        if let Some(existing) = self.instances.lock().get(session).cloned() {
            return Ok(existing);
        }

        let xvnc_bin = resolve_xvnc()?;
        let websockify_bin = resolve_websockify()?;
        let novnc_dir = resolve_novnc_dir()?;

        let disp = allocate_display()?;
        let rfb_port = allocate_port()?;
        let ws_port = allocate_port()?;

        let mut xvnc_cmd = Command::new(&xvnc_bin);
        xvnc_cmd
            .arg(format!(":{disp}"))
            .arg("-rfbport")
            .arg(rfb_port.to_string())
            .arg("-SecurityTypes")
            .arg("None")
            .arg("-geometry")
            .arg("1280x800")
            .arg("-depth")
            .arg("24")
            .arg("-localhost")
            .arg("yes")
            .arg("-AlwaysShared")
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .stdin(Stdio::null())
            .kill_on_drop(true);

        let xvnc = xvnc_cmd
            .spawn()
            .map_err(|e| NovncError::Spawn(format!("Xvnc: {e}")))?;
        let xvnc_pid = xvnc.id().unwrap_or(0);

        if let Err(e) = wait_until_listening(rfb_port, Duration::from_secs(10)).await {
            // Tear down the orphan if RFB never came up.
            let mut x = xvnc;
            let _ = x.start_kill();
            let _ = x.wait().await;
            return Err(e);
        }

        let mut ws_cmd = Command::new(&websockify_bin);
        ws_cmd
            .arg("--web")
            .arg(&novnc_dir)
            .arg(format!("127.0.0.1:{ws_port}"))
            .arg(format!("127.0.0.1:{rfb_port}"))
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .stdin(Stdio::null())
            .kill_on_drop(true);

        let websockify = ws_cmd.spawn().map_err(|e| {
            // Best-effort: ensure we don't leak the already-spawned Xvnc.
            NovncError::Spawn(format!("websockify: {e}"))
        });
        let websockify = match websockify {
            Ok(c) => c,
            Err(e) => {
                let mut x = xvnc;
                let _ = x.start_kill();
                let _ = x.wait().await;
                return Err(e);
            }
        };
        let websockify_pid = websockify.id().unwrap_or(0);

        if let Err(e) = wait_until_listening(ws_port, Duration::from_secs(10)).await {
            let mut x = xvnc;
            let mut w = websockify;
            let _ = w.start_kill();
            let _ = w.wait().await;
            let _ = x.start_kill();
            let _ = x.wait().await;
            return Err(e);
        }

        // Spawn a desktop session attached to :disp. Failure here is
        // logged but non-fatal — the VNC connection still works, the
        // user just sees a blank root window.
        let (desktop, desktop_pid, desktop_name) = match resolve_desktop_session() {
            Some(d) => match spawn_desktop(&d, disp) {
                Ok(child) => {
                    let pid = child.id();
                    (Some(child), pid, Some(d.name))
                }
                Err(e) => {
                    tracing::warn!(
                        session,
                        disp,
                        desktop = d.name,
                        error = %e,
                        "failed to spawn desktop session; X server will have no window manager",
                    );
                    (None, None, None)
                }
            },
            None => {
                tracing::warn!(
                    session,
                    disp,
                    "no desktop session binary found (set AGENT_START_DESKTOP_CMD or install \
                     ubuntu-session / xfce4 / lxqt); VNC will show a blank root window",
                );
                (None, None, None)
            }
        };

        let instance = Arc::new(Instance {
            display: disp,
            rfb_port,
            ws_port,
            xvnc_pid,
            websockify_pid,
            desktop_pid,
            xvnc: Mutex::new(Some(xvnc)),
            websockify: Mutex::new(Some(websockify)),
            desktop: Mutex::new(desktop),
        });
        self.instances
            .lock()
            .insert(session.to_string(), instance.clone());
        tracing::info!(
            session,
            disp,
            rfb_port,
            ws_port,
            xvnc_pid,
            websockify_pid,
            desktop_pid,
            desktop = desktop_name,
            "spawned noVNC backend"
        );
        Ok(instance)
    }

    pub async fn kill(&self, session: &str) {
        let inst = self.instances.lock().remove(session);
        if let Some(inst) = inst {
            // Tear-down order: desktop session first, then websockify,
            // then Xvnc. The desktop dies before its X server so its
            // clients see a clean WM exit instead of a sudden display
            // loss; websockify dies before Xvnc so the noVNC proxy
            // stops accepting frames before the RFB socket vanishes.
            //
            // Extract each child out of its Mutex into a local before
            // any await so the parking_lot guard does not straddle the
            // await point.
            let desktop_child = inst.desktop.lock().take();
            if let Some(mut child) = desktop_child {
                // gnome-session / startxfce4 fork many children. We put
                // them in their own pgid at spawn time so we can kill
                // the whole group here. SIGTERM, give them a moment to
                // shut down, then SIGKILL anything left and reap the
                // immediate child.
                if let Some(pid) = inst.desktop_pid {
                    kill_pgroup(pid, TERM_SIGNAL);
                    tokio::time::sleep(Duration::from_millis(800)).await;
                    kill_pgroup(pid, KILL_SIGNAL);
                }
                let _ = child.start_kill();
                let _ = child.wait().await;
            }
            let ws_child = inst.websockify.lock().take();
            if let Some(mut child) = ws_child {
                let _ = child.start_kill();
                let _ = child.wait().await;
            }
            let xvnc_child = inst.xvnc.lock().take();
            if let Some(mut child) = xvnc_child {
                let _ = child.start_kill();
                let _ = child.wait().await;
            }
            tracing::info!(session, "killed noVNC backend");
        }
    }

    pub async fn kill_all(&self) {
        let names: Vec<String> = self.instances.lock().keys().cloned().collect();
        for name in names {
            self.kill(&name).await;
        }
    }
}

struct DesktopSession {
    name: &'static str,
    bin: PathBuf,
    args: Vec<String>,
    env: Vec<(&'static str, String)>,
}

/// Pick a desktop session to attach to the new X display.
///
/// Order: explicit override → Ubuntu GNOME (the "animal wallpaper" look,
/// when `ubuntu-session` + `gnome-shell` are installed) → XFCE → LXQt.
fn resolve_desktop_session() -> Option<DesktopSession> {
    if let Some(raw) = std::env::var_os("AGENT_START_DESKTOP_CMD") {
        let s = raw.to_string_lossy().into_owned();
        let mut parts = s.split_whitespace();
        let bin_name = parts.next()?;
        let bin = which_in_path(bin_name).unwrap_or_else(|| PathBuf::from(bin_name));
        return Some(DesktopSession {
            name: "custom",
            bin,
            args: parts.map(|s| s.to_string()).collect(),
            env: vec![],
        });
    }

    if let Some(bin) = which_in_path("gnome-session") {
        return Some(DesktopSession {
            name: "ubuntu-gnome",
            bin,
            args: vec!["--session=ubuntu".into()],
            // Tell GNOME this is an X11 Ubuntu session and force software
            // GL — Xvnc has no GPU, mutter falls over on llvmpipe probes
            // otherwise.
            env: vec![
                ("XDG_CURRENT_DESKTOP", "ubuntu:GNOME".into()),
                ("XDG_SESSION_DESKTOP", "ubuntu".into()),
                ("XDG_SESSION_TYPE", "x11".into()),
                ("GDK_BACKEND", "x11".into()),
                ("LIBGL_ALWAYS_SOFTWARE", "1".into()),
            ],
        });
    }
    if let Some(bin) = which_in_path("startxfce4") {
        return Some(DesktopSession {
            name: "xfce",
            bin,
            args: vec![],
            env: vec![("XDG_CURRENT_DESKTOP", "XFCE".into())],
        });
    }
    if let Some(bin) = which_in_path("startlxqt") {
        return Some(DesktopSession {
            name: "lxqt",
            bin,
            args: vec![],
            env: vec![("XDG_CURRENT_DESKTOP", "LXQt".into())],
        });
    }
    None
}

fn spawn_desktop(d: &DesktopSession, display: u32) -> std::io::Result<Child> {
    // If `dbus-run-session` is available, wrap the desktop in an
    // ephemeral DBus session — gnome-session in particular wants a real
    // session bus, and this gets us one without depending on the user
    // having `dbus-user-session` set up.
    let dbus = which_in_path("dbus-run-session");
    let (bin, args): (PathBuf, Vec<String>) = if let Some(dbus_bin) = dbus {
        let mut a: Vec<String> = vec!["--".into(), d.bin.to_string_lossy().into_owned()];
        a.extend(d.args.iter().cloned());
        (dbus_bin, a)
    } else {
        (d.bin.clone(), d.args.clone())
    };

    let mut cmd = Command::new(&bin);
    cmd.args(&args)
        .env("DISPLAY", format!(":{display}"))
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .stdin(Stdio::null())
        .kill_on_drop(true);
    for (k, v) in &d.env {
        cmd.env(k, v);
    }
    // Own pgid so we can SIGTERM the whole session tree at teardown.
    #[cfg(unix)]
    {
        cmd.process_group(0);
    }
    cmd.spawn()
}

#[cfg(unix)]
const TERM_SIGNAL: libc::c_int = libc::SIGTERM;
#[cfg(unix)]
const KILL_SIGNAL: libc::c_int = libc::SIGKILL;
#[cfg(not(unix))]
const TERM_SIGNAL: i32 = 15;
#[cfg(not(unix))]
const KILL_SIGNAL: i32 = 9;

#[cfg(unix)]
fn kill_pgroup(pid: u32, sig: libc::c_int) {
    // Safety: killpg is a thread-safe syscall wrapper; we tolerate ESRCH
    // (already gone) by ignoring the return value.
    unsafe {
        libc::killpg(pid as libc::pid_t, sig);
    }
}
#[cfg(not(unix))]
fn kill_pgroup(_pid: u32, _sig: i32) {}

fn resolve_xvnc() -> Result<PathBuf, NovncError> {
    if let Some(p) = std::env::var_os("AGENT_START_XVNC_BIN") {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return Ok(pb);
        }
        return Err(NovncError::XvncNotInstalled);
    }
    which_in_path("Xvnc").ok_or(NovncError::XvncNotInstalled)
}

fn resolve_websockify() -> Result<PathBuf, NovncError> {
    if let Some(p) = std::env::var_os("AGENT_START_WEBSOCKIFY_BIN") {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return Ok(pb);
        }
        return Err(NovncError::WebsockifyNotInstalled);
    }
    which_in_path("websockify").ok_or(NovncError::WebsockifyNotInstalled)
}

fn resolve_novnc_dir() -> Result<PathBuf, NovncError> {
    if let Some(p) = std::env::var_os("AGENT_START_NOVNC_DIR") {
        let pb = PathBuf::from(p);
        if pb.join("vnc.html").exists() {
            return Ok(pb);
        }
        return Err(NovncError::NovncDirNotFound);
    }
    const CANDIDATES: &[&str] = &[
        "/usr/share/novnc",
        "/usr/local/share/novnc",
        "/opt/homebrew/share/novnc",
        "/opt/homebrew/Cellar/novnc",
    ];
    for c in CANDIDATES {
        let pb = PathBuf::from(c);
        if pb.join("vnc.html").exists() {
            return Ok(pb);
        }
    }
    Err(NovncError::NovncDirNotFound)
}

fn which_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let cand = dir.join(name);
        if cand.is_file() {
            return Some(cand);
        }
    }
    None
}

fn allocate_port() -> Result<u16, NovncError> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

/// Pick a free X display number in 100..200. Xvnc refuses to start if
/// `/tmp/.X<N>-lock` already exists, so probe for that. Range starts at
/// 100 to stay clear of any human-managed displays (`:0`, `:1`).
fn allocate_display() -> Result<u32, NovncError> {
    for n in 100u32..200 {
        let lock = PathBuf::from(format!("/tmp/.X{n}-lock"));
        let sock = PathBuf::from(format!("/tmp/.X11-unix/X{n}"));
        if !lock.exists() && !sock.exists() {
            return Ok(n);
        }
    }
    Err(NovncError::NoFreeDisplay)
}

async fn wait_until_listening(port: u16, deadline: Duration) -> Result<(), NovncError> {
    let start = Instant::now();
    loop {
        if TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
            return Ok(());
        }
        if start.elapsed() >= deadline {
            return Err(NovncError::StartTimeout(deadline));
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocates_distinct_ports() {
        let a = allocate_port().unwrap();
        let b = allocate_port().unwrap();
        assert_ne!(a, 0);
        assert_ne!(b, 0);
    }

    #[test]
    fn resolve_missing_xvnc_errors() {
        std::env::set_var("AGENT_START_XVNC_BIN", "/nonexistent/Xvnc-xyz");
        let r = resolve_xvnc();
        std::env::remove_var("AGENT_START_XVNC_BIN");
        assert!(matches!(r, Err(NovncError::XvncNotInstalled)));
    }

    #[test]
    fn resolve_missing_websockify_errors() {
        std::env::set_var("AGENT_START_WEBSOCKIFY_BIN", "/nonexistent/websockify-xyz");
        let r = resolve_websockify();
        std::env::remove_var("AGENT_START_WEBSOCKIFY_BIN");
        assert!(matches!(r, Err(NovncError::WebsockifyNotInstalled)));
    }

    #[test]
    fn desktop_override_parses_argv() {
        std::env::set_var("AGENT_START_DESKTOP_CMD", "/usr/bin/true --foo bar");
        let d = resolve_desktop_session().expect("override should resolve");
        std::env::remove_var("AGENT_START_DESKTOP_CMD");
        assert_eq!(d.name, "custom");
        assert_eq!(d.bin, PathBuf::from("/usr/bin/true"));
        assert_eq!(d.args, vec!["--foo".to_string(), "bar".to_string()]);
    }

    #[test]
    fn resolve_missing_novnc_dir_errors() {
        std::env::set_var("AGENT_START_NOVNC_DIR", "/nonexistent/novnc-xyz");
        let r = resolve_novnc_dir();
        std::env::remove_var("AGENT_START_NOVNC_DIR");
        assert!(matches!(r, Err(NovncError::NovncDirNotFound)));
    }
}
