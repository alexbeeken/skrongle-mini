# AWS setup

You do these steps once, in order, before `scripts/deploy-all.sh aws`
works. Budget about 30 minutes, mostly waiting for AWS.

## 1. An AWS account

1. Go to <https://aws.amazon.com/> → *Create an AWS account*. You need an
   e-mail address, a phone number for the verification call/SMS and a payment
   card. Choose the **Basic (free) support plan**.
2. New accounts are offered a **free plan** (credits, up to six months, no
   charges beyond them) or a **paid plan** (pay as you go, with some free-tier
   credits). Either runs this app. The paid plan keeps working after the
   credits are used. Accounts created before July 2025 have the older 12-month
   free tier instead, which covers the instance and the IPv4 address.
3. Sign in as the root user, open the account menu (top right) → *Security
   credentials* → **assign an MFA device** to the root user. Then stop using
   the root user for day-to-day work.

## 2. A spending alarm (recommended)

Billing console → *Budgets* → *Create budget* → *Use a template* → **Monthly
cost budget**: amount `10` USD, your e-mail. AWS e-mails you if the month is
heading past $10. The stack this app creates costs about $7.40/month; anything
above that means something else is running.

## 3. A deployment user and its access key

The deploy script needs an identity that may create EC2 instances and
CloudFormation stacks, the instance's IAM role, and the Parameter Store
entries the app's secrets live in. Do not use the root user's keys for this.

1. Console → **IAM** → *Users* → *Create user*.
   - User name: `learnmap-deploy`.
   - Leave *Provide user access to the AWS Management Console* unticked.
2. *Set permissions* → **Attach policies directly** → search and tick:
   - `AmazonEC2FullAccess`
   - `AWSCloudFormationFullAccess`
   - `AmazonSSMReadOnlyAccess` (the stack looks up the current Amazon Linux
     image through an SSM parameter)
   Next → *Create user*.
