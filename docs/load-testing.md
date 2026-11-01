# Load testing: finding the limits before a crowd does

The [scalability reports](scalability) ([AWS](scalability-aws)) found
their bottlenecks by reading code and pointing `ab` at one URL at a time.
That was enough to rank them, but nobody can repeat it. It doesn't catch
the next bottleneck a pull request adds, and it never tried a real mix of
learners. This page proposes a repeatable way to spike-test the app
ahead of time. It is a proposal: none of the files named below exist yet.

**Short version.** Use k6 (one `brew install`, nothing added to the
app). Write one scenario file that plays a learner the way the
reports describe one: sign up, open a course, read, answer every ~6 s,
leave after ~20 min. Run it in three places, each closer to
production than the last:

1. The Mac, against `./.dev-start --release`. Ten minutes. Finds code
   bottlenecks.
2. A Docker container on the Colima runners, with the AWS unit's CPU and
   memory limits. Thirty minutes. Gives Linux memory and small-box numbers.
3. A throwaway copy of the AWS stack, before a launch or press. An hour,
   under a dollar. Gives real TLS, Caddy, CPU credits and network.

Each run passes or fails on numbers written in the scenario file, so it
answers "did this get worse?" without anyone reading graphs. Setup takes
about two days.

## 1. Why not just `ab` again

| Need | `ab` | k6 |
|---|---|---|
| A different e-mail address per sign-up | no (one fixed body) | yes |
| A cookie jar per learner | no (one shared cookie) | yes, automatically per virtual user |
| A mix: pages, answers, finishes, sign-ups | one URL per run | weighted scenarios in one run |
| A spike: 0 → 3× peak in a minute | no | `ramping-arrival-rate` |
| Arrivals that keep coming when the server slows (as real users do) | no (closed loop) | yes (open model), so queues show up as queues |
| Pass/fail | no | `thresholds` set the exit code |
| Numbers per endpoint | no | tags, plus a JSON summary to diff |

The alternatives to k6:

- oha: keep it for single-URL micro-benchmarks. It is `ab` with
  better output.
- Locust: needs Python and a worker process per core.
- Artillery: adds an npm tree.
- Playwright (already in `e2e/`): one browser per learner, so a
  laptop manages a few dozen, not 1,500. It stays the tool for
  correctness.

k6 is one static binary, scenarios are plain JavaScript, and it can drive
a few thousand virtual users from one laptop.

## 2. What to add to the repo

All of it is dev-only. Nothing is compiled into the app or shipped.

| File | What it holds |
|---|---|
| `load/learner.js` | The virtual learner and visitor (section 3), and one k6 scenario per profile (section 4), chosen with `-e PROFILE=spike`. |
| `load/thresholds.js` | Pass/fail numbers per profile, in one place. |
| `scripts/load-test.sh` | `--where mac\|docker\|URL`, `--profile …`. Starts the target, seeds it, runs k6, samples the server while it runs (section 5), checks the server-side facts, and writes `load/results/<date>-<profile>.json`. |
| `load/baseline.json` | The last accepted results per profile and place, like `scripts/panic-baseline.txt`. The script prints the difference and fails on a >25 % p99 regression. |

**One change to the app**, needed only for the staging run.
The rate limits in `security.rs` are constants that only dev mode turns off.
Dev mode needs a loopback bind (`src/dev.rs`), so it can't serve a load
generator on another machine. Every request from that generator then
shares one address and hits `REGISTER_PER_IP` after 20 sign-ups. Add
`SKRONGLE_RATE_LIMITS=off`. It logs a warning at startup, and
`deploy-aws.sh` refuses to write it unless `AWS_STACK_NAME` contains
`load`. The change is about 20 lines.

## 3. The virtual learner

The traffic model comes from the first report's section 1, so test numbers
and report numbers mean the same thing.

A learner (75 % of arrivals) is one k6 virtual user each:

1. `GET /` (landing), `GET /register`, then submit the form with
   `res.submitForm()` and the address `load-<run>-<vu>-<iter>@example.test`.
   This exercises argon2, the outbox row and the session. Follow the Terms
   step if the app asks.
2. `POST /courses/open/<id>` for a random built-in course.<!-- themes --> 20 % of
   learners also pick a theme, spread evenly over all of them.<!-- /themes -->
3. Repeat for ~20 minutes: `GET` a lesson or `/practice` every ~30 s.
   Read the question ids from `data-qid` and `POST /api/answer` with
   `{question_id, answer}` every ~6 s (wrong answers cost the same). Every
   ~40 answers, finish a round.
