#!/bin/sh
# The upgrade path (#43): start a candidate release on the database each
# older release really wrote (scripts/release-qa/fixtures/), and check what a
# self-hoster who updates would depend on:
#
#   - the server starts, runs exactly the migrations between the fixture's
#     schema and its own, and leaves PRAGMA user_version at its own schema;
#   - the owner and the learner sign in with the passwords in the fixture's .env;
#   - the learner's progress is the digest the fixture recorded when it was made;
#   - the course the owner shared is still there (the learner's list, or the
#     owner's when the release had no sharing yet);
#   - the invitation link nobody had used still registers an account;
#   - a clean stop, then a second start that migrates nothing and changes
#     nothing;
#   - the other direction: the older release's own binary, started on the
#     database the candidate migrated, refuses it with exit status 78 and a
#     message that names the newer schema, and leaves the file byte for byte
#     as it was (checksum).
#
#   scripts/release-qa/upgrade-check.sh dist/skrongle-0.1.2-aarch64.tar.gz
#   FIXTURES=/some/dir scripts/release-qa/upgrade-check.sh <tarball>
#   ONLY=0.1.1 scripts/release-qa/upgrade-check.sh <tarball>     # one fixture
#
# Older releases' binaries (for the refusal check) are taken from the
# directory the candidate is in, or downloaded from the GitHub release when
# missing (OLD_TARBALLS=<dir> to say where to look; SKRONGLE_REPO to change
# the repository). Run it on the Linux machine whose architecture the tarball
# is for; scripts/release-qa/matrix.sh does that in a container per
# architecture. POSIX sh, curl, tar, sha256sum, od. Prints one line per check
# and exits non-zero on any FAIL. The fixtures hold test data only
# (fixtures/README.md).
set -u

TARBALL="${1:?usage: upgrade-check.sh <tarball>}"
[ -f "$TARBALL" ] || { echo "no such file: $TARBALL"; exit 2; }
for tool in curl tar sha256sum od; do
  command -v "$tool" >/dev/null 2>&1 || { echo "needs $tool"; exit 2; }
done
HERE=$(cd "$(dirname "$0")" && pwd)
FIXTURES="${FIXTURES:-$HERE/fixtures}"
TARBALL_DIR=$(cd "$(dirname "$TARBALL")" && pwd)
OLD_TARBALLS="${OLD_TARBALLS:-$TARBALL_DIR}"
REPO="${SKRONGLE_REPO:-alexbeeken/skrongle-mini}"
ARCH=$(uname -m)

WORK=$(mktemp -d "${TMPDIR:-/tmp}/skrongle-upgrade.XXXXXX")
PID=""
FAILED=0
cleanup() {
  if [ -n "$PID" ]; then kill "$PID" 2>/dev/null; wait "$PID" 2>/dev/null; fi
  rm -rf "$WORK"
}
trap cleanup EXIT INT TERM

ok()   { echo "  ok   $1"; }
fail() { echo "  FAIL $1"; FAILED=1; }
note() { echo "  note $1"; }

# The schema level (PRAGMA user_version) lives in the SQLite header, bytes 60-63.
user_version() {
  set -- $(od -An -tu1 -j60 -N4 "$1")
  echo $(( $1 * 16777216 + $2 * 65536 + $3 * 256 + $4 ))
}
# older <a> <b>: a is a lower version than b (dotted numbers).
older() { [ "$1" != "$2" ] && [ "$(printf '%s\n%s\n' "$1" "$2" | sort -t. -k1,1n -k2,2n -k3,3n | head -1)" = "$1" ]; }
env_of() { sed -n "s/^$2=//p" "$1" | head -1; }

unpack() { # <tarball> <dir>
  mkdir -p "$2" && tar -xzf "$1" -C "$2"
}

echo "==> candidate $TARBALL"
unpack "$TARBALL" "$WORK/cand" || { echo "cannot unpack $TARBALL"; exit 2; }
CAND="$WORK/cand"
CAND_VERSION=$(head -1 "$CAND/VERSION")
CAND_SCHEMA=$("$CAND/skrongle" --schema-version 2>/dev/null || true)
case "$CAND_SCHEMA" in [0-9]*) ok "skrongle $CAND_VERSION migrates to schema v$CAND_SCHEMA" ;; *) echo "  FAIL --schema-version printed '$CAND_SCHEMA'"; exit 1 ;; esac

