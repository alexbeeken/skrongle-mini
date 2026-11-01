#!/bin/bash
# Runs on the AWS instance as the learnmap user (learnmap-backup.service,
# every night from learnmap-backup.timer, and from `scripts/deploy-aws.sh
# --backup` / `--destroy`). Takes a consistent copy of the live database with
# SQLite's online backup (a plain file copy of a WAL database can miss the
# last writes or catch a half-written page), checks it, gzips it and uploads
# it to s3://$AWS_BACKUP_BUCKET/daily/YYYY-MM-DD.db.gz (UTC date; a second
# run on the same day replaces that day's copy). With LEARNMAP_BACKUP_NAME set
# (scripts/deploy-aws.sh, before a release that migrates the database) it
# goes to daily/$LEARNMAP_BACKUP_NAME.db.gz instead and replaces nothing.
#
# The database alone is enough to restore: the secret key that encrypts saved
# AI keys is SKRONGLE_SECRET_KEY in Parameter Store, not a file on this disk.
# AWS_BACKUP_BUCKET and AWS_REGION come from /etc/learnmap/env (written by
# scripts/deploy-aws.sh); the credentials from the instance role, which may
# put objects under daily/ and nothing else.
set -euo pipefail
: "${AWS_BACKUP_BUCKET:?AWS_BACKUP_BUCKET is not set (scripts/deploy-aws.sh writes it to /etc/learnmap/env)}"
: "${AWS_REGION:?AWS_REGION is not set (scripts/deploy-aws.sh writes it to /etc/learnmap/env)}"
export AWS_DEFAULT_REGION="$AWS_REGION"
DB="${SKRONGLE_DB:-/var/lib/learnmap/learn.db}"
[ -f "$DB" ] || { echo "learnmap-backup: no database at $DB" >&2; exit 1; }

# On the data disk, not /tmp: /tmp is RAM on this instance.
WORK=$(mktemp -d "$(dirname "$DB")/backup.XXXXXX")
trap 'rm -rf "$WORK"' EXIT
COPY="$WORK/learn.db"

# Waits up to 30 s for a writer instead of failing on SQLITE_BUSY.
sqlite3 "$DB" ".timeout 30000" ".backup '$COPY'"
CHECK=$(sqlite3 "$COPY" "PRAGMA quick_check;" 2>&1 || true)
[ "$CHECK" = ok ] || { echo "learnmap-backup: the copy fails quick_check: $CHECK" >&2; exit 1; }
gzip -6 "$COPY"

NAME="${LEARNMAP_BACKUP_NAME:-$(date -u +%F)}"
[[ "$NAME" =~ ^[A-Za-z0-9._-]+$ ]] || { echo "learnmap-backup: bad LEARNMAP_BACKUP_NAME: $NAME" >&2; exit 1; }
KEY="daily/$NAME.db.gz"
aws s3 cp "$COPY.gz" "s3://$AWS_BACKUP_BUCKET/$KEY" --only-show-errors
echo "learnmap-backup: $(stat -c %s "$COPY.gz") bytes to s3://$AWS_BACKUP_BUCKET/$KEY"
