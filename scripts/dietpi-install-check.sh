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
# keep the env file and unit and copy the database aside. The later runs
# get a stand-in `systemctl` (it starts and stops the binary and prints the
# unit's Environment= lines), so the script's systemd half runs too and its
# last lines, the owner link block, are checked (#124, #125). The stand-in
# answers every user, so it cannot show what real systemd refuses; that
# takes a real DietPi. Last it runs deploy/uninstall.sh without and with
# --purge (#127). One line per check, ok or FAIL; non-zero exit on any FAIL.
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
  SERVER=$(cat "$DATA/server.pid" 2>/dev/null) || SERVER=
  [ -z "$SERVER" ] || kill "$SERVER" 2>/dev/null
  [ -z "$PID" ] || wait "$PID" 2>/dev/null
  PID=
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

# 4. Run the script again, now with a stand-in systemctl on the PATH, so it
# goes on past "no systemctl" to its end: stop, copy the database aside,
# start, print the owner link block (#124, #125). An update keeps the env
# file and unit.
cat > /usr/local/bin/systemctl <<'STUBEOF'
#!/bin/sh
# A stand-in for systemd in scripts/dietpi-install-check.sh: stop, restart,
# is-active and `show -p Environment` for the skrongle unit. Anything else
# (daemon-reload, enable) does nothing.
DATA=/mnt/dietpi_userdata/skrongle
unit_env() { sed -n 's/^Environment=//p' /etc/systemd/system/skrongle.service | tr '\n' ' '; }
alive() { P=$(cat "$DATA/server.pid" 2>/dev/null) && [ -n "$P" ] && kill -0 "$P" 2>/dev/null; }
stop() { if alive; then kill "$P"; while alive; do sleep 1; done; fi; }
case "${1:-}" in
  stop) stop ;;
  restart)
    stop
    su -s /bin/sh skrongle -c "echo \$\$ >$DATA/server.pid; cd /opt/skrongle && exec env $(unit_env) /opt/skrongle/skrongle >>$DATA/server.log 2>&1" >/dev/null 2>&1 &
    # systemd would not wait, but under QEMU a start can take longer than
    # the 20 s the install script allows.
    i=0
    while [ "$i" -lt "${HEALTH_WAIT:-600}" ]; do
      if curl -fsS -o /dev/null http://127.0.0.1:7576/healthz 2>/dev/null; then break; fi
      i=$((i + 2)); sleep 2
    done
    ;;
  is-active) alive ;;
  show) case "$*" in *"-p Environment "*) unit_env; echo ;; esac ;;
esac
STUBEOF
chmod 0755 /usr/local/bin/systemctl
IP=$(hostname -I 2>/dev/null | cut -d' ' -f1)
IP="${IP:-$(hostname)}"
# The last line of the script's output that is not blank.
last_line() { grep -v '^$' "$LOG" | tail -n 1; }

if sh "$INSTALL" >"$LOG" 2>&1; then ok "a second run succeeds"; else fail "a second run succeeds"; fi
cat "$LOG"
grep -q "^kept /etc/skrongle/env" "$LOG" && ok "second run keeps the env file" || fail "second run keeps the env file"
grep -q "^kept /etc/systemd/system/skrongle.service" "$LOG" && ok "second run keeps the unit" || fail "second run keeps the unit"
[ -f "$DATA/learn.db.before-$VERSION" ] && ok "learn.db.before-$VERSION exists" || fail "learn.db.before-$VERSION exists"
[ -x /opt/skrongle/skrongle.previous ] && ok "skrongle.previous kept" || fail "skrongle.previous kept"
grep -q "^Skrongle $VERSION is installed and running\.$" "$LOG" \
  && ok "second run says installed and running" || fail "second run says installed and running"
grep -q "^  http://$IP:$PORT/register?invite=[A-Za-z0-9_-]*$" "$LOG" \
  && ok "the owner link is on a line of its own, with this machine's address ($IP)" \
  || fail "the owner link is on a line of its own, with this machine's address ($IP)"
case "$(last_line)" in
  "  It works once. For a new one: sudo /opt/skrongle/skrongle owner-link --service") ok "the owner link block ends the output" ;;
  *) fail "the owner link block ends the output (last line: $(last_line))" ;;
esac
NOT_OURS=$(find "$DATA" ! -user skrongle 2>/dev/null)
[ -z "$NOT_OURS" ] && ok "every file in $DATA is owned by skrongle" || fail "every file in $DATA is owned by skrongle (not: $NOT_OURS)"

# 5. With SKRONGLE_PUBLIC_URL set, the link is printed as the binary gave it.
echo "SKRONGLE_PUBLIC_URL=http://example.test:$PORT" >> /etc/skrongle/env
sh "$INSTALL" >"$LOG" 2>&1 || fail "a run with SKRONGLE_PUBLIC_URL succeeds"
if grep -q "^  http://example.test:$PORT/register?invite=" "$LOG"; then
  ok "with SKRONGLE_PUBLIC_URL the link starts with it"
else
  cat "$LOG"; fail "with SKRONGLE_PUBLIC_URL the link starts with it"
fi

# 6. A failing owner-link is shown, not swallowed: the skrongle user cannot
# read an env file that is root's alone.
chmod 0600 /etc/skrongle/env
sh "$INSTALL" >"$LOG" 2>&1 || fail "a run where owner-link fails still ends well"
if grep -q "cannot read /etc/skrongle/env" "$LOG"; then
  ok "a failing owner-link shows what it said"
else
  cat "$LOG"; fail "a failing owner-link shows what it said"
fi
case "$(last_line)" in
  "Skrongle $VERSION is up: open http://$IP:$PORT") ok "and the run ends with the is-up line" ;;
  *) fail "and the run ends with the is-up line (last line: $(last_line))" ;;
esac
chmod 0640 /etc/skrongle/env

# 7. Once the owner has registered, a run ends with the is-up line alone.
TOKEN=$(run_as "cd /opt/skrongle && /opt/skrongle/skrongle owner-link" 2>&1 | sed -n 's/.*\/register?invite=//p')
CODE=$(curl -s -o /dev/null -w '%{http_code}' --data-urlencode "invite=$TOKEN" \
  --data-urlencode "email=owner@example.test" --data-urlencode "password=a long test password 7" \
  --data-urlencode "age=on" "http://127.0.0.1:$PORT/register")
[ "$CODE" = 303 ] && ok "the owner registers with the link" || fail "the owner registers with the link (HTTP $CODE)"
sh "$INSTALL" >"$LOG" 2>&1 || fail "a run with an account succeeds"
if grep -q "register?invite=\|could not make the owner link" "$LOG"; then
  cat "$LOG"; fail "with an account, no link and no error text"
else
  ok "with an account, no link and no error text"
fi
case "$(last_line)" in
  "Skrongle $VERSION is up: open http://$IP:$PORT") ok "and the run ends with the is-up line" ;;
  *) fail "and the run ends with the is-up line (last line: $(last_line))" ;;
esac

# 8. Uninstall keeps the data folder, --purge removes it (#127). This is the
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
