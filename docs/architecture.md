# Architecture

A single Rust binary (axum + SQLite + askama; templates, static files and docs
compiled in, courses read from `data/` at startup) sized
for a Raspberry Pi Zero 2 W: ~3.5 MB, 15–25 MB RSS, one async worker, two
blocking threads, one SQLite connection in WAL mode.

## Where things live

One line per area: what it owns, then its files. The file to open
first comes first. Rust paths are under `src/` (`routes/sprint.rs` is
`src/routes/sprint.rs`). A page's handler is in `routes/<area>.rs`, its
markup in `templates/`, its script in `static/`. The sections below this
one have the detail; stop here once you know which file to open.
`cargo test architecture_map` fails when a file in `src/` or a folder
under `static/`, `templates/` or `data/` is missing from this list.

### Server and plumbing

- Start-up: `main.rs` (env config, commands), `caps.rs` (the fixed limits of a mini, `MAX_ACCOUNTS`, #29), `startup.rs` (start-up errors), `doctor.rs` (`skrongle doctor`), `server.rs` (listener limits).
- HTTP: `routes/mod.rs` (Router, embedded static files and docs, CourseView), `error.rs` (AppError to responses), `security.rs` (headers, CSRF, rate limits).
- Database: `db.rs` (connection, `Db::call`), `db/migrations.rs` (`MIGRATIONS`, the schema in `db/migrations/v001_schema.sql`); described in "SQLite schema" below; background jobs (the hourly housekeeping) in `jobs.rs`.
- App text: `i18n.rs` reads `i18n/<code>.yaml` (`docs/i18n.md`). Randomness: `random.rs` (OS bytes), `quiz.rs` has the small PRNG.

### Courses and content

- Course model and validation: `content.rs` (Manifest, Lesson, ContentStore), `content/paradigm.rs` (paradigm slots and sets, `who`), `content/quiz_type.rs` (the quiz question types); contract in `docs/course-format.md`. Placement test (`placement.json`, #1106): `placement.rs` (format, drawing, marking), routes in `routes/courses/settings.rs`, `templates/placement_test.html`, `static/placement.js`.
- Course registry: `courses.rs` (built-in, user-made, projects); pages in `routes/courses/` (`routes/courses/mod.rs` routes and shared views, `routes/courses/picker.rs` the list, `routes/courses/settings.rs` the settings dialog and opening, `routes/courses/stat.rs` the body of a card's stat modal (fetched by `static/course-stat.js`), `routes/courses/create.rs` new, copy and translate, `routes/courses/editor.rs` the editor page, `routes/courses/files.rs` files, upload and export, `routes/courses/lessons.rs` the lesson table, `routes/courses/generate.rs` AI generation, `routes/courses/revise.rs` Add content, Review AI changes, Undo), `static/json-problems.js` (JSON problems worded as the server words them), `static/lesson-editor.js`, `static/advice.js` (the yellow ! on lesson rows and its *Things to check* modal), `static/lesson-preview.js`, `static/schema-form.js`, `static/lesson-duplicate.js` (a lesson row's ⧉), `static/lesson-ai-edit.js` + `static/lesson-ai-edit.css` + `templates/lessons_ai_dialog.html` (a lesson row's ✦, Edit with AI, #1242), `static/ai-examples.js` + `static/ai-examples.css` (Use AI's example requests, `templates/ai_examples.html`), `templates/course_edit.html`.
- Editing helpers: `arrange.rs` (lesson table, Undo), `word_list.rs` + `static/word-list.js` (Paste a word list), `problems.rs` (refusal messages), `report.rs` (feature usage report), `lint.rs` (course lints, `lint/language.rs` the language guess), `review.rs` (Review AI changes; its why and check lines styled by `static/review-notes.css`).
- AI generation: `ai.rs` (LLM client, job runner), `ai/error.rs` (the typed wire error), `ai/queue.rs` (the one slot and the lease of AI jobs, #874, #43), `ai/providers.rs` (the providers, #24: ids, base URLs, course and tutor models, wire formats, and the one OpenAI-compatible client for OpenAI, Google, DeepSeek, Mistral, xAI, OpenRouter and an Ollama-compatible server), `ai/prompt.rs` (the instructions and each kind of course's rules), `ai/budget.rs` (effort levels and the tokens a job reserves, #918), `ai/edit.rs` (Edit lessons, #1240), `ai/check.rs` (the `check` plan action and the why lines of a fact check, #1238), `ai/edit_lesson.rs` (Edit with AI on one row, #1242), `translate.rs` (copy and translate a course), `prices.rs` (the estimate's price table)).
- Sharing: `share.rs`, `routes/share.rs`.
- Course pickers in `static/courses.js`, `static/course-settings.js`.
- Test course: `test_course.rs` (tests only), `data/test-courses/`.

### Learning modes

- Lessons and home: `routes/pages.rs` (`/`, `/lessons`, `/history`, `/mistakes`), `templates/home.html`, `static/home.js`, `static/lessons.js`; a learner's note on a lesson: `notes.rs`, `static/lesson-note.js` (#336). Tapping a conversation line to hear it and see its meaning: `static/conversation.js` (#1264). Music notation in lesson text (#1176) and on quiz items (#1218): `notation.rs` (does a lesson have any; `music.note_names` and note options), `static/notation.js` draws ```` ```abc ```` blocks and quiz staves with abcjs from `static/vendor/` (third-party files, unchanged, with their licences); `static/music-sound.js` plays a quiz item's or sort item's `play` with Web Audio tones (#1219). The Fill the bar quiz type (#1230): `notation/fill_bar.rs` (gap and note lengths, validation, grading), `static/fill-bar.js` and `static/fill-bar.css` (the bar and its tiles in every mode). A Sprint spot on a staff (#1227): `sprint/spot.rs` (both kinds of spot: exercise, grading, validation), `notation::abc_notes` counts the notes, `static/wrong-note.js` takes the taps. The Spell the chord quiz type (#1228): `notation/chord_spell.rs` (validation, grading as a set, the tiles in the course's note names), `static/chord-spell.js` and `static/chord-spell.css` (the slots, tiles and chord staff in every mode). The Place the note quiz type (#1226): `notation/staff_note.rs` (validation, grading by pitch and octave, the clef and note names for the page), `static/staff-note.js` and `static/staff-note.css` (the staff it draws itself, in every mode). The keyboard quiz type (#1231): `notation/keyboard.rs` (ABC notes as pitches, validation, grading), `static/keyboard.js` and `static/keyboard.css` (the piano keyboard component, in every mode).
- Questions: `quiz.rs` (picking, checking), `routes/quiz.rs` (`/api/answer`, the one grading path), `mathcheck.rs` (maths answers), `clock.rs` and `static/clock.js` (Set the clock, #1340; lesson, stage and quiz pages load it and `measure.js` only when `has_clock()` / `has_measure()`), `static/quiz.js`, `static/lesson-check.js`; the algebra tiles question (#1355): `algebra_tiles.rs` (reading the pile, the two-sides check), `static/algebra-tiles.js` and `.css` (the board); Measure the angle (#1352): `measure.rs` (fields, grading, which modes skip it), `templates/measure.html`, `static/measure.js` draws the protractor with JSXGraph from `static/vendor/`.
- Passes and progress: `progress.rs` (the pass rule), `routes/progress.rs` (agent API), `mastery.rs` (scores), `mastery/cache.rs` (each learner's progress rows for their course, in memory, #38), `activity.rs` (daily goal, resume point), `earnings.rs` (XP, streak totals).
- Motivators: `motivators.rs`, `motivators/xp.rs`, `templates/motivators/`.
- Sprint: `sprint.rs` (fill-the-gap tiles in `sprint/gap.rs`), `routes/sprint.rs`, `static/sprint.js`, `static/sprint.css`. Build the scale (#1229), the ladder with notes: `sprint/scale.rs` (exercise, grading, validation); `static/scale-ladder.js` draws and plays each note as it lands, `static/sprint.js` runs the ladder.
- Study: `guide.rs` + `routes/guide.rs` (Guide), `tutor.rs` + `routes/tutor.rs` (AI tutor), `listening.rs` + `routes/listening.rs`, `vocab.rs` + `routes/vocab.rs`, `routes/map.rs` (`/map`); scripts `static/guide.js` (list styles `static/guide.css`), `static/tutor.js`, `static/listening.js`, `static/vocab.js`.
- Play: `games.rs` + `routes/games.rs` (hub, Word Match, numbers, dates), `conjugation.rs` (drill), `static/matching.js`, `static/conjugation.js`, `static/numbers.js`, `static/dates.js`.

### Accounts

- Sign-in: `auth.rs` (sessions, CurrentUser, Project), `auth/cache.rs` (the signed-in users kept in memory by session, #37), `routes/auth.rs`, `routes/auth/code.rs` (the two-factor step), `proxy_auth.rs` (proxy header), `signup.rs` (who may register), `urlenc.rs` (percent-encoding for the redirects they build).
- Two-factor: `two_factor.rs`, `totp.rs`.
- Profile: `profile.rs`, `routes/profile.rs`, `static/profile.js`, `static/autosave.js`.
- Data rights: `db/tables.rs` (the user-table list), `jobs/retention.rs` (retention rules), `signup.rs` (the age confirmation).
- Owner: `owner.rs`, `routes/manage.rs`, `static/manage.js`, `routes/password.rs` (temporary password gate); the server's AI API keys in `secrets.rs` and the hide-AI switch in `settings.rs`, both on Manage Users (#25).
- Docs pages: `routes/docs.rs` (`/docs` sidebar `SECTIONS`, `/llms.txt`).

### Dev builds only (the `dev` cargo feature)

- `dev.rs` + `routes/dev.rs` (`/dev/login`, the dev authenticator; `routes/dev/jobs_page.rs` is `/dev/jobs`), `dev_ai.rs` + `routes/dev_ai.rs` (`/dev/ai`, fixtures in `data/test-fixtures/`), `static-dev/` (`devbox.js`, `devbox.css`, served under `/static/`).

### Elsewhere

- `data/courses/` built-in courses, `docs/` these pages, `i18n/` app text, `e2e/` browser tests, `scripts/` tooling, `deploy/` the systemd unit and update script.

## Modules

```
src/
  main.rs        config from env, command line (`doctor`, `--version`), tokio runtime (1 worker, 2 blocking threads), startup, graceful shutdown
  startup.rs     startup errors for the usual self-hosting mistakes: port in use, database folder not writable, secret key missing
  doctor.rs      `skrongle doctor`: read-only diagnostics for bug reports (version, OS, database, schema level, yes/no for secrets, disk, memory)
  content.rs     course model: Manifest, Lesson, QuizItem, …; validation; ContentStore (one course in memory)
  courses.rs     course registry: built-in courses from data/courses/, user-made ones in SQLite; projects
  ai.rs          LLM client (Anthropic Messages API / OpenAI-compatible, incl. Gemini) + "generate with AI" job runner (generate, resume, revise with feedback, add a lesson)
  ai/
    error.rs     `AiError`: the typed wire failure (HTTP status, timeout, unreachable, declined, bad reply); `Display` is the stored sentence, `kind` the learner's reading of it
    edit.rs      Edit lessons (#1240): rewrite lessons, add, remove or move none; the per-lesson writer shared with revise
    edit_lesson.rs  Edit with AI on one lesson's row (#1242): one `edit-lesson` request, checked by `arrange::only_edits_one`
    queue.rs     AI jobs, one at a time (#874, #43): the one slot, the `generate_course` rows on the jobs table, the lease heartbeat, going on after a restart
    tests.rs     generation, resume, revision and slot tests
  review.rs      "Review AI changes": side-by-side line diff (Hirschberg LCS) of the files the latest AI run changed,
                 JSON shown as labelled text, summaries in words, and Accept minus what was left out (Exclusions)
  arrange.rs     restructure a course by lesson: apply the editor's lesson table (order, units, names, deletions, new and edited lessons), place an AI lesson; `only_adds`, `only_edits` and `only_edits_one` check an Only add, an Edit lessons run and a one-lesson edit; the result is one Undo step; the digest of written lessons, the course map and the neighbouring lessons for the next lesson's request
  report.rs      the editor's feature usage report (#328): per unit and lesson, which question types and lesson features it uses
  lint.rs        course lints (#445, #1199): cheap checks, no AI, for lessons that load but teach badly; the top of the feature usage report, an amber ! on the field in the lesson editor, logged at the end of a generation; lint/language.rs guesses a text's language
  db.rs          rusqlite connection, pragmas, migrations, spawn_blocking wrapper
  auth.rs        argon2id hashing, sessions, CurrentUser (route guard) and Project (user + active course) extractors
  auth/cache.rs  the signed-in users by session in memory (#37): forgotten by temporary triggers (db.rs) on any write to a table the session query reads, and after 60 s
  proxy_auth.rs  sign-in through a reverse proxy's identity header (SKRONGLE_PROXY_AUTH_HEADER / _FROM, #642): a middleware before every handler
  signup.rs      the age confirmation and SKRONGLE_MIN_AGE; who may make an account (SKRONGLE_SIGNUP: open / invite / closed, #634): the first account is the owner, through the owner link written to the log at startup, invitation links (table signup_invites, the token hashed), the Manage Users page's Accounts part and invitation links (routes/manage.rs)

  share.rs       sharing a course with other learners by invitation link (#451, #16): links the owner names and passes on (table course_shares, the link's token hashed), who may open a shared course
  totp.rs        time-based one-time passwords: RFC 6238 TOTP over RFC 4226 HOTP (SHA-1 written out, SHA-256/512 via sha2), base32, the otpauth://totp/ Key Uri Format and its QR code as inline SVG (the qrcode crate, no image feature); the dev authenticator (/dev/authenticator) reads its keys with it
  two_factor.rs  two-factor sign-in: on or off, the authenticator app's enrolment, sign-in challenges, the proof for turning it off (tables totp_keys, two_factor_challenges)
  profile.rs     user profiles: name/e-mail validation, profile persistence
  secrets.rs     the server's AI API keys (#25, table server_api_keys; the owner's, set on /manage, used by every account): master secret, ChaCha20-Poly1305 sealing
  settings.rs    server-wide settings the owner changes on /manage (table settings, #25): ai_off, the switch that hides every AI feature for every account (read into CurrentUser::ai_off by the session query)
  prices.rs      the price table behind the Generate dialog's cost estimate (SKRONGLE_AI_PRICES)
  server.rs      the listener: header and body timeouts, a connection cap (axum::serve plus limits)
  security.rs    security headers and the cross-site (CSRF) check on every request, client address, rate limits
  tutor.rs       AI tutoring: in-memory chat sessions, exercise picking, prompts, the reply markers ([[ask]], [[skip]], [[answer: …]]), the model call thread
  mastery.rs     mastery score update + decay, progress/history persistence (per course); mastery/cache.rs holds each learner's user_progress rows for the course they are on (one entry per account, at most MAX_ACCOUNTS), updated by `save`, the one write (#38)
  activity.rs    activity from every mode (#558): the daily goal's effort points per kind (tables activity_log), the lesson every mode resumes at (lesson_touches)
  notes.rs       a learner's own note on a lesson (#336): "My note" on the lesson page, POST /api/lessons/{slug}/note (table lesson_notes)
  earnings.rs    what a learner earned in a course: a window's tally from the log (a run, today), the running totals and the day streak as one `earnings` row per course (#589) that activity::record moves; priced by the motivators' rates
  motivators.rs  motivators (#559): how progress is dressed up (projects.motivator, per course since #574); each draws the course home's top from the shared measures; registry ALL, default "stats"; motivators/xp.rs: XP and the character level (#561)
  placement.rs   the placement test (#1106): placement.json parsed and checked against the manifest, one sitting drawn (written questions, else random_per_unit of a unit's multiple-choice quiz items), the answers marked (each unit's tally, the first unit with a miss as the start)
  progress.rs    lesson passes shared by every mode: the rule (seen + a run of >= 70 %), the RULES text for tutors and agents, table lesson_passes
  quiz.rs        adaptive question selection, answer normalisation/checking, tiny PRNG
  problems.rs    what the editor and the file page say when a course file is refused: kind, lesson, field, line, message, fix hint (every ContentError and arrange.rs refusal)
  owner.rs       what the owner does to an account with no message sent (#635): a temporary password (users.must_change_password), two-factor off, make an owner; the binary's commands (owner-link, reset-password, disable-2fa, make-owner, help)
  db/tables.rs   USER_TABLES and SHARED_TABLES: which tables hold a learner's rows, and the test that every table is listed (docs/data-inventory.md)
  jobs/retention.rs  the retention rules the hourly housekeeping applies (old answers, finished notes, failed AI courses)
  jobs.rs        background jobs (table jobs, #872): enqueue, the one-statement claim with a lease, retries with per-kind delays, periodic kinds that queue their next run; the `jobs` runner thread; the hourly housekeeping as one job with a step per purge
  mathcheck.rs   numeric and expression answers: a small parser/evaluator, numbers in every spelling, equivalence by sampling
  algebra_tiles.rs  algebra-tiles questions: the pile from `tiles`, an answer must be two sides whose product is the tiles
  clock.rs       clock answers: H:MM on the 12-hour face, the hands' angles, what a clock question may say
  measure.rs     the measure question type (#1352): tool and angle, graded like numeric, the other-scale note
  guide.rs       stage progress and stars for the Guide (table journey_progress)
  listening.rs   listening practice: dealing sentences, word-by-word grading, the finished round
  vocab.rs       terms collected from passed questions + flashcard review stats and schedule (words due), word lookup
  games.rs       Word Match dealing/replay, timed sessions, leaderboard + activity persistence
  conjugation.rs paradigm drill: unlocked items, prompt dealing, answer checking
  sprint.rs       Sprint mode: session building, exercise grading, streaks and XP
  i18n.rs        the app's own text: i18n/<code>.yaml compiled in, the request's language, t / t1 / tn
  random.rs      random bytes from the operating system (getrandom), in one place
  translate.rs   copy and translate a course (#630): which texts the AI translates, one file's request and reply, putting the translations back
  error.rs       AppError -> HTTP responses (incl. redirects to /login and /courses)
  dev_ai.rs      the dev AI provider's logic (#297): key profiles (platform, learner), usage limits and their quota errors, injected failures, the fixture reply for a wire request, Manual replies (held requests), the call log; in memory
  dev.rs         local testing (the `dev` feature; SKRONGLE_DEV): the dev settings and dev::start, the gate, the dev user (seed with the simulator's keys, reset), SKRONGLE_LLM_MODE and the dev AI provider's default base URLs, the fixture directory and its in-memory edits, held requests on disk (Manual replies on /dev/ai)
  test_course.rs tests only: keep the test course covering every feature (schema, aliases, quiz types, exercises)
  routes/
    mod.rs       axum Router, embedded static assets and docs, CourseView for templates
    courses/     the course routes, one file per feature (handlers keep their names: `courses::picker`, …)
      mod.rs       shared imports and the module list; re-exports every handler
      picker.rs    GET /courses: the course list, its cards and `?notice=` keys, the editor's notice keys
      settings.rs  POST /courses/{id}/settings (the gear dialog and the first-visit wizard), POST /courses/open/{id}
      stat.rs      GET /courses/{id}/stat/{metric}: a card chip's modal body (what is next), fetched as it opens
      create.rs    POST /courses/new (a draft), /courses/{id}/copy, /courses/{id}/translate (copy and translate with AI)
      editor.rs    GET /courses/{id}: the editor (lesson table, Use AI form, review modal, Undo/Redo, Share links), `owned`
      files.rs     GET/POST /courses/{id}/file, file/delete, publish, delete, export.json; GET/POST /api/courses/{id}/files, GET /api/courses/{id}/file, POST /api/courses/{id}/preview-answer, POST /api/courses/{id}/answer-key
      generate.rs  POST /courses/{id}/generate (its Generate dialog) and /cancel, provider and quota checks, GET /api/generate/{job}
      revise.rs    POST /courses/{id}/ai (add, edit or revise with AI)|undo|redo|review (Accept in the review: what is left out), POST /api/review-quiet
      lessons.rs   POST /courses/{id}/lessons (JSON: the whole table) and /api/courses/{id}/lessons (the same, the editor's autosave: `session` + `base` version, 409 when behind or busy, 422 refused)
      tests.rs     the editor autosave, notices, write limits, prompt boxes and preview-answer tests
    share.rs     POST /courses/{id}/share (make a named link) and /courses/{id}/share/remove (by id, owner only); GET /courses/shared/{token} (the invitation's link: signed in, the course joins the list and opens; signed out, /login?share=<token>)
    docs.rs      /docs (sidebar sections: SECTIONS), /llms.txt (docs/llms.txt)
    auth.rs      /login /register (signs the new account in; a taken address is refused on the form) /logout
    auth/code.rs /login/code… (the two-factor step: code form, resend, cancel, status, dev device)
    password.rs  /password/change (the gate after signing in with a temporary password: the new one twice, no old one)
    manage.rs    /manage, the Manage Users page (owners only, 404 otherwise; /superuser redirects here): every account on one page with Temporary password, Turn off two-factor and Make owner per row; invitation links under SKRONGLE_SIGNUP=invite; and the AI section (#25): the server's AI API keys (autosaved, POST /api/manage/keys) and Hide AI features for everyone. POST /manage/make-owner|invites|invites/revoke|temp-password|two-factor-off|api-keys/remove|ai
    profile.rs   /profile (personal information, account), POST /api/profile (autosave), POST /profile/password
    pages.rs     / (landing for visitors, home: the three modes) /lessons /lessons/{slug} /history /mistakes, POST /api/lessons/{slug}/note (the lesson note, autosave)
    map.rs       /map
    quiz.rs      /practice, POST /api/answer (grade_and_record: the one grading path for every mode), POST /api/lesson-check (the reading check)
    progress.rs  POST /lessons/{slug}/read (lessons without questions), POST /api/practice/finish (runs + the reward line), the agent API: GET /api/progress[/lessons/{lesson}], POST /api/progress/seen|answers|pass
    tutor.rs     /tutor, GET /api/tutor/session, POST /api/tutor/start|turn|cancel
    guide.rs     /guide, /guide/{slug}, POST /api/guide/step|restart
    listening.rs /listening, /listening/play, POST /api/listening/check|finish
    vocab.rs     /vocabulary, /vocabulary/study, /lookup, POST /api/vocabulary/review
    games.rs     /play (Play mode hub; /g and /games redirect here), /play/matching…, /play/conjugation…, /play/numbers, /play/dates, their APIs
    sprint.rs     /sprint, /sprint/play, POST /api/sprint/check|finish
    dev.rs       /dev/login, /dev/reset, /dev/jobs, /dev/authenticator, /dev/boot (the --watch reload poll); all mounted only in dev mode and for local clients, see testing.md; the dev pages need no sign-in
    dev_ai.rs    /dev/ai: the dev AI provider's wire API (Anthropic Messages, OpenAI chat completions, Gemini's OpenAI endpoint; checked by API key; a held request waits for its reply file) and its page among the dev pages (log, limits, failures, Manual replies with the held requests and their reply forms, fixture editor)
templates/       askama templates (compiled in)
static/          CSS + dependency-free JS (compiled in); vendor/ holds the one third-party script, abcjs
docs/            this documentation (compiled in, served at /docs)
data/courses/    built-in courses: <id>/course.json + lessons/**.json + pictures/**.svg
data/test-courses/  courses for automated tests, loaded only with TEST_COURSE=1 (see testing.md, "The test course")
data/test-fixtures/llm/  canned model replies the dev AI provider answers from, one <purpose>.json per call site
deploy/          the systemd unit, update.sh and uninstall.sh of the release tarball
scripts/         build-pi.sh (cross-compile with zig), package-release.sh, dev-progress.sh, smoke.sh (local smoke test)
```

## Request flow

Every handler is `async`; all SQLite and argon2 work goes to Tokio's blocking
pool (`Db::call`) so the one async worker never stalls. Course generation runs
on its own OS thread (it is minutes of HTTP waiting) and writes through
`Db::with`. Building a course from its files (up to 2 s for a big one on the
Pi) also runs on a short-lived OS thread (`courses::off_worker`, #5): a
request for a course being built waits for that one build, every other
request is served meanwhile. Saving is split the same way (#84):
`courses::check` builds the files with no connection, `courses::record`
writes the outcome (`courses.error`, `title`, the cache) and skips it if the
files changed since they were checked, because the newer save records its
own. `courses::revalidate` is the two together and still builds while it
holds the database. The editor's autosave (`routes/courses/lessons.rs`)
commits the save, lets go of the database, builds with
`courses::check_off_lock` (own thread, request language), then records in a
second short `Db::call`. The other writers still use `revalidate`. Boot builds each built-in course once to check
it and keeps the seed course (`dev::SEED_COURSE`, else the first) in the
course cache, so the first request for it builds nothing; the others are
built on first use (keeping them all costs about 4 MB).

**Background jobs** (`src/jobs.rs`, #872). One `jobs` table and one OS
thread (`jobs`, not the Tokio blocking pool) that runs due rows. Taking a
row is one `UPDATE … RETURNING` that sets `state = 'running'` and a lease
(`locked_by`, `locked_until`, 10 minutes); a running row whose lease ran
out is taken again, so a job its server stopped in the middle of is not
lost. A failure goes back to `queued` after its kind's delay
(`jobs::RETRY_DELAYS` unless the kind says otherwise) until
`max_attempts`, then `failed` with `last_error`; each failure is a log
line, and so is a job of an unknown kind, which is refused. A periodic kind
queues its next run when a run ends. The thread wakes on `AppState::queue`
or when the next row is due, at least once a minute, and reaches SQLite
through `Db::with`. The first user is the hourly housekeeping, one kind
(`housekeeping`, #45) that runs every purge as a step in turn (sessions, the
game, AI job and tutor maps, rate limits, retention,
links, old job rows, `PRAGMA optimize` for the planner's statistics, then the WAL checkpoint); each step logs its
count, a failing step is logged and the next still runs. At
boot it gets a row due now if it has none, so a server restarted more
often than hourly still cleans up. AI generation still has its own thread. `/dev/jobs` lists the job rows in dev mode.

**Proxy sign-in.** With `SKRONGLE_PROXY_AUTH_HEADER` and `_FROM` set,
`proxy_auth::middleware` runs before every handler (not on `/dev/*`,
`/static/*` or `/logout`): a request whose TCP peer is in the list and that
carries the header gets a session for the account with that address (or
one made per `SKRONGLE_SIGNUP`), written into its `Cookie` header, so the
extractors below see it as any other session. See
`docs/self-hosting.md`, "Signing in at the proxy".

**Extractors.** `CurrentUser` resolves the session cookie (browser routes get
redirected to `/login`, `/api/*` routes get a 401). It asks the database once
per session: the user is then kept in memory (`auth/cache.rs`, #37) until a
row of a table the query reads changes (the connection's temporary triggers forget
them all), the session expires or a minute passes. Before that, an account signed in with a temporary password the owner set
(`users.must_change_password`, #635) is sent to `/password/change?next=<path>`
the same way (`/api/*`: 403); that page takes `MaybeUser`.
`MaybeUser` (pages that work logged out) does not check. Both set the
request's language to the account's `users.lang` (see [i18n](i18n)); before
them, the `i18n::middleware` layer picked one from the `lang` cookie or
`Accept-Language`. `Project` adds the active
course: `users.active_course` → the registry (`Courses::get`) → an
`Arc<ContentStore>`, and `start`, the unit index of the level the learner
started that course at (`projects.start_unit`, read in the same session
query). No active course, or one that no longer loads, sends
browser routes to `/courses` and API routes a 410, which the page
scripts answer by going to `/courses` too (#1083).

**Courses.** Built-in courses live in `data/courses/<id>/`; the folder name
must equal the manifest `id`. Boot builds each one once to validate it (a
broken course stops the server) and keeps only a `BuiltinCourse` summary
(manifest, totals, start levels) for the landing page, the course list and
its dialogs; the first request that needs a course's lessons builds it
again (#32). User-made courses are rows in `courses` with their files in
`course_files`; the first request for one builds a `ContentStore` from those
files. Both kinds share one cache, bounded by the bytes of their files
(least recently used out first), and a user-made one leaves it at its next
edit (`Courses::invalidate`). Both go through
`ContentStore::from_files`, so validation is identical. A course's non-JSON
files (the SVG pictures under `pictures/`) are kept in the `ContentStore` as
text and served by `/pictures/{name}` for the active course, with an ETag and
a content-security policy that forbids scripts. A user-made course is
visible to its owner, and read only to whoever the owner shared it with
(`share::can_open`; [features](features#sharing-a-course)): `POST
/courses/open/{id}` checks both, the editor and file routes the owner only.

**No e-mail.** The server sends no mail. Sharing shows the link to the owner, and a forgotten password gets the owner's temporary password (`docs/self-hosting.md`, "No e-mail").


**Templates** are server-rendered. The small scripts (`quiz.js`, `guide.js`,
`sprint.js`, …) post JSON to `/api/*` and render feedback inline. Everything
that talks (Numbers by ear, Date Dash, Listening, Sprint mode) shares `speech.js`,
a wrapper over the browser's speech synthesis: voice choice per
language, remembered in `localStorage`, the site-wide audio switch
(`speech:audio`), `say(text, lang)` and the cancel-then-settle sequence Chrome
needs. `base.html` loads it on every page inside `static/core.js` (one request, built by `scripts/bundle-core.sh` from `scripts/core-src/modal.js`, `static/speech.js` and `scripts/core-src/say.js`; edit those, run the script, commit both), together with `say.js`, which makes
the words of a language course tappable (read aloud) wherever the mode bar
carries `data-say-lang`, and with `modal.js`, which closes any modal on a
click on its backdrop (#226): a `<dialog>` opened with `showModal()`, or a
server-drawn `.modal-backdrop` overlay (by following its `[data-modal-close]`
link). A modal stays when it or its backdrop has `data-modal-static` (a
running AI job's box), or while a field of a form in it differs from what it
was drawn with (a password half-typed, a wish not sent yet). New modals need
nothing to get this. Course
specifics the browser needs (term/meaning labels and languages, helper
characters, praise strings) arrive as `data-` attributes or inside the Sprint mode
session JSON, never hard-coded.

## SQLite schema

```sql
users          (id AUTOINCREMENT, email, password_hash, created_at, email_verified_at, first_name, last_name,
                age_confirmed, owner, must_change_password, two_factor, active_course, lang, review_quiet,
                ai_effort, delete_at)
                -- id is never reused after a deletion (AUTOINCREMENT);
                -- email is what a password account signs in with, set once and never changed;
                -- password_hash is '' for accounts without one (proxy sign-in);
                -- email_verified_at: when the address was confirmed (a proxy-made account, the dev user), NULL = not;
                -- age_confirmed: the "I am at least N" box at sign-up;
                -- owner 1 opens /manage and links it from the account menu (by hand, make-owner, or the first
                -- account under SKRONGLE_SIGNUP=invite|closed; the dev user in dev mode);
                -- must_change_password 1: signed in with a temporary password the owner set (src/owner.rs);
                -- CurrentUser sends the account to /password/change until it picks its own;
                -- two_factor: the second sign-in step, '' (off) | 'totp' (src/two_factor.rs);
                -- active_course: the course the account is in;
                -- lang: the language the app speaks to this account in (i18n/<code>.yaml), '' = the browser's;
                -- review_quiet 1: "Review AI changes" does not open by itself;
                -- ai_effort: the effort Generate the course last ran at: low | medium | high (Fast, Balanced, Thorough);
                -- delete_at: unused, nothing reads or writes it; stays until the next schema change
sessions       (id, user_id, session_token UNIQUE, created_at, expires_at)
                -- session_token is the SHA-256 (hex) of the cookie's token (auth::hash_token), never the token
totp_keys      (user_id PRIMARY KEY, secret, created_at, confirmed_at, last_step)
                -- the account's authenticator app: the secret sealed with ChaCha20-Poly1305 (secrets::seal_totp,
                -- bound to the user; codes are worked out from it, so it can't be hashed),
                -- confirmed_at NULL while enrolling (purged after a day),
                -- last_step the newest RFC 6238 time step accepted: a code works once
two_factor_challenges (id, user_id, token_hash UNIQUE, attempts, created_at, expires_at)
                -- a sign-in waiting for its second step: token_hash is the SHA-256 of the `twofa` cookie's token;
                -- 10 minutes, 5 tries
signup_invites (id, token_hash UNIQUE, created_by, created_at, expires_at, used_at, used_by)
                -- invitation links for SKRONGLE_SIGNUP=invite (src/signup.rs): token_hash is the SHA-256
                -- of the link's token; one use (used_at, used_by), 7 days; created_by NULL for the first
                -- owner's link, else it cascades; used_by is set NULL when the invited account is deleted;
                -- gone 30 days after expiry
settings       (name PRIMARY KEY, value)  WITHOUT ROWID
                -- server-wide settings (src/settings.rs): 'ai_off' = '1' hides every AI feature for
                -- every account (no row: shown). The session query reads it, so auth/cache.rs watches it
server_api_keys (provider PRIMARY KEY, ciphertext, set_at)  WITHOUT ROWID
                -- the server's AI API keys, the owner's, set on /manage; provider is secrets::Provider::id;
                -- ciphertext is sealed by src/secrets.rs with the provider as associated data
jobs           (id, kind, payload_json, state, priority, run_at, attempts, max_attempts, locked_by, locked_until,
                last_error, created_at, finished_at)
                -- background jobs (src/jobs.rs); state 'queued' | 'running' | 'done' | 'failed';
                -- locked_by/locked_until: the lease of a running row. Done rows deleted after a day, failed after 30
courses        (id, owner_id, title, created_at, updated_at, error, state, source, job_id, brief, outline, notes)
                -- user-made courses, private to their owner (and whoever they share them with);
                -- state 'generating' | 'revising' | 'ready' | 'failed', source 'hand' | 'ai' | 'translate';
                -- error: the last validation error, NULL when it loads; brief/notes/outline let a stopped AI run resume
course_files   (course_id, path, content, updated_at)  PRIMARY KEY (course_id, path)
course_undo    (course_id, seq, title, outline, label, created_at, session, started_at)  PRIMARY KEY (course_id, seq)
course_undo_files (course_id, seq, path, content)  PRIMARY KEY (course_id, seq, path)
                -- the files from before each change to a course as a whole (feedback, new order,
                -- lesson removed or added), for Undo: seq > 0, newest (highest) first, ten per
                -- course at most. Redo steps share the table with seq < 0 (newest = lowest): an
                -- Undo files the course as it was there; any new change clears them;
                -- `session`: the editor page whose saves join this step (for ten minutes from
                -- `started_at`; `created_at` is then its latest save)
course_ai_runs (course_id PRIMARY KEY, label, created_at, excluded, edits, why)
course_ai_run_files (course_id, side, path, content)  PRIMARY KEY (course_id, side, path)
                -- the latest AI run's changed files, side 'before' | 'after' (missing: added or
                -- removed), for "Review AI changes" (src/review.rs); the next run replaces it.
                -- `excluded`: what the review left out (JSON `review::Exclusions`, '' for nothing)
                -- `edits`: the review's own text for lines the AI wrote (JSON `review::Edits`:
                -- path → row key → text, '' for none); `why`: the run's why and check lines per
                -- lesson (JSON `ai::LessonNote`s, '' for none)
course_shares  (id, course_id, label, token_hash UNIQUE, user_id, created_at, opened_at)
                -- a course shared by link (src/share.rs): token_hash is the SHA-256 of the
                -- link's token; user_id the account that opened the link (NULL until then, or when
                -- that account is deleted; the share stays and can be opened again). The course is
                -- open to that account only; `label` is a name the owner gave the link, or ''
projects       (user_id, course_id, created_at, last_opened_at, start_unit, motivator, set_up)  PRIMARY KEY (user_id, course_id)
               -- a course the learner opened; motivator: the course's setting, '' = the default;
               -- set_up: 0 until the first visit's settings wizard is left
user_progress  (user_id, course_id, lesson_id, mastery_score, times_tested, times_correct, streak,
                last_practiced_at, read_at)  PRIMARY KEY (user_id, course_id, lesson_id)
                -- read_at: when the lesson was first read or taught (reading check, or seen in any mode); NULL = unread
lesson_passes  (user_id, course_id, lesson_id, seen_at, seen_via, best_pct, best_via, passed_at, passed_via, runs,
                evidence, updated_at)  PRIMARY KEY (user_id, course_id, lesson_id)
                -- via is 'k' | 'd' | 'g' (Study, Sprint, Play: the modes' old letters, progress::VIA_*)
                -- | 'tutor' | 'agent' | 'legacy'; best_pct -1 = no counted run yet
quiz_history   (id, user_id, course_id, lesson_id, question_id, selected_answer, was_correct, answered_at)
journey_progress (user_id, course_id, lesson_id, step, completed, best_stars, run_correct, run_answered,
                updated_at)  PRIMARY KEY (user_id, course_id, lesson_id)      -- the Guide
user_vocab     (user_id, course_id, lesson_id, word, learned_at, times_seen, times_known, last_reviewed_at, due_at, gap_days)
                -- collected words and their review schedule (src/vocab.rs): due_at the start of the day it is due
                -- (NULL: no schedule yet), gap_days its rung on the ladder
lesson_notes   (user_id, course_id, lesson_id, body, updated_at)  PRIMARY KEY (user_id, course_id, lesson_id)
                -- the learner's own note on a lesson ("My note"); an emptied note has no row
lesson_touches (user_id, course_id, lesson_id, mode, at)  PRIMARY KEY (user_id, course_id, lesson_id)
                -- when and in which mode a lesson was last opened or worked on: where every mode resumes
activity_log   (id, user_id, course_id, kind, n, at)     -- n of activity::Kind `kind` done at `at`: the daily goal
earnings       (user_id, course_id, points, goals, day, day_points, streak)  PRIMARY KEY (user_id, course_id)
                -- running totals: effort points ever logged, daily goals reached, the local day last
                -- active, its points and the streak ending on it; XP and the streak read this, not the log
game_scores    (id, user_id, course_id, game, board, score_ms, wrong, played_at)
game_activity  (id, user_id, course_id, game, board, label, duration_ms, total, correct, score_ms, played_at)
                -- every finished game round or Sprint session
game_rounds    (user_id, course_id, game, rounds)  PRIMARY KEY (user_id, course_id, game)
                -- rounds finished per game, moved by games::record_activity_at: "rounds played" reads this
```

The schema is `src/db/migrations/v001_schema.sql` plus the later
migrations (`v002_no_skc.sql` drops `motivator_purchases` and turns a saved
`skc` motivator into `xp`; `v003_share_links.sql` rebuilds `course_shares` with a
`label` instead of an address), which `src/db/migrations.rs` (`MIGRATIONS`) loads; `PRAGMA user_version` records
how many have run, and a later change appends its own entry. A database
from a build before this schema (its `users` table still has a column of
the old history) is refused at start with one line naming the file: there
is no upgrade from it (`db::check_not_newer`). The indexes are in the
migration; `db::tests::every_foreign_key_is_indexed` keeps every foreign
key indexed, so deleting an account never reads a whole table, and the
housekeeping sweeps read through `quiz_history(answered_at)`,
`game_activity(played_at)`, `activity_log(at)`, `courses(state,
updated_at)`, `courses(state) WHERE notes <> ''`, `projects(course_id)` and
`users(active_course)`. What each table holds, and how long, is in the
[data inventory](data-inventory); a table with user data must be listed in
`db::tables::USER_TABLES` (a test checks every table in the schema).

**Model calls.** Every call site (`POST /courses/{id}/generate`,
`POST /courses/{id}/ai`, the tutor) uses the server's key (or a pasted one on the
generate form) and the provider's base URL; nothing in the app knows
whether the other end is real. There is no mock provider and no stub in
`ai::complete` (#298): in dev mode the default base URLs
(`ai::ProviderKind::default_base_url`, through `dev::provider_base_url`)
point at the dev AI provider on this server, and the seeded keys
are its keys, so fake replies come only from a key whose base URL points
there. Before any of that, every such handler asks `user.require_ai()` (pages) or
`user.require_ai_api()` (JSON): while the owner hides the AI features
(`settings::AI_OFF`, which the session query reads into `CurrentUser::ai_off`
for every account, #25) a page answers 403 saying so (`AppError::AiOff`) and
JSON a 403 (`AppError::ApiAiOff`), and the pages that link to those handlers
leave the links out (`user.ai_off` in the templates).
Every request carries an `x-skrongle-call` header naming its purpose,
key, job and vars (`ai::call_header`). The providers ignore it. The dev AI
provider picks its fixture reply by it. Error statuses are read as
responses (`http_status_as_error(false)`), so a provider's own message (a
used-up quota) reaches `ai::plain_failure`.

**AI jobs on the editor.** `POST /courses/{id}/generate` and
`POST /courses/{id}/ai` start a job and redirect to `/courses/{id}?job=<job>`;
the editor (`modal_job`) renders `ai_job_modal.html` for that job, or for the
job generating or revising the course when there is no `?job=`, except a
generation that succeeded (the editor is its result). The generator writes
the row before it marks the job done, so the editor reads the row again
when it saw a busy row and a done job (`settled`, #183); a row that still
says busy gets the finished job's modal, never a "being generated" page
with nothing to refresh it: a generation's with Continue, an Only add or
Allow changes job's still following (`AiJobModal::follows`), so the reload
opens the review (unless `users.review_quiet`: then a notice says the AI
has finished, and *Review changes* opens it). `courses.js` polls
`/api/generate/{job}` and reloads with `?job=` when it is done.
`/courses/generate/{job}`, the old status page, redirects there.

**Accepting the review.** The AI's changes are in the course as soon as the
job ends; the review lets the learner take parts back out. Its form posts
`file=<path>` per lesson left out and `lines=<path>:<key>,…` per group of
rows (`key` "3-4": line 3 before, line 4 after the AI, in the pretty-printed
JSON the diff compares), and `edit:<path>:<key>=<text>` per line edited in
place (#287: any line on the right, the AI's or an unchanged one, as raw
JSON text; it may hold several lines). `review::apply` rebuilds each file
from the run's before/after minus those rows, with the edited lines in
place of theirs (commas mended, the JSON must parse, else Accept is refused
by lesson name; an edited line is always in), refuses
a file that is neither as the AI wrote it nor as the last Accept left it
(edited by hand since), and the handler checks the course still loads,
commits one revision (an Undo step) and stores the set on `course_ai_runs.excluded` and `.edits`. A course that no longer loads is refused in the editor's words (`problems::Course::explain`). A course whose first generation stopped
(`failed`) has no editor: `stopped_redirect` sends its pages to `/courses`,
or to `/courses?generate=<id>` (its dialog, opened) when they come from its
job.

## Commands

The binary takes an optional first argument, read in `main` before the
config or the server (`owner::command`, #635): `reset-password <email>`
(prints a temporary password, sets `must_change_password`, ends the
account's sessions), `disable-2fa <email>`, `make-owner <email>` (sets
`owner = 1`) and `help`. They open `SKRONGLE_DB` like the server does
(migrations included) and change it directly, so they work while the server
runs (SQLite WAL, `busy_timeout`) or with it stopped, and send nothing; a
SKRONGLE_DB with no file there is refused rather than created. An
unknown argument prints the list and exits 2; no argument starts the server.
Built from the repo the binary is `spanish-learning`; a release names it
`skrongle` (`docs/self-hosting.md`, "If you lock yourself out").

## Configuration

Environment variables, all optional on a mini. Every name starts `SKRONGLE_`. For one release `crate::env` also reads the
older `LEARN_<name>` and `SPANISH_<name>` (#584): the `SKRONGLE_` one wins when both are set, and
startup logs one warning naming each old variable and its new name.

**Skrongle mini** (#632, #641, #578) is the self-hosted release: one
binary plus the defaults `deploy/skrongle.service` sets, marked *mini* in the table
below: `SKRONGLE_SIGNUP=invite`,
A mini has no e-mail and no outside service, and no setting for one. The startup line and `/healthz`
(`ok skrongle mini <version>`) name it. The
release tarballs are `skrongle-<version>-<arch>.tar.gz`
(`scripts/package-release.sh`); the pitch and the security
posture are in `docs/self-hosting.md`, "Skrongle mini".

| Variable | Default | Meaning |
|---|---|---|
| `SKRONGLE_BIND` | `0.0.0.0:7576` | Listen address; a bare port (`9000`) means every interface. A value that doesn't parse stops the server |
| `SKRONGLE_SIGNUP` | `open` (*mini:* `invite`) | Who may make an account (`src/signup.rs`, #634): `open` (anyone), `invite` (only through a link an owner makes on `/manage`) or `closed`. Under `invite` and `closed` the first account on a server with none is made only through the owner link the server logs at startup (a new one each start), and is made an owner. Read as is, with no older spelling; any other value stops startup. `deploy/skrongle.service` sets `invite`, `deploy/aws` nothing |
| `SKRONGLE_PROXY_AUTH_HEADER`, `SKRONGLE_PROXY_AUTH_FROM` | unset | Sign-in by a reverse proxy (`src/proxy_auth.rs`, #642): the header holding the person's e-mail address, and the proxy's addresses (IPs or CIDRs, comma-separated) it is trusted from, judged by the TCP peer, never `X-Forwarded-For`. Both or neither (one alone stops startup). A trusted request with the header is signed in without password, two-factor or rate limit; an unknown address follows `SKRONGLE_SIGNUP` (`open` makes a password-less account with its address confirmed). `/login` and `/register` only say the proxy signs in. Read as is, no older spelling. |
| `SKRONGLE_DATA_DIR` | `data` | Directory containing `courses/` |
| `SKRONGLE_DB` | `learn.db` | SQLite file (WAL + SHM files are created next to it) |
| `SKRONGLE_COURSES_PER_USER` | `50` | Courses one account may make (no limit in dev mode unless set) |
| `SKRONGLE_COURSE_BYTES` | `8388608` | Total size of one user-made course's files |
| `SKRONGLE_COURSE_BYTES_PER_USER` | `33554432` | Total size of the files of one account's courses |
| `SKRONGLE_RETAIN_HISTORY_DAYS` | `730` | Delete answers in `quiz_history`, finished rounds in `game_activity` and the daily goal's `activity_log` older than this many days (at startup and hourly). `0`: keep them while the account exists. A placeholder until the retention periods are decided (TODO.md) |
| `SKRONGLE_KEEP_NOTES` | `0` | `1`: keep the notes of an AI course once it is ready (revisions then see them). Default: emptied within the hour |
| `SKRONGLE_FAILED_COURSE_RETENTION_DAYS` | `30` | Delete an AI course whose generation failed (with its notes and files) once it has gone this many days without a resume or edit (at startup and hourly, `jobs::retention::apply_retention`). `0`: keep them |
| `SKRONGLE_MIN_AGE` | `14` | The age the sign-up forms' age checkbox names (#976) |
| `SKRONGLE_LOG` | `info` | One level: `error`, `warn`, `info`, `debug` or `trace`. Per-module filters like `skrongle=debug` are not supported; an unrecognised value logs one warning and runs at `info`. |
| `SKRONGLE_SECURE_COOKIES` | `0` | Set `1` behind a TLS proxy to add `Secure` to cookies. Required outside mini and dev mode: the server refuses to start without it |
| `SKRONGLE_BASE_PATH` | *(empty)* | URL prefix when mounted behind a reverse proxy, e.g. `/spanish` |
| `SKRONGLE_PUBLIC_URL` | — | Public URL, for the links the app hands out: the owner link, invitations, share links (unset: links use the request's host, `X-Forwarded-Host` behind a proxy) |
| `SKRONGLE_AI_PRICES` | see `prices::DEFAULT_PRICES` | `model=in/out[/cached],…` in dollars per million tokens, added to (or replacing) the built-in table of Anthropic's list prices; `*` prices unknown models (default: the dearest rate) |
| `SKRONGLE_OPENAI_BASE_URL` | `https://api.openai.com` | Where the OpenAI provider sends requests: any OpenAI-compatible server (Ollama, LM Studio) |
| `SKRONGLE_ANTHROPIC_BASE_URL` | `https://api.anthropic.com` | Where the Anthropic provider sends requests (a proxy, the dev AI provider) |
| `SKRONGLE_GOOGLE_BASE_URL` | `https://generativelanguage.googleapis.com/v1beta/openai` | Where the Google provider sends requests |
| `SKRONGLE_DEEPSEEK_BASE_URL`, `SKRONGLE_MISTRAL_BASE_URL`, `SKRONGLE_XAI_BASE_URL`, `SKRONGLE_OPENROUTER_BASE_URL` | `https://api.deepseek.com`, `https://api.mistral.ai`, `https://api.x.ai`, `https://openrouter.ai/api/v1` | Where those providers send requests (#24). An Ollama-compatible server has no variable: its base URL is the one the owner saves on Manage Users |
| `SKRONGLE_AI_TIMEOUT_SECS` | — | How long one model request may take, in place of 900 s for generation and 120 s for a chat turn (`scripts/e2e.sh` sets 8) |
| `SKRONGLE_AI_JOB_BUDGET_SECS` | `3600` | How long one AI job (a generation, Only add, Edit lessons, Allow overall changes) may run in all; past it the job stops before its next request, and no request runs past it |
| `SKRONGLE_ALLOW_PRIVATE_AI_URLS` | `0` | `1`: a base URL typed into an AI form may point at loopback, private or link-local addresses (a self-hosted Ollama on the LAN), on any port. Off, such URLs are refused, and only ports 443 and 80 are allowed, so learners cannot make the server call its own network (dev mode always allows them). `SKRONGLE_OPENAI_BASE_URL` is never checked |
| `SKRONGLE_SECRET_KEY` | *(generated)* | 64 hex characters; encrypts stored API keys. Unset: read from or created at `<SKRONGLE_DB>.key` (mode 0600) |
| `SKRONGLE_NEW_SECRET_KEY` | `0` | `1`: the secret key is lost for good; create a new one, delete the saved API keys it cannot read, and turn off the authenticator apps it cannot read (two-factor off for those accounts, #1068). Without it the server refuses to start when the database has saved keys but neither `SKRONGLE_SECRET_KEY` nor `<SKRONGLE_DB>.key` is there, or the key there opens none of them |

A build with the `dev` cargo feature (`./.dev-start`, the test scripts, CI;
never a release) also reads the dev settings: `SKRONGLE_DEV`,
`SKRONGLE_DEV_SEED`, `SKRONGLE_DEV_RELOAD`, `SKRONGLE_DEV_FORCE`,
`SKRONGLE_RNG_SEED`, `TEST_COURSE`, `SKRONGLE_SQL_TRACE` and the
`SKRONGLE_LLM_*` settings of the simulated provider. Without the feature
they are unknown settings. Dev mode and the dev AI provider are described in
[testing](testing).

## Generating courses with an LLM

`src/ai.rs`, driven by the editor's Generate dialog (`POST
/courses/{id}/generate`, `src/routes/courses/generate.rs`). It writes into a draft
(`courses::draft_of`: the starter course with only names changed) or
resumes a stopped generation. On submit the handler empties the draft
(`courses::restart_generation`) and marks the row `generating` (with no files,
so `Courses::get` never loads or caches a half-written course: it reads
`load_ready_files`), then starts the job on its own OS thread. The key, from
the form or decrypted from the profile, is held in the job in memory and
dropped when the job ends; it is never written with the course.

One request produces the manifest and an outline, both saved at once
(`course.json`, `courses.outline`); one request per lesson produces the
lesson file, validated against the manifest and everything written so far; a
failed validation is sent back to the model (up to three attempts per
lesson). Files are saved as they arrive. When the job ends the row becomes
`ready` or `failed` (`courses::finish_generation`). Each job is also a `jobs` row of kind
`generate_course` (`src/ai/queue.rs`, #874) that names the job, the course
and how to rebuild the request, never the key. One job runs at
once (`ai::slots()`, #43); a second request while it runs is refused at
once ("a course is already being generated; try again when it finishes")
and leaves no row waiting. That holds for generations, revisions, added
lessons and translations alike. One heartbeat thread renews the lease of
the running row every two minutes, and the generator thread marks its row
done or failed. At boot `ai::at_boot` queues a generation with the
server's key or a saved key again; the runner (`jobs.rs`) hands it to a
new generator thread (`Step::Handed`) and it resumes by itself; any other unfinished job (a pasted key, a revision, a
translation) is failed, and `fail_interrupted` marks its course `failed`,
as before. A failed run resumes from the saved outline and notes, writing only the
lessons that are missing. Cancelling sets a flag the job checks before each
request; so does the job's time budget (`ai::JOB_BUDGET`, one hour), which
also cuts each request's timeout to what is left. `ai::start` checks
capacity and takes the slot under one lock; a slot is held by the
generator thread and freed when it exits, so a job dropped from the table
(its account deleted) still counts until then. The routes add a per-account
quota (`security::AI_JOBS_PER_USER_HOUR`/`_DAY`), and at most
`ai::MAX_FINISHED_PER_USER` finished jobs are kept per learner.

Each lesson request starts with a digest of the lessons already written
(`arrange::written_digest`, #446): one line per lesson with its title, its
`vocabulary` terms and its `## ` headings, and two sentences asking the model
to build on them, not teach them again, and use the same words. The list is
capped at `arrange::DIGEST_CAP` (2,000 bytes): past it the older lessons keep
only their terms, then the oldest go whole. The first lesson's request has
none. After the manifest comes `arrange::lesson_context`: the course map
(each unit from the manifest with its outline lessons in order, numbered,
with summary and focus, this lesson marked) and the lesson just before and
just after this one in full. A neighbour that is not written yet (the next
lesson, in a fresh run) comes as its outline entry.

The system prompt is `ai::DEFAULT_INSTRUCTIONS` unless the learner edited it
on the form; every user prompt embeds `docs/course-format.md` verbatim (so the
documentation is the specification the model follows) and the learner's
material inside `<material>` tags. The reference opens the user prompt, then
the kind's rules, then the part that changes per request
(`prompt::with_reference`): providers cache by prefix, and the reference is
the one part identical in every request. Anthropic gets it as its own first
text block with `cache_control` (`providers::anthropic_content`), so repeat
lessons read it from the cache; OpenAI-compatible providers get the same
text as one string. The cost estimate (`budget.rs`) still prices every
request at the full input rate, an upper bound. It also carries the rules for one kind of
course (`ai::SubjectKind`, `src/ai/prompt.rs`): the outline gets the kind
picked on the form, or all three while the model decides and names its
choice as the outline's `kind`; that kind is saved in `courses.outline` and
every lesson, revision and added lesson gets only its rules.

A course that exists changes with AI through `POST /courses/{id}/ai`, in
one of three modes (`mode`, the Use AI box's Add / Edit / Add and edit radios
in `templates/ai_form.html`, #1241; `revise::AiMode`, kept on a refused
form's `Refill`): `add` (Only add, `ai::add_lesson`: one request
writes a lesson and says where it goes, checked by `arrange::only_adds`),
`edit` (Edit lessons, `src/ai/edit.rs`: an `edit-plan` request picks which lessons
to keep or rewrite, then one `revise-lesson` request per rewrite, checked by
`arrange::only_edits`: the same files, `course.json` byte for byte, each
lesson in its place; with `lesson=<file>`, Edit with AI on one row, #1242: one
`edit-lesson` request in `src/ai/edit_lesson.rs`, checked by `arrange::only_edits_one`) and `change` (Allow overall changes, `ai::revise`: a
`revise-plan` request may keep, rewrite, add, drop and reorder, then one
request per lesson to write). Each saves nothing until the whole result
validates, then commits it as one revision that Undo takes back.
Both plans may also `check` a lesson (`src/ai/check.rs`, #1238): one
`check-lesson` request reads it for errors and returns it unchanged (the
file stays byte for byte) unless a fact is wrong. A `check` or `rewrite`
reply wraps the lesson as `{"lesson", "why", "check"}`; the why and check
lines go on the job (`Status::notes`) and are kept with the run
(`course_ai_runs.why`), and Review AI changes shows them per lesson.

Supported wire formats: the Anthropic Messages API (`POST /v1/messages`,
default model `claude-opus-5-5`, with the server-side refusal fallback enabled
on the models that support it) and OpenAI-compatible chat completions
(`POST /v1/chat/completions`, any base URL, so Ollama and similar work;
Google's Gemini is reached through its OpenAI-compatible endpoint). The same
client (`ai::chat`) serves the AI tutor with a multi-turn transcript.

## AI job states

Whether an AI job is running, done or failed is kept in three places that
change at different moments (#85). This section says who writes each one,
when, and who reads it. Line numbers are as of the commit it was written at;
they drift, so find the function by name first. It was written by reading
the code, not by running it. "Not traced" means the cell was not followed
to the end.

The three places:

- **Course row**: `courses.state` (`generating`, `revising`, `ready`,
  `failed`), `job_id` and `error`.
- **Jobs row**: a `jobs` row of kind `generate_course`: `queued`,
  `running`, `done` or `failed`, with `locked_until` (the lease) and
  `last_error`.
- **Memory**: `ai::Jobs` (`queue.rs:73`): `map` (every job by id, with its
  `Status`), `live` (one entry per generator thread that has not exited),
  `waiting` (jobs adopted after a restart that the runner has not started).
  Each `Job` also holds a cancel flag and a deadline.

A fourth place, the course content cache (`Courses`, `courses.rs:412`), is
filled by `revalidate`; it comes up in window 8.

How the writes are grouped:

- Every `Db::call` / `Db::with` closure holds the one connection mutex for
  its whole run (`db.rs:169`, `db.rs:182`), so no other request runs between
  two statements of the same closure. A closure is a transaction only when
  its function opens one (`restart_generation`, `create_course`,
  `commit_revision`, `record_ai_run`, `jobs::finish`).
- The memory lock (`Jobs.inner`, and each `Job.status`) is a different mutex.
  No code holds the database mutex and a memory lock together as one unit.
- So between a course-row write, a jobs-row write and a memory write, other
  requests can run. Where a cell says "separate", that is the case.

### Table

| Event | Course row | Jobs row | Memory | Same transaction or lock? | Who reads it |
|---|---|---|---|---|---|
| Submit: generate (fresh) | `restart_generation` (`courses.rs:1565`, own transaction: deletes files and undo steps, clears outline and error), called at `generate.rs:582`. Then `set_generating` (`courses.rs:582`, called at `generate.rs:590`): `generating`, `job_id`, `error` NULL | `ai::submit` (`queue.rs:601`) calls `hold` (`queue.rs:395`): `enqueue`, then `UPDATE` to `running`, lease now + 600 s, `attempts` 1. `payload.resume` is true only if no key was pasted (`generate.rs:533`) | `start` (`queue.rs:236`), under one memory lock: `capacity`, `make_room`, insert into `map` and `live`. Then `launch` (`queue.rs:250`): phase "Starting", heartbeat thread (first time), generator thread | No. Four separate steps in this order: `restart_generation`, `set_generating`, `hold`, `start`. `hold`'s two statements share one closure but no transaction | Editor: `modal_job` (`editor.rs:102`) reads row `state`/`job_id` and memory. Course list: `broken_card` (`picker.rs:236`), row and memory. Capacity: `check_capacity` (`queue.rs:197`), memory only. Progress polling: `generate_api` (`generate.rs:660`), memory only |
| Submit: resume a stopped generation | No `restart_generation` (`generate.rs:568`). `set_generating` (`generate.rs:590`) clears `error`; files, outline and notes stay | Same as above | Same as above; the request carries the saved outline | Same as above | Same as above |
| Submit: add a lesson, edit, edit one lesson, revise | `set_revising` (`courses.rs:593`, called at `revise.rs:123`): `UPDATE ... WHERE state = 'ready'`, so only one request wins. `error` is not touched | Same `submit` (`revise.rs:143`). Payload mode is `revise` for all four (`queue.rs:369`), `resume` false | Same `start` | No. Two steps: `set_revising`, then `submit` | Same readers. Editor locks writes while `is_busy()` (`editor.rs:306`, `files.rs:259`) |
| Submit: translate a copy | New course: `create_course` (own transaction, `courses.rs:995`) then `start_translation` (`courses.rs:568`): `generating`, `source` `translate`, `job_id`, `notes` = plan. Both in one closure (`create.rs:115`-`117`). Resume of a stopped translation: `set_generating` (`create.rs:251`) | `submit` (`create.rs:274`); mode `translate`, `resume` false | Same `start` | The two course writes share one closure and mutex, not one transaction. Then separate from the jobs row and memory | Same readers. `gen_mode` (`generate.rs:216`) offers no Generate dialog for `source = translate` |
| A lesson is written (generate, resume) | `write_file` per lesson (`ai.rs:569`): `course_files` only; `courses.state` and `job_id` untouched. The first request writes the manifest and the outline together (`ai.rs:487`-`491`: `write_file`, `set_outline`: one closure, no transaction) | Nothing. The heartbeat renews the lease separately, every 120 s (`queue.rs:320`) | `lessons_done` and a log line, after the write (`ai.rs:572`) | No. Files are written before memory is updated | Progress bar and list card: memory (`/api/generate`, `courses.js:1908`). A lesson written to the database is not visible to the page until the next poll |
| A lesson is written (translate) | `write_file` plus `set_notes` (`ai.rs:1118`-`1121`, one closure, no transaction): file and plan progress | Nothing | `lessons_done` (`ai.rs:1127`) | Course writes share a closure; memory is separate | Same |
| A lesson is written (revise, add, edit) | Nothing yet. Files are swapped once at the end: `commit_revision` (`courses.rs:661`, one transaction) then `revalidate` in the same closure (`ai.rs:904` revise, `ai.rs:1024` add, `edit.rs:322` edit, `edit_lesson.rs:108` one lesson). `state` stays `revising` until the thread's end sequence | Nothing | `lessons_done`: `edit.rs:137` (edit), `ai.rs:1017` (add). Revise: not traced | Commit and revalidate share a closure; commit is a transaction, revalidate is not | Progress: memory. Files on the page: the course row's files, already new while `state` is still `revising` (window 8) |
| End: success | Thread end sequence, `launch` closure (`queue.rs:252`-`305`), three separate `db.with` calls and one memory update, in this order: (1) `record_ai_run` (`queue.rs:276`, own transaction; skipped for translate); (2) generate/resume/translate: `finish_generation(None)` (`courses.rs:935`): `ready`, `error` kept as it was (`COALESCE`); revision kinds: `finish_revision` (`courses.rs:600`): `ready` only if `state = 'revising'`. Both at `queue.rs:288`-`292` | (3) `settle` (`queue.rs:403`, called at `queue.rs:294`): `done`, lease cleared, `finished_at`; only if the row is still `queued` or `running` | (4) `job.update` (`queue.rs:297`): `done`, `finished_at`, phase "Done". Then the closure ends and `LiveSlot::drop` (`queue.rs:84`) removes `live` and wakes the runner | No. Four steps, each its own lock hold. (3) runs only if (2) returned `Ok` (`finished.and_then`, `queue.rs:294`) | Editor: row first, then memory; `settled` (`editor.rs:120`) re-reads the row if the row is busy and the job is done. Course list card: row; the card script reloads once when memory says done and the card still says generating (`courses.js:1908`). Capacity: memory (`live`) |
| End: failure | Same sequence. Generate/resume/translate: `finish_generation(Some(err))`: `failed`, `error` set; lessons and outline stay, so Resume works. Revision kinds: `finish_revision`: `ready`; the course is as before, unless the error came after `commit_revision` ("does not load ... Undo it", `edit_lesson.rs:113`), when the new files are in | `settle` with the error: `failed`, `last_error` | Phase "Stopped", `error` | Same as success | Editor: `stopped_redirect` (`editor.rs:520`) sends a `failed` course to the course list. `modal_job` shows a failed job's modal until closed |
| Cancel a running job | Nothing | Nothing | `generate_cancel` (`generate.rs:625`) finds the job by `row.job_id` and `!done`, calls `ai::cancel` (`queue.rs:615`) which falls to `Job::cancel` (`ai.rs:322`): sets the flag, phase "Cancelling…", a log line. The thread sees it at the next `check_cancelled` (`ai.rs:329`) and ends as a failure with a "Cancelled" message | Flag and phase are memory only | Modal: memory. Row and jobs row change only when the thread ends (End: failure) |
| Cancel with no job in memory | Generation only: `finish_generation(Some("cancelled"))` at `generate.rs:629`-`634` (only when `is_generating()`). A `revising` course: nothing (`generate.rs:635`) | Nothing | Nothing | n/a | Course list, editor |
| Cancel a job adopted after a restart and not started | `queue::cancel` (`queue.rs:630`-`641`): `finish_revision` or `finish_generation(Some(msg))` | `settle` with the message, in the same closure | `waiting` entry removed and cancel flag set first (`queue.rs:619`-`621`); job marked done after the database write (`queue.rs:643`) | Course and jobs row: one closure, not one transaction. Memory: before and after, separate | Course list, editor |
| Job time budget reached | As End: failure | As End: failure | `check_cancelled` (`ai.rs:329`) returns the "took longer than N minutes" error before the next request; `llm` (`ai.rs:359`) also cuts the request timeout to the time left. Budget set in `new_job` (`queue.rs:205`), reset in `launch` (`queue.rs:251`) | As End: failure | As End: failure |
| Server stops mid-job | Left as it was: `generating` or `revising`, `job_id` set | Left `running`, `locked_until` in the future, `locked_by` this server's pid. Whether shutdown waits for the generator thread: not traced | Lost | n/a | Nobody until the next boot |
| Server starts again | `fail_interrupted` (`courses.rs:948`, `main.rs:460`): every `revising` course becomes `ready`; a `generating` course with no `queued`/`running` jobs row for it becomes `failed` with "interrupted". A course whose row was kept `queued` stays `generating` | `at_boot` (`queue.rs:434`, `main.rs:456`, before `fail_interrupted`): `payload.resume` rows become `queued`, run now; all other `queued` or `running` rows are settled `failed` with "interrupted" | `adopt` (`queue.rs:449`, `main.rs:527`): the `queued` rows go into `map` (phase "Waiting") and `waiting` | No. Three steps, three `db.with` or memory calls, in the order `at_boot`, `fail_interrupted`, `adopt`. Each is safe to run again | Course list and editor: `generating` plus a "Waiting" job |
| Server starts again, key saved on the server (resume true) | Stays `generating` until the runner reaches the row | The runner's gate (`slot_free`, `queue.rs:486`) opens; `claim` (`jobs.rs:154`) sets `running`, `attempts` + 1. `handler` (`queue.rs:492`) rebuilds the request with `rebuild` (`queue.rs:564`): the row must still name this job and be `generating`, and `server_provider` (`queue.rs:574`) must still give a key. If so: `launch`. Otherwise `give_up` (`queue.rs:538`) | Job moves from `waiting` to `live` (`queue.rs:512`, `527`-`528`); a new generator thread resumes from the lessons written | `give_up`: course and jobs row in one closure, not one transaction; memory after. Past `max_attempts` (3) claims the handler gives up without rebuilding (`queue.rs:519`) | Course list and editor follow the job again |
| Server starts again, key pasted (resume false) | `failed` with "interrupted" (via `fail_interrupted`); keeps its Resume button | `failed`, `last_error` "interrupted" (`at_boot`) | Nothing | Separate | Course list |
| Account deleted while its job runs | No code that deletes an account was found in `src/` (searched `DELETE FROM users`; only a test, `db/tests.rs:85`). The schema would cascade the courses and files away (`v001_schema.sql:105`, `:122`; foreign keys on, `db.rs:31`) | No foreign key; the row is not touched | `Jobs::drop_user` (`queue.rs:177`) sets the cancel flag, drops the user's jobs from `map` and `waiting`; the thread keeps its `live` entry until it exits. It has no caller outside a test (`ai/tests.rs:233`). What the thread does when its course is gone: not traced | n/a | n/a |
| Lease expires while the thread is alive | Nothing | `locked_until` passes. `renew` (`queue.rs:412`) every 120 s should prevent it; if the update fails it is logged only (`queue.rs:330`) | Nothing | n/a | Nobody reads it while the thread lives: `slot_free` is false, so the runner does not claim `generate_course` rows (`jobs.rs:389`, `main.rs:531`). `handler`'s "still running here" branch (`queue.rs:497`) is the only code that looks at the case; no path to it was found |

### Where two stores can be read in different states

Each window says what a reader sees, and whether a test covers it. "Test"
means a unit test in `src/ai/tests.rs`, `src/routes/tests.rs`,
`src/routes/courses/tests.rs`, `src/courses/tests.rs` or `src/jobs/tests.rs`,
or an end-to-end spec in `e2e/specs/` that I looked at (`create.spec.ts` and
`revise.spec.ts`, by their titles). A window with no test named has none that
I found.

1. **Submit gap.** The course row says busy before the jobs row and the
   memory entry exist (`generate.rs:590` to `queue.rs:244`). The editor has
   no job to show (`modal_job` finds nothing); `check_capacity` says free;
   `generate_cancel` finds no job and, for a generation, fails the course at
   `generate.rs:629`, after which `submit` goes on to start the job anyway.
   Untested.
2. **Submit refused after the row was marked.** `start` checks capacity
   again under its lock (`queue.rs:241`). If it refuses, `submit` settles the
   jobs row `failed` (`queue.rs:607`) and the route then fails the course
   (`generate.rs:618`, `revise.rs:157`, `create.rs:282`). In between the course
   still says busy. For a fresh generation the files were already deleted by
   `restart_generation`. Partly tested: `ai/tests.rs:200` tests that `start`
   refuses, but does not read the rows.
3. **Two Generate posts at once on one course.** Both can read a course that
   is not busy (`generate.rs:473`) before either calls `set_generating`
   (`generate.rs:590`). Both then write the course, and `hold` makes two jobs
   rows. `set_generating` has no `WHERE state` guard, unlike `set_revising`.
   One `start` wins; the other is refused, settles its row, and the route marks
   the course `failed` (`generate.rs:618`) while the first job runs, with
   `job_id` possibly the loser's. Untested for generation. Revisions are
   covered: `e2e/specs/revise.spec.ts:308` ("two revisions asked for at the
   same moment start only one job") and `ai/tests.rs:200` (eight `start`
   calls, one wins).
4. **End of job, row final but memory not done yet** (between
   `queue.rs:288` and `queue.rs:297`). The editor sees a ready course and a
   job that is not done, and draws a running modal for a moment
   (`modal_job`: `finished` is false, `editor.rs:111`). `check_capacity` still
   refuses a new job. Untested.
5. **End of job, row busy but memory done.** The stale read `settled`
   covers (`editor.rs:120`). Tested with a faked job in memory:
   `editor_of_a_course_whose_job_just_finished` (`routes/tests.rs:1178`). The
   test does not run the real thread, so the order of the real writes is
   asserted only by reading the code. The course list card has no `settled`;
   it relies on the order of writes and a single reload (`courses.js:1908`).
6. **Memory done, slot still held** (between `queue.rs:297` and the thread
   exit that runs `LiveSlot::drop`, `queue.rs:84`). `check_capacity` refuses
   a new job although the job shows done. Partly tested: the slot being held
   until the thread exits (`ai/tests.rs:200`, after `drop_user`), not the
   moment after `done`.
7. **The end-of-job database write fails** (`queue.rs:294`-`296`, logged
   only). The course stays busy, the jobs row stays `running`, memory says
   done, and `settle` was skipped. The lease is no longer renewed (the `live`
   entry is gone). After 600 s the runner claims the row and `handler` settles
   it `failed` with "cancelled" because the job in memory is done
   (`queue.rs:511`-`516`); the course stays busy until the next restart.
   Untested.
8. **Files and cache ahead of the row.** A revision's files are swapped and
   the cache refreshed (`ai.rs:904` etc.) while the course still says
   `revising`. A generation's final `revalidate` (`ai.rs:578`) puts the
   finished course into the content cache (`courses.rs:1267`) while the row
   still says `generating`; `Courses::get` answers from the cache without
   looking at the row (`courses.rs:416`). Whether any page asks for a course
   by id without reading its row first: not traced. Untested.
9. **The generator thread dies without its end sequence** (a panic; whether
   one can happen: not traced). The slot is freed (`LiveSlot::drop`), but the
   course stays busy, the jobs row stays `running`, memory is not done. After
   the lease runs out the runner claims the row: a resumable generation is
   rebuilt and run again; any other kind goes to `give_up`
   (`queue.rs:519`-`522`). Untested.
10. **Cancel racing the end of the job.** `generate_cancel` checks `!done`
    (`generate.rs:627`) and then `Job::cancel` sets the phase to
    "Cancelling…" without checking `done` again (`ai.rs:322`). A job that
    finished in between shows "Cancelling…" with `done` true. Untested
    (`ai/tests.rs:1122` cancels a new job). Cancel of a running job is covered
    end to end: `e2e/specs/create.spec.ts:224`, `e2e/specs/revise.spec.ts:259`.
11. **Cancel of a revision with no job in memory** is a no-op
    (`generate.rs:635`), so the course stays `revising` until a restart.
    Reached through windows 1 and 7. Untested.
12. **Cancel of a job adopted after a restart.** `waiting` is cleared first,
    the database written next, memory marked done last (`queue.rs:619`,
    `:634`, `:643`); the runner can claim the row in between and settle it
    "cancelled" (`queue.rs:513`-`516`). Untested.
13. **After a restart.** `at_boot`, `fail_interrupted` and `adopt` are three
    steps (`main.rs:456`, `:460`, `:527`). The three together are tested
    (`ai/tests.rs:259`; `fail_interrupted` alone, `courses/tests.rs:134`).
    The runner's side is not: `handler`, `rebuild`, `give_up`, the attempts
    count and the `queued` requeue at `queue.rs:503`-`508` have no test.
    Whether the server answers requests before `adopt` has run: not traced.
14. **Lease and gate.** Claiming an expired lease is tested
    (`jobs/tests.rs:38`), and so is the gate that keeps `generate_course`
    rows from being claimed (`jobs/tests.rs:159`). `renew`, the heartbeat
    thread, and `handler`'s "still running" branch (`queue.rs:497`) are not.
15. **Account or course deleted while a job runs.** No account-deletion code
    was found, and `Jobs::drop_user` has no caller outside a test
    (`ai/tests.rs:233`, which checks only that the slot stays taken). For a
    course: `delete` (`files.rs:174`) reads the row, then deletes in a
    later call, so a job started in between would lose its course; what the
    thread does then: not traced. Untested.

Fifteen windows. One is covered by a test of the window itself (5). Four have
a test of one side only (2, 6, 13, 14). Ten have none (1, 3 for generation,
4, 7, 8, 9, 10, 11, 12, 15).

Also true, and not a window: a course keeps its `job_id` after the job ends
(`finish_generation` and `finish_revision` do not clear it), so readers must
check `is_busy()` before following it (`modal_job` does, `editor.rs:107`).

## Deploying

A release is a tarball built by CI on a `v*` tag (`scripts/package-release.sh`,
`.github/workflows/release.yml`); a self-hoster unpacks it and runs
`deploy/skrongle.service` (`docs/self-hosting.md`). Read-only install, database in `/var/lib/skrongle`.

User-made courses live in the database, so the install needs no other
writable path, and a release never touches the database.

## Security notes

- Passwords: argon2id with a per-user salt; unknown addresses still run a
  verification against a dummy hash so login timing does not reveal existence.
- Sessions: 256-bit random tokens, 30-day TTL, deleted on logout, purged hourly.
- CSRF: the cookie is `SameSite=Lax`, the JSON API only accepts
  `Content-Type: application/json`, and every POST from another site is
  refused with a 403 before its handler runs (`security::csrf`: the
  browser's `Sec-Fetch-Site`, or else `Origin` against `Host`); requests with
  neither header (curl, agents) pass.
- Every response carries `Content-Security-Policy` (`security::CSP`: scripts
  and styles from the app only, `frame-ancestors 'none'`), `X-Frame-Options:
  DENY`, `X-Content-Type-Options: nosniff` and `Referrer-Policy:
  same-origin`; with `SKRONGLE_SECURE_COOKIES=1` also
  `Strict-Transport-Security`. The inline scripts still need
  `'unsafe-inline'`. In dev mode only, `/login/code/device` (the dev device
  framed by `/login/code`) sets `frame-ancestors 'self'`.
- Rate limits (in memory, `security::RateLimits`, off in dev mode): 30
  sign-in attempts per address per 10 minutes, 10 wrong passwords per
  account per 15 minutes, 20 registrations per address per hour, 10
  password checks per account per 15 minutes (every profile proof of
  ownership, `prove_owner`, shares the allowance; its wrong passwords and
  codes count against sign-in, and a blocked sign-in blocks it). The address is the peer, or
  the last `X-Forwarded-For` entry when the peer is a proxy on loopback;
  an IPv6 address counts by its /64. Each limit keeps its own table of at
  most 64 keys (`caps::MAX_RATE_LIMIT_KEYS`); a full table drops ended
  windows first, and then depends on the limit (`security::WhenFull`).
  The per-account limits refuse a new key (as over the limit) rather than
  evict a running window, so a flood cannot reset an account's
  wrong-password count; to keep that table from filling, the sign-in
  handler counts a wrong password against an account only when the address
  has one (a made-up address is still counted against the client's
  address). The per-address limits (sign-in and registration) evict the
  entry with the fewest hits (ties: the oldest window) and admit the new
  address, so a flood of addresses cannot lock everyone else out, while an
  address at its limit, having the most hits, stays blocked. A full table
  logs a warning at most once per window.
  Changing the password or two-factor sign-in
  and *Sign out everywhere else* sign out the account's other sessions; the
  owner's temporary password signs out all of them.
- Two-factor sign-in (`src/two_factor.rs`, [features](features#two-factor-sign-in)):
  with a method on, the password only leaves a challenge named
  by a 256-bit token in the `twofa` cookie (HttpOnly, SameSite=Lax, 10
  minutes; the table keeps its SHA-256); the session is made after the
  code, 5 tries per sign-in. Wrong codes (the last one too) also count as
  wrong passwords for the account's limit, and while that limit is reached
  `/login/code` refuses too. Authenticator codes: RFC 6238, ±1 step, a step accepted once
  (`totp_keys.last_step`), compared in constant time. The TOTP secret
  can't be hashed (codes are worked out from it), so it is sealed with the
  AI API keys' vault (`secrets::seal_totp`: same master secret, its own
  associated data bound to the user). Turning the second step off or
  switching it needs the password (checked like `/login`: argon2 off the
  async worker, 10 tries per account per 15 minutes, wrong ones count
  against the sign-in limit); an account without a password gives a code from its
  current method instead. `/dev/login` skips the second step.
- Course files are validated on every save and limited to 1 MiB each and 400
  per course, 8 MiB per course and 32 MiB per account in all, and 50 courses
  per account (`courses::Limits`, checked by `courses::room_for` in the same
  `Db::call` that writes). One account may write course files 120 times in
  5 minutes (`security::COURSE_WRITES_PER_USER`). The cache of built
  courses (built-in and user-made) keeps at most 8
  of them (`courses::MAX_STORES`, one active course per account; boot
  puts the seed course in), least recently used out first. Lesson Markdown is rendered by pulldown-cmark; raw HTML in
  Markdown is escaped (shown as text) and links or images with a scheme
  other than http(s) or mailto point to `#` (`content::render_markdown`),
  so a hostile upload or an AI misled by pasted material cannot put script
  on a page. SVG pictures are only checked to start with `<svg` (or
  an XML prologue); they are served with `Content-Security-Policy:
  default-src 'none'` so a script inside one does not run even when the file
  is opened directly, and `nosniff`.
- API keys pasted into the course generator are used for that job only and
  never stored. The server's keys, which an owner saves on Manage Users
  (`src/secrets.rs`, #25), are sealed with ChaCha20-Poly1305 under a key
  derived (SHA-256, labelled) from the master secret, with the provider as
  associated data, so a row cannot be read from a copy of the database alone
  nor moved to another provider's row. Only owners see the keys form; every
  account's AI features use the keys. The secret comes from `SKRONGLE_SECRET_KEY` or `<db>.key`; losing it
  makes stored keys unreadable (the page says so and asks for them again;
  the server won't start on a database with saved keys and no key at all,
  or a key that opens none of them, until `SKRONGLE_NEW_SECRET_KEY=1` says the
  old one is gone),
  and anyone with both files, i.e. root on the box, can read them. The page
  never echoes a key back: only its last four characters. Back up the
  `.key` file with the database if you want saved keys to survive a restore.
- Logs name users by id, never by name or address; model errors are scrubbed of
  the key before they are logged or shown (`ai::redact_key`).
