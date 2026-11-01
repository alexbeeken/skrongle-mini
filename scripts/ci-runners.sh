#!/usr/bin/env bash
# The laptop's GitHub Actions runners for skrongle-mini (ci/runner/compose.yml), in Docker.
#   scripts/ci-runners.sh up       build the image and start the four runners
#   scripts/ci-runners.sh down     stop them (they deregister from GitHub)
#   scripts/ci-runners.sh status   containers here, runners as GitHub sees them
#   scripts/ci-runners.sh logs     follow the runners' logs
#   scripts/ci-runners.sh prune    delete the runners GitHub lists as offline
#   scripts/ci-runners.sh reset    stop them and delete their work volumes
# See docs/testing.md, "CI runners".
set -euo pipefail
cd "$(dirname "$0")/../ci/runner"

REPO=alexbeeken/skrongle-mini

if docker compose version >/dev/null 2>&1; then
  compose() { docker compose -p skrongle-mini-ci "$@"; }
elif command -v docker-compose >/dev/null; then
  compose() { docker-compose -p skrongle-mini-ci "$@"; }
else
  echo "needs Docker with Compose (brew install colima docker docker-compose)" >&2; exit 1
fi

docker info >/dev/null 2>&1 || {
  echo "Docker isn't running: colima start (brew services start colima starts it at login)" >&2; exit 1; }

# The image bakes in the Chromium of the Playwright version the specs use.
PLAYWRIGHT_VERSION=$(sed -n 's/.*"@playwright\/test": *"\([^"]*\)".*/\1/p' ../../e2e/package.json)
export PLAYWRIGHT_VERSION
export SKRONGLE_MINI_CI_RUNNER_TOKEN="${SKRONGLE_MINI_CI_RUNNER_TOKEN:-}"
export SKRONGLE_MINI_CI_REGISTRATION_TOKEN=""

case "${1:-}" in
  up)
    # A fine-grained token in the shell keeps the runners registered across
    # restarts. Without one, a registration token from `gh` (valid an hour)
    # starts them now; they keep running, but a recreated container (reboot,
    # the next `up`) needs a fresh one, so `up` fetches it each time.
    if [ -z "$SKRONGLE_MINI_CI_RUNNER_TOKEN" ]; then
      SKRONGLE_MINI_CI_REGISTRATION_TOKEN=$(gh api -X POST "repos/$REPO/actions/runners/registration-token" --jq .token) \
        || { echo "set SKRONGLE_MINI_CI_RUNNER_TOKEN, or sign gh in to $REPO (docs/testing.md, \"CI runners\")" >&2; exit 1; }
      echo "no SKRONGLE_MINI_CI_RUNNER_TOKEN: registering with a one-hour token from gh; the runners won't come back after a reboot until the next 'up'" >&2
    fi
    # Every service names the same image; built per service, the builds race
    # for its tag ("already exists"). Build it once, then start them all.
    compose build runner-1
    # --force-recreate: a container that kept an old (expired) token would otherwise be reused as is.
    compose up -d --no-build --force-recreate ;;
  down) compose down ;;
  status)
    compose ps
    gh api "repos/$REPO/actions/runners" \
      --jq '.runners[] | "\(.name)\t\(.status)\t\(if .busy then "busy" else "idle" end)"' ;;
  logs) compose logs -f ;;
  prune)
    # Runners that stopped without deregistering (an expired token) stay
    # listed as offline; drop them.
    gh api "repos/$REPO/actions/runners" --jq '.runners[] | select(.status == "offline") | "\(.id)\t\(.name)"' |
      while IFS=$'\t' read -r id name; do
        echo "deleting offline runner $name"
        gh api -X DELETE "repos/$REPO/actions/runners/$id"
      done ;;
  reset) compose down -v ;;
  *) sed -n '2,9p' "$0"; exit 1 ;;
esac
