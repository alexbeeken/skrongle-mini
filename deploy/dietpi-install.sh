#!/bin/sh
# Install or update Skrongle the way DietPi does (docs/self-hosting.md,
# "Installing on DietPi"). Run it again to update; nothing is asked.
#
#   sudo ./dietpi-install.sh              # install, or update an install
#   sudo ./dietpi-install.sh --unit       # also write the systemd unit again
#   sudo ./dietpi-install.sh --uninstall  # remove it, keep the data
#   sudo ./dietpi-install.sh --uninstall --purge   # remove the data too
#
# The steps, in plain words:
#
#  1. Check that this runs as root, that curl is installed, and which
#     machine type this is (aarch64, x86_64, armv7l or armv6l).
#  2. Find the latest release of alexbeeken/skrongle-mini on GitHub.
#  3. Download skrongle-<version>-<arch>.tar.gz and the .sha256 file beside
#     it, and check the checksum. A mismatch stops everything.
#  4. Unpack it to a temporary folder and make sure the binary runs here.
#  5. Create the system user "skrongle" (no login shell) if it is missing.
#  6. Create /mnt/dietpi_userdata/skrongle, owned by that user, mode 0750.
#     The database lives there (learn.db, learn.db.key), so it must be on a
#     local disk, not a network mount.
#  7. If an install exists: stop the service and copy the database aside as
#     learn.db.before-<version> (and its -wal file, if there is one).
#  8. Put the new files in /opt/skrongle: replace the binary and data/
#     (the old binary is kept as skrongle.previous). The database is not
#     touched; the new binary migrates it when it starts.
#  9. Create /etc/skrongle/env (a short file of commented examples, mode
#     0640 root:skrongle) if it does not exist. An existing one is never
#     changed.
# 10. Write /etc/systemd/system/skrongle.service (user skrongle, port 7576,
#     sign-up by invitation, database in the folder from step 6) if it does
#     not exist, or when --unit is given. A unit you edited is left alone.
# 11. Reload systemd, enable the service, start it, and wait until it
#     answers on port 7576.
# 12. If nobody has an account yet, print the owner link
#     (skrongle owner-link --service); otherwise say the server is up and
#     where to open it.
#
# Every systemd step is skipped, with a note, on a machine without
# systemctl (a container).
#
# SKRONGLE_REPO=owner/repo picks another GitHub repository;
# SKRONGLE_VERSION=0.2.0 a given release instead of the latest.
set -eu

REPO="${SKRONGLE_REPO:-alexbeeken/skrongle-mini}"
OPT=/opt/skrongle
DATA=/mnt/dietpi_userdata/skrongle
CONF=/etc/skrongle
UNIT=/etc/systemd/system/skrongle.service
DROPIN=/etc/systemd/system/skrongle.service.d
PORT=7576

usage() { echo "usage: sudo $0 [--unit] | --uninstall [--purge]"; exit "$1"; }

WRITE_UNIT= UNINSTALL= PURGE=
for a in "$@"; do
  case "$a" in
    --unit) WRITE_UNIT=1 ;;
    --uninstall) UNINSTALL=1 ;;
    --purge) PURGE=1 ;;
    -h|--help) usage 0 ;;
    *) usage 2 ;;
  esac
done
if [ -n "$PURGE" ] && [ -z "$UNINSTALL" ]; then usage 2; fi
if [ -n "$UNINSTALL" ] && [ -n "$WRITE_UNIT" ]; then usage 2; fi

[ "$(id -u)" = 0 ] || { echo "run it as root: sudo $0 $*"; exit 1; }

HAVE_SYSTEMCTL=
if command -v systemctl >/dev/null 2>&1; then HAVE_SYSTEMCTL=1; fi
skipped() { echo "no systemctl on this machine: skipped $1"; }
static_user() { grep -q '^skrongle:' /etc/passwd 2>/dev/null; }

# ---------- uninstall ----------

# A symlink is removed as a link only; its target is not touched.
remove() {
  if [ -L "$1" ] || [ -f "$1" ]; then
    rm -f -- "$1"; echo "removed $1"
  elif [ -d "$1" ]; then
    rm -rf -- "$1"; echo "removed $1"
  fi
}

