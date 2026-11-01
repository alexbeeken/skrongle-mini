# Scalability on AWS: 10,000 users in a day

The [scalability report](scalability) asked what breaks first if 10,000
people sign up in one day, mostly with the Pi in mind. This page asks the
same question for the AWS deployment only: the stack that
`scripts/deploy-aws.sh` creates from `deploy/aws/cloudformation.yaml`.
The traffic assumptions are the same (section 1 of that report): ~500
learners online in the peak hour, ~110 dynamic requests a second on
average and ~250 in bursts, 1,500 sign-ups in the peak hour, ~7,000
verification e-mails and ~300 AI generations over the day.

Everything here is read from the code and deploy files on `main` as of
2026-10-21. **Nothing was measured on an instance.** The AWS figures are
estimates from the laptop probe in the first report, and they say so where
they appear.

**Short version.** One `t4g.nano` has plenty of CPU and bandwidth for the
day, as long as the account keeps T4g's default *unlimited* CPU credits.
The day costs about a dollar more than a normal day. What goes wrong is the
same as on the Pi: the 100-an-hour mail ceiling, three threads shared by
password hashing and the database, and two AI slots. AWS adds risks of its
own. Everything runs on one box with no backups, so losing the instance
loses every new account. The box has 512 MB and a 1 GB swap file on its disk. And nothing
alerts you or records request times. Resizing to a `t4g.small` for the
event takes one setting and about two minutes of downtime.

## 1. What the stack is

