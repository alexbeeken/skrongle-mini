#!/usr/bin/env bash
# Pack a built binary into the release tarball self-hosters download.
#
#   scripts/package-release.sh <version> <arch> [binary]
#   scripts/package-release.sh 0.2.0 aarch64     # binary from dist/aarch64-unknown-linux-gnu/ (scripts/build-pi.sh)
#   scripts/package-release.sh 0.2.0 x86_64 target/release/spanish-learning
#
# <arch> is what `uname -m` prints on the target: aarch64 or x86_64.
# Output: dist/skrongle-<version>-<arch>.tar.gz (+ .sha256), which unpacks
# flat into /opt/skrongle:
#
#   skrongle            the server (built as spanish-learning, renamed)
#   data/courses/       built-in courses   } SKRONGLE_DATA_DIR
#   data/themes/        built-in themes    }
#   data/badges/        the SKC shop       }
#   skrongle.service    deploy/skrongle.service
#   terms.example.md    deploy/terms.example.md: a template for the owner's own Terms (SKRONGLE_TERMS)
#   update.sh           deploy/update.sh: `sudo /opt/skrongle/update.sh`
#   VERSION             the version, one line
#
# The release workflow (.github/workflows/release.yml) runs this for each
# architecture on a `v*` tag. Run it locally to check the layout.
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION="${1:?usage: scripts/package-release.sh <version> <arch> [binary]}"
ARCH="${2:?usage: scripts/package-release.sh <version> <arch> [binary]}"
VERSION="${VERSION#v}"
case "$ARCH" in
  aarch64) TARGET=aarch64-unknown-linux-gnu ;;
  x86_64)  TARGET=x86_64-unknown-linux-gnu ;;
  *) echo "unknown arch: $ARCH (use aarch64 or x86_64)"; exit 1 ;;
esac
BIN="${3:-dist/$TARGET/spanish-learning}"
[ -x "$BIN" ] || { echo "no binary at $BIN: build it first (scripts/build-pi.sh $ARCH)"; exit 1; }

NAME="skrongle-$VERSION-$ARCH"
STAGE="dist/$NAME"
rm -rf "$STAGE"
mkdir -p "$STAGE/data"

cp "$BIN" "$STAGE/skrongle"
chmod 0755 "$STAGE/skrongle"
# Courses, themes and the shop's badges only: test courses and LLM fixtures are for the test
# suites, and the authoring briefs are for whoever writes the content.
for d in courses themes badges; do
  cp -R "data/$d" "$STAGE/data/$d"
done
find "$STAGE/data" \( -name AUTHORING.md -o -name fact-check-ok.txt -o -name .DS_Store \) -delete
cp deploy/skrongle.service "$STAGE/skrongle.service"
cp deploy/terms.example.md "$STAGE/terms.example.md"
cp deploy/update.sh "$STAGE/update.sh"
chmod 0755 "$STAGE/update.sh"
echo "$VERSION" > "$STAGE/VERSION"

TARBALL="dist/$NAME.tar.gz"
# Stable owner and order, so the archive only changes when its files do.
TAR_FLAGS=()
if tar --version 2>/dev/null | grep -q GNU; then
  TAR_FLAGS=(--owner=0 --group=0 --numeric-owner --sort=name)
else
  TAR_FLAGS=(--uid 0 --gid 0)
fi
COPYFILE_DISABLE=1 tar "${TAR_FLAGS[@]}" -czf "$TARBALL" -C "$STAGE" .
rm -rf "$STAGE"
if command -v sha256sum >/dev/null; then SHA=(sha256sum); else SHA=(shasum -a 256); fi
( cd dist && "${SHA[@]}" "$NAME.tar.gz" > "$NAME.tar.gz.sha256" )

echo "packed $TARBALL"
ls -lh "$TARBALL"
tar -tzf "$TARBALL" | grep -vE '^\./data/.+/.+/' | sort
