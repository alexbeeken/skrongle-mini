#!/usr/bin/env bash
# Deploy to AWS: one t4g.nano behind Caddy (automatic HTTPS), created with
# CloudFormation on first run and reused afterwards. Settings come from
# deploy/deploy.env (see deploy/deploy.env.example for every variable); the
# app's provider secrets (Google, Stripe, e-mail, texts) from Parameter Store
# (scripts/aws-env.sh), which the instance reads itself at every start.
#
#   scripts/deploy-aws.sh                # build (aarch64) + create/update stack + rsync + restart + health check
#   scripts/deploy-aws.sh --skip-build   # use the binary already in dist/
#   scripts/deploy-aws.sh --with-backup  # back up the database before this release even if it doesn't migrate
#   scripts/deploy-aws.sh --status       # print the stack's IP / URL / latest backup / last releases
#   scripts/deploy-aws.sh --backup       # back up the database to the stack's S3 bucket now
#   scripts/deploy-aws.sh --restore [WHAT]  # replace the database with a backup: the latest
#                                        # one, a date (2026-10-27) or an s3://bucket/key
#   scripts/deploy-aws.sh --rollback     # put the previous release back (when the last one didn't migrate)
#   scripts/deploy-aws.sh --rollback --restore  # same, plus the backup taken before the last release
#   scripts/deploy-aws.sh --destroy      # back up, then delete the stack (instance + disk + database!)
#
# Re-running is safe: the stack is updated in place, the binary is swapped
# atomically, the database on the instance is never touched. A release that
# migrates the database is backed up first. The previous release stays on the
# instance for --rollback (#1329). Backups and the restore runbook:
# docs/aws-setup.md, "Backups" and "Rolling back".
set -euo pipefail
cd "$(dirname "$0")/.."
. scripts/deploy-env.sh
need_aws_env
need_aws_cli

SKIP_BUILD=0; MODE=deploy; RESTORE_FROM=; BACKUP_EVERY=0; ROLLBACK_RESTORE=0
for a in "$@"; do
  case "$a" in
    --skip-build) SKIP_BUILD=1 ;;
    --with-backup) BACKUP_EVERY=1 ;;
    --status) MODE=status ;;
    --backup) MODE=backup ;;
    --restore) if [ "$MODE" = rollback ]; then ROLLBACK_RESTORE=1; else MODE=restore; fi ;;
    --rollback) if [ "$MODE" = restore ] && [ -z "$RESTORE_FROM" ]; then ROLLBACK_RESTORE=1; fi; MODE=rollback ;;
    --destroy) MODE=destroy ;;
    -h|--help) sed -n '2,23p' "$0"; exit 0 ;;
    *) if [ "$MODE" = restore ] && [ -z "$RESTORE_FROM" ]; then RESTORE_FROM="$a"; else echo "unknown flag: $a"; exit 1; fi ;;
  esac
done

TEMPLATE=deploy/aws/cloudformation.yaml
TARGET=aarch64-unknown-linux-gnu
BIN="dist/$TARGET/spanish-learning"
SSH_KEY=$(expand_path "$AWS_SSH_KEY")
[ -f "$SSH_KEY" ] || { echo "AWS_SSH_KEY not found: $SSH_KEY  (ssh-keygen -t ed25519 to make one)"; exit 1; }

stack_output() {
  aws cloudformation describe-stacks --stack-name "$AWS_STACK_NAME" \
    --query "Stacks[0].Outputs[?OutputKey=='$1'].OutputValue" --output text 2>/dev/null || true
}
stack_ip() { stack_output PublicIp; }

# The instance's ssh host key is pinned (#391): the deploy runs commands as root
# on the box, so it must not talk to whoever answers on the IP. The pin lives
# in deploy/aws/known_hosts.<instance-id> (git-ignored; a replaced instance gets
# a new file) and is taken once, either from the instance's boot console via the
# authenticated AWS API (cloud-init prints its host keys there at the end of
# first boot, and the key's fingerprint as it makes it; with only the
# fingerprint, ssh-keyscan's key must match it) or, when
# AWS_SSH_HOST_KEY_FINGERPRINT is set (SHA256:…, from `ssh-keygen -lf
# /etc/ssh/ssh_host_ed25519_key.pub` on the instance), from ssh-keyscan checked
# against that fingerprint. After that, ssh runs with StrictHostKeyChecking=yes.
KNOWN_HOSTS_DIR=deploy/aws
set_ssh() {  # $1 = instance id
  KNOWN_HOSTS="$KNOWN_HOSTS_DIR/known_hosts.$1"
  SSH="ssh -i $SSH_KEY -o IdentitiesOnly=yes -o StrictHostKeyChecking=yes -o UserKnownHostsFile=$KNOWN_HOSTS -o LogLevel=ERROR -o ConnectTimeout=15"
}