PORT="${PORT:-$(shuf -i 20000-40000 -n 1 2>/dev/null || echo 18090)}"
URL="http://127.0.0.1:$PORT"
HEALTH_WAIT="${HEALTH_WAIT:-120}"

# start <opt dir> <database> <log>: the server in the background, PID set; 0 once /healthz answers.
start() {
  ( cd "$1" && SKRONGLE_BIND="127.0.0.1:$PORT" SKRONGLE_DB="$2" SKRONGLE_DATA_DIR="$1/data" \
      SKRONGLE_SIGNUP=invite SKRONGLE_LOG=info SKRONGLE_PUBLIC_URL="$URL" exec ./skrongle ) >> "$3" 2>&1 &
  PID=$!
  i=0
  while [ $i -lt $((HEALTH_WAIT * 5)) ]; do
    curl -fsS "$URL/healthz" >/dev/null 2>&1 && return 0
    kill -0 "$PID" 2>/dev/null || break
    sleep 0.2; i=$((i + 1))
  done
  return 1
}
stop() { kill -TERM "$PID" 2>/dev/null; wait "$PID"; RC=$?; PID=""; return $RC; }
http() { # <jar> <method> <path> [curl args...]: CODE and $WORK/body
  jar="$1"; method="$2"; path="$3"; shift 3
  CODE=$(curl -s -o "$WORK/body" -w '%{http_code}' -b "$WORK/$jar" -c "$WORK/$jar" -X "$method" "$@" "$URL$path")
}
digest() {
  http "$1" GET /api/progress
  sed 's/.*"lessons":\[//; s/},{"id"/}\n{"id"/g' "$WORK/body" |
    sed -n 's/^[{]*"id":"\([^"]*\)".*"seen":\([a-z]*\),"passed":\([a-z]*\),.*"best_pct":\([a-z0-9]*\),.*/\1 \2 \3 \4/p'
}
# old_binary <version>: unpack that release's tarball (found beside the candidate, or downloaded) into $WORK/old-<version>.
old_binary() {
  v="$1"; name="skrongle-$v-$ARCH.tar.gz"
  if [ ! -f "$OLD_TARBALLS/$name" ]; then
    mkdir -p "$WORK/dl"
    code=$(curl -sSL -o "$WORK/dl/$name" -w '%{http_code}' "https://github.com/$REPO/releases/download/v$v/$name") || return 1
    if [ "$code" != 200 ]; then
      rm -f "$WORK/dl/$name"
      # 2: the release is there but has no tarball for this architecture
      # (the 32-bit builds started with 0.2.0). Anything else is a failure.
      [ "$code" = 404 ] && curl -fsSL -o /dev/null "https://github.com/$REPO/releases/tag/v$v" && return 2
      return 1
    fi
    unpack "$WORK/dl/$name" "$WORK/old-$v"
  else
    unpack "$OLD_TARBALLS/$name" "$WORK/old-$v"
  fi
}

