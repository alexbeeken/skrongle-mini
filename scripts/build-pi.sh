#!/usr/bin/env bash
# Cross-compile a release binary for the Raspberry Pi from macOS (Apple Silicon),
# or for any 64-bit Linux (the release workflow runs it on Linux too), with
# cargo-zigbuild.
#
#   scripts/build-pi.sh                 # aarch64 (64-bit Raspberry Pi OS), via cargo-zigbuild
#   scripts/build-pi.sh armv7           # 32-bit Raspberry Pi OS
#   scripts/build-pi.sh x86_64          # 64-bit PC / server Linux
#   PROFILE=release-small scripts/build-pi.sh   # opt-level="z" variant
#
# Output: dist/<target>/spanish-learning
set -euo pipefail
cd "$(dirname "$0")/.."

ARCH="${1:-aarch64}"
PROFILE="${PROFILE:-release}"
# Raspberry Pi OS Bookworm ships glibc 2.36, Bullseye 2.31. Targeting 2.31 keeps
# the binary working on both; the dynamic glibc is the only non-static dependency.
GLIBC="${GLIBC:-2.31}"

case "$ARCH" in
  aarch64|arm64) TARGET=aarch64-unknown-linux-gnu ;;
  armv7|arm32)   TARGET=armv7-unknown-linux-gnueabihf ;;
  x86_64|amd64)  TARGET=x86_64-unknown-linux-gnu ;;
  *) echo "unknown arch: $ARCH (use aarch64, armv7 or x86_64)"; exit 1 ;;
esac

rustup target add "$TARGET" >/dev/null

command -v cargo-zigbuild >/dev/null || { echo "install with: brew install zig && cargo install cargo-zigbuild"; exit 1; }
cargo zigbuild --profile "$PROFILE" --target "$TARGET.$GLIBC"

OUT_DIR="target/$TARGET/$PROFILE"
mkdir -p "dist/$TARGET"
cp "$OUT_DIR/spanish-learning" "dist/$TARGET/spanish-learning"
echo
echo "built dist/$TARGET/spanish-learning"
ls -lh "dist/$TARGET/spanish-learning"
file "dist/$TARGET/spanish-learning" 2>/dev/null || true
