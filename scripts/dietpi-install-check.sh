#!/bin/sh
# The DietPi install block against a published release (#31), in a Debian
# container, as root. .github/workflows/dietpi-install.yml runs it per
# architecture; to run what CI runs:
#
#   docker run --rm -v "$PWD:/repo:ro" -e SKRONGLE_VERSION=0.2.0 \
#     debian:bookworm-slim sh /repo/scripts/dietpi-install-check.sh amd64
#
# SKRONGLE_VERSION unset checks the latest release. It runs
# deploy/dietpi-install.sh, starts the installed binary by hand (a
# container has no systemd) with the environment the unit gives it, checks
# /healthz and `owner-link`, then runs the script again to see an update
# keep the env file and unit and copy the database aside, and last runs
# deploy/uninstall.sh without and with --purge. One line per check, ok or
# FAIL; non-zero exit on any FAIL.
set -u

LABEL="${1:-container}"
HERE=$(cd "$(dirname "$0")" && pwd)
INSTALL="$HERE/../deploy/dietpi-install.sh"
DATA=/mnt/dietpi_userdata/skrongle
PORT=7576
WAIT="${HEALTH_WAIT:-600}" # seconds; QEMU's first start is slow
FAILED=0
PID=
LOG=$(mktemp)

ok()   { echo "  ok   $1"; }
fail() { echo "  FAIL $1"; FAILED=1; }
stop_server() {
  [ -z "$PID" ] || kill "$(cat "$DATA/server.pid" 2>/dev/null)" 2>/dev/null
  wait "$PID" 2>/dev/null; PID=
}
finish() {
  stop_server
  if [ "$FAILED" = 1 ]; then
    echo "==> server log:"; tail -n 30 "$DATA/server.log" 2>/dev/null
    echo "==> DietPi install check FAILED on $LABEL"; exit 1
  fi
  echo "==> DietPi install check passed on $LABEL"; exit 0
}
trap 'rm -f "$LOG"' EXIT
die() { fail "$1"; finish; }

[ "$(id -u)" = 0 ] || { echo "run as root (in a container)"; exit 2; }
[ -f "$INSTALL" ] || { echo "no $INSTALL"; exit 2; }
echo "==> $LABEL: $(uname -m), ${SKRONGLE_VERSION:-latest release}"

if ! command -v curl >/dev/null 2>&1; then
  apt-get update -qq >/dev/null && apt-get install -y -qq curl ca-certificates >/dev/null \
    || { echo "could not install curl"; exit 2; }
fi

# 1. The first install.
if sh "$INSTALL" >"$LOG" 2>&1; then ok "dietpi-install.sh installs"; cat "$LOG"; else cat "$LOG"; die "dietpi-install.sh installs"; fi
VERSION=$(cat /opt/skrongle/VERSION 2>/dev/null || true)
[ -n "$VERSION" ] && ok "installed version $VERSION" || die "/opt/skrongle/VERSION is missing"
if [ -n "${SKRONGLE_VERSION:-}" ] && [ "$VERSION" != "${SKRONGLE_VERSION#v}" ]; then
  die "installed $VERSION, asked for $SKRONGLE_VERSION"
fi
grep -q "^skrongle:" /etc/passwd && ok "user skrongle exists" || fail "user skrongle exists"
[ -f /etc/skrongle/env ] && ok "/etc/skrongle/env written" || fail "/etc/skrongle/env written"
[ -f /etc/systemd/system/skrongle.service ] && ok "unit file written" || fail "unit file written"
grep -q "no systemctl" "$LOG" && ok "systemd steps skipped with a note" || fail "systemd steps skipped with a note"

# 2. Start it as the unit would.
run_as() {
  su -s /bin/sh skrongle -c "export SKRONGLE_BIND=127.0.0.1:$PORT SKRONGLE_DB=$DATA/learn.db SKRONGLE_DATA_DIR=/opt/skrongle/data SKRONGLE_SIGNUP=invite; $1"
}
# The shell records its pid, then becomes the server, so the pid is the server's.
run_as "echo \$\$ >$DATA/server.pid; cd /opt/skrongle && exec /opt/skrongle/skrongle >$DATA/server.log 2>&1" &
PID=$!
i=0; BODY=
while [ "$i" -lt "$WAIT" ]; do
  BODY=$(curl -fsS "http://127.0.0.1:$PORT/healthz" 2>/dev/null) && break
  kill -0 "$PID" 2>/dev/null || die "the server stays up (it exited; log below)"
  BODY=; i=$((i + 2)); sleep 2
done
[ -n "$BODY" ] || die "/healthz answers within ${WAIT}s"
ok "/healthz answers (after ~${i}s)"
case "$BODY" in
  *"skrongle mini $VERSION"*) ok "/healthz says: $BODY" ;;
  *) fail "/healthz says skrongle mini $VERSION (got: $BODY)" ;;
esac

# 3. The owner link, as the service's user.
LINK=$(run_as "cd /opt/skrongle && /opt/skrongle/skrongle owner-link" 2>&1)
case "$LINK" in
  *"/register?invite="*) ok "owner-link prints a /register?invite= link" ;;
  *) echo "$LINK"; fail "owner-link prints a /register?invite= link" ;;
esac

# 4. Run the script again: an update keeps the env and unit and copies the
# database aside. Stop the server first, as systemctl stop would.
stop_server
if sh "$INSTALL" >"$LOG" 2>&1; then ok "a second run succeeds"; else fail "a second run succeeds"; fi
cat "$LOG"
grep -q "^kept /etc/skrongle/env" "$LOG" && ok "second run keeps the env file" || fail "second run keeps the env file"
grep -q "^kept /etc/systemd/system/skrongle.service" "$LOG" && ok "second run keeps the unit" || fail "second run keeps the unit"
[ -f "$DATA/learn.db.before-$VERSION" ] && ok "learn.db.before-$VERSION exists" || fail "learn.db.before-$VERSION exists"
[ -x /opt/skrongle/skrongle.previous ] && ok "skrongle.previous kept" || fail "skrongle.previous kept"

# 5. Uninstall keeps the data folder, --purge removes it (#127). This is the
# repository's uninstall.sh: at a release tag, the one in the tarball.
UNINSTALL="$HERE/../deploy/uninstall.sh"
if sh "$UNINSTALL" >"$LOG" 2>&1; then ok "uninstall.sh runs"; else fail "uninstall.sh runs"; fi
cat "$LOG"
[ ! -e /opt/skrongle ] && ok "uninstall removes /opt/skrongle" || fail "uninstall removes /opt/skrongle"
[ -f "$DATA/learn.db" ] && ok "uninstall keeps $DATA/learn.db" || fail "uninstall keeps $DATA/learn.db"
grep -q "^kept $DATA" "$LOG" && ok "uninstall says it kept $DATA" || fail "uninstall says it kept $DATA"
if sh "$UNINSTALL" --purge >"$LOG" 2>&1; then ok "uninstall.sh --purge runs"; else fail "uninstall.sh --purge runs"; fi
cat "$LOG"
[ ! -e "$DATA" ] && ok "--purge removes $DATA" || fail "--purge removes $DATA"
[ ! -e /etc/skrongle ] && ok "--purge removes /etc/skrongle" || fail "--purge removes /etc/skrongle"
grep -q "and its data with it" "$LOG" && ok "--purge says the data is gone" || fail "--purge says the data is gone"
finish
