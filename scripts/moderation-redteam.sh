#!/usr/bin/env bash
# Try to beat the content check (#1061). Claude writes one lesson that tries
# to break a category's rule without being caught; the lesson goes through
# the term check, the real Jev and the real stage 2 review (the server's own
# code, on a database in memory). When it gets through, the same
# conversation is asked to explain its technique. Each round tells Claude
# what happened last time, so it tries something new. It prints each
# round's scores, verdict and cost, and writes a report (lessons, scores,
# verdicts, explanations) to --out, by default a file in $TMPDIR. Never in
# docs/. data/moderation/CONTROLS.md, "Trying to beat the filter".
#
#   zsh -ic 'scripts/moderation-redteam.sh --category violence --rounds 2'
#   zsh -ic 'scripts/moderation-redteam.sh'                  # one round, a random category
#   zsh -ic 'scripts/moderation-redteam.sh --category hate --keep --out audit/redteam.md'
#
#   --category NAME  a category of content_check::CATEGORIES, or random (the default)
#   --rounds N       lessons to write (default 1)
#   --keep           add each lesson that got through to data/moderation/controls/
#                    as a subtle control, so the next scorecard counts it
#   --out FILE       where the report goes
#   --model NAME     the Claude model that writes the lessons (default claude-sonnet-5-5)
#
# The minors category is refused: no model is asked to write for it.
# Needs TYPESAFE_API_KEY and SKRONGLE_AI_KEY or ANTHROPIC_API_KEY (the shell,
# ./.env or e2e/.env). Manual only: CI never runs it. Each round costs a few cents.
set -euo pipefail
cd "$(dirname "$0")/.."
category=random rounds=1 keep=0 out="" model=""
while [ $# -gt 0 ]; do
  case "$1" in
    --category) category="${2:?--category needs a name}"; shift 2 ;;
    --rounds) rounds="${2:?--rounds needs a number}"; shift 2 ;;
    --keep) keep=1; shift ;;
    --out) out="${2:?--out needs a file}"; shift 2 ;;
    --model) model="${2:?--model needs a name}"; shift 2 ;;
    -h | --help) sed -n '2,25p' "$0"; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done
case "$category" in
  minors)
    echo "refused: the red-team script never writes for the minors category. Its controls are written by hand in data/moderation/controls/minors.json." >&2
    exit 2 ;;
esac
case "$rounds" in
  '' | *[!0-9]* | 0) echo "--rounds needs a whole number of 1 or more" >&2; exit 2 ;;
esac
# shellcheck source=scripts/content-check-keys.sh
. scripts/content-check-keys.sh
if [ -n "$out" ]; then
  mkdir -p "$(dirname "$out")"
  case "$out" in /*) ;; *) out="$PWD/$out" ;; esac
fi
REDTEAM_CATEGORY="$category" REDTEAM_ROUNDS="$rounds" REDTEAM_KEEP="$keep" REDTEAM_OUT="$out" REDTEAM_MODEL="$model" \
  exec cargo test --quiet content_check::controls::tests::redteam -- --ignored --nocapture --exact
