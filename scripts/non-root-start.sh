#!/usr/bin/env bash
# First start as a non-root user, database in a fresh temp dir outside the
# repo (the shape of a static `skrongle` user with data under /mnt/...,
# docs/self-hosting.md "A static user"): /healthz answers, the log has the
# owner link, <db>.key is mode 600 and SQLite made its WAL files there.
#
#   SERVER_BIN=path/to/binary scripts/non-root-start.sh   # CI: scripts/ci-build.sh
#   scripts/non-root-start.sh                             # builds a debug binary
#
# As root (a container) it drops to `nobody` with setpriv; otherwise it runs
# as the current user (CI's `runner`, or you on macOS).
set -euo pipefail
cd "$(dirname "$0")/.."
command -v cargo >/dev/null 2>&1 || { [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"; }
BIN="${SERVER_BIN:-}"
[ -n "$BIN" ] || { cargo build -q --features dev >&2; BIN="${CARGO_TARGET_DIR:-target}/debug/spanish-learning"; }
BIN=$(cd "$(dirname "$BIN")" && pwd)/$(basename "$BIN")
DATA=$(pwd)/data

WORK=$(mktemp -d "${TMPDIR:-/tmp}/learn-nonroot.XXXXXX")
PID=""
trap '[ -n "$PID" ] && kill "$PID" 2>/dev/null || true; rm -rf "$WORK"' EXIT
RUN=()
if [ "$(id -u)" = 0 ]; then
  chown nobody "$WORK" && chmod 750 "$WORK"
  RUN=(setpriv --reuid=nobody --regid="$(id -g nobody)" --clear-groups)
fi
PORT=$(python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')

env SKRONGLE_BIND="127.0.0.1:$PORT" SKRONGLE_DB="$WORK/learn.db" SKRONGLE_DATA_DIR="$DATA" \
  SKRONGLE_SIGNUP=invite SKRONGLE_LOG=info ${RUN[@]+"${RUN[@]}"} "$BIN" >"$WORK/server.log" 2>&1 &
PID=$!
for _ in $(seq 1 100); do curl -fsS "http://127.0.0.1:$PORT/healthz" >/dev/null 2>&1 && break; sleep 0.2; done
fail() { echo "non-root start: $1"; cat "$WORK/server.log"; exit 1; }
curl -fsS "http://127.0.0.1:$PORT/healthz" >/dev/null || fail "/healthz did not answer"
grep -q "Owner link" "$WORK/server.log" || fail "no owner link in the log"
mode=$(stat -c %a "$WORK/learn.db.key" 2>/dev/null || stat -f %Lp "$WORK/learn.db.key")
[ "$mode" = 600 ] || fail "learn.db.key has mode $mode, expected 600"
[ -f "$WORK/learn.db-wal" ] || fail "no learn.db-wal next to the database"
echo "  ok   non-root start (uid $(id -u)${RUN[0]:+, as nobody}): /healthz, owner link, learn.db.key mode 600, WAL files"
