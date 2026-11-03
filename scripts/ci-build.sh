#!/usr/bin/env bash
# CI only: build the dev server once per commit for every job of a run when
# the runners share a /_shared volume, and print the binary's path. The first
# job to get here builds the commit in its own target dir and copies the
# binary in, the others wait for that and use it, so a run compiles the server
# once instead of once per job. The binary finds static/ by the checkout path
# it was built in, so the runners must check out at the same path. Without
# /_shared or flock (GitHub-hosted runners, a Mac) it just builds. See
# docs/testing.md, "CI runners".
set -euo pipefail
cd "$(dirname "$0")/.."
command -v cargo >/dev/null 2>&1 || { [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"; }
built="${CARGO_TARGET_DIR:-target}/debug/spanish-learning"
shared="${CI_SHARED_DIR:-/_shared}/bin"
if [ ! -d "${shared%/bin}" ] || ! command -v flock >/dev/null; then
  cargo build -q >&2
  echo "$built"; exit
fi
mkdir -p "$shared"
out="$shared/$(git rev-parse HEAD)-${CARGO_PROFILE_DEV_DEBUG:-full}"
exec 9>"$out.lock"
flock 9
if [ ! -x "$out" ]; then
  cargo build -q >&2
  cp "$built" "$out.tmp" && mv "$out.tmp" "$out"
else
  echo "==> server already built for this commit by another job" >&2
fi
find "$shared" -type f -mmin +720 -delete 2>/dev/null || true
echo "$out"
