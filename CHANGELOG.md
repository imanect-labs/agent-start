# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Screenshots / demo GIF in `docs/screenshots/` (TODO).

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

[Unreleased]: https://github.com/imanect-labs/agent-start/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/imanect-labs/agent-start/releases/tag/v0.1.0
