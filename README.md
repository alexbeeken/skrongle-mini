<picture>
  <source media="(prefers-color-scheme: dark)" srcset="static/logo-dark.svg">
  <img src="static/logo.svg" alt="Skrongle" width="176" height="40">
</picture>

# Skrongle

*Courses are folders of JSON files. Self-hosted, one small binary.*

A lightweight, self-hosted learning app for lessons, quizzes and short practice sessions,
driven by JSON courses. It started as a Spanish app; Spanish and French courses
ship with it, and any subject that fits *units → lesson groups → lessons*
can be a course: pick a ready-made one, build your own from files, or describe
what you want to learn and let an LLM write it.

Every feature reads the same course files:

| | |
|---|---|
| **Lessons** | Lesson pages with vocabulary and paradigm tables |
| **Quizzes** | adaptive micro-quizzes with per-lesson mastery, decay and targeted practice |
| **Guide** | the short-attention-span version: each lesson as a stage of small steps, stars and XP |
| **Sprint mode** | phone-first five-minute sessions (sort / spot / build / ladder / pick / match, and listening: tap / fill in what you hear, what do you hear, select the matching audio, listen and match), an audio switch, streaks, lightning round |
| **Games** | Word Match with a shared leaderboard; a drill for paradigm tables (verb conjugations…) |
| **Map** | every lesson of every unit with your mastery |
| **Vocabulary** | terms collected by answering questions, studied as flashcards |
| **History** | every answer and game |

It is a single Rust binary (SQLite and all templates/CSS/JS/docs compiled in;
courses are read from `data/`)
sized for a **Raspberry Pi Zero 2 W** and built on an Apple Silicon Mac.

```
~3.5 MB binary · ~15-25 MB RSS · 2 async workers · 1 SQLite connection (WAL)
```

## Skrongle mini