# The ed25519 key ssh-keyscan gets from $1, in SCANNED, if its fingerprint is
# $2 ($3 says where that fingerprint came from). Returns 1 while ssh does not
# answer yet; a different key stops the deploy.
keyscan_matching() {
  local fp
  SCANNED=$(ssh-keyscan -T 10 -t ed25519 "$1" 2>/dev/null | grep -v '^#' || true)
  [ -n "$SCANNED" ] || return 1
  fp=$(printf '%s\n' "$SCANNED" | ssh-keygen -lf - | awk '{print $2}')
  if [ "$fp" != "$2" ]; then
    echo "host key of $1 is $fp, not $2 from $3: refusing to deploy" >&2
    exit 1
  fi
}

# The ed25519 line of cloud-init's SSH HOST KEY KEYS block in the console
# output on stdin, as a known_hosts line for $1. Empty when the block is missing.
console_host_key() {
  sed -n '/-----BEGIN SSH HOST KEY KEYS-----/,/-----END SSH HOST KEY KEYS-----/p' \
    | awk -v ip="$1" '$1 == "ssh-ed25519" { print ip, $1, $2 }'
}

# The SHA256 fingerprint cloud-init printed in the console output on stdin
# when it generated the ed25519 host key ("Generating public/private ed25519
# key pair", then "The key fingerprint is:" and the SHA256 line).
console_key_fingerprint() {
  awk '
    /Generating public\/private / { ed = ($0 ~ /ed25519/); next_is_fp = 0; next }
    ed && /The key fingerprint is:/ { next_is_fp = 1; next }
    next_is_fp {
      for (i = 1; i <= NF; i++) if ($i ~ /^SHA256:/) { print $i; exit }
      next_is_fp = 0
    }'
}

# Pin the host key of $1 (IP) for instance $2 unless it already is. Returns 1
# while the key is not available yet (the console output lags the boot).
pin_host_key() {
  local ip="$1" id="$2" keys fp console
  [ -s "$KNOWN_HOSTS" ] && ssh-keygen -F "$ip" -f "$KNOWN_HOSTS" >/dev/null 2>&1 && return 0
  if [ -n "${AWS_SSH_HOST_KEY_FINGERPRINT:-}" ]; then
    keyscan_matching "$ip" "$AWS_SSH_HOST_KEY_FINGERPRINT" AWS_SSH_HOST_KEY_FINGERPRINT || return 1
    keys=$SCANNED
  else
    console=$(aws ec2 get-console-output --instance-id "$id" --latest --query Output --output text 2>/dev/null | tr -d '\r' || true)
    keys=$(printf '%s\n' "$console" | console_host_key "$ip")
    if [ -z "$keys" ]; then
      # cloud-init prints that block only at the end of first boot, so it is
      # missing while first boot runs and never comes if it fails (#1362).
      # The fingerprint printed when the key was generated is there sooner.
      fp=$(printf '%s\n' "$console" | console_key_fingerprint)
      [ -n "$fp" ] || return 1
      keyscan_matching "$ip" "$fp" "the boot console" || return 1
      keys=$SCANNED
    fi
  fi
  mkdir -p "$KNOWN_HOSTS_DIR"
  printf '%s\n' "$keys" > "$KNOWN_HOSTS"
  chmod 600 "$KNOWN_HOSTS"
  echo "    pinned the ssh host key of $ip in $KNOWN_HOSTS"
}

host_for_ip() {
  if [ -n "${AWS_DOMAIN:-}" ]; then echo "$AWS_DOMAIN"; else echo "${1//./-}.sslip.io"; fi
}

set_value() { [ -n "$1" ] && [ "$1" != None ]; }

# Sets IP, INSTANCE_ID, BUCKET and the ssh command for the running stack.
# Returns 1 when the stack has no instance (it does not exist, or its first
# creation failed).
find_stack() {
  IP=$(stack_ip); INSTANCE_ID=$(stack_output InstanceId); BUCKET=$(stack_output BackupBucket)
  set_value "$IP" && set_value "$INSTANCE_ID" || return 1
  set_ssh "$INSTANCE_ID"
}

# The newest backup in the stack's bucket, as s3://bucket/key (empty: none yet).
latest_backup() {
  set_value "$BUCKET" || return 0
  local key
  key=$(aws s3api list-objects-v2 --bucket "$BUCKET" --prefix daily/ --query 'Contents[].Key' --output text 2>/dev/null \
    | tr '\t' '\n' | grep -E '^daily/[0-9]{4}-[0-9]{2}-[0-9]{2}\.db\.gz$' | sort | tail -1 || true)
  [ -z "$key" ] || echo "s3://$BUCKET/$key"
}

