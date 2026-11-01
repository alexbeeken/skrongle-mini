#!/bin/bash
# Runs on the AWS instance, as root, right before the app starts
# (ExecStartPre in learnmap.service). Reads the hosted server's provider
# settings (Google, Stripe, e-mail, texts) from AWS Systems Manager Parameter
# Store and writes them as an env file on tmpfs (/run/learnmap/env), which
# the unit loads next. Nothing secret is on the disk or in the deploy.
#
# Every parameter under AWS_SSM_PATH becomes one line, named like its last
# path element:   /skrongle/learnmap/SKRONGLE_GOOGLE_CLIENT_ID -> SKRONGLE_GOOGLE_CLIENT_ID="…"
# A value of TODO is a placeholder (scripts/aws-env.sh init) and is skipped,
# so the app sees that setting as unset. AWS_SSM_PATH and AWS_REGION come from
# /etc/learnmap/env (written by scripts/deploy-aws.sh); the credentials from
# the instance's IAM role (deploy/aws/cloudformation.yaml, InstanceRole).
set -euo pipefail
: "${AWS_SSM_PATH:?AWS_SSM_PATH is not set (scripts/deploy-aws.sh writes it to /etc/learnmap/env)}"
: "${AWS_REGION:?AWS_REGION is not set (scripts/deploy-aws.sh writes it to /etc/learnmap/env)}"
export AWS_DEFAULT_REGION="$AWS_REGION"
OUT="${LEARNMAP_ENV_OUT:-/run/learnmap/env}"   # overridable to try the script elsewhere
mkdir -p "$(dirname "$OUT")"
TMP=$(mktemp "$OUT.XXXXXX")
trap 'rm -f "$TMP"' EXIT

# name<TAB>value per line. Fails loudly (and so does the service start) when
# the role may not read the path or the network is not up yet; systemd retries.
LIST=$(aws ssm get-parameters-by-path --path "$AWS_SSM_PATH" --with-decryption \
  --query 'Parameters[].[Name,Value]' --output text)

n=0; skipped=0
while IFS=$'\t' read -r name value; do
  key="${name##*/}"
  [ -n "$key" ] || continue
  if ! [[ "$key" =~ ^[A-Z][A-Z0-9_]*$ ]]; then echo "learnmap-env: skipping $name: not an environment variable name" >&2; continue; fi
  if [ "$value" = TODO ]; then skipped=$((skipped + 1)); continue; fi
  # systemd EnvironmentFile syntax: double quotes, with \ and " escaped.
  value="${value//\\/\\\\}"; value="${value//\"/\\\"}"
  printf '%s="%s"\n' "$key" "$value" >> "$TMP"
  n=$((n + 1))
done <<< "$LIST"

chmod 600 "$TMP"
mv -f "$TMP" "$OUT"
trap - EXIT
echo "learnmap-env: $n settings from $AWS_SSM_PATH ($skipped still TODO)"
