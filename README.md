# agent-start

[![Rust CI](https://github.com/imanect-labs/agent-start/actions/workflows/rust.yml/badge.svg)](https://github.com/imanect-labs/agent-start/actions/workflows/rust.yml)
[![Web CI](https://github.com/imanect-labs/agent-start/actions/workflows/web.yml/badge.svg)](https://github.com/imanect-labs/agent-start/actions/workflows/web.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)
[![Latest release](https://img.shields.io/github/v/release/imanect-labs/agent-start?include_prereleases&sort=semver)](https://github.com/imanect-labs/agent-start/releases)

> 日本語版: [README.ja.md](./README.ja.md)

A self-hosted web launcher that lets you start and manage **`claude` / `codex` CLI** sessions on a remote machine (e.g. your home Linux box) from any browser, including a phone over a tailnet.

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

After install:

```bash
agent-start-host --port 3030      # binds 127.0.0.1 by default
# open http://localhost:3030
```

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

## Keeping the host running after SSH disconnect

Run the host as a systemd-user service:

```ini
# ~/.config/systemd/user/agent-start.service
[Unit]
Description=agent-start host

[Service]
ExecStart=%h/.cargo/bin/agent-start-host --bind 0.0.0.0 --port 3030
Restart=on-failure

[Install]
WantedBy=default.target
```

```bash
systemctl --user daemon-reload
systemctl --user enable --now agent-start
```

If the host crashes, running `cc-*` sessions keep their PIDs and scrollback in SQLite, so reconnecting from the UI just works. Child processes reparent to PID 1 — running under systemd-user lets it reap them cleanly.

## Contributing & community

- [CONTRIBUTING.md](./CONTRIBUTING.md) — build, test, and PR workflow.
- [CODE_OF_CONDUCT.md](./CODE_OF_CONDUCT.md) — Contributor Covenant v2.1.
- [SECURITY.md](./SECURITY.md) — security model and how to disclose vulnerabilities.
- [CHANGELOG.md](./CHANGELOG.md) — release notes.

## License

MIT. See [LICENSE](./LICENSE).
