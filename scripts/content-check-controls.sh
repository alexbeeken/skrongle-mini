#!/usr/bin/env bash
# The content check's control set (#1061, data/moderation/controls/): every
# control through the term check, the real Jev (stage 1) and the real stage 2
# review, the same code the server runs, then a line per control and a
# scorecard per category: obvious and subtle ones caught, fine look-alikes
# passed, and the cost. `cargo test controls` (CI) covers the term check
# alone. Notes: data/moderation/CONTROLS.md (not docs/: that is served).
#
#   zsh -ic 'scripts/content-check-controls.sh'                 # every category
#   zsh -ic 'scripts/content-check-controls.sh --category hate' # one
#
# Needs TYPESAFE_API_KEY (Jev) and SKRONGLE_AI_KEY or ANTHROPIC_API_KEY
# (stage 2), from the shell, ./.env or e2e/.env. Manual only: CI never runs it.
set -euo pipefail
cd "$(dirname "$0")/.."
category=""
while [ $# -gt 0 ]; do
  case "$1" in
    --category) category="${2:?--category needs a name}"; shift 2 ;;
    -h | --help) sed -n '2,14p' "$0"; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done
# shellcheck source=scripts/content-check-keys.sh
. scripts/content-check-keys.sh
CONTROLS_CATEGORY="$category" exec cargo test --quiet content_check::controls::tests::scorecard -- --ignored --nocapture --exact
