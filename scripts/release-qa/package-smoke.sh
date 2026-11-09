#!/bin/sh
# Check one release tarball on the machine this runs on (#39): the package's
# layout and checksum, the binary (version, doctor, dynamic linking), then a
# real first day over HTTP with curl: the owner link from the log, the
# owner's account, an invitation, a learner, the built-in course and its
# pages, a copied course shared by link, a clean stop and a second start on
# the same database. No dev routes: this is the binary self-hosters run.
#
#   scripts/release-qa/package-smoke.sh dist/skrongle-0.1.2-aarch64.tar.gz
#   KEEP=1 scripts/release-qa/package-smoke.sh <tarball>   # leave the server running (prints its URL)
#   PORT=18080 scripts/release-qa/package-smoke.sh <tarball>
#
# POSIX sh, curl, tar and sha256sum only, so it runs unchanged in a minimal
# Debian container or VM of any architecture (scripts/release-qa/compose.yaml
# runs it per architecture under QEMU). Linux only: the tarball holds a Linux
# binary. Prints one line per check and exits non-zero on any FAIL.
set -u

TARBALL="${1:?usage: package-smoke.sh <tarball>}"
[ -f "$TARBALL" ] || { echo "no such file: $TARBALL"; exit 2; }
for tool in curl tar sha256sum; do
  command -v "$tool" >/dev/null 2>&1 || { echo "needs $tool"; exit 2; }
done

WORK=$(mktemp -d "${TMPDIR:-/tmp}/skrongle-smoke.XXXXXX")
PID=""
FAILED=0
cleanup() {
  if [ -n "$PID" ] && [ "${KEEP:-0}" != 1 ]; then kill "$PID" 2>/dev/null; wait "$PID" 2>/dev/null; fi
  if [ "${KEEP:-0}" = 1 ] && [ -n "$PID" ]; then
    echo "==> server still running: $URL (pid $PID, log $WORK/server.log, database $WORK/state/learn.db)"
  else
    rm -rf "$WORK"
  fi
}
trap cleanup EXIT INT TERM

ok()   { echo "  ok   $1"; }
fail() { echo "  FAIL $1"; FAILED=1; }
# check <description> <command...>: ok when the command succeeds.
check() { d="$1"; shift; if "$@" >/dev/null 2>&1; then ok "$d"; else fail "$d"; fi; }

# ---------- the package ----------
echo "==> package $TARBALL"
DIR=$(dirname "$TARBALL"); NAME=$(basename "$TARBALL")
if [ -f "$TARBALL.sha256" ]; then
  check "checksum matches $NAME.sha256" sh -c "cd '$DIR' && sha256sum -c '$NAME.sha256'"
else
  echo "  note no $NAME.sha256 beside it; checksum not checked"
fi
tar -tzf "$TARBALL" > "$WORK/list" 2>/dev/null || { fail "tarball unpacks (tar -tzf)"; echo "cannot read $TARBALL"; exit 1; }
sed 's|^\./||' "$WORK/list" | grep -v '^$' > "$WORK/files"
for f in skrongle VERSION update.sh skrongle.service; do
  check "has $f" grep -qx "$f" "$WORK/files"
done
check "has data/courses/" grep -q '^data/courses/.' "$WORK/files"
EXTRA=$(sed 's|/.*||' "$WORK/files" | sort -u | grep -vxE 'skrongle|VERSION|update\.sh|skrongle\.service|data' || true)
if [ -z "$EXTRA" ]; then ok "nothing but skrongle, VERSION, update.sh, skrongle.service and data/ at the top"; else fail "unexpected at the top: $(echo "$EXTRA" | tr '\n' ' ')"; fi
check "no test fixtures, test courses or authoring briefs in data/" sh -c "! grep -qE '^data/(test-|.*/AUTHORING\.md|.*/fact-check-ok\.txt)' '$WORK/files'"
check "no .DS_Store or other Mac droppings" sh -c "! grep -qE '(^|/)(\.DS_Store|\._)' '$WORK/files'"

