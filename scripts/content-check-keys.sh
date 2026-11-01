# Sourced by scripts/content-check-controls.sh and scripts/moderation-redteam.sh
# (#1061): exports TYPESAFE_API_KEY and SKRONGLE_AI_KEY from the shell,
# ./.env or e2e/.env (SKRONGLE_AI_KEY falls back to ANTHROPIC_API_KEY), or
# stops with a message.
key_from_files() {
  local name="$1" f v
  for f in .env e2e/.env; do
    if [ -f "$f" ]; then
      v=$(sed -n -E "s/^(export )?$name=//p" "$f" | tr -d "\"' " | head -1)
      if [ -n "$v" ]; then
        echo "$v"
        return
      fi
    fi
  done
}
TYPESAFE_API_KEY="${TYPESAFE_API_KEY:-$(key_from_files TYPESAFE_API_KEY)}"
SKRONGLE_AI_KEY="${SKRONGLE_AI_KEY:-${ANTHROPIC_API_KEY:-$(key_from_files SKRONGLE_AI_KEY)}}"
SKRONGLE_AI_KEY="${SKRONGLE_AI_KEY:-$(key_from_files ANTHROPIC_API_KEY)}"
[ -n "$TYPESAFE_API_KEY" ] || { echo "no Jev key: set TYPESAFE_API_KEY (or put it in ./.env or e2e/.env)" >&2; exit 2; }
[ -n "$SKRONGLE_AI_KEY" ] || { echo "no Claude key: set SKRONGLE_AI_KEY or ANTHROPIC_API_KEY (or put one in ./.env or e2e/.env)" >&2; exit 2; }
export TYPESAFE_API_KEY SKRONGLE_AI_KEY
