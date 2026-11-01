#!/usr/bin/env bash
# Check and publish a PR's demo folder (the format is references/manifest.md).
#
#   demo-folder.sh check <dir>             # is <dir>/manifest.json valid, and are its files there?
#   demo-folder.sh publish <N> <staging>   # check <staging>, then make it ~/skrongle-demos/pr-<N>/
#
# publish replaces everything in ~/skrongle-demos/pr-<N>/ with the files the
# manifest names, and nothing else. It refuses if the manifest's "pr" is not
# <N>. Set SKRONGLE_DEMOS to use another demos folder (for tests).
set -euo pipefail

demos=${SKRONGLE_DEMOS:-$HOME/skrongle-demos}

# Prints one line per problem; prints nothing when the manifest is fine.
problems() {
  local dir=$1
  if [[ ! -f "$dir/manifest.json" ]]; then
    echo "no manifest.json in $dir"
    return
  fi
  if ! jq -e 'type == "object"' "$dir/manifest.json" >/dev/null 2>&1; then
    echo "manifest.json is not a JSON object"
    return
  fi
  jq -r '
    def need(f; t): if has(f) | not then "missing \(f)"
                    elif (.[f] | type) != t then "\(f) should be a \(t)" else empty end;
    def opt(f; t): if has(f) and (.[f] | type) != t then "\(f) should be a \(t)" else empty end;
    def plain(f): if (f | type) == "string" and (f | test("/|^\\.\\.?$")) then "\(f) should be a file name, not a path" else empty end;
    (if has("schema") and .schema != 1 then "schema should be 1" else empty end),
    need("pr"; "number"),
    (if has("issue") | not then "missing issue"
     elif (.issue | type) != "number" and .issue != null then "issue should be a number or null" else empty end),
    need("title"; "string"), need("why"; "string"),
    (if has("verdict") | not then "missing verdict"
     elif (.verdict | IN("merge", "not ready")) | not then "verdict should be \"merge\" or \"not ready\"" else empty end),
    opt("verdict_note"; "string"),
    (if has("checked_at") | not then "missing checked_at"
     elif (.checked_at | type) != "string" or (.checked_at | test("^[0-9a-f]{7,40}$") | not)
       then "checked_at should be a commit hash (8 hex characters)" else empty end),
    need("qa_comment_url"; "string"), opt("qa_page_url"; "string"),
    opt("video"; "string"), opt("made_at"; "string"),
    need("items"; "array"),
    (if (.items | type) == "array" and (.items | length) == 0 and (has("video") | not)
       then "items is empty and there is no video" else empty end),
    (if (.items | type) == "array" then
       .items | to_entries[] | .key as $i | .value |
       if type != "object" then "items[\($i)] should be an object"
       else
         (if (.caption | type) != "string" then "items[\($i)].caption should be a string" else empty end),
         (if (.after | type) != "string" then "items[\($i)].after should be a string" else empty end),
         (if has("before") and (.before | type) != "string" then "items[\($i)].before should be a string" else empty end),
         plain(.after), plain(.before)
       end
     else empty end),
    plain(.video)
  ' "$dir/manifest.json"
  # Every file the manifest names must be in the folder.
  local f
  while IFS= read -r f; do
    [[ -z "$f" || "$f" == */* ]] && continue
    [[ -f "$dir/$f" ]] || echo "manifest names $f, which is not in $dir"
  done < <(jq -r '[(.items // [] | if type == "array" then .[] else empty end
                    | objects | .before, .after), .video]
                  | .[] | strings' "$dir/manifest.json")
}

check() {
  local out
  out=$(problems "$1")
  if [[ -n "$out" ]]; then
    echo "$out" >&2
    return 1
  fi
  echo ok
}

publish() {
  local n=$1 staging=$2
  [[ "$n" =~ ^[0-9]+$ ]] || { echo "PR number should be a number: $n" >&2; exit 2; }
  check "$staging" >/dev/null
  local pr
  pr=$(jq -r .pr "$staging/manifest.json")
  [[ "$pr" == "$n" ]] || { echo "manifest says pr $pr, not $n" >&2; exit 1; }

  local dest="$demos/pr-$n" tmp="$demos/.pr-$n.new"
  mkdir -p "$demos"
  rm -rf "$tmp"
  mkdir "$tmp"
  cp "$staging/manifest.json" "$tmp/"
  local f
  while IFS= read -r f; do
    cp "$staging/$f" "$tmp/"
  done < <(jq -r '[.items[] | .before, .after] + [.video] | .[] | strings' "$staging/manifest.json" | sort -u)
  # Swap in the new folder, so a reader sees the old one or the new one, never half of each.
  rm -rf "$dest"
  mv "$tmp" "$dest"
  echo "$dest"
}

case ${1:-} in
  check) [[ $# -eq 2 ]] || { echo "usage: $0 check <dir>" >&2; exit 2; }; check "$2" ;;
  publish) [[ $# -eq 3 ]] || { echo "usage: $0 publish <N> <staging>" >&2; exit 2; }; publish "$2" "$3" ;;
  *) sed -n '2,9p' "$0" | sed 's/^# \{0,1\}//' >&2; exit 2 ;;
esac