| Piece | What it is | Where |
|---|---|---|
| Instance | One `t4g.nano`: 2 Graviton2 vCPUs (burstable, 5 % baseline each), 512 MiB RAM, 1 GB swap file made at first boot (#1362) | `cloudformation.yaml`, `AWS_INSTANCE_TYPE` |
| Disk | 8 GB gp3 root volume (3,000 IOPS, 125 MB/s baseline). Holds the OS, the binary and `learn.db`. Deleted with the stack. | `VolumeSize`, `DeleteOnTermination: true` |
| Proxy | Caddy: HTTPS (Let's Encrypt), `encode zstd gzip`, `reverse_proxy 127.0.0.1:8080`. No `log` directive, so no access log. | Caddyfile written by `deploy-aws.sh` |
| App | `learnmap.service`: `MemoryHigh=128M`, `MemoryMax=192M`, `TasksMax=64`, 2 async workers, 3 blocking threads | `deploy/aws/learnmap.service`, `src/main.rs` |
| Network | One Elastic IP, default VPC, one Availability Zone. ~32 Mbit/s baseline bandwidth, bursting to 5 Gbit/s. | `StaticIp` |
| Backups, alarms, logs off the box | None | — |
| Cost | ~$7.40 a month (`docs/aws-setup.md`) | |

## 2. The day on one t4g.nano

Estimates, not measurements. A Graviton2 core is taken to be roughly 3–5×
slower than the laptop's (an Apple M5 Plus). The app uses only two cores for
async work either way (`worker_threads(2)`), so the laptop figures are
divided by ~4.

| Path | Laptop (measured) | t4g.nano (estimate) | Peak-hour need |
|---|---|---|---|
| `POST /api/answer` | 10,700/s | ~2,500/s | ~85/s |
| `GET /` home, new learner | 8,520/s | ~2,000/s | ~17/s, plus landing views |
| `GET /lessons/{slug}` | 18,900/s | ~4,500/s | (included above) |
| argon2 hash (sign-up, login) | ~35 ms at concurrency 10 | ~30–50 ms each, ~40–60/s on both cores | 0.4/s, bursts of 2–3/s |

**CPU.** The app and Caddy (TLS handshakes, compression) need about
15–25 % of the two vCPUs in the peak hour and well under 10 % on average.
That is above the 5 % baseline, so the instance spends CPU credits. T4g
instances start in **unlimited** mode unless the account's default was
changed. In that mode they never slow down, and use above the baseline
costs $0.04 per vCPU-hour, which comes to cents for this day. In
**standard** mode a nano earns 6 credits an hour and holds at most 144. A
busy morning spends them, and then the instance is held to 5 % of each
core. That is about a twentieth of the speed, and the day would not fit.

**Memory.** 512 MiB in all, plus a 1 GB swap file on the gp3 disk. Amazon Linux 2023 and its services
take roughly 150–200 MB, and Caddy 30–60 MB under TLS load. That leaves
the app's 128 MB soft and 192 MB hard limit with some room.<!-- themes --> The app
measured 54 MB on macOS with every built-in themed course copy loaded.
(Linux was not measured.)<!-- /themes -->

**Disk.** ~90 transactions a second at peak, with WAL and
`synchronous = NORMAL`, is far below gp3's 3,000 IOPS. ~100 MB of new data
a day fits the 8 GB volume for months.

**Bandwidth.** ~4 Mbit/s in the peak hour before compression, less after
Caddy's `encode`. This is well inside the nano's 32 Mbit/s baseline. The
first report's Wi-Fi bottleneck (#7) does not exist here.

**Cost of the day** (us-east-1 prices, rounded):

| Item | Normal day | Viral day |
|---|---|---|
| t4g.nano ($0.0042/h) | $0.10 | $0.10 |
| CPU above the baseline (unlimited mode) | $0 | ~$0.10–0.30 |
| Elastic IP, 8 GB gp3 | $0.14 | $0.14 |
| Data out (~11 GB uncompressed, ~4–6 GB compressed) | $0 | $0 within the 100 GB/month free allowance, then $0.09/GB |
| **or** a t4g.small for the event ($0.0168/h) | | +$0.30 |

The bill is small. A month of learners who keep coming back could
use more than the 100 GB free allowance, which costs a few dollars, not
tens.

## 3. Bottlenecks, ranked

Ranked by when each one bites on a viral day. Numbers 1, 2, 5 and 6 are the
first report's bottlenecks, unchanged by AWS. They are summarised here;
the fixes are in that report.

### 1. The hourly e-mail ceiling drops verification mail

Same as the first report's #1, and still the first thing users notice:
from the 101st sign-up in an hour, nobody gets a verification e-mail
(`src/mail.rs`, `DEFAULT_PER_HOUR = 100`, capped messages are dropped).
AWS changes nothing here. The app sends through Brevo, Postmark or Resend
over HTTPS, so EC2's block on outbound port 25 does not matter.

- **Cheapest fix:** `SKRONGLE_MAIL_PER_HOUR=3000` in `/etc/learnmap/env` (so
  in `deploy-aws.sh`'s env file), and a provider plan for ~10,000 messages
  a day. Amazon SES is not one of the supported providers. Adding it would
  be new code, and a new SES account starts in a sandbox that takes about a
  day to leave, so don't start that the week of the event.

### 2. Password hashing and the database share three threads

Same as the first report's #2 (`max_blocking_threads(3)` in
`src/main.rs`). Graviton hashes faster than the Pi does, so the stalls
are shorter (estimate ~30–50 ms instead of 60–120 ms), but three
simultaneous sign-ins still leave every page and answer waiting.
Measured on the laptop, six concurrent logins cut home-page throughput
18×. With only two vCPUs, more threads would not make hashing faster.
They would only keep SQLite from waiting behind it, which is the point of
the fix: ~8 blocking threads, and a 2-permit semaphore around argon2 and
the Google and Stripe calls.

### 3. Burstable CPU: one account setting decides the day

- **Where:** `t4g.nano` in `cloudformation.yaml`. The template sets no
  `CreditSpecification`, so the instance gets the account default for
  T4g, which is `unlimited` unless someone changed it.
- **What breaks, and when:** in `standard` mode, the moment the credit
  balance reaches zero, probably in the first busy hours. CPU is then
  capped at 5 % a core, and every request, TLS handshake and hash runs
  about 20× slower. Pages time out under the peak's ~250 requests a
  second.
- **How you'd notice:** CloudWatch's `CPUCreditBalance` for the instance
  falls to 0 and `CPUUtilization` sits flat at 5 %. These metrics are
  free, at 5-minute resolution.
- **Cheapest fix:** check it now, with
  `aws ec2 describe-instance-credit-specifications --instance-ids <id>`.
  If it says `standard`, run
  `aws ec2 modify-instance-credit-specification --instance-credit-specification "InstanceId=<id>,CpuCredits=unlimited"`.
- **Proper fix:** set `CreditSpecification: { CPUCredits: unlimited }` in
  the template, so a recreated stack can't come back in standard mode.

### 4. 512 MB, a small swap file, and a unit limit sized for the OS

- **Where:** `t4g.nano` has 512 MiB. Amazon Linux has no swap by default; first boot adds a 1 GB `/swapfile` (#1362), because
  dnf alone ran the box out of memory.
  `deploy/aws/learnmap.service` sets `MemoryHigh=128M` and
  `MemoryMax=192M`. The memory growers are <!-- themes -->the first report's #3: a
  full themed copy of a course for each (course, theme) pair anyone wears
  (`Courses::get_themed`), <!-- /themes -->the cache of user-made courses (no bound on
  `main`, 24 MB with #437), tutor chats, and 256 connections' buffers.
- **What breaks, and when:** probably not with the built-in courses alone.<!-- themes -->
  All six themed copies measured 54 MB.<!-- /themes --> But user-made courses<!-- themes --> and themed
  copies of them<!-- /themes --> grow without a bound on `main`. At 128 MB the kernel
  throttles the app. At 192 MB it kills it, and the restart loses
  in-memory state: running AI jobs, tutor chats, games, Google sign-ups at
  the Terms step, rate-limit windows. The swap file gives the box as a whole
  room before the kernel's OOM killer picks a process (it can pick Caddy, and
  then the site is down until it restarts); a box that lives in swap is slow.
- **How you'd notice:** `journalctl -u learnmap` shows the OOM kill and a
  restart. Learners see Caddy's 502 for a few seconds. CloudWatch has no
  memory metric without its agent.
- **Cheapest fix:** for the event, `AWS_INSTANCE_TYPE=t4g.small` (2 GiB,
  $0.0168/h) and `scripts/deploy-aws.sh`. CloudFormation stops and starts
  the instance; the Elastic IP and the disk stay. Plan for ~2 minutes of
  downtime at a quiet hour. Raise the unit to `MemoryHigh=512M`,
  `MemoryMax=768M`, and merge #437.
- **Proper fix:** <!-- themes -->the first report's #3: themed copies share the plain
  store's lessons, or an LRU cache with a byte budget.<!-- /themes --><!-- no-themes -->an LRU cache of user-made courses with a byte budget.<!-- /no-themes -->

### 5. One box: an outage loses the day's sign-ups

- **Where:** `learn.db` lives on the root volume, which has
  `DeleteOnTermination: true`. Since #964 a nightly `sqlite3 .backup` goes
  to an S3 bucket the stack keeps on delete, and a nightly EBS snapshot is
  kept 14 days ([AWS setup, Backups](aws-setup#backups)). The stack is one
  instance in one Availability Zone.
- **What breaks, and when:** a failed instance, a bad deploy or a
  corrupted database loses every account made since the last nightly
  backup, up to a day's sign-ups. AWS recovers a failed instance onto new
  hardware for most current types, and the volume survives that.
- **How you'd notice:** `StatusCheckFailed` in CloudWatch. The site is
  down.
- **Cheapest fix:** before a big day, `scripts/deploy-aws.sh --backup` by
  hand, and again after it.
- **Proper fix:** an hourly backup timer instead of a nightly one (the
  same unit, `OnCalendar=hourly`), or streaming the write-ahead log to S3
  (Litestream) if minutes matter.

### 6. Two AI generation slots, and a thread limit for the tutor

Same as the first report's #5: `MAX_JOBS = 2` in `src/ai.rs` turns
away roughly half of the peak hour's ~45 *Generate* presses. On AWS the
cheap fix is the real fix. Jobs are threads waiting on the learner's own
API key, and a `t4g.small` has memory for ~8.

The AWS unit adds one limit: `TasksMax=64`. The app's fixed threads take
about ten (workers, blocking pool, the job runner, AI jobs). Every tutor
turn on `main` spawns its own OS thread (`src/tutor.rs`). So past ~50
tutor replies at once, the next learner gets the tutor's "thread failed"
error. #458's cap of 4 tutor calls makes that unreachable.

### 7. Per-address sign-up limits hit shared networks

Same as the first report's #6. Caddy appends the real client address to
`X-Forwarded-For`, which `security::client_ip` reads. So a classroom or
a carrier's shared address still hits `REGISTER_PER_IP` (20 an hour)
after 20 sign-ups. Raise it about 10×.

### 8. Nothing tells you any of this is happening

- **Where:** the Caddyfile has no `log` directive, so no request is logged
  and no request time is kept. The app logs to journald on the instance
  only. The stack creates no CloudWatch alarms, and the deploy user can't
  create any (it has EC2, CloudFormation and SSM read-only).
- **What breaks, and when:** nothing breaks, but #2, #3 and #4 are only
  visible as "it feels slow" until a learner complains. Afterwards there is
  no record of how slow, for how long, or which path.
- **Cheapest fix:** add `log { output file /var/log/caddy/access.log
  { roll_size 50MiB roll_keep 5 } format json }` to the Caddyfile in
  `deploy-aws.sh`. Each line then carries `duration` and `status`. Watch
  `CPUCreditBalance`, `CPUUtilization` and `StatusCheckFailed` in the EC2
  console's *Monitoring* tab during the day.
- **Proper fix:** two CloudWatch alarms in the stack (`StatusCheckFailed`
  for 2 minutes, `CPUUtilization` > 80 % for 10 minutes) that e-mail you
  through SNS. Add `CloudWatchFullAccess` and `AmazonSNSFullAccess` to the
  deploy user.

### Later, not on day one

- **The home page reads all of a learner's history** (the first report's
  #4) and **e-mail lookups scan the users table** (#8). On Graviton they
  hurt about 5× less than on the Pi. They are still worth the one
  migration, weeks after the day.
- **Deploys restart the app.** `deploy-aws.sh` runs `systemctl restart`:
  a few seconds of 502, and in-memory state is lost. Don't release during
  the peak.
- **The ceiling of one box.** SQLite is one file with one writer, and AI
  jobs, tutor chats, games and rate limits live in memory, so the app
  can't run on two instances behind a load balancer. It doesn't need to.
  Bigger instances scale it, but only up to 2 vCPUs until
  `worker_threads(2)` becomes a setting. A non-burstable `c7g.large`
  (2 vCPU, 4 GiB) removes the credit question for ~$53 a month.

## 4. What AWS already does well

- **No Wi-Fi, no home uplink.** The Pi's network bottleneck is gone, and
  Caddy compresses every text response (`encode zstd gzip`).
- **Automatic HTTPS.** Caddy renews certificates and serves HTTP/2, which
  helps a first visit's ~170 KB of assets.
- **Resizing is one setting.** `AWS_INSTANCE_TYPE` plus a deploy. The
  Elastic IP, the disk and the database stay.
- **The disk is fast enough.** gp3's 3,000 IOPS is about 30× the peak's
  write rate, so the Pi's SD-card stalls don't apply.
- **Cost stays small.** A viral day adds about a dollar, or $0.40 with a
  `t4g.small` for the day.

## 5. The five changes worth doing first on AWS

| # | Change | Fixes | Effort |
|---|---|---|---|
| 1 | `SKRONGLE_MAIL_PER_HOUR=3000` and a mail plan for ~10k/day. Then per-purpose ceilings, and queue capped mail instead of dropping it (first report). | #1 | 15 min config; ~½ day code |
| 2 | Check the CPU credit mode (set `unlimited`, and put it in the template). Move to `t4g.small` for the event and raise the unit to 512M/768M. Merge #437. | #3, #4 | 30 min |
| 3 | A nightly SQLite backup off the box and a snapshot right before the event. Then a DLM snapshot policy in the stack. | #5 | 1 h; ½ day for DLM |
| 4 | A Caddy access log with durations. Then CloudWatch alarms for status checks and CPU. | #8 | 15 min; ½ day for alarms |
| 5 | `max_blocking_threads(8)` and a semaphore around argon2. `SKRONGLE_AI_JOBS` (~8) and a log line for refused starts. Raise the per-address limits 10×. Merge #435 and #458. | #2, #6, #7 | ~1 day |

To find these limits before a real crowd does, run the spike tests in
[Load testing](load-testing) against a copy of this stack.

Follow-up issues:
- #579: the e-mail limit.
- #578: a self-hosted mode on SQLite and a hosted mode on Postgres.
- #580: a background job framework.
- #581: several servers on one database.
- #584: renaming settings to `SKRONGLE_`.

### Not measured

- Any of it on an instance: the throughput column is the laptop's numbers
  divided by ~4, and the memory split between the OS, Caddy and the app is
  typical for Amazon Linux 2023, not read off a box.
- Caddy's TLS and compression cost under load, and Let's Encrypt issuance
  on a fresh stack.
- Real mail-provider and LLM latency.
- Prices are us-east-1 list prices as of 2026-10. Other regions differ by
  cents.
