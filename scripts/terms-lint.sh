#!/usr/bin/env bash
# Terms lint (#966, #978): a change to the built-in Terms or to the privacy
# notice goes through the update flow. If docs/terms.md or docs/privacy.md
# changed, docs/terms-changes.json must gain an entry (version, announced
# and effective dates, material flag, summary; for the privacy notice the
# summary starts "The privacy policy changed:") in the same change; if the
# changelog gained an entry, one of the two texts must have changed.
# `cargo test terms` checks the entries themselves (dates, summary, 30
# days' notice, the "Last updated" lines).
#
#   scripts/terms-lint.sh <base> [head]   compare head (default HEAD) with its merge base with <base>
#
# CI runs it on every PR (.github/workflows/terms-lint.yml).
set -euo pipefail
cd "$(dirname "$0")/.."
base=${1:?usage: scripts/terms-lint.sh <base> [head]}
head=${2:-HEAD}
from=$(git merge-base "$base" "$head")

entries() { git show "$1:docs/terms-changes.json" 2>/dev/null | grep -c '"version"' || true; }
before=$(entries "$from")
after=$(entries "$head")
changed=""
for doc in docs/terms.md docs/privacy.md; do
  git diff --quiet "$from" "$head" -- "$doc" || changed="$changed $doc"
done
changed=${changed# }

if [ -n "$changed" ] && [ "${after:-0}" -le "${before:-0}" ]; then
  for doc in $changed; do
    echo "::error file=$doc,title=Terms lint::$doc changed but docs/terms-changes.json has no new entry. Add one (version, announced, effective, material, summary) so learners are told what changed."
  done
  exit 1
fi
if [ -z "$changed" ] && [ "${after:-0}" -gt "${before:-0}" ]; then
  echo "::error file=docs/terms-changes.json,title=Terms lint::docs/terms-changes.json has a new entry but neither docs/terms.md nor docs/privacy.md changed. Change the text (and its \"Last updated\" line) in the same PR."
  exit 1
fi
echo "terms lint: ok (changed: ${changed:-nothing}, changelog entries $before -> $after)"
