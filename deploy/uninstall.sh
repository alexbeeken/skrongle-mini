#!/bin/sh
# Remove a Skrongle install (docs/self-hosting.md, "Uninstalling"):
#
#   sudo /opt/skrongle/uninstall.sh           # keep the data: accounts, progress, AI keys
#   sudo /opt/skrongle/uninstall.sh --purge   # delete the data too (permanent)
#
# Shipped in every release tarball, next to update.sh. It stops and disables
# the skrongle unit, removes the unit file, /opt/skrongle and /etc/skrongle,
# and the skrongle system user if there is a static one (DietPi-style
# installs; a unit with DynamicUser=yes has none).
#
# The database folder is the data. With DynamicUser, systemd keeps it in
# /var/lib/private/skrongle and links /var/lib/skrongle to it; a DietPi
# install (deploy/dietpi-install.sh) keeps it in
# /mnt/dietpi_userdata/skrongle. All three are handled; a database the
# settings put anywhere else is left alone, and the script says so. The data
# stays unless --purge. /etc/skrongle stays too without
# --purge, because its env file may hold SKRONGLE_SECRET_KEY, which the kept
# database needs to read its saved AI keys.
#
# Running it again is harmless: what is already gone is skipped.
set -eu

usage() { echo "usage: sudo $0 [--purge]"; exit "$1"; }

[ "$(id -u)" = 0 ] || { echo "run it as root: sudo $0 $*"; exit 1; }
if [ $# -gt 1 ]; then usage 2; fi
PURGE=
case "${1:-}" in
  "") ;;
  --purge) PURGE=1 ;;
  -h|--help) usage 0 ;;
  *) usage 2 ;;
esac

UNIT=/etc/systemd/system/skrongle.service
DROPIN=/etc/systemd/system/skrongle.service.d
OPT=/opt/skrongle
CONF=/etc/skrongle
DATA=/var/lib/skrongle
DATA_REAL=/var/lib/private/skrongle
DATA_DIETPI=/mnt/dietpi_userdata/skrongle
HAVE_SYSTEMCTL=
if command -v systemctl >/dev/null 2>&1; then HAVE_SYSTEMCTL=1; fi

# A file, a symlink or a folder is there (a dangling symlink counts).
exists() { [ -e "$1" ] || [ -L "$1" ]; }
# A static skrongle user (DietPi-style). DynamicUser has no entry in /etc/passwd.
static_user() { grep -q '^skrongle:' /etc/passwd 2>/dev/null; }

# Remove a file or a folder. A symlink is removed as a link only: its target
# is not touched, so a link to a folder elsewhere cannot delete that folder.
remove() {
  if [ -L "$1" ] || [ -f "$1" ]; then
    rm -f -- "$1"; echo "removed $1"
  elif [ -d "$1" ]; then
    rm -rf -- "$1"; echo "removed $1"
  fi
}

found=
for p in "$UNIT" "$DROPIN" "$OPT" "$CONF" "$DATA" "$DATA_REAL" "$DATA_DIETPI"; do
  if exists "$p"; then found=1; fi
done
if static_user; then found=1; fi
if [ -z "$found" ]; then
  echo "no Skrongle install found: nothing to remove"
  exit 0
fi

# Where the service was told to keep the database, read before the unit is
# removed: the unit's Environment= line, then the env file on top, as
# systemd orders them. Empty means the default. A path outside the folders
# above is never touched (it may sit beside files that are not Skrongle's).
DB=
if [ -f "$UNIT" ]; then
  DB=$(sed -n 's/^Environment="\{0,1\}SKRONGLE_DB=\([^"]*\)"\{0,1\}[[:space:]]*$/\1/p' "$UNIT" | tail -n 1)
fi
if [ -f "$CONF/env" ]; then
  FROM_ENV=$(sed -n 's/^SKRONGLE_DB=//p' "$CONF/env" | tail -n 1)
  [ -z "$FROM_ENV" ] || DB=$FROM_ENV
fi
ELSEWHERE=
case "$DB" in
  ""|"$DATA"/*|"$DATA_REAL"/*|"$DATA_DIETPI"/*|"$OPT"/*|[!/]*) ;;
  *) ELSEWHERE=$DB ;;
esac
if [ -n "$ELSEWHERE" ]; then
  echo "note: the settings put the database at $ELSEWHERE. This script does not touch it;"
  echo "      remove it by hand if you want it gone."
fi

if [ -n "$HAVE_SYSTEMCTL" ]; then
  if systemctl is-active --quiet skrongle 2>/dev/null; then
    systemctl stop skrongle; echo "stopped the skrongle service"
  fi
  if systemctl is-enabled --quiet skrongle 2>/dev/null; then
    systemctl disable skrongle; echo "disabled the skrongle service"
  fi
else
  echo "no systemctl on this machine: any skrongle unit is not stopped or disabled"
fi

remove "$UNIT"
remove "$DROPIN"
if [ -n "$HAVE_SYSTEMCTL" ]; then
  systemctl daemon-reload
  systemctl reset-failed skrongle 2>/dev/null || true
fi

if static_user; then
  userdel skrongle
  echo "removed the skrongle user"
  if grep -q '^skrongle:' /etc/group 2>/dev/null; then
    groupdel skrongle 2>/dev/null || true
    echo "removed the skrongle group"
  fi
fi

if [ -n "$PURGE" ]; then
  remove "$DATA"
  remove "$DATA_REAL"
  rmdir /var/lib/private 2>/dev/null || true
  remove "$DATA_DIETPI"
  remove "$CONF"
else
  for d in "$DATA" "$DATA_REAL" "$DATA_DIETPI"; do
    if exists "$d"; then
      echo "kept $d: the database (accounts, progress, courses made in the app, saved AI keys) and its key"
    fi
  done
  if exists "$CONF"; then
    echo "kept $CONF: settings; its env file may hold SKRONGLE_SECRET_KEY"
  fi
fi

remove "$OPT"

echo
if [ -n "$PURGE" ] && [ -n "$ELSEWHERE" ]; then
  echo "Skrongle is removed. The database at $ELSEWHERE is NOT: remove it by hand."
elif [ -n "$PURGE" ]; then
  echo "Skrongle is removed, and its data with it."
else
  echo "Skrongle is removed. Its data is kept, so installing it again brings the accounts back."
  echo "To delete the data as well (permanent; back it up first, see docs/self-hosting.md):"
  echo "  sudo $0 --purge"
fi
