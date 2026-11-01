#!/usr/bin/env bash
# Play mode playthrough: Jev plays every lesson in Play mode as a new account.
# Same options as scripts/jev-playthrough.sh (--lessons, --attempts, --headed, …).
exec "$(dirname "$0")/jev-playthrough.sh" play "$@"
