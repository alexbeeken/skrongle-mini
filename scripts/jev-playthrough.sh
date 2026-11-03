#!/usr/bin/env bash
# Playthrough: an end-to-end test that plays a whole course in one mode, with
# Jev as the learner. A new account (registered through /register) opens the
# course and plays every lesson in the mode's order. A lesson Jev can't pass
# in --attempts tries is marked failed and passed artificially (the agents'
# API), so the run reaches the end. Along the way it records exact bugs
# (HTTP errors, console errors, overflow), checks every page agrees on what
# is passed, and saves a database checkpoint after each lesson.
#
#   scripts/jev-playthrough.sh sprint               # Sprint mode; also: study, play (or the -sprint/-study/-play scripts)
#   scripts/jev-playthrough.sh study --lessons 5    # the first 5 lessons only
#   scripts/jev-playthrough.sh play --attempts 1    # tries per lesson before the fudge (default 2)
#   scripts/jev-playthrough.sh sprint --fail-on bug,invariant,stuck   # what exits 1 (default bug,invariant)
#   scripts/jev-playthrough.sh sprint --headed --keep               # watch it; leave the server up
#   scripts/jev-playthrough.sh sprint --url http://127.0.0.1:9000   # use a running server
#                                               # (E2E_MINI=1 too if it is a mini: registering signs in there)
#   scripts/jev-playthrough.sh serve e2e/results/playthrough/<run>/checkpoints/12-ser-vs-estar.db
#                                               # a server on that checkpoint, logged-in account printed
#   scripts/jev-playthrough.sh calibrate [--lessons 12]   # how often Jev answers the quiz right, asked
#                                               # as the player asks; rerun after changing player.ts
#
# The key comes from TYPESAFE_API_KEY (or JEV_API_KEY), ./.env or e2e/.env.
# Each run: e2e/results/playthrough/<timestamp>-<mode>/ (report.md, report.json,
# shots/, checkpoints/). Exit 1 on a failing finding. See docs/testing.md, "Playthrough".
set -euo pipefail
cd "$(dirname "$0")/.."
usage() { sed -n '2,24p' "$0" | sed 's/^# \{0,1\}//'; exit "${1:-0}"; }
case " $* " in *" -h "*|*" --help "*) usage ;; esac
[ $# -ge 1 ] || usage 2
command -v cargo >/dev/null 2>&1 || { [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"; }

free_port() { python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])'; }
start_server() { # $1 database, $2 log; SKRONGLE_DEV in the environment for calibrate's dev login
  PORT=$(free_port)
  URL="http://127.0.0.1:$PORT"
  # A plain server, as a learner meets it: no dev mode, no seed. A mini (SKRONGLE_MINI=1),
  # because a server without SKRONGLE_MINI or dev mode needs every provider set (#641).
  # TEST_COURSE=1 also offers the test course (scripts/jev-playthrough-test-course.sh).
  SKRONGLE_BIND="127.0.0.1:$PORT" SKRONGLE_DB="$1" SKRONGLE_DATA_DIR=./data SKRONGLE_LOG=info SKRONGLE_DEV="${SKRONGLE_DEV:-}" TEST_COURSE="${TEST_COURSE:-}" SKRONGLE_MINI=1 \
    "${CARGO_TARGET_DIR:-target}/debug/spanish-learning" >"$2" 2>&1 &
  PID=$!
  for _ in $(seq 1 100); do curl -fsS "$URL/healthz" >/dev/null 2>&1 && break; sleep 0.2; done
  curl -fsS "$URL/healthz" >/dev/null || { echo "server did not start:"; cat "$2"; exit 1; }
}

if [ "$1" = serve ]; then
  CP="${2:?serve needs a checkpoint .db}"
  [ -f "$CP" ] || { echo "no such checkpoint: $CP"; exit 2; }
  cargo build -q
  DB=./playthrough-serve.db
  rm -f "$DB" "$DB-wal" "$DB-shm" "$DB.key"
  cp "$CP" "$DB"
  [ -f "$CP.key" ] && cp "$CP.key" "$DB.key"
  start_server "$DB" playthrough-server.log
  echo "==> $URL on $(basename "$CP") (log playthrough-server.log). Ctrl-C stops it."
  ACC="$(dirname "$CP")/../account.json"
  [ -f "$ACC" ] && sed -n -E 's/.*"(email|password)": "(.*)".*/    \1: \2/p' "$ACC"
  trap 'kill $PID 2>/dev/null' EXIT INT TERM
  wait "$PID"
  exit 0
fi

MODE="$1"
case "$MODE" in d) MODE=sprint ;; k) MODE=study ;; g) MODE=play ;; esac # the old one-letter names
case "$MODE" in sprint|study|play|calibrate) ;; *) echo "unknown mode $MODE (sprint, study, play, serve or calibrate)"; usage 2 ;; esac
set -- "$MODE" "${@:2}"

KEY="${TYPESAFE_API_KEY:-${JEV_API_KEY:-}}"
for f in .env e2e/.env; do
  [ -z "$KEY" ] && [ -f "$f" ] && KEY=$(sed -n -E 's/^(TYPESAFE_API_KEY|JEV_API_KEY)=//p' "$f" | tr -d "\"' " | head -1)
done
if [ -z "$KEY" ]; then
  echo "No Jev key. Create one at https://console.typesafe.ai/keys and put it in ./.env or e2e/.env:"
  echo "  TYPESAFE_API_KEY=..."
  exit 2
fi
# JEV_MODEL and the like, for the runner only.
for f in .env e2e/.env; do
  if [ -f "$f" ]; then set -a; . "./$f"; set +a; fi
done
export TYPESAFE_API_KEY="$KEY"

if [ ! -d e2e/node_modules/@playwright/test ]; then
  (cd e2e && npm ci --no-fund --no-audit && npx playwright install chromium)
fi

KEEP=0 GIVEN_URL=""
prev=""
for a in "$@"; do
  [ "$a" = --keep ] && KEEP=1
  [ "$prev" = --url ] && GIVEN_URL="$a"
  prev="$a"
done

PID=""
if [ -z "$GIVEN_URL" ]; then
  # One database per mode, so the -all script's runs don't share one.
  DB="./playthrough-$MODE.db" LOG="playthrough-$MODE-server.log"
  cleanup() {
    [ -z "$PID" ] && return
    if [ "$KEEP" = 1 ]; then echo "==> server still running: $URL (pid $PID, log $LOG; stop: kill $PID)"
    else kill "$PID" 2>/dev/null || true; fi
  }
  trap cleanup EXIT
  cargo build -q
  rm -f "$DB" "$DB-wal" "$DB-shm" "$DB.key"
  [ "$MODE" = calibrate ] && export SKRONGLE_DEV=1 # it logs in through /dev/login
  start_server "$DB" "$LOG"
  echo "==> server $URL ($DB)"
  # Our server is a mini: registering signs the learner in, with no "check
  # your mail" page, and signUp (e2e/lib/learner.ts) only expects that with
  # E2E_MINI=1 (#895).
  export PLAYTHROUGH_URL="$URL" PLAYTHROUGH_DB="$DB" E2E_MINI=1
fi

STATUS=0
if [ "$MODE" = calibrate ]; then shift; node e2e/playthrough/calibrate.ts "$@" || STATUS=$?
else node e2e/playthrough/index.ts "$@" || STATUS=$?; fi
exit "$STATUS"