4. Some learners come back: 30 % of them sign in again with `POST /login`
   (the second argon2 path).

A visitor (25 % of arrivals) loads three pages (`/`, a static asset,
`/register`) and leaves.

One setting scales the load: `-e LEARNERS=500` is the reports' peak hour.
Sleeps are randomised (±50 %), so 500 learners don't answer in lockstep.

## 4. The profiles

Each profile targets bottlenecks the reports found, and fails if the
bottleneck is still there. The thresholds are a starting point. Tune them
after the first green run on each target.

| Profile | Shape | Finds | Fails if |
|---|---|---|---|
| `smoke` | 10 learners, 1 min | the script itself is broken | any error |
| `peak` | ramp to 500 learners over 5 min, hold 15 min | #2 blocking threads; slow home page | answer p99 > 200 ms, page p99 > 500 ms, errors > 0.1 % |
| `spike` | 50 learners, then 1,500 within 60 s plus 10 sign-ups a second for 2 min, then back to 50 | argon2 queue with no backpressure; the 256-connection limit; time to recover | errors > 1 %; p99 still > 2× its pre-spike value 2 min after the spike |
| `signups` | 1,000 sign-ups over 10 min, nothing else | #1 mail ceiling; the e-mail lookups that scan `users` | **outbox rows ≠ sign-ups** (failed at 100 before #579; capped mail now waits), sign-up p99 > 1 s |
| `memory` | 50 learners<!-- themes -->, each wearing every theme on every course<!-- /themes -->; 20 open user-made courses | <!-- themes -->#3/#4 themed copies and the course cache<!-- /themes --><!-- no-themes -->the course cache<!-- /no-themes --> | RSS > the unit's `MemoryHigh`, or any restart |
| `history` | 50 learners seeded with 14,000 answers each, browsing home | #4 home page reads all history | home p99 > 300 ms |
| `ai` (Mac and Docker only) | 20 *Generate* presses within a minute, `SKRONGLE_LLM_DELAY_MS=60000` | #5 two AI slots | any refused start (today: 18 refused) |
| `soak` | 150 learners for 2 h | leaks, growing caches, hourly housekeeping stalls | RSS grows > 20 % after the first 15 min; p99 spikes on the hour |

`smoke`, `peak`, `spike` and `signups` cover the viral day. The rest target
specific findings and run when the code they test changes.

