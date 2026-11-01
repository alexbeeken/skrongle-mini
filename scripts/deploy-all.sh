#!/usr/bin/env bash
# The master deploy: tests, one cross-compile, then every configured target.
#
#   scripts/deploy-all.sh                 # targets from DEPLOY_TARGETS in deploy/deploy.env
#   scripts/deploy-all.sh aws             # one target: aws | pi
#   scripts/deploy-all.sh pi aws --skip-tests
#   scripts/deploy-all.sh --dry-run       # show what would happen, run nothing
#
# Settings: deploy/deploy.env (copy deploy/deploy.env.example and fill it in).
set -euo pipefail
cd "$(dirname "$0")/.."
. scripts/deploy-env.sh

TARGETS=()
SKIP_TESTS=0; DRY=0
for a in "$@"; do
  case "$a" in
    pi|aws) TARGETS+=("$a") ;;
    --skip-tests) SKIP_TESTS=1 ;;
    --dry-run) DRY=1 ;;
    -h|--help) sed -n '2,9p' "$0"; exit 0 ;;
    *) echo "unknown argument: $a (targets: pi aws)"; exit 1 ;;
  esac
done
if [ ${#TARGETS[@]} -eq 0 ]; then
  # shellcheck disable=SC2206
  TARGETS=(${DEPLOY_TARGETS:-pi aws})
fi
for t in "${TARGETS[@]}"; do
  case "$t" in
    pi) need_pi_env ;;
    aws) need_aws_env ;;
    *) echo "unknown target in DEPLOY_TARGETS: $t"; exit 1 ;;
  esac
done

echo "==> deploying to: ${TARGETS[*]}"
if [ "$DRY" = 1 ]; then
  for t in "${TARGETS[@]}"; do
    case "$t" in
      pi)  echo "    pi:  $PI_HOST (key: ${PI_SSH_KEY:-ssh config}) -> $PI_PUBLIC_URL" ;;
      aws) echo "    aws: stack $AWS_STACK_NAME in $AWS_REGION, $AWS_INSTANCE_TYPE, key $AWS_SSH_KEY, domain ${AWS_DOMAIN:-<ip>.sslip.io}"
           echo "         provider settings from Parameter Store $AWS_SSM_PATH (scripts/aws-env.sh list)" ;;
    esac
  done
  exit 0
fi

# rustup installs to ~/.cargo/bin without touching the shell profile
if ! command -v cargo >/dev/null 2>&1; then [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"; fi

if [ "$SKIP_TESTS" = 0 ]; then
  echo "==> running tests"
  cargo test --quiet 2>&1 | tail -3
fi

echo "==> cross-compiling once for aarch64 (used by AWS and a 64-bit Pi)"
scripts/build-pi.sh aarch64 2>&1 | grep -vE '^\s+Compiling|linker stderr|^\s+\||= note|generated [0-9]+ warning' || true

FAILED=()
for t in "${TARGETS[@]}"; do
  echo
  echo "=================== $t ==================="
  case "$t" in
    pi)  scripts/release.sh --skip-tests --skip-build || FAILED+=(pi) ;;
    aws) scripts/deploy-aws.sh --skip-build || FAILED+=(aws) ;;
  esac
done

echo
if [ ${#FAILED[@]} -gt 0 ]; then
  echo "==> FAILED: ${FAILED[*]}"
  exit 1
fi
echo "==> all targets deployed"
