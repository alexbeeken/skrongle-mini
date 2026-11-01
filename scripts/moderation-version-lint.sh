#!/usr/bin/env bash
# Moderation version lint (#1039): a change to the content check's rules
# comes with a new version, so the courses checked under the old rules can
# be found and checked again.
#
# - The term list (data/moderation/terms.txt) goes with TERMS_VERSION
#   (src/course_flags/terms.rs).
# - The AI check's rules (the Jev questions, the threshold and stage 2's
#   prompt: every block between "// moderation-criteria(ai): start" and
#   "// moderation-criteria(ai): end" under src/) go with AI_CHECK_VERSION
#   (src/content_check.rs). Moving a block to another file is not a change.
#
# A PR that changes one fails unless it raises the matching version, or its
# body says `moderation-version: unchanged` (a typo, a comment: nothing that
# could flip a verdict). `cargo test` checks that docs/content-check-versions.md
# has a line for each current version.
#
#   PR_BODY="…" scripts/moderation-version-lint.sh <base> [head]   compare head (default HEAD) with its merge base with <base>
#
# CI runs it on every PR that touches src/ or data/moderation/
# (.github/workflows/moderation-version-lint.yml).
set -euo pipefail
cd "$(dirname "$0")/.."
base=${1:?usage: scripts/moderation-version-lint.sh <base> [head]}
head=${2:-HEAD}
from=$(git merge-base "$base" "$head")

# The version a const has at a revision, '' when it doesn't exist there.
version() {
  git grep -h -E "^pub const $2: i64 = [0-9]+;" "$1" -- src ':!**/tests.rs' 2>/dev/null | grep -oE '[0-9]+;' | tr -d ';' | head -1 || true
}

# The rules a version covers at a revision, as text to compare.
criteria() {
  case "$2" in
    terms) git show "$1:data/moderation/terms.txt" 2>/dev/null || true ;;
    ai)
      git grep -l -E '^// moderation-criteria\(ai\): start' "$1" -- src ':!**/tests.rs' 2>/dev/null | while read -r f; do
        git show "$f" | awk '/^\/\/ moderation-criteria\(ai\): start/ { on = 1; next } /^\/\/ moderation-criteria\(ai\): end/ { on = 0 } on'
      done | sort
      ;;
  esac
}

unchanged=no
if printf '%s' "${PR_BODY:-}" | grep -qiE 'moderation-version:[[:space:]]*unchanged'; then unchanged=yes; fi

failed=0
for check in "terms TERMS_VERSION src/course_flags/terms.rs data/moderation/terms.txt" \
             "ai AI_CHECK_VERSION src/content_check.rs the_moderation-criteria(ai)_blocks"; do
  read -r name const file what <<<"$check"
  what=${what//_/ }
  if [ "$(criteria "$from" "$name")" = "$(criteria "$head" "$name")" ]; then
    echo "moderation version lint: $what unchanged"
    continue
  fi
  old=$(version "$from" "$const")
  new=$(version "$head" "$const")
  if [ -z "$old" ]; then
    echo "moderation version lint: $what changed; $const is new here ($new)"
  elif [ -n "$new" ] && [ "$new" -gt "$old" ]; then
    echo "moderation version lint: $what changed; $const $old -> $new"
  elif [ "$unchanged" = yes ]; then
    echo "moderation version lint: $what changed, $const stays $old (the PR body says moderation-version: unchanged)"
  else
    echo "::error file=$file,title=Moderation version lint::$what changed but $const is still $old. Raise it to $((old + 1)) in $file and add a line to docs/content-check-versions.md, or, if the change can't flip a verdict (a typo, a comment), write 'moderation-version: unchanged' in the PR body."
    failed=1
  fi
done
exit "$failed"
