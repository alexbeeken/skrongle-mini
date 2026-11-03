#!/usr/bin/env bash
# Checks that `.dev-start --background` leaves no server behind when the
# server never answers /healthz (#679). Not in CI; run it after changing
# .dev-start:
#
#   scripts/dev-start-orphan-check.sh
#
# It copies .dev-start into a temporary folder with a stand-in `cargo` (does
# nothing) and a stand-in server binary that never listens, so no real build
# or server is involved. Four cases, about two and a half minutes in all:
#   1. a server that never answers and ignores SIGTERM: .dev-start gives up
#      after 20 s and kills it;
#   2. a server that logs crowd-seed progress for 25 s, then goes quiet:
#      .dev-start keeps waiting past 20 s while the crowd grows, then kills it;
#   3. a server that binds its port and never answers, as the real one does
#      while it migrates and seeds: .dev-start gives up after 20 s and kills
#      it (#869).
#   4. a server that binds, logs crowd-seed progress every 5 s for 45 s (as
#      the real one does since #936), then answers: .dev-start waits it out
#      and exits 0.
# A case still running after 90 s has hung, and fails.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TMP=$(mktemp -d "${TMPDIR:-/tmp}/dev-start-check.XXXXXX")
trap 'rm -rf "$TMP"' EXIT
FAIL=0

setup() {
  rm -rf "$TMP/app" "$TMP/bin" "$TMP/server.pid"
  mkdir -p "$TMP/app/target/debug" "$TMP/bin"
  cp "$ROOT/.dev-start" "$TMP/app/.dev-start"
  printf '#!/usr/bin/env bash\nexit 0\n' >"$TMP/bin/cargo"
  chmod +x "$TMP/bin/cargo"
}

# $1: the stand-in server's body.
stand_in() {
  printf '#!/usr/bin/env bash\necho $$ >"%s"\n%s\n' "$TMP/server.pid" "$1" >"$TMP/app/target/debug/spanish-learning"
  chmod +x "$TMP/app/target/debug/spanish-learning"
}

# Runs `.dev-start --background` into $TMP/out. Sets rc to its exit code, and
# hung=1 if it was still waiting after 90 s (then it is killed).
start_dev() {
  rc=0 hung=""
  ( cd "$TMP/app" && PATH="$TMP/bin:$PATH" ./.dev-start --background >"$TMP/out" 2>&1 ) &
  local starter=$! t=$SECONDS
  while kill -0 "$starter" 2>/dev/null; do
    if [ $((SECONDS - t)) -ge 90 ]; then hung=1; pkill -P "$starter" 2>/dev/null || true; kill "$starter" 2>/dev/null || true; break; fi
    sleep 1
  done
  wait "$starter" || rc=$?
}

# $1: name, $2: the stand-in server's body, $3: minimum seconds .dev-start should wait.
run_case() {
  local name=$1 body=$2 min=$3 t0 pid
  setup
  stand_in "$body"
  t0=$SECONDS
  start_dev
  local took=$((SECONDS - t0))
  # The stand-in writes its own pid, since .dev-start's pid file should be gone.
  pid=$(cat "$TMP/server.pid" 2>/dev/null || true)
  kill -0 "$pid" 2>/dev/null || pid=""
  if [ -n "$hung" ]; then echo "FAIL $name: .dev-start still waiting after 90 s"; [ -n "$pid" ] && kill -9 $pid; FAIL=1
  elif [ "$rc" = 0 ]; then echo "FAIL $name: .dev-start exited 0"; FAIL=1
  elif [ -n "$pid" ]; then echo "FAIL $name: server left running (pid $pid)"; kill -9 $pid; FAIL=1
  elif [ -e "$TMP/app/.dev-server.pid" ]; then echo "FAIL $name: pid file left behind"; FAIL=1
  elif [ "$took" -lt "$min" ]; then echo "FAIL $name: gave up after $took s, expected at least $min s"; FAIL=1
  else echo "ok   $name ($took s)"; fi
}

# exec keeps the pid .dev-start recorded; the ignored TERM survives the exec.
run_case "silent server, ignores TERM" "trap '' TERM; exec sleep 600" 20
run_case "crowd seed, then silence" \
  'for i in 1 2 3 4 5; do echo "INFO dev crowd: $((i*50))/1000 learners"; sleep 5; done; exec sleep 600' 25
# Binds $SKRONGLE_BIND (set by .dev-start), accepts nothing, answers nothing.
run_case "binds, never answers" \
  "exec python3 -c 'import os, socket, time; h, p = os.environ[\"SKRONGLE_BIND\"].rsplit(\":\", 1); s = socket.socket(); s.bind((h, int(p))); s.listen(5); time.sleep(600)'" 20

# Case 4: binds $SKRONGLE_BIND, logs "dev crowd: n/1000 learners" at once and
# every 5 s, and answers every request with "ok" after 45 s.
setup
stand_in "exec python3 -u -c '
import os, socket, time
h, p = os.environ[\"SKRONGLE_BIND\"].rsplit(\":\", 1)
s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1); s.bind((h, int(p))); s.listen(5)
for i in range(10):
    print(\"INFO dev crowd: %d/1000 learners\" % (i * 100)); time.sleep(5)
while True:
    c, _ = s.accept(); c.recv(4096); c.sendall(b\"HTTP/1.1 200 OK\\r\\nContent-Length: 2\\r\\nConnection: close\\r\\n\\r\\nok\"); c.close()
'"
name="slow crowd seed, then answers"
t0=$SECONDS
start_dev
took=$((SECONDS - t0))
pid=$(cat "$TMP/server.pid" 2>/dev/null || true)
if [ -n "$hung" ]; then echo "FAIL $name: .dev-start still waiting after 90 s"; FAIL=1
elif [ "$rc" != 0 ]; then echo "FAIL $name: .dev-start exited $rc after $took s:"; sed 's/^/     /' "$TMP/out"; FAIL=1
elif [ "$took" -lt 45 ]; then echo "FAIL $name: exited 0 after $took s, before the server could answer"; FAIL=1
elif ! grep -q "^site:" "$TMP/out"; then echo "FAIL $name: no site URL printed"; FAIL=1
else echo "ok   $name ($took s)"
fi
[ -n "$pid" ] && kill -9 "$pid" 2>/dev/null || true

exit $FAIL