3. Open the user → *Permissions* → *Add permissions* → **Create inline
   policy** → *JSON*, paste this, name it `learnmap-deploy-extra`. It lets
   the stack create the instance's role, the backup bucket and the snapshot
   policy, lets `scripts/aws-env.sh` manage the settings under `/skrongle/`,
   and lets the deploy script read backups (it can't delete them):

   ```json
   {
     "Version": "2012-10-17",
     "Statement": [
       {
         "Effect": "Allow",
         "Action": [
           "iam:CreateRole", "iam:DeleteRole", "iam:GetRole", "iam:TagRole",
           "iam:PutRolePolicy", "iam:DeleteRolePolicy", "iam:GetRolePolicy",
           "iam:PassRole",
           "iam:CreateInstanceProfile", "iam:DeleteInstanceProfile",
           "iam:GetInstanceProfile", "iam:AddRoleToInstanceProfile",
           "iam:RemoveRoleFromInstanceProfile",
           "iam:AttachRolePolicy", "iam:DetachRolePolicy"
         ],
         "Resource": [
           "arn:aws:iam::*:role/learnmap-*",
           "arn:aws:iam::*:instance-profile/learnmap-*"
         ]
       },
       {
         "Effect": "Allow",
         "Action": [
           "ssm:PutParameter", "ssm:DeleteParameter", "ssm:GetParameter",
           "ssm:GetParameters", "ssm:GetParametersByPath", "ssm:AddTagsToResource"
         ],
         "Resource": "arn:aws:ssm:*:*:parameter/skrongle/*"
       },
       {
         "Effect": "Allow",
         "Action": ["kms:Encrypt", "kms:Decrypt"],
         "Resource": "*",
         "Condition": { "StringLike": { "kms:ViaService": "ssm.*.amazonaws.com" } }
       },
       {
         "Effect": "Allow",
         "Action": [
           "s3:CreateBucket", "s3:ListBucket", "s3:ListBucketVersions",
           "s3:GetBucket*", "s3:PutBucket*", "s3:DeleteBucketPolicy",
           "s3:GetLifecycleConfiguration", "s3:PutLifecycleConfiguration",
           "s3:GetEncryptionConfiguration", "s3:PutEncryptionConfiguration",
           "s3:GetObject", "s3:GetObjectVersion"
         ],
         "Resource": ["arn:aws:s3:::learnmap-*", "arn:aws:s3:::learnmap-*/*"]
       },
       {
         "Effect": "Allow",
         "Action": [
           "s3:ListAllMyBuckets",
           "dlm:CreateLifecyclePolicy", "dlm:GetLifecyclePolicy", "dlm:GetLifecyclePolicies",
           "dlm:UpdateLifecyclePolicy", "dlm:DeleteLifecyclePolicy",
           "dlm:TagResource", "dlm:UntagResource"
         ],
         "Resource": "*"
       }
     ]
   }
   ```

   (`learnmap-*` is the stack name; change every one if you set `AWS_STACK_NAME`.)
4. Open the user → *Security credentials* tab → *Access keys* → **Create
   access key** → use case *Command Line Interface (CLI)* → tick the
   confirmation → *Next* → *Create access key*.
5. Copy the **Access key ID** and the **Secret access key** now; the secret
   is shown once. They go into `deploy/deploy.env` as `AWS_ACCESS_KEY_ID` and
   `AWS_SECRET_ACCESS_KEY`. (Alternatively run `aws configure --profile
   learnmap` and set `AWS_PROFILE=learnmap` instead.)

If a key ever leaks, come back here, *Deactivate* then *Delete* it, and create
a new one; nothing else changes.

## 4. A region

Pick one region and keep it; the stack lives there. `eu-west-1` (Ireland) or
`us-east-1` (N. Virginia) are good defaults; a region near you lowers latency.
Prices for `t4g.nano` are within cents of each other everywhere. Put it in
`deploy.env` as `AWS_REGION`.

Every region comes with a **default VPC**; the stack uses it. If someone
deleted it (console → VPC → *Your VPCs* is empty), restore it with
*Actions* → *Create default VPC*.

## 5. Tools on your Mac

```sh
brew install awscli zig          # the AWS CLI and the cross-compiler zig provides
cargo install cargo-zigbuild     # cross-compiles the Rust binary for ARM
rustup target add aarch64-unknown-linux-gnu
aws --version                    # should print aws-cli/2.x
```

You do not need `aws configure` when the keys are in `deploy.env`.

## 6. An ssh key

The public half is placed on the instance when it is created; the deploy
script uses the private half to ssh in as `ec2-user`. Any existing key works
(`~/.ssh/id_ed25519`); to make a dedicated one:

```sh
ssh-keygen -t ed25519 -f ~/.ssh/learnmap -N "" -C learnmap
```

and set `AWS_SSH_KEY=~/.ssh/learnmap`. The private key stays on your Mac.

## 7. Fill in `deploy/deploy.env`

```sh
cp deploy/deploy.env.example deploy/deploy.env
$EDITOR deploy/deploy.env
```

Minimum for AWS:

```sh
DEPLOY_TARGETS="aws"             # or "pi aws" once the Pi is configured too
AWS_ACCESS_KEY_ID=AKIA…
AWS_SECRET_ACCESS_KEY=…
AWS_REGION=eu-west-1
AWS_SSH_KEY=~/.ssh/learnmap
AWS_DOMAIN=                      # empty: use the free <ip>.sslip.io name
```

No secrets of the app go in this file; those are the next step.

Check it without touching AWS:

```sh
scripts/deploy-all.sh aws --dry-run
```

## 8. The app's secrets, in Parameter Store

This is a hosted server, so it needs every provider or it refuses to start
and names what is missing: e-mail (`SKRONGLE_MAIL_*`), text messages
(`SKRONGLE_SMS_*`), Google sign-in (`SKRONGLE_GOOGLE_*`, step 10) and Stripe
(`SKRONGLE_STRIPE_*`). Only a Skrongle mini runs without them.

They live in **AWS Systems Manager → Parameter Store**, one encrypted
`SecureString` per setting, named like the environment variable it becomes,
under `/skrongle/<stack name>/` (`AWS_SSM_PATH`). Nothing secret is on your
Mac or in the deploy: the instance has an IAM role that may read that path
and nothing else, and reads the settings right before every start into
tmpfs (`/run/learnmap/env`). Standard-tier parameters are free.

```sh
scripts/aws-env.sh init                          # one TODO placeholder per setting the server needs
openssl rand -hex 32 | scripts/aws-env.sh set SKRONGLE_SECRET_KEY   # once, never again (see below)
scripts/aws-env.sh set SKRONGLE_MAIL_FROM        # asks for the value, hidden
scripts/aws-env.sh set SKRONGLE_MAIL_PROVIDERS resend
scripts/aws-env.sh set SKRONGLE_MAIL_RESEND_KIND resend
scripts/aws-env.sh set SKRONGLE_MAIL_RESEND_API_KEY
…
scripts/aws-env.sh list                          # which are set, which still TODO
scripts/aws-env.sh check                         # what the deploy will refuse on
```

Or fill them in the console: Systems Manager → Parameter Store → the
parameter → *Edit*. A value of `TODO` is a placeholder: the instance skips
it, so the app sees the setting as unset. Each setting's description says
what goes in it; which settings a provider needs is in
[configuration](architecture#configuration). Change a value, then
`scripts/deploy-aws.sh --skip-build` (or `sudo systemctl restart learnmap`
on the box) to make the app read it.

`SKRONGLE_SECRET_KEY` encrypts the AI keys learners save. Set it once, before
the first deploy, and never change it: a new key makes every saved AI key
unreadable. Keep a copy in your password manager too. Because it lives here
and not on the instance's disk, a backup of the database is all a restore
needs ([Backups](#backups)). The deploy refuses to go ahead without it. On a
stack that already ran without it, copy the instance's existing key up
instead of making a new one ([Moving the key off the disk](#moving-the-key-off-the-disk)).

Settings stored before the rename to `SKRONGLE_` (#584) keep working for
one release, with a warning in the log. `scripts/aws-env.sh migrate` moves
each to its new name, and every deploy runs it first.

## 9. First deploy

```sh
scripts/deploy-all.sh aws
```

What happens, and how long it takes:

| Step | Time |
|---|---|
| tests + cross-compile (first time; later builds are incremental) | 2–5 min |
| the required settings are checked in Parameter Store (`scripts/aws-env.sh check`) | seconds |
| CloudFormation creates the security group, the instance and its read-only role, and the static IP | ~2 min |
| first boot makes a 1 GB swap file and installs a pinned, checksum-verified Caddy (the script waits for it) | ~2 min |
| the script pins the instance's ssh host key from its boot console (AWS API) in `deploy/aws/known_hosts.<instance-id>`: the key cloud-init lists at the end of first boot, or sooner, the key `ssh-keyscan` gets if it matches the fingerprint printed when the key was made; every later ssh checks it | with the wait above |
| rsync; the nightly backup timer is installed; at start the instance reads its settings from Parameter Store; health check | seconds |
| Caddy obtains the HTTPS certificate for `<ip>.sslip.io` | 10–60 s |

The last line printed is the site URL, e.g.
`https://54-72-10-20.sslip.io`. Open it, register, pick a course.

If the public health check says anything but 200 on the first run, wait a
minute and run `scripts/deploy-aws.sh --status`; the certificate is usually
the only thing still in progress.

## 10. Google sign-in

In the Google Cloud console, open your OAuth client (APIs & Services →
Credentials) and add an **Authorized redirect URI**:

```
https://<your AWS host>/auth/google/callback
```

exactly as printed at the end of the deploy. The client id and secret are
the `SKRONGLE_GOOGLE_CLIENT_ID` and `SKRONGLE_GOOGLE_CLIENT_SECRET` settings of
step 8.

## 11. Your own domain (optional)

Set `AWS_DOMAIN=learn.example.com`, deploy, then create an **A record** for
that name pointing at the IP from `scripts/deploy-aws.sh --status`. Caddy
notices when the name resolves and fetches a certificate; no restart needed.
Add the new redirect URI to the Google client if you use it.

## E-mail

The hosted site's only; a Skrongle mini ignores these settings.

The app writes every message to an outbox in its database and a background
thread sends it. `SKRONGLE_MAIL_PROVIDERS` lists one or more providers in the
order to try them; if one cannot be reached or refuses a message, the next
gets it. A provider that could not be reached is tried last for five minutes.
A message nobody took is retried after 1 minute, 5 minutes, 30 minutes, 2 and
12 hours, then marked failed; one that every provider refused (a bad
address) fails at once. Restarts lose nothing. In dev mode, without
providers, mail goes to the app's own dev mailbox at `/dev/inbox`
([testing](testing#e-mail-the-dev-mailbox)).

Two caps protect your sender domain's reputation, since anyone can register
with any address. One address gets at most 10 address-confirmation e-mails
a day, whichever account asks for them (deleting the account and
registering again does not reset this); past that, nothing is sent. And
at most `SKRONGLE_MAIL_PER_HOUR` e-mails of each kind (default 3000) are due
in any clock hour. Each kind has its own budget, so a burst of course
invitations never holds back a confirmation e-mail or a sign-in code. The
app does not drop a message past the ceiling. It queues the message for the
next hour with room, up to 12 hours ahead, and sends it then. Sign-in codes
are the exception. They expire in 10 minutes, so the app does not queue one
past the ceiling and the page says it could not be sent. Set the ceiling to what your
provider's plan allows (a plan for about 10,000 messages a day suits 3000),
or `0` to turn it off. Dev mode has no hourly ceiling unless you set one.
The log says when a message waits (`mail #12 (email-verify) waits 1800s`)
and when a cap holds one back.

Each provider is named in `SKRONGLE_MAIL_PROVIDERS` and configured with
`SKRONGLE_MAIL_<NAME>_…`. `_KIND` says what it is (`smtp`; defaults to the
name). Any SMTP submission service works: Amazon SES, Postmark, Resend,
Brevo, Mailgun.

| Variable | Default | Meaning |
|---|---|---|
| `SKRONGLE_MAIL_<NAME>_HOST` | — | SMTP server. |
| `SKRONGLE_MAIL_<NAME>_TLS` | `starttls` | `starttls` (port 587), `tls` (port 465), or `none` for a server on this machine only. |
| `SKRONGLE_MAIL_<NAME>_PORT` | from `_TLS` | |
| `SKRONGLE_MAIL_<NAME>_USER`, `_PASS` | — | SMTP credentials; both or neither. |

HTTP API providers (`_KIND=resend`, `postmark`, `brevo`) send over HTTPS (port 443)
instead, so they work where outbound SMTP is blocked, and their errors
tell a bad address (refused at once) from an outage or a bad key (retried,
next provider). The message's purpose goes to the provider as its tag.

| Variable | Default | Meaning |
|---|---|---|
| `SKRONGLE_MAIL_<NAME>_KIND` | the name | `resend`: [Resend](https://resend.com)'s API. `postmark`: [Postmark](https://postmarkapp.com)'s API. `brevo`: [Brevo](https://brevo.com)'s API. |
| `SKRONGLE_MAIL_<NAME>_API_KEY` | — | Required. Resend: an API key (`re_…`) with sending access. Postmark: the Server API token. Brevo: an API key (`xkeysib-…`, SMTP & API → API Keys). |
| `SKRONGLE_MAIL_<NAME>_API_URL` | the provider's (`https://api.resend.com`, `https://api.postmarkapp.com`, `https://api.brevo.com/v3/smtp/email`) | Where API calls go, for pointing tests at a mock. `https://`, or `http://` on this machine. |
| `SKRONGLE_MAIL_<NAME>_STREAM` | `outbound` | `postmark` only: the message stream. |

Resend and Brevo take an idempotency key built from the outbox id, so a
retry after a timeout does not send twice. Postmark has none: a retry after
a timeout it did accept sends the message twice (the outbox id is in its
metadata). `SKRONGLE_MAIL_FROM` must be a sender the provider has verified.

Amazon SES first, a second service as fallback:

```sh
SKRONGLE_MAIL_FROM=Skrongle <noreply@learn.example.org>
SKRONGLE_MAIL_PROVIDERS=ses,backup
SKRONGLE_MAIL_SES_KIND=smtp
SKRONGLE_MAIL_SES_HOST=email-smtp.eu-west-1.amazonaws.com
SKRONGLE_MAIL_SES_USER=AKIA…
SKRONGLE_MAIL_SES_PASS=…
SKRONGLE_MAIL_BACKUP_KIND=smtp
SKRONGLE_MAIL_BACKUP_HOST=smtp.postmarkapp.com
SKRONGLE_MAIL_BACKUP_USER=…
SKRONGLE_MAIL_BACKUP_PASS=…
```

Or Resend's API as the fallback after SES:

```sh
SKRONGLE_MAIL_PROVIDERS=ses,resend
SKRONGLE_MAIL_RESEND_KIND=resend
SKRONGLE_MAIL_RESEND_API_KEY=re_…
```

Or Postmark's HTTP API as the fallback:

```sh
SKRONGLE_MAIL_PROVIDERS=ses,postmark
SKRONGLE_MAIL_POSTMARK_KIND=postmark
SKRONGLE_MAIL_POSTMARK_API_KEY=…   # Server API token
```

Or Brevo's HTTP API as the fallback:

```sh
SKRONGLE_MAIL_PROVIDERS=ses,brevo
SKRONGLE_MAIL_BREVO_KIND=brevo
SKRONGLE_MAIL_BREVO_API_KEY=xkeysib-…
```

The sender's domain needs SPF and DKIM records for each provider (each
provider's console shows them), or mail lands in spam. A new SES account
only sends to verified addresses until you ask AWS to lift the sandbox.
The log names each message by id and says which provider took it.

## Text messages

The hosted site's only; a Skrongle mini ignores these settings.

Texts carry phone verification and two-factor sign-in codes. Texts work like e-mail: an outbox, a background sender, providers
tried in `SKRONGLE_SMS_PROVIDERS` order with the same fallback and retries.
Each account may queue 5 texts an hour and 20 a day, and each phone number
may receive 5 an hour and 10 a day: sign-in codes and other texts (a
number's verification code) apart, so verification texts never use up a
number's sign-in codes. The whole server sends at most
`SKRONGLE_SMS_MAX_PER_HOUR` and `SKRONGLE_SMS_MAX_PER_DAY` texts of each of those
two kinds, and only to the countries in `SKRONGLE_SMS_COUNTRIES`. The limits
count a ledger (`sms_sends`: the number's SHA-256, the account, when) that
stays when an account is deleted and is purged after two days, so deleting
and re-registering gives no allowance back. Only accounts with a confirmed
e-mail address or Google sign-in may add a phone number.

| Variable | Default | Meaning |
|---|---|---|
| `SKRONGLE_SMS_COUNTRIES` | `1,44,353,33,34,49,39,31,32,41,43,45,46,47,358,351,61,64` | Calling codes texts may go to, separated by commas (`+` optional). A longer prefix narrows one (`1415`: San Francisco only). `*` allows every country. The default is North America (+1, which includes some Caribbean ranges), the UK and Ireland, western and northern Europe, Australia and New Zealand; toll fraud mostly targets premium ranges elsewhere. |
| `SKRONGLE_SMS_MAX_PER_HOUR` | `100` | Texts the whole server may queue in an hour, for sign-in codes and for other texts each. |
| `SKRONGLE_SMS_MAX_PER_DAY` | `500` | The same per day. |

Each provider is named in `SKRONGLE_SMS_PROVIDERS` and configured with
`SKRONGLE_SMS_<NAME>_…`. `_KIND` is `http` (defaults to the name): one HTTP
POST per text, which fits most SMS APIs and any webhook.

| Variable | Default | Meaning |
|---|---|---|
| `SKRONGLE_SMS_<NAME>_URL` | — | Where to POST. Must be `https://` (`http://` only for this machine). |
| `SKRONGLE_SMS_<NAME>_FORMAT` | `json` | `json` or `form` (`application/x-www-form-urlencoded`). |
| `SKRONGLE_SMS_<NAME>_BODY` | see below | The body, with `{to}`, `{from}`, `{text}`, `{id}`, `{purpose}`; must contain `{to}` and `{text}`. In JSON put each placeholder inside quotes. |
| `SKRONGLE_SMS_<NAME>_AUTH` | — | An auth header's value, e.g. `Bearer abc…`. |
| `SKRONGLE_SMS_<NAME>_AUTH_HEADER` | `Authorization` | The header `_AUTH` goes in, e.g. `X-API-Key`. |
| `SKRONGLE_SMS_<NAME>_USER`, `_PASS` | — | HTTP Basic credentials instead of `_AUTH`; both or neither. |
| `SKRONGLE_SMS_<NAME>_FROM` | `SKRONGLE_SMS_FROM` | The sender number or ID, `{from}`. |

The default bodies are `{"to":"{to}","from":"{from}","text":"{text}","id":"{id}","purpose":"{purpose}"}`
for `json` and `To={to}&From={from}&Body={text}` for `form`. A 2xx answer
means sent; 5xx, 408, 429 or no answer means try later (and the next
provider now); any other 4xx means this provider refuses the text.

Twilio first, an own webhook as fallback:

```sh
SKRONGLE_SMS_FROM=+15017122661
SKRONGLE_SMS_PROVIDERS=twilio,hook
SKRONGLE_SMS_TWILIO_KIND=http
SKRONGLE_SMS_TWILIO_URL=https://api.twilio.com/2010-04-01/Accounts/AC…/Messages.json
SKRONGLE_SMS_TWILIO_FORMAT=form
SKRONGLE_SMS_TWILIO_USER=AC…
SKRONGLE_SMS_TWILIO_PASS=…            # the auth token
SKRONGLE_SMS_HOOK_KIND=http
SKRONGLE_SMS_HOOK_URL=https://hooks.example.org/sms
SKRONGLE_SMS_HOOK_AUTH=Bearer …
```

Numbers are international (`+` and the country code). The log names each
text by id and its number's last two digits, and says which provider took it.

## Day to day

```sh
scripts/deploy-all.sh aws                   # release a new version (backed up first if it migrates)
scripts/deploy-aws.sh --status              # URL, IP, ssh command, the last releases
scripts/deploy-aws.sh --rollback            # put the previous release back (see "Rolling back")
scripts/aws-env.sh list                     # the app's settings in Parameter Store (set NAME to change one)
scripts/deploy-aws.sh --backup              # back up the database to S3 now
ssh -i ~/.ssh/learnmap ec2-user@<ip>        # look around
  sudo journalctl -u learnmap -f            #   app log
  sudo journalctl -u caddy -n 50            #   proxy / certificate log
  systemctl list-timers learnmap-backup     #   when the next backup runs
scripts/deploy-aws.sh --destroy             # back up, then delete everything (asks for confirmation)
```

The ssh rule in the security group is rewritten to your current public IP on
every deploy (`AWS_ADMIN_CIDR=auto`), so after changing networks just deploy
again or run `scripts/deploy-aws.sh --skip-build`.

## Rolling back

The instance keeps the release before the live one: its binary and course
files sit beside the live ones as `/opt/learnmap/learnmap.prev` and
`data.prev`. It keeps two releases, never more.

Before a release whose binary knows a newer database schema than the live
database's (`PRAGMA user_version`), the deploy backs the database up to
`daily/release-<time>-<git sha>.db.gz` in the backup bucket. If that backup
fails, the release stops and nothing changes. `--with-backup` takes the
backup on every release. A build that knows an older schema than the live
database is refused.

Each deploy and rollback adds a line to `/opt/learnmap/releases.log` on the
instance: time, `deploy` or `rollback`, git sha, schema before and after,
and the backup taken before it. The deploy prints its line at the end;
`--status` prints the last five.

```sh
scripts/deploy-aws.sh --rollback             # the last release didn't migrate: swap the previous one back
scripts/deploy-aws.sh --rollback --restore   # it did: the previous release plus the backup taken before it
```

- When the database is still at the schema the previous release ran on,
  `--rollback` swaps the previous binary and course files back in and
  restarts the app. Nothing is lost.
- When the last release migrated the database, `--rollback` stops and says
  so. It names the backup taken just before that release and how long ago
  it was. Everything learners did since then is lost on a restore.
  `--rollback --restore` puts the previous release back and restores that
  backup, the same way as `--restore` (the current database stays on the
  instance as `learn.db.before-restore-<time>`).

Both ask for the stack name, run the same health check as a deploy and
print what is live. After a rollback there is no previous release on the
instance until the next deploy.

A Skrongle from #1329 on refuses to start on a database a newer release
upgraded. It logs "this database was upgraded by a newer Skrongle; deploy
that version or restore a backup", exits with status 78 and systemd leaves
the service failed instead of restarting it. The database is not touched.

There are no down-migrations: a schema change is undone by restoring the
backup.

## Backups

The stack keeps two copies of the database, both in the stack's region:

| What | When | Kept | Where |
|---|---|---|---|
| The database, copied with SQLite's online backup, checked and gzipped | every night at 03:30 UTC, and before `--destroy` | 30 days (a replaced copy one more day) | the stack's private S3 bucket, `daily/YYYY-MM-DD.db.gz` |
| The database, the same way, before a release that migrates it | at that deploy | 30 days | the same bucket, `daily/release-<time>-<git sha>.db.gz` |
| A snapshot of the whole disk | every night at 04:30 UTC | 14 days | EBS snapshots tagged `learnmap-snapshot-of=<stack>` |

Nothing is kept longer than 31 days, the limit the privacy notice gives for
data of a deleted account. The bucket blocks public access, encrypts every
object, refuses plain HTTP and keeps old versions for a day, so a bad copy
doesn't replace a good one at once. The instance may add backups but can't
read or delete them. Your deploy user can read them but can't delete them.

Deleting the stack (`--destroy`) takes a backup first and refuses to delete
if that backup fails. The bucket outlives the stack. It empties itself as
the copies expire, and is then an empty bucket you can delete. The disk
snapshots outlive it too, but nothing prunes them any more; delete them in
the EC2 console (*Snapshots*, filter on the tag) once you no longer need them.

The backup is the database alone. The key that encrypts saved AI keys is
`SKRONGLE_SECRET_KEY` in Parameter Store (step 8), which a new stack reads
the same way. A disk snapshot holds the database and, on a stack from
before the move, the old key file as well.

Costs: the S3 copies are a few cents a month; the snapshots store only what
changed from day to day, well under $1 a month for the 8 GB disk.

To check that backups run: `scripts/deploy-aws.sh --status` names the
latest one. On the instance, `systemctl list-timers learnmap-backup` shows
the next run and `journalctl -u learnmap-backup` the last ones. A failed
run shows in `systemctl --failed`.

### Moving the key off the disk

A stack deployed before #964 made its own key file,
`/var/lib/learnmap/learn.db.key`. Copy that value into Parameter Store,
without making a new one:

```sh
scripts/deploy-aws.sh --status                       # the ssh command
ssh … ec2-user@<ip> 'sudo cat /var/lib/learnmap/learn.db.key' | scripts/aws-env.sh set SKRONGLE_SECRET_KEY
scripts/deploy-aws.sh --skip-build
```

Save the same value in your password manager. The deploy compares the key
in Parameter Store with the file on the instance and stops, changing
nothing, if they differ. Once a deploy has gone through, the app reads the
key from Parameter Store; the file can stay where it is.

### Restore a backup onto the running stack

For a database that went wrong (a bad migration, a mistake in the admin
pages):

```sh
scripts/deploy-aws.sh --restore              # the latest backup
scripts/deploy-aws.sh --restore 2026-10-27   # the backup of that day (UTC)
```

It asks for the stack name, then the instance downloads the backup through
a link that works for 15 minutes, checks it, stops the app, puts the
current database aside as `learn.db.before-restore-<time>`, starts the app
and checks its health. Sign in and look at your account. Delete the
`before-restore` file once you are happy (it holds personal data too).

### Restore onto a new stack

When the instance or the whole stack is gone:

1. Find the bucket of the old stack: `aws s3 ls | grep learnmap` lists
   `learnmap-backupbucket-…` buckets; the old one has the copies in it
   (`aws s3 ls s3://<bucket>/daily/`).
2. Deploy as usual: `scripts/deploy-all.sh aws`. It makes a new stack with
   a new, empty bucket and an empty database. `SKRONGLE_SECRET_KEY` and the
   other settings are still in Parameter Store.
3. Restore from the old bucket:
   `scripts/deploy-aws.sh --restore s3://<old bucket>/daily/<date>.db.gz`.
4. Sign in and check your account. If the domain pointed at the old IP,
   point it at the new one (`--status`).

### Restore from a disk snapshot

When the S3 copies are no help (say a backup ran after the damage was
done), the nightly disk snapshots go back 14 days:

1. EC2 console → *Snapshots* → filter on the tag
   `learnmap-snapshot-of=<stack>` → pick one → *Create volume from
   snapshot*, in the instance's Availability Zone.
2. *Volumes* → the new volume → *Attach volume* to the instance (it shows up
   as `/dev/nvme1n1`).
3. On the instance:
   ```sh
   sudo mkdir -p /mnt/old && sudo mount -o ro,nouuid /dev/nvme1n1p1 /mnt/old
   sudo systemctl stop learnmap
   sudo mv /var/lib/learnmap/learn.db /var/lib/learnmap/learn.db.before-restore
   sudo rm -f /var/lib/learnmap/learn.db-wal /var/lib/learnmap/learn.db-shm
   sudo cp -a /mnt/old/var/lib/learnmap/learn.db* /var/lib/learnmap/
   sudo systemctl start learnmap
   sudo umount /mnt/old
   ```
   The snapshot was taken from a running disk, so copy the `-wal` file with
   the database (the `learn.db*` above does); SQLite replays it on start.
4. Detach and delete the extra volume.

### Restore test

Do one full restore on a throwaway stack before launch, and again after a
change to backups. Use the live settings, so the test stack reads the same
key:

```sh
export AWS_STACK_NAME=learnmap-restoretest AWS_SSM_PATH=/skrongle/learnmap AWS_DOMAIN=
scripts/deploy-aws.sh
scripts/deploy-aws.sh --restore s3://<live bucket>/daily/<date>.db.gz
```

Sign in with a real account and check its courses and plan. Then
`scripts/deploy-aws.sh --destroy` the test stack (same exports), delete its
bucket and its snapshots, and write the date here. The test server sends
e-mails and texts like the live one, so test soon after a backup, when
none are waiting to go out.

Last full restore test: not done yet.

## The content check in its own process

The AI content check (Jev and the AI review of what scored high) can run
in a process of its own, so its work and its keys stay apart from the
web server. Set `SKRONGLE_CONTENT_CHECK=external` in Parameter Store; the
server then only queues the checks. Start the binary's `content-check`
command on the same box, with the server's settings (the same
`SKRONGLE_DB`, and `TYPESAFE_API_KEY` and `SKRONGLE_AI_KEY` from Parameter
Store):

```sh
sudo systemd-run --unit=learnmap-check --uid=learnmap --gid=learnmap \
  -p EnvironmentFile=/etc/learnmap/env -p EnvironmentFile=/run/learnmap/env \
  -E SKRONGLE_DB=/var/lib/learnmap/learn.db -E SKRONGLE_DATA_DIR=/opt/learnmap/data \
  /opt/learnmap/learnmap content-check
sudo journalctl -u learnmap-check -f        # its log
sudo systemctl stop learnmap-check          # stop it
```

The course table on /superuser shows when the worker last checked in and
how many checks wait; a red ! means it has been silent for a day. Stop it
any time: a check it was in the middle of runs again after 10 minutes.
It doesn't come back after a reboot; start it again.

It has to run on the server's computer: the database is one SQLite file.
Running it on your own computer waits for the shared database (#581).

## If something goes wrong

- **`An error occurred (AccessDenied)`**: the IAM user is missing one of the
  policies in step 3, or the keys were pasted with a stray space.
- **"the hosted server refuses to start without these"**: a required setting
  in Parameter Store is missing or still `TODO`; the lines under it are the
  commands to set them (step 8).
- **The service keeps restarting, the log says `learnmap-env`**: the instance
  could not read Parameter Store. `sudo journalctl -u learnmap -n 20` shows
  the AWS error; usually the stack was created before the role existed (deploy
  again) or `AWS_SSM_PATH` in `deploy.env` is not where the settings are
  (`scripts/aws-env.sh list`).
- **`Stack … is in ROLLBACK_COMPLETE state`** (a first creation failed):
  delete it in the console (CloudFormation → the stack → *Delete*) or with
  `scripts/deploy-aws.sh --destroy`, read the *Events* tab for the reason,
  then deploy again. The usual causes are no default VPC (step 4) or a
  region where `t4g.nano` is not offered (change `AWS_INSTANCE_TYPE` or region).
- **ssh never connects**: the security group only allows your IP at deploy
  time; on a VPN or a network that changes IPs set `AWS_ADMIN_CIDR` to your
  network's range, or `0.0.0.0/0` to allow all (key-only login is still required).
- **"first boot of i-… did not finish"**: the instance's setup script
  stopped. The script prints `cloud-init status`, any out-of-memory kill and
  the end of `/var/log/learnmap-firstboot.log`. Fix the cause, run
  `sudo bash /var/lib/cloud/instance/scripts/part-001` on the instance and
  deploy again, or `--destroy` and deploy a fresh stack.
- **"could not pin the instance's ssh host key"**: the boot console showed
  neither a host key nor its fingerprint. Open the instance with EC2 Instance Connect, run
  `ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub`, put the `SHA256:…`
  value in `AWS_SSH_HOST_KEY_FINGERPRINT` and deploy again.
- **`Host key verification failed`**: the instance no longer has the key
  pinned in `deploy/aws/known_hosts.<instance-id>`. If you know why (you
  rebuilt it by hand), delete that file and deploy again; otherwise stop.
- **"SKRONGLE_SECRET_KEY in Parameter Store is not the key in … on the
  instance"**: the stack ran before the key moved to Parameter Store, and
  the value there is a different key. Copy the instance's key up
  ([Moving the key off the disk](#moving-the-key-off-the-disk)).
- **`--destroy` says "no backup, so no delete"**: the backup failed; the
  lines above it are the backup's log. A stack from before #964 has no
  bucket yet: deploy once, then destroy.
- **Site shows "waiting for the first deploy"**: Caddy is up but the rsync
  step did not finish; run the deploy again.
