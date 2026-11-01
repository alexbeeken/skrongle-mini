#!/usr/bin/env bash
# Put a finding's screenshots where a GitHub issue can show them, and print
# the Markdown to paste into the issue body. `gh` can't upload issue
# attachments, so the PNGs go on the `qa-screenshots` branch (never merged,
# no workflow runs on it) under <folder>/, and the body links to them. The
# repo is private: the images show for anyone who can see the issue.
#
#   issue-shots.sh <folder> <png>...     # e.g. issue-shots.sh 2026-10-28-phone-tables $S/shots/tables-*.png
#
# Prints one line per image: ![<file name>](https://github.com/<repo>/blob/qa-screenshots/<folder>/<file>?raw=true)
# Built with git plumbing (no checkout), so it works from any worktree, and
# safe to run from two sessions at once: a rejected push refetches and retries.
set -euo pipefail
FOLDER="${1:?issue-shots.sh <folder> <png>...}"; shift
[ "$#" -gt 0 ] || { echo "no images given" >&2; exit 1; }
case "$FOLDER" in *[!A-Za-z0-9._-]*) echo "folder: letters, digits, . _ - only" >&2; exit 1 ;; esac
for f in "$@"; do
  case "$f" in *.png|*.PNG|*.jpg|*.jpeg) ;; *) echo "not an image: $f" >&2; exit 1 ;; esac
  [ -f "$f" ] || { echo "missing: $f" >&2; exit 1; }
done
BR=qa-screenshots
REPO=$(gh repo view --json nameWithOwner --jq .nameWithOwner)
IDX=$(mktemp); trap 'rm -f "$IDX"' EXIT
for try in 1 2 3; do
  rm -f "$IDX"
  if git fetch -q origin "refs/heads/$BR" 2>/dev/null; then
    parent=$(git rev-parse FETCH_HEAD)
    GIT_INDEX_FILE="$IDX" git read-tree "$parent"
  else
    parent=""
    GIT_INDEX_FILE="$IDX" git read-tree --empty
  fi
  for f in "$@"; do
    blob=$(git hash-object -w "$f")
    GIT_INDEX_FILE="$IDX" git update-index --add --cacheinfo 100644 "$blob" "$FOLDER/$(basename "$f")"
  done
  tree=$(GIT_INDEX_FILE="$IDX" git write-tree)
  commit=$(git commit-tree "$tree" ${parent:+-p "$parent"} -m "qa screenshots: $FOLDER")
  git push -q origin "$commit:refs/heads/$BR" && break
  [ "$try" = 3 ] && { echo "push to $BR failed three times" >&2; exit 1; }
done
for f in "$@"; do
  name=$(basename "$f")
  echo "![${name%.*}](https://github.com/$REPO/blob/$BR/$FOLDER/$name?raw=true)"
done
