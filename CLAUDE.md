# Working on Skrongle

A self-hosted learning app (Rust, axum, SQLite, askama) driven by JSON
courses, built for one small box such as a Raspberry Pi Zero 2 W.
When compacting, keep the issue number, the list of files changed and the
test or spec commands used.

Read `docs/` before changing anything substantial:

- `docs/course-format.md` — the course/lesson JSON format.
- `docs/features.md` — what each feature does and which fields it reads.
- `docs/architecture.md` — module layout, request flow, schema, config.
  Start at its "Where things live" map; it names the file to open first.
- User guides (`docs/index.md`, `getting-started`, `course-structure`,
  `making-courses`, `faq`) explain for people and link to the
  reference pages instead of copying field tables; keep them true when a
  feature changes. The `/docs` sidebar is `SECTIONS` in `src/routes/docs.rs`
  (a page not listed lands under Developers); `/llms.txt` is `docs/llms.txt`.

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
- One shape. Skrongle mini is a handful of accounts on one box: SQLite, no
  e-mail, and no outside service it has to reach except the AI providers the
  owner's keys point at. Don't write code, docs, tests or issues that assume
  such a service: no "if a provider is configured", no half-configured
  servers, no fallbacks for a setting nobody has. A feature that would need one is not a
  feature of this app; say so in the issue and leave it out. The dev server
  is for development, not a second shape.
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
(`.github/workflows/smoke.yml`). Don't run them
locally before pushing just to see them pass: push, then read the PR's
checks (`gh pr checks`) and fix what fails. Locally, `cargo build` (it
compiles) and the one test or spec you are debugging are enough. A check CI does not run is still yours to run when the change calls for it.

CI is for finding failures, not debugging them: its jobs keep output
minimal and build with line tables only. Debug a failure by rerunning that
one test or spec locally (full debug info there). Only for a failure CI
alone shows, turn on more for one run (`CI_DEBUGINFO=full`, extra logging)
and turn it off again; docs/testing.md "CI runners" has how.

The commands, for when you do need one:

```sh
cargo test
scripts/smoke.sh         # fresh DB + fixtures: main pages return 200, one AI generation end to end
scripts/e2e.sh           # headless browser specs (Playwright): exact + smoke specs (also CI on every PR)
scripts/pi-budget.sh     # release binary size, idle RSS and threads vs scripts/pi-budget.txt (CI job "pi budget", off for now, #1387); --update after an accepted growth
./.dev-start --reset --seed --background   # then open <url>/dev/login and click through
./.dev-start --test-course …   # also offer the test course (/dev/login?open=test-course)
./.dev-start --stop
```
