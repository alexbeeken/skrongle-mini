#!/usr/bin/env bash
# The hosted server's provider settings (Google, Stripe, e-mail, texts, Jev) live in
# AWS Systems Manager Parameter Store, one SecureString per setting, named like
# the environment variable it becomes, under AWS_SSM_PATH (deploy/deploy.env;
# default /skrongle/<AWS_STACK_NAME>). The instance reads them at start
# (deploy/aws/learnmap-env.sh); nothing secret is in deploy.env or on its disk.
#
#   scripts/aws-env.sh init                 # create every setting the hosted server needs, as TODO placeholders (keeps existing values)
#   scripts/aws-env.sh set NAME [VALUE]     # set one (no VALUE: read it from stdin, hidden on a terminal)
#   scripts/aws-env.sh set NAME TODO        # back to a placeholder: the app sees the setting as unset
#   scripts/aws-env.sh list                 # every setting at the path: name, set or TODO, when it changed
#   scripts/aws-env.sh check                # exit 1 if a required setting is missing or TODO (deploy-aws.sh runs this)
#   scripts/aws-env.sh delete NAME
#   scripts/aws-env.sh migrate              # rename settings still stored under an old LEARN_ / SPANISH_ name (#584; deploy-aws.sh runs this)
#
# A placeholder's value is the word TODO; the instance skips those, so a
# half-filled provider (say, Stripe without a price id) is simply off until
# every one of its settings is set. Which settings a provider needs:
# docs/architecture.md, "Configuration". Required by the hosted server
# (it refuses to start without them, src/main.rs require_providers): REQUIRED below.
set -euo pipefail
cd "$(dirname "$0")/.."
. scripts/deploy-env.sh
need_aws_env
need_aws_cli

# The ones the hosted server refuses to start without, plus what each needs to
# work, plus SKRONGLE_SECRET_KEY: the app would make itself a key file on the
# disk, but backups hold only the database, so the deploy insists on this one (#964).
# The two contact addresses too: without them /terms shows placeholders (#1328).
REQUIRED=(
  SKRONGLE_SECRET_KEY
  SKRONGLE_MAIL_FROM SKRONGLE_MAIL_PROVIDERS
  SKRONGLE_SMS_PROVIDERS
  SKRONGLE_GOOGLE_CLIENT_ID SKRONGLE_GOOGLE_CLIENT_SECRET
  SKRONGLE_STRIPE_SECRET_KEY SKRONGLE_STRIPE_WEBHOOK_SECRET SKRONGLE_STRIPE_PRICE_ID SKRONGLE_STRIPE_PRICE_ID_BASIC
  TYPESAFE_API_KEY
  SKRONGLE_LEGAL_EMAIL SKRONGLE_SUPPORT_EMAIL
)
# Optional ones `init` creates as well, so they can be filled in from the console.
OPTIONAL=(SKRONGLE_STRIPE_PRICE_LABEL SKRONGLE_MAIL_PER_HOUR SKRONGLE_SMS_FROM)

describe() {
  case "$1" in
    SKRONGLE_SECRET_KEY) echo 'Encrypts the AI keys learners save: 64 hex characters (openssl rand -hex 32). Never change it once set; keep a copy in your password manager' ;;
    SKRONGLE_MAIL_FROM) echo 'Sender of every e-mail, e.g. "Skrongle <noreply@example.org>"' ;;
    SKRONGLE_MAIL_PROVIDERS) echo 'E-mail providers in fallback order (smtp, resend, postmark, brevo); each needs its SKRONGLE_MAIL_<NAME>_* settings' ;;
    SKRONGLE_MAIL_PER_HOUR) echo 'E-mails of one purpose per hour (default 3000; 0 = no ceiling)' ;;
    SKRONGLE_SMS_PROVIDERS) echo 'Text message providers in fallback order; each needs its SKRONGLE_SMS_<NAME>_* settings' ;;
    SKRONGLE_SMS_FROM) echo 'Sender number for text messages, E.164' ;;
    SKRONGLE_GOOGLE_CLIENT_ID) echo 'Google OAuth client id (Google Cloud console, APIs & Services, Credentials)' ;;
    SKRONGLE_GOOGLE_CLIENT_SECRET) echo 'Google OAuth client secret' ;;
    SKRONGLE_STRIPE_SECRET_KEY) echo 'Stripe secret key (sk_live_… or sk_test_…); docs/features.md "Setting up Stripe"' ;;
    SKRONGLE_STRIPE_WEBHOOK_SECRET) echo 'Signing secret (whsec_…) of the webhook endpoint <public URL>/billing/webhook' ;;
    SKRONGLE_STRIPE_PRICE_ID) echo 'Stripe price id (price_…) of the Plus monthly subscription (CA$15)' ;;
    SKRONGLE_STRIPE_PRICE_ID_BASIC) echo 'Stripe price id (price_…) of the Basic monthly subscription (CA$3); the webhook tells the plans apart by price, so it must differ from SKRONGLE_STRIPE_PRICE_ID' ;;
    SKRONGLE_LEGAL_EMAIL) echo 'Privacy, copyright and legal contact shown on /terms and to suspended accounts: privacy@skrongle.com' ;;
    SKRONGLE_SUPPORT_EMAIL) echo 'Help address shown on /terms and the Reply-To of every e-mail: support@skrongle.com' ;;
    TYPESAFE_API_KEY) echo 'Jev (TypeSafe) key for the AI content check; console.typesafe.ai/keys' ;;
    SKRONGLE_STRIPE_PRICE_LABEL) echo 'What the Subscribe button says it costs, e.g. "CA$15 a month"' ;;
    *) echo "Setting $1 of the Skrongle hosted server" ;;
  esac
}

