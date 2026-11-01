#!/usr/bin/env bash
# How many agents a factory session may run at once, from the free disk.
# Every factory skill uses this, so the four sessions a rotation starts
# (mergeme, backlog-sweep, pr-qa-demo, qa) share the disk without being told
# about each other: the sum assumes all four are running.
#
#   budget.sh            # free=<GB> builders=<n> qa=<n>
#   budget.sh builders   # the number of backlog-sweep builders
#   budget.sh qa         # the number of pr-qa-demo QA agents
#   budget.sh total      # how many agents one run may start in all (#1314)
#
# A builder's or QA agent's worktree grows to 3-6 GB and the four sessions
# take about 6 GB each, so agents = (free GB - 40) / 6, split between
# builders (at most 4) and QA agents (at most 5). Under 16 GB free there is
# no room for an agent at all: both are 0 and the skill stops. Between 16
# and 46 GB each gets one. A full disk takes the CI runners down.
#
# builders and qa limit agents running at the same time. total limits how
# many one run starts in all: 10, or FACTORY_MAX_AGENTS. October's runs
# started 30-45 agents each, and those runs were most of the month's usage.
# The rest wait for the next rotation. Small haiku jobs (watching CI) don't
# count; the rules are in ../references/factory-rules.md.
set -euo pipefail
free=$(df -g / | awk 'NR==2 {print $4}')
if [ "$free" -lt 16 ]; then
  b=0; q=0
else
  agents=$(( (free - 40) / 6 ))
  b=$(( (agents + 1) / 2 )); [ "$b" -gt 4 ] && b=4; [ "$b" -lt 1 ] && b=1
  q=$(( agents - b ));       [ "$q" -gt 5 ] && q=5; [ "$q" -lt 1 ] && q=1
fi
t="${FACTORY_MAX_AGENTS:-10}"
case "$t" in ''|*[!0-9]*) echo "budget.sh: FACTORY_MAX_AGENTS must be a number" >&2; exit 2 ;; esac
case "${1:-}" in
  builders) echo "$b" ;;
  qa)       echo "$q" ;;
  total)    echo "$t" ;;
  "")       echo "free=$free builders=$b qa=$q total=$t" ;;
  *)        echo "usage: budget.sh [builders|qa|total]" >&2; exit 2 ;;
esac
