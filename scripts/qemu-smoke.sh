#!/usr/bin/env bash
# Run a 32-bit ARM release binary under qemu-user on an x86_64 Linux box:
# --version, then the server answering /healthz. The release workflow calls
# it for armv6 and armv7 (a real Pi Zero / Pi 2 is the final word; this
# catches a binary that does not load or that uses instructions the CPU lacks).
#
#   sudo apt-get install -y qemu-user-static libc6-armhf-cross
#   scripts/qemu-smoke.sh armv6 [binary]     # emulates an ARM1176 (Pi 1 / Zero)
#   scripts/qemu-smoke.sh armv7 [binary]     # emulates a Cortex-A7 (Pi 2)
set -euo pipefail
cd "$(dirname "$0")/.."

ARCH="${1:?usage: scripts/qemu-smoke.sh <armv6|armv7> [binary]}"
case "$ARCH" in
  armv6) TARGET=arm-unknown-linux-gnueabihf; CPU=arm1176 ;;
  armv7) TARGET=armv7-unknown-linux-gnueabihf; CPU=cortex-a7 ;;
  *) echo "unknown arch: $ARCH (use armv6 or armv7)"; exit 1 ;;
esac
BIN="${2:-dist/$TARGET/spanish-learning}"
[ -x "$BIN" ] || { echo "no binary at $BIN"; exit 1; }
# The armhf dynamic loader and glibc (libc6-armhf-cross).
SYSROOT="${QEMU_LD_PREFIX:-/usr/arm-linux-gnueabihf}"
run() { qemu-arm-static -cpu "$CPU" -L "$SYSROOT" "$BIN" "$@"; }

WORK=$(mktemp -d)
trap 'kill "${PID:-}" 2>/dev/null || true; rm -rf "$WORK"' EXIT

echo "version: $(run --version)"
run --version | grep -q '^skrongle '

PORT=$(python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')
SKRONGLE_BIND="127.0.0.1:$PORT" SKRONGLE_DB="$WORK/qemu.db" SKRONGLE_DATA_DIR=./data SKRONGLE_LOG=info \
  run >"$WORK/server.log" 2>&1 &
PID=$!
# Emulated, so allow a minute: the first start creates the database.
for _ in $(seq 1 300); do
  curl -fsS "http://127.0.0.1:$PORT/healthz" >/dev/null 2>&1 && break
  kill -0 "$PID" 2>/dev/null || break
  sleep 0.2
done
if ! out=$(curl -fsS "http://127.0.0.1:$PORT/healthz"); then
  echo "server did not answer /healthz under qemu ($CPU):"; cat "$WORK/server.log"; exit 1
fi
echo "healthz: $out"
