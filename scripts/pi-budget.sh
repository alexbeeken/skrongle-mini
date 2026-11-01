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
# The server runs with no optional settings (no mail, SMS, Google, Stripe or
# AI key, no SKRONGLE_DEV), only what deploy/skrongle.service sets, on a temp
# database and a free loopback port. It answers /, /login and /healthz
# twenty times each; then the numbers are read and the server stopped.
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
    *) sed -n '2,22p' "$0"; exit 1 ;;
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
  SKRONGLE_BIND="127.0.0.1:$PORT" SKRONGLE_DB="$WORK/learn.db" SKRONGLE_DATA_DIR="$PWD/data" SKRONGLE_LOG=info SKRONGLE_MINI=1 \
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
kill "$PID" 2>/dev/null || true; wait "$PID" 2>/dev/null || true; PID=""

baseline=$(setting binary_baseline "$platform")
slack=$(setting binary_slack_percent)
rss_max=$(setting rss_max_kib)
rss_limit="$rss_max KiB"
[ -z "${rss_unchecked:-}" ] || rss_limit="$rss_max KiB on Linux, not checked here"
threads_max=$(setting threads_max)
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
  } >> "$GITHUB_STEP_SUMMARY"
fi

over=""
[ -z "$binary_max" ] || [ "$binary" -le "$binary_max" ] || over="$over binary"
[ -n "${rss_unchecked:-}" ] || [ "$rss" -le "$rss_max" ] || over="$over rss"
[ "$threads" -le "$threads_max" ] || over="$over threads"
if [ -n "$over" ]; then
  cat <<EOF

Pi budget exceeded:$over
$summary

The server must fit a Pi Zero 2 W (MemoryMax=64M, TasksMax=32). Make the new
code cheaper (a cargo feature, a lighter crate, no extra thread), or, if the
cost is worth it, raise the number in $budget in this PR so the
reviewer sees it (scripts/pi-budget.sh --update writes the binary baseline).
EOF
  exit 1
fi
echo "$summary"
echo "pi budget: within limits"
