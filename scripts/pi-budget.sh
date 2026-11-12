#!/usr/bin/env bash
# Pi budget (#638). The target is a Pi Zero 2 W (deploy/skrongle.service:
# MemoryHigh=48M, MemoryMax=64M, TasksMax=32), so a new dependency or a new
# background task should not quietly grow the binary or the idle server.
# This measures three numbers and checks them against scripts/pi-budget.txt:
#
#   binary   the stripped release binary, in bytes: at most the baseline for
#            this platform plus the allowed percentage
#   rss      resident memory of an idle server, in KiB (VmRSS)
#   threads  its thread count (Threads)
#
# The server runs with no optional settings (no AI keys, no SKRONGLE_DEV), only what deploy/skrongle.service sets, on a temp
# database and a free loopback port. It answers /, /login and /healthz
# twenty times each; then the numbers are read. Then connected apps (#191):
# the owner's account is made from the owner link in the log, Let apps
# connect turned on and two tokens made, as on the real pages; RSS is read
# before and after 300 MCP `list_courses` calls, again with users,
# oauth_clients, api_grants and api_tokens filled to their caps
# (src/caps.rs), and each growth is checked against a margin; the thread
# count after both must be the idle one. Then the server is stopped.
# On Linux they come from /proc/<pid>/status; on macOS from ps, so the script
# runs locally too. A Mac's numbers differ from Linux's: its binary is
# checked against the baseline of its own platform, and its RSS (ps counts
# shared system libraries) is shown but not checked.
#
#   scripts/pi-budget.sh                 build --release, measure, check (CI runs this)
#   scripts/pi-budget.sh --bin <path>    measure an already-built binary instead
#   scripts/pi-budget.sh --update        also write this platform's binary size as the baseline
set -euo pipefail
cd "$(dirname "$0")/.."
budget=scripts/pi-budget.txt
command -v cargo >/dev/null 2>&1 || { [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"; }

BIN=""; UPDATE=0
while [ $# -gt 0 ]; do
  case "$1" in
    --bin) BIN="$2"; shift 2 ;;
    --update) UPDATE=1; shift ;;
    *) sed -n '2,28p' "$0"; exit 1 ;;
  esac
done

platform="$(uname -s | tr '[:upper:]' '[:lower:]')-$(uname -m)"
platform=${platform/arm64/aarch64}
# setting <key> [platform]: the value from the budget file.
setting() { awk -v k="$1" -v p="${2:-}" '$1 == k && (p == "" || $2 == p) { print (p == "" ? $2 : $3) }' "$budget"; }

if [ -z "$BIN" ]; then
  echo "==> cargo build --release" >&2
  start=$(date +%s)
  cargo build --release -q
  echo "    built in $(( $(date +%s) - start ))s" >&2
  BIN="${CARGO_TARGET_DIR:-target}/release/spanish-learning"
fi
[ -x "$BIN" ] || { echo "no binary at $BIN" >&2; exit 1; }

WORK=$(mktemp -d "${TMPDIR:-/tmp}/pi-budget.XXXXXX")
PID=""
cleanup() { [ -z "$PID" ] || kill "$PID" 2>/dev/null || true; rm -rf "$WORK"; }
trap cleanup EXIT

# Stripped size (the release profile strips already; a binary from
# elsewhere may not be).
cp "$BIN" "$WORK/bin"
strip "$WORK/bin" 2>/dev/null || true
binary=$(wc -c < "$WORK/bin" | tr -d ' ')

PORT=$(python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')
URL="http://127.0.0.1:$PORT"
# env -i: nothing from the caller's shell (keys, SKRONGLE_DEV) reaches the
# server; the rest is what deploy/skrongle.service sets.
env -i PATH="$PATH" HOME="$WORK" \
  SKRONGLE_BIND="127.0.0.1:$PORT" SKRONGLE_DB="$WORK/learn.db" SKRONGLE_DATA_DIR="$PWD/data" SKRONGLE_LOG=info \
  MALLOC_ARENA_MAX=2 MALLOC_TRIM_THRESHOLD_=65536 \
  "$BIN" >"$WORK/server.log" 2>&1 &
PID=$!
for _ in $(seq 1 150); do curl -fsS "$URL/healthz" >/dev/null 2>&1 && break; sleep 0.2; done
curl -fsS "$URL/healthz" >/dev/null || { echo "server did not start:"; cat "$WORK/server.log"; exit 1; }

for _ in $(seq 1 20); do
  for p in / /login /healthz; do
    code=$(curl -s -o /dev/null -w '%{http_code}' "$URL$p")
    case "$code" in 2*|3*) ;; *) echo "GET $p: $code"; tail -20 "$WORK/server.log"; exit 1 ;; esac
  done
