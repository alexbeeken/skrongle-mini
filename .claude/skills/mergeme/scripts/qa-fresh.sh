#!/usr/bin/env bash
# Did a PR change since its latest QA report checked it?
#
#   qa-fresh.sh <pr> [<pr> ...]
#
# One tab-separated line per PR:
#   number  head  checked_at  from  state  own_commits
#
# checked_at:  the commit QA checked: the "Checked at" line of the latest
#              "## QA: PR #<n>" comment, else `checked_at` in
#              ~/skrongle-demos/pr-<n>/manifest.json, else -
# from:        comment | manifest | -
# state:       current   the head is the commit QA checked
#              main-only new commits since QA, but they only merged main (or
#                        the PR's base branch) in
#              stale     the PR has its own new work since QA (listed after)
#              none      no "Checked at" anywhere, so this can't tell
# own_commits: the PR's own commits since QA, short shas joined by commas,
#              or - . "rewritten" when GitHub can't find the checked commit
#              (a rebase or force push), so everything counts as new.
#
# The rule is the "Stale" section of pr-qa-demo's references/manifest.md, and
# this does the same as pr-qa-demo's scripts/find-prs.sh:
#   own   = compare <base>...<head>: the PR's commits its base branch lacks
#   new   = compare <checked_at>...<head>; not found (rebase, force push)
#           means stale
#   a base merge is an own commit with two parents whose second parent is
#   not in own: it pulled main (or the base branch) in
#   stale = some commit is in own and new and is not a base merge
# One difference: a report with no Checked at (and no manifest) is "none"
# here, not stale by date, because mergeme doesn't block on it.
#
# Only REST calls (`gh api`), like find-prs.sh. It only reads.
#
# Test on any PR, even a merged one: QA_FRESH_AT=<sha> stands in for the
# checked commit and QA_FRESH_BASE=<sha> for the PR's base branch.
set -euo pipefail

[[ $# -gt 0 ]] || { echo "usage: qa-fresh.sh <pr> [<pr> ...]" >&2; exit 2; }

R='repos/{owner}/{repo}'
demos=${SKRONGLE_DEMOS:-$HOME/skrongle-demos}

for n in "$@"; do
  read -r head base < <(gh api "$R/pulls/$n" --jq '[.head.sha, .base.ref] | @tsv')
  base=${QA_FRESH_BASE:-$base}

  # Same match as find-prs.sh: the latest QA report, its last "Checked at".
  at=$(gh api --paginate --slurp "$R/issues/$n/comments?per_page=100" | jq -r --argjson n "$n" '
    [.[][]] | map(select((.body // "") | test("^## QA: PR #\($n)(\\D|$)"))) | last
    | if . == null then "" else
        ((.body // "") | [match("(?i)checked at\\W*([0-9a-f]{7,40})\\b").captures[0].string] | last // "")
      end')
  from=comment
  if [[ -n ${QA_FRESH_AT:-} ]]; then at=$QA_FRESH_AT from=test; fi
  if [[ -z $at ]]; then
    from=manifest
    at=$(jq -r '.checked_at // ""' "$demos/pr-$n/manifest.json" 2>/dev/null || true)
  fi
  if [[ -z $at ]]; then
    printf '%s\t%s\t-\t-\tnone\t-\n' "$n" "${head:0:8}"
    continue
  fi
  if [[ $head == "$at"* ]]; then
    printf '%s\t%s\t%s\t%s\tcurrent\t-\n' "$n" "${head:0:8}" "$at" "$from"
    continue
  fi

  own=$(gh api --paginate --slurp "$R/compare/$base...$head?per_page=100" \
        | jq -c '[.[].commits[] | {sha, parents: [.parents[].sha]}]')
  # A 404 means checked_at is gone from the branch: new stays null, so stale.
  new=$(gh api --paginate --slurp "$R/compare/$at...$head?per_page=100" 2>/dev/null \
        | jq -c '[.[].commits[].sha]' 2>/dev/null) || new=null

  own=$(jq -nr --argjson own "$own" --argjson new "${new:-null}" '
    if $new == null then "rewritten"
    else
      ($own | map(.sha)) as $own_shas
      | [ $own[]
          | select(.sha | IN($new[]))
          | select((.parents | length) > 1 and (.parents[1] | IN($own_shas[]) | not) | not)
          | .sha[0:8] ]
      | join(",")
    end')
  if [[ -z $own ]]; then
    printf '%s\t%s\t%s\t%s\tmain-only\t-\n' "$n" "${head:0:8}" "$at" "$from"
  else
    printf '%s\t%s\t%s\t%s\tstale\t%s\n' "$n" "${head:0:8}" "$at" "$from" "$own"
  fi
done
