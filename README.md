<picture>
  <source media="(prefers-color-scheme: dark)" srcset="static/logo-dark.svg">
  <img src="static/logo.svg" alt="Skrongle" width="176" height="40">
</picture>

# Skrongle

*A learning app you run yourself, on a Raspberry Pi, for the people at home.*

Skrongle is one small binary that teaches from courses made of JSON files:
lessons, adaptive quizzes, five-minute practice sessions, games, flashcards
and an AI tutor. Spanish and French courses ship with it; any subject that
fits *units → lesson groups → lessons* can be a course, written by hand,
uploaded from files, or described to an AI that writes it for you.

It is built for a handful of people who know whoever runs it: a family, a
class, a few friends. Up to eight accounts, no e-mail, no outside service of
any kind except the AI providers you choose to give it a key for. A
Raspberry Pi Zero 2 W runs it with room to spare.

## Run it yourself

The app has a step-by-step guide at `/self-host` ("Host it yourself"),
written for someone who has never run a server, and the technical version in
[docs/self-hosting.md](docs/self-hosting.md). The short form:

1. Download the [release](https://github.com/alexbeeken/skrongle-mini/releases)
   tarball for your machine: `skrongle-<version>-aarch64.tar.gz` for a Pi
   (64-bit Raspberry Pi OS) or `-x86_64` for a PC running Linux.
2. Unpack it, install the systemd unit it contains and start the service.
   The server writes an **owner link** to its log: open it to make the
   first account, which is the owner's.
3. Everyone else joins with an invitation link the owner makes on the
   **Manage Users** page.
4. Optional: paste an AI provider's API key on the same page. It serves every
   account, and the owner can hide the AI features for everyone with one
   switch.

What you get, and what you are responsible for:

| | |
|---|---|
| Accounts | Up to 8, each with up to 4 signed-in devices |
| Who signs up | Only people the owner invites (the default, `SKRONGLE_SIGNUP=invite`) |
| E-mail | None. Every link is shown on the page instead of sent |
| Password reset | The owner sets a temporary password from the Manage Users page or the command line |
| Second factor | An authenticator app, per account |
| AI | Your own keys: Anthropic, OpenAI, Google, DeepSeek, Mistral, xAI, OpenRouter, or an Ollama-compatible server on your network |
| Database | One SQLite file next to the binary. Back it up and you have everything |
| Reaching it from outside | You: Tailscale or a VPN, never a forwarded port ([docs/self-hosting.md](docs/self-hosting.md), "Reaching it from outside") |
| Help | [GitHub Discussions](https://github.com/alexbeeken/skrongle-mini/discussions); bugs through the *Self-hosting bug* issue form with the output of `skrongle doctor` |

A database from before version 1.0 is not upgraded; start from a fresh one.

## What it teaches with

Every feature reads the same course files:

| | |
|---|---|
| **Lessons** | Lesson pages with vocabulary and paradigm tables, and a private note per lesson |
| **Quizzes** | Adaptive micro-quizzes with per-lesson mastery, decay and targeted practice |
| **Guide** | Each lesson as a stage of small steps, with stars and XP |
| **Sprint mode** | Phone-first five-minute sessions: sort, spot, build, ladder, pick, match, listening exercises, streaks, a lightning round |
| **Games** | Word Match with a leaderboard; a drill for paradigm tables such as verb conjugations |
| **Tutor** | A chat about the current lesson, on your AI key |
| **Map** | Every lesson of every unit with your mastery |
| **Vocabulary** | Terms collected by answering questions, studied as flashcards |
| **History** | Every answer and game |

Courses also carry music notation, clocks, protractors and algebra tiles as
question types, so the format is not limited to languages.

## Courses

A course is a folder: `course.json` (title, units, groups, labels, an
optional paradigm) plus `lessons/**/*.json`, one lesson per file. The format
is in [docs/course-format.md](docs/course-format.md), with a JSON Schema at
[docs/course.schema.json](docs/course.schema.json); a minimal course is two
short files.

**Ready-made.** `data/courses/<id>/` is loaded at start-up; the folder name
must equal the manifest `id`. Spanish (101 lessons, CEFR A1 to C2) and French
(75 lessons) ship with the app. Drop another folder next to them and restart.

**Build your own.** *Courses → Build a course* creates a starter course in
the database. Edit the files in the browser or upload JSON files (or a whole
folder). Every save validates the whole course and names the exact file and
field that is wrong. The edit page links to the format reference, so you can
also hand it to any LLM and upload what it writes.

**Generate with AI.** *Courses → Describe a course* takes a description and
the number of lessons. The model plans the course and writes each lesson in
turn; each file is validated against the format, with the validator's
message sent back to the model when it gets something wrong. One generation
runs at a time.

## Documentation

The documentation lives in `docs/` and is served by the app at `/docs`, so a
page can be handed to a person or an LLM; `/llms.txt` lists what an LLM needs
to write a course.

- [docs/getting-started.md](docs/getting-started.md), the user guides and
  the [FAQ](docs/faq.md).
- [docs/self-hosting.md](docs/self-hosting.md) and the
  [self-hosting FAQ](docs/self-hosting-faq.md).
- [docs/course-format.md](docs/course-format.md): the complete course
  format, also the specification the AI generator follows.
- [docs/features.md](docs/features.md): what each feature does, the mastery
  engine, and which lesson fields each feature reads.
- [docs/architecture.md](docs/architecture.md): source layout, request
  flow, database schema, configuration, security notes.
- [docs/testing.md](docs/testing.md): how to run the checks.

## Developing

```sh
./.dev-start             # build + run on a free port (printed), DB in ./dev.db, debug logging
./.dev-start --release   # the exact profile a release gets
./.dev-start --reset     # wipe ./dev.db first
./.dev-start --lan       # listen on 0.0.0.0 so a phone on your network can try it
./.dev-start --watch     # rebuild + restart on changes
cargo test               # unit tests
scripts/smoke.sh         # fresh server + fixtures: main pages and one AI generation over HTTP
scripts/e2e.sh           # headless browser tests (Playwright, Node 22+); see docs/testing.md
cargo build --release    # hardened release profile (fat LTO, 1 codegen unit, stripped, panic=abort)
```

Dev mode (`SKRONGLE_DEV=1`, what `./.dev-start` sets) adds `/dev/login`, a
simulated AI provider that answers from fixtures, and seeded accounts, so
every AI feature can be exercised without a key. Working on the code with an
LLM? Start with `CLAUDE.md`.

The app itself keeps no developer keys.

### Cross-compiling and releases

```sh
brew install zig && cargo install cargo-zigbuild      # on a Mac
rustup target add aarch64-unknown-linux-gnu
scripts/build-pi.sh            # aarch64, to dist/aarch64-unknown-linux-gnu/
scripts/build-pi.sh x86_64     # 64-bit PC Linux
scripts/package-release.sh 0.2.0 aarch64    # the release tarball
```

Release tarballs are built by CI on a `v*` tag (`.github/workflows/release.yml`).
`scripts/release.sh` deploys a checkout to a Pi over ssh (settings in
`deploy/deploy.env`, template in `deploy/deploy.env.example`): it
cross-compiles, copies the binary and `data/`, installs the unit and the
nginx snippets, restarts and checks `/healthz`.

## Configuration

Everything is an environment variable; all are optional. The full table with
defaults is in [docs/self-hosting.md](docs/self-hosting.md), "Configuration".
The ones most installs touch:

| Variable | Default | Meaning |
|---|---|---|
| `SKRONGLE_BIND` | `0.0.0.0:8080` | Listen address |
| `SKRONGLE_DB` | `learn.db` | SQLite file (WAL and SHM files are created next to it) |
| `SKRONGLE_DATA_DIR` | `data` | Directory containing `courses/` |
| `SKRONGLE_SIGNUP` | `open` (`invite` in the shipped unit) | Who may make an account |
| `SKRONGLE_PUBLIC_URL` | — | The address the server is reached at, for the links it shows |
| `SKRONGLE_BASE_PATH` | *(empty)* | URL prefix behind a reverse proxy, e.g. `/learn` |
| `SKRONGLE_SECURE_COOKIES` | `0` | Set `1` behind a TLS proxy |
| `SKRONGLE_SECRET_KEY` | *(generated)* | Encrypts the saved API keys. Unset: created once at `<SKRONGLE_DB>.key`; back it up with the database |
| `SKRONGLE_LOG` | `info` | One level: `error`, `warn`, `info`, `debug` or `trace` |

`skrongle doctor` prints what the server would use: version, OS, database and
schema level, which secrets are set (yes or no, never their values), free
disk and memory.

## On a Raspberry Pi Zero 2 W

Measured on the Pi, release build:

| | |
|---|---|
| Binary | about 15 MB, everything compiled in (SQLite, templates, CSS, JS, docs) |
| Memory | about 16 MB idle, about 21 MB in use; the unit caps it at 48 MB |
| Threads | one async worker, two for the database and password hashing |
| Sign-in | about 160 ms (argon2id, 8 MiB, t=3) |
| A page | 3 to 10 ms warm; the first page after a restart about 50 ms |

The limits that keep it there are in `src/caps.rs`: 8 accounts, 4 sessions
each, 32 open connections, 2 tutor replies at once, one course generation at
a time, and a course cache bounded by the account count. All state lives in
SQLite, so the process has no growth path.

## Security notes

- Passwords: argon2id with a per-user salt; unknown addresses verify against
  a dummy hash so login timing does not reveal whether an account exists.
- Sessions: 256-bit random tokens, 30-day TTL, deleted on logout, purged
  hourly; `HttpOnly; SameSite=Lax` cookie, `Secure` behind TLS; at most four
  per account.
- Every state-changing request passes a same-origin check; the JSON API only
  accepts `Content-Type: application/json`.
- Course files are validated on every save (1 MiB per file, 400 per course).
  Lesson Markdown passes raw HTML through, so only load courses you trust.
- API keys are saved encrypted at rest (ChaCha20-Poly1305 under
  `SKRONGLE_SECRET_KEY` or `<db>.key`) and never shown again; only owners see
  the page that holds them.
- The person running it secures the login: a strong password per account,
  an authenticator app, and the server reachable only over Tailscale or a
  VPN, never through a forwarded port.

## Licence

AGPL-3.0. See [LICENSE](LICENSE).