**Skrongle mini** is the release for small boxes: one binary, set up
for a handful of people who know whoever runs it (a family, a
class, a few friends) with no outside services. Download the
[release](https://github.com/alexbeeken/skrongle-mini/releases) tarball for your
machine (`skrongle-<version>-aarch64.tar.gz` for a Pi, `-x86_64` for a PC),
install `skrongle.service`, and the first account you make is the owner's;
everyone else joins with an invitation link from your profile. Step by step:
the app's *Host it yourself* page (`/self-host`, linked from `/docs`), or
[docs/self-hosting.md](docs/self-hosting.md) for the details.

| | |
|---|---|
| Who signs up | Only people the owner invites (`SKRONGLE_SIGNUP=invite`) |
| E-mail | None: links are shown in the page |
| Terms of Service | Yours (`SKRONGLE_TERMS=/etc/skrongle/terms.md`) |
| Background work | In process, in SQLite: no Redis or queue service |
| Database | SQLite |
| Securing the login | You: a strong password, two-factor, Tailscale or a VPN, never a forwarded port |

## Documentation

The documentation lives in `docs/` and is served by the app at `/docs`, so a
page can be handed to a person or an LLM. `/docs` opens on the user guides
(getting started, how a course is built, building and generating courses,
FAQ); developer pages are in their own section
([docs/dev.md](docs/dev.md)); `/llms.txt` lists what an LLM needs to write a
course.

- **[docs/course-format.md](docs/course-format.md)** — the complete course
  format: `course.json` and lesson files, every field, every rule the
  validator checks. This is also the specification the AI generator follows
  (it is embedded in the prompt). A JSON Schema is at
  [docs/course.schema.json](docs/course.schema.json).
- **[docs/features.md](docs/features.md)** — what each feature does, the
  mastery engine, and which lesson fields each feature reads.
- **[docs/architecture.md](docs/architecture.md)** — source layout, request
  flow, database schema, configuration, security notes.

Working on the code with an LLM? Start with `CLAUDE.md`.

## Self-hosting and support

Self-hosting is free: download a [release](https://github.com/alexbeeken/skrongle-mini/releases)
for 64-bit ARM (Raspberry Pi) or x86_64 Linux and follow
[docs/self-hosting.md](docs/self-hosting.md) (the
[FAQ](docs/self-hosting-faq.md) answers the first questions). Help is
best-effort, from the maintainer and other self-hosters, in
[GitHub Discussions](https://github.com/alexbeeken/skrongle-mini/discussions); a bug
goes in an issue with the *Self-hosting bug* form and the output of
`skrongle doctor`.

## Quick start (local)

```sh
./.dev-start             # build + run on a free port in 8100-8999 (printed), DB in ./dev.db, debug logging
./.dev-start --release   # run the exact profile the Pi gets
./.dev-start --reset     # wipe ./dev.db first (fresh users/progress)
./.dev-start --lan       # listen on 0.0.0.0 so a phone on your network can try the UI
./.dev-start --watch     # rebuild + restart on changes, same port and DB; open pages reload themselves
scripts/dev-progress.sh complete all   # fake "completed" lessons in ./dev.db (unlocks the drill)
cargo test               # unit tests (content loading, mastery maths, grading, migrations, courses)
scripts/smoke.sh         # fresh server + fixtures: main pages and one AI generation over HTTP
scripts/e2e.sh           # headless browser tests (Playwright, Node 22+); see docs/testing.md
scripts/e2e-jev.sh       # just the Jev-driven browser tests (needs a Jev key, below)
scripts/fact-check.sh    # Jev reads every built-in lesson and reports what is likely wrong (needs the key)
cargo build --release    # hardened release profile (fat LTO, 1 codegen unit, stripped, panic=abort)
```

### Developer secrets: `.env`

The app keeps no developer keys. Learners' Claude and OpenAI keys are
pasted into the app and saved encrypted in the database (see Security
notes). Server secrets such as `SKRONGLE_SECRET_KEY`
come from the environment (on the Pi, from `/etc/spanish-learning/env`,
written by `scripts/deploy.sh` from `deploy/deploy.env`).

The one key a developer brings is **Jev's** (TypeSafe), and only for the
browser tests. It never goes in the database, in the app's environment or
in a deploy. Keep it in a git-ignored `.env` at the repo root (or
`e2e/.env`):

```sh
# ./.env, never committed (.gitignore has .env)
TYPESAFE_API_KEY=...        # from https://console.typesafe.ai/keys
```

Only `scripts/e2e.sh`, `scripts/e2e-jev.sh`, `scripts/e2e-explore.sh` and
`scripts/fact-check.sh` read it, and they pass it to
the test runner, not to the server. `./.dev-start` and the app ignore `.env`.
Without the key, `scripts/e2e.sh` skips the Jev tests and `scripts/e2e-jev.sh`
stops and says where to put it. `e2e/.env.example` lists the optional Jev
settings. Never write a key into a script, a test or a doc; if one leaks,
revoke it in the TypeSafe console.

Visitors see a landing page at `/` (the pitch, with a lesson file and the
features it switches on); register, and the app asks *What would you like to
learn?*

## Courses

A course is a folder: `course.json` (title, units, groups, labels, an
optional paradigm) plus `lessons/**/*.json`, one lesson per file. The format
is in [docs/course-format.md](docs/course-format.md); a minimal course is
two short files.

**Ready-made.** `data/courses/<id>/` is loaded at start-up; the folder name
must equal the manifest `id`. Spanish (`data/courses/spanish`, 22 lessons for
CEFR A1–A2 with quizzes, vocabulary, conjugations, conversations and
hand-written Sprint mode exercises) ships with the app. Drop another folder next
to it and restart.

**Build my own.** *Courses → Build a course* creates a starter course stored
in the database. Edit the files in the browser, or upload JSON files (or a
whole folder) from the course's edit page. Every save validates the whole
course and reports the exact file and field that is wrong. The edit page
links to the format reference, so you can also paste it into any LLM and ask
it to write lessons for you, then upload the replies.

**Generate with AI.** *Courses → Describe a course* takes a description, an
API key (Anthropic, OpenAI, or any OpenAI-compatible server such as Ollama)
and the number of lessons. The model plans the course and writes each lesson
in turn; each file is validated against the format and the manifest, with
the validator's message sent back to the model on failure. The key is used
for that job only and is never stored.

Each user may have a project in any number of courses; progress, vocabulary,
games and history are kept per course, and the navigation bar switches
course from the current course's name.

## Architecture

See [docs/architecture.md](docs/architecture.md) for the full tour. In short:
`src/content.rs` is the course model and validator, `src/courses.rs` the
registry (built-in from disk, user-made from SQLite), every learning page
takes the `Project` extractor (user + active course), the engines
(`mastery.rs`, `quiz.rs`, `guide.rs`, `sprint.rs`, `games.rs`, `conjugation.rs`,
`vocab.rs`) persist with a `course_id`, and `src/ai.rs` runs
course generation on its own thread with raw HTTP calls through `ureq`.

## Configuration

Everything is an environment variable; all are optional. Every name starts
`SKRONGLE_`; the older names work for one more release, with a warning at
startup ([Renamed settings](docs/self-hosting.md#renamed-settings)).

| Variable | Default | Meaning |
|---|---|---|
| `SKRONGLE_BIND` | `0.0.0.0:8080` | Listen address |
| `SKRONGLE_DATA_DIR` | `data` | Directory containing `courses/` |
| `SKRONGLE_DB` | `learn.db` | SQLite file (WAL + SHM files are created next to it) |
| `SKRONGLE_LOG` | `info` | `error`/`warn`/`info`/`debug`/`trace` |
| `SKRONGLE_SECURE_COOKIES` | `0` | Set `1` behind a TLS proxy to add `Secure` to cookies |
| `SKRONGLE_BASE_PATH` | *(empty)* | URL prefix when mounted behind a reverse proxy, e.g. `/spanish` |
| `SKRONGLE_PUBLIC_URL` | — | Public URL, for the links in e-mail |
| `SKRONGLE_OPENAI_BASE_URL` | `https://api.openai.com` | Any OpenAI-compatible server (Ollama, LM Studio) for the OpenAI provider |
| `SKRONGLE_DEV`, `SKRONGLE_LLM_MODE`, … | off | Local testing only; see [docs/testing.md](docs/testing.md) |
| `SKRONGLE_SECRET_KEY` | *(generated)* | 64 hex characters that encrypt the API keys saved on profile pages. Unset: created once at `<SKRONGLE_DB>.key` (mode 0600); back it up with the database |

`skrongle doctor` (the binary is `target/release/spanish-learning` in a
checkout) prints what the server would use: version, OS, database and schema
level, which secrets are set (yes/no, never their values), free disk and
memory.

## Cross-compiling for the Pi

The build machine is an Apple Silicon Mac. Two options; `cargo-zigbuild` is
simpler (no Docker) and is what `scripts/build-pi.sh` uses by default.

| Pi OS | `uname -m` | Rust target |
|---|---|---|
| Raspberry Pi OS 64-bit (recommended) | `aarch64` | `aarch64-unknown-linux-gnu` |
| Raspberry Pi OS 32-bit | `armv7l` | `armv7-unknown-linux-gnueabihf` |
| 64-bit PC / server Linux | `x86_64` | `x86_64-unknown-linux-gnu` |

```sh
brew install zig && cargo install cargo-zigbuild
rustup target add aarch64-unknown-linux-gnu
scripts/build-pi.sh            # aarch64, copies to dist/aarch64-unknown-linux-gnu/
scripts/build-pi.sh armv7
PROFILE=release-small scripts/build-pi.sh   # opt-level="z" variant, ~30% smaller
BUILDER=cross scripts/build-pi.sh           # use `cross` (Docker/OrbStack) instead of zig
scripts/build-pi.sh x86_64                  # 64-bit PC Linux
scripts/package-release.sh 0.2.0 aarch64    # the release tarball, dist/skrongle-0.2.0-aarch64.tar.gz
```

Release tarballs for self-hosters are built by CI on a `v*` tag
(`.github/workflows/release.yml`); see "Releases" in
[docs/self-hosting.md](docs/self-hosting.md).

## Deploying

One target, one settings file, one command:

```sh
cp deploy/deploy.env.example deploy/deploy.env   # fill it in; it is gitignored
scripts/deploy-all.sh                            # tests -> one aarch64 build -> every target in DEPLOY_TARGETS
scripts/deploy-all.sh pi                         # name the target (add --skip-tests, --dry-run)
```

`deploy/deploy.env.example` documents every setting next to its field: the
Pi's ssh host and key path and its public URL. The example file holds placeholders
and paths and is safe to commit. Any setting may instead be exported from
your shell (`export PI_HOST=... ` in `~/.zshrc`, say); a variable set in the
environment overrides the same one in the file, and `scripts/release.sh` runs
with no file at all.

`scripts/release.sh` (what
`deploy-all` calls) asks the Pi for its architecture, cross-compiles, rsyncs
the binary and `data/` to `/opt/spanish-learning`, installs
`deploy/spanish-learning.service` and the nginx snippets, writes the public
URL to `/etc/spanish-learning/env`, restarts and checks
`/healthz`. Install path and service name are unchanged from the Spanish-only
days, so an existing Pi upgrades in place (the first start migrates all
existing progress into the `spanish` course).

## Resource budget on the Pi Zero 2 W

| Knob | Setting | Why |
|---|---|---|
| Tokio workers | 2 | Four slow cores; two handle a household with headroom |
| Blocking threads | max 3 | SQLite + argon2; bounded so a login burst can't spawn dozens of stacks |
| Course generation | own OS thread, max 2 jobs | Minutes of HTTP waiting must not occupy the blocking pool |
| SQLite | 1 connection, WAL, 2 MB cache, no mmap | One page cache, no pool overhead |
| argon2id | m=8 MiB, t=3, p=1 | ~100 ms per login on a Cortex-A53, at most 8 MiB transient |
| systemd | `MemoryHigh=48M`, `MemoryMax=64M` | Hard backstop; expected steady state is 15–25 MB |
| Binary | fat LTO, `codegen-units=1`, `panic=abort`, stripped | ~3.5 MB (aarch64) |

Courses are held in memory once each (a few hundred KB per realistic course;
user-made courses are loaded on first use and dropped from the cache when
edited). All state lives in SQLite, so the process has no growth path.

## Security notes

- Passwords: argon2id with per-user salt; unknown usernames still verify
  against a dummy hash so login timing does not reveal existence.
- Sessions: 256-bit random tokens, 30-day TTL, deleted on logout, purged hourly;
  `HttpOnly; SameSite=Lax` cookie, `Secure` behind TLS.
- The JSON API only accepts `Content-Type: application/json`, so no CSRF token
  is needed there; the HTML forms are low-impact.
- Course files are validated on every save (1 MiB per file, 400 per course).
  Lesson Markdown passes raw HTML through, so only load courses you trust.
- No key is hard-coded: server secrets come from the environment, and the
  only developer key (Jev, for browser tests) lives in a git-ignored `.env`
  read by the test scripts alone (see "Developer secrets" above).
- API keys pasted into the course generator are never stored. Keys saved on
  the profile page are encrypted at rest (ChaCha20-Poly1305, secret in
  `SKRONGLE_SECRET_KEY` or `<db>.key`, bound to the account) and never shown
  again; see `docs/architecture.md`.
- On Skrongle mini the person running it secures the login: a strong
  password per account, two-factor with an authenticator app, and the server
  reachable only over Tailscale or a VPN, never through a forwarded port
  ([docs/self-hosting.md](docs/self-hosting.md), "Security").