done
sleep 1 # let the last requests' connections close

if [ -r "/proc/$PID/status" ]; then
  rss=$(awk '/^VmRSS:/ { print $2 }' "/proc/$PID/status")
  threads=$(awk '/^Threads:/ { print $2 }' "/proc/$PID/status")
else
  # macOS: ps counts the shared system libraries in RSS (some 10 MiB more
  # than Linux's VmRSS for the same server), so it is shown, not checked.
  rss=$(ps -o rss= -p "$PID" | tr -d ' ')
  threads=$(( $(ps -M -p "$PID" | wc -l) - 1 ))
  rss_unchecked=1
fi

# Connected apps (#191): 300 MCP calls must not grow the server, nor must
# they once the tables an app can fill are at their caps.
# rss_now / threads_now: the server's numbers right now.
rss_now() {
  if [ -r "/proc/$PID/status" ]; then awk '/^VmRSS:/ { print $2 }' "/proc/$PID/status"
  else ps -o rss= -p "$PID" | tr -d ' '; fi
}
threads_now() {
  if [ -r "/proc/$PID/status" ]; then awk '/^Threads:/ { print $2 }' "/proc/$PID/status"
  else echo $(( $(ps -M -p "$PID" | wc -l) - 1 )); fi
}
# mcp_calls <token> <n>: n `tools/call list_courses`, one connection each,
# each answered 200 without an error.
mcp_calls() {
  for _ in $(seq 1 "$2"); do
    code=$(curl -s -o "$WORK/mcp" -w '%{http_code}' -H "Authorization: Bearer $1" -H 'content-type: application/json' \
      --data '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list_courses"}}' "$URL/mcp")
    if [ "$code" != 200 ] || ! grep -q '"isError":false' "$WORK/mcp"; then
      echo "POST /mcp list_courses: $code $(head -c 300 "$WORK/mcp")"; tail -20 "$WORK/server.log"; exit 1
    fi
  done
}
# The owner's account, made the way a release server allows: registering
# first (with SKRONGLE_SIGNUP unset sign-up is open and the first account is
# the owner's; with invite, the owner link from the log), then Let apps
# connect (Manage Users) and Make a token (Profile), as scripts/smoke.sh does
# on the dev server.
JAR="$WORK/cookies"
pw=pi-budget-password
invite=$(grep -o 'register?invite=[A-Za-z0-9_-]*' "$WORK/server.log" | head -1 | cut -d= -f2 || true)
post() { # post <path> <form>: the status of a form sent with the session
  curl -s -o "$WORK/post" -w '%{http_code}' -b "$JAR" -c "$JAR" --data "$2" "$URL$1"
}
code=$(post /register "email=owner%40example.org&password=$pw&age=on&invite=$invite")
case "$code" in 2*|3*) ;; *) echo "POST /register: $code $(head -c 300 "$WORK/post")"; exit 1 ;; esac
code=$(post /manage/apps "on=1")
[ "$code" = 303 ] || { echo "POST /manage/apps (Let apps connect): $code"; exit 1; }
make_token() { # make_token <name>: a personal token of the owner's
  post /profile/apps/token "name=$1&scope=courses%3Aread&lifetime=30&password=$pw" >/dev/null
  grep -o 'skr_[0-9a-f]\{64\}' "$WORK/post" | head -1 || true
}
warm=$(make_token Warm-up); first=$(make_token Budget)
[ -n "$warm" ] && [ -n "$first" ] || { echo "Make a token showed no token"; tail -20 "$WORK/server.log"; exit 1; }
# A few calls on another grant first, so what the first call sets up once
# (the per-grant limit's table, SQLite's statement cache) is in "before".
# 300 is also the per-grant limit (security::MCP_CALLS_PER_GRANT).
mcp_calls "$warm" 10; sleep 1
apps_before=$(rss_now)
mcp_calls "$first" 300; sleep 1
apps_after=$(rss_now)

