#!/bin/sh
# The release tarball in a real VM with systemd (Lima), per architecture: the
# layer the containers of matrix.sh cannot reach (issue #42; docs/testing.md,
# "Release QA", "VMs"). Follows docs/self-hosting.md: the "Install" block is
# run as written (its code block is read from the doc, not copied here),
# update.sh and uninstall.sh are the ones from the tarball.
#
#   scripts/release-qa/vm.sh <arch> <command>
#
#   arch       aarch64 (Apple's Virtualization framework on an Apple Silicon Mac)
#              x86_64  (needs QEMU: `brew install qemu`; written, not yet run)
#   up         create and start the VM (skrongle-qa-<arch>, Debian 12, 1 CPU, 512 MiB)
#   install    start from nothing and follow "Install" with the candidate tarball,
#              then check the unit, /healthz, the owner link in the journal,
#              `skrongle doctor --service`, the state folder's mode, the owner's sign-in
#   update     start from nothing, install the newest published release, update to
#              the candidate through update.sh's own download and checksum path
#              (a bad checksum is refused first), then go back to the published
#              release the documented way (`update.sh <version>`, which first takes
#              the schema back with the installed binary's `skrongle migrate` when
#              the published release's is older: checked, with the database's
#              schema after each step); the server must start and the owner must
#              still sign in at each step. Last, upgrade-check.sh runs in the
#              guest on the candidate tarball and every fixture (#43), its log
#              in results/release-qa/<version>/<arch>-vm.upgrade.log
#              (UPGRADE_CHECK=0 leaves it out)
#   e2e        the Playwright projects `release-first-run` and `release` from this
#              machine against the guest's forwarded port (a fresh database first)
#   measure    results/release-qa/<version>/<arch>-vm.txt: the service's memory,
#              binary size, start-up time
#   uninstall  uninstall.sh plain (the data stays, a reinstall brings the owner
#              back), then --purge (nothing left)
#   down       stop and delete the VM
#   shell      a shell in the VM
#
# The candidate is the newest dist/skrongle-*-<arch>.tar.gz (+ .sha256), built
# with scripts/build-pi.sh and scripts/package-release.sh; TARBALL=path picks
# another. PUBLISHED=<version> picks the "newest published" release (default:
# the GitHub latest). Only the instance named skrongle-qa-<arch> is touched.
set -eu

HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
REPO=alexbeeken/skrongle-mini

usage() { sed -n '2,/^set -eu/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'; exit "${1:-2}"; }
[ $# -ge 2 ] || usage 2
ARCH=$1; CMD=$2
case "$ARCH" in
  aarch64) HOSTPORT=18576 ;;
  x86_64)  HOSTPORT=18577 ;;
  *) echo "architecture: aarch64 or x86_64"; usage 2 ;;
esac
NAME=skrongle-qa-$ARCH
command -v limactl >/dev/null || { echo "needs Lima: brew install lima"; exit 1; }

PASS=0; FAILED=0
ok()   { PASS=$((PASS + 1)); echo "  ok    $*"; }
fail() { FAILED=$((FAILED + 1)); echo "  FAIL  $*"; }
finish() {
  echo "$PASS ok, $FAILED failed"
  [ "$FAILED" = 0 ]
}

# shellcheck disable=SC2120,SC2119
# Run a script (stdin) as root in the guest, from /tmp.
gsh() { limactl shell --workdir /tmp "$NAME" sudo sh -s -- "$@"; }
# check <what> <script>: the script's exit status is the verdict.
check() { if printf '%s\n' "$2" | gsh >/dev/null 2>&1; then ok "$1"; else fail "$1"; fi; }
# A script's output, as the answer to something.
gout() { printf '%s\n' "$1" | gsh 2>&1; }

vm_exists() { limactl list -q 2>/dev/null | grep -qx "$NAME"; }
need_vm() {
  vm_exists || { echo "no VM: scripts/release-qa/vm.sh $ARCH up"; exit 1; }
}

