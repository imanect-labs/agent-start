# agent-start

[![Rust CI](https://github.com/imanect-labs/agent-start/actions/workflows/rust.yml/badge.svg)](https://github.com/imanect-labs/agent-start/actions/workflows/rust.yml)
[![Web CI](https://github.com/imanect-labs/agent-start/actions/workflows/web.yml/badge.svg)](https://github.com/imanect-labs/agent-start/actions/workflows/web.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)
[![Latest release](https://img.shields.io/github/v/release/imanect-labs/agent-start?include_prereleases&sort=semver)](https://github.com/imanect-labs/agent-start/releases)

> 日本語版: [README.ja.md](./README.ja.md)

A self-hosted web launcher that lets you start and manage **`claude` / `codex` CLI** sessions on a remote machine (e.g. your home Linux box) from any browser, including a phone over a tailnet.

<img width="1338" height="882" alt="agent-start-screenshot" src="https://github.com/user-attachments/assets/8edfb7a1-97d4-46d1-8882-93a6e3cd13fe" />

- Browse projects, tap one, and spawn `claude` or `codex` inside a persistent PTY session.
- List, preview, and stop running sessions.
- Optionally start each session inside a fresh **`git worktree`** so parallel agents do not stomp on each other.
- Clean up the worktree (and the branch it was on) when you stop the session.
- Configure CLI flags such as `--dangerously-skip-permissions` / `--full-auto` from the UI and persist them.
- **Closing the browser does not kill sessions** — the Rust host process keeps the PTYs alive.

> ⚠️ **Security model — read before exposing this service.**
> agent-start has **no built-in authentication** and is designed to run agents that execute arbitrary commands (including `--dangerously-skip-permissions`). It must only ever be reachable through a trusted network (a tailnet, VPN, or a LAN behind a firewall). **Never expose it to the public internet.** The default bind is `127.0.0.1`; you must opt into broader exposure via `--bind`. See [SECURITY.md](./SECURITY.md) for details and how to report vulnerabilities.

## Architecture

```
Browser ── tailnet ── agent-start-host (Rust, :3030)
                          │  /api/*, /v1/*, /ws/*  +  embedded SPA
                          │  axum + tokio + portable-pty + sqlx
                          ▼
                     bash -lc 'claude ...' / 'codex --full-auto' / ...
```

`agent-start-host` is a single binary that serves HTTP/WebSocket, multiplexes PTYs, and persists state to SQLite. The built front-end (`front/dist/`) is **embedded into the binary via `rust-embed`**, so production deployments ship a single executable with no static-asset directory to manage.

The front-end (`/front/`) is a Vite+ + React + TanStack Router SPA. In development it runs out-of-process via `vp dev` on `:5173` and proxies `/api/*`, `/v1/*`, `/ws/*` to the host on `:3030`.

## Installation

Pre-built binaries are published on the [Releases page](https://github.com/imanect-labs/agent-start/releases) for Linux (x86_64, aarch64), macOS (Apple Silicon, Intel), and Windows (x86_64). They are fully self-contained — the web UI is embedded into the binary.

### Linux / macOS (one-liner)

```bash
curl -fsSL https://agentstart.imanect.app/install.sh | bash
```

This fetches the latest release, picks the right target for your OS/arch, and drops `agent-start-host` into `~/.local/bin/`. Environment overrides:

- `AGENT_START_VERSION=v0.1.0` — pin a specific release instead of latest.
- `INSTALL_DIR=/usr/local/bin` — install somewhere else (you may need `sudo`).
- `AGENT_START_SERVICE=1` — also register a systemd-user unit (Linux) or launchd agent (macOS) so the host runs on boot. See [Run as a daemon](#run-as-a-daemon-systemd-user).
- `AGENT_START_BIND=0.0.0.0` / `AGENT_START_PORT=3030` — bind / port baked into the service unit (default `127.0.0.1` / `3030`).

One-liner that installs **and** daemonizes for tailnet access:

```bash
curl -fsSL https://agentstart.imanect.app/install.sh | \
  AGENT_START_SERVICE=1 AGENT_START_BIND=0.0.0.0 bash
```

After install:

```bash
agent-start-host --port 3030                  # localhost only (default 127.0.0.1)
# open http://localhost:3030

# Reach it from another machine over a tailnet / VPN / firewalled LAN:
agent-start-host --bind 0.0.0.0 --port 3030
# open http://<host-name-or-ip>:3030
```

> ⚠️ `--bind 0.0.0.0` exposes the host on every interface. Only use it behind tailscale / WireGuard / a LAN firewall. Read [SECURITY.md](./SECURITY.md) first — there is no built-in auth.

To keep the host running after you log out, see [Run as a daemon](#run-as-a-daemon-systemd-user) below.

### Manual download

Grab the archive that matches your platform from the [Releases page](https://github.com/imanect-labs/agent-start/releases), extract it, and put `agent-start-host` somewhere on your `PATH`:

| Platform        | Asset                                                   |
| --------------- | ------------------------------------------------------- |
| Linux x86_64    | `agent-start-<tag>-x86_64-unknown-linux-gnu.tar.gz`     |
| Linux aarch64   | `agent-start-<tag>-aarch64-unknown-linux-gnu.tar.gz`    |
| macOS arm64     | `agent-start-<tag>-aarch64-apple-darwin.tar.gz`         |
| macOS x86_64    | `agent-start-<tag>-x86_64-apple-darwin.tar.gz`          |
| Windows x86_64  | `agent-start-<tag>-x86_64-pc-windows-msvc.zip`          |

### Build from source

```bash
git clone https://github.com/imanect-labs/agent-start
cd agent-start
(cd front && vp install && vp build)              # builds the SPA into front/dist
(cd server-rs && cargo build --release)           # rust-embed pulls front/dist into the binary
./server-rs/target/release/agent-start-host --port 3030
```

You need the `vp` (Vite+) CLI for the front bundle — see [Toolchain](#toolchain) below. Rust toolchain version: see `rust-version` in `server-rs/Cargo.toml`.

> ⚠️ Read [SECURITY.md](./SECURITY.md) **before** binding to anything other than `127.0.0.1`. agent-start has no built-in authentication.

## Setup (development)

Two processes during development (Rust host + Vite+ SPA). One binary in production.

### Toolchain

Install the `vp` (Vite+) CLI once:

```bash
# macOS / Linux
curl -fsSL https://vite.plus | bash

# Windows (PowerShell)
# irm https://vite.plus/ps1 | iex

vp env    # verify the install
```

Vite+ bundles `vite` / `vitest` / `oxlint` / `oxfmt` / `tsgo` / a Node runtime, so you do **not** need to install Node or npm separately for the front-end side.

### Development mode (two terminals)

```bash
git clone https://github.com/imanect-labs/agent-start
cd agent-start

# Terminal A: Rust host (:3030)
npm run dev:host
# = cd server-rs && cargo run -p agent-start-host -- --port 3030

# Terminal B: Vite+ SPA (:5173, hot reload)
(cd front && vp install)   # first time only
npm run dev:front
# = cd front && vp dev
```

Open <http://localhost:5173>. Vite+'s proxy forwards `/api/*`, `/v1/*`, and `/ws/*` to the host on `:3030`.

### Production runtime notes

The release binary is fully self-contained — see [Installation](#installation) to grab one or build from source. Open `http://<server>:3030` from your phone (via tailnet, VPN, or LAN).

If you want to override the embedded SPA at runtime (e.g. staging a newer front bundle without rebuilding the host), pass `--frontend-dist <path>` or set `AGENT_START_FRONTEND_DIST`.

Config files and runtime data live under **`~/.agent-start/`** (legacy XDG paths are migrated automatically on first boot). Override the root with `AGENT_START_HOME`:

```
~/.agent-start/
├── config.json                  # CLI presets (generated on first boot)
├── preferences.json             # launch flags saved from the UI
├── host.db                      # sessions + pty_history (SQLite)
├── runtime/manifest.json        # URL / PID of the running host
├── projects/                    # cloned / imported projects (override with AGENT_START_PROJECTS)
└── worktrees/<session>/         # per-session git worktrees (override with AGENT_START_WORKTREE_ROOT)
```

## CLI

```bash
cargo install --path server-rs/bin/agent-start
agent-start status                # host version
agent-start projects              # list projects via host
agent-start sessions              # list sessions via host
agent-start start --port 3030     # spawn host in the foreground
agent-start stop                  # SIGTERM via manifest.json
```

Across a tailnet, pass `--url http://server:3030`.

## Configuration (UI)

The gear icon (top-right of the left sidebar) opens `/settings`. Every field is persisted to `~/.agent-start/config.json` and `preferences.json`; the page warns if you try to leave with unsaved changes.

Highlights:
- **Project directories** (formerly `roots`): where projects are discovered. Defaults to `~/.agent-start/projects`. One path per line, multiple allowed.
- **Launch defaults**: default CLI, skip-permissions flag, worktree creation, extra flags.
- **Sessions**: session prefix, shell, show hidden files, git-only filter.

The bottom-left **"Add project"** button lets you clone a git repo or import a local directory (both land in `~/.agent-start/projects/`). A spinner shows on the project row while it is being cloned/copied. Right-click a project to delete it (with confirmation).

## Configuration file

`~/.agent-start/config.json` schema:

```json
{
  "roots": ["/home/user/.agent-start/projects"],
  "sessionPrefix": "cc-",
  "shell": "/bin/bash",
  "showHidden": false,
  "gitOnly": false,
  "defaultCli": "claude",
  "clis": {
    "claude": {
      "command": "claude",
      "skipPermissionsFlag": "--dangerously-skip-permissions",
      "label": "Claude Code"
    },
    "codex": {
      "command": "codex",
      "skipPermissionsFlag": "--full-auto",
      "label": "Codex CLI"
    }
  }
}
```

Add a new CLI (e.g. `aider`, `opencode`) by appending another entry to `clis`. The legacy `claudeCommand` key is migrated automatically.

## Worktree mode

When you tick "create a git worktree" on the start sheet:

1. Run `git -C <project> worktree add -b agent-start/<session> ~/.cache/agent-start/worktrees/<session> HEAD`.
2. Start the PTY session with that worktree as `cwd`.

When you tick "delete the worktree too" on session stop:
- `git worktree remove --force` tears down the tree.
- The `agent-start/*` branch is deleted.

## GUI display (noVNC)

Adding a "GUI" tab to a session asks `agent-start-host` to launch a per-session
virtual X desktop (`Xvnc`) and a `websockify` proxy, then renders them via
noVNC in the browser. Launching a GUI app from the session terminal with
`DISPLAY=:<N> firefox &` makes it visible in the iframe (`<N>` is shown at
the top of the GUI tab).

> **Supported only on Linux / WSL.** macOS is not supported as a host
> for the GUI feature: the Homebrew `tiger-vnc` formula ships only the
> viewer, not the `Xvnc` server (which needs X.Org), and macOS GUI
> apps run on Quartz rather than X11, so a virtual X desktop cannot
> host them anyway. Run `agent-start-host` on a Linux box (bare metal,
> WSL, or a Linux Docker container) when you need this tab.

Install the dependencies on the Linux host:

```bash
# Debian/Ubuntu
sudo apt install tigervnc-standalone-server novnc websockify

# Fedora / RHEL
sudo dnf install tigervnc-server novnc python3-websockify

# Arch
sudo pacman -S tigervnc novnc python-websockify
```

Override binary locations if autodetection fails:

- `AGENT_START_XVNC_BIN=/path/to/Xvnc`
- `AGENT_START_WEBSOCKIFY_BIN=/path/to/websockify`
- `AGENT_START_NOVNC_DIR=/path/to/novnc`  (directory containing `vnc.html`)

Security: both `Xvnc` and `websockify` bind only `127.0.0.1`, so they are
reachable through the same tailnet boundary as the host itself (mirroring
the code-server posture).

## Run as a daemon (systemd user)

The fastest path is to let the installer do it for you:

```bash
curl -fsSL https://agentstart.imanect.app/install.sh | AGENT_START_SERVICE=1 bash
# add AGENT_START_BIND=0.0.0.0 if you want to reach it from another machine
```

That writes the unit file, runs `daemon-reload`, and starts the service. Then run **once** to keep the service alive past logout / across reboots:

```bash
sudo loginctl enable-linger "$USER"
```

If you'd rather configure it manually, the steps below are what the installer does.

### Manual setup

1. Write the unit file (adjust `ExecStart` if you installed somewhere other than `~/.local/bin/`):

```ini
# ~/.config/systemd/user/agent-start.service
[Unit]
Description=agent-start host
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=%h/.local/bin/agent-start-host --bind 0.0.0.0 --port 3030
Restart=on-failure
RestartSec=3
# Optional: pin runtime data dir / extra env
# Environment=AGENT_START_HOME=%h/.agent-start

[Install]
WantedBy=default.target
```

2. Enable lingering so the service starts at boot and survives logout (without this the user manager exits when you log out and the host dies with it):

```bash
sudo loginctl enable-linger "$USER"
```

3. Reload, enable, and start:

```bash
systemctl --user daemon-reload
systemctl --user enable --now agent-start
```

4. Useful operations:

```bash
systemctl --user status agent-start          # current state
journalctl --user -u agent-start -f          # tail logs
systemctl --user restart agent-start         # after upgrading the binary
systemctl --user stop agent-start            # stop the host (PTYs survive — see below)
```

Upgrading the binary in-place (e.g. re-running the install one-liner) does not interrupt running sessions — restart the service afterwards to pick up the new version.

If the host crashes or is restarted, running `cc-*` sessions keep their PIDs and scrollback in SQLite, so reconnecting from the UI just works. Child processes reparent to PID 1 — running under systemd-user lets it reap them cleanly.

### macOS (launchd)

On macOS the equivalent is a launchd user agent at `~/Library/LaunchAgents/app.agent-start.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>app.agent-start</string>
  <key>ProgramArguments</key>
  <array>
    <string>/Users/YOU/.local/bin/agent-start-host</string>
    <string>--bind</string><string>0.0.0.0</string>
    <string>--port</string><string>3030</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardOutPath</key><string>/tmp/agent-start.out.log</string>
  <key>StandardErrorPath</key><string>/tmp/agent-start.err.log</string>
</dict>
</plist>
```

Then:

```bash
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/app.agent-start.plist
launchctl kickstart -k gui/$(id -u)/app.agent-start
```

## Contributing & community

- [CONTRIBUTING.md](./CONTRIBUTING.md) — build, test, and PR workflow.
- [CODE_OF_CONDUCT.md](./CODE_OF_CONDUCT.md) — Contributor Covenant v2.1.
- [SECURITY.md](./SECURITY.md) — security model and how to disclose vulnerabilities.
- [CHANGELOG.md](./CHANGELOG.md) — release notes.

## License

MIT. See [LICENSE](./LICENSE).
