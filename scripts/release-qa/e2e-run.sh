#!/usr/bin/env bash
# The browser-spec container of the rig's `e2e` profile (compose.yaml, #41):
# runs the Playwright `release` projects (e2e/specs/release) against the
# release server in the e2e-server-<arch> container. The repo is mounted
# read-only at /repo; the specs read e2e/, data/courses and
# src/test_course.rs relative to their own folder, so those are copied to a
# scratch tree. Owner link: from the server's log (E2E_SERVER_LOG), which is
# all a self-hoster has. The HTML report and traces land in /out.
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

cd "$W/e2e"
npm ci --no-fund --no-audit --loglevel=error
STATUS=0
E2E_BASE_URL="$E2E_BASE_URL" E2E_OWNER_LINK="$LINK" npx playwright test --project release || STATUS=$?
rm -rf /out/report /out/artifacts
[ -d results/report ] && cp -r results/report /out/report
[ -d results/artifacts ] && cp -r results/artifacts /out/artifacts
if [ "$STATUS" != 0 ]; then
  echo "release e2e FAILED. Report: results/release-qa/e2e/report/index.html; server log tail:"
  tail -20 "$E2E_SERVER_LOG"
fi
exit "$STATUS"
