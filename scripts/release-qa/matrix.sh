#!/usr/bin/env bash
# Check a release on every architecture (#41): for each architecture we
# ship, run package-smoke.sh on its tarball in a Debian container of that
# architecture (compose.yaml's qa-<arch> services), then print one table.
#
#   scripts/release-qa/matrix.sh 0.1.2                 # a published release: fetched into dist/ if missing
#   scripts/release-qa/matrix.sh dist                  # the tarballs in a directory (a candidate you built)
#   scripts/release-qa/matrix.sh 0.1.2 aarch64 x86_64  # only these architectures
#
# After the smoke check, each architecture also runs upgrade-check.sh (#43):
# this release on the database every older release wrote (fixtures/), and the
# older release refusing the migrated one. SKIP_UPGRADE=1 leaves that out.
#
# Logs: results/release-qa/<version>/<arch>.log (and <arch>.upgrade.log). Exit status: non-zero when
# any architecture FAILs. An architecture the release has no tarball for is
# skipped and says so. The 32-bit ARM architectures need QEMU's handlers
# once: docker compose --profile setup run --rm binfmt.
set -uo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
command -v docker >/dev/null || { echo "needs docker (with compose)"; exit 2; }

WHAT="${1:?usage: matrix.sh <version | tarball dir> [arch ...]}"; shift || true
ARCHES=("$@"); [ ${#ARCHES[@]} -gt 0 ] || ARCHES=(aarch64 x86_64 armv7l armv6l)
# A directory is taken from where the command was run; compose runs from here.
[ -d "$WHAT" ] && WHAT=$(cd "$WHAT" && pwd)
cd "$HERE"

if [ -d "$WHAT" ]; then
  TARBALLS="$WHAT"
  FIRST=$(ls "$TARBALLS"/skrongle-*-*.tar.gz 2>/dev/null | head -1)
  [ -n "$FIRST" ] || { echo "no skrongle-<version>-<arch>.tar.gz in $TARBALLS"; exit 2; }
  VERSION=$(basename "$FIRST" | sed -E 's/^skrongle-(.*)-[^-]+\.tar\.gz$/\1/')
else
  VERSION="${WHAT#v}"
  TARBALLS="$ROOT/dist"
  mkdir -p "$TARBALLS"
  missing=0
  for a in "${ARCHES[@]}"; do [ -f "$TARBALLS/skrongle-$VERSION-$a.tar.gz" ] || missing=1; done
  if [ "$missing" = 1 ]; then
    echo "==> fetching release $VERSION into $TARBALLS"
    SKRONGLE_VERSION="$VERSION" TARBALLS="$TARBALLS" ARCHES="${ARCHES[*]}" docker compose --profile fetch run --rm fetch || { echo "fetch failed"; exit 2; }
  fi
fi
export SKRONGLE_VERSION="$VERSION" TARBALLS

OUT="$ROOT/results/release-qa/$VERSION"
mkdir -p "$OUT"
declare -A VERDICT UPGRADE
STATUS=0
for a in "${ARCHES[@]}"; do
  if [ ! -f "$TARBALLS/skrongle-$VERSION-$a.tar.gz" ]; then
    VERDICT[$a]="skipped (no tarball for $a in $TARBALLS)"
    UPGRADE[$a]="-"
    continue
  fi
  echo "==> $a: package-smoke.sh on skrongle-$VERSION-$a.tar.gz (log: results/release-qa/$VERSION/$a.log)"
  if docker compose run --rm --build --no-deps "qa-$a" > "$OUT/$a.log" 2>&1; then
    VERDICT[$a]=ok
  else
    rc=$?
    if grep -q 'exec format error' "$OUT/$a.log"; then
      VERDICT[$a]="FAIL (this Docker cannot run $a: docker compose --profile setup run --rm binfmt)"
    else
      VERDICT[$a]="FAIL (exit $rc; $(grep -c '^  FAIL' "$OUT/$a.log") FAIL lines)"
    fi
    STATUS=1
    grep -E '^  FAIL|^FAIL|^PASS' "$OUT/$a.log" | head -20 | sed 's/^/     /'
  fi
  if [ "${SKIP_UPGRADE:-0}" = 1 ]; then
    UPGRADE[$a]="-"
  else
    echo "==> $a: upgrade-check.sh on skrongle-$VERSION-$a.tar.gz (log: results/release-qa/$VERSION/$a.upgrade.log)"
    if docker compose run --rm --no-deps "qa-$a" upgrade > "$OUT/$a.upgrade.log" 2>&1; then
      UPGRADE[$a]=ok
    else
      rc=$?
      UPGRADE[$a]="FAIL (exit $rc; $(grep -c '^  FAIL' "$OUT/$a.upgrade.log") FAIL lines)"
      STATUS=1
      grep -E '^  FAIL|^FAIL|^PASS' "$OUT/$a.upgrade.log" | head -20 | sed 's/^/     /'
    fi
  fi
done

echo
echo "release $VERSION"
printf '%-10s %-8s %s\n' architecture verdict upgrade
for a in "${ARCHES[@]}"; do
  case "${VERDICT[$a]}" in
    ok) printf '%-10s %-8s %s\n' "$a" ok "${UPGRADE[$a]}" ;;
    *) printf '%-10s %s\n' "$a" "${VERDICT[$a]}" ;;
  esac
done
exit $STATUS