if [ -n "$UNINSTALL" ]; then
  if [ -n "$HAVE_SYSTEMCTL" ]; then
    if systemctl is-active --quiet skrongle 2>/dev/null; then
      systemctl stop skrongle; echo "stopped the skrongle service"
    fi
    if systemctl is-enabled --quiet skrongle 2>/dev/null; then
      systemctl disable skrongle; echo "disabled the skrongle service"
    fi
  else
    skipped "stopping and disabling the service"
  fi
  remove "$UNIT"
  remove "$DROPIN"
  if [ -n "$HAVE_SYSTEMCTL" ]; then
    systemctl daemon-reload
    systemctl reset-failed skrongle 2>/dev/null || true
  else
    skipped "systemctl daemon-reload"
  fi
  if static_user; then
    userdel skrongle
    echo "removed the skrongle user"
    if grep -q '^skrongle:' /etc/group 2>/dev/null; then
      groupdel skrongle 2>/dev/null || true
    fi
  fi
  if [ -n "$PURGE" ]; then
    remove "$DATA"
    remove "$CONF"
  else
    [ ! -e "$DATA" ] || echo "kept $DATA: the database (accounts, progress, saved AI keys) and its key"
    [ ! -e "$CONF" ] || echo "kept $CONF: its env file may hold SKRONGLE_SECRET_KEY"
  fi
  remove "$OPT"
  echo
  if [ -n "$PURGE" ]; then
    echo "Skrongle is removed, and its data with it."
  else
    echo "Skrongle is removed. Its data is kept, so installing it again brings the accounts back."
    echo "To delete the data as well (permanent): sudo $0 --uninstall --purge"
  fi
  exit 0
fi

# ---------- install or update ----------

command -v curl >/dev/null 2>&1 || { echo "needs curl: sudo apt install curl"; exit 1; }

case "$(uname -m)" in
  aarch64|arm64) ARCH=aarch64 ;;
  x86_64|amd64)  ARCH=x86_64 ;;
  armv7l)        ARCH=armv7l ;;
  armv6l)        ARCH=armv6l ;;
  *) echo "no release for $(uname -m); build from source (docs/self-hosting.md)"; exit 1 ;;
esac

if [ -n "${SKRONGLE_VERSION:-}" ]; then
  VERSION="${SKRONGLE_VERSION#v}"
else
  # /releases/latest redirects to /releases/tag/v<version>.
  URL=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest")
  VERSION="${URL##*/}"
  VERSION="${VERSION#v}"
  case "$VERSION" in
    ''|latest|releases) echo "could not find the latest release of $REPO"; exit 1 ;;
  esac
fi
CURRENT=$(cat "$OPT/VERSION" 2>/dev/null || echo none)

NAME="skrongle-$VERSION-$ARCH.tar.gz"
BASE="https://github.com/$REPO/releases/download/v$VERSION"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
echo "downloading $NAME"
curl -fL --progress-bar -o "$TMP/$NAME" "$BASE/$NAME"
curl -fsSL -o "$TMP/$NAME.sha256" "$BASE/$NAME.sha256" || { echo "could not download $NAME.sha256; nothing changed"; exit 1; }
(cd "$TMP" && sha256sum -c "$NAME.sha256" >/dev/null) || { echo "checksum mismatch for $NAME; nothing changed"; exit 1; }
echo "checksum ok"
mkdir "$TMP/new"
tar -xzf "$TMP/$NAME" -C "$TMP/new"
"$TMP/new/skrongle" --version >/dev/null || { echo "the new binary does not run on this machine; nothing changed"; exit 1; }

if static_user; then
  echo "user skrongle exists"
else
  useradd --system --no-create-home --home-dir "$DATA" --shell /usr/sbin/nologin skrongle
  echo "created the skrongle user"
fi
mkdir -p "$DATA"
# After an uninstall that kept the data, the user may have a new id.
chown -R skrongle:skrongle "$DATA"
chmod 0750 "$DATA"

if [ -n "$HAVE_SYSTEMCTL" ]; then
  echo "stopping skrongle ($CURRENT -> $VERSION)"
  systemctl stop skrongle 2>/dev/null || true
else
  skipped "stopping the service"
fi
DB="$DATA/learn.db"
if [ -f "$DB" ]; then
  cp -p "$DB" "$DB.before-$VERSION"
  if [ -s "$DB-wal" ]; then cp -p "$DB-wal" "$DB.before-$VERSION-wal"; fi
  echo "database copied to $DB.before-$VERSION"
fi

mkdir -p "$OPT"
[ ! -f "$OPT/skrongle" ] || cp -p "$OPT/skrongle" "$OPT/skrongle.previous"
rm -rf "$OPT/data"
cp -R "$TMP/new/." "$OPT/"
echo "installed $VERSION in $OPT"

mkdir -p "$CONF"
if [ -e "$CONF/env" ]; then
  echo "kept $CONF/env"
