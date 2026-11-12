#!/bin/sh
# Update a Skrongle install to the latest release (or the one given):
#
#   sudo /opt/skrongle/update.sh            # latest release
#   sudo /opt/skrongle/update.sh 0.3.1      # a given version (also how to go back)
#
# Shipped in every release tarball. It downloads
# skrongle-<version>-<arch>.tar.gz for this machine, checks its checksum,
# stops the service, replaces the binary and data/, and starts the service
# again. Learners' accounts and progress live in the database. Going up, the
# new binary migrates it when it starts; going back to an older release, the
# installed binary first takes the schema back to that release's
# (`skrongle migrate`). The database is never copied.
#
# SKRONGLE_REPO=owner/repo picks another GitHub repository; SKRONGLE_SERVICE
# another systemd unit name. SKRONGLE_RELEASE_BASE=http://host:port/dir is a
# testing aid (scripts/release-qa/vm.sh): download skrongle-<version>-<arch>.tar.gz
# and its .sha256 from that folder instead of GitHub (a version is required).
# The checksum check is the same.
set -eu

# Run from a copy of this file. The copy of the new release below replaces
# this very script while it runs, and sh reads a script as it goes, so it
# would read the rest from the new file (0.1.2 -> 0.2.0 ended with a syntax
# error that way). SKRONGLE_UPDATE_SELF is the installed path, for messages.
if [ -z "${SKRONGLE_UPDATE_SELF:-}" ]; then
  SKRONGLE_UPDATE_SELF="$(cd "$(dirname "$0")" && pwd)/$(basename "$0")"
  SKRONGLE_UPDATE_COPY=$(mktemp)
  cp "$SKRONGLE_UPDATE_SELF" "$SKRONGLE_UPDATE_COPY"
  export SKRONGLE_UPDATE_SELF SKRONGLE_UPDATE_COPY
  exec sh "$SKRONGLE_UPDATE_COPY" "$@"
fi
SELF="$SKRONGLE_UPDATE_SELF"
trap 'rm -f "$SKRONGLE_UPDATE_COPY"' EXIT

REPO="${SKRONGLE_REPO:-alexbeeken/skrongle-mini}"
SERVICE="${SKRONGLE_SERVICE:-skrongle}"
DIR="$(dirname "$SELF")"

[ "$(id -u)" = 0 ] || { echo "run it as root: sudo $SELF $*"; exit 1; }
command -v curl >/dev/null || { echo "needs curl: sudo apt install curl"; exit 1; }

case "$(uname -m)" in
  aarch64|arm64) ARCH=aarch64 ;;
  x86_64|amd64)  ARCH=x86_64 ;;
  armv7l)        ARCH=armv7l ;;
  armv6l)        ARCH=armv6l ;;
  *) echo "no release for $(uname -m); build from source (docs/self-hosting.md)"; exit 1 ;;
esac

if [ $# -ge 1 ]; then
  VERSION="${1#v}"
elif [ -n "${SKRONGLE_RELEASE_BASE:-}" ]; then
  echo "SKRONGLE_RELEASE_BASE needs a version: sudo $SELF <version>"; exit 1
else
  # /releases/latest redirects to /releases/tag/v<version>.
  URL=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest")
  VERSION="${URL##*/}"
  VERSION="${VERSION#v}"
  case "$VERSION" in
    ''|latest|releases) echo "could not find the latest release of $REPO"; exit 1 ;;
  esac
fi
CURRENT=$(cat "$DIR/VERSION" 2>/dev/null || echo unknown)
if [ "$CURRENT" = "$VERSION" ] && [ -z "${FORCE:-}" ]; then
  echo "already on $VERSION (FORCE=1 to install it again)"
  exit 0
fi

NAME="skrongle-$VERSION-$ARCH.tar.gz"
BASE="https://github.com/$REPO/releases/download/v$VERSION"
[ -z "${SKRONGLE_RELEASE_BASE:-}" ] || BASE="${SKRONGLE_RELEASE_BASE%/}"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"; rm -f "$SKRONGLE_UPDATE_COPY"' EXIT
echo "downloading $NAME"
curl -fL --progress-bar -o "$TMP/$NAME" "$BASE/$NAME"
if curl -fsSL -o "$TMP/$NAME.sha256" "$BASE/$NAME.sha256"; then
  (cd "$TMP" && sha256sum -c "$NAME.sha256" >/dev/null) || { echo "checksum mismatch for $NAME; nothing changed"; exit 1; }
fi
mkdir "$TMP/new"
tar -xzf "$TMP/$NAME" -C "$TMP/new"
"$TMP/new/skrongle" --version >/dev/null || { echo "the new binary does not run on this machine; nothing changed"; exit 1; }

# Where the service keeps its database (SKRONGLE_DB in the unit), for the
# step back below. A unit from before #584 still says LEARN_DB; read that too for one release.
DB=$(systemctl show "$SERVICE" -p Environment --value 2>/dev/null | tr ' ' '\n' | sed -nE 's/^(SKRONGLE|LEARN)_DB=//p' | head -1)
DB="${DB:-/var/lib/skrongle/learn.db}"

if [ "$CURRENT" = "$VERSION" ]; then
  echo "stopping $SERVICE (installing $VERSION again)"
else
  echo "stopping $SERVICE ($CURRENT -> $VERSION)"
fi
systemctl stop "$SERVICE" 2>/dev/null || true
# Going back to an older release: its binary refuses a database whose schema
# is newer than its own, and only the installed binary knows how to take
# that schema back (`skrongle migrate`, the `down` half of each migration).
# So that runs first. Nothing is copied aside: going up, the new binary
# migrates the database when it starts, and this is the way back.
if [ -f "$DB" ] && [ -x "$DIR/skrongle" ]; then
  WANT=$("$TMP/new/skrongle" --schema-version 2>/dev/null || true)
  HAVE=$("$DIR/skrongle" --schema-version 2>/dev/null || true)
  case "$WANT:$HAVE" in
    *[!0-9:]*|:*|*:) ;; # a release from before --schema-version
    *)
      if [ "$WANT" -lt "$HAVE" ]; then
        echo "taking the database back to schema v$WANT for $VERSION (it is at v$HAVE)"
        if ! SKRONGLE_DB="$DB" "$DIR/skrongle" migrate "$WANT"; then
          echo "the database could not be taken back to schema v$WANT; nothing changed, starting $CURRENT again"
          echo "(a Skrongle from before 'skrongle migrate' cannot do this: put back the copy its update made, $DB.before-$CURRENT)"
          systemctl start "$SERVICE" || true
          exit 1
        fi
      fi
      ;;
  esac
fi
[ -f "$DIR/skrongle" ] && cp -p "$DIR/skrongle" "$DIR/skrongle.previous"
rm -rf "$DIR/data"
cp -R "$TMP/new/." "$DIR/"

if [ -f "/etc/systemd/system/$SERVICE.service" ] && ! cmp -s "$DIR/skrongle.service" "/etc/systemd/system/$SERVICE.service"; then
  echo "note: $DIR/skrongle.service differs from /etc/systemd/system/$SERVICE.service; if you have not edited yours, copy the new one and run systemctl daemon-reload"
fi

systemctl start "$SERVICE"
sleep 3
if systemctl is-active --quiet "$SERVICE"; then
  echo "Skrongle $VERSION is running"
else
  echo "Skrongle $VERSION did not start; the log:"
  journalctl -u "$SERVICE" -n 20 --no-pager || true
  echo "to go back: sudo $SELF $CURRENT"
  exit 1
fi
