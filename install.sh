#!/usr/bin/env bash
# agent-start installer
#
# Usage:
#   curl -fsSL https://agentstart.imanect.app/install.sh | bash
#
#   # Install and register as a user-level daemon (systemd on Linux,
#   # launchd on macOS). Binds 127.0.0.1 by default — set
#   # AGENT_START_BIND=0.0.0.0 for tailnet/VPN access.
#   curl -fsSL https://agentstart.imanect.app/install.sh | AGENT_START_SERVICE=1 bash
#
# Environment overrides:
#   AGENT_START_VERSION=v0.1.0   # pin a specific release (default: latest)
#   INSTALL_DIR=$HOME/.local/bin # where to drop the binary (default: ~/.local/bin)
#   AGENT_START_REPO=org/repo    # source repo (default: imanect-labs/agent-start)
#   AGENT_START_SERVICE=1        # also register a systemd-user / launchd unit
#   AGENT_START_BIND=127.0.0.1   # service bind address (default: 127.0.0.1)
#   AGENT_START_PORT=3030        # service port (default: 3030)
#
# Windows is not supported by this script — download the .zip from the
# Releases page or use a package manager.

set -euo pipefail

REPO="${AGENT_START_REPO:-imanect-labs/agent-start}"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${AGENT_START_VERSION:-}"
SERVICE="${AGENT_START_SERVICE:-0}"
BIND="${AGENT_START_BIND:-127.0.0.1}"
PORT="${AGENT_START_PORT:-3030}"

err() { printf 'error: %s\n' "$*" >&2; exit 1; }
info() { printf '==> %s\n' "$*"; }
warn() { printf 'warn: %s\n' "$*" >&2; }

need() {
  command -v "$1" >/dev/null 2>&1 || err "missing required command: $1"
}

have() {
  command -v "$1" >/dev/null 2>&1
}

check_runtime_deps() {
  # agent-start-host shells out to these at runtime. They are not required
  # to *install* the binary, but the host will fail to do useful work
  # without them. Warn rather than abort so users can install the missing
  # pieces on their own schedule.
  local missing=() optional_missing=()

  have git         || missing+=("git (required for worktree / project ops)")
  have code-server || missing+=("code-server (required for the in-browser editor — https://github.com/coder/code-server)")

  if ! have claude && ! have codex; then
    optional_missing+=("an agent CLI: 'claude' (https://docs.anthropic.com/en/docs/claude-code) and/or 'codex' (https://github.com/openai/codex) — at least one is needed to launch sessions")
  fi

  if [ ${#missing[@]} -gt 0 ] || [ ${#optional_missing[@]} -gt 0 ]; then
    printf '\n'
    warn "agent-start-host installed, but some runtime dependencies are missing:"
    for m in "${missing[@]}" "${optional_missing[@]}"; do
      printf '  - %s\n' "$m" >&2
    done
    printf '\nInstall the missing tools before starting agent-start-host.\n' >&2
  fi
}

need uname
need tar
if command -v curl >/dev/null 2>&1; then
  DL="curl -fsSL"
elif command -v wget >/dev/null 2>&1; then
  DL="wget -qO-"
else
  err "need either curl or wget"
fi

detect_target() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"

  case "$os" in
    Linux)
      case "$arch" in
        x86_64|amd64) echo "x86_64-unknown-linux-gnu" ;;
        aarch64|arm64) echo "aarch64-unknown-linux-gnu" ;;
        *) err "unsupported Linux arch: $arch" ;;
      esac
      ;;
    Darwin)
      case "$arch" in
        x86_64) echo "x86_64-apple-darwin" ;;
        arm64|aarch64) echo "aarch64-apple-darwin" ;;
        *) err "unsupported macOS arch: $arch" ;;
      esac
      ;;
    *)
      err "unsupported OS: $os (Windows users: download the .zip from Releases)"
      ;;
  esac
}

resolve_version() {
  if [ -n "$VERSION" ]; then
    echo "$VERSION"
    return
  fi
  # Use the GitHub API's redirect to /tag/<tag> to learn the latest tag
  # without parsing JSON.
  local url
  url="https://github.com/${REPO}/releases/latest"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSLI -o /dev/null -w '%{url_effective}\n' "$url" | awk -F/ '{print $NF}'
  else
    # wget doesn't expose final URL easily; fall back to API.
    wget -qO- "https://api.github.com/repos/${REPO}/releases/latest" \
      | sed -n 's/^[[:space:]]*"tag_name":[[:space:]]*"\(.*\)".*/\1/p' \
      | head -n1
  fi
}

