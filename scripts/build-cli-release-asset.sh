#!/usr/bin/env bash
# Build a moss-cli release binary, named exactly like the GitHub release
# asset the npm wrapper and Homebrew formula already expect
# (packages/moss/lib/fetch.js's resolveAsset(), scripts/release-channels.json).
# moss-cli has no window system in its dependency graph at all, so the same
# recipe covers a coding agent's container and a desktop machine alike.
#
# Usage (run from the repo root):
#   scripts/build-cli-release-asset.sh darwin-universal   # needs both Apple rustup targets
#   scripts/build-cli-release-asset.sh linux-x86_64       # native build, no extra targets
#
# darwin-universal builds moss-cli twice (x86_64-apple-darwin,
# aarch64-apple-darwin) and lipo's the two into one universal binary — the
# standard way to ship one macOS binary for both Apple silicon and Intel.
# linux-x86_64 is a plain native build: moss-cli has no GUI toolkit
# dependency (check crates/moss-cli/Cargo.toml — no tauri, no wry, direct or
# transitive), so unlike a full desktop-app build it needs no GTK/WebKit dev
# headers to link.
#
# Writes the finished, executable binary to the CURRENT directory as
# moss-<platform> and prints its own --version line so the caller's log shows
# what shipped. `--locked` so a release build never silently re-resolves
# Cargo.lock.
#
# `--target-dir` is passed explicitly on every cargo invocation, pointing at
# this repo's own `target/` (derived from this script's own path, not from
# `pwd` or Cargo's config-file search). A caller that checks this repo out as
# a subdirectory of a larger workspace — as the desktop app's release
# workflow does — would otherwise have `cargo build` walk up from `pwd` and
# pick up an ancestor `.cargo/config.toml`'s `build.target-dir` if one is
# ever added there, silently moving where the binary lands and breaking the
# hardcoded `target/<triple>/release/moss-cli` paths below. An explicit
# `--target-dir` cannot be overridden by an ancestor config.

set -euo pipefail

PLATFORM="${1:-}"
if [ -z "$PLATFORM" ]; then
  echo "usage: $0 <darwin-universal|linux-x86_64>" >&2
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"
TARGET_DIR="$REPO_ROOT/target"

case "$PLATFORM" in
  darwin-universal)
    OUT="moss-darwin-universal"
    for TARGET in x86_64-apple-darwin aarch64-apple-darwin; do
      cargo build --locked --release -p moss-cli --target "$TARGET" --target-dir "$TARGET_DIR"
    done
    lipo -create -output "$OUT" \
      "$TARGET_DIR/x86_64-apple-darwin/release/moss-cli" \
      "$TARGET_DIR/aarch64-apple-darwin/release/moss-cli"
    lipo -info "$OUT"
    ;;
  linux-x86_64)
    OUT="moss-linux-x86_64"
    cargo build --locked --release -p moss-cli --target-dir "$TARGET_DIR"
    cp "$TARGET_DIR/release/moss-cli" "$OUT"
    ;;
  *)
    echo "unknown platform: $PLATFORM (want darwin-universal or linux-x86_64)" >&2
    exit 1
    ;;
esac

chmod +x "$OUT"
"./$OUT" --version
echo "built $OUT"
