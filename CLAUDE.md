# Working on Skrongle

A learning app (Rust, axum, SQLite, askama) driven by JSON courses, run as
the hosted site or self-hosted as Skrongle mini. Session habits that keep
the context small are in `docs/dev.md` "Working with Claude".
When compacting, keep the issue number, the list of files changed and the
test or spec commands used.

Read `docs/` before changing anything substantial:

- `docs/course-format.md` — the course/lesson JSON format.
- `docs/features.md` — what each feature does and which fields it reads.
- `docs/qa.md` — how to QA a pull request (click through it, leave e2e
  specs behind, write the merge verdict); read it before reviewing a PR.
- `docs/architecture.md` — module layout, request flow, schema, config.
  Start at its "Where things live" map; it names the file to open first.
- User guides (`docs/index.md`, `getting-started`, `course-structure`,
  `making-courses`, `using-themes`, `faq`) explain for people and link to the
  reference pages instead of copying field tables; keep them true when a
  feature changes. The `/docs` sidebar is `SECTIONS` in `src/routes/docs.rs`
  (a page not listed lands under Developers); `/llms.txt` is `docs/llms.txt`.

Rules for one area live in `.claude/rules/` and load when you open a
matching file: `content.md` (course files, the format contract, the test
course, fact and plagiarism checks), `i18n.md` (app text), `ui.md`
(information behind icons), `themes.md`, `e2e.md` (testids, journeys, model
call fixtures, Jev scripts). They don't load on their own in a worktree
other than the one the session started in, so read the ones for your area.

## Conventions

- Every learning handler takes `Project` (`src/auth.rs`): the user plus
  `Arc<ContentStore>` for the active course. Persistence functions take a
  `course_id`. Never hard-code Spanish: labels, languages and helper
  characters come from the manifest (`CourseView` for templates, the
  `course` object in the Sprint mode session JSON for `sprint.js`).
- A lesson that lacks the data for a feature simply does not appear there;
  keep that property when adding features.
- "Passed" means the same in every mode (`src/progress.rs`, rule in
  `docs/progress.md`): seen + one run of >= 70 %. A new lesson-based
  feature reports through `progress::mark_seen` / `record_run` /
  `record_taught_run`; show completion from passes, not from mode tables.
- Two shapes, no third. Skrongle runs as **the hosted site** (we run it,
  with every provider set: e-mail, SMS, Google, payments) or as **Skrongle
  mini** (self-hosted, a handful of users, SQLite, no e-mail, SMS, Google or
  payments; #632). Self-hosting the full build is not supported. Don't write
  code, docs, tests or issues for it: no "a self-hoster who skipped SMTP", no
  half-configured servers, no fallbacks that only that case needs. A feature
  that needs a provider is a hosted feature, and mini leaves it out; say it
  that way in docs and issues ("not in Skrongle mini"). Self-hosting docs
  (`docs/self-hosting*.md`, `/self-host`) describe mini only. The dev server
  is for development, not a third shape. The server enforces it: outside
  mini and dev mode it refuses to start until every provider is set
  (`require_providers` in `src/main.rs`); `SKRONGLE_MINI=1` is what makes a
  server a mini (`src/shape.rs`, read once at startup; code asks
  `shape::current()` or `crate::is_mini()`), so a script that starts a plain
  server sets `SKRONGLE_MINI=1`.
- Memory matters: the target is a Pi Zero 2 W (Skrongle mini). No new heavy dependencies;
  SQLite work goes through `Db::call`; long blocking work (LLM calls) gets
  its own OS thread, not the Tokio blocking pool.
- A source file stays under 1,500 non-test lines. One that grows past it
  gets split, not baselined, unless there is a reason
  (`scripts/file-size-lint.sh`, run in CI).

## Pull requests

- Every PR title carries the GitHub issue number it works on in square
  brackets, first: `[123] Short summary`. A PR for several issues lists
  them all (`[56][58] …`). No issue yet? Open one (`gh issue create`)
  before the PR. The `pr-title` workflow fails a title without one; it is
  not in the required-checks ruleset.
- Still say `Closes #123` (or `Part of #123`) in the body so GitHub links it.
- Every PR body starts with these two sections, before any detail:
  - **What this PR is for**: a short bulleted list of what the PR intends.
    ADHD friendly: short sentences, 1–3 sentences per bullet.
  - **How to QA it**: step-by-step instructions as GitHub checkboxes
    (`- [ ] …`), one action and what you should see per box, in the order
    to do them, starting from how to run it (e.g. `./.dev-start …`), so
    the reviewer can tick them off as they go.

## Checks before finishing

**Let CI run the checks, once, after the push.** `cargo test`,
`scripts/smoke.sh` and the exact and smoke e2e specs run on every PR
(`.github/workflows/smoke.yml`, on self-hosted runners). Don't run them
locally before pushing just to see them pass: push, then read the PR's
checks (`gh pr checks`) and fix what fails. Locally, `cargo build` (it
compiles) and the one test or spec you are debugging are enough. Checks
CI does not run (Jev journeys, fact check, plagiarism check, playthroughs)
are still yours to run when the change calls for them.

CI is for finding failures, not debugging them: its jobs keep output
minimal and build with line tables only. Debug a failure by rerunning that
one test or spec locally (full debug info there). Only for a failure CI
alone shows, turn on more for one run (`CI_DEBUGINFO=full`, extra logging)
and turn it off again; docs/testing.md "CI runners" has how.

The commands, for when you do need one:

```sh
cargo test
scripts/smoke.sh         # fresh DB + fixtures: main pages return 200, one AI generation end to end
scripts/e2e.sh           # headless browser specs (Playwright): exact + smoke specs (no key, also CI on every PR); Jev journeys too if TYPESAFE_API_KEY is set
scripts/pi-budget.sh     # release binary size, idle RSS and threads vs scripts/pi-budget.txt (CI job "pi budget", off for now, #1387); --update after an accepted growth
./.dev-start --reset --seed --background   # then open <url>/dev/login and click through
./.dev-start --test-course …   # also offer the test course (/dev/login?open=test-course)
./.dev-start --stop
```
