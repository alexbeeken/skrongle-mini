#!/bin/sh
# Update a Skrongle install to the latest release (or the one given):
#
#   sudo /opt/skrongle/update.sh            # latest release
#   sudo /opt/skrongle/update.sh 0.3.1      # a given version (also how to go back)
#
# Shipped in every release tarball. It downloads
# skrongle-<version>-<arch>.tar.gz for this machine, checks its checksum,
# stops the service, copies the database aside (learn.db.before-<version>),
# replaces the binary and data/, and starts the service again. Learners'
# accounts and progress live in the database, which it never changes;
# the new binary migrates it when it starts.
#
# SKRONGLE_REPO=owner/repo picks another GitHub repository; SKRONGLE_SERVICE
# another systemd unit name.
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

# Where the service keeps its database (SKRONGLE_DB in the unit), to copy it
# aside. A unit from before #584 still says LEARN_DB; read that too for one release.
DB=$(systemctl show "$SERVICE" -p Environment --value 2>/dev/null | tr ' ' '\n' | sed -nE 's/^(SKRONGLE|LEARN)_DB=//p' | head -1)
DB="${DB:-/var/lib/skrongle/learn.db}"

echo "stopping $SERVICE ($CURRENT -> $VERSION)"
systemctl stop "$SERVICE" 2>/dev/null || true
if [ -f "$DB" ]; then
  cp -p "$DB" "$DB.before-$VERSION"
  # A clean stop folds the WAL into learn.db; after a crash it still holds
  # recent writes, and SQLite reads <file>-wal as that file's log.
  if [ -s "$DB-wal" ]; then cp -p "$DB-wal" "$DB.before-$VERSION-wal"; fi
  echo "database copied to $DB.before-$VERSION"
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
  echo "to go back: sudo $SELF $CURRENT  (the database from before is at $DB.before-$VERSION)"
  exit 1
fi
