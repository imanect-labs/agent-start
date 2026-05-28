# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Screenshots / demo GIF in `docs/screenshots/` (TODO).

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

[Unreleased]: https://github.com/imanect-labs/agent-start/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/imanect-labs/agent-start/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/imanect-labs/agent-start/releases/tag/v0.1.0