elif [ -f "$OPT/env.example" ]; then
  cp "$OPT/env.example" "$CONF/env"
  chown root:skrongle "$CONF/env"
  chmod 0640 "$CONF/env"
  echo "created $CONF/env"
else
  cat > "$CONF/env" <<'ENVEOF'
# /etc/skrongle/env  (root:skrongle, mode 0640; read by systemd)
#
# Every setting is optional; the unit file sets the ones a box needs. One
# NAME=value per line, then `sudo systemctl restart skrongle`. The full list
# is docs/self-hosting.md "Configuration". To see what the service ends up
# with: sudo /opt/skrongle/skrongle doctor --service

# The address people open, for the links the server shows (the owner link,
# invitation links).
#SKRONGLE_PUBLIC_URL=http://<hostname>.local:7576

# Where to listen. The unit sets every interface, port 7576. 127.0.0.1:7576
# answers only through Tailscale Serve or a proxy on the same machine.
#SKRONGLE_BIND=0.0.0.0:7576

# Optional: 64 hex characters that encrypt the AI keys saved in the app. Left
# unset, the server makes one next to the database (learn.db.key). Set it to
# keep the secret out of the data folder; then keep a copy of the value.
#SKRONGLE_SECRET_KEY=
ENVEOF
  echo "created $CONF/env"
  chown root:skrongle "$CONF/env"
  chmod 0640 "$CONF/env"
fi

if [ -e "$UNIT" ] && [ -z "$WRITE_UNIT" ]; then
  echo "kept $UNIT (--unit writes it again)"
else
  mkdir -p "$(dirname "$UNIT")"
  cat > "$UNIT" <<UNITEOF
[Unit]
Description=Skrongle learning app
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=skrongle
Group=skrongle
WorkingDirectory=$OPT
ExecStart=$OPT/skrongle
Restart=on-failure
# 78: the server refused the database (a newer release upgraded it, or it
# predates the 1.0 schema). Restarting cannot help.
RestartPreventExitStatus=78
RestartSec=3

Environment=SKRONGLE_BIND=0.0.0.0:$PORT
Environment=SKRONGLE_DATA_DIR=$OPT/data
Environment=SKRONGLE_DB=$DB
Environment=SKRONGLE_LOG=info
Environment=SKRONGLE_SIGNUP=invite
EnvironmentFile=-$CONF/env
Environment=MALLOC_ARENA_MAX=2
Environment=MALLOC_TRIM_THRESHOLD_=65536

MemoryMax=64M
MemoryHigh=48M
TasksMax=32
LimitNOFILE=256
CPUWeight=80
Nice=2

NoNewPrivileges=true
ProtectSystem=strict
ReadWritePaths=$DATA
ProtectHome=true
PrivateTmp=true
PrivateDevices=true
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectControlGroups=true
RestrictAddressFamilies=AF_INET AF_INET6
RestrictNamespaces=true
LockPersonality=true
SystemCallArchitectures=native

[Install]
WantedBy=multi-user.target
UNITEOF
  echo "wrote $UNIT"
fi

if [ -z "$HAVE_SYSTEMCTL" ]; then
  skipped "daemon-reload, enable and start"
  echo "Skrongle $VERSION is unpacked in $OPT; the service was not started."
  exit 0
fi

systemctl daemon-reload
systemctl enable skrongle >/dev/null 2>&1
systemctl restart skrongle
i=0
while [ "$i" -lt 20 ]; do
  if curl -fsS -o /dev/null "http://127.0.0.1:$PORT/healthz" 2>/dev/null; then break; fi
  i=$((i + 1))
  sleep 1
done
if ! systemctl is-active --quiet skrongle || [ "$i" -ge 20 ]; then
  echo "Skrongle $VERSION did not start; the log:"
  journalctl -u skrongle -n 20 --no-pager || true
  [ "$CURRENT" = none ] || echo "to go back: sudo $OPT/update.sh $CURRENT (the database from before is at $DB.before-$VERSION)"
  exit 1
fi

# The command opens the database, so run it as the service's user: files it
# creates (-wal, -shm) must not end up owned by root.
LINK=$(su -s /bin/sh skrongle -c "$OPT/skrongle owner-link --service" 2>&1) || LINK=
HOST=$(hostname -I 2>/dev/null | cut -d' ' -f1)
HOST="${HOST:-$(hostname)}"
case "$LINK" in
  "Owner link"*) echo "Skrongle $VERSION is up. Make your account with this link:"; echo "$LINK" ;;
  *) echo "Skrongle $VERSION is up: open http://$HOST:$PORT" ;;
esac
