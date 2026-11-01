#!/usr/bin/env bash
# The laptop's GitHub Actions runners (ci/runner/compose.yml), in Docker.
#   scripts/ci-runners.sh up       build the image and start the eight runners
#   scripts/ci-runners.sh down     stop them (they deregister from GitHub)
#   scripts/ci-runners.sh status   containers here, runners as GitHub sees them
#   scripts/ci-runners.sh logs     follow the runners' logs
#   scripts/ci-runners.sh reset    stop them and delete their work volumes
# See docs/testing.md, "CI runners".
set -euo pipefail
cd "$(dirname "$0")/../ci/runner"

if docker compose version >/dev/null 2>&1; then
  compose() { docker compose -p pooki-ci "$@"; }
elif command -v docker-compose >/dev/null; then
  compose() { docker-compose -p pooki-ci "$@"; }
else
  echo "needs Docker with Compose (brew install colima docker docker-compose)" >&2; exit 1
fi

docker info >/dev/null 2>&1 || {
  echo "Docker isn't running: colima start (brew services start colima starts it at login)" >&2; exit 1; }

# The image bakes in the Chromium of the Playwright version the specs use.
PLAYWRIGHT_VERSION=$(sed -n 's/.*"@playwright\/test": *"\([^"]*\)".*/\1/p' ../../e2e/package.json)
export PLAYWRIGHT_VERSION

# compose.yml insists on the token; only `up` registers runners with it.
[ "${1:-}" = up ] || export POOKI_CI_RUNNER_TOKEN="${POOKI_CI_RUNNER_TOKEN:-unused}"

case "${1:-}" in
  up)
    [ -n "${POOKI_CI_RUNNER_TOKEN:-}" ] || { echo "set POOKI_CI_RUNNER_TOKEN (see docs/testing.md, \"CI runners\")" >&2; exit 1; }
    # Every service names the same image; built per service, the builds race
    # for its tag ("already exists"). Build it once, then start them all.
    compose build runner-1
    compose up -d --no-build ;;
  down) compose down ;;
  status)
    compose ps
    gh api repos/alexbeeken/pooki/actions/runners \
      --jq '.runners[] | "\(.name)\t\(.status)\t\(if .busy then "busy" else "idle" end)"' ;;
  logs) compose logs -f ;;
  reset) compose down -v ;;
  *) sed -n '2,8p' "$0"; exit 1 ;;
esac
