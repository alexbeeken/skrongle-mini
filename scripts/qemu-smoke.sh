#!/usr/bin/env bash
# Run a 32-bit ARM release binary under QEMU user-mode emulation, with the C
# library of a real board of that architecture: --version, then the server
# answering /healthz. The release workflow calls it for armv6 and armv7 (a
# real Pi Zero / Pi 2 is the final word; this catches a binary that does not
# load or that uses instructions the CPU lacks).
#
# It runs the binary in a Docker container of the target architecture, so
# the dynamic loader and glibc are the board's own: Raspbian's for armv6
# (Debian's armhf is built for ARMv7 and itself faults on an ARMv6 CPU),
# Debian's for armv7. QEMU_CPU pins the emulated core to the oldest the
# build must run on, so an ARMv7 instruction in an armv6 build faults here
# instead of on someone's Pi Zero.
#
#   scripts/qemu-smoke.sh armv6 [binary]     # an ARM1176 (Pi 1 / Zero), Raspbian 11 glibc
#   scripts/qemu-smoke.sh armv7 [binary]     # a Cortex-A7 (Pi 2), Debian 11 glibc
#
# Needs Docker. The QEMU handlers for 32-bit ARM are installed once per
# Docker host (tonistiigi/binfmt), which needs a privileged container.
set -euo pipefail
cd "$(dirname "$0")/.."
ARCH="${1:?usage: scripts/qemu-smoke.sh <armv6|armv7> [binary]}"
case "$ARCH" in
  armv6) TARGET=arm-unknown-linux-gnueabihf; CPU=arm1176; PLATFORM=linux/arm/v6; IMAGE=balenalib/rpi-raspbian:bullseye; MACHINE=armv6l ;;
  armv7) TARGET=armv7-unknown-linux-gnueabihf; CPU=cortex-a7; PLATFORM=linux/arm/v7; IMAGE=arm32v7/debian:bullseye-slim; MACHINE=armv7l ;;
  *) echo "unknown arch: $ARCH (use armv6 or armv7)"; exit 1 ;;
esac
BIN="${2:-dist/$TARGET/spanish-learning}"
[ -x "$BIN" ] || { echo "no binary at $BIN"; exit 1; }
command -v docker >/dev/null || { echo "no docker"; exit 1; }

# The handlers, once: a Docker that already runs this platform needs nothing.
if ! docker run --rm --platform "$PLATFORM" "$IMAGE" true >/dev/null 2>&1; then
  docker run --rm --privileged tonistiigi/binfmt --install arm >/dev/null
fi

# A folder with the binary as /opt/skrongle/skrongle and the courses, like an
# install. Under dist/ (ignored by git), not the system temp dir: a Docker
# that runs in a VM shares the repository with it, not necessarily /tmp.
mkdir -p dist
WORK=$(mktemp -d "$PWD/dist/.qemu-smoke.XXXXXX")
NAME="skrongle-qemu-smoke-$$"
trap 'docker rm -f "$NAME" >/dev/null 2>&1 || true; rm -rf "$WORK"' EXIT
mkdir -p "$WORK/opt/data"
cp "$BIN" "$WORK/opt/skrongle"; chmod 0755 "$WORK/opt/skrongle"
cp -R data/courses "$WORK/opt/data/courses"

run() { docker run --rm --platform "$PLATFORM" -e QEMU_CPU="$CPU" -v "$WORK/opt:/opt/skrongle:ro" "$IMAGE" "$@"; }
echo "machine: $(run uname -m) (QEMU_CPU=$CPU, $IMAGE)"
[ "$(run uname -m)" = "$MACHINE" ] || { echo "the container is not $MACHINE"; exit 1; }
version=$(run /opt/skrongle/skrongle --version)
echo "version: $version"
echo "$version" | grep -q '^skrongle '

PORT=$(python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')
docker run -d --name "$NAME" --platform "$PLATFORM" -e QEMU_CPU="$CPU" -v "$WORK/opt:/opt/skrongle:ro" \
  -p "127.0.0.1:$PORT:7576" -e SKRONGLE_BIND=0.0.0.0:7576 -e SKRONGLE_DB=/tmp/qemu.db -e SKRONGLE_DATA_DIR=/opt/skrongle/data -e SKRONGLE_LOG=info \
  -w /tmp "$IMAGE" /opt/skrongle/skrongle >/dev/null
# Emulated, so allow a few minutes: the first start creates the database.
for _ in $(seq 1 900); do
  curl -fsS "http://127.0.0.1:$PORT/healthz" >/dev/null 2>&1 && break
  [ "$(docker inspect -f '{{.State.Running}}' "$NAME" 2>/dev/null)" = true ] || break
  sleep 0.2
done
if ! out=$(curl -fsS "http://127.0.0.1:$PORT/healthz"); then
  echo "server did not answer /healthz under qemu ($CPU):"; docker logs "$NAME" 2>&1 | tail -40; exit 1
fi
echo "healthz: $out"