COUNT=0
for DB in "$FIXTURES"/learn-*.db; do
  [ -f "$DB" ] || continue
  V=$(basename "$DB" .db); V=${V#learn-}
  [ -z "${ONLY:-}" ] || [ "$ONLY" = "$V" ] || continue
  older "$V" "$CAND_VERSION" || { note "fixture $V is not older than $CAND_VERSION; skipped"; continue; }
  COUNT=$((COUNT + 1))
  ENVF="${DB%.db}.env"; DIGEST="${DB%.db}.progress.txt"
  echo "==> $V -> $CAND_VERSION"
  for f in "$DB.key" "$ENVF" "$DIGEST"; do [ -f "$f" ] || { fail "fixture file missing: $f"; continue 2; }; done
  OWNER=$(env_of "$ENVF" OWNER); LEARNER=$(env_of "$ENVF" LEARNER); PW=$(env_of "$ENVF" PASSWORD)
  COURSE=$(env_of "$ENVF" COURSE); SHARE=$(env_of "$ENVF" SHARE); SPARE=$(env_of "$ENVF" SPARE_INVITE)

  STATE="$WORK/state-$V"; mkdir -p "$STATE"
  cp "$DB" "$STATE/learn.db"; cp "$DB.key" "$STATE/learn.db.key"
  LOG="$WORK/server-$V.log"; : > "$LOG"
  FROM=$(user_version "$STATE/learn.db")
  if [ "$FROM" = "$(env_of "$ENVF" SCHEMA)" ]; then ok "the fixture is at schema v$FROM, as its .env says"; else fail "the fixture is at v$FROM, its .env says v$(env_of "$ENVF" SCHEMA)"; fi

  if start "$CAND" "$STATE/learn.db" "$LOG"; then ok "starts on the $V database"; else fail "did not start on the $V database; the log:"; sed 's/^/       /' "$LOG"; continue; fi
  WANT=$((CAND_SCHEMA - FROM))
  GOT=$(grep -c 'applying schema migration' "$LOG")
  if [ "$GOT" = "$WANT" ]; then ok "ran $GOT migration(s), v$FROM to v$CAND_SCHEMA"; else fail "ran $GOT migration(s), expected $WANT (v$FROM to v$CAND_SCHEMA)"; fi
  if grep -q 'Owner link' "$LOG"; then fail "printed an owner link although the database has an owner"; else ok "no owner link: the owner is still there"; fi

  rm -f "$WORK/owner" "$WORK/learner"
  http owner POST /login --data-urlencode "email=$OWNER" --data-urlencode "password=$PW"
  if [ "$CODE" = 303 ]; then ok "the owner signs in"; else fail "owner sign-in: $CODE"; fi
  http learner POST /login --data-urlencode "email=$LEARNER" --data-urlencode "password=$PW"
  if [ "$CODE" = 303 ]; then ok "the learner signs in"; else fail "learner sign-in: $CODE"; fi

  digest learner > "$WORK/digest-$V"
  if [ -s "$WORK/digest-$V" ] && cmp -s "$DIGEST" "$WORK/digest-$V"; then ok "the learner's progress is what the fixture recorded ($(grep -c ' true true ' "$DIGEST") passed, $(grep -c ' true false ' "$DIGEST") seen)"; else fail "the learner's progress differs from $DIGEST:"; diff "$DIGEST" "$WORK/digest-$V" | head -6 | sed 's/^/       /'; fi

  if [ -n "$SHARE" ]; then
    http learner GET /courses
    if grep -q "data-course-id=\"$COURSE\"" "$WORK/body"; then ok "the shared course $COURSE is on the learner's list"; else fail "the shared course $COURSE is not on the learner's list"; fi
    http owner GET "/courses/$COURSE?share=1"
    if grep -q 'data-opened="true"' "$WORK/body"; then ok "the share link still shows as opened"; else fail "the share link no longer shows as opened"; fi
  else
    note "release $V had no course sharing; checking the owner's copy instead"
    http owner GET /courses
    if grep -q "data-course-id=\"$COURSE\"" "$WORK/body"; then ok "the owner's course $COURSE is on the list"; else fail "the owner's course $COURSE is not on the list"; fi
  fi

  http none POST /register --data-urlencode "email=upgrade-$V@example.org" --data-urlencode "password=$PW" --data "age=on" --data "invite=$SPARE"
  if [ "$CODE" = 303 ]; then ok "the invitation link nobody used still registers an account"; else fail "the unused invitation answered $CODE"; fi
  http none POST /register --data-urlencode "email=twice-$V@example.org" --data-urlencode "password=$PW" --data "age=on" --data "invite=$SPARE"
  if [ "$CODE" = 403 ]; then ok "and then it is used up ($CODE)"; else fail "the used invitation answered $CODE, expected 403"; fi

  stop; RC=$?
  if [ "$RC" = 0 ]; then ok "SIGTERM: a clean exit"; else fail "exit code $RC on SIGTERM"; fi
  if grep -q panicked "$LOG"; then fail "a panic in the log"; else ok "no panic in the log"; fi
  AFTER=$(user_version "$STATE/learn.db")
  if [ "$AFTER" = "$CAND_SCHEMA" ]; then ok "PRAGMA user_version went from $FROM to $AFTER"; else fail "PRAGMA user_version is $AFTER, expected $CAND_SCHEMA (was $FROM)"; fi

  # The second start: nothing to migrate, nothing to change.
  SUM1=$(sha256sum < "$STATE/learn.db")
  LOG2="$WORK/server2-$V.log"; : > "$LOG2"
  if start "$CAND" "$STATE/learn.db" "$LOG2"; then ok "starts again on the migrated database"; else fail "second start failed; the log:"; sed 's/^/       /' "$LOG2"; continue; fi
  if grep -q 'applying schema migration' "$LOG2"; then fail "the second start migrated again"; else ok "the second start migrates nothing"; fi
  rm -f "$WORK/learner"
  http learner POST /login --data-urlencode "email=$LEARNER" --data-urlencode "password=$PW"
  if [ "$CODE" = 303 ]; then ok "the learner signs in again"; else fail "learner sign-in after the second start: $CODE"; fi
  digest learner > "$WORK/digest2-$V"
  if cmp -s "$WORK/digest-$V" "$WORK/digest2-$V"; then ok "the progress is the same after the second start"; else fail "the progress changed on the second start"; fi
  stop || fail "second stop was not clean"
  if [ "$(user_version "$STATE/learn.db")" = "$CAND_SCHEMA" ]; then ok "the schema is still v$CAND_SCHEMA"; else fail "the schema changed on the second start"; fi

  # The other way: the older release on the database the candidate migrated.
  if [ "$FROM" -ge "$CAND_SCHEMA" ]; then
    note "$V has the same schema (v$FROM) as $CAND_VERSION: no newer database for it to refuse"
    continue
  fi
  old_binary "$V"; RC=$?
  if [ "$RC" = 2 ]; then note "$V was never built for $ARCH: there is no older binary to refuse the database"; continue; fi
  if [ "$RC" != 0 ]; then fail "no skrongle-$V-$ARCH.tar.gz in $OLD_TARBALLS and it cannot be downloaded"; continue; fi
  NEWER="$WORK/newer-$V"; mkdir -p "$NEWER"
  cp "$STATE/learn.db" "$NEWER/learn.db"; cp "$STATE/learn.db.key" "$NEWER/learn.db.key"
  SUM_BEFORE=$(sha256sum < "$NEWER/learn.db")
  OLDLOG="$WORK/old-$V.log"; : > "$OLDLOG"
  ( cd "$WORK/old-$V" && SKRONGLE_BIND="127.0.0.1:$PORT" SKRONGLE_DB="$NEWER/learn.db" SKRONGLE_DATA_DIR="$WORK/old-$V/data" \
      SKRONGLE_SIGNUP=invite SKRONGLE_LOG=info SKRONGLE_PUBLIC_URL="$URL" exec ./skrongle ) > "$OLDLOG" 2>&1 &
  PID=$!
  i=0; while [ $i -lt $((HEALTH_WAIT * 5)) ] && kill -0 "$PID" 2>/dev/null; do sleep 0.2; i=$((i + 1)); done
  if kill -0 "$PID" 2>/dev/null; then
    fail "skrongle $V is still running on the database $CAND_VERSION migrated (it should have refused it)"
    stop
  else
    wait "$PID"; RC=$?; PID=""
    if [ "$RC" = 78 ]; then ok "skrongle $V refuses the migrated database with exit 78"; else fail "skrongle $V exited $RC on the migrated database, expected 78"; fi
    if grep -qi "newer" "$OLDLOG" && grep -q "schema v$CAND_SCHEMA" "$OLDLOG"; then ok "and says why: $(grep -i newer "$OLDLOG" | head -1 | sed 's/^.*ERROR *//' | cut -c1-110)"; else fail "its message does not name the newer schema v$CAND_SCHEMA:"; sed 's/^/       /' "$OLDLOG" | tail -5; fi
  fi
  SUM_AFTER=$(sha256sum < "$NEWER/learn.db")
  if [ "$SUM_BEFORE" = "$SUM_AFTER" ]; then ok "learn.db is byte for byte what it was (checksum unchanged)"; else fail "skrongle $V changed learn.db"; fi
  if [ "$(user_version "$NEWER/learn.db")" = "$CAND_SCHEMA" ]; then ok "the schema is still v$CAND_SCHEMA"; else fail "the schema is no longer v$CAND_SCHEMA"; fi
done

if [ "$COUNT" = 0 ]; then echo "no fixture older than $CAND_VERSION in $FIXTURES"; fi
echo
if [ "$FAILED" = 0 ]; then echo "PASS upgrade to $CAND_VERSION ($COUNT fixture(s))"; else echo "FAIL upgrade to $CAND_VERSION (see the FAIL lines above)"; fi
exit "$FAILED"
