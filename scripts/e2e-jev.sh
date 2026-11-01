#!/usr/bin/env bash
# Run the Jev tests: the journeys in e2e/journeys.ts and the judged specs.
# Unlike scripts/e2e.sh, a missing key is an error here, not a skip.
#
#   scripts/e2e-jev.sh                          # every journey and judged spec
#   scripts/e2e-jev.sh find-generate-form       # one journey, by id
#   scripts/e2e-jev.sh open-lesson start-sprint  # several journeys
#   scripts/e2e-jev.sh --judged                 # only the judged specs
#   scripts/e2e-jev.sh --explore impatient      # same as scripts/e2e-explore.sh impatient
#   JEV_DEBUG=1 scripts/e2e-jev.sh open-lesson  # also print what Jev is shown each step
#
# The key comes from TYPESAFE_API_KEY (or JEV_API_KEY), ./.env or e2e/.env.
# Results: the step logs print as it runs; e2e/results/journeys/<id>.json
# per journey; npx --prefix e2e playwright show-report e2e/results/report.
set -euo pipefail
cd "$(dirname "$0")/.."

KEY="${TYPESAFE_API_KEY:-${JEV_API_KEY:-}}"
for f in .env e2e/.env; do
  [ -z "$KEY" ] && [ -f "$f" ] && KEY=$(sed -n -E 's/^(TYPESAFE_API_KEY|JEV_API_KEY)=//p' "$f" | tr -d "\"' " | head -1)
done
if [ -z "$KEY" ]; then
  echo "No Jev key. Create one at https://console.typesafe.ai/keys and put it in ./.env or e2e/.env:"
  echo "  TYPESAFE_API_KEY=..."
  exit 2
fi

case "${1:-}" in
  "") GREP="@jev" ;;
  --judged) GREP="judged .*@jev" ;;
  --explore)
    # Exploratory runs have their own runner now (issue #40); the old goal argument becomes --goal.
    shift
    [ -n "${1:-}" ] || { echo "usage: scripts/e2e-explore.sh <profile> [--goal …] (see --help)"; exit 2; }
    P="$1"; shift
    echo "note: --explore moved to scripts/e2e-explore.sh"
    if [ -n "${1:-}" ] && [[ "$1" != -* ]]; then exec scripts/e2e-explore.sh "$P" --goal "$@"; fi
    exec scripts/e2e-explore.sh "$P" "$@" ;;
  -*) echo "unknown option $1 (try a journey id or --judged; explorations: scripts/e2e-explore.sh)"; exit 2 ;;
  *) GREP="journey ($(IFS="|"; echo "$*"))(\\s|$)" ;;
esac
exec scripts/e2e.sh --grep "$GREP"
