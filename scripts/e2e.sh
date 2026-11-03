#!/usr/bin/env bash
# Browser tests (Playwright, headless) against a real server: fresh
# database, seeded dev user (with the simulated provider's keys). The exact specs need no network
# and no key; the Jev specs and journeys run only when TYPESAFE_API_KEY (or
# JEV_API_KEY) is set, in the environment, e2e/.env or ./.env, and are skipped
# otherwise. See docs/testing.md, "Browser tests".
#
#   scripts/e2e.sh                       # everything (Jev parts skipped without a key)
#   scripts/e2e.sh --grep "reading check" # any Playwright option works
#   scripts/e2e-jev.sh [journey-id…]     # only the Jev tests (see that script)
#   KEEP=1 scripts/e2e.sh                # leave the server running afterwards
#   SERVER_BIN=path scripts/e2e.sh       # run that server binary, don't build
#
# Draws are seeded: E2E_SEED (default 1) goes to the server as
# SKRONGLE_RNG_SEED and to the browser's Math.random (e2e/lib/fixtures.ts), so
# a failing run replays with the same E2E_SEED; the seed is printed.
#
# The server offers the test course too (TEST_COURSE=1, data/test-courses/):
# specs open it with devLogin(page, "open=test-course").
#
# Model calls go to the simulated provider at /dev/ai, which answers from
# data/test-fixtures/llm (docs/testing.md, "The dev AI provider");
# E2E_LLM_MODE=capture starts it holding every request for /dev/ai's Manual replies. A model
# request gives up after SKRONGLE_AI_TIMEOUT_SECS=8, so a `timeout` fixture or an
# injected timeout (specs/dev-ai.spec.ts) is over in seconds.
#
# E2E_MINI=1 is the mini lane (docs/testing.md, "The mini lane"): the
# server runs without the dev stand-ins a self-hoster lacks (no dev mailbox,
# no dev Stripe), and
# the specs see E2E_MINI=1 so the ones that need a stand-in skip.
#
# Uses its own database (./e2e.db, or E2E_DB) and a free port, so a running
# ./.dev-start is left alone. First run installs e2e/node_modules and
# Playwright's Chromium (needs network once).
set -euo pipefail
cd "$(dirname "$0")/.."
command -v cargo >/dev/null 2>&1 || { [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"; }

if [ ! -d e2e/node_modules/@playwright/test ]; then
  (cd e2e && npm ci --no-fund --no-audit && npx playwright install chromium)
fi

LOG=$(mktemp "${TMPDIR:-/tmp}/learn-e2e.XXXXXX")
PORT=$(python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')
URL="http://127.0.0.1:$PORT"
PID=""
cleanup() {
  if [ -n "$PID" ] && [ "${KEEP:-0}" != 1 ]; then kill "$PID" 2>/dev/null || true; fi
  [ "${KEEP:-0}" = 1 ] && echo "==> server still running: $URL (pid $PID, log $LOG)" || rm -f "$LOG"
}
trap cleanup EXIT

SEED="${E2E_SEED:-1}"
BIN="${SERVER_BIN:-}" # a server already built (CI: scripts/ci-build.sh)
[ -n "$BIN" ] || { cargo build -q; BIN="${CARGO_TARGET_DIR:-target}/debug/spanish-learning"; }
DB="${E2E_DB:-./e2e.db}" # another name lets two runs share the checkout
rm -f "$DB" "$DB-wal" "$DB-shm" "$DB.key"
MINI="${E2E_MINI:-0}"
# The mini lane: mail and texts only logged, no stand-in Stripe, and
# SKRONGLE_MINI=1, which makes the server a Skrongle mini (#578: no plans and
# no dev stand-ins, the hosted-only settings ignored;
# /healthz says so, #641).
MINI_ENV=()
if [ "$MINI" = 1 ]; then
  MINI_ENV=(SKRONGLE_MINI=1)
fi
env ${MINI_ENV[@]+"${MINI_ENV[@]}"} SKRONGLE_BIND="127.0.0.1:$PORT" SKRONGLE_DB="$DB" SKRONGLE_DATA_DIR=./data SKRONGLE_LOG=info \
  SKRONGLE_DEV=1 SKRONGLE_DEV_SEED=1 SKRONGLE_LLM_MODE="${E2E_LLM_MODE:-}" SKRONGLE_LLM_TIMEOUT_SECS=30 SKRONGLE_AI_TIMEOUT_SECS=8 \
  TEST_COURSE=1 SKRONGLE_RNG_SEED="$SEED" \
  "$BIN" >"$LOG" 2>&1 &
PID=$!
for _ in $(seq 1 100); do curl -fsS "$URL/healthz" >/dev/null 2>&1 && break; sleep 0.2; done
curl -fsS "$URL/healthz" >/dev/null || { echo "server did not start:"; cat "$LOG"; exit 1; }

# The Jev key (and JEV_* settings) for the test runner only, not the app server.
for f in .env e2e/.env; do
  if [ -f "$f" ]; then set -a; . "./$f"; set +a; fi
done
echo "==> e2e against $URL, seed $SEED$( [ "$MINI" = 1 ] && echo ", mini lane") ($( [ -n "${TYPESAFE_API_KEY:-}${JEV_API_KEY:-}" ] && echo "Jev on" || echo "Jev off: no key, those specs are skipped"))"
STATUS=0
(cd e2e && E2E_BASE_URL="$URL" E2E_SEED="$SEED" E2E_MINI="$MINI" npx playwright test "$@") || STATUS=$?
if [ "$STATUS" != 0 ]; then
  echo "==> e2e FAILED (seed $SEED: rerun with E2E_SEED=$SEED). Report: npx --prefix e2e playwright show-report e2e/results/report; server log tail:"
  tail -20 "$LOG"
fi
exit "$STATUS"