mkdir -p "$WORK/opt" "$WORK/state"
tar -xzf "$TARBALL" -C "$WORK/opt" || { fail "unpacks"; exit 1; }
OPT="$WORK/opt"
VERSION=$(cat "$OPT/VERSION" 2>/dev/null | head -1)
check "VERSION is one line like 1.2.3 ($VERSION)" sh -c "[ \$(wc -l < '$OPT/VERSION') -le 1 ] && echo '$VERSION' | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+'"
case "$NAME" in *"skrongle-$VERSION-"*) ok "the file is named for $VERSION" ;; *) fail "the file name does not carry VERSION $VERSION" ;; esac
check "skrongle is executable" test -x "$OPT/skrongle"
check "update.sh is executable" test -x "$OPT/update.sh"
check "skrongle.service starts /opt/skrongle/skrongle" grep -q '^ExecStart=/opt/skrongle/skrongle$' "$OPT/skrongle.service"
check "update.sh is a /bin/sh script" sh -n "$OPT/update.sh"

# ---------- the binary ----------
echo "==> binary"
if command -v ldd >/dev/null 2>&1; then
  ldd "$OPT/skrongle" > "$WORK/ldd" 2>&1 || true
  if grep -q 'not found' "$WORK/ldd"; then fail "every shared library and glibc symbol version is on this system: $(grep 'not found' "$WORK/ldd" | tr '\n' ' ')"; else ok "links only what this system has ($(grep -c '=>' "$WORK/ldd") libraries)"; fi
  check "nothing beyond glibc is linked dynamically" sh -c "! grep -E '=>' '$WORK/ldd' | grep -vqE 'lib(c|m|pthread|dl|rt|gcc_s)\.so|ld-linux'"
fi
GOT=$("$OPT/skrongle" --version 2>&1 || true)
if [ "$GOT" = "skrongle $VERSION" ]; then ok "--version says skrongle $VERSION"; else fail "--version says '$GOT', VERSION says $VERSION"; fi
SCHEMA=$("$OPT/skrongle" --schema-version 2>/dev/null || true)
case "$SCHEMA" in [0-9]*) ok "--schema-version is $SCHEMA" ;; *) fail "--schema-version printed '$SCHEMA'" ;; esac
if SKRONGLE_DB="$WORK/doctor.db" "$OPT/skrongle" doctor > "$WORK/doctor" 2>&1; then ok "doctor exits 0 on a fresh database"; else fail "doctor failed:"; sed 's/^/       /' "$WORK/doctor"; fi
check "doctor names the schema it migrates to (v$SCHEMA)" grep -q "v$SCHEMA" "$WORK/doctor"

# ---------- a first day ----------
PORT="${PORT:-$(shuf -i 20000-40000 -n 1 2>/dev/null || echo 18080)}"
URL="http://127.0.0.1:$PORT"
HEALTH_WAIT="${HEALTH_WAIT:-120}" # seconds; a QEMU-emulated start is slow
start_server() {
  ( cd "$OPT" && SKRONGLE_BIND="127.0.0.1:$PORT" SKRONGLE_DB="$WORK/state/learn.db" SKRONGLE_DATA_DIR="$OPT/data" \
      SKRONGLE_SIGNUP=invite SKRONGLE_LOG=info SKRONGLE_PUBLIC_URL="$URL" exec ./skrongle ) >> "$WORK/server.log" 2>&1 &
  PID=$!
  i=0
  while [ $i -lt $((HEALTH_WAIT * 5)) ]; do
    curl -fsS "$URL/healthz" >/dev/null 2>&1 && return 0
    kill -0 "$PID" 2>/dev/null || break
    sleep 0.2; i=$((i + 1))
  done
  return 1
}
# http <jar> <method> <path> [curl args...]: status code in $CODE, body in $WORK/body, Location in $LOC.
http() {
  jar="$1"; method="$2"; path="$3"; shift 3
  CODE=$(curl -s -o "$WORK/body" -D "$WORK/headers" -w '%{http_code}' -b "$WORK/$jar" -c "$WORK/$jar" -X "$method" "$@" "$URL$path")
  LOC=$(sed -n 's/^[Ll]ocation: *//p' "$WORK/headers" | tr -d '\r')
}
# page <jar> <path> [marker]: a 200 (redirects followed) that contains the marker.
page() {
  jar="$1"; path="$2"; marker="${3:-}"
  CODE=$(curl -s -L -o "$WORK/body" -w '%{http_code}' -b "$WORK/$jar" -c "$WORK/$jar" "$URL$path")
  if [ "$CODE" = 200 ] && { [ -z "$marker" ] || grep -q -- "$marker" "$WORK/body"; }; then ok "$CODE $path${marker:+ ($marker)}"; else fail "$CODE $path${marker:+ (expected $marker)}"; fi
}

