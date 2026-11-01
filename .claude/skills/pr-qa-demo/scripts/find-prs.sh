#!/usr/bin/env bash
# List open PRs and what each one lacks: a QA report (a comment starting
# "## QA: PR #<n>") and/or a demo (~/skrongle-demos/pr-<n>/manifest.json).
# From before that folder existed, a "## Demo" section in the body or the
# "<!-- qa-page -->" link line of a claude.ai QA page also counts as a demo.
#
#   find-prs.sh            # tab-separated: number head base mergeable checks has_qa has_demo needs draft checked_at title
#
# checks:     pass | fail (+ failing check names) | pending | none
# needs:      both | qa | demo | stale | skip | approved | -
#             stale    = the PR has its own new work since the commit the latest
#                        QA report checked, so QA and the demo both need doing
#                        again (also when it is labelled mergeme: the approval
#                        was for the code QA checked)
#             skip     = labelled "needs work": its fix agent re-runs QA itself
#             approved = labelled "mergeme" and not stale: the engineer approved
#                        it on the Review Desk and a mergeme run lands it
# checked_at: the latest QA report's "Checked at" commit, or - if it has none
#
# The stale rule (references/manifest.md says the same):
#   own   = the PR's commits that its base branch doesn't have
#           (compare <base>...<head>)
#   new   = own commits after checked_at (in compare <checked_at>...<head>)
#   stale = some new commit is not a base merge. A base merge is a merge
#           commit whose second parent is not an own commit, i.e. it pulled
#           main (or the PR's base branch) in. Main-merge sweeps never make
#           a PR stale; merging another branch in does.
# If checked_at is no longer in the branch (a rebase or force push), or
# GitHub can't compare, the PR is stale. A report from before the "Checked at" line uses the commit dates
# instead: own commits newer than the report, minus base merges.
# Cost: two extra calls for a PR whose head moved since QA, one for an old
# report, none otherwise.
#
# Only REST calls (`gh api`): cloud sessions reach GitHub through a proxy that
# refuses GraphQL, which `gh pr list` uses. It only reads.
#
# Test without GitHub: FIND_PRS_JSON=<file of {pr, comments, runs, statuses,
# own, new, has_dir} objects> find-prs.sh runs only the last step. own is
# [{sha, parents: [sha], date}] or null; new is [sha], or null when
# checked_at was not found.
set -euo pipefail

R='repos/{owner}/{repo}'
demos=${SKRONGLE_DEMOS:-$HOME/skrongle-demos}

# jq helpers: the latest QA report on PR $n, and its "Checked at" commit.
# shellcheck disable=SC2016
checked_at_jq='
  def latest_qa($n): map(select((.body // "") | test("^## QA: PR #\($n)(\\D|$)"))) | last;
  def checked_at: (.body // "") | [match("(?i)checked at\\W*([0-9a-f]{7,40})\\b").captures[0].string] | last;'

collect() {
  gh api --paginate "$R/pulls?state=open&per_page=100" \
    --jq '.[] | [.number, .head.sha, .base.ref] | @tsv' \
    | while IFS=$'\t' read -r n sha base; do
        comments=$(gh api --paginate --slurp "$R/issues/$n/comments?per_page=100" | jq -c '[.[][]]')
        at=$(jq -r --argjson n "$n" "$checked_at_jq"' latest_qa($n) | if . == null then "none" else (checked_at // "old") end' <<<"$comments")
        # The commit lists only matter when the head moved since the report.
        own=null new=null
        if [[ "$at" != none && "$sha" != "$at"* ]]; then
          own=$(gh api --paginate --slurp "$R/compare/$base...$sha?per_page=100" \
                | jq -c '[.[].commits[] | {sha, parents: [.parents[].sha], date: .commit.committer.date}]') || own=null
          if [[ "$at" != old ]]; then
            # A 404 means checked_at is gone from the branch: new stays null, so stale.
            new=$(gh api --paginate --slurp "$R/compare/$at...$sha?per_page=100" 2>/dev/null \
                  | jq -c '[.[].commits[].sha]') || new=null
          fi
        fi
        has_dir=false
        [[ -f "$demos/pr-$n/manifest.json" ]] && has_dir=true
        # mergeable is null until GitHub has worked it out; the old GraphQL
        # field said UNKNOWN then too.
        jq -n \
          --argjson pr "$(gh api "$R/pulls/$n")" \
          --argjson comments "$comments" \
          --argjson runs "$(gh api --paginate --slurp "$R/commits/$sha/check-runs?per_page=100")" \
          --argjson status "$(gh api "$R/commits/$sha/status")" \
          --argjson own "$own" \
          --argjson new "${new:-null}" \
          --argjson has_dir "$has_dir" \
          '{pr: $pr, comments: $comments, runs: [$runs[].check_runs[]], statuses: $status.statuses,
            own: $own, new: $new, has_dir: $has_dir}'
      done
}

if [[ -n "${FIND_PRS_JSON:-}" ]]; then cat "$FIND_PRS_JSON"; else collect; fi \
  | jq -r "$checked_at_jq"'
    .pr as $p
    | (.comments | latest_qa($p.number)) as $report
    | ($report != null) as $qa
    | ($report | if . == null then null else checked_at end) as $at
    | ((.own // []) | map(.sha)) as $own_shas
    | ((.own // []) | map(select((.parents | length) > 1 and (.parents[1] | IN($own_shas[]) | not)) | .sha)) as $base_merges
    | (if ($qa | not) then false
       elif $at != null and ($p.head.sha | startswith($at)) then false
       elif .own == null or ($at != null and .new == null) then true
       else
         (if $at != null then .new else null end) as $new
         | [(.own // [])[]
            | select(if $new != null then (.sha | IN($new[])) else .date > $report.created_at end)
            | select(.sha | IN($base_merges[]) | not)]
         | length > 0
       end) as $stale
    | (.has_dir or (($p.body // "") | test("(?m)^## Demo\\s*$|<!-- qa-page -->"))) as $demo
    | ((.runs | map({name, done: (.status == "completed"),
                     failed: ((.conclusion // "") | test("failure|timed_out|cancelled|action_required"))}))
       + (.statuses | map({name: .context, done: (.state != "pending"),
                           failed: (.state | test("failure|error"))}))) as $c
    | ($c | map(select(.failed)) | map(.name)) as $failed
    | (if ($c | length) == 0 then "none"
       elif ($failed | length) > 0 then "fail:" + ($failed | join(","))
       elif ($c | map(select(.done | not)) | length) > 0 then "pending"
       else "pass" end) as $checks
    | (if $p.mergeable == true then "MERGEABLE" elif $p.mergeable == false then "CONFLICTING" else "UNKNOWN" end) as $mergeable
    | ($p.labels // [] | map(.name)) as $labels
    | (if ($labels | index("needs work")) then "skip"
       elif $stale then "stale"
       elif ($labels | index("mergeme")) then "approved"
       elif ($qa | not) and ($demo | not) then "both" elif ($qa | not) then "qa" elif ($demo | not) then "demo" else "-" end) as $needs
    | [$p.number, $p.head.ref, $p.base.ref, $mergeable, $checks, $qa, $demo, $needs, $p.draft, ($at // "-"), $p.title]
    | @tsv'
