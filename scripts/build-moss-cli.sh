#!/usr/bin/env bash
# Build moss-cli with the browser editor's bundle in it: the bundle first, then
# cargo with MOSS_REQUIRE_UI_BUNDLE=1, which refuses to build without it. A
# plain `cargo build -p moss-cli` makes a binary with no editor, whose
# `moss edit --serve` refuses to start.
#
# Every argument goes to `cargo build`, run from the repo root (so a relative
# path in them is relative to the root):
#   bash scripts/build-moss-cli.sh                 # target/debug/moss-cli
#   bash scripts/build-moss-cli.sh --release
#   bash scripts/build-moss-cli.sh --locked --profile release-optimized \
#     --target x86_64-apple-darwin --target aarch64-apple-darwin
set -euo pipefail
cd "$(dirname "$0")/.."
pnpm --filter ./packages/moss-ui run build:browser
MOSS_REQUIRE_UI_BUNDLE=1 cargo build -p moss-cli "$@"
