#!/usr/bin/env bash
# Panic lint (#389). Release builds use panic = "abort" (Cargo.toml): a panic
# anywhere, in any request, kills the whole server and every learner's
# in-memory state with it. So code outside tests should not panic on data a
# request (or a course file) can control.
#
# This counts, per file under src/ and outside `#[cfg(test)] mod tests` (inline
# or a `tests.rs` file of its own, which is skipped whole), the
# lines that can panic: `.unwrap()`, `.expect(`, `panic!`/`unreachable!`/
# `todo!`/`unimplemented!`, and indexing or slicing (`x[i]`, `s[a..b]`). The
# counts in scripts/panic-baseline.txt are the sites that were there when
# the lint was added (#389, after a review found none reachable from a
# request); a file may go down, never up. A new site either handles the error (`?`,
# `.get(i)`, `unwrap_or`, an AppError) or says on the same line why it cannot
# fail: `// panic-ok: <reason>`.
#
#   scripts/panic-check.sh            check (CI runs this; fails on any increase)
#   scripts/panic-check.sh --update   write the current counts as the baseline
#                                     (after removing sites, or with a reviewed panic-ok)
set -euo pipefail
cd "$(dirname "$0")/.."
baseline=scripts/panic-baseline.txt

# Lines that can panic, outside the test module, without a panic-ok note.
sites() {
  awk '
    /^#\[cfg\(test\)\]/ { pending = 1; next }
    pending && /^(pub(\(crate\))? )?mod / { exit }
    { pending = 0 }
    /^[[:space:]]*\/\// { next }
    /panic-ok/ { next }
    /\.unwrap\(\)|\.expect\(|(panic|unreachable|todo|unimplemented)!|[]A-Za-z0-9_)]\[/ { printf "%s:%d: %s\n", FILENAME, FNR, $0 }
  ' "$1"
}

current() {
  find src -name '*.rs' ! -name tests.rs | sort | while read -r f; do
    n=$(sites "$f" | wc -l | tr -d ' ')
    if [ "$n" -gt 0 ]; then echo "$n $f"; fi
  done
  return 0
}

if [ "${1:-}" = "--update" ]; then
  current > "$baseline"
  echo "wrote $baseline ($(awk '{s += $1} END {print s}' "$baseline") sites in $(wc -l < "$baseline" | tr -d ' ') files)"
  exit 0
fi

fail=0
while read -r n f; do
  allowed=$(awk -v f="$f" '$2 == f { print $1 }' "$baseline")
  allowed=${allowed:-0}
  if [ "$n" -gt "$allowed" ]; then
    echo "$f: $n lines that can panic, $allowed allowed (scripts/panic-baseline.txt):"
    sites "$f" | sed 's/^/  /'
    fail=1
  fi
done < <(current)

if [ "$fail" = 1 ]; then
  cat <<'EOF'

A panic aborts the release server (panic = "abort"). Handle the error instead
(`?`, `.get(i)`, `unwrap_or`, an AppError), or, where it truly cannot fail,
end the line with `// panic-ok: <why>`. Removed some? Run
scripts/panic-check.sh --update to lower the baseline.
EOF
  exit 1
fi
echo "panic check: no new panicking lines outside tests"
