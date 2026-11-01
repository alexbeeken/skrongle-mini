#!/usr/bin/env bash
# Show a PR's state and its newest "needs work" request. Only reads GitHub.
#
#   request.sh <N>
#
# Prints "key  value" lines, then a line "---" and the request's text:
#
#   pr         1103
#   state      open | closed | merged
#   draft      true | false
#   head       <branch>            (push to it with git push origin HEAD:<branch>)
#   head_sha   <commit>
#   base       <branch>
#   mergeable  MERGEABLE | CONFLICTING | UNKNOWN
#   label      yes | no            (is "needs work" on the PR)
#   request    <comment url> | none
#   asked_at   <time> | -
#   answered   yes | no            (a "## Fixed" comment came after the request)
#   ---
#   <the text alexbeeken typed on the Review Desk>
#
# A request is a PR conversation comment by alexbeeken whose body is exactly
#   ## Needs work
#
#   <text>
#
#   <!-- review-desk:needs-work -->
# The newest one counts. A "## Fix stopped" reply does not answer it, so a
# rerun of /pr-fix tries again.
#
# REST only (`gh api`), like pr-qa-demo's find-prs.sh.
# Test without GitHub: REQUEST_JSON=<file with {pr, comments}> request.sh <N>
set -euo pipefail

n=${1:-}
if [[ ! $n =~ ^[0-9]+$ ]]; then
  echo "usage: request.sh <PR number>" >&2
  exit 64
fi
R=repos/alexbeeken/pooki

if [[ -n "${REQUEST_JSON:-}" ]]; then
  data=$(cat "$REQUEST_JSON")
else
  data=$(jq -n \
    --argjson pr "$(gh api "$R/pulls/$n")" \
    --argjson comments "$(gh api --paginate --slurp "$R/issues/$n/comments?per_page=100" | jq -c '[.[][]]')" \
    '{pr: $pr, comments: $comments}')
fi

jq -r '
  def clean: (. // "") | gsub("\r"; "") | sub("^\\s+"; "") | sub("\\s+$"; "");
  def is_request: .user.login == "alexbeeken"
    and (.body | clean | test("^## Needs work\\n[\\s\\S]*<!-- review-desk:needs-work -->$"));
  def text: .body | clean | sub("^## Needs work\\n"; "") | sub("<!-- review-desk:needs-work -->$"; "") | clean;
  .pr as $p
  | (.comments | sort_by(.id)) as $cs
  | ($cs | map(select(is_request)) | last) as $req
  | (if $req == null then false
     else $cs | any(.id > $req.id and (.body | clean | test("^## Fixed(\\s|$)"))) end) as $answered
  | "pr         \($p.number)",
    "state      \(if $p.merged_at != null then "merged" else $p.state end)",
    "draft      \($p.draft)",
    "head       \($p.head.ref)",
    "head_sha   \($p.head.sha)",
    "base       \($p.base.ref)",
    "mergeable  \(if $p.mergeable == true then "MERGEABLE" elif $p.mergeable == false then "CONFLICTING" else "UNKNOWN" end)",
    "label      \(if ($p.labels // [] | map(.name) | index("needs work")) then "yes" else "no" end)",
    "request    \($req.html_url // "none")",
    "asked_at   \($req.created_at // "-")",
    "answered   \(if $answered then "yes" else "no" end)",
    "---",
    (if $req == null then empty else ($req | text) end)
' <<<"$data"
