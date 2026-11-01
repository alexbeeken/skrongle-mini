# Scalability: 10,000 users in a day

What would break first if Skrongle went viral and 10,000 people signed up in
one day (#354). Everything here is read from the code on `main` as of
2026-10-21 (after #459). The numbers come from a load probe on one laptop.
Open PRs that change a limit (#435, #437, #458) are named where they matter.

**Short version.** On the AWS box the app holds a day like this, as long as
five things change first. On the Pi Zero 2 W it holds the request rate, but
not its memory ceiling and not its Wi-Fi. The first problem users would
notice is not CPU. From the 101st sign-up in an hour, nobody gets a
verification e-mail.

## 1. The day in numbers

### Assumptions

| Quantity | Assumption | Peak hour |
|---|---|---|
| Sign-ups | 10,000 in 24 h; the peak hour takes 15 % | 1,500 (0.4/s, bursts of 2–3/s) |
| Visitors who don't sign up | 3 per sign-up, 3 pages each | ~4 page views/s |
| Learners online at once | each sign-up stays ~20 min | ~500 |
| Answer POSTs | one every ~6 s per learner (Sprint pace) | ~85/s |
| Page views (signed in) | one every ~30 s per learner | ~17/s |
| Dynamic requests in all | answers + pages + finishes | ~110/s on average, ~250/s in bursts |
| DB transactions per learner session | 1 sign-up (user, terms, session, token, outbox row), ~40 answers (one transaction each, 2–4 rows), 2–3 round finishes | ~45 per session, ~90/s at peak |
| Verification e-mails | 70 % sign up with a password (Google accounts arrive verified) | ~1,050 (7,000 a day) |
| AI course generations | 3 % try *Create a course → Generate* with their own key | ~45 starts (300 a day) |
| Storage | 50 answers × 131 B (measured, `quiz_history` plus its three indexes) + profile, progress and vocabulary rows, ~10 KB a learner | ~100 MB on day one |
| Bandwidth | first visit 170 KB uncompressed (HTML 21 KB, `style.css` 115 KB, JS 19 KB, icons); ~40 pages of ~22 KB and ~45 small JSON replies a session | ~1.1 MB a learner, ~11 GB a day, ~4 Mbit/s in the peak hour |

### Measured: one process on a laptop

`./.dev-start --release --reset --seed`, then ApacheBench (`ab -k`) on
loopback, on an Apple M5 Plus. The app uses only its own 2 async workers
and 3 blocking threads (`src/main.rs`); dev mode turns rate limits off, so
these are raw handler costs. One signed-in session throughout; 10,000
extra users and outbox rows were added before the login rows. **A Pi Zero 2 W is
roughly 20× slower** than this laptop (Cortex-A53 at 1 GHz, SD card), so the
last column divides by 20. The Pi was not measured.

| Path | Concurrency | Requests/s | p50 / p99 (ms) | Pi estimate (req/s) |
|---|---|---|---|---|
| `GET /` landing, signed out (no DB) | 50 | 37,800 | 1 / 4 | ~1,900 |
| `GET /` home, new learner | 50 | 8,520 | 5 / 14 | ~425 |
| `GET /` home, new learner | 200 | 3,640 | 54 / 68 | ~180 |
| `GET /lessons/{slug}` | 50 | 18,900 | 3 / 4 | ~950 |
| `GET /static/style.css` | 50 | 14,000 | 4 / 6 | ~700 (Wi-Fi binds first) |
| `POST /api/answer` (the grading path every mode uses) | 50 / 200 | 10,700 / 13,900 | 5 / 9, 14 / 18 | ~500 |
| `POST /login` (argon2id) | 10 | 269 | 35 / 66 | ~13–25 (60–120 ms a hash, `src/auth.rs`) |
| home **while 6 logins run** | 20 | **458** | **40 / 108** | ~25 |
| answer while 6 logins run | 20 | 3,670 | 6 / 12 | ~180 |
| home, learner with 3,000 answers | 50 | 2,730 | 18 / 39 | ~135 |
| home, learner with 14,000 answers | 50 | 695 | 69 / 121 | ~35 |
<!-- themes -->
| RSS: boot → all 6 themed course copies built | — | 12 MB → 54 MB | — | over the Pi unit's `MemoryHigh=48M` |
<!-- /themes -->

So the request rate itself (~250/s at worst) fits on the Pi with a little
room, and fits easily on anything bigger. The bottlenecks are elsewhere.

## 2. Bottlenecks, ranked

Ranked by when each one bites on a viral day, not by how hard it is to fix.

### 1. The hourly e-mail ceiling drops verification mail

- **Where:** `src/mail.rs`, `DEFAULT_PER_HOUR = 100`, checked by `over_cap`
  in `enqueue`. `verify::send` uses `try_queue`, and a capped message is
  never queued, not even later.
- **What breaks, and when:** sign-up number 101 in any hour, which is about
  1 % of the day. Every later sign-up gets no verification link. The ceiling
  covers all purposes together, so e-mailed sign-in codes (`two_factor`,
  through `Mailer::queue`) can't be sent (the code page says so) and
  course-share invites fail with an error. The mail provider's plan is the next
  wall. Free tiers allow a few hundred messages a day, and this day needs
  ~7,000.
- **How you'd notice:** `mail (email-verify) not queued: SKRONGLE_MAIL_PER_HOUR
  reached` and `no verification e-mail for user N: a mail cap said no` in
  the journal. Learners report that "the e-mail never came".
- **Cheapest fix:** `SKRONGLE_MAIL_PER_HOUR=3000` in the env file, and a
  provider plan sized for ~10k a day. A fallback provider is already
  supported (`SKRONGLE_MAIL_PROVIDERS=a,b`).
- **Proper fix:** separate ceilings per purpose (sign-in codes never wait
  behind verification mail). A capped verification is queued with a later
  `next_attempt_at` instead of dropped. Index `mail_outbox(created_at)`;
  both cap counts scan the whole table today (measured 0.3–1 ms at 10,000
  rows on the laptop, so ~6–20 ms on the Pi, inside the DB lock).

### 2. Password hashing and the database share three threads

- **Where:** `src/main.rs` builds the runtime with
  `max_blocking_threads(3)`. Every `Db::call` (`src/db.rs`) and every
  argon2 hash or verify (`routes/auth.rs`, `routes/profile.rs`) is a
  `spawn_blocking` on that pool. So are Google's code exchange
  (`routes/auth.rs`, a network round trip) and Stripe calls
  (`routes/billing.rs`).
- **What breaks, and when:** three sign-ins at the same moment take every
  blocking thread. Then every page and every answer waits behind them.
  Measured: six concurrent logins cut the signed-in home page from 8,520/s
  (p50 5 ms) to 458/s (p50 40 ms, p99 108 ms), an 18× drop. On the Pi a
  hash takes 60–120 ms, so the peak's bursts of 2–3 sign-ups a second add
  ~100 ms stalls to everyone; past ~25 a second the queue grows without
  bound (`spawn_blocking` has no backpressure, so nothing is refused).
  A slow Google token endpoint holds a thread for its whole timeout.
- **How you'd notice:** nothing in the app's log. Request times in the
  proxy's access log (Caddy logs `duration`; nginx needs `$request_time` in
  its `log_format`) rise during sign-up bursts.
- **Cheapest fix:** raise `max_blocking_threads` to ~8, and put a
  `tokio::sync::Semaphore` with 2 permits around hashing and the outbound
  HTTP calls, so SQLite always has a free thread.
- **Proper fix:** a dedicated hashing thread with a bounded queue that
  answers "busy, try again" (503) when it is full. Google and Stripe calls
  move to their own threads, like the AI calls already do.

### 3. Memory: the Pi's unit ceiling is 64 MB

- **Where:** `deploy/skrongle.service` sets `MemoryHigh=48M` and
  `MemoryMax=64M` (the AWS unit: 128M/192M). <!-- themes -->`Courses::get_themed`
  (`src/courses.rs`) caches a full themed copy of a course for every
  (course, theme) pair anyone wears. <!-- /themes -->On `main` the cache of user-made
  courses has no bound; #437 caps it at 24 MB. Tutor chats
  (`src/tutor.rs`, up to 80 entries each, dropped after 2 h idle) and 256
  connections' buffers come on top.
- **What breaks, and when:** <!-- themes -->this needs no load at all. Six learners
  wearing the three themes on Spanish and French made the process grow
  from 12 MB to 54 MB (measured; each themed Spanish copy is ~7–13 MB).<!-- /themes --><!-- no-themes -->as user-made courses fill the cache.<!-- /no-themes --> At
  `MemoryHigh` the kernel throttles and reclaims, so every request gets
  slower. At `MemoryMax` it kills the process, and the restart loses what
  lives in memory: running AI jobs, tutor chats, games, Google sign-ups at
  the Terms step, rate-limit windows.<!-- themes --> Expect it in the first hour.
  (RSS measured on macOS; Linux was not checked.)<!-- /themes -->
- **How you'd notice:** `journalctl -u skrongle` shows "A process of this
  unit has been killed by the OOM killer", and `systemctl status skrongle`
  shows memory at the limit. Learners see a 502 from nginx while the
  service restarts.
- **Cheapest fix:** raise the Pi's limits to `MemoryHigh=160M`,
  `MemoryMax=256M` (the board has 512 MB), and merge #437.
- **Proper fix:** <!-- themes -->a themed copy shares the plain store's lessons and keeps
  only what the theme adds, or the themed cache becomes an LRU with a byte
  budget, as #437 does for user-made courses.<!-- /themes --><!-- no-themes -->an LRU with a byte budget for user-made courses (#437).<!-- /no-themes -->

### 4. One SQLite connection, and the home page reads all of a learner's history

- **Where:** `Db` is one `Connection` behind a `Mutex` (`src/db.rs`), so
  every read and write in the app runs one at a time. The home page
  (`routes/pages.rs`) calls `sprint::day_streak`, which reads every answer
  and game the learner ever made in the course. It also calls
  `mastery::stats_since` twice (today, this week), which does a table
  lookup per row.
- **What breaks, and when:** not on day one. A new learner's home costs
  ~0.12 ms of lock time on the laptop. It grows with the learner's
  history: 2,730/s at 3,000 answers and 695/s at 14,000 (measured), so
  ~1.4 ms (laptop) or ~30 ms (Pi) per view for a keen learner a few weeks
  in. The lock tops out near ~500 simple requests a second on the Pi, and
  200 concurrent clients already halve home throughput on the laptop. WAL
  checkpoints (`wal_autocheckpoint = 500`) hit the SD card inside whichever
  request commits. The hourly housekeeping runs one background job per
  purge (`src/jobs.rs`, #872), so it holds the lock in short pieces, but
  `wal_checkpoint(TRUNCATE)` is still one of them.
- **How you'd notice:** home p99 in the proxy log climbs week by week
  while answers stay fast. Short stalls on the hour.
- **Cheapest fix:** bound `day_streak` to `answered_at > now − 400 days`
  (it keeps 400 days but reads all first), and add `was_correct` to the
  `(user_id, course_id, answered_at)` index for `stats_since`.
- **Proper fix:** a per-day activity table, written in the answer's
  transaction, for the streak and "today/this week". Later, a few
  read-only connections beside the one writer (WAL allows it; no new
  crate).

### 5. Two AI generation slots for the whole server

- **Where:** `src/ai.rs`, `MAX_JOBS = 2` and `check_capacity`. A job runs
  one model request for the outline and one per lesson (6 by default,
  15 at most), on its own OS thread, so it takes minutes.
- **What breaks, and when:** the third learner who presses *Generate*
  while two jobs run gets "Too many courses are being generated right now;
  try again in a few minutes." At ~45 starts in the peak hour and ~5
  minutes a job, two slots serve about 24 an hour, so roughly half are
  turned away. The tutor has the opposite problem on `main`: every turn
  spawns an OS thread with no cap. #458 caps it at 4 tutor calls server
  wide, which then becomes the same kind of bottleneck.
- **How you'd notice:** nothing is logged when a start is refused, so only
  complaints would tell you.
- **Cheapest fix:** make the cap a setting (`SKRONGLE_AI_JOBS`), set it to ~8
  on AWS, and log each refusal. A job is a thread waiting on HTTP. The
  model calls use the learner's own key, so slots cost memory, not money.
- **Proper fix:** a queue. The dialog shows "you're third in line" and the
  job starts when a slot frees, instead of a refusal. Merge #435 first:
  it makes slot-taking atomic and adds the per-account job budget.

### 6. Per-address sign-up limits hit shared networks

- **Where:** `src/security.rs`: `REGISTER_PER_IP` is 20 an hour,
  `LOGIN_PER_IP` 30 per 10 minutes, kept in memory and keyed by the
  client address (the last `X-Forwarded-For` entry behind the proxy).
- **What breaks, and when:** viral spread comes in clusters: a class, an
  office, a mobile carrier's NAT address shared by thousands of phones.
  The 21st sign-up from one address in an hour is refused. One classroom
  can hit it.
- **How you'd notice:** `registration rate limit hit from <ip>` and
  `sign-in rate limit hit for an account from <ip>` in the journal,
  repeated for the same few addresses.
- **Cheapest fix:** raise both limits about 10×. The per-account
  wrong-password limit (`LOGIN_FAILURES_PER_USER`) still stops guessing.
- **Proper fix:** key IPv6 limits on the /64, and past the per-address
  limit ask for a cheap proof (a small proof-of-work or a captcha)
  instead of refusing.

### 7. The Pi's network and the missing compression

- **Where:** the Pi Zero 2 W has 2.4 GHz 802.11n Wi-Fi only, sitting on a
  home uplink. `deploy/nginx-spanish.conf` turns on no `gzip_types`
  (Debian's default compresses `text/html` only), so the 115 KB
  stylesheet goes out uncompressed. The AWS Caddyfile already has
  `encode zstd gzip`.
- **What breaks, and when:** the peak hour needs ~4 Mbit/s on average and
  several times that in bursts, near the Pi's real Wi-Fi throughput and
  many home upload speeds. Pages get slow before the CPU is busy.
- **How you'd notice:** slow first loads while `top` on the Pi is idle.
- **Cheapest fix:** `gzip_types text/css application/javascript
  image/svg+xml;` in nginx (static files are already cached for a year),
  or serve a viral day from AWS.
- **Proper fix:** a CDN in front of `/static/`<!-- themes --> and `/themes/`<!-- /themes -->.

### 8. Lookups that scan a whole table

- **Where:** `auth::find_user_credentials` (every login) and
  `profile::email_taken` (twice per sign-up) use `lower(email) =
  lower(?)`, which can't use `idx_users_email` (`EXPLAIN QUERY PLAN`:
  `SCAN users`). The outbox caps scan `mail_outbox` (see #1).
- **What breaks, and when:** nothing at 10,000 users (0.4 ms a lookup on
  the laptop, ~8 ms on the Pi, inside the lock), but it grows linearly:
  at 100,000 users a sign-up holds the Pi's lock for ~¼ s.
- **Cheapest fix:** expression indexes, `CREATE INDEX … ON users(lower(email))`
  and `ON mail_outbox(created_at)`, in one migration.
- **Proper fix:** the same indexes, plus storing addresses lowercased so
  plain equality works.
- **Done for `users`** (#604, #605, #606, v63): `idx_users_email_lower`
  serves every lookup by address, all written `lower(email) = lower(?)`;
  course shares are found by `user_id` or their stored lower-case address
  (#621, #622). `mail_outbox(created_at)` is still open.

## 3. What is already fine

- **The listener** (#459, `src/server.rs`): 256 connections at most, 15 s
  for headers, 30 s idle bodies, 64 KiB bodies outside the editor. A body
  of 64 KiB or less is read whole before the handler runs
  (`routes::read_small_body`, #1063), so a handler that ignores it does not
  cost the connection. Slow clients wait in the backlog; at the limit it logs `256 connections open
  (SKRONGLE_MAX_CONNECTIONS); new ones wait`.
- **Static files** are compiled into the binary, carry an ETag and
  `Cache-Control: public, max-age=31536000, immutable`, and their URLs
  carry a content hash (`?v=`). Repeat visits cost nothing.
- **Sessions:** one indexed query per request (`auth::user_for_session`),
  with no write per request because the TTL is fixed, not sliding. Expired
  sessions are purged hourly.
- **Grading:** `POST /api/answer` is one transaction and scales with
  concurrency (10,700/s at 50, 13,900/s at 200 on the laptop). With WAL
  and `synchronous = NORMAL`, a commit doesn't wait for a disk flush.
- **E-mail and texts never block a request:** handlers write an outbox
  row; the job runner thread sends them with fallback, cooldown and
  retries. One thread at ~300 ms a message manages ~12,000 an hour. Only
  two-factor codes send texts, with per-account and per-number limits.
- **Rate-limit memory** is bounded (`MAX_KEYS = 20,000`, oldest windows
  dropped first).
- **AI work** runs on its own OS threads with the learner's own keys: no
  model spend for the operator.
- **Built-in courses** load once at boot; lesson pages render from memory.
- **Storage** is small: ~131 bytes an answer, ~100 MB for the whole day.
  The AWS disk (8 GB) holds months of it. Finished outbox rows are deleted
  after 30 days. #458 adds a two-year default retention for answer
  history (on `main` it's kept forever).

## 4. The five changes worth doing first

| # | Change | Fixes | Effort |
|---|---|---|---|
| 1 | Set `SKRONGLE_MAIL_PER_HOUR=3000`, buy a mail plan for ~10k/day, add a fallback provider. Then per-purpose ceilings, and queue capped verification mail for later instead of dropping it. | #1 | 15 min config; ~½ day code |
| 2 | `max_blocking_threads(8)` plus a 2-permit semaphore around argon2 and the Google/Stripe calls; log request durations at the proxy. | #2 | ~½ day |
| 3 | Pi unit memory limits to 160M/256M, merge #437<!-- themes -->, then an LRU (or overlay) for themed course copies<!-- /themes -->. Better still, serve a viral day from AWS, not the Pi. | #3, #7 | 10 min<!-- themes -->; 1–2 days for the cache<!-- /themes --> |
| 4 | Merge #435 and #458, make `MAX_JOBS` a setting (`SKRONGLE_AI_JOBS`, ~8 on AWS), log refused starts, raise the per-address sign-up limits about 10×. | #5, #6 | ~½ day |
| 5 | One migration: indexes on `lower(email)` and `mail_outbox(created_at)`, `was_correct` in the history index, and `day_streak` bounded to 400 days. | #4, #8 | ~1 day |

### Not measured

- The Pi and the AWS t4g.nano themselves: the 20× Pi factor is an
  estimate, and the t4g.nano sits somewhere in between.
- SD-card write latency and checkpoint stalls; TLS in Caddy/nginx; the
  Pi's Wi-Fi; real mail-provider and LLM latencies (dev mode fakes both).
- Sign-up itself: `ab` can't vary the address, so `POST /login` (the same
  argon2 cost) stood in.
- Many distinct sessions: the probe used one learner, so caches were
  warmer than a crowd's. Memory on Linux (RSS is from macOS).