# Backs up the database now (learnmap-backup.service on the instance), then
# checks from here that today's copy is in the bucket. Returns 1 if not.
take_backup() {
  if ! set_value "$BUCKET"; then
    echo "the stack has no backup bucket yet; run scripts/deploy-aws.sh once to add it" >&2
    return 1
  fi
  local key; key="daily/$(date -u +%F).db.gz"
  echo "==> backing up the database to s3://$BUCKET/$key"
  if ! $SSH "ec2-user@$IP" 'sudo systemctl start learnmap-backup'; then
    $SSH "ec2-user@$IP" 'sudo journalctl -u learnmap-backup -n 20 --no-pager' || true
    return 1
  fi
  aws s3api head-object --bucket "$BUCKET" --key "$key" \
    --query '[LastModified, ContentLength]' --output text | awk '{ print "    in the bucket: " $1 ", " $2 " bytes" }'
}

# Releases on the instance (#1329). /opt/learnmap/release describes the live
# one and release.prev the one before it, which learnmap.prev and data.prev
# hold; one line each: "<UTC time> <git sha> <schema before> <schema after>
# <backup taken before it, or ->". releases.log keeps one line per deploy or
# rollback: "<UTC time> deploy|rollback <git sha> <before> <after> <backup>".
RELEASES=/opt/learnmap/releases.log

show_releases() {  # $1 = how many
  echo "releases (time, what, git sha, schema before, after, backup taken before it):"
  $SSH -n "ec2-user@$IP" "sudo tail -n $1 $RELEASES 2>/dev/null | sed 's/^/    /'" || true
}

# The app's health check on the instance. Prints the log and returns 1 if not 200.
check_health() {
  local code
  code=$($SSH "ec2-user@$IP" 'curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:8080/healthz' || echo 000)
  echo "    app on the instance: HTTP $code"
  [ "$code" = 200 ] && return 0
  echo "    app is not healthy; recent log:"
  $SSH "ec2-user@$IP" 'sudo journalctl -u learnmap -n 20 --no-pager' || true
  return 1
}

# Runs on the instance for --restore and --rollback. With URL set it downloads
# that backup and checks it before anything stops (with MAX_SCHEMA set, its
# schema must not be newer than that). Then it stops the app, puts the
# previous release back if ROLLBACK=1 (logging LOG), swaps the database in if
# there is one, and starts the app again.
# shellcheck disable=SC2016
REMOTE_SWAP='
set -euo pipefail
L=/opt/learnmap
D=/var/lib/learnmap
T=$(date -u +%Y%m%dT%H%M%SZ)
if [ -n "${URL:-}" ]; then
  command -v sqlite3 >/dev/null 2>&1 || sudo dnf install -y -q sqlite
  sudo rm -f "$D/restore.db" "$D/restore.db.gz"
  sudo -u learnmap curl -fsS "$URL" -o "$D/restore.db.gz"
  sudo -u learnmap gunzip "$D/restore.db.gz" || { echo "the backup is not a whole gzip file"; sudo rm -f "$D/restore.db" "$D/restore.db.gz"; exit 1; }
  CHECK=$(sudo -u learnmap sqlite3 "$D/restore.db" "PRAGMA quick_check;" 2>&1 || true)
  [ "$CHECK" = ok ] || { echo "the backup fails quick_check: $CHECK"; sudo rm -f "$D/restore.db"; exit 1; }
  if [ -n "${MAX_SCHEMA:-}" ]; then
    V=$(sudo -u learnmap sqlite3 "$D/restore.db" "PRAGMA user_version;")
    [ "$V" -le "$MAX_SCHEMA" ] || { echo "the backup is at schema v$V, newer than the previous release (v$MAX_SCHEMA)"; sudo rm -f "$D/restore.db"; exit 1; }
  fi
fi
sudo systemctl stop learnmap
if [ "${ROLLBACK:-0}" = 1 ]; then
  sudo mv -f "$L/learnmap.prev" "$L/learnmap"
  sudo rm -rf "$L/data.rolled-back"
  sudo mv "$L/data" "$L/data.rolled-back"
  sudo mv "$L/data.prev" "$L/data"
  sudo rm -rf "$L/data.rolled-back"
  sudo mv -f "$L/release.prev" "$L/release"
  echo "$(date -u +%FT%TZ) $LOG" | sudo tee -a "$L/releases.log" >/dev/null
fi
if [ -n "${URL:-}" ]; then
  for f in learn.db learn.db-wal learn.db-shm; do
    if sudo test -e "$D/$f"; then sudo mv "$D/$f" "$D/$f.before-restore-$T"; fi
  done
  sudo mv "$D/restore.db" "$D/learn.db"
  echo "    previous database: $D/learn.db.before-restore-$T"
fi
sudo systemctl start learnmap
sleep 2
'

if [ "$MODE" = status ]; then
  find_stack || { echo "stack $AWS_STACK_NAME does not exist in $AWS_REGION"; exit 1; }
  echo "stack:  $AWS_STACK_NAME ($AWS_REGION)"
  echo "ip:     $IP"
  echo "url:    https://$(host_for_ip "$IP")"
  echo "ssh:    ssh -i $SSH_KEY -o StrictHostKeyChecking=yes -o UserKnownHostsFile=$KNOWN_HOSTS_DIR/known_hosts.$INSTANCE_ID ec2-user@$IP"
  echo "backup: $(latest_backup || true)"
  show_releases 5
  exit 0
