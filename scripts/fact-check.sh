#!/usr/bin/env bash
# Fact check the built-in courses with Jev: every sentence, lesson line,
# quiz question, sort item, spot, conjugation and vocabulary entry. The
# report lists what is wrong, where (the word in «»), and the fix when Jev
# picks one. A few US cents for everything, cached, so rerun it after edits.
#
#   scripts/fact-check.sh                          # every course in data/courses/
#   scripts/fact-check.sh spanish                  # one course
#   scripts/fact-check.sh spanish --lesson a2/     # lessons whose path contains the text
#   scripts/fact-check.sh --errors                 # only ERRORs (each with hard evidence)
#   scripts/fact-check.sh --why                    # also each finding's ok-list line
#   scripts/fact-check.sh --calibrate              # measure it: planted mistakes caught, false flags
#   scripts/fact-check.sh --help                   # all options
#
# The key comes from TYPESAFE_API_KEY (or JEV_API_KEY), ./.env or e2e/.env.
# A finding checked and found right goes in data/courses/<id>/fact-check-ok.txt.
# See docs/testing.md, "Fact check".
set -euo pipefail
cd "$(dirname "$0")/.."
case " $* " in *" -h "*|*" --help "*) sed -n '2,18p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;; esac

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

# Node runs the TypeScript as is (type stripping, Node 22.18+); only the Jev SDK is needed, not the browsers.
[ -d e2e/node_modules/@typesafe-ai/sdk ] || (cd e2e && npm ci --no-fund --no-audit >/dev/null)
exec node e2e/fact-check/index.ts "$@"
