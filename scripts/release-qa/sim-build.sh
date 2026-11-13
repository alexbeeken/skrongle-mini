#!/usr/bin/env bash
# Build the AI simulator for the rig's `simulator` profile (compose.yaml, #41):
# this checkout with the dev feature, as a Linux binary for <arch> (glibc 2.31,
# cargo-zigbuild, as scripts/build-pi.sh). It is a test tool, never packed in
# a tarball. Prints the binary's path on the last line.
#
#   scripts/release-qa/sim-build.sh aarch64     # target/aarch64-unknown-linux-gnu/release-qa-sim/spanish-learning
#   scripts/release-qa/sim-build.sh x86_64
#
# Why not a plain debug build: in a debug build rust-embed reads i18n/,
# static/ and the templates from this checkout's absolute path at run time,
# which the container does not have. A build without debug assertions embeds
# them, as the release does. The profile `release-qa-sim` is release without
# fat LTO and with parallel codegen (defined here with --config, not in
# Cargo.toml), so it builds in a fraction of the time; speed is not what the
# simulator is for.
set -euo pipefail
cd "$(dirname "$0")/../.."
command -v cargo >/dev/null 2>&1 || { [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"; }

case "${1:?usage: sim-build.sh <aarch64 | x86_64>}" in
  aarch64) TARGET=aarch64-unknown-linux-gnu ;;
  x86_64) TARGET=x86_64-unknown-linux-gnu ;;
  *) echo "sim-build.sh: aarch64 or x86_64 (the rig's e2e architectures)" >&2; exit 2 ;;
esac
command -v cargo-zigbuild >/dev/null || { echo "install with: brew install zig && cargo install cargo-zigbuild" >&2; exit 1; }
rustup target add "$TARGET" >/dev/null 2>&1 || true

P=release-qa-sim
cargo zigbuild -q --features dev --target "$TARGET.2.31" --profile "$P" \
  --config "profile.$P.inherits=\"release\"" --config "profile.$P.lto=false" \
  --config "profile.$P.codegen-units=16" >&2

BIN="${CARGO_TARGET_DIR:-$(pwd)/target}/$TARGET/$P/spanish-learning"
[ -x "$BIN" ] || { echo "no binary at $BIN" >&2; exit 1; }
echo "$BIN"
