#!/usr/bin/env bash
# Provenance check of the built-in courses: are the lessons original? Lints the
# briefs for "imitate product X" instructions, shingle-matches every passage
# against reference texts and between lessons (both free), and has Jev judge
# every passage for text it recognises from a published source. Cached, cents.
# Reports similarity risk, not infringement; a clean run is weak evidence.
#
#   scripts/plagiarism-check.sh                       # every course in data/courses/
#   scripts/plagiarism-check.sh spanish --lesson a2/  # one course, some lessons
#   scripts/plagiarism-check.sh --test-course         # also the test course
#   scripts/plagiarism-check.sh --prefilter-only      # no key: lint + shingle overlap
#   scripts/plagiarism-check.sh --sources refs/       # also match against reference texts
#   scripts/plagiarism-check.sh --calibrate           # measure the judge and the pre-filter
#   scripts/plagiarism-check.sh --help                # all options
#
# The key comes from TYPESAFE_API_KEY (or JEV_API_KEY), ./.env or e2e/.env.
# A finding reviewed and found fine goes in data/courses/<id>/plagiarism-ok.txt.
# See docs/testing.md, "Plagiarism check".
set -euo pipefail
cd "$(dirname "$0")/.."
case " $* " in *" -h "*|*" --help "*) sed -n '2,19p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;; esac

KEY="${TYPESAFE_API_KEY:-${JEV_API_KEY:-}}"
for f in .env e2e/.env; do
  [ -z "$KEY" ] && [ -f "$f" ] && KEY=$(sed -n -E 's/^(TYPESAFE_API_KEY|JEV_API_KEY)=//p' "$f" | tr -d "\"' " | head -1)
done
export TYPESAFE_API_KEY="$KEY"

case " $* " in
  *" --prefilter-only "*) ;;
  *) if [ -z "$KEY" ]; then
       echo "No Jev key. Create one at https://console.typesafe.ai/keys and put it in ./.env or e2e/.env:"
       echo "  TYPESAFE_API_KEY=..."
       echo "(or --prefilter-only for the free checks)"
       exit 2
     fi ;;
esac

[ -d e2e/node_modules/@typesafe-ai/sdk ] || (cd e2e && npm ci --no-fund --no-audit >/dev/null)
exec node e2e/plagiarism/index.ts "$@"