echo "==> start on $URL"
if start_server; then ok "answers /healthz within ${HEALTH_WAIT}s"; else fail "did not start; the log:"; sed 's/^/       /' "$WORK/server.log"; exit 1; fi
OWNER_LINK=$(sed -n 's/.*Owner link.*\(http:\/\/[^ ]*invite=[A-Za-z0-9_-]*\).*/\1/p' "$WORK/server.log" | head -1)
OWNER_TOKEN="${OWNER_LINK##*invite=}"
if [ -n "$OWNER_TOKEN" ] && [ "$OWNER_TOKEN" != "$OWNER_LINK" ]; then ok "the log has the owner link"; else fail "no owner link in the log:"; sed 's/^/       /' "$WORK/server.log"; exit 1; fi
case "$OWNER_LINK" in "$URL"/*) ok "the owner link points at SKRONGLE_PUBLIC_URL" ;; *) fail "the owner link is $OWNER_LINK, not under $URL" ;; esac

echo "==> the owner"
OWNER=owner@example.org; LEARNER=learner@example.org; PW=a-long-enough-password
http owner POST /register --data-urlencode "email=$OWNER" --data-urlencode "password=$PW" --data "age=on" --data "invite=$OWNER_TOKEN"
if [ "$CODE" = 303 ] && grep -qi '^set-cookie:' "$WORK/headers"; then ok "the owner link registers the owner ($CODE, a session cookie)"; else fail "registering the owner: $CODE $(head -c 200 "$WORK/body")"; fi
http none POST /register --data-urlencode "email=nobody@example.org" --data-urlencode "password=$PW" --data "age=on"
if [ "$CODE" = 403 ]; then ok "without an invitation, /register is closed ($CODE)"; else fail "/register without an invitation answered $CODE, expected 403"; fi
page owner /courses 'data-testid="courses-card"'
http owner POST /courses/open/spanish
if [ "$CODE" = 303 ]; then ok "opens the built-in Spanish course"; else fail "open spanish: $CODE"; fi
page owner / 'nav-account-toggle'
page owner /lessons 'href="/lessons/'
LESSON=$(grep -o 'href="/lessons/[a-z0-9-]*"' "$WORK/body" | head -1 | sed 's/href="//; s/"$//')
[ -n "$LESSON" ] && page owner "$LESSON"
for p in /practice /sprint /map /profile /manage /docs /docs/self-hosting /docs/getting-started; do page owner "$p"; done
page owner /healthz

echo "==> an invitation and a learner"
http owner POST /manage/invites
INVITE=$(grep -o '/register?invite=[A-Za-z0-9_-]*' "$WORK/body" | head -1 | sed 's/.*invite=//')
if [ "$CODE" = 200 ] && [ -n "$INVITE" ]; then ok "Manage Users makes an invitation link"; else fail "making an invitation: $CODE"; fi
http learner POST /register --data-urlencode "email=$LEARNER" --data-urlencode "password=$PW" --data "age=on" --data "invite=$INVITE"
if [ "$CODE" = 303 ]; then ok "the learner registers through it"; else fail "the learner's registration: $CODE $(head -c 200 "$WORK/body")"; fi
http none POST /register --data-urlencode "email=again@example.org" --data-urlencode "password=$PW" --data "age=on" --data "invite=$INVITE"
if [ "$CODE" = 403 ]; then ok "a used invitation is refused ($CODE)"; else fail "a used invitation answered $CODE"; fi
http learner POST /courses/open/spanish
page learner /lessons 'href="/lessons/'

echo "==> a course of the owner's, shared by link"
http owner POST /courses/spanish/copy
COURSE="${LOC##*/courses/}"; COURSE="${COURSE%%\?*}"
if [ "$CODE" = 303 ] && [ -n "$COURSE" ] && [ "$COURSE" != "$LOC" ]; then ok "copies the Spanish course ($COURSE)"; else fail "copying: $CODE -> $LOC"; fi
page owner "/courses/$COURSE" 'data-testid="course-edit"'
http owner POST "/courses/$COURSE/share" --data-urlencode "label=The learner"
SHARE=$(grep -o '/courses/shared/[A-Za-z0-9_-]*' "$WORK/body" | head -1 | sed 's|.*/||')
if [ "$CODE" = 200 ] && [ -n "$SHARE" ]; then ok "makes an invitation link for it"; else fail "sharing: $CODE"; fi
http learner GET "/courses/shared/$SHARE"
if [ "$CODE" = 303 ]; then ok "the learner opens the link ($CODE -> $LOC)"; else fail "opening the share link: $CODE"; fi
page learner /courses "data-course-id=\"$COURSE\""
page owner "/courses/$COURSE?share=1" 'data-opened="true"'
http learner POST "/courses/open/$COURSE"
page learner /lessons 'href="/lessons/'

