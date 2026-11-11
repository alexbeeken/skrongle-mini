#!/bin/sh
# Lint for browser specs: a judge statement must say what is present, not what
# is absent. Jev (the AI judge, #58) is asked to confirm a positive statement;
# absence is checked exactly with expect(...).not.toMatch or a locator count of 0.
#
#   scripts/e2e-lint.sh               # scan e2e/specs, e2e/lib, e2e/journeys.ts
#   scripts/e2e-lint.sh --self-test   # prove the check flags a bad statement and nothing else
#
# Flags a judge(...) or judgeMany(...) call whose statement (its first string
# literal, on the same line as the call) starts with "No", "Nothing" or
# "Never", or contains " not " or "n't". Exit 1 on a hit, 0 when clean.
# See docs/testing.md, "Judge statements".
set -eu
cd "$(dirname "$0")/.."

# scan FILE...: prints "file:line: message" for each flagged statement.
scan() {
  awk '
    {
      rest = $0
      while (match(rest, /(^|[^A-Za-z0-9_])judge(Many)?\(/)) {
        after = substr(rest, RSTART + RLENGTH)
        rest = after
        if (!match(after, /["'"'"'`]/)) continue
        q = substr(after, RSTART, 1)
        body = substr(after, RSTART + 1)
        end = index(body, q)
        if (end == 0) continue
        stmt = substr(body, 1, end - 1)
        low = tolower(stmt)
        if (low ~ /^(no|nothing|never)([^a-z]|$)/ || index(low, " not ") > 0 || index(low, "n'\''t") > 0) {
          printf "%s:%d: judge statement \"%s\" says what is absent; check it with expect(...).not.toMatch or a locator count of 0 instead\n", FILENAME, FNR, stmt
        }
      }
    }
  ' "$@"
}

self_test() {
  dir=$(mktemp -d "${TMPDIR:-/tmp}/e2e-lint.XXXXXX")
  trap 'rm -rf "$dir"' EXIT
  f="$dir/sample.spec.ts"
  cat >"$f" <<'EOF'
await judge(page, "The page rendered normally");
await judge(page, "No lesson on this page shows a raw group identifier");
EOF
  out=$(scan "$f")
  count=$(printf '%s\n' "$out" | grep -c . || true)
  if [ "$count" = 1 ] && printf '%s\n' "$out" | grep -q ':2: '; then
    echo "e2e-lint self-test: ok (flags line 2 only)"
    exit 0
  fi
  echo "e2e-lint self-test: FAILED (expected exactly line 2 flagged)" >&2
  printf '%s\n' "$out" >&2
  exit 1
}

if [ "${1:-}" = "--self-test" ]; then
  self_test
fi

FILES=$(find e2e/specs e2e/lib -name '*.ts' -type f | sort)
[ -f e2e/journeys.ts ] && FILES="$FILES
e2e/journeys.ts"

HITS=""
if [ -n "$FILES" ]; then
  # shellcheck disable=SC2086
  HITS=$(scan $FILES)
fi
if [ -n "$HITS" ]; then
  printf '%s\n' "$HITS"
  echo "e2e-lint: FAILED" >&2
  exit 1
fi
echo "e2e-lint: ok"
