#!/usr/bin/env bash
# Calibrate the AI content check's stage 1 (#1036, src/content_check.rs):
# send the planted and ordinary lessons of
# data/test-fixtures/content-check/calibration.json, then every part of the
# built-in Spanish and French courses (as if they were made courses), to the
# real Jev, and print each lesson's scores, the cost, and caught / missed /
# false flags at each threshold. Pick content_check::THRESHOLD from it and
# write the numbers in docs/testing.md, "Content check calibration".
#
#   scripts/content-check-calibrate.sh     # needs TYPESAFE_API_KEY (the shell, ./.env or e2e/.env); about $0.04 and two minutes
set -euo pipefail
cd "$(dirname "$0")/.."
KEY="${TYPESAFE_API_KEY:-}"
for f in .env e2e/.env; do
  [ -z "$KEY" ] && [ -f "$f" ] && KEY=$(sed -n -E 's/^TYPESAFE_API_KEY=//p' "$f" | tr -d "\"' " | head -1)
done
[ -n "$KEY" ] || { echo "no Jev key: set TYPESAFE_API_KEY (or put it in ./.env or e2e/.env)" >&2; exit 2; }
TYPESAFE_API_KEY="$KEY"
export TYPESAFE_API_KEY
exec cargo test --quiet content_check::tests::calibrate -- --ignored --nocapture --exact
