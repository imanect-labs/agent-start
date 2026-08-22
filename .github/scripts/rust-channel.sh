#!/usr/bin/env bash
#
# Emit the Rust channel pinned in `server-rs/rust-toolchain.toml` as a step
# output named `channel`, for `dtolnay/rust-toolchain` to install.
#
# `dtolnay/rust-toolchain` does not read the toolchain file, so without this
# CI would install whatever `stable` is today while local builds used the
# pinned version — the two drifting apart is exactly what pinning is meant to
# prevent. Reading the file keeps one source of truth.
set -euo pipefail

file="server-rs/rust-toolchain.toml"

channel=$(sed -n 's/^[[:space:]]*channel[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$file" | head -n 1)

if [ -z "$channel" ]; then
  # Failing here beats silently falling back to `stable`: a job that installs
  # an unintended compiler is worse than one that refuses to start.
  echo "::error file=$file::could not read 'channel' from $file" >&2
  exit 1
fi

echo "Using Rust toolchain: $channel (from $file)"
echo "channel=$channel" >>"$GITHUB_OUTPUT"