fi

if [ "$MODE" = backup ]; then
  find_stack || { echo "stack $AWS_STACK_NAME has no instance in $AWS_REGION"; exit 1; }
  take_backup
  exit 0
fi

if [ "$MODE" = restore ]; then
  find_stack || { echo "stack $AWS_STACK_NAME has no instance in $AWS_REGION; deploy first"; exit 1; }
  case "$RESTORE_FROM" in
    "") SRC=$(latest_backup) ;;
    s3://*) SRC="$RESTORE_FROM" ;;
    [0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]) set_value "$BUCKET" && SRC="s3://$BUCKET/daily/$RESTORE_FROM.db.gz" ;;
    *) echo "--restore takes a date (2026-10-27) or an s3:// address, not: $RESTORE_FROM"; exit 1 ;;
  esac
  [ -n "${SRC:-}" ] || { echo "no backup to restore: the stack's bucket ($BUCKET) has none yet"; exit 1; }
  aws s3 ls "$SRC" >/dev/null || { echo "not found: $SRC"; exit 1; }
  echo "This replaces the database of $AWS_STACK_NAME with $SRC."
  echo "The app stops for a few seconds; the current database stays on the instance as learn.db.before-restore-<time>."
  read -r -p "Type the stack name to confirm: " ANSWER
  [ "$ANSWER" = "$AWS_STACK_NAME" ] || { echo "not confirmed"; exit 1; }
  # A link that works for 15 minutes: the instance downloads the backup itself
  # (its role may not read the bucket), and the database never lands on this Mac.
  URL=$(aws s3 presign "$SRC" --expires-in 900)
  echo "==> restoring"
  $SSH "ec2-user@$IP" "URL='$URL' bash -s" <<<"$REMOTE_SWAP"
  check_health || exit 1
  echo "==> restored $SRC; sign in and check your account"
  exit 0
fi

if [ "$MODE" = rollback ]; then
  find_stack || { echo "stack $AWS_STACK_NAME has no instance in $AWS_REGION"; exit 1; }
  # Three lines: the live release, the previous one, the database's schema
  # now and the minutes since the live release went out.
  # shellcheck disable=SC2016
  STATE=$($SSH -n "ec2-user@$IP" 'L=/opt/learnmap
    sudo test -x $L/learnmap.prev && sudo test -f $L/release.prev && sudo test -f $L/release || exit 3
    sudo cat $L/release $L/release.prev
    T=$(cut -d" " -f1 $L/release)
    echo "$(sudo -u learnmap sqlite3 -readonly /var/lib/learnmap/learn.db "PRAGMA user_version;" 2>/dev/null || echo 0) $(( ($(date +%s) - $(date -d "$T" +%s)) / 60 ))"') || {
    echo "no previous release on the instance to roll back to (only a release deployed by this script keeps one;"
    echo "after a rollback there is none until the next deploy)"
    exit 1
  }
  read -r CUR_TIME CUR_SHA CUR_BEFORE CUR_AFTER CUR_BACKUP <<<"$(sed -n 1p <<<"$STATE")"
  read -r _ PREV_SHA _ _ _ <<<"$(sed -n 2p <<<"$STATE")"
  read -r LIVE AGE_MIN <<<"$(sed -n 3p <<<"$STATE")"
  echo "live:     $CUR_SHA, out since $CUR_TIME (schema v$CUR_BEFORE to v$CUR_AFTER; the database is at v$LIVE)"
  echo "previous: $PREV_SHA (schema v$CUR_BEFORE)"
  URL=; MAX_SCHEMA=; RESTORED=-
  if [ "$LIVE" -le "$CUR_BEFORE" ]; then
    [ "$ROLLBACK_RESTORE" = 0 ] || echo "    the database is at v$LIVE, which the previous release knows: no restore needed, nothing is lost"
    echo "This puts $PREV_SHA back. The database stays as it is."
  else
    if [ "$CUR_BACKUP" = - ]; then
      echo "Release $CUR_SHA moved the database from v$CUR_BEFORE to v$LIVE and no backup was recorded before it."
      echo "The previous release can't run on this database. Pick a backup by hand: scripts/deploy-aws.sh --status, then"
      echo "docs/aws-setup.md, \"Rolling back\"."
      exit 1
    fi
    if [ "$ROLLBACK_RESTORE" = 0 ]; then
      echo "Release $CUR_SHA moved the database from v$CUR_BEFORE to v$LIVE. The previous release can't run on it."
      echo "Rolling back means restoring the backup taken just before that release:"
      echo "    $CUR_BACKUP"
      echo "Everything learners did in the last $((AGE_MIN / 60)) h $((AGE_MIN % 60)) min (since $CUR_TIME) would be lost."
      echo "To go ahead: scripts/deploy-aws.sh --rollback --restore"
      exit 1
    fi
    aws s3 ls "$CUR_BACKUP" >/dev/null || { echo "not found: $CUR_BACKUP"; exit 1; }
    echo "This puts $PREV_SHA back and replaces the database with $CUR_BACKUP."
    echo "Everything learners did in the last $((AGE_MIN / 60)) h $((AGE_MIN % 60)) min (since $CUR_TIME) is lost;"
    echo "the current database stays on the instance as learn.db.before-restore-<time>."
    URL=$(aws s3 presign "$CUR_BACKUP" --expires-in 900)
    MAX_SCHEMA=$CUR_BEFORE; RESTORED=$CUR_BACKUP
  fi
  read -r -p "Type the stack name to confirm: " ANSWER
  [ "$ANSWER" = "$AWS_STACK_NAME" ] || { echo "not confirmed"; exit 1; }
  echo "==> rolling back to $PREV_SHA"
  AFTER=$LIVE; [ -z "$URL" ] || AFTER=$CUR_BEFORE
  $SSH "ec2-user@$IP" "URL='$URL' MAX_SCHEMA='$MAX_SCHEMA' ROLLBACK=1 LOG='rollback $PREV_SHA $LIVE $AFTER $RESTORED' bash -s" <<<"$REMOTE_SWAP"
  check_health || exit 1
  echo "==> live: $PREV_SHA$([ -z "$URL" ] || echo ", database from $CUR_BACKUP")"
  show_releases 3
  exit 0
