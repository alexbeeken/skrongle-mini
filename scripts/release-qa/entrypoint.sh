#!/bin/sh
# The release QA image's entrypoint (scripts/release-qa/compose.yaml): one
# image per architecture, built from Dockerfile, that either serves a
# release tarball or checks it.
#
#   entrypoint.sh serve   unpack /tarballs/skrongle-$SKRONGLE_VERSION-$ARCH.tar.gz
#                         into /opt/skrongle and run it, as a self-hoster would
#   entrypoint.sh smoke   run package-smoke.sh on that tarball and exit with its status
#   entrypoint.sh upgrade run upgrade-check.sh: that tarball on the database each older release
#                         wrote (the fixtures mounted at /usr/local/lib/release-qa/fixtures)
#
# ARCH is what `uname -m` prints on the target (aarch64, x86_64, armv7l,
# armv6l); the tarballs are named for it.
set -eu

: "${SKRONGLE_VERSION:?set SKRONGLE_VERSION (the release to run, e.g. 0.1.2)}"
: "${ARCH:?set ARCH (aarch64, x86_64, armv7l or armv6l)}"
# The image must be the architecture it is named for: a Docker without
# BuildKit can tag an image for one architecture and build another, and a
# green check on the wrong architecture would say nothing.
MACHINE=$(uname -m)
if [ "$MACHINE" != "$ARCH" ]; then
  echo "this container runs as $MACHINE but ARCH=$ARCH: the image was built for the wrong architecture."
  echo "Rebuild it (docker compose build skrongle-$ARCH) and, for armv7l or armv6l, install QEMU's handlers"
  echo "first (docker compose --profile setup run --rm binfmt)."
  exit 4
fi
TARBALL="/tarballs/skrongle-$SKRONGLE_VERSION-$ARCH.tar.gz"
if [ ! -f "$TARBALL" ]; then
  echo "no tarball at $TARBALL."
  echo "Fetch the release first (docker compose --profile fetch run --rm fetch), or build one"
  echo "(scripts/build-pi.sh + scripts/package-release.sh) into the TARBALLS directory."
  echo "A release that has no build for $ARCH yet is skipped by matrix.sh."
  exit 3
fi

case "${1:-serve}" in
  serve)
    if [ -f "$TARBALL.sha256" ]; then
      (cd /tarballs && sha256sum -c "$(basename "$TARBALL").sha256" >/dev/null) || { echo "checksum mismatch for $TARBALL"; exit 1; }
    fi
    rm -rf /opt/skrongle && mkdir -p /opt/skrongle
    tar -xzf "$TARBALL" -C /opt/skrongle
    echo "skrongle $(cat /opt/skrongle/VERSION) for $ARCH on $(uname -m); the owner link is in this log, open it with the host port"
    cd /opt/skrongle
    exec ./skrongle
    ;;
  smoke)
    exec sh /usr/local/lib/release-qa/package-smoke.sh "$TARBALL"
    ;;
  upgrade)
    exec sh /usr/local/lib/release-qa/upgrade-check.sh "$TARBALL"
    ;;
  *)
    exec "$@"
    ;;
esac