echo "==> stop and start again"
kill -TERM "$PID"; wait "$PID"; RC=$?; PID=""
if [ "$RC" = 0 ]; then ok "SIGTERM: a clean exit"; else fail "exit code $RC on SIGTERM"; fi
check "learn.db.key was made beside the database" test -s "$WORK/state/learn.db.key"
if [ ! -s "$WORK/state/learn.db-wal" ]; then ok "the write-ahead log folded into learn.db"; else fail "learn.db-wal still holds $(wc -c < "$WORK/state/learn.db-wal") bytes"; fi
SKRONGLE_DB="$WORK/state/learn.db" "$OPT/skrongle" doctor > "$WORK/doctor2" 2>&1 || true
check "doctor: the database is at v$SCHEMA (up to date)" grep -q "v$SCHEMA (up to date)" "$WORK/doctor2"
check "no panic in the log" sh -c "! grep -q panicked '$WORK/server.log'"
: > "$WORK/server.log"
if start_server; then ok "starts again on the same database"; else fail "second start failed; the log:"; sed 's/^/       /' "$WORK/server.log"; exit 1; fi
check "no owner link on a server that has an owner" sh -c "! grep -q 'Owner link' '$WORK/server.log'"
rm -f "$WORK/owner" "$WORK/learner"
http owner POST /login --data-urlencode "email=$OWNER" --data-urlencode "password=$PW"
if [ "$CODE" = 303 ]; then ok "the owner signs in"; else fail "owner sign-in: $CODE"; fi
http learner POST /login --data-urlencode "email=$LEARNER" --data-urlencode "password=$PW"
if [ "$CODE" = 303 ]; then ok "the learner signs in"; else fail "learner sign-in: $CODE"; fi
page learner /courses "data-course-id=\"$COURSE\""
page owner /manage 'data-testid="manage-page"'
check "no panic in the log" sh -c "! grep -q panicked '$WORK/server.log'"

echo
if [ "$FAILED" = 0 ]; then echo "PASS $NAME"; else echo "FAIL $NAME (see the FAIL lines above)"; fi
exit "$FAILED"
