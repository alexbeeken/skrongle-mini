#!/usr/bin/env bash
# Was a PR's mergeme approval given before or after its latest QA report?
#
#   approval.sh <pr> [<pr> ...]
#
# One tab-separated line per PR:
#   number  verdict  labelled_at  qa_at  order
#
# verdict:      merge | not ready | none      the latest "## QA: PR #<n>" comment's verdict
# labelled_at:  when the mergeme label was last added (a GitHub issue event), or -
# qa_at:        when the latest QA report was posted, or -
# order:        label-after-qa   the engineer approved it knowing this report
#                                (a red card confirmed on the Review Desk)
#               qa-after-label   the report came after the approval: a re-QA
#                                (pr-fix, or pr-qa-demo on a stale PR). A
#                                "not ready" here is news the engineer hasn't
#                                seen, so the PR goes back to the desk
#               -                no label event or no report
#
# Only REST calls (`gh api`). It only reads.
set -euo pipefail
[[ $# -gt 0 ]] || { echo "usage: approval.sh <pr> [<pr> ...]" >&2; exit 2; }
R='repos/{owner}/{repo}'
for n in "$@"; do
  read -r verdict qa_at < <(gh api --paginate --slurp "$R/issues/$n/comments?per_page=100" | jq -r --argjson n "$n" '
    [.[][]] | map(select((.body // "") | test("^## QA: PR #\($n)(\\D|$)"))) | last
    | if . == null then "none\t-" else
        . as $c
        | (($c.body // "") | split("\n") | map(select(test("^\\W*verdict\\W*"; "i"))) | first // "") as $line
        | ($line | gsub("\\*"; "") | sub("(?i)^\\W*verdict\\W*"; "") | ascii_downcase) as $v
        | (if ($v | test("^not (ready|yet)") or test("not ready")) then "not ready"
           elif ($v | startswith("merge")) then "merge" else "none" end) + "\t" + $c.created_at
      end')
  labelled_at=$(gh api --paginate --slurp "$R/issues/$n/events?per_page=100" | jq -r '
    [.[][] | select(.event == "labeled" and (.label.name == "mergeme"))] | last | .created_at // "-"')
  order=-
  if [[ $labelled_at != - && $qa_at != - ]]; then
    if [[ $labelled_at > $qa_at ]]; then order=label-after-qa; else order=qa-after-label; fi
  fi
  printf '%s\t%s\t%s\t%s\t%s\n' "$n" "$verdict" "$labelled_at" "$qa_at" "$order"
done
