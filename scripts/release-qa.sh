#!/usr/bin/env bash
# One command for the automated release check (#45): the release-check
# script, then the per-architecture matrix, then one summary.
#
#   scripts/release-qa.sh 0.2.0        # a version: release-check.sh 0.2.0, then matrix.sh 0.2.0
#   scripts/release-qa.sh dist         # a tarball directory: matrix.sh only (release-check.sh needs a version)
#
# Logs: results/release-qa/<version>/ (release-check.log, matrix.log and
# one <arch>.log per architecture). Exit status: non-zero on any FAIL.
# The manual list for real hardware is docs/release-qa.md.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
WHAT="${1:?usage: scripts/release-qa.sh <version | tarball dir>}"

if [ -d "$WHAT" ]; then
  WHAT=$(cd "$WHAT" && pwd)
  FIRST=$(ls "$WHAT"/skrongle-*-*.tar.gz 2>/dev/null | head -1 || true)
  [ -n "$FIRST" ] || { echo "no skrongle-<version>-<arch>.tar.gz in $WHAT"; exit 2; }
  VERSION=$(basename "$FIRST" | sed -E 's/^skrongle-(.*)-[^-]+\.tar\.gz$/\1/')
  VERSION_GIVEN=0
else
  VERSION="${WHAT#v}"
  VERSION_GIVEN=1
fi
OUT="results/release-qa/$VERSION"
mkdir -p "$ROOT/$OUT"
STATUS=0

if [ "$VERSION_GIVEN" = 1 ]; then
  echo "==> release-check.sh $VERSION"
  if "$ROOT/scripts/release-check.sh" "$VERSION" 2>&1 | tee "$ROOT/$OUT/release-check.log"; then
    CHECK=ok
  else
    CHECK=FAIL; STATUS=1
  fi
else
  CHECK="skipped (a tarball directory has no version to check against Cargo.toml and CHANGELOG.md)"
  echo "==> release-check.sh: $CHECK"
fi

echo "==> matrix.sh $WHAT"
if "$ROOT/scripts/release-qa/matrix.sh" "$WHAT" 2>&1 | tee "$ROOT/$OUT/matrix.log"; then
  MATRIX=ok
else
  MATRIX=FAIL; STATUS=1
fi

echo
echo "release QA $VERSION"
printf '%-14s %s\n' "release-check" "$CHECK"
printf '%-14s %s   (per architecture: %s/<arch>.log)\n' "matrix" "$MATRIX" "$OUT"
sed -n '/^architecture /,$p' "$ROOT/$OUT/matrix.log" | sed 's/^/  /'
echo "logs: $OUT/"
[ "$STATUS" = 0 ] && echo "Next: the hardware checklist, docs/release-qa.md"
exit $STATUS
