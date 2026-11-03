#!/usr/bin/env bash
# Smoke test against a real server: fresh database, seeded dev user (with
# the simulated provider's keys). Hits the main pages (expects 200s), then
# generates a course end to end through the dev AI provider at /dev/ai,
# which answers from data/test-fixtures/llm, and opens it. No network needed.
#
#   scripts/smoke.sh             # debug build
#   scripts/smoke.sh --release   # release profile
#   KEEP=1 scripts/smoke.sh      # leave the server running afterwards (prints its URL)
#
# Uses its own database (./smoke.db) and a free port, so a running
# ./.dev-start is left alone.
set -euo pipefail
cd "$(dirname "$0")/.."
PROFILE_FLAG=""; PROFILE_DIR=debug
[ "${1:-}" = --release ] && { PROFILE_FLAG=--release; PROFILE_DIR=release; }
command -v cargo >/dev/null 2>&1 || { [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"; }

WORK=$(mktemp -d "${TMPDIR:-/tmp}/learn-smoke.XXXXXX")
PORT=$(python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')
URL="http://127.0.0.1:$PORT"
JAR="$WORK/cookies"
PID=""
cleanup() {
  if [ -n "$PID" ] && [ "${KEEP:-0}" != 1 ]; then kill "$PID" 2>/dev/null || true; fi
  [ "${KEEP:-0}" = 1 ] || rm -rf "$WORK"
}
trap cleanup EXIT

BIN="${SERVER_BIN:-}" # a server already built (CI: scripts/ci-build.sh)
[ -n "$BIN" ] || { cargo build -q $PROFILE_FLAG; BIN="${CARGO_TARGET_DIR:-target}/$PROFILE_DIR/spanish-learning"; }
rm -f smoke.db smoke.db-wal smoke.db-shm smoke.db.key
SKRONGLE_BIND="127.0.0.1:$PORT" SKRONGLE_DB=./smoke.db SKRONGLE_DATA_DIR=./data SKRONGLE_LOG=info \
  SKRONGLE_DEV=1 SKRONGLE_DEV_SEED=1 SKRONGLE_LLM_MODE= SKRONGLE_LLM_TIMEOUT_SECS=30 \
  "$BIN" >"$WORK/server.log" 2>&1 &
PID=$!
for _ in $(seq 1 100); do curl -fsS "$URL/healthz" >/dev/null 2>&1 && break; sleep 0.2; done
curl -fsS "$URL/healthz" >/dev/null || { echo "server did not start:"; cat "$WORK/server.log"; exit 1; }

FAILED=0
check() { # check <path> [expected status]
  local code
  code=$(curl -s -o "$WORK/body" -w '%{http_code}' -b "$JAR" -c "$JAR" "$URL$1")
  if [ "$code" = "${2:-200}" ]; then echo "  ok   $code $1"; else echo "  FAIL $code $1 (expected ${2:-200})"; FAILED=1; fi
}

echo "==> pages"
check /healthz
check /login
check /docs/testing
check /self-host 200   # the self-hosting page: a mini always serves it (#13)
check /privacy
echo "==> every link between docs pages"
DOCS=$( { for p in /docs $(curl -s "$URL/docs" | grep -o 'href="/docs/[^"#]*"' | sed 's/href="//; s/"//' | sort -u); do
  curl -s "$URL$p" | grep -o 'href="/docs/[^"#]*"' | sed 's/href="//; s/"//'; done; } | sort -u)
for p in $DOCS; do check "$p"; done
check "/dev/login?open=spanish" 303
LESSON=$(curl -s -b "$JAR" "$URL/lessons" | grep -o 'href="/lessons/[a-z0-9-]*"' | head -1 | sed 's/href="//; s/"//')
for p in / /lessons "$LESSON" /practice /map /history /vocabulary /listening /sprint /play /guide /tutor /courses /profile /dev/mail /dev/inbox /dev/ai; do
  check "$p"
done
echo "==> the dev mailbox needs no sign-in"
for p in /dev/inbox /dev/mail /dev/authenticator /dev/ai; do
  code=$(curl -s -o /dev/null -w '%{http_code}' "$URL$p?format=json")
  if [ "$code" = 200 ]; then echo "  ok   $code $p signed out"; else echo "  FAIL $code $p signed out (expected 200)"; FAILED=1; fi
done
echo "==> the dev AI provider answers its wire API with a dev key"
code=$(curl -s -o "$WORK/body" -w '%{http_code}' -H 'x-api-key: sk-ant-dev-anthropic' -H 'anthropic-version: 2023-06-01' -H 'content-type: application/json' -H 'x-skrongle-call: purpose=tutor-turn' \
  --data '{"model":"claude-opus-5-5","max_tokens":50,"messages":[{"role":"user","content":"Hola"}]}' "$URL/dev/ai/anthropic/v1/messages")
if [ "$code" = 200 ] && grep -q '"end_turn"' "$WORK/body"; then echo "  ok   $code /dev/ai/anthropic/v1/messages"; else echo "  FAIL $code /dev/ai/anthropic/v1/messages: $(head -c 300 "$WORK/body")"; FAILED=1; fi
code=$(curl -s -o /dev/null -w '%{http_code}' -H 'authorization: Bearer sk-not-a-dev-key' -H 'content-type: application/json' \
  --data '{"model":"gpt-5","messages":[{"role":"user","content":"Hola"}]}' "$URL/dev/ai/openai/v1/chat/completions")
if [ "$code" = 401 ]; then echo "  ok   $code an unknown key is refused"; else echo "  FAIL $code an unknown key (expected 401)"; FAILED=1; fi

echo "==> create a course and generate it with the seeded key (the simulator answers from the fixtures)"
DRAFT=$(curl -s -o /dev/null -w '%{redirect_url}' -b "$JAR" -c "$JAR" --data "title=" "$URL/courses/new")
DRAFT=${DRAFT#"$URL"}
check "$DRAFT"
grep -q 'data-testid="generate-form"' "$WORK/body" && echo "  ok   the new course opens with the generate dialog" || { echo "  FAIL no generate dialog on $DRAFT"; FAILED=1; }
LOC=$(curl -s -o /dev/null -w '%{redirect_url}' -b "$JAR" -c "$JAR" \
  --data-urlencode "provider=anthropic" --data-urlencode "wish=Fractions for Friday" \
  --data-urlencode "depth=full" --data-urlencode "kind=auto" "$URL${DRAFT%%\?*}/generate")
JOB=${LOC##*\?job=}
if [ -z "$JOB" ] || [ "$JOB" = "$LOC" ]; then echo "  FAIL no job started (redirect: '$LOC')"; FAILED=1; else
  STATUS=""
  for _ in $(seq 1 150); do
    STATUS=$(curl -s -b "$JAR" "$URL/api/generate/$JOB")
    echo "$STATUS" | grep -q '"done":true' && break
    sleep 0.2
  done
  if echo "$STATUS" | grep -q '"done":true' && echo "$STATUS" | grep -q '"error":null'; then
    COURSE=$(echo "$STATUS" | python3 -c 'import json,sys; print(json.load(sys.stdin)["course_id"])')
    echo "  ok   job $JOB wrote $COURSE"
    check "/courses/$COURSE"
    curl -s -o /dev/null -b "$JAR" -c "$JAR" -X POST "$URL/courses/open/$COURSE"
    check /lessons
    grep -q "Fractions\|fraction" "$WORK/body" && echo "  ok   the generated course is open" || { echo "  FAIL the generated course is not open"; FAILED=1; }
    calls=$(curl -s -b "$JAR" "$URL/dev/ai?format=json" | python3 -c 'import json,sys; print(sum(1 for c in json.load(sys.stdin)["calls"] if c["status"] == 200 and c["purpose"].startswith("generate-")))')
    [ "$calls" -ge 2 ] && echo "  ok   the simulator answered $calls generate calls" || { echo "  FAIL the simulator logged $calls generate calls"; FAILED=1; }
  else
    echo "  FAIL generation did not finish cleanly: $STATUS"; FAILED=1
  fi
fi
check "/dev/reset" 303

if [ "$FAILED" = 1 ]; then echo "==> smoke test FAILED; server log:"; tail -30 "$WORK/server.log"; exit 1; fi
echo "==> smoke test passed"
[ "${KEEP:-0}" = 1 ] && echo "==> server still running at $URL (pid $PID, log $WORK/server.log)"
exit 0
