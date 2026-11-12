#!/usr/bin/env bash
# The release browser specs (e2e/specs/release, Playwright projects
# `release-first-run`, `release` and `release-open`; docs/testing.md, "Release
# specs") against a server this script starts: a fresh database, no seeded
# user, SKRONGLE_SIGNUP=invite, and the owner link read from the server's log,
# which is all a self-hoster has.
#
#   scripts/release-e2e.sh                    # builds a release binary (no dev feature) and runs both
#   SERVER_BIN=path scripts/release-e2e.sh    # that binary (CI: the dev build, run with SKRONGLE_DEV=1)
#   RELEASE_E2E_DATA=dir ...                  # the server's SKRONGLE_DATA_DIR instead of ./data (a release's unpacked data/, #46)
#   RELEASE_E2E_DEV=1 ...                   # start it with SKRONGLE_DEV=1 (a dev build): /dev routes exist, the specs must not
#                                             # use them; the dev user is the owner, so the owner-link test is skipped
#   scripts/release-e2e.sh --grep "docs"      # any Playwright option works
#
# To run the same specs against a server you did not start (the unpacked
# tarball from `KEEP=1 scripts/release-qa/package-smoke.sh`, a container, a
# Pi), do not use this script:
#   E2E_BASE_URL=http://host:port E2E_OWNER_LINK='<link from its log>' \
#     npx --prefix e2e playwright test --project release
# (or E2E_OWNER_EMAIL and E2E_OWNER_PASSWORD for a server that has an owner).
set -euo pipefail
cd "$(dirname "$0")/.."
command -v cargo >/dev/null 2>&1 || { [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"; }

if [ ! -d e2e/node_modules/@playwright/test ]; then
  (cd e2e && npm ci --no-fund --no-audit && npx playwright install chromium)
fi

BIN="${SERVER_BIN:-}"
[ -n "$BIN" ] || { cargo build -q --release; BIN="${CARGO_TARGET_DIR:-target}/release/spanish-learning"; }

WORK=$(mktemp -d "${TMPDIR:-/tmp}/release-e2e.XXXXXX")
PID=""
stop() { if [ -n "$PID" ]; then kill "$PID" 2>/dev/null || true; wait "$PID" 2>/dev/null || true; PID=""; fi; }
trap 'stop; rm -rf "$WORK"' EXIT

# start_server <signup mode>: sets URL and LOG.
start_server() {
  stop
  local port
  port=$(python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')
  URL="http://127.0.0.1:$port"
  LOG="$WORK/server-$port.log"
  local dev=()
  [ "${RELEASE_E2E_DEV:-0}" = 1 ] && dev=(SKRONGLE_DEV=1)
  env SKRONGLE_BIND="127.0.0.1:$port" SKRONGLE_DB="$WORK/learn-$port.db" SKRONGLE_DATA_DIR="${RELEASE_E2E_DATA:-./data}" \
    SKRONGLE_SIGNUP="$1" SKRONGLE_LOG=info ${dev[@]+"${dev[@]}"} "$BIN" >"$LOG" 2>&1 &
  PID=$!
  for _ in $(seq 1 100); do curl -fsS "$URL/healthz" >/dev/null 2>&1 && break; sleep 0.2; done
  curl -fsS "$URL/healthz" >/dev/null || { echo "server did not start:"; cat "$LOG"; exit 1; }
}

STATUS=0

echo "==> release specs against a fresh invite-only server"
start_server invite
OWNER=()
if [ "${RELEASE_E2E_DEV:-0}" = 1 ]; then
  # Dev mode starts with the dev user, so there is no owner link: that user is the owner.
  OWNER=(E2E_OWNER_EMAIL=dev@example.com E2E_OWNER_PASSWORD=dev-password)
else
  LINK=$(sed -n 's/.*Owner link.*\(http:\/\/[^ ]*invite=[A-Za-z0-9_-]*\).*/\1/p' "$LOG" | head -1)
  [ -n "$LINK" ] || { echo "no owner link in the server log:"; cat "$LOG"; exit 1; }
  OWNER=(E2E_OWNER_LINK="$LINK")
fi
(cd e2e && env E2E_BASE_URL="$URL" "${OWNER[@]}" npx playwright test "$@" --pass-with-no-tests --project release) || STATUS=$?

echo "==> dev-free specs of the other folders against a fresh open-signup server"
start_server open
(cd e2e && E2E_BASE_URL="$URL" npx playwright test "$@" --pass-with-no-tests --project release-open) || STATUS=$?

if [ "$STATUS" != 0 ]; then
  echo "==> release e2e FAILED. Report: npx --prefix e2e playwright show-report e2e/results/report; last server log:"
  tail -20 "$LOG"
fi
exit "$STATUS"