# The tables at their caps (src/caps.rs, read here so they cannot drift):
# MAX_ACCOUNTS accounts, MAX_API_GRANTS grants (MAX_GRANTS_PER_ACCOUNT
# each), MAX_API_TOKENS tokens and MAX_OAUTH_CLIENTS registered apps, the
# rows written straight into the database (WAL, beside the running server).
# The owner's two grants above count; its other two each hold one token
# known here, stored as auth::hash_token stores it (SHA-256, hex), for
# another warm-up and another 300 calls.
caps_tokens=$(python3 - "$WORK/learn.db" <<'PY'
import hashlib, re, secrets, sqlite3, sys, time
caps = {}
for name, expr in re.findall(r'pub const (\w+): usize = ([A-Z0-9_ *+]+);', open("src/caps.rs").read()):
    caps[name] = eval(expr, {"__builtins__": {}}, caps)
now = int(time.time()); later = now + 30 * 86400
db = sqlite3.connect(sys.argv[1], timeout=30)
db.execute("PRAGMA foreign_keys = ON")
owner = db.execute("SELECT id FROM users WHERE owner = 1").fetchone()[0]
for i in range(caps["MAX_OAUTH_CLIENTS"]):
    db.execute("INSERT INTO oauth_clients (client_id, name, redirect_uris, created_at, last_grant_at) VALUES (?, ?, ?, ?, ?)",
               (secrets.token_hex(16), f"Budget app {i}", "https://app.example.org/callback", now, now))
clients = [r[0] for r in db.execute("SELECT client_id FROM oauth_clients")]
users = [owner]
for i in range(caps["MAX_ACCOUNTS"] - 1):
    users.append(db.execute("INSERT INTO users (email, password_hash, created_at) VALUES (?, '', ?)",
                            (f"budget{i}@example.org", now)).lastrowid)
def grant(user, client):
    return db.execute("INSERT INTO api_grants (user_id, client_id, client_name, scopes, created_at, expires_at) VALUES (?, ?, ?, 'courses:read courses:write', ?, ?)",
                      (user, client, f"App {client[:6]}", now, later)).lastrowid
def token(g, kind, used=None):
    t = "skr_" + secrets.token_hex(32)
    db.execute("INSERT INTO api_tokens (grant_id, kind, token_hash, expires_at, used_at) VALUES (?, ?, ?, ?, ?)",
               (g, kind, hashlib.sha256(t.encode()).hexdigest(), later, used))
    return t
known = []
for n, u in enumerate(users):
    have = db.execute("SELECT COUNT(*) FROM api_grants WHERE user_id = ?", (u,)).fetchone()[0]
    for k in range(caps["MAX_GRANTS_PER_ACCOUNT"] - have):
        if u == owner:
            known.append(token(grant(u, "personal"), "personal"))
        else:
            grant(u, clients[(n * caps["MAX_GRANTS_PER_ACCOUNT"] + k) % len(clients)])
grants = [r[0] for r in db.execute("SELECT id FROM api_grants ORDER BY id")]
# An OAuth app's four rows (caps.rs): access, refresh, the access token it
# replaced and the used refresh token, until api_tokens is full.
kinds = [("access", None), ("refresh", None), ("access", None), ("refresh", now)]
i = 0
while db.execute("SELECT COUNT(*) FROM api_tokens").fetchone()[0] < caps["MAX_API_TOKENS"]:
    kind, used = kinds[(i // len(grants)) % 4]
    token(grants[i % len(grants)], kind, used); i += 1
db.commit()
counts = {t: db.execute(f"SELECT COUNT(*) FROM {t}").fetchone()[0] for t in ("users", "oauth_clients", "api_grants", "api_tokens")}
want = {"users": caps["MAX_ACCOUNTS"], "oauth_clients": caps["MAX_OAUTH_CLIENTS"], "api_grants": caps["MAX_API_GRANTS"], "api_tokens": caps["MAX_API_TOKENS"]}
if counts != want or len(known) != 2:
    sys.exit(f"tables not at their caps: {counts}, want {want}")
print(" ".join(known), " ".join(f"{t}={n}" for t, n in counts.items()))
PY
) || { echo "filling the tables failed"; exit 1; }
set -- $caps_tokens
warm=$1; second=$2; shift 2; caps_counts="$*"
mcp_calls "$warm" 10; sleep 1
caps_before=$(rss_now)
mcp_calls "$second" 300; sleep 1
caps_after=$(rss_now)
# Db::call runs on Tokio's blocking pool (at most 2 threads, main.rs), whose
# threads end after 10 idle seconds; the idle count above had none. So wait
# up to 15 s for the count to come back to the idle one.
for _ in $(seq 1 30); do
  apps_threads=$(threads_now)
  [ "$apps_threads" -gt "$threads" ] || break
  sleep 0.5
done

kill "$PID" 2>/dev/null || true; wait "$PID" 2>/dev/null || true; PID=""

baseline=$(setting binary_baseline "$platform")
slack=$(setting binary_slack_percent)
rss_max=$(setting rss_max_kib)
rss_limit="$rss_max KiB"
[ -z "${rss_unchecked:-}" ] || rss_limit="$rss_max KiB on Linux, not checked here"
threads_max=$(setting threads_max)
growth_max=$(setting apps_rss_growth_max_kib)
# verdict <before> <after>: ok, over, or "not checked here" (a Mac).
verdict() {
  if [ -n "${rss_unchecked:-}" ]; then echo "not checked here"
  elif [ $(( $2 - $1 )) -le "$growth_max" ]; then echo ok
  else echo over; fi
}
apps_verdict=$(verdict "$apps_before" "$apps_after")
caps_verdict=$(verdict "$caps_before" "$caps_after")
apps_threads_verdict=ok; [ "$apps_threads" -le "$threads" ] || apps_threads_verdict=over
mib() { awk -v b="$1" 'BEGIN { printf "%.2f MiB", b / 1048576 }'; }

if [ "$UPDATE" = 1 ]; then
  if [ -n "$baseline" ]; then
    awk -v p="$platform" -v n="$binary" '$1 == "binary_baseline" && $2 == p { $3 = n } { print }' "$budget" > "$WORK/b"
    cat "$WORK/b" > "$budget"
  else
    echo "binary_baseline $platform $binary" >> "$budget"
  fi
  echo "wrote $budget: binary_baseline $platform $binary"
  baseline=$binary
fi

if [ -n "$baseline" ]; then
  binary_max=$(( baseline + baseline * slack / 100 ))
  binary_limit="$binary_max ($(mib "$binary_max"), baseline $baseline + $slack %)"
else
  binary_max=""
  binary_limit="none, no baseline for $platform"
fi

summary="binary  $binary bytes ($(mib "$binary")), limit $binary_limit
idle RSS $rss KiB ($(mib $(( rss * 1024 )))), limit $rss_limit
threads $threads, limit $threads_max
connected apps, 300 list_courses: RSS $apps_before -> $apps_after KiB ($(( apps_after - apps_before )) KiB, limit +$growth_max): $apps_verdict
connected apps, tables at caps ($caps_counts), 300 list_courses: RSS $caps_before -> $caps_after KiB ($(( caps_after - caps_before )) KiB, limit +$growth_max): $caps_verdict
connected apps, threads after: $apps_threads, limit the idle $threads: $apps_threads_verdict
platform $platform"

if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
  {
    echo "### Pi budget ($platform)"
    echo
    echo "| | measured | limit |"
    echo "|---|---|---|"
    echo "| binary (stripped) | $binary bytes ($(mib "$binary")) | ${binary_max:-no baseline} |"
    echo "| idle RSS | $rss KiB ($(mib $(( rss * 1024 )))) | $rss_limit |"
    echo "| threads | $threads | $threads_max |"
    echo "| connected apps: RSS before and after 300 \`list_courses\` | $apps_before → $apps_after KiB ($(( apps_after - apps_before ))) | +$growth_max KiB: $apps_verdict |"
    echo "| connected apps: the same, tables at their caps ($caps_counts) | $caps_before → $caps_after KiB ($(( caps_after - caps_before ))) | +$growth_max KiB: $caps_verdict |"
    echo "| connected apps: threads after both | $apps_threads | the idle $threads: $apps_threads_verdict |"
  } >> "$GITHUB_STEP_SUMMARY"
fi

over=""
[ -z "$binary_max" ] || [ "$binary" -le "$binary_max" ] || over="$over binary"
[ -n "${rss_unchecked:-}" ] || [ "$rss" -le "$rss_max" ] || over="$over rss"
[ "$threads" -le "$threads_max" ] || over="$over threads"
[ "$apps_verdict" != over ] || over="$over apps-rss"
[ "$caps_verdict" != over ] || over="$over apps-at-caps-rss"
[ "$apps_threads_verdict" != over ] || over="$over apps-threads"
if [ -n "$over" ]; then
  cat <<EOF

Pi budget exceeded:$over
$summary

The server must fit a Pi Zero 2 W (MemoryMax=64M, TasksMax=32). Make the new
code cheaper (a cargo feature, a lighter crate, no extra thread), or, if the
cost is worth it, raise the number in $budget in this PR so the
reviewer sees it (scripts/pi-budget.sh --update writes the binary baseline).
A connected-apps line over its margin is memory a request kept: find what
holds it rather than raising the margin.
EOF
  exit 1
fi
echo "$summary"
echo "pi budget: within limits"