candidate() {
  TAR=${TARBALL:-}
  [ -n "$TAR" ] || TAR=$(ls -t "$ROOT"/dist/skrongle-*-"$ARCH".tar.gz 2>/dev/null | head -1 || true)
  [ -f "${TAR:-}" ] || { echo "no candidate: build dist/skrongle-<version>-$ARCH.tar.gz (scripts/build-pi.sh $ARCH; scripts/package-release.sh <version> $ARCH)"; exit 1; }
  [ -f "$TAR.sha256" ] || { echo "no $TAR.sha256"; exit 1; }
  CAND=$(basename "$TAR" .tar.gz); CAND=${CAND#skrongle-}; CAND=${CAND%-"$ARCH"}
}

published() {
  if [ -n "${PUBLISHED:-}" ]; then PUB=${PUBLISHED#v}; return; fi
  PUB=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest")
  PUB=${PUB##*/v}
  case "$PUB" in ''|*/*) echo "could not find the latest release"; exit 1 ;; esac
}

# The code block under "## Install" in docs/self-hosting.md.
install_block() {
  awk '/^## Install/ { s = 1; next } s && /^```sh/ { b = 1; next } b && /^```/ { exit } b { print }' "$ROOT/docs/self-hosting.md"
}

gwait() { # wait for /healthz in the guest
  printf '%s\n' 'for i in $(seq 1 120); do curl -fsS http://127.0.0.1:7576/healthz >/dev/null 2>&1 && exit 0; sleep 1; done; exit 1' | gsh >/dev/null 2>&1
}

# Take everything away (a purge) so the next step starts from nothing.
clean_slate() {
  gout '[ ! -x /opt/skrongle/uninstall.sh ] || sh /opt/skrongle/uninstall.sh --purge
rm -rf /tmp/qa-dl /var/lib/skrongle /var/lib/private/skrongle /etc/skrongle /opt/skrongle /etc/systemd/system/skrongle.service
systemctl daemon-reload' >/dev/null
}

# install_candidate: the doc's Install block with the downloads replaced by a copy.
install_candidate() {
  candidate
  clean_slate
  gout 'mkdir -p /tmp/qa-dl && chmod 777 /tmp/qa-dl' >/dev/null
  limactl copy "$TAR" "$TAR.sha256" "$NAME:/tmp/qa-dl/" >/dev/null
  { echo "set -e; cd /tmp/qa-dl; V=$CAND"
    install_block | grep -v 'curl ' ; } | gsh 2>&1 | grep -v 'Ignoring unknown extended header' || true
}

# install_published: the doc's Install block, unchanged (newest release from GitHub).
install_published() {
  clean_slate
  gout 'mkdir -p /tmp/qa-dl' >/dev/null
  { echo "set -e; cd /tmp/qa-dl"; [ -z "${PUBLISHED:-}" ] || echo "V=$PUB"
    install_block | { if [ -n "${PUBLISHED:-}" ]; then grep -v '^V='; else cat; fi; }; } | gsh 2>&1 | grep -v 'Ignoring unknown extended header' || true
}

OWNER_EMAIL=qa-owner@example.org
OWNER_PW=a-long-enough-password
JAR=$(mktemp)
trap 'rm -f "$JAR"; [ -z "${HTTP_PID:-}" ] || { kill "$HTTP_PID"; wait "$HTTP_PID"; } 2>/dev/null || true; [ -z "${SERVE:-}" ] || rm -rf "$SERVE"' EXIT
URL=http://127.0.0.1:$HOSTPORT

http_code() { # method path [curl args]
  m=$1; p=$2; shift 2
  curl -s -o /dev/null -w '%{http_code}' -b "$JAR" -c "$JAR" -X "$m" "$@" "$URL$p"
}
login_code() { : > "$JAR"; http_code POST /login --data-urlencode "email=$OWNER_EMAIL" --data-urlencode "password=$OWNER_PW"; }

# Make the owner through the owner link in the journal (the second time, sign in).
ensure_owner() {
  [ "$(login_code)" = 303 ] && return 0
  # The documented way (Install, step 3). It makes a new link each time it runs,
  # so the one in the journal may be out of date by now.
  LINK=$(gout '/opt/skrongle/skrongle owner-link --service' | grep -o "invite=[A-Za-z0-9_-]*" | tail -1)
  [ -n "$LINK" ] || { reset_db; LINK=$(gout '/opt/skrongle/skrongle owner-link --service' | grep -o "invite=[A-Za-z0-9_-]*" | tail -1); }
  : > "$JAR"
  code=$(http_code POST /register --data-urlencode "email=$OWNER_EMAIL" --data-urlencode "password=$OWNER_PW" --data age=on --data "invite=${LINK#invite=}")
  [ "$code" = 303 ] || { echo "could not register the owner: $code"; return 1; }
}

# A fresh database: a new owner link in the journal.
reset_db() {
  gout 'systemctl stop skrongle
rm -f /var/lib/private/skrongle/learn.db*
systemctl start skrongle' >/dev/null
  gwait
}

# The installed version is the candidate, else a clean install of it.
ensure_candidate() {
  candidate
  have=$(gout 'cat /opt/skrongle/VERSION 2>/dev/null || true')
  if [ "$have" != "$CAND" ]; then echo "(installing $CAND first; the VM has '${have:-nothing}')"; install_candidate >/dev/null 2>&1; gwait; fi
}

post_install_checks() { # expected version, "fresh" on a database with no owner yet
  want=$1; fresh=${2:-}
  check "the unit is active" 'systemctl is-active --quiet skrongle'
  check "the unit is enabled" 'systemctl is-enabled --quiet skrongle'
  gwait && ok "/healthz answers in the guest" || fail "/healthz does not answer in the guest"
  curl -fsS "$URL/healthz" >/dev/null 2>&1 && ok "/healthz answers on this machine ($URL, forwarded)" || fail "/healthz does not answer on $URL"
  check "installed VERSION is $want" "[ \"\$(cat /opt/skrongle/VERSION)\" = '$want' ]"
  if [ -n "$fresh" ]; then
    check "the owner link is in the journal" 'journalctl -u skrongle --no-pager | grep -q "Owner link.*invite="'
    check "skrongle owner-link --service prints a link" '/opt/skrongle/skrongle owner-link --service | grep -q "invite="'
  fi
  doc=$(gout '/opt/skrongle/skrongle doctor --service')
  case "$doc" in *PROBLEM:*) fail "skrongle doctor --service: $(printf '%s\n' "$doc" | grep PROBLEM:)" ;; *Version*|*version*) ok "skrongle doctor --service shows no PROBLEM: line" ;; *) fail "skrongle doctor printed nothing useful: $doc" ;; esac
  unit_mode=$(gout "sed -n 's/^StateDirectoryMode=//p' /etc/systemd/system/skrongle.service" | sed 's/^0*//')
  real_mode=$(gout 'stat -c %a /var/lib/private/skrongle')
  if [ -n "$unit_mode" ] && [ "$unit_mode" = "$real_mode" ]; then ok "the state folder's mode is $real_mode, as StateDirectoryMode says"; else fail "state folder mode $real_mode, the unit says '$unit_mode'"; fi
  check "/var/lib/skrongle leads to the state folder" '[ -d /var/lib/skrongle ] && [ -f /var/lib/skrongle/learn.db ]'
}

# The database's schema, as the installed binary's doctor reads it (through
# the write-ahead log, which the file's own header may not show yet).
DB_SCHEMA="/opt/skrongle/skrongle doctor --service | sed -n 's/^[[:space:]]*schema[:[:space:]]*v\([0-9][0-9]*\).*/\1/p' | head -1"

signs_in() { # what
  if ensure_owner; then ok "$1: the owner signs in"; else fail "$1: the owner cannot sign in"; fi
}

case "$CMD" in
  up)
    if [ "$ARCH" = x86_64 ] && ! command -v qemu-system-x86_64 >/dev/null; then
      echo "x86_64 needs QEMU: brew install qemu (not installed here; this path has not been run)"; exit 1
    fi
    if vm_exists; then
      limactl start "$NAME" >/dev/null 2>&1 || true
    else
      limactl start --name="$NAME" --tty=false "$HERE/lima/$ARCH.yaml"
    fi
    check "systemd is running in the guest" '[ "$(systemctl is-system-running --wait 2>/dev/null)" = running ] || systemctl is-system-running | grep -q degraded'
    check "the guest is $ARCH" "[ \"\$(uname -m)\" = $ARCH ]"
    check "the guest has curl" 'command -v curl'
    check "the guest has 512 MiB or a little less" '[ "$(awk "/MemTotal/ {print int(\$2/1024)}" /proc/meminfo)" -lt 520 ]'
    finish
    ;;
  install)
    need_vm
    echo "==> docs/self-hosting.md, Install, with the candidate"
    install_candidate
    echo "==> checks"
    gwait || true
    post_install_checks "$CAND" fresh
    signs_in "after install"
    finish
    ;;
  update)
    need_vm; candidate; published
    echo "==> newest published release: $PUB (Install block, from GitHub)"
    install_published
    gwait || true
    post_install_checks "$PUB" fresh
    signs_in "published $PUB"
    schema_pub=$(gout '/opt/skrongle/skrongle --schema-version')
    echo "==> serve the candidate from this machine, for update.sh"
    SERVE=$(mktemp -d)
    cp -p "$TAR" "$TAR.sha256" "$SERVE/"
    # A copy whose checksum is wrong: update.sh must refuse it and change nothing.
    BAD=$(echo "$CAND" | sed 's/$/-badsum/')
    cp -p "$TAR" "$SERVE/skrongle-$BAD-$ARCH.tar.gz"
    echo "0000000000000000000000000000000000000000000000000000000000000000  skrongle-$BAD-$ARCH.tar.gz" > "$SERVE/skrongle-$BAD-$ARCH.tar.gz.sha256"
    HTTPPORT=$((18600 + $$ % 300))
    python3 -m http.server "$HTTPPORT" --bind 0.0.0.0 --directory "$SERVE" >/dev/null 2>&1 &
    HTTP_PID=$!
    sleep 1
    HOSTNAME_G=host.lima.internal
    gout "getent hosts $HOSTNAME_G" | grep -q . || HOSTNAME_G=$(gout "ip route | awk '/default/ {print \$3}'")
    BASE="http://$HOSTNAME_G:$HTTPPORT"
    check "the guest reaches $BASE" "curl -fsS -o /dev/null $BASE/skrongle-$CAND-$ARCH.tar.gz.sha256"
    # The update.sh installed by the published release predates SKRONGLE_RELEASE_BASE, so
    # the candidate's own copy is put in its place first (it is what the next release ships).
    tar -xzOf "$TAR" ./update.sh | limactl shell --workdir /tmp "$NAME" sudo tee /opt/skrongle/update.sh >/dev/null
    check "the candidate's update.sh is in place" 'grep -q SKRONGLE_RELEASE_BASE /opt/skrongle/update.sh'
    echo "==> update.sh with a wrong checksum is refused"
    out=$(gout "SKRONGLE_RELEASE_BASE=$BASE sh /opt/skrongle/update.sh $BAD; echo status=\$?") || true
    case "$out" in *"checksum mismatch"*status=1*) ok "refused: checksum mismatch, exit 1" ;; *) fail "a wrong checksum was not refused: $out" ;; esac
    check "nothing changed: still $PUB and active" "[ \"\$(cat /opt/skrongle/VERSION)\" = '$PUB' ] && systemctl is-active --quiet skrongle"
    echo "==> update.sh $CAND (SKRONGLE_RELEASE_BASE=$BASE)"
    force=""; [ "$CAND" != "$PUB" ] || force="FORCE=1"
    out=$(gout "SKRONGLE_RELEASE_BASE=$BASE $force sh /opt/skrongle/update.sh $CAND; echo status=\$?") || true
    printf '%s\n' "$out" | sed 's/^/    | /' | grep -v '###\|%\|Ignoring unknown extended header' || true
    case "$out" in *"Skrongle $CAND is running"*status=0*) ok "update.sh: Skrongle $CAND is running" ;; *) fail "update.sh did not end well" ;; esac
    case "$out" in *"downloading skrongle-$CAND-$ARCH.tar.gz"*) ok "it downloaded from $BASE, not GitHub" ;; *) fail "no download line" ;; esac
    check "the previous binary is kept as skrongle.previous" '[ -x /opt/skrongle/skrongle.previous ]'
    # Since #165 an update copies no database (docs/self-hosting.md "Updating");
    # going back is `skrongle migrate`, checked below, not a learn.db.before-<version>.
    check "update.sh made no copy of the database (no learn.db.before-*)" '! ls /var/lib/private/skrongle | grep -q "^learn.db.before"'
    post_install_checks "$CAND"
    signs_in "updated to $CAND"
    schema_cand=$(gout '/opt/skrongle/skrongle --schema-version')
    echo "    (schema $schema_pub -> $schema_cand)"
    db_schema=$(gout "$DB_SCHEMA")
    if [ "$db_schema" = "$schema_cand" ]; then ok "the database is at schema v$db_schema"; else fail "the database is at schema v$db_schema, expected v$schema_cand"; fi
    # Going back (docs/self-hosting.md "Updating"; docs/packaging.md "Upgrade"):
    # `update.sh <older version>`. Across a schema change the installed
    # binary first takes the database back (`skrongle migrate`), then the
    # older release is put in place and started.
    echo "==> going back: update.sh $PUB (GitHub; docs/self-hosting.md, Updating; schema v$schema_cand -> v$schema_pub)"
    out=$(gout "$force sh /opt/skrongle/update.sh $PUB; echo status=\$?") || true
    printf '%s\n' "$out" | sed 's/^/    | /' | grep -v '###\|%\|Ignoring unknown extended header' || true
    case "$out" in *"Skrongle $PUB is running"*status=0*) ok "update.sh $PUB: Skrongle $PUB is running" ;; *) fail "going back did not end well" ;; esac
    if [ "$schema_pub" -lt "$schema_cand" ]; then
      case "$out" in
        *"taking the database back to schema v$schema_pub"*"schema v$schema_cand -> v$schema_pub"*) ok "update.sh took the database back first: skrongle migrate $schema_pub (v$schema_cand -> v$schema_pub)" ;;
        *) fail "update.sh did not take the database back to v$schema_pub with skrongle migrate" ;;
      esac
    fi
    db_schema=$(gout "$DB_SCHEMA")
    if [ "$db_schema" = "$schema_pub" ]; then ok "the database is at schema v$db_schema, $PUB's"; else fail "the database is at schema v$db_schema, expected v$schema_pub"; fi
    check "no root-owned file left beside the database" '! find /var/lib/private/skrongle -user root ! -path /var/lib/private/skrongle | grep -q .'
    check "the journal has no refusal (exit 78) since going back" "! journalctl -u skrongle --no-pager --since '-2 min' | grep -q 'status=78'"
    post_install_checks "$PUB"
    signs_in "back on $PUB"
    if [ -n "${BACK_TO:-}" ]; then
      # Further back, to a release with an older schema, if the caller names one:
      # the installed binary first takes the database back (skrongle migrate).
      echo "==> and further back: update.sh $BACK_TO (BACK_TO=$BACK_TO); schema is $(gout '/opt/skrongle/skrongle --schema-version')"
      out=$(gout "sh /opt/skrongle/update.sh $BACK_TO; echo status=\$?") || true
      printf '%s\n' "$out" | sed 's/^/    | /' | grep -v '###\|%\|Ignoring unknown extended header' || true
      case "$out" in *"Skrongle $BACK_TO is running"*status=0*) ok "update.sh $BACK_TO: Skrongle $BACK_TO is running" ;; *) fail "going back to $BACK_TO did not end well" ;; esac
      post_install_checks "$BACK_TO"
      signs_in "back on $BACK_TO"
    fi
    if [ "${UPGRADE_CHECK:-1}" != 0 ]; then
      # The same upgrade check matrix.sh runs in a container, here on the VM's
      # own Debian and kernel: the candidate on every fixture, up, the older
      # release refusing it, and back down (it downloads the older releases).
      # As the guest's user, not root, on a copy of the tarball and fixtures.
      echo "==> upgrade-check.sh in the guest ($CAND on every fixture)"
      gout 'rm -rf /tmp/qa-up && mkdir -p /tmp/qa-up/fixtures && chmod -R 777 /tmp/qa-up' >/dev/null
      limactl copy "$TAR" "$HERE/upgrade-check.sh" "$NAME:/tmp/qa-up/" >/dev/null
      limactl copy "$HERE"/fixtures/learn-* "$NAME:/tmp/qa-up/fixtures/" >/dev/null
      ulog="$ROOT/results/release-qa/$CAND/$ARCH-vm.upgrade.log"
      mkdir -p "$(dirname "$ulog")"
      if limactl shell --workdir /tmp/qa-up "$NAME" sh ./upgrade-check.sh "./$(basename "$TAR")" > "$ulog" 2>&1; then
        ok "upgrade-check.sh in the guest: $(tail -1 "$ulog")"
      else
        fail "upgrade-check.sh in the guest: $(tail -1 "$ulog") ($ulog)"
        grep 'FAIL' "$ulog" | head -8 | sed 's/^/    | /'
      fi
      echo "    ($(grep -c '^  ok' "$ulog") ok, $(grep -c '^  FAIL' "$ulog") FAIL, $(grep -c '^  note' "$ulog") note; $(grep -c 'back to v' "$ulog") 'back to vN' lines)"
    fi
    finish
    ;;
  e2e)
    need_vm; ensure_candidate
    reset_db
    LINK=$(gout '/opt/skrongle/skrongle owner-link --service' | grep -o "invite=[A-Za-z0-9_-]*" | tail -1)
    [ -n "$LINK" ] || { echo "no owner link in the journal"; exit 1; }
    if [ ! -d "$ROOT/e2e/node_modules/@playwright/test" ]; then
      (cd "$ROOT/e2e" && npm ci --no-fund --no-audit && npx playwright install chromium)
    fi
    echo "==> Playwright --project release against $URL (guest $ARCH, $CAND)"
    cd "$ROOT/e2e"
    if E2E_BASE_URL=$URL E2E_OWNER_LINK="$URL/register?$LINK" npx playwright test --project release; then ok "release specs pass"; else fail "release specs failed (npx --prefix e2e playwright show-report e2e/results/report)"; fi
    finish
    ;;
  measure)
    need_vm; ensure_candidate
    gwait || true
    ver=$(gout 'cat /opt/skrongle/VERSION')
    out="$ROOT/results/release-qa/$ver/$ARCH-vm.txt"
    mkdir -p "$(dirname "$out")"
    # Idle after a restart: start-up is the time from `systemctl start` to the first /healthz answer.
    report=$(gout '
pid=$(systemctl show skrongle -p MainPID --value)
systemctl restart skrongle
t0=$(date +%s%N)
while ! curl -fsS http://127.0.0.1:7576/healthz >/dev/null 2>&1; do sleep 0.05; done
t1=$(date +%s%N)
echo "start_up_ms=$(( (t1 - t0) / 1000000 ))"
sleep 5
pid=$(systemctl show skrongle -p MainPID --value)
echo "rss_kb=$(awk "/VmRSS/ {print \$2}" /proc/$pid/status)"
echo "hwm_kb=$(awk "/VmHWM/ {print \$2}" /proc/$pid/status)"
echo "threads=$(awk "/Threads/ {print \$2}" /proc/$pid/status)"
echo "cgroup_memory_bytes=$(systemctl show skrongle -p MemoryCurrent --value)"
echo "memory_max=$(systemctl show skrongle -p MemoryMax --value)"
echo "binary_bytes=$(stat -c %s /opt/skrongle/skrongle)"
echo "kernel=$(uname -r)"
echo "guest_mem_total_mb=$(awk "/MemTotal/ {print int(\$2/1024)}" /proc/meminfo)"
echo "cpus=$(nproc)"')
    { echo "Skrongle $ver on $ARCH in the Lima VM $NAME (Debian 12, systemd, 1 CPU, 512 MiB)"
      echo "measured $(date -u +%Y-%m-%dT%H:%M:%SZ); idle, 5 s after a restart; the virtual CPU is not a Pi's, so compare memory and size, not time"
      echo "$report"; } > "$out"
    cat "$out"
    grep -q '^rss_kb=[0-9]' "$out" && ok "wrote $out" || fail "no RSS in $out"
    finish
    ;;
  uninstall)
    need_vm; ensure_candidate
    gwait || true
    signs_in "before"
    echo "==> uninstall.sh (the data stays)"
    out=$(gout 'sh /opt/skrongle/uninstall.sh; echo status=$?') || true
    printf '%s\n' "$out" | sed 's/^/    | /'
    case "$out" in *status=0*) ok "uninstall.sh exits 0" ;; *) fail "uninstall.sh failed" ;; esac
    check "the unit file is gone" '[ ! -e /etc/systemd/system/skrongle.service ]'
    check "/opt/skrongle is gone" '[ ! -e /opt/skrongle ]'
    check "the service is stopped" '! systemctl is-active --quiet skrongle'
    check "the service is not enabled" '! systemctl is-enabled --quiet skrongle 2>/dev/null'
    check "no skrongle user (DynamicUser has none)" '! getent passwd skrongle'
    check "the database is kept (/var/lib/private/skrongle/learn.db)" '[ -f /var/lib/private/skrongle/learn.db ] && [ -f /var/lib/private/skrongle/learn.db.key ]'
    check "/etc/skrongle is kept" '[ -d /etc/skrongle ]'
    check "the port no longer answers" '! curl -fsS http://127.0.0.1:7576/healthz'
    echo "==> install again over the kept data: the owner is still there"
    candidate
    gout 'mkdir -p /tmp/qa-dl && chmod 777 /tmp/qa-dl; rm -rf /tmp/qa-dl/*' >/dev/null
    limactl copy "$TAR" "$TAR.sha256" "$NAME:/tmp/qa-dl/" >/dev/null
    { echo "set -e; cd /tmp/qa-dl; V=$CAND"; install_block | grep -v 'curl '; } | gsh >/dev/null 2>&1
    gwait || true
    signs_in "reinstalled over the kept data"
    echo "==> uninstall.sh --purge"
    out=$(gout 'sh /opt/skrongle/uninstall.sh --purge; echo status=$?') || true
    printf '%s\n' "$out" | sed 's/^/    | /'
    case "$out" in *status=0*) ok "uninstall.sh --purge exits 0" ;; *) fail "uninstall.sh --purge failed" ;; esac
    check "/var/lib/skrongle is gone" '[ ! -e /var/lib/skrongle ] && [ ! -e /var/lib/private/skrongle ]'
    check "/etc/skrongle is gone" '[ ! -e /etc/skrongle ]'
    check "/opt/skrongle and the unit are gone" '[ ! -e /opt/skrongle ] && [ ! -e /etc/systemd/system/skrongle.service ]'
    check "nothing named skrongle is left in systemd" '! systemctl list-unit-files | grep -q "^skrongle"'
    echo "==> uninstall.sh once more is harmless: the script is gone with /opt/skrongle, so use the tarball's copy"
    gout 'mkdir -p /tmp/qa-un && tar -xzf /tmp/qa-dl/skrongle-*-'"$ARCH"'.tar.gz -C /tmp/qa-un ./uninstall.sh' >/dev/null
    out=$(gout 'sh /tmp/qa-un/uninstall.sh; echo status=$?') || true
    case "$out" in *"nothing to remove"*status=0*) ok "a second run says there is nothing to remove, exit 0" ;; *) fail "second run: $out" ;; esac
    finish
    ;;
  down)
    if vm_exists; then
      limactl stop --force "$NAME" >/dev/null 2>&1 || true
      limactl delete --force "$NAME"
      echo "deleted $NAME"
    else
      echo "no VM named $NAME"
    fi
    ;;
  shell)
    need_vm
    exec limactl shell "$NAME"
    ;;
  *) usage 2 ;;
esac
