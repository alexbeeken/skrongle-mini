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
# E2E=1 adds a third check for aarch64 and x86_64 (#41): the Playwright `release`
# projects (docs/testing.md, "Release specs") against the tarball's server in
# the rig, from a Playwright container (compose.yaml, profile e2e). Off by
# default. Its log and report: results/release-qa/<version>/<arch>.e2e.log and
# results/release-qa/e2e/report.
#
# SIM=1 (with E2E=1) also starts the AI simulator (compose.yaml, profile
# simulator), so e2e/specs/release/ai.spec.ts runs against the tarball instead
# of skipping itself; the e2e cell then says whether it ran. The simulator is a
# dev-feature Linux build of this checkout for that architecture:
# SIM_BIN_AARCH64 / SIM_BIN_X86_64 if set, else built here with
# sim-build.sh (cargo zigbuild). Off by default.
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

# sim_bin <arch>: the simulator binary for that architecture (SIM_BIN_<ARCH>, else sim-build.sh); its absolute path, or fails.
sim_bin() {
  local var bin
  var="SIM_BIN_$(echo "$1" | tr '[:lower:]' '[:upper:]')"
  bin="${!var:-}"
  [ -n "$bin" ] || bin=$("$HERE/sim-build.sh" "$1" 2>"$OUT/$1.sim-build.log" | tail -1) || { echo "     sim-build.sh failed: results/release-qa/$VERSION/$1.sim-build.log" >&2; return 1; }
  [ -f "$bin" ] || { echo "no simulator binary at $bin" >&2; return 1; }
  (cd "$(dirname "$bin")" && echo "$(pwd)/$(basename "$bin")")
}

OUT="$ROOT/results/release-qa/$VERSION"
mkdir -p "$OUT"
declare -A VERDICT UPGRADE E2E_RESULT
STATUS=0
for a in "${ARCHES[@]}"; do
  if [ ! -f "$TARBALLS/skrongle-$VERSION-$a.tar.gz" ]; then
    VERDICT[$a]="skipped (no tarball for $a in $TARBALLS)"
    UPGRADE[$a]="-"
    E2E_RESULT[$a]="-"
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
  E2E_RESULT[$a]="-"
  if [ "${E2E:-0}" = 1 ]; then
    case "$a" in
      aarch64|x86_64)
        echo "==> $a: release browser specs against skrongle-$VERSION-$a.tar.gz (log: results/release-qa/$VERSION/$a.e2e.log)"
        mkdir -p "$ROOT/results/release-qa/e2e"
        PROFILES=(--profile e2e)
        SIM_VARS=()
        if [ "${SIM:-0}" = 1 ]; then
          echo "==> $a: the AI simulator, a dev-feature build of this checkout for $a (log: results/release-qa/$VERSION/$a.sim-build.log)"
          if ! bin=$(sim_bin "$a"); then
            E2E_RESULT[$a]="FAIL (no simulator binary for $a)"
            STATUS=1
            continue
          fi
          PROFILES+=(--profile simulator)
          SIM_VARS=(E2E_SIM=1 "SIM_BIN_$(echo "$a" | tr '[:lower:]' '[:upper:]')=$bin")
        fi
        if env E2E_RESULTS="$ROOT/results/release-qa/e2e" ${SIM_VARS[@]+"${SIM_VARS[@]}"} docker compose "${PROFILES[@]}" run --rm --build "e2e-$a" > "$OUT/$a.e2e.log" 2>&1; then
          E2E_RESULT[$a]=ok
          if [ "${SIM:-0}" = 1 ]; then
            # The list reporter marks a passed test ✓ and a skipped one -: with SIM=1 the AI spec must have run.
            if grep -E 'ai\.spec\.ts' "$OUT/$a.e2e.log" | grep -q '✓'; then
              E2E_RESULT[$a]="ok (AI spec ran)"
            else
              E2E_RESULT[$a]="FAIL (SIM=1 but the AI spec did not run; see $a.e2e.log)"
              STATUS=1
            fi
          fi
        else
          rc=$?
          E2E_RESULT[$a]="FAIL (exit $rc; see $a.e2e.log)"
          STATUS=1
          grep -E '^ +[0-9]+ (failed|passed)|✘|FAILED' "$OUT/$a.e2e.log" | head -20 | sed 's/^/     /'
        fi
        # The simulator lives in the server's network namespace, so it goes first.
        docker compose --profile e2e --profile simulator rm -fs "simulator-$a" "e2e-server-$a" >/dev/null 2>&1
        ;;
      *) E2E_RESULT[$a]="n/a" ;;
    esac
  fi
done

echo
echo "release $VERSION"
if [ "${E2E:-0}" = 1 ]; then printf '%-10s %-8s %-8s %s\n' architecture verdict upgrade e2e; else printf '%-10s %-8s %s\n' architecture verdict upgrade; fi
for a in "${ARCHES[@]}"; do
  case "${VERDICT[$a]}" in
    ok)
      if [ "${E2E:-0}" = 1 ]; then printf '%-10s %-8s %-8s %s\n' "$a" ok "${UPGRADE[$a]}" "${E2E_RESULT[$a]}"
      else printf '%-10s %-8s %s\n' "$a" ok "${UPGRADE[$a]}"; fi ;;
    *) printf '%-10s %s\n' "$a" "${VERDICT[$a]}" ;;
  esac
done
exit $STATUS
