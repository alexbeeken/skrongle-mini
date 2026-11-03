#!/usr/bin/env bash
# File size lint (#908). An agent working on this repo pays for every line it
# reads out of its context window, and one of our biggest files costs a fifth
# of it before any work starts. So a source file may not grow past LIMIT
# non-test lines.
#
# Counted: .rs, .js, .ts, .html and .css files under src/, static/,
# templates/ and e2e/ (tracked or new, not ignored). In a .rs file only the
# lines before the first `#[cfg(test)]` at the start of a line count, and a
# file named tests.rs is test code and does not count at all. Other files
# count whole. The i18n/*.yaml files are not counted.
#
# The files that were already over the limit are listed in
# scripts/file-size-baseline.txt with their size then. A listed file may
# shrink, never grow; an unlisted file may not pass LIMIT. A file that grows
# past it gets split, not baselined, unless there is a reason (say it in the
# PR and run --update).
#
#   scripts/file-size-lint.sh            check (CI runs this; fails on any growth)
#   scripts/file-size-lint.sh --update   write the current offenders as the baseline
set -euo pipefail
cd "$(dirname "$0")/.."
LIMIT=1500
baseline=scripts/file-size-baseline.txt

# Non-test lines of one file.
count() {
  case "$1" in
    tests.rs | */tests.rs) echo 0 ;;
    *.rs) awk '/^#\[cfg\(test\)\]/ { exit } { n++ } END { print n + 0 }' "$1" ;;
    *) awk 'END { print NR }' "$1" ;;
  esac
}

# Listed here, not inside current(): current() runs in a process
# substitution, where a git failure would go unnoticed and the lint pass.
files=$(git ls-files --cached --others --exclude-standard -- src static templates e2e)

# "<lines> <path>" for every counted file over LIMIT.
current() {
  printf '%s\n' "$files" |
    grep -E '\.(rs|js|ts|html|css)$' | sort -u | while read -r f; do
      [ -f "$f" ] || continue
      n=$(count "$f")
      if [ "$n" -gt "$LIMIT" ]; then echo "$n $f"; fi
    done
  return 0
}

if [ "${1:-}" = "--update" ]; then
  current > "$baseline"
  echo "wrote $baseline ($(wc -l < "$baseline" | tr -d ' ') files over $LIMIT lines)"
  exit 0
fi

fail=0
while read -r n f; do
  allowed=$(awk -v f="$f" '$2 == f { print $1 }' "$baseline")
  allowed=${allowed:-$LIMIT}
  if [ "$n" -gt "$allowed" ]; then
    echo "$f: $n non-test lines, $allowed allowed (limit $LIMIT, scripts/file-size-baseline.txt)"
    fail=1
  fi
done < <(current)

if [ "$fail" = 1 ]; then
  cat <<EOF

A file over $LIMIT non-test lines costs every agent that reads it a large
part of its context window. Split it into smaller modules or files instead
of growing it. If it has to grow, say why in the PR and run
scripts/file-size-lint.sh --update. Shrank a listed file? --update lowers
its line too.
EOF
  exit 1
fi
echo "file size lint: no file grew past its limit ($LIMIT non-test lines, or its baseline)"