fi

if [ "$MODE" = destroy ]; then
  echo "This deletes the stack $AWS_STACK_NAME in $AWS_REGION: the instance, its disk and the database."
  # A fresh backup first, or no delete. A stack whose first creation failed
  # has no instance and nothing to back up.
  if find_stack; then
    if ! take_backup; then
      echo "no backup, so no delete. Fix the backup (scripts/deploy-aws.sh --backup), or delete the stack" >&2
      echo "in the CloudFormation console if its database really is not needed." >&2
      exit 1
    fi
    echo "    the backup bucket $BUCKET is kept after the delete; restore from it with"
    echo "    scripts/deploy-aws.sh --restore $(latest_backup)"
  fi
  read -r -p "Type the stack name to confirm: " ANSWER
  [ "$ANSWER" = "$AWS_STACK_NAME" ] || { echo "not confirmed"; exit 1; }
  OLD_ID=$(stack_output InstanceId)
  aws cloudformation delete-stack --stack-name "$AWS_STACK_NAME"
  echo "==> deleting; waiting…"
  aws cloudformation wait stack-delete-complete --stack-name "$AWS_STACK_NAME"
  [ -z "$OLD_ID" ] || [ "$OLD_ID" = None ] || rm -f "$KNOWN_HOSTS_DIR/known_hosts.$OLD_ID"
  echo "==> gone. Its disk snapshots stay until you delete them (EC2 console, Snapshots, tag learnmap-snapshot-of=$AWS_STACK_NAME)."
  exit 0
fi

# ---- 0. settings ----
# The hosted server refuses to start without its providers; find out here, not
# at the health check. Provider values in deploy.env are a leftover from before
# #914 and are not shipped. Settings stored under their old LEARN_ name are
# renamed first (#584), so an older server migrates itself.
scripts/aws-env.sh migrate
scripts/aws-env.sh check
# The secret key that encrypts saved AI keys lives in Parameter Store, so a
# backup of the database alone restores everything. On an instance that ran
# before it moved there, the key is still a file next to the database; it must
# be the same key, or every saved AI key becomes unreadable.
KEY_SHA=$(aws ssm get-parameter --name "$AWS_SSM_PATH/SKRONGLE_SECRET_KEY" --with-decryption \
  --query Parameter.Value --output text | tr -d '[:space:]' | tr 'A-F' 'a-f' | shasum -a 256 | cut -d' ' -f1)
LEFTOVER=$(env | grep -oE '^(GOOGLE_CLIENT|STRIPE_|LEARN_MAIL_|LEARN_SMS_|SKRONGLE_MAIL_|SKRONGLE_SMS_)[A-Z0-9_]*' | sort | tr '\n' ' ' || true)
[ -z "$LEFTOVER" ] || echo "    note: $LEFTOVER in $DEPLOY_ENV are ignored on AWS; the settings come from Parameter Store ($AWS_SSM_PATH)"

# ---- 1. build ----
if [ "$SKIP_BUILD" = 0 ] || [ ! -x "$BIN" ]; then
  echo "==> [1/5] cross-compiling for aarch64 (release profile)"
  scripts/build-pi.sh aarch64 2>&1 | grep -vE '^\s+Compiling|linker stderr|^\s+\||= note|generated [0-9]+ warning' || true
else
  echo "==> [1/5] build skipped (using $BIN)"
