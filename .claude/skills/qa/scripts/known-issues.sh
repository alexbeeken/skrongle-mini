#!/usr/bin/env bash
# Everything already filed, so a QA run doesn't file it twice: every open
# issue, plus the ones closed in the last --days (default 14), one per line.
#
#   known-issues.sh              # tab-separated: number state labels title
#   known-issues.sh --days 30    # closed ones from the last 30 days too
#   known-issues.sh | grep -i overflow    # look for a finding before filing it
set -euo pipefail
DAYS=14
case "${1:-}" in --days) DAYS="${2:?--days needs a number}" ;; -h|--help) sed -n '2,7p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;; esac
SINCE=$(date -u -v-"${DAYS}"d +%Y-%m-%d 2>/dev/null || date -u -d "-${DAYS} days" +%Y-%m-%d)
fmt='.[] | [.number, .state, ([.labels[].name] | join(",")), .title] | @tsv'
gh issue list --state open --limit 500 --json number,state,labels,title | jq -r "$fmt"
gh issue list --state closed --limit 500 --search "closed:>=$SINCE" --json number,state,labels,title | jq -r "$fmt"
