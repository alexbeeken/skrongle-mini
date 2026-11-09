#!/bin/sh
# Download a release's tarballs and checksums into /tarballs (the `fetch`
# profile of scripts/release-qa/compose.yaml), so the rig needs nothing but
# Docker and a version number. Architectures that the release has no build
# for are reported and skipped, not failed.
#
#   SKRONGLE_VERSION=0.1.2 ARCHES="aarch64 x86_64" sh fetch.sh
set -eu
: "${SKRONGLE_VERSION:?set SKRONGLE_VERSION}"
REPO="${SKRONGLE_REPO:-alexbeeken/skrongle-mini}"
ARCHES="${ARCHES:-aarch64 x86_64 armv7l armv6l}"
BASE="https://github.com/$REPO/releases/download/v$SKRONGLE_VERSION"
cd /tarballs
status=0
for arch in $ARCHES; do
  name="skrongle-$SKRONGLE_VERSION-$arch.tar.gz"
  if [ -f "$name" ] && [ -f "$name.sha256" ] && sha256sum -c "$name.sha256" >/dev/null 2>&1; then
    echo "have   $name"
    continue
  fi
  if ! curl -fsL -o "$name.sha256" "$BASE/$name.sha256"; then
    echo "none   $name (the release has no $arch build)"; rm -f "$name.sha256"
    continue
  fi
  echo "get    $name"
  curl -fL --progress-bar -o "$name" "$BASE/$name"
  if sha256sum -c "$name.sha256" >/dev/null; then echo "ok     $name"; else echo "FAIL   $name: checksum mismatch"; status=1; fi
done
exit $status