fi
[ -x "$BIN" ] || { echo "no binary at $BIN"; exit 1; }
# Names the release in the instance's release log (with --skip-build, the
# binary in dist/ may be from an older commit than this).
SHA=$(git rev-parse --short=12 HEAD)
[ -z "$(git status --porcelain --untracked-files=no)" ] || SHA="$SHA-dirty"

# ---- 2. stack ----
echo "==> [2/5] creating/updating CloudFormation stack $AWS_STACK_NAME in $AWS_REGION"
PUBKEY=$(ssh-keygen -y -f "$SSH_KEY")
if [ "$AWS_ADMIN_CIDR" = auto ]; then
  MYIP=$(curl -s --max-time 10 https://checkip.amazonaws.com | tr -d '[:space:]')
  [[ "$MYIP" =~ ^[0-9.]+$ ]] || { echo "could not find your public IP for the ssh rule; set AWS_ADMIN_CIDR"; exit 1; }
  ADMIN_CIDR="$MYIP/32"
else
  ADMIN_CIDR="$AWS_ADMIN_CIDR"
fi
aws cloudformation deploy \
  --stack-name "$AWS_STACK_NAME" \
  --template-file "$TEMPLATE" \
  --no-fail-on-empty-changeset \
  --capabilities CAPABILITY_IAM \
  --parameter-overrides "InstanceType=$AWS_INSTANCE_TYPE" "SshPublicKey=$PUBKEY" "AdminCidr=$ADMIN_CIDR" "SettingsPath=$AWS_SSM_PATH"
IP=$(stack_ip)
[ -n "$IP" ] && [ "$IP" != None ] || { echo "stack has no PublicIp output"; exit 1; }
INSTANCE_ID=$(stack_output InstanceId)
[ -n "$INSTANCE_ID" ] && [ "$INSTANCE_ID" != None ] || { echo "stack has no InstanceId output"; exit 1; }
set_ssh "$INSTANCE_ID"
HOST=$(host_for_ip "$IP")
PUBLIC_URL="https://$HOST"
echo "    instance ip $IP · site $PUBLIC_URL"

# ---- 3. wait for ssh (first boot makes swap and installs caddy) ----
echo "==> [3/5] waiting for the instance"
# ready: first boot finished. failed: cloud-init ended (or was killed) without
# the marker. Anything else: still running, or ssh does not answer yet.
# /var/lib/learnmap is learnmap's (0750), so ec2-user needs sudo to look in it.
FIRSTBOOT_STATE='if sudo test -f /var/lib/learnmap/.firstboot-done; then echo ready
elif systemctl is-failed --quiet cloud-final.service || cloud-init status 2>/dev/null | grep -qE "^status: (done|error|degraded)"; then echo failed
else echo running; fi'
firstboot_failed() {
  echo "first boot of $INSTANCE_ID did not finish:" >&2
  $SSH "ec2-user@$IP" 'cloud-init status --long 2>&1 | head -8
    systemctl --no-pager --lines=0 status cloud-final.service 2>&1 | sed -n 3p
    sudo journalctl -k --no-pager 2>/dev/null | grep -i "out of memory" | tail -3
    echo "--- last lines of /var/log/learnmap-firstboot.log:"
    sudo tail -n 15 /var/log/learnmap-firstboot.log' >&2 || true
  echo "Fix the cause, then run first boot again on the instance (sudo bash /var/lib/cloud/instance/scripts/part-001)" >&2
  echo "and deploy again, or delete the stack (--destroy) and deploy a fresh one." >&2
  exit 1
}
STATE=
for i in $(seq 1 40); do
  if pin_host_key "$IP" "$INSTANCE_ID"; then
    STATE=$($SSH "ec2-user@$IP" "$FIRSTBOOT_STATE" 2>/dev/null || true)
    [ "$STATE" = ready ] && break
    [ "$STATE" = failed ] && firstboot_failed
  fi
  if [ "$i" = 40 ]; then
    [ "$STATE" = running ] && firstboot_failed
    if [ ! -s "$KNOWN_HOSTS" ]; then
      echo "could not pin the instance's ssh host key: its boot console shows neither the key nor its fingerprint. Read the ed25519 fingerprint"
      echo "from the instance (EC2 Instance Connect / Session Manager: ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub)"
      echo "and set AWS_SSH_HOST_KEY_FINGERPRINT=SHA256:… in $DEPLOY_ENV."
    else
      echo "instance not reachable over ssh after 10 minutes (check the security group / AWS_ADMIN_CIDR;"
      echo "a 'host key verification failed' means the key no longer matches $KNOWN_HOSTS)"
    fi
    exit 1
  fi
  sleep 15
done

# ---- 4. ship files ----
echo "==> [4/5] deploying"
STAGE=/tmp/learnmap-deploy
ENVFILE=$(mktemp)
# Nothing secret: the provider settings are read from Parameter Store on the
# instance (deploy/aws/learnmap-env.sh), which needs the path and the region.
{
  echo "# written by scripts/deploy-aws.sh from deploy/deploy.env"
  echo "SKRONGLE_PUBLIC_URL=$PUBLIC_URL"
  echo "AWS_REGION=$AWS_REGION"
  echo "AWS_SSM_PATH=$AWS_SSM_PATH"
  echo "AWS_BACKUP_BUCKET=$(stack_output BackupBucket)"
  echo "SKRONGLE_S3_BUCKET=$(stack_output FilesBucket)"
} > "$ENVFILE"
CADDYFILE=$(mktemp)
cat > "$CADDYFILE" <<EOF
# written by scripts/deploy-aws.sh
$HOST {
	encode zstd gzip
	reverse_proxy 127.0.0.1:8080
}
EOF
$SSH "ec2-user@$IP" "mkdir -p $STAGE/data"
rsync -az --delete -e "$SSH" "$BIN" "ec2-user@$IP:$STAGE/learnmap"
rsync -az --delete --exclude test-courses/ -e "$SSH" data/ "ec2-user@$IP:$STAGE/data/"
rsync -az -e "$SSH" deploy/aws/learnmap.service deploy/aws/learnmap-env.sh \
  deploy/aws/learnmap-backup.sh deploy/aws/learnmap-backup.service deploy/aws/learnmap-backup.timer \
  "$ENVFILE" "$CADDYFILE" "ec2-user@$IP:$STAGE/"
ENVNAME=$(basename "$ENVFILE"); CADDYNAME=$(basename "$CADDYFILE")
rm -f "$ENVFILE" "$CADDYFILE"
$SSH "ec2-user@$IP" "ENVNAME=$ENVNAME CADDYNAME=$CADDYNAME KEY_SHA=$KEY_SHA SHA=$SHA BACKUP_EVERY=$BACKUP_EVERY BUCKET=$(stack_output BackupBucket) bash -s" <<'REMOTE'
set -euo pipefail
STAGE=/tmp/learnmap-deploy
KEYFILE=/var/lib/learnmap/learn.db.key
if sudo test -f "$KEYFILE"; then
  HAVE=$(sudo cat "$KEYFILE" | tr -d '[:space:]' | tr 'A-F' 'a-f' | sha256sum | cut -d' ' -f1)
  if [ "$HAVE" != "$KEY_SHA" ]; then
    echo "SKRONGLE_SECRET_KEY in Parameter Store is not the key in $KEYFILE on the instance." >&2
    echo "Copy that one up (docs/aws-setup.md, \"Backups\"); nothing was changed." >&2
    exit 1
  fi
fi
L=/opt/learnmap
DB=/var/lib/learnmap/learn.db
sudo install -m 0600 -o root -g root "$STAGE/$ENVNAME" /etc/learnmap/env
command -v sqlite3 >/dev/null 2>&1 || sudo dnf install -y -q sqlite
sudo install -m 0755 -o root -g root "$STAGE/learnmap-backup.sh" $L/learnmap-backup
sudo install -m 0755 "$STAGE/learnmap" $L/learnmap.new
# Compare the schema the new binary migrates to with the live database's (#1329).
NEW=$(sudo $L/learnmap.new --schema-version 2>/dev/null || true)
[[ "$NEW" =~ ^[0-9]+$ ]] || { echo "the new binary does not print its schema version; nothing was changed" >&2; sudo rm -f $L/learnmap.new; exit 1; }
HAVE_DB=0; LIVE=0
if sudo test -f "$DB"; then HAVE_DB=1; LIVE=$(sudo -u learnmap sqlite3 -readonly "$DB" 'PRAGMA user_version;'); fi
if [ "$NEW" -lt "$LIVE" ]; then
  echo "the database is at schema v$LIVE, newer than this build knows (v$NEW); nothing was changed." >&2
  echo "Deploy a newer commit, or go back with scripts/deploy-aws.sh --rollback --restore." >&2
  sudo rm -f $L/learnmap.new; exit 1
fi
NOW=$(date -u +%FT%TZ)
BACKUP=-
if [ "$HAVE_DB" = 1 ] && { [ "$NEW" -gt "$LIVE" ] || [ "$BACKUP_EVERY" = 1 ]; }; then
  [ -n "$BUCKET" ] && [ "$BUCKET" != None ] || { echo "no backup bucket on the stack, so no release that migrates; nothing was changed" >&2; sudo rm -f $L/learnmap.new; exit 1; }
  NAME="release-$(date -u +%Y%m%dT%H%M%SZ)-$SHA"
  echo "    backing up the database first (schema v$LIVE to v$NEW): s3://$BUCKET/daily/$NAME.db.gz"
  # The same backup as learnmap-backup.service, under its own name.
  if ! sudo systemd-run --quiet --wait --pipe --collect --uid=learnmap --gid=learnmap \
      -p EnvironmentFile=/etc/learnmap/env -E SKRONGLE_DB="$DB" -E HOME=/var/lib/learnmap \
      -E LEARNMAP_BACKUP_NAME="$NAME" $L/learnmap-backup; then
    echo "the backup failed, so the release stops here; nothing was changed" >&2
    sudo rm -f $L/learnmap.new; exit 1
  fi
  BACKUP="s3://$BUCKET/daily/$NAME.db.gz"
fi
# Keep the live release as the previous one: two releases, not more.
sudo rm -rf $L/learnmap.prev $L/data.prev $L/release.prev
if sudo test -x $L/learnmap; then
  sudo ln -f $L/learnmap $L/learnmap.prev
  if sudo test -d $L/data; then sudo cp -al $L/data $L/data.prev; fi   # hard links: rsync below writes new files
  if sudo test -f $L/release; then
    sudo cp $L/release $L/release.prev
  else
    echo "unknown unknown - $LIVE -" | sudo tee $L/release.prev >/dev/null   # released before #1329
  fi
fi
sudo mv -f $L/learnmap.new $L/learnmap        # atomic swap
sudo rsync -a --delete "$STAGE/data/" $L/data/
sudo chown -R root:learnmap $L
echo "$NOW $SHA $LIVE $NEW $BACKUP" | sudo tee $L/release >/dev/null
echo "$NOW deploy $SHA $LIVE $NEW $BACKUP" | sudo tee -a $L/releases.log >/dev/null
sudo tail -n 50 $L/releases.log | sudo tee $L/releases.log.new >/dev/null && sudo mv -f $L/releases.log.new $L/releases.log
sudo install -m 0755 -o root -g root "$STAGE/learnmap-env.sh" /opt/learnmap/learnmap-env
if ! cmp -s "$STAGE/learnmap.service" /etc/systemd/system/learnmap.service; then
  sudo install -m 0644 "$STAGE/learnmap.service" /etc/systemd/system/learnmap.service
  sudo systemctl daemon-reload
fi
sudo systemctl enable learnmap >/dev/null 2>&1 || true
sudo systemctl restart learnmap
# Nightly backup to S3 (#964).
for u in learnmap-backup.service learnmap-backup.timer; do
  if ! cmp -s "$STAGE/$u" "/etc/systemd/system/$u"; then
    sudo install -m 0644 "$STAGE/$u" "/etc/systemd/system/$u"
    sudo systemctl daemon-reload
  fi
done
sudo systemctl enable --now learnmap-backup.timer >/dev/null 2>&1
if ! cmp -s "$STAGE/$CADDYNAME" /etc/caddy/Caddyfile; then
  sudo install -m 0644 -o caddy -g caddy "$STAGE/$CADDYNAME" /etc/caddy/Caddyfile
  sudo systemctl reload caddy || sudo systemctl restart caddy
fi
# The journal keeps no line older than 31 days, the privacy notice's limit
# (#972): a journal file is closed after a day and deleted 30 days later.
JOURNALD=$'[Journal]\nMaxFileSec=1day\nMaxRetentionSec=30day'
JCONF=/etc/systemd/journald.conf.d/50-skrongle-retention.conf
if [ "$(sudo cat "$JCONF" 2>/dev/null)" != "$JOURNALD" ]; then
  sudo mkdir -p /etc/systemd/journald.conf.d
  printf '%s\n' "$JOURNALD" | sudo tee "$JCONF" >/dev/null
  sudo systemctl restart systemd-journald
fi
rm -rf "$STAGE"
sleep 1
sudo systemctl --no-pager --lines=3 status learnmap || true
REMOTE

# ---- 5. verify ----
echo "==> [5/5] verifying"
if ! check_health; then
  echo "    scripts/deploy-aws.sh --rollback puts the previous release back"
  exit 1
fi
PUB=000
for i in $(seq 1 12); do
  PUB=$(curl -s -o /dev/null -w "%{http_code}" --max-time 15 "$PUBLIC_URL/healthz" || echo 000)
  [ "$PUB" = 200 ] && break
  sleep 10
done
echo "    public ($PUBLIC_URL/healthz): HTTP $PUB"
if [ "$PUB" != 200 ]; then
  if [ -n "${AWS_DOMAIN:-}" ]; then
    echo "    If $AWS_DOMAIN does not point at $IP yet, create an A record for it; Caddy fetches the certificate once it resolves."
  else
    echo "    The certificate for $HOST may still be on its way; try again in a minute."
  fi
fi
echo "==> released: $PUBLIC_URL"
show_releases 1
if [ -n "${GOOGLE_CLIENT_ID:-}" ]; then
  echo "    Google sign-in redirect URI to register on the OAuth client: $PUBLIC_URL/auth/google/callback"
fi
if [ -n "${STRIPE_SECRET_KEY:-}" ]; then
  echo "    Stripe webhook endpoint to register: $PUBLIC_URL/billing/webhook"
fi
