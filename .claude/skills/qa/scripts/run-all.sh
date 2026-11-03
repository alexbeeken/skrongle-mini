#!/usr/bin/env bash
# Start every Jev QA script of the repo, each in the background with its own
# log, and print where the logs and reports go. The scripts start their own
# servers on free ports, so they run side by side. Nothing here needs the dev
# server. Run from the repo root; the caller reads the logs when they end.
#
#   run-all.sh <logdir>                      # explore (4 profiles) + playthroughs (course + test course) + content checks
#   ONLY="explore" run-all.sh <logdir>       # some groups: explore, playthrough, test-course, journeys, content
#   LESSONS=5 run-all.sh <logdir>            # cap the playthroughs (default: the whole course)
#   PROFILES="impatient accurate" run-all.sh <logdir>   # some explore profiles (default: all in e2e/personas/)
#   WANDER=2 run-all.sh <logdir>             # let each profile also wander 2 sections (default 1)
#   DRY=1 run-all.sh <logdir>                # only print what would run
set -euo pipefail
LOG="${1:?run-all.sh <logdir>}"
mkdir -p "$LOG"
ONLY="${ONLY:-explore playthrough test-course journeys content}"
WANDER="${WANDER:-1}"
PIDS=()
run() { # $1 name, rest: command. Each run gets its own database (scripts/e2e.sh honours E2E_DB).
  local name="$1"; shift
  echo "==> $name: $* (log: $LOG/$name.log)"
  [ -n "${DRY:-}" ] && return 0
  ( E2E_DB="$LOG/$name.db" "$@" ) >"$LOG/$name.log" 2>&1 &
  PIDS+=("$!:$name")
}
# The Playwright runs (explore, journeys) all write e2e/results/report and
# last-run.json, so they go one after another in a single background chain;
# their own report folders (e2e/results/explore/<profile>-<timestamp>/) are
# per run and survive. The playthroughs have their own output folders.
PW=()
queue() { PW+=("$(printf '%q ' "$@")"); }
run_queue() {
  [ ${#PW[@]} -gt 0 ] || return 0
  echo "==> playwright chain, one after another: ${#PW[@]} runs (log: $LOG/playwright-chain.log)"
  [ -n "${DRY:-}" ] && { printf '    %s\n' "${PW[@]}"; return 0; }
  ( for c in "${PW[@]}"; do echo "==> $c"; E2E_DB="$LOG/playwright.db" eval "$c" || echo "==> exit $? for: $c"; done ) >"$LOG/playwright-chain.log" 2>&1 &
  PIDS+=("$!:playwright-chain")
}
has() { case " $ONLY " in *" $1 "*) return 0 ;; *) return 1 ;; esac; }

# One build first, so the parallel runs don't all compile at once.
if [ -z "${DRY:-}" ]; then
  cargo build 2>&1 | tail -1
  [ -d e2e/node_modules ] || npm --prefix e2e ci >"$LOG/npm-ci.log" 2>&1
fi

if has explore; then
  for p in ${PROFILES:-$(scripts/e2e-explore.sh --list | awk '{print $1}')}; do
    queue scripts/e2e-explore.sh "$p" --wander "$WANDER"
  done
fi
# The two playthrough scripts both write ./playthrough-<mode>.db in the
# checkout and register an account named by the second, so they run one
# after the other in one chain, never side by side.
PT=()
has playthrough && PT+=("scripts/jev-playthrough-all.sh ${LESSONS:+--lessons $LESSONS}")
has test-course && PT+=("scripts/jev-playthrough-test-course.sh ${LESSONS:+--lessons $LESSONS}")
if [ ${#PT[@]} -gt 0 ]; then
  echo "==> playthrough chain, one after another: ${PT[*]} (log: $LOG/playthrough-chain.log)"
  if [ -z "${DRY:-}" ]; then
    ( for c in "${PT[@]}"; do echo "==> $c"; eval "$c" || echo "==> exit $? for: $c"; done ) >"$LOG/playthrough-chain.log" 2>&1 &
    PIDS+=("$!:playthrough-chain")
  fi
fi
if has journeys; then
  queue scripts/e2e-jev.sh
fi
run_queue
if has content; then
  run fact-check scripts/fact-check.sh
fi

echo
echo "running: ${PIDS[*]:-nothing (dry run)}"
echo "wait for them with: while pgrep -qf 'e2e-explore|jev-playthrough|e2e-jev|fact-check'; do sleep 30; done"
echo "reports: e2e/results/explore/<profile>-<timestamp>/report.md, e2e/results/playthrough/<timestamp>-{all,test-course}/summary.md,"
echo "         $LOG/playwright-chain.log (explore summaries and the journeys' step logs)"