valid_name() { [[ "$1" =~ ^[A-Z][A-Z0-9_]*$ ]] || { echo "not an environment variable name: $1" >&2; exit 1; }; }

put() {  # NAME VALUE [--no-overwrite]
  local args=(ssm put-parameter --name "$AWS_SSM_PATH/$1" --type SecureString --value "$2" --description "$(describe "$1")")
  [ "${3:-}" = --no-overwrite ] && args+=(--no-overwrite) || args+=(--overwrite)
  aws "${args[@]}" --output text --query Version >/dev/null
}

# name<TAB>value<TAB>last modified, one per line (values decrypted).
fetch() {
  aws ssm get-parameters-by-path --path "$AWS_SSM_PATH" --with-decryption \
    --query 'Parameters[].[Name,Value,LastModifiedDate]' --output text
}

CMD="${1:-}"; shift || true
case "$CMD" in
  init)
    echo "==> placeholders under $AWS_SSM_PATH ($AWS_REGION); existing values are kept"
    for name in "${REQUIRED[@]}" "${OPTIONAL[@]}"; do
      if put "$name" TODO --no-overwrite 2>/dev/null; then echo "    created $name (TODO)"; else echo "    kept    $name"; fi
    done
    echo "Fill them in:  scripts/aws-env.sh set NAME   (or the AWS console: Systems Manager, Parameter Store)"
    ;;
  set)
    NAME="${1:?usage: aws-env.sh set NAME [VALUE]}"; valid_name "$NAME"
    if [ $# -ge 2 ]; then VALUE="$2"
    elif [ -t 0 ]; then read -r -s -p "$NAME: " VALUE; echo
    else VALUE=$(cat); VALUE="${VALUE%$'\n'}"
    fi
    [ -n "$VALUE" ] || { echo "empty value; to unset a setting use:  aws-env.sh set $NAME TODO" >&2; exit 1; }
    put "$NAME" "$VALUE"
    echo "set $AWS_SSM_PATH/$NAME$([ "$VALUE" = TODO ] && echo ' (placeholder)')"
    ;;
  list)
    echo "settings under $AWS_SSM_PATH ($AWS_REGION):"
    fetch | sort | while IFS=$'\t' read -r name value when; do
      [ -n "$name" ] || continue
      printf '    %-32s %-5s %s\n' "${name##*/}" "$([ "$value" = TODO ] && echo TODO || echo set)" "${when%%.*}"
    done
    ;;
  check)
    missing=()
    found=$(fetch || true)
    for name in "${REQUIRED[@]}"; do
      value=$(printf '%s\n' "$found" | awk -F'\t' -v n="$AWS_SSM_PATH/$name" '$1 == n { print $2 }')
      [ -n "$value" ] && [ "$value" != TODO ] || missing+=("$name")
    done
    if [ ${#missing[@]} -gt 0 ]; then
      echo "the deploy refuses to go ahead without these; set them in Parameter Store at $AWS_SSM_PATH:" >&2
      for m in "${missing[@]}"; do echo "    scripts/aws-env.sh set $m" >&2; done
      exit 1
    fi
    echo "every required setting is set under $AWS_SSM_PATH"
    ;;
  migrate)
    # For one release (#584) the app still reads LEARN_<name> and SPANISH_<name>,
    # so a server whose parameters predate the rename keeps working, with a
    # warning. This moves each to SKRONGLE_<name>: copy, then delete the old one.
    # A SKRONGLE_ one that is already set (not TODO) wins and is kept.
    found=$(fetch || true)
    moved=0
    while IFS=$'\t' read -r path value _; do
      old="${path##*/}"
      case "$old" in LEARN_?*|SPANISH_?*) ;; *) continue ;; esac
      new="SKRONGLE_${old#*_}"
      have=$(printf '%s\n' "$found" | awk -F'\t' -v n="$AWS_SSM_PATH/$new" '$1 == n { print $2 }')
      if [ -n "$have" ] && [ "$have" != TODO ]; then
        echo "    kept    $new (already set; dropping $old)"
      else
        put "$new" "$value"
        echo "    renamed $old -> $new"
      fi
      aws ssm delete-parameter --name "$path"
      moved=$((moved + 1))
    # SPANISH_ before LEARN_ (reverse order), so a LEARN_ value wins as it does in the app.
    done <<< "$(printf '%s\n' "$found" | sort -r)"
    [ "$moved" -gt 0 ] || echo "no settings with an old name under $AWS_SSM_PATH"
    ;;
  delete)
    NAME="${1:?usage: aws-env.sh delete NAME}"; valid_name "$NAME"
    aws ssm delete-parameter --name "$AWS_SSM_PATH/$NAME"
    echo "deleted $AWS_SSM_PATH/$NAME"
    ;;
  *) sed -n '2,20p' "$0"; exit 1 ;;
esac