install_systemd_unit() {
  local bin="$1" unit_dir unit
  command -v systemctl >/dev/null 2>&1 || {
    warn "systemctl not found; skipping service registration"
    return 1
  }
  unit_dir="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
  unit="$unit_dir/agent-start.service"
  mkdir -p "$unit_dir"
  cat > "$unit" <<EOF
[Unit]
Description=agent-start host
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=${bin} --bind ${BIND} --port ${PORT}
Restart=on-failure
RestartSec=3

[Install]
WantedBy=default.target
EOF
  info "wrote $unit"

  systemctl --user daemon-reload
  systemctl --user enable --now agent-start.service
  info "service enabled and started"

  cat <<EOF

To survive logout / reboots run once (requires sudo):

    sudo loginctl enable-linger "\$USER"

Service ops:
    systemctl --user status agent-start
    journalctl --user -u agent-start -f
    systemctl --user restart agent-start    # after upgrading the binary
EOF
}

install_launchd_plist() {
  local bin="$1" plist label
  command -v launchctl >/dev/null 2>&1 || {
    warn "launchctl not found; skipping service registration"
    return 1
  }
  label="app.agent-start"
  plist="$HOME/Library/LaunchAgents/${label}.plist"
  mkdir -p "$HOME/Library/LaunchAgents"
  cat > "$plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>${label}</string>
  <key>ProgramArguments</key>
  <array>
    <string>${bin}</string>
    <string>--bind</string><string>${BIND}</string>
    <string>--port</string><string>${PORT}</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardOutPath</key><string>/tmp/agent-start.out.log</string>
  <key>StandardErrorPath</key><string>/tmp/agent-start.err.log</string>
</dict>
</plist>
EOF
  info "wrote $plist"

  # Re-bootstrap so config changes take effect on a re-run.
  launchctl bootout "gui/$(id -u)/${label}" 2>/dev/null || true
  launchctl bootstrap "gui/$(id -u)" "$plist"
  launchctl kickstart -k "gui/$(id -u)/${label}"
  info "service loaded and started"

  cat <<EOF

Service ops:
    launchctl list | grep ${label}
    tail -f /tmp/agent-start.out.log /tmp/agent-start.err.log
    launchctl kickstart -k gui/\$(id -u)/${label}   # restart after upgrade
EOF
}

main() {
  local target version url tmp archive bin os installed_bin
  target="$(detect_target)"
  version="$(resolve_version)"
  [ -n "$version" ] || err "could not resolve latest version"
  info "target:  $target"
  info "version: $version"
  info "dest:    $INSTALL_DIR/agent-start-host"

  archive="agent-start-${version}-${target}.tar.gz"
  url="https://github.com/${REPO}/releases/download/${version}/${archive}"
  tmp="$(mktemp -d)"
  trap 'rm -rf "${tmp:-}"' EXIT

  info "downloading $url"
  $DL "$url" > "$tmp/$archive"

  info "extracting"
  tar -xzf "$tmp/$archive" -C "$tmp"

  bin="$(find "$tmp" -type f -name 'agent-start-host' -perm -u+x | head -n1 || true)"
  [ -n "$bin" ] || bin="$(find "$tmp" -type f -name 'agent-start-host' | head -n1 || true)"
  [ -n "$bin" ] || err "agent-start-host binary not found inside archive"

  mkdir -p "$INSTALL_DIR"
  installed_bin="$INSTALL_DIR/agent-start-host"
  install -m 0755 "$bin" "$installed_bin"

  info "installed: $installed_bin"

  case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *)
      cat <<EOF

Note: $INSTALL_DIR is not on your PATH.
Add this to your shell rc (~/.bashrc, ~/.zshrc, ...):

    export PATH="$INSTALL_DIR:\$PATH"

Then reopen the shell or run: source <that file>
EOF
      ;;
  esac

  check_runtime_deps

  if [ "$SERVICE" = "1" ] || [ "$SERVICE" = "true" ]; then
    os="$(uname -s)"
    info "registering daemon (bind=${BIND} port=${PORT})"
    case "$os" in
      Linux)  install_systemd_unit "$installed_bin" || true ;;
      Darwin) install_launchd_plist "$installed_bin" || true ;;
      *)      warn "service registration not supported on $os" ;;
    esac
    cat <<EOF

Open http://${BIND}:${PORT} in your browser
(or http://<host>:${PORT} from another machine on the tailnet/LAN if BIND=0.0.0.0).
EOF
  else
    cat <<EOF

Next steps:
  agent-start-host --port ${PORT}                  # start on 127.0.0.1
  agent-start-host --bind 0.0.0.0 --port ${PORT}   # expose to tailnet/LAN
  open http://localhost:${PORT}

Upgrade later:
  agent-start-host update                          # in-place upgrade
  agent-start-host update --version v0.2.0         # pin a release

Register as a daemon (systemd on Linux, launchd on macOS):
  curl -fsSL https://agentstart.imanect.app/install.sh | AGENT_START_SERVICE=1 bash
  # Add AGENT_START_BIND=0.0.0.0 for tailnet access.

Security: bind to 0.0.0.0 only behind a tailnet / VPN / firewalled LAN.
See https://github.com/imanect-labs/agent-start/blob/main/SECURITY.md
EOF
  fi
}

main "$@"
