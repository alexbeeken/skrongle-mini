#!/usr/bin/env bash
# Shared by the deploy scripts: loads deploy/deploy.env (or $DEPLOY_ENV),
# letting variables already set in the environment win, and provides small
# helpers. Source it; do not run it.
#
#   . scripts/deploy-env.sh
#   need_pi_env / need_aws_env   -> exit with a clear message if settings are missing

DEPLOY_ENV="${DEPLOY_ENV:-deploy/deploy.env}"

# A variable set in the environment (exported from the shell, or given on the
# command line) wins over the same variable in the file: snapshot the exported
# variables, source the file, then put the snapshot back.
if [ -f "$DEPLOY_ENV" ]; then
  _deploy_env_before=$(export -p)
  set -a
  # shellcheck disable=SC1090
  . "$DEPLOY_ENV"
  set +a
  eval "$_deploy_env_before"
  unset _deploy_env_before
elif [ -z "${DEPLOY_ENV_OPTIONAL:-}" ]; then
  echo "no $DEPLOY_ENV yet. Create it from the template and fill it in:" >&2
  echo "    cp deploy/deploy.env.example deploy/deploy.env" >&2
  exit 1
fi

# The Pi's e-mail and text message settings (#579, #641): every
# SKRONGLE_MAIL_* and SKRONGLE_SMS_* variable set in deploy.env (or the
# environment), one NAME=value line each, for the env file scripts/deploy.sh
# writes. The AWS target does not use this: its settings come from Parameter
# Store (scripts/aws-env.sh, #914).
#
# For one release (#584) a deploy.env still spelled LEARN_MAIL_* / LEARN_SMS_*
# works too: those lines are written under their SKRONGLE_ name, and a
# SKRONGLE_ one set as well wins. The env file only ever gets SKRONGLE_ names.
mail_env_lines() {
  env | grep -E '^(SKRONGLE|LEARN)_(MAIL|SMS)_[A-Z0-9_]+=' | awk '
    { eq = index($0, "="); key = substr($0, 1, eq - 1); val = substr($0, eq + 1)
      name = key; sub(/^(SKRONGLE|LEARN)_/, "", name)
      if (!(name in v) || key ~ /^SKRONGLE_/) v[name] = val }
    END { for (n in v) print "SKRONGLE_" n "=" v[n] }' | sort || true
}

# Expand a leading ~ in a path setting.
expand_path() { case "$1" in "~"*) echo "$HOME${1#\~}" ;; *) echo "$1" ;; esac; }

# ssh/rsync options for a host, given an optional key path.
ssh_opts_for_key() {
  local key="$1"
  local opts="-o BatchMode=yes -o ConnectTimeout=15"
  if [ -n "$key" ]; then
    key=$(expand_path "$key")
    [ -f "$key" ] || { echo "ssh key not found: $key" >&2; return 1; }
    opts="$opts -i $key -o IdentitiesOnly=yes"
  fi
  echo "$opts"
}

need_pi_env() {
  : "${PI_HOST:?set PI_HOST in $DEPLOY_ENV (user@host)}"
  : "${PI_PUBLIC_URL:?set PI_PUBLIC_URL in $DEPLOY_ENV (the https URL the app is reached at)}"
  : "${PI_BASE_PATH:=/spanish}"
  export PI_HOST PI_PUBLIC_URL PI_BASE_PATH
}

need_aws_cli() {
  command -v aws >/dev/null 2>&1 || { echo "the AWS CLI is not installed: brew install awscli  (https://aws.amazon.com/cli/)" >&2; exit 1; }
}

need_aws_env() {
  : "${AWS_REGION:?set AWS_REGION in $DEPLOY_ENV}"
  if [ -z "${AWS_ACCESS_KEY_ID:-}" ] && [ -z "${AWS_PROFILE:-}" ]; then
    echo "set AWS_ACCESS_KEY_ID + AWS_SECRET_ACCESS_KEY (or AWS_PROFILE) in $DEPLOY_ENV" >&2; exit 1
  fi
  # An empty AWS_PROFILE must not reach the CLI (it would look for a profile named "").
  [ -n "${AWS_PROFILE:-}" ] || unset AWS_PROFILE
  : "${AWS_SSH_KEY:?set AWS_SSH_KEY in $DEPLOY_ENV (path to an ssh private key)}"
  : "${AWS_STACK_NAME:=learnmap}"
  : "${AWS_INSTANCE_TYPE:=t4g.nano}"
  : "${AWS_ADMIN_CIDR:=auto}"
  # Where the hosted server's provider settings live (scripts/aws-env.sh).
  : "${AWS_SSM_PATH:=/skrongle/$AWS_STACK_NAME}"
  export AWS_REGION AWS_DEFAULT_REGION="$AWS_REGION" AWS_STACK_NAME AWS_INSTANCE_TYPE AWS_ADMIN_CIDR AWS_SSM_PATH
}
