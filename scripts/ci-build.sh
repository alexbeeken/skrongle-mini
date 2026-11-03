#!/usr/bin/env bash
# CI only: build the dev server (`--features dev`) and print the binary's path, so a job can
# pass it on as SERVER_BIN. See docs/testing.md, "CI runners".
set -euo pipefail
cd "$(dirname "$0")/.."
command -v cargo >/dev/null 2>&1 || { [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"; }
cargo build -q --features dev >&2
echo "${CARGO_TARGET_DIR:-target}/debug/spanish-learning"
