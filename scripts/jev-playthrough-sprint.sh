#!/usr/bin/env bash
# Sprint mode playthrough: Jev plays every lesson in Sprint mode as a new account.
# Same options as scripts/jev-playthrough.sh (--lessons, --attempts, --headed, …).
exec "$(dirname "$0")/jev-playthrough.sh" sprint "$@"
