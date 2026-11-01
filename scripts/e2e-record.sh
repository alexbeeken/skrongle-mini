#!/usr/bin/env bash
# Record a smoke spec with Jev's help (issue #48): Jev walks toward a goal
# once on a seeded server, the recorder notes each step with the testids
# around it (answers come from the answer key, not from Jev), and a
# converter writes a draft spec. Review it, then copy it into
# e2e/specs/smoke/. The spec itself never calls Jev. Same server setup as
# scripts/e2e.sh (fresh database, E2E_SEED, LLM fixtures, the test course).
#
#   scripts/e2e-record.sh --start /register --id k-first-lesson \
#     --goal "Create an account, open the Spanish course, read the first lesson and pass its reading check"
#   scripts/e2e-record.sh --course test-course --start /sprint --goal "…" --id d-sort   # any start: a fresh account first
#   scripts/e2e-record.sh … --budget 30            # at most 30 Jev steps (default 20)
#   scripts/e2e-record.sh --convert e2e/results/record/<id>/trace.json   # redo the draft, no Jev, no server
#
# Output: e2e/results/record/<id>/<id>.spec.ts and trace.json. See
# docs/testing.md, "Recording a smoke spec".
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
usage() { sed -n '2,17p' "$0" | sed 's/^# \{0,1\}//'; exit "${1:-0}"; }

START="/" GOAL="" ID="" COURSE="spanish" BUDGET="" CONVERT=""
while [ $# -gt 0 ]; do
  case "$1" in
    -h|--help) usage ;;
    --start) START="${2:?--start needs a path}"; shift ;;
    --goal) GOAL="${2:?--goal needs the goal in words}"; shift ;;
    --id) ID="${2:?--id needs a kebab-case id}"; shift ;;
    --course) COURSE="${2:?--course needs a course id}"; shift ;;
    --budget) BUDGET="${2:?--budget needs a number of steps}"; shift ;;
    --convert) CONVERT="${2:?--convert needs a trace.json}"; shift ;;
    *) echo "unknown argument $1"; usage 2 ;;
  esac
  shift
done

if [ -n "$CONVERT" ]; then
  [ -f "$CONVERT" ] || { echo "no such trace: $CONVERT"; exit 2; }
  OUT="$(dirname "$CONVERT")/$(basename "$(dirname "$CONVERT")").spec.ts"
  node "$ROOT/e2e/record/convert.ts" --from "$CONVERT" --out "$OUT"
  echo "==> draft: $OUT"
  exit 0
fi

[ -n "$GOAL" ] && [ -n "$ID" ] || usage 2
[[ "$ID" =~ ^[a-z0-9][a-z0-9-]*$ ]] || { echo "--id must be kebab-case (it names the spec file)"; exit 2; }
[[ -z "$BUDGET" || "$BUDGET" =~ ^[0-9]+$ ]] || { echo "--budget takes a number"; exit 2; }

cd "$ROOT"
KEY="${TYPESAFE_API_KEY:-${JEV_API_KEY:-}}"
for f in .env e2e/.env; do
  [ -z "$KEY" ] && [ -f "$f" ] && KEY=$(sed -n -E 's/^(TYPESAFE_API_KEY|JEV_API_KEY)=//p' "$f" | tr -d "\"' " | head -1)
done
if [ -z "$KEY" ]; then
  echo "No Jev key. Create one at https://console.typesafe.ai/keys and put it in ./.env or e2e/.env:"
  echo "  TYPESAFE_API_KEY=..."
  exit 2
fi
export TYPESAFE_API_KEY="$KEY"

export E2E_RECORD_ID="$ID" E2E_RECORD_GOAL="$GOAL" E2E_RECORD_START="$START" E2E_RECORD_COURSE="$COURSE" E2E_RECORD_BUDGET="$BUDGET"
exec scripts/e2e.sh --grep "@record" --reporter list
