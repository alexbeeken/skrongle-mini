#!/bin/sh
# Check a release before tagging it: Cargo.toml has the version, CHANGELOG.md
# has its section, and, when the release tarball for this machine is in
# dist/, that it runs and prints the version.
#
#   scripts/release-check.sh 0.2.0      (a leading v is fine too)
#
# Exits 1 on the first failure. The tarball check needs Linux (the binary is
# Linux) and a tarball from scripts/package-release.sh; without one it says so
# and passes.
set -eu
cd "$(dirname "$0")/.."

if [ "$#" -ne 1 ]; then
  echo "usage: scripts/release-check.sh <version>" >&2
  exit 2
fi
version="${1#v}"
fail() { echo "FAIL: $*" >&2; exit 1; }

cargo_version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)
[ "$cargo_version" = "$version" ] \
  || fail "Cargo.toml says version $cargo_version, not $version: bump it first"
echo "ok: Cargo.toml is $version"

# The section runs from its "## <version> (" heading to the next "## " heading.
section() {
  awk -v v="$1" 'index($0, "## " v " (") == 1 { on = 1; next } /^## / { on = 0 } on' CHANGELOG.md
}
section "$version" | grep -q '[^[:space:]]' \
  || fail "CHANGELOG.md has no section for $version: add \"## $version (<date>)\" with its notes"
echo "ok: CHANGELOG.md has the $version section"

case "$(uname -m)" in
  arm64 | aarch64) arch=aarch64 ;;
  x86_64 | amd64) arch=x86_64 ;;
  *) arch=$(uname -m) ;;
esac
tarball="dist/skrongle-$version-$arch.tar.gz"
if [ ! -f "$tarball" ]; then
  echo "skip: no $tarball (scripts/build-pi.sh and scripts/package-release.sh make it)"
  exit 0
fi
if [ "$(uname -s)" != Linux ]; then
  echo "skip: $tarball is a Linux build; run this check on Linux to run its binary"
  exit 0
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
tar -xzf "$tarball" -C "$tmp"
got=$("$tmp/skrongle" --version) || fail "$tarball: skrongle --version did not run"
[ "$got" = "skrongle $version" ] || fail "$tarball: skrongle --version printed '$got', not 'skrongle $version'"
echo "ok: $tarball runs and prints $got"
