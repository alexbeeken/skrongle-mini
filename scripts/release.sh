#!/usr/bin/env bash
# One-shot release to the Pi: test -> cross-compile -> rsync + systemd + nginx -> verify.
#
#   scripts/release.sh                         # settings from deploy/deploy.env (PI_HOST, PI_SSH_KEY, PI_PUBLIC_URL…)
#   scripts/release.sh --skip-tests            # faster
#   scripts/release.sh --skip-build            # use the binary already in dist/ (scripts/deploy-all.sh does this)
#   PI_HOST=pi@other.local scripts/release.sh  # environment overrides the file
#
# PI_HOST and PI_PUBLIC_URL have no defaults: set them in deploy/deploy.env.
#
# Builds with the hardened release profile (fat LTO, codegen-units=1, panic=abort,
# stripped, target-cpu=cortex-a53) for whichever architecture the Pi reports.
set -euo pipefail
cd "$(dirname "$0")/.."
DEPLOY_ENV_OPTIONAL=1 . scripts/deploy-env.sh

: "${PI_HOST:?set PI_HOST in deploy/deploy.env (user@host; template: deploy/deploy.env.example)}"
PUBLIC_URL="${PI_PUBLIC_URL:-${PUBLIC_URL:-}}"
: "${PUBLIC_URL:?set PI_PUBLIC_URL in deploy/deploy.env (the https URL the app is reached at)}"
SSH_OPTS=$(ssh_opts_for_key "${PI_SSH_KEY:-}")
export SSH_OPTS PI_PUBLIC_URL="$PUBLIC_URL" PI_BASE_PATH="${PI_BASE_PATH:-/spanish}"
SKIP_TESTS=0; SKIP_BUILD=0
for a in "$@"; do
  case "$a" in
    --skip-tests) SKIP_TESTS=1 ;;
    --skip-build) SKIP_BUILD=1 ;;
    -h|--help) sed -n '2,13p' "$0"; exit 0 ;;
    *) echo "unknown flag: $a"; exit 1 ;;
  esac
done

# rustup installs to ~/.cargo/bin without touching the shell profile
if ! command -v cargo >/dev/null 2>&1; then [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"; fi

echo "==> [1/5] checking $PI_HOST"
# shellcheck disable=SC2086
ARCH_RAW=$(ssh $SSH_OPTS "$PI_HOST" 'uname -m') || { echo "cannot reach $PI_HOST over ssh (first attempts to the Pi sometimes fail: ping it and re-run)"; exit 1; }
case "$ARCH_RAW" in
  aarch64) ARCH=aarch64 ;;
  armv7l)  ARCH=armv7 ;;
  *) echo "unsupported Pi architecture: $ARCH_RAW"; exit 1 ;;
esac
echo "    Pi reports $ARCH_RAW -> building for $ARCH"

if [ "$SKIP_TESTS" = 0 ]; then
  echo "==> [2/5] running tests"
  cargo test --quiet 2>&1 | tail -3
else
  echo "==> [2/5] tests skipped"
fi

case "$ARCH" in aarch64) BIN=dist/aarch64-unknown-linux-gnu/spanish-learning ;; *) BIN=dist/armv7-unknown-linux-gnueabihf/spanish-learning ;; esac
if [ "$SKIP_BUILD" = 1 ] && [ -x "$BIN" ]; then
  echo "==> [3/5] build skipped (using $BIN)"
else
  echo "==> [3/5] cross-compiling (release profile)"
  scripts/build-pi.sh "$ARCH" 2>&1 | grep -vE '^\s+Compiling|linker stderr|^\s+\||= note|generated [0-9]+ warning' || true
fi

echo "==> [4/5] deploying to $PI_HOST"
scripts/deploy.sh "$PI_HOST" "$ARCH"

echo "==> [5/5] verifying"
sleep 1
# shellcheck disable=SC2086
LOCAL=$(ssh $SSH_OPTS "$PI_HOST" "curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:8080$PI_BASE_PATH/healthz" || echo "000")
echo "    app on the Pi (127.0.0.1:8080$PI_BASE_PATH/healthz): HTTP $LOCAL"
PUB=$(curl -s -o /dev/null -w "%{http_code}" --max-time 10 "$PUBLIC_URL/healthz" || echo "000")
echo "    public ($PUBLIC_URL/healthz): HTTP $PUB"
if [ "$LOCAL" != "200" ]; then
  # shellcheck disable=SC2086
  echo "    app is not healthy; recent log:"; ssh $SSH_OPTS "$PI_HOST" 'sudo journalctl -u spanish-learning -n 20 --no-pager'; exit 1
fi
[ "$PUB" = "200" ] || echo "    (public check failed from this network; the app itself is healthy)"
echo "==> released: $PUBLIC_URL"
