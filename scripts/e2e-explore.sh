#!/usr/bin/env bash
# Exploratory testing: Jev explores the app as a user profile from
# e2e/personas/, collects bugs (exact signals) and unclear or frustrating
# moments (Jev judgements), and writes a report that proposes new e2e tests.
# Same server setup as scripts/e2e.sh. Exits 0 and reports, unless --fail-on.
#
#   scripts/e2e-explore.sh --list                          # the profiles
#   scripts/e2e-explore.sh impatient                       # every goal of that profile
#   scripts/e2e-explore.sh classroom --goal "find help with ser vs estar"   # a goal of your own
#   scripts/e2e-explore.sh classroom --goal 1              # the profile's goal 1 (0-based)
#   scripts/e2e-explore.sh testing-bot --budget 80         # cap the steps (and so the cost)
#   scripts/e2e-explore.sh accurate --max-calls 100        # cap the Jev calls (default 300)
#   scripts/e2e-explore.sh impatient --wander 2            # then 2 sections Jev picks itself
#   scripts/e2e-explore.sh --persona-file draft.ts         # a profile not in the folder yet
#   scripts/e2e-explore.sh --replay e2e/results/explore/<run>/report.json   # rerun a route, no Jev
#   scripts/e2e-explore.sh impatient --fail-on bug         # exit 1 when there is a bug finding
#   JEV_DEBUG=1 scripts/e2e-explore.sh accurate            # also print each step's options
#
# Reports: e2e/results/explore/<profile>-<timestamp>/report.md, report.json, shots/.
# See docs/testing.md, "Exploratory testing".
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

usage() { sed -n '2,19p' "$0" | sed 's/^# \{0,1\}//'; exit "${1:-0}"; }
abspath() { case "$1" in /*) echo "$1" ;; *) echo "$PWD/$1" ;; esac; }

PROFILE="" GOAL="" BUDGET="" MAXCALLS="" WANDER="" FAILON="" FILE="" REPLAY=""
while [ $# -gt 0 ]; do
  case "$1" in
    -h|--help) usage ;;
    --list)
      for f in "$ROOT"/e2e/personas/*.ts; do
        id=$(basename "$f" .ts)
        case "$id" in _*) continue ;; esac
        who=$(sed -n -E 's/^  who: "(.*)",$/\1/p' "$f" | head -1)
        printf '%-14s %s\n' "$id" "$who"
      done
      exit 0 ;;
    --goal) GOAL="${2:?--goal needs a goal in words or a number}"; shift ;;
    --budget) BUDGET="${2:?--budget needs a number of steps}"; shift ;;
    --max-calls) MAXCALLS="${2:?--max-calls needs a number}"; shift ;;
    --wander)
      # an optional count: --wander 3; default 2
      if [[ "${2:-}" =~ ^[0-9]+$ ]]; then WANDER="$2"; shift; else WANDER=2; fi ;;
    --fail-on) FAILON="${2:?--fail-on needs kinds: bug,content,unclear,frustrating,testability}"; shift ;;
    --persona-file) FILE="$(abspath "${2:?--persona-file needs a path}")"; shift ;;
    --replay) REPLAY="$(abspath "${2:?--replay needs a report.json}")"; shift ;;
    -*) echo "unknown option $1"; usage 2 ;;
    *) [ -z "$PROFILE" ] || { echo "one profile at a time (got $PROFILE and $1)"; exit 2; }; PROFILE="$1" ;;
  esac
  shift
done

if [ -n "$REPLAY" ]; then
  [ -f "$REPLAY" ] || { echo "no such report: $REPLAY"; exit 2; }
elif [ -n "$FILE" ]; then
  [ -f "$FILE" ] || { echo "no such persona file: $FILE"; exit 2; }
elif [ -z "$PROFILE" ]; then
  usage 2
elif [ ! -f "$ROOT/e2e/personas/$PROFILE.ts" ]; then
  echo "no profile \"$PROFILE\" in e2e/personas/; try: $(cd "$ROOT/e2e/personas" && ls *.ts | sed 's/\.ts$//' | grep -v '^_' | tr '\n' ' ')"
  exit 2
fi
[[ -z "$BUDGET$MAXCALLS" || "$BUDGET$MAXCALLS" =~ ^[0-9]+$ ]] || { echo "--budget and --max-calls take numbers"; exit 2; }

cd "$ROOT"
# A replay needs no Jev; an exploration does, so a missing key is an error here, not a skip.
if [ -z "$REPLAY" ]; then
  KEY="${TYPESAFE_API_KEY:-${JEV_API_KEY:-}}"
  for f in .env e2e/.env; do
    [ -z "$KEY" ] && [ -f "$f" ] && KEY=$(sed -n -E 's/^(TYPESAFE_API_KEY|JEV_API_KEY)=//p' "$f" | tr -d "\"' " | head -1)
  done
  if [ -z "$KEY" ]; then
    echo "No Jev key. Create one at https://console.typesafe.ai/keys and put it in ./.env or e2e/.env:"
    echo "  TYPESAFE_API_KEY=..."
    exit 2
  fi
fi

COMMIT="$(git rev-parse --short HEAD 2>/dev/null || echo unknown)"
git diff --quiet HEAD 2>/dev/null || COMMIT="$COMMIT+dirty"
export E2E_COMMIT="$COMMIT"
export JEV_EXPLORE="$PROFILE" JEV_EXPLORE_FILE="$FILE" JEV_EXPLORE_REPLAY="$REPLAY" JEV_EXPLORE_GOAL="$GOAL" \
  JEV_EXPLORE_BUDGET="$BUDGET" JEV_EXPLORE_MAX_CALLS="$MAXCALLS" JEV_EXPLORE_WANDER="$WANDER" JEV_EXPLORE_FAIL_ON="$FAILON"
# Only the explore spec, one test; --reporter list keeps the step logs and the summary on screen.
exec scripts/e2e.sh --grep "@explore" --reporter list