**Seeding.** The `history` profile needs learners with weeks of answers.
Writing 14,000 answers each through the API would take an hour, so the
script seeds them straight into SQLite before the run (copy `quiz_history`
rows from a template learner with shifted timestamps). It must stop the
app first and start it with `SKRONGLE_DEV_FORCE=1`, because dev mode refuses
a database with other accounts (#388).

## 5. What to watch on the server

The client sees only latency and status codes. While k6 runs, the script
also samples the server every 5 s, and it checks a few facts at the end.

| Signal | How (Mac / Docker / AWS) | Points at |
|---|---|---|
| RSS of the app | `ps -o rss` / `docker stats` / `systemctl show learnmap -p MemoryCurrent` | #3/#4 memory |
| Thread count | `ps -M` / `/proc/<pid>/status` | tutor threads vs `TasksMax` |
| CPU, credits | `top` / `docker stats` / CloudWatch `CPUUtilization`, `CPUCreditBalance` | burstable CPU (AWS #3) |
| Journal lines | `grep -c` for `not queued`, `rate limit hit`, `killed`, `Too many courses` | #1, #6, OOM, #5 |
| Outbox rows | `sqlite3 learn.db "select count(*) from mail_outbox where created_at >= <start>"` | #1: must equal sign-ups |
| Restarts | `systemctl show -p NRestarts` | any crash |
| Request times at the proxy | Caddy's access log, once AWS report #8 adds it | which path got slow |

**Reading a failure.** Each bottleneck has a recognisable shape:

| Shape | Likely cause |
|---|---|
| Every endpoint's p99 jumps together during sign-up bursts, CPU not full | blocking pool taken by argon2 (#2) |
| Home p99 grows with the `history` seed, answers stay flat | `day_streak` / `stats_since` (#4) |
| p99 climbs slowly and then a burst of 502s | memory limit and restart (#3) |
| Flat ~5 % CPU and everything slow, on AWS only | CPU credits ran out (AWS #3) |
| Latency fine, sign-ups fine, outbox count short | mail ceiling (#1) |
| Connect errors while latency is fine | 256-connection limit (`SKRONGLE_MAX_CONNECTIONS`) |

## 6. Where to run it

### 1. The Mac: every time a hot path changes

```sh
scripts/load-test.sh --where mac --profile peak
```

`./.dev-start --release --reset --background` with the simulated LLM, then
k6 on the same machine. Dev mode turns rate limits off and keeps all mail
in the dev mailbox. Dev mode also has no hourly mail ceiling unless one is
set, so the script sets `SKRONGLE_MAIL_PER_HOUR` to the production default.
Without it, `signups` would pass falsely.

The numbers are flattering (a fast laptop, no TLS). What counts is the
comparison with `load/baseline.json`, so a change that makes the home page
3× slower fails here, before review. Run it when a PR touches `src/db.rs`,
`src/auth.rs`, `routes/auth.rs`, `routes/pages.rs`, `routes/quiz.rs`,
`src/mail.rs`, `src/courses.rs` or `src/ai.rs`. The QA playbook
(`docs/qa.md`) can name it for those paths.

### 2. Docker on Colima: monthly, and before a release that changes limits

```sh
scripts/load-test.sh --where docker --size aws --profile spike
```

The runners already run arm64 Linux in Colima, the same architecture and OS
family as the Graviton instance. The script starts the release binary in a
container sized like the systemd unit:

| `--size` | Docker flags | Imitates |
|---|---|---|
| `aws` | `--cpus 2 --memory 192m --pids-limit 64` | `learnmap.service` on a t4g.nano |
| `aws-throttled` | `--cpus 0.1 --memory 192m` | a t4g.nano out of CPU credits |
| `pi` | `--cpus 0.25 --memory 64m --pids-limit 32` | the Pi's unit (roughly; it can't imitate an SD card) |

Dev mode answers only requests addressed to localhost, so k6 runs in a
second container that shares the app's network namespace
(`docker run --network container:skrongle-load grafana/k6 …`). This run
gives the Linux memory numbers both reports list as not measured.

Run it outside CI hours, one at a time. The runners share the Mac's disk,
and a full disk takes them all offline.

### 3. A throwaway AWS stack: before an expected spike

```sh
AWS_STACK_NAME=skrongle-load AWS_INSTANCE_TYPE=t4g.nano scripts/deploy-aws.sh
scripts/load-test.sh --where https://<ip>.sslip.io --profile spike
scripts/deploy-aws.sh --destroy      # with AWS_STACK_NAME=skrongle-load
```

This uses the same template, Caddy and unit as production, with
`SKRONGLE_RATE_LIMITS=off` and no mail provider (the `log` chain, so no real
e-mail is sent and the ceiling still applies). Run k6 from a second
instance in the same region, a `c7g.large` for an hour (~$0.07), so the
laptop's uplink doesn't become the bottleneck. Never point it at
production. Nothing stops the run there, and 1,500 fake accounts would
stay in the real database.

Only this run sees real TLS cost, Caddy's compression, CPU credits being
spent, and the instance's memory with the OS and Caddy in it. Watch
`CPUCreditBalance` in CloudWatch during `spike`. Run `peak` once on
`t4g.nano` and once on `t4g.small` to decide whether the event needs the
bigger box. A run costs well under a dollar (instance-hours plus a few GB of
traffic within the free allowance).

## 7. Why not on every pull request

CI runs on eight Docker runners on one laptop, alongside the e2e shards.
Load numbers taken there move with whatever else is running, so a
threshold either flaps or is too loose to catch anything. Instead:

- PRs that touch a hot path get the Mac run (an author's two-line note in
  the PR body: profile, p99 before and after).
- A weekly scheduled `peak` + `signups` in Docker, at night when the
  runners are idle, posts its result file to an issue and fails loudly
  against the baseline.
- The AWS run is on the checklist for any planned launch.

## 8. Getting started, in order

1. `brew install k6`. Write `load/learner.js` with the `smoke` and `peak`
   profiles, and run them against `./.dev-start --release`. (~½ day)
2. `scripts/load-test.sh` for the Mac: start, run, sample RSS, grep the
   journal, count outbox rows, write the result file, compare with the
   baseline. (~½ day)
3. Add `spike` and `signups`. `signups` should fail today on the mail
   ceiling, and that failure proves the harness works. (~2 h)
4. Docker mode with `--size`, and the first Linux memory numbers. (~½ day)
5. `SKRONGLE_RATE_LIMITS=off`, the guard in `deploy-aws.sh`, and one staging
   run before the next launch. (~½ day)
6. `memory`, `history`, `ai` and `soak` as the matching bottlenecks get
   fixed. Each one turns a fix into a regression test.
