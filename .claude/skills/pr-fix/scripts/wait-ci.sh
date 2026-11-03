#!/usr/bin/env bash
# Wait for CI on one commit of a PR, then say how it went. Only reads GitHub.
# Run it in the background (it sleeps between looks).
#
#   wait-ci.sh <N> [commit]     # commit defaults to the PR's head now
#
# What it prints last, and the exit code:
#   pass                         0   every check finished and none failed
#   fail, then "<name> <url>"    1   one line per failed check
#   timeout                      2   still running after WAIT_CI_MINUTES (90)
#   none                         3   no check started within WAIT_CI_START_MINUTES (10)
#                                    ([skip ci] in the commit, or a conflict)
#
# WAIT_CI_INTERVAL (seconds, default 30) sets how often it looks.
set -euo pipefail

n=${1:-}
if [[ ! $n =~ ^[0-9]+$ ]]; then
  echo "usage: wait-ci.sh <PR number> [commit]" >&2
  exit 64
fi
R=repos/alexbeeken/skrongle-mini
sha=${2:-$(gh api "$R/pulls/$n" --jq .head.sha)}
interval=${WAIT_CI_INTERVAL:-30}
limit=$(( ${WAIT_CI_MINUTES:-90} * 60 ))
first=$(( ${WAIT_CI_START_MINUTES:-10} * 60 ))
start=$SECONDS
echo "waiting for CI on PR #$n at ${sha:0:8}"

while :; do
  # One object per check: name, done, failed, url. Check runs and commit statuses both count.
  checks=$(jq -n \
    --argjson runs "$(gh api --paginate --slurp "$R/commits/$sha/check-runs?per_page=100" | jq -c '[.[].check_runs[]]')" \
    --argjson status "$(gh api "$R/commits/$sha/status")" '
    # A rerun adds a second run with the same name; the newest one counts.
    ($runs | group_by(.name) | map(max_by(.id))
      | map({name, done: (.status == "completed"),
             failed: ((.conclusion // "") | test("failure|timed_out|cancelled|action_required")),
             url: .html_url}))
    + ($status.statuses | map({name: .context, done: (.state != "pending"),
             failed: (.state | test("failure|error")), url: .target_url}))')
  total=$(jq length <<<"$checks")
  running=$(jq 'map(select(.done | not)) | length' <<<"$checks")
  elapsed=$(( SECONDS - start ))

  if (( total == 0 )); then
    if (( elapsed >= first )); then echo none; exit 3; fi
  elif (( running == 0 )); then
    if jq -e 'any(.failed)' <<<"$checks" >/dev/null; then
      echo fail
      jq -r '.[] | select(.failed) | "\(.name) \(.url // "-")"' <<<"$checks"
      exit 1
    fi
    echo pass
    exit 0
  fi
  if (( elapsed >= limit )); then echo timeout; exit 2; fi
  sleep "$interval"
done
