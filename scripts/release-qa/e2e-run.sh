#!/usr/bin/env bash
# The browser-spec container of the rig's `e2e` profile (compose.yaml, #41):
# runs the Playwright `release` projects (e2e/specs/release) against the
# release server in the e2e-server-<arch> container. The repo is mounted
# read-only at /repo; the specs read e2e/, data/courses and
# src/test_course.rs relative to their own folder, so those are copied to a
# scratch tree. Owner link: from the server's log (E2E_SERVER_LOG), which is
# all a self-hoster has. The HTML report and traces land in /out. With
# E2E_AI_SIM_URL (compose.yaml, profile simulator) ai.spec.ts reads the AI
# simulator's call log there; without it that spec skips itself.
set -euo pipefail
: "${E2E_BASE_URL:?}" "${E2E_SERVER_LOG:?}"

W=/work
mkdir -p "$W/src" "$W/e2e"
tar -C /repo/e2e --exclude=./node_modules --exclude=./results -cf - . | tar -C "$W/e2e" -xf -
cp -r /repo/data "$W/data"
cp /repo/src/test_course.rs "$W/src/"

LINK=""
for _ in $(seq 1 50); do
  LINK=$(sed -n 's/.*Owner link.*\(http:\/\/[^ ]*invite=[A-Za-z0-9_-]*\).*/\1/p' "$E2E_SERVER_LOG" | head -1)
  [ -n "$LINK" ] && break
  sleep 0.2
done
[ -n "$LINK" ] || { echo "no owner link in the server log:"; cat "$E2E_SERVER_LOG"; exit 1; }

# The AI simulator (profile simulator with E2E_SIM=1): this container shares the server's
# network namespace, so it is on 127.0.0.1 here, as for the release server. Asked for and
# not answering is a failure, not a silent skip of ai.spec.ts.
if [ -n "${E2E_AI_SIM_URL:-}" ]; then
  if ! node -e 'fetch(process.argv[1]).then((r) => process.exit(r.ok ? 0 : 1), () => process.exit(1))' "$E2E_AI_SIM_URL/dev/ai?format=json"; then
    echo "E2E_SIM=1 but no AI simulator answers at $E2E_AI_SIM_URL: start with --profile simulator (matrix.sh: E2E=1 SIM=1)"
    exit 1
  fi
  echo "==> AI simulator at $E2E_AI_SIM_URL: specs/release/ai.spec.ts runs"
else
  echo "==> no AI simulator (E2E_SIM unset): specs/release/ai.spec.ts skips itself"
fi

cd "$W/e2e"
npm ci --no-fund --no-audit --loglevel=error
STATUS=0
E2E_BASE_URL="$E2E_BASE_URL" E2E_OWNER_LINK="$LINK" E2E_AI_SIM_URL="${E2E_AI_SIM_URL:-}" npx playwright test --project release || STATUS=$?
rm -rf /out/report /out/artifacts
[ -d results/report ] && cp -r results/report /out/report
[ -d results/artifacts ] && cp -r results/artifacts /out/artifacts
if [ "$STATUS" != 0 ]; then
  echo "release e2e FAILED. Report: results/release-qa/e2e/report/index.html; server log tail:"
  tail -20 "$E2E_SERVER_LOG"
fi
exit "$STATUS"
