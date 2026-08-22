# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **Task queue: submit a request, get a pull request.** `POST /api/tasks`
  takes a project and a sentence ("fix the login validation and add a
  test"), queues it, and the scheduler runs it as a headless agent on
  whichever node has room. When the agent exits cleanly the node that
  holds the worktree commits, pushes the branch and opens a PR with
  `gh`; the PR link lands on the task. A task that loses its node is
  requeued, one that has already pushed never is — a second run would
  open a second pull request for one request. New surfaces:
  `GET /api/tasks`, `GET /api/tasks/{id}`, `POST /api/tasks/{id}/cancel`,
  `POST /api/tasks/{id}/retry`, and a `/tasks` page with a "タスクを投げる"
  sheet sized for a phone.
- **One chat, many agents.** The launcher no longer has an entry per
  provider. A chat starts on the default agent and the picker at the
  bottom-left of the composer switches provider *and* model —
  `Claude Code / Opus`, the way paseo addresses `claude/opus-4.6`.
  Switching model continues the conversation via `--resume`; switching
  provider starts a fresh one and says so, because the new agent has
  never seen the old conversation. Agents are declared in
  `chat.providers[]`; a config predating this keeps its model list.
  Ships a `codex-proto` driver marked experimental — it is written
  against the published protocol but has not been run against a real
  `codex` binary. See [docs/chat-ui-plan.md](docs/chat-ui-plan.md) §4.
- **Multi-node scheduling.** `agent-start-host` now splits into a control
  plane (API, UI, scheduler, relay) and node agents that run the agents
  themselves, and places each session on whichever node has room.
  `--role all` remains the default and behaves exactly as before — the
  in-process node registers over a loopback link through the same path a
  remote node uses. Nodes dial out (`--role node --join-url … --join-token …`),
  so a machine behind NAT joins without opening a port; terminals for
  remote sessions relay through the control plane on the existing wire
  format. Placement filters on readiness, cordon state, session caps,
  resource requests, isolation profile and node labels, then scores on
  reserved capacity, observed load and repository-cache affinity.
  New surfaces: `GET/PATCH/DELETE /api/nodes`, `POST /api/join-tokens`,
  a `/nodes` page in the UI, a node badge on session rows, and
  `agent-start node list|cordon|uncordon|token`. See
  [docs/multinode-cloud-design.ja.md](docs/multinode-cloud-design.ja.md).
- Session start accepts `cpuMillis`, `memMb`, `isolation`, `nodeSelector`
  and `nodeId` so a session can state what it needs and where it may run.
- Screenshots / demo GIF in `docs/screenshots/` (TODO).

### Fixed
- **The "VSCode を開けませんでした" toast now shows why.** It passed the
  error under a key the toast does not read, so the reason was dropped.
- **Session names no longer collide within the same second.** Two
  sessions created in one second shared a worktree, a branch and a
  primary key; names now carry a random suffix.
- **`git worktree add` resolves its base to a commit SHA.** Passing a
  branch name let git's DWIM ignore `-b` and create the wrong branch, and
  failed outright on a bare mirror whose HEAD names a ref it does not
  have.

## [0.2.3] - 2026-06-01

Chat-UI fix for iOS PWAs.

### Fixed
- **iOS PWA chat no longer jitters up and down** while the on-screen
  keyboard is up (#103). The layout viewport is now locked
  (`body { position: fixed; inset: 0 }`) so iOS can't scroll the page to
  lift a focused field, removing the `window.scrollTo(0, 0)` tug-of-war
  against iOS's own focus handling that caused the bounce. `--app-h`
  resizes are coalesced per frame and written only on change.

## [0.2.2] - 2026-06-01

Permission-prompt UI for the chat mode plus chat-UI and host robustness fixes.

### Added
- **Ask-question & plan-approval permission UI for chat mode** (#95): the
  headless Claude chat now surfaces `AskUserQuestion` and `ExitPlanMode`
  prompts interactively, reflecting the plan-mode toggle state from
  `chat_status`.
- Open-core licensing: MIT core + Enterprise Edition, DCO sign-off for
  contributions, and a Japanese licensing overview report.

### Fixed
- **Host never serves a stale `index.html`**, so freshly built front-end
  bundles are picked up reliably.
- Chat-UI session create/delete robustness: instant create/delete, a
  loading state on the session row during delete, optimistic-row grace
  windows, and tab/selection restore when a delete fails.
- PWA header stays visible when the chat input is focused on iOS.

## [0.2.1] - 2026-05-28

Mobile + welcome-screen polish. Wraps up the remaining sub-issues of #88.

### Added
- **Shift+Tab virtual key** on the mobile terminal toolbar (#83). Sends
  `ESC [Z` so Claude Code's `Shift+Tab` mode toggle (plan / auto-accept)
  works on phones without a hardware keyboard.
- **Recent projects on the welcome screen** (#86). When no session is
  selected, the main pane now shows up to six recently-launched projects;
  clicking a card reopens its most recent session.

## [0.2.0] - 2026-05-28

Second feature release. Adds a chat UI mode for headless Claude, a top-level
`agent-start` CLI with auto-daemon, GitHub-issue-driven session launches,
full git write operations + commit graph, a per-session noVNC desktop, and
a long list of front-end polish + installer improvements.

### Added
- **Chat UI mode for headless Claude** (#34): new `ChatTab` drives
  `claude --output-format=stream-json`, with `--resume`, skip-perms,
  model picker, and attachments.
- **`agent-start` CLI** binary: launches the host as an auto-daemon, adds
  update notifications, and exposes a cross-platform `stop` command.
- **Launch a session from a GitHub issue** (#67): issue browser with
  pagination, load-more, and search; one-click session creation pre-filled
  with the issue context.
- **Git write operations + commit graph & file tree** (#24): stage, commit,
  branch switching, and a visual commit/file tree powered by `git-ops`.
- **noVNC desktop tab** (#66): view per-session GUI in the browser.
- **Xvnc desktop boot + opt-in Ubuntu VNC installer** (#70).
- **`agent-start-host update` subcommand** (#68): in-place host upgrade.
- **Optional daemon registration via `AGENT_START_SERVICE=1`** (#59) in the
  installer.
- **Runtime-dep warnings** in `install.sh` for missing `git`, `code-server`,
  or agent CLIs (#63).
- Optimistic UI for session create / tab add / restart.
- Documentation for `--bind 0.0.0.0` and the daemonization workflow (#56).
- Product logo / favicon; roadmap wording neutralized.

### Fixed
- `git-ops`: branch worktrees off the latest `origin` default branch (#71).
- Front: align React to v19 to match `react-dom` (#60).
- Front: right-pane skeleton during boot (#61); save-button polish, root
  scroll, loading feedback (#65); BranchSwitcher respects tracked upstream;
  save-state dirty flag; settings scroll; mobile terminal copy/paste; reset
  selection on remount; leak-proof copy fallback; guard launch sheet
  against duplicate submissions.
- Host: convert routes to axum 0.8 capture syntax (#55); serve dist-root
  public assets and open terminals in chat sessions; canonicalize-based
  path traversal guard.
- CLI: reject malformed version tags; cross-platform `stop`.
- Installer: quiet "tmp: unbound variable" on cleanup (#54).
- Issues: offload `gh` to `spawn_blocking`; reject issue number 0.

### Changed
- Workspace version bumped to `0.2.0` (Rust crates + `package.json` +
  `front/package.json`).

## [0.1.0] - 2026-05-27

Initial public release.

### Added
- Self-hosted Rust HTTP/WebSocket host (`agent-start-host`) with embedded SPA
  (via `rust-embed`), single-binary distribution.
- Vite+ + React + TanStack Router front-end: project browser, session
  launcher, persistent PTYs, optional per-session `git worktree`, code-server
  proxy at `/v/<session>/`.
- Configurable CLIs in `~/.agent-start/config.json` (`claude`, `codex`, custom).
- SQLite-backed session + scrollback persistence; sessions survive host
  restarts (UI shows them as stopped, with full scrollback).
- Multi-target release CI: Linux x86_64 / aarch64, macOS arm64 / x86_64,
  Windows x86_64. Binaries published as GitHub Release assets.
- `install.sh` one-line installer for Linux / macOS.
- OSS scaffolding: `LICENSE` (MIT), `README` (English default + Japanese),
  `CONTRIBUTING`, `SECURITY`, `CODE_OF_CONDUCT`, issue / PR templates,
  dependabot configuration.

[Unreleased]: https://github.com/imanect-labs/agent-start/compare/v0.2.1...HEAD
[0.2.1]: https://github.com/imanect-labs/agent-start/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/imanect-labs/agent-start/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/imanect-labs/agent-start/releases/tag/v0.1.0
