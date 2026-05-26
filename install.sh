#!/usr/bin/env bash
# agent-start installer
#
# Usage:
#   curl -fsSL https://agentstart.imanect.app/install.sh | bash
#
# Environment overrides:
#   AGENT_START_VERSION=v0.1.0   # pin a specific release (default: latest)
#   INSTALL_DIR=$HOME/.local/bin # where to drop the binary (default: ~/.local/bin)
#   AGENT_START_REPO=org/repo    # source repo (default: imanect-labs/agent-start)
#
# Windows is not supported by this script — download the .zip from the
# Releases page or use a package manager.

set -euo pipefail

REPO="${AGENT_START_REPO:-imanect-labs/agent-start}"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${AGENT_START_VERSION:-}"

err() { printf 'error: %s\n' "$*" >&2; exit 1; }
info() { printf '==> %s\n' "$*"; }

need() {
  command -v "$1" >/dev/null 2>&1 || err "missing required command: $1"
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

main() {
  local target version url tmp archive bin
  target="$(detect_target)"
  version="$(resolve_version)"
  [ -n "$version" ] || err "could not resolve latest version"
  info "target:  $target"
  info "version: $version"
  info "dest:    $INSTALL_DIR/agent-start-host"

  archive="agent-start-${version}-${target}.tar.gz"
  url="https://github.com/${REPO}/releases/download/${version}/${archive}"
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT

  info "downloading $url"
  $DL "$url" > "$tmp/$archive"

  info "extracting"
  tar -xzf "$tmp/$archive" -C "$tmp"

  bin="$(find "$tmp" -type f -name 'agent-start-host' -perm -u+x | head -n1 || true)"
  [ -n "$bin" ] || bin="$(find "$tmp" -type f -name 'agent-start-host' | head -n1 || true)"
  [ -n "$bin" ] || err "agent-start-host binary not found inside archive"

  mkdir -p "$INSTALL_DIR"
  install -m 0755 "$bin" "$INSTALL_DIR/agent-start-host"

  info "installed: $INSTALL_DIR/agent-start-host"

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

  cat <<'EOF'

Next steps:
  agent-start-host --port 3030          # start the host on 127.0.0.1
  open http://localhost:3030            # in your browser

Security: bind to 0.0.0.0 only behind a tailnet / VPN / firewalled LAN.
See https://github.com/imanect-labs/agent-start/blob/main/SECURITY.md
EOF
}

main "$@"
