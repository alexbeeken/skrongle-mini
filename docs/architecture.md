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

- Start-up: `main.rs` (env config, commands, `require_providers`), `caps.rs` (the fixed limits of a mini, `MAX_ACCOUNTS`, #29), `shape.rs` (Skrongle mini or the hosted site, `SKRONGLE_MINI`), `startup.rs` (start-up errors), `doctor.rs` (`skrongle doctor`), `server.rs` (listener limits).
- HTTP: `routes/mod.rs` (Router, embedded static files and docs, CourseView), `error.rs` (AppError to responses), `security.rs` (headers, CSRF, rate limits).
- Database: `db.rs` (connection, `Db::call`), `db/migrations.rs` (`MIGRATIONS`); schema in "SQLite schema" below; background jobs (the hourly housekeeping) in `jobs.rs`.
- App text: `i18n.rs` reads `i18n/<code>.yaml` (`docs/i18n.md`). Randomness: `random.rs` (OS bytes), `quiz.rs` has the small PRNG.

### Courses and content

- Course model and validation: `content.rs` (Manifest, Lesson, ContentStore), `content/paradigm.rs` (paradigm slots and sets, `who`), `content/quiz_type.rs` (the quiz question types); contract in `docs/course-format.md`. Placement test (`placement.json`, #1106): `placement.rs` (format, drawing, marking), routes in `routes/courses/settings.rs`, `templates/placement_test.html`, `static/placement.js`.
- Course registry: `courses.rs` (built-in, user-made, projects); pages in `routes/courses/` (`routes/courses/mod.rs` routes and shared views, `routes/courses/picker.rs` the list, `routes/courses/settings.rs` the settings dialog and opening, `routes/courses/stat.rs` the body of a card's stat modal (fetched by `static/course-stat.js`), `routes/courses/create.rs` new, copy and translate, `routes/courses/editor.rs` the editor page, `routes/courses/files.rs` files, upload and export, `routes/courses/lessons.rs` the lesson table, `routes/courses/generate.rs` AI generation, `routes/courses/revise.rs` Add content, Review AI changes, Undo), `static/json-problems.js` (JSON problems worded as the server words them), `static/lesson-editor.js`, `static/advice.js` (the yellow ! on lesson rows and its *Things to check* modal), `static/lesson-preview.js`, `static/schema-form.js`, `static/lesson-duplicate.js` (a lesson row's ⧉), `static/lesson-ai-edit.js` + `static/lesson-ai-edit.css` + `templates/lessons_ai_dialog.html` (a lesson row's ✦, Edit with AI, #1242), `static/ai-examples.js` + `static/ai-examples.css` (Use AI's example requests, `templates/ai_examples.html`), `templates/course_edit.html`.
- Editing helpers: `arrange.rs` (lesson table, Undo), `word_list.rs` + `static/word-list.js` (Paste a word list), `problems.rs` (refusal messages), `report.rs` (feature usage report), `lint.rs` (course lints, `lint/language.rs` the language guess), `review.rs` (Review AI changes; its why and check lines styled by `static/review-notes.css`).
- AI generation: `ai.rs` (LLM client, job runner), `ai/queue.rs` (the one slot and the lease of AI jobs, #874, #43), `ai/providers.rs` (the providers, #24: ids, base URLs, course and tutor models, wire formats, and the one OpenAI-compatible client for OpenAI, Google, DeepSeek, Mistral, xAI, OpenRouter and an Ollama-compatible server), `ai/prompt.rs` (the instructions and each kind of course's rules), `ai/budget.rs` (effort levels and the tokens a job reserves, #918), `ai/edit.rs` (Edit lessons, #1240), `ai/check.rs` (the `check` plan action and the why lines of a fact check, #1238), `ai/edit_lesson.rs` (Edit with AI on one row, #1242), `translate.rs` (copy and translate a course), `prices.rs` (the estimate's price table)).
- Sharing: `share.rs`, `routes/share.rs`.
- Course pickers in `static/courses.js`, `static/course-settings.js`.
- Test course: `test_course.rs` (tests only), `data/test-courses/`.

### Learning modes

- Lessons and home: `routes/pages.rs` (`/`, `/lessons`, `/history`, `/mistakes`), `templates/home.html`, `static/home.js`, `static/lessons.js`; a learner's note on a lesson: `notes.rs`, `static/lesson-note.js` (#336). Tapping a conversation line to hear it and see its meaning: `static/conversation.js` (#1264). Music notation in lesson text (#1176) and on quiz items (#1218): `notation.rs` (does a lesson have any; `music.note_names` and note options), `static/notation.js` draws ```` ```abc ```` blocks and quiz staves with abcjs from `static/vendor/` (third-party files, unchanged, with their licences); `static/music-sound.js` plays a quiz item's or sort item's `play` with Web Audio tones (#1219). The Fill the bar quiz type (#1230): `notation/fill_bar.rs` (gap and note lengths, validation, grading), `static/fill-bar.js` and `static/fill-bar.css` (the bar and its tiles in every mode). A Sprint spot on a staff (#1227): `sprint/spot.rs` (both kinds of spot: exercise, grading, validation), `notation::abc_notes` counts the notes, `static/wrong-note.js` takes the taps. The Spell the chord quiz type (#1228): `notation/chord_spell.rs` (validation, grading as a set, the tiles in the course's note names), `static/chord-spell.js` and `static/chord-spell.css` (the slots, tiles and chord staff in every mode). The Place the note quiz type (#1226): `notation/staff_note.rs` (validation, grading by pitch and octave, the clef and note names for the page), `static/staff-note.js` and `static/staff-note.css` (the staff it draws itself, in every mode). The keyboard quiz type (#1231): `notation/keyboard.rs` (ABC notes as pitches, validation, grading), `static/keyboard.js` and `static/keyboard.css` (the piano keyboard component, in every mode).
- Questions: `quiz.rs` (picking, checking), `routes/quiz.rs` (`/api/answer`, the one grading path), `mathcheck.rs` (maths answers), `clock.rs` and `static/clock.js` (Set the clock, #1340; lesson, stage and quiz pages load it and `measure.js` only when `has_clock()` / `has_measure()`), `static/quiz.js`, `static/lesson-check.js`; the algebra tiles question (#1355): `algebra_tiles.rs` (reading the pile, the two-sides check), `static/algebra-tiles.js` and `.css` (the board); Measure the angle (#1352): `measure.rs` (fields, grading, which modes skip it), `templates/measure.html`, `static/measure.js` draws the protractor with JSXGraph from `static/vendor/`.
- Passes and progress: `progress.rs` (the pass rule), `routes/progress.rs` (agent API), `mastery.rs` (scores), `mastery/cache.rs` (each learner's progress rows for their course, in memory, #38), `activity.rs` (daily goal, resume point), `earnings.rs` (XP, SKC, streak totals).
- Motivators: `motivators.rs`, `motivators/xp.rs`, `motivators/skc.rs`, `templates/motivators/`; the Skrongle Cash shop: `motivators/shop.rs`, `routes/shop.rs`, catalogue in `data/badges/`.
- Sprint: `sprint.rs` (fill-the-gap tiles in `sprint/gap.rs`), `routes/sprint.rs`, `static/sprint.js`, `static/sprint.css`. Build the scale (#1229), the ladder with notes: `sprint/scale.rs` (exercise, grading, validation); `static/scale-ladder.js` draws and plays each note as it lands, `static/sprint.js` runs the ladder.
- Study: `guide.rs` + `routes/guide.rs` (Guide), `tutor.rs` + `routes/tutor.rs` (AI tutor), `listening.rs` + `routes/listening.rs`, `vocab.rs` + `routes/vocab.rs`, `routes/map.rs` (`/map`); scripts `static/guide.js` (list styles `static/guide.css`), `static/tutor.js`, `static/listening.js`, `static/vocab.js`.
- Play: `games.rs` + `routes/games.rs` (hub, Word Match, numbers, dates), `conjugation.rs` (drill), `static/matching.js`, `static/conjugation.js`, `static/numbers.js`, `static/dates.js`.

### Accounts

- Sign-in: `auth.rs` (sessions, CurrentUser, Project), `auth/cache.rs` (the signed-in users kept in memory by session, #37), `routes/auth.rs`, `routes/auth/code.rs` (the two-factor step), `proxy_auth.rs` (proxy header), `signup.rs` (who may register), `urlenc.rs` (percent-encoding for the redirects they build).
- Mail: `mail.rs` (`mail/resend.rs`, `mail/postmark.rs`, `mail/brevo.rs`, `mail/sink.rs`), `verify.rs` (address check), `reset.rs` + `routes/reset.rs` (password reset).
- Two-factor: `two_factor.rs`, `totp.rs`.
- Profile: `profile.rs`, `routes/profile.rs`, `static/profile.js`, `static/autosave.js`.
- Data rights: `privacy.rs` (export columns, deletion), `consent.rs` (optional consents for the Privacy and consent card), `account_deletion.rs` and `routes/account_deletion.rs` (deleting an account after 14 days, #970).
- Owner: `owner.rs`, `routes/manage.rs`, `static/manage.js`, `routes/password.rs` (temporary password gate); the server's AI API keys in `secrets.rs` and the hide-AI switch in `settings.rs`, both on Manage Users (#25).
- Docs pages: `routes/docs.rs` (`/docs` sidebar `SECTIONS`, `/llms.txt`).

### Dev mode only

- `dev.rs` + `routes/dev.rs` (`/dev/login`, mailbox; `routes/dev/jobs_page.rs` is `/dev/jobs`), `dev_ai.rs` + `routes/dev_ai.rs` (`/dev/ai`, fixtures in `data/test-fixtures/`), `dev_crowd.rs` (made-up learners), `static/devbox.js`.

### Elsewhere

- `data/courses/` built-in courses, `docs/` these pages, `i18n/` app text, `e2e/` browser tests, `scripts/` tooling, `deploy/` systemd and nginx.

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
  mail.rs        e-mail: the outbox (table mail_outbox), the provider chain with fallback and cooldown, the sender the job runner drains (`mail::Outbox`), SMTP and log providers, SKRONGLE_MAIL_* config
  mail/resend.rs the `resend` kind: Resend's HTTP API (POST /emails, idempotency key per outbox row)
  mail/postmark.rs the `postmark` kind: Postmark's HTTP API (POST /email, Server API token, message stream, outbox id as metadata)
  mail/sink.rs   the dev mailbox: an SMTP server on 127.0.0.1 that keeps what it receives in memory, decoded (/dev/inbox)
  mail/brevo.rs  the `brevo` kind: Brevo's transactional HTTP API, response → Rejected/Unavailable, idempotency key per outbox row
  proxy_auth.rs  sign-in through a reverse proxy's identity header (SKRONGLE_PROXY_AUTH_HEADER / _FROM, #642): a middleware before every handler
  signup.rs      who may make an account (SKRONGLE_SIGNUP: open / invite / closed, #634): the first account is the owner, through the owner link written to the log at startup, invitation links (table signup_invites, the token hashed), the Manage Users page's Accounts part and invitation links (routes/manage.rs)

  account_deletion.rs  the learner's own account deletion after 14 days of grace (#970): users.delete_at, signing out, the dated mail, the housekeeping step that deletes due accounts
  shape.rs       the two shapes (#578): SKRONGLE_MINI=1 is Skrongle mini, unset is the hosted site; read once at startup (shape::current, crate::is_mini); in a mini crate::env reads the hosted-only settings (HOSTED_ONLY: mail, SMS, Google, S3) as unset
  share.rs       sharing a course with other learners by e-mail address (#451): invitations (table course_shares, the link's token hashed), who may open a shared course, the message
  verify.rs      e-mail verification: link tokens (table email_verifications, SHA-256 only), confirm, the message, the link's host (SKRONGLE_PUBLIC_URL or the request)
  reset.rs       password reset links (table password_resets, SHA-256 only, 1 h, single use), the "you already have an account" message, which unconfirmed accounts a registration replaces (SKRONGLE_UNCONFIRMED_DAYS)
  totp.rs        time-based one-time passwords: RFC 6238 TOTP over RFC 4226 HOTP (SHA-1 written out, SHA-256/512 via sha2), base32, the otpauth://totp/ Key Uri Format and its QR code as inline SVG (the qrcode crate, no image feature); the dev authenticator (/dev/authenticator) reads its keys with it
  two_factor.rs  two-factor sign-in: the method in use, the authenticator app's enrolment, sign-in challenges, the proof for turning it off (tables totp_keys, two_factor_challenges, two_factor_confirmations)
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
  motivators.rs  motivators (#559): how progress is dressed up (projects.motivator, per course since #574); each draws the course home's top from the shared measures; registry ALL, default "stats"; motivators/xp.rs: XP and the character level (#561); motivators/skc.rs: Skrongle Cash (#562); motivators/shop.rs: the Skrongle Cash shop (#569), badges per course in motivator_purchases, catalogue data/badges/
  placement.rs   the placement test (#1106): placement.json parsed and checked against the manifest, one sitting drawn (written questions, else random_per_unit of a unit's multiple-choice quiz items), the answers marked (each unit's tally, the first unit with a miss as the start)
  progress.rs    lesson passes shared by every mode: the rule (seen + a run of >= 70 %), the RULES text for tutors and agents, table lesson_passes
  quiz.rs        adaptive question selection, answer normalisation/checking, tiny PRNG
  problems.rs    what the editor and the file page say when a course file is refused: kind, lesson, field, line, message, fix hint (every ContentError and arrange.rs refusal)
  owner.rs       what the owner does to an account with no message sent (#635): a temporary password (users.must_change_password), two-factor off, make an owner; the binary's commands (reset-password, disable-2fa, make-owner, help)
  privacy.rs     a learner's data as a whole: USER_TABLES, account deletion, retention rules, the age confirmation and SKRONGLE_MIN_AGE (docs/data-inventory.md)
  jobs.rs        background jobs (table jobs, #872): enqueue, the one-statement claim with a lease, retries with per-kind delays, periodic kinds that queue their next run; the `jobs` runner thread, which also drains the mail and text outboxes with a lease (#873); the hourly housekeeping as one job with a step per purge
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
  dev_crowd.rs   a crowd of made-up learners with weeks of backdated history (SKRONGLE_DEV_SEED_USERS, #583), written through the modes' own progress functions; deterministic per learner
  dev.rs         local testing (SKRONGLE_DEV): the gate, the dev user (seed with the simulator's keys, reset), SKRONGLE_LLM_MODE and the dev AI provider's default base URLs, the fixture directory and its in-memory edits, held requests on disk (Manual replies on /dev/ai)
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
    share.rs     POST /courses/{id}/share (invite addresses) and /courses/{id}/share/remove (owner only); GET /courses/shared/{token} (the invitation's link: signed in, the course joins the list and opens; signed out, /login?share=<token>)
    docs.rs      /docs (sidebar sections: SECTIONS), /llms.txt (docs/llms.txt)
    reset.rs     GET/POST /reset (ask for a link: the same answer for any address; ?token=: choose a new password, which ends every session; 404 without mail)
    auth.rs      /login /register (with mail: always "check your mail", signs nobody in) /logout, GET /verify-email?token= (confirms the address; signed in or not)
    auth/code.rs /login/code… (the two-factor step: code form, resend, cancel, status, dev device)
    account_deletion.rs  /account/deletion (the only page of an account scheduled for deletion: its date, Cancel deletion, Log out)
    password.rs  /password/change (the gate after signing in with a temporary password: the new one twice, no old one)
    manage.rs    /manage, the Manage Users page (owners only, 404 otherwise; /superuser redirects here): every account on one page with Temporary password, Turn off two-factor, Make owner and Reset email verification per row; invitation links under SKRONGLE_SIGNUP=invite. and the AI section (#25): the server's AI API keys (autosaved, POST /api/manage/keys) and Hide AI features for everyone. POST /manage/make-owner|reset-email-verification|invites|invites/revoke|temp-password|two-factor-off|terms-announce|api-keys/remove|ai
    profile.rs   /profile (personal information, account), POST /api/profile (autosave), POST /profile/password|verify-email/resend, POST /profile/delete
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
    dev.rs       /dev/login, /dev/reset, /dev/mail, /dev/jobs, /dev/inbox, /dev/authenticator, /dev/boot (the --watch reload poll); all mounted only in dev mode and for local clients, see testing.md; the dev mailbox needs no sign-in
    dev_ai.rs    /dev/ai: the dev AI provider's wire API (Anthropic Messages, OpenAI chat completions, Gemini's OpenAI endpoint; checked by API key; a held request waits for its reply file) and its page in the dev mailbox (log, limits, failures, Manual replies with the held requests and their reply forms, fixture editor)
templates/       askama templates (compiled in)
static/          CSS + dependency-free JS (compiled in); vendor/ holds the one third-party script, abcjs
docs/            this documentation (compiled in, served at /docs)
data/courses/    built-in courses: <id>/course.json + lessons/**.json + pictures/**.svg
data/test-courses/  courses for automated tests, loaded only with TEST_COURSE=1 (see testing.md, "The test course")
data/test-fixtures/llm/  canned model replies the dev AI provider answers from, one <purpose>.json per call site
deploy/          systemd unit + nginx snippets
scripts/         release.sh (one-shot), build-pi.sh (cross-compile), deploy.sh, dev-progress.sh, smoke.sh (local smoke test)
```

## Request flow

Every handler is `async`; all SQLite and argon2 work goes to Tokio's blocking
pool (`Db::call`) so the one async worker never stalls. Course generation runs
on its own OS thread (it is minutes of HTTP waiting) and writes through
`Db::with`.

**Background jobs** (`src/jobs.rs`, #872). One `jobs` table and one OS
thread (`jobs`, not the Tokio blocking pool) that runs due rows. Taking a
row is one `UPDATE … RETURNING` that sets `state = 'running'` and a lease
(`locked_by`, `locked_until`, 10 minutes); a running row whose lease ran
out is taken again, so a job its server stopped in the middle of is not
lost. A failure goes back to `queued` after its kind's delay (the mail
outbox's `RETRY_DELAYS` unless the kind says otherwise) until
`max_attempts`, then `failed` with `last_error`; each failure is a log
line, and so is a job of an unknown kind, which is refused. A periodic kind
queues its next run when a run ends. The thread wakes on `AppState::queue`
or when the next row is due, at least once a minute, and reaches SQLite
through `Db::with`. The first user is the hourly housekeeping, one kind
(`housekeeping`, #45) that runs every purge as a step in turn (sessions, the
game, AI job and tutor maps, rate limits, account deletions, retention, outbox,
links, old job rows, then the WAL checkpoint); each step logs its
count, a failing step is logged and the next still runs. At
boot it gets a row due now if it has none, so a server restarted more
often than hourly still cleans up. The same thread sends the mail
outbox (#873): it keeps its own table (`mail_outbox`)
but takes a row with the same kind of lease (`jobs::lease_outbox` sets
`locked_by` and `locked_until`; the row stays `queued`), so a row being sent
is never handed out twice and one whose server stopped mid-send is sent
again once its lease is over. Each pass sends up to 10 rows from the outbox
before the next job, the outbox first, so a sign-in code never waits behind
housekeeping. AI generation still has its own thread.
`/dev/jobs` lists the job rows in dev mode; `/dev/mail` shows
a leased row as `sending`.

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

**What the server can send.** `AppState::caps()` is a `Caps { mail }`
(`Mailer::can_send`, fixed at boot): false when the
chain is the `log` provider alone; dev mode's mailbox
counts as a provider. Features ask it, not the mode, and hide what would send
a message (#633); templates that need it carry a copy (`caps`, or a `mail`
flag). `docs/self-hosting.md`, "Without e-mail", lists
what changes.


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
users          (id AUTOINCREMENT, password_hash, created_at, google_sub UNIQUE, email,
                active_course, first_name, last_name, avatar, avatar_color, age_confirmed, developer, ai_off,
                email_verified_at, superuser, phone, phone_verified_at, two_factor, lang, billing_ref UNIQUE, stripe_customer,
                must_change_password, suspended_at, beta, owner)
                -- id is never reused after a deletion (AUTOINCREMENT since v45, #364);
                -- billing_ref: random per account, what Stripe's subscription metadata names (never the id);
                -- stripe_customer: the one Stripe customer every Checkout of the account shares (#394);
                -- google_sub is no longer read (the Google sign-in is gone; the column waits for the schema squash);
                -- password_hash is '' for accounts without one (proxy sign-in); email is what a password account
                -- signs in with (no usernames since v26), set once and never changed;
                -- email_verified_at: when the address was confirmed (link), NULL = not yet;
                -- avatar, avatar_color: the profile picture and its background, no longer read (the pictures are gone; the columns wait for the schema squash);
                -- lang the language the app speaks to this account in (i18n/<code>.yaml, v42), '' = the browser's;
                -- developer and superuser: the two old flags, merged into owner by v98 (#16); nothing reads them, they wait for the schema squash
                -- ai_off: the old per-account "Hide all AI features" (#147); since v99 (#25) the switch is the
                -- server's (settings 'ai_off'), nothing reads it, it waits for the schema squash
                -- owner 1 opens /manage and links it from the account menu (by hand, make-owner, or the first account under SKRONGLE_SIGNUP=invite|closed; the dev user in dev mode)
                -- phone: E.164 or ''; phone_verified_at: when a texted code confirmed it (v39);
                -- phone_opted_out_at: a provider refused a text to it as opted out (STOP, v69, #969);
                -- cleared when a text to it goes through or the number changes; codes go by e-mail meanwhile
                -- two_factor: the second sign-in step, '' (off) | 'email' | 'totp' (src/two_factor.rs)
                -- must_change_password 1: signed in with a temporary password the owner set (src/owner.rs, v62);
                -- CurrentUser sends the account to /password/change until it picks its own
                -- suspended_at: unused (moderation is gone, #9)
                -- beta 1: made through an e-mailed waitlist invitation (v86, #934); nothing reads it
                -- delete_at: the learner asked to delete it; deleted then by housekeeping.deletions (src/account_deletion.rs, v88, #970);
                -- CurrentUser sends it to /account/deletion until then, its courses are not shared out
sessions       (id, user_id, session_token UNIQUE, created_at, expires_at)
                -- session_token is the SHA-256 (hex) of the cookie's token (auth::hash_token), never the token
courses        (id, owner_id, title, created_at, updated_at, error, state, source, visibility, job_id, brief, outline, notes, taken_down_at)
                -- user-made courses; state 'generating' | 'revising' | 'ready' | 'failed', source 'hand' | 'ai',
                -- visibility 'private' (publishing to come); brief/notes/outline let a stopped AI run resume
                -- taken_down_at: unused (moderation is gone, #9)
course_files   (course_id, path, content, updated_at)  PRIMARY KEY (course_id, path)
course_undo    (course_id, seq, title, outline, label, created_at, session, started_at)  PRIMARY KEY (course_id, seq)
course_undo_files (course_id, seq, path, content)  PRIMARY KEY (course_id, seq, path)
                -- the files from before each change to a course as a whole (feedback, new order,
                -- lesson removed or added), for Undo: seq > 0, newest (highest) first, ten per
                -- course at most. Redo steps share the table with seq < 0 (newest = lowest): an
                -- Undo files the course as it was there; any new change clears them;
                -- `session`: the editor page whose saves join this step (for ten minutes from
                -- `started_at`; `created_at` is then its latest save)
course_ai_runs (course_id PRIMARY KEY, label, created_at, excluded, edits)
course_ai_run_files (course_id, side, path, content)  PRIMARY KEY (course_id, side, path)
                -- the latest AI run's changed files, side 'before' | 'after' (missing: added or
                -- removed), for "Review AI changes" (src/review.rs); the next run replaces it.
                -- `excluded`: what the review left out (JSON `review::Exclusions`, '' for nothing)
                -- `edits`: the review's own text for lines the AI wrote (JSON `review::Edits`:
                -- path → row key → text, '' for none; v41, #287)
course_shares  (id, course_id, email, token_hash UNIQUE, user_id, created_at, opened_at)  UNIQUE (course_id, email)
                -- a course shared with an address (src/share.rs, v43): token_hash is the SHA-256 of the
                -- link's token; user_id the account that opened the link (NULL until then, or when
                -- that account is deleted; the share stays and can be opened again). The course is
                -- open to that account and to any account whose confirmed address is `email`
course_flags   (course_id PRIMARY KEY, state, source, matches_json, reason, content_hash, checked_at,
                override_note, overridden_by, overridden_at)
                -- unused: the content check and moderation are gone (#9); the table goes with the schema squash (#2)
signup_invites (id, token_hash UNIQUE, created_by, created_at, expires_at, used_at, used_by, email)
                -- invitation links for SKRONGLE_SIGNUP=invite (src/signup.rs, v61): token_hash is the SHA-256
                -- of the link's token; one use (used_at, used_by), 7 days; created_by cascades,
                -- used_by is set NULL when the invited account is deleted; gone 30 days after expiry.
                -- email (v86, #934): from the removed waitlist; nothing reads it
waitlist       (id, email UNIQUE, joined_at, confirm_hash, confirmed_at, invite_id, invited_at, invites_sent, signed_up_at, user_id)
                -- the removed waitlist (v86, #934; nothing reads it): email lower case; confirm_hash the SHA-256 of the
                -- confirmation link's token until it is used; invite_id the latest e-mailed invitation (set NULL
                -- when purged); user_id the account it signed up as (cascades); unconfirmed rows go after 30 days
release_settings (id = 1, waitlist)  -- the removed waitlist switch (#934; nothing reads it)
projects       (user_id, course_id, created_at, last_opened_at, start_unit, avatar, avatar_color, motivator, set_up)  PRIMARY KEY (user_id, course_id)
               -- motivator: the course's setting (#574), starting as the account's; avatar, avatar_color: no longer read;
               -- set_up: 0 until the first visit's settings wizard is left (#574)
user_progress  (user_id, course_id, lesson_id, mastery_score, times_tested, times_correct, streak,
                last_practiced_at, read_at)  PRIMARY KEY (user_id, course_id, lesson_id)
                -- read_at: when the lesson was first read or taught (reading check, or seen in any mode); NULL = unread
quiz_history   (id, user_id, course_id, lesson_id, question_id, selected_answer, was_correct, answered_at)
journey_progress (user_id, course_id, lesson_id, step, completed, best_stars, xp, run_correct, run_answered,
                updated_at)  PRIMARY KEY (user_id, course_id, lesson_id)      -- the Guide; xp unused since #558
user_vocab     (user_id, course_id, lesson_id, word, learned_at, times_seen, times_known, last_reviewed_at, due_at, gap_days)
game_scores    (id, user_id, course_id, game, board, score_ms, wrong, played_at)
game_activity  (id, user_id, course_id, game, board, label, duration_ms, total, correct, score_ms, played_at, xp)
                -- xp unused since #558
activity_log   (id, user_id, course_id, kind, n, at)     -- n of activity::Kind `kind` done at `at`: the daily goal
lesson_touches (user_id, course_id, lesson_id, mode, at)  PRIMARY KEY (user_id, course_id, lesson_id)
                -- when and in which mode a lesson was last opened or worked on: where every mode resumes
lesson_notes   (user_id, course_id, lesson_id, body, updated_at)  PRIMARY KEY (user_id, course_id, lesson_id)
                -- the learner's own note on a lesson ("My note", #336, v93); an emptied note has no row
earnings       (user_id, course_id, points, goals, day, day_points, streak)  PRIMARY KEY (user_id, course_id)
                -- running totals (#589): effort points ever logged, daily goals reached, the local day last
                -- active, its points and the streak ending on it; XP, SKC and the streak read this, not the log
game_rounds    (user_id, course_id, game, rounds)  PRIMARY KEY (user_id, course_id, game)
                -- rounds finished per game (v64), moved by games::record_activity_at: "rounds played" reads this
lesson_passes  (user_id, course_id, lesson_id, seen_at, seen_via, best_pct, best_via, passed_at, passed_via, runs,
                evidence, updated_at)  PRIMARY KEY (user_id, course_id, lesson_id)
                -- via is 'k' | 'd' | 'g' (Study, Sprint, Play: the modes' old letters, progress::VIA_*)
                -- | 'tutor' | 'agent' | 'legacy'; best_pct -1 = no counted run yet
user_api_keys  (user_id, provider, ciphertext, updated_at)  PRIMARY KEY (user_id, provider)
                -- learners' own keys from before #25: v99 copied the first owner's into server_api_keys;
                -- nothing reads or writes it now, kept until the schema squash drops it
server_api_keys (provider PRIMARY KEY, ciphertext, set_at, sealed_for)  WITHOUT ROWID
                -- the server's AI API keys (v99, #25), the owner's, set on /manage; provider is
                -- 'anthropic' | 'openai' | 'google'; ciphertext is sealed by src/secrets.rs; sealed_for = the
                -- user id its associated data names (a key v99 brought over from that owner's profile), NULL
                -- for a key saved for the server
settings       (name PRIMARY KEY, value)  WITHOUT ROWID
                -- server-wide settings (src/settings.rs, v99, #25): 'ai_off' = '1' hides every AI feature for
                -- every account (no row: shown). The session query reads it, so auth/cache.rs watches it
subscriptions  (user_id PRIMARY KEY, customer_id UNIQUE, subscription_id, status, price_id, current_period_end,
                cancel_at_period_end, event_at, updated_at, plan)
                -- the learner's Stripe subscription as the last webhook described it; status is Stripe's;
                -- plan is what it pays for ('basic' | 'plus', from its price, never metadata; v55, #314, #963)
plan_grants    (user_id PRIMARY KEY, granted_at, expires_at, note)   -- Plus without paying; expires_at NULL = no end
plan_cancellations (id, user_id, plan, reason, feedback, created_at)   -- why a learner cancelled (billing::CANCEL_REASONS)
terms_acceptances (user_id, version, accepted_at)  UNIQUE (user_id, version)
terms_announcements (user_id, version, mailed_at)  PRIMARY KEY (user_id, version)
                -- both tables are no longer written or read (the Terms of Service were removed, #20); they stay until the schema squash
consents       (id, user_id, purpose, granted_at, withdrawn_at)
                -- an optional consent (src/consent.rs, v82, #974): the 'ai' rows went in v99 when the AI
                -- features became the server's to show or hide (#25); nothing writes it now
mail_outbox    (id, user_id, to_addr, subject, body_text, body_html, purpose, state, attempts, next_attempt_at,
                scheduled_at, created_at, sent_at, provider, last_error, locked_by, locked_until)
                -- e-mail to send (src/mail.rs); state 'queued' | 'sent' | 'failed'; provider = the one that took it;
                -- locked_by/locked_until (v84, #873): the job runner's lease while it sends a queued row
                -- user_id NULL: about no account. Sent bodies emptied (not in dev mode); finished rows kept 30 days.
                -- scheduled_at (v58, #579): when it was first due, i.e. the hour whose SKRONGLE_MAIL_PER_HOUR budget it uses
sms_outbox     (id, user_id, to_number, body_text, purpose, state, attempts, next_attempt_at, created_at, sent_at,
                provider, last_error, locked_by, locked_until)
                -- text messages to send (src/sms.rs); to_number is E.164; otherwise as mail_outbox
sms_sends      (id, user_id, number_key, kind, created_at)
                -- the send limits' ledger (v42, #362): one row per text queued; number_key = SHA-256 of the
                -- number, kind 'signin' | 'other'; user_id ON DELETE SET NULL (the row outlives the account);
                -- purged after two days
jobs           (id, kind, payload_json, state, priority, run_at, attempts, max_attempts, locked_by, locked_until,
                last_error, created_at, finished_at)
                -- background jobs (src/jobs.rs, v72, #872); state 'queued' | 'running' | 'done' | 'failed';
                -- locked_by/locked_until: the lease of a running row. Done rows deleted after a day, failed after 30
content_scores, content_category_scores, content_checks, content_check_worker: unused tables of the content check
                (#9); they go with the schema squash (#2)
email_verifications (id, user_id, email, token_hash UNIQUE, created_at, expires_at, used_at)
                -- e-mail confirmation links (src/verify.rs): only the SHA-256 of the token; 24 h, single use;
                -- users.email_verified_at is set when one is opened. Rows deleted after 7 days
password_resets (id, user_id, email, token_hash UNIQUE, created_at, expires_at, used_at)
                -- password reset links (src/reset.rs, v66): only the SHA-256 of the token; 1 h, single use;
                -- using one sets the password, confirms the address, deletes the sessions. Rows deleted after 7 days
totp_keys      (user_id PRIMARY KEY, secret, created_at, confirmed_at, last_step)
                -- the account's authenticator app (v39): the secret sealed like user_api_keys (ChaCha20-Poly1305,
                -- secrets::seal_totp, bound to the user; codes are worked out from it, so it can't be hashed),
                -- confirmed_at NULL while enrolling (purged after a day),
                -- last_step the newest RFC 6238 time step accepted: a code works once
phone_verifications (id, user_id, phone, code_hash, salt, attempts, created_at, expires_at)
                -- the texted code confirming users.phone: SHA-256(salt:code) only; 10 minutes, 5 tries
two_factor_challenges (id, user_id, token_hash UNIQUE, method, code_hash, salt, attempts, created_at, expires_at, sent_by)
                -- a sign-in waiting for its second step: token_hash is the SHA-256 of the `twofa` cookie's token;
                -- code_hash SHA-256(salt:token:code) for texted/e-mailed codes ('' for an authenticator); 10 minutes, 5 tries;
                -- sent_by 'email' when a texted code went by e-mail because the number opted out (v69)
two_factor_confirmations (user_id PRIMARY KEY, method, code_hash, salt, attempts, created_at, expires_at)
                -- the code of an account without a password for turning its texted/e-mailed second step off or switching it:
                -- SHA-256(salt:code), only for the method it was sent for; 10 minutes, 5 tries
ai_usage       (id, user_id, course_id, job_id, purpose, provider, model, input_tokens, output_tokens, cached_tokens,
                cost_micros, credit_micros, key_source, created_at)
                -- one row per model call; key_source 'hosted' (the server's key, costed) | 'own' (cost 0);
                -- credit_micros is the part of the cost paid from credits, the rest came from the monthly allowance;
                -- purpose 'content-review' (#1037) is the server's own call about the owner's course: no credits, and
                -- the triggers leave it out of ai_usage_months (migration 76), so only the day's spend counts it
ai_usage_months (user_id, month, cost_micros, credit_micros, hosted_calls, own_calls)  PRIMARY KEY (user_id, month)
                -- running totals of ai_usage per learner and UTC calendar month (month = its first second); v65, #624
ai_spend_days  (day PRIMARY KEY, cost_micros)
                -- everyone's hosted spend per UTC day (day = its first second), for the daily cap; v65, #623.
                -- Triggers on ai_usage (insert, delete) move both tables; the meter reads one row of each
blocked_ips    (id, cidr UNIQUE, created_at)
                -- unused: the content check and moderation are gone (#9); the table goes with the schema squash (#2)
privacy_actions (id, superuser_id, action, reason, created_at)
                -- the privacy log on /manage (v85, #981): export | forget | delete-account | sign-out | notice with the reason typed; no learner named
breach_notices (user_id, notice, mailed_at)  PRIMARY KEY (user_id, notice)
                -- who had which breach notice (v91, #1068); notice = SHA-256 of subject and body, so Send again mails nobody twice
ai_credit_purchases (session_id PRIMARY KEY, user_id, payment_intent, price_id, amount_cents, credit_micros,
                refunded_micros, created_at)     -- Stripe payments for credit packs: applied once, refunds take back their share
users.ai_credits                                 -- the prepaid credit balance, micro-dollars
users.signup_credit_at                           -- when a new account got its one-time Skrongle AI credit (v87, #931); 0 for accounts made before
users.ai_effort                                  -- the effort Generate the course last ran at: low | medium | high (Fast, Balanced, Thorough; v92, #918)
data_exports    (id, user_id, state, token_hash, file, link_base, bytes, error, created_at, ready_at, expires_at, downloaded_at)
                -- unused: the data export was removed (#17); the table stays until the schema squash
```

Migrations live in `src/db/migrations.rs` (`MIGRATIONS`), tracked with `PRAGMA
user_version`. Migration 8 added the course scoping: existing rows were
assigned to the `spanish` course. Migration 9 added the profile columns;
accounts from before it have no e-mail address until they set one on the
profile page. Migration 10 added `user_api_keys`. Migration 13 added `lesson_passes` and carried over earlier
Guide and Sprint mode completions (seen; passed with two stars or more) and read
lessons (seen). Migration 14 gave user-made courses their lifecycle columns
(state, source, visibility, the generation job, brief, outline and notes).
Migration 15 added `subscriptions` and `plan_grants`. Migration 22 added
`users.age_confirmed` (the "I am at least N" box at
sign-up, set by registration).
Migration 34 stores session tokens hashed: it ends every existing session
(everyone signs in once more) and marks accounts that accepted the Terms as
age-confirmed. Migration 23 added `terms_acceptances` (no longer used, #20). Migration 24 added `users.developer`
and `users.mock_ai` (both 0 for every existing account; migration 40 drops
`mock_ai` again, #298). Migration 25 added
`mail_outbox`. Migration 27 added `users.ai_off` ("Hide all AI features",
0 for every existing account; setting it also deletes the account's
`user_api_keys` rows). Migration 29 added `users.email_verified_at` and
`email_verifications`; existing accounts start unconfirmed. Migration 30 added
`users.superuser` (0 for every existing account). Migration 32 added `course_undo.session` and
`started_at` (the editor's autosave, #151/#190; older steps have neither
and are never added to). Migration 35 added `sms_outbox`. Migration 39 added two-factor sign-in:
`users.phone`, `phone_verified_at` and `two_factor` (off for every existing
account), `totp_keys`, `phone_verifications`, `two_factor_challenges` and `two_factor_confirmations`. Migration 43 added
`course_shares` (#451), migration 44 `data_exports` (#430). Migration 99 (#25) added `server_api_keys`, seeded with the first owner's
`user_api_keys` rows (which stay, unread), and `settings`; the `'ai'` consent rows went.
Migration 45 rebuilt `users` with `AUTOINCREMENT` (ids are never handed out
again, #364) and added `billing_ref` (a random value for every account) and
`stripe_customer` (filled from the account's `subscriptions` row). Migration 46 added the `sms_sends` ledger and a unique index on verified
phone numbers (`users.phone WHERE phone_verified_at IS NOT NULL`), undoing
the later verification where two accounts had verified one number (#362, #363). Migration 47 added `ai_usage`,
`ai_credit_purchases` and `users.ai_credits` (Skrongle AI metering). Migration 48 added the plans bought on the profile:
`plan_subscriptions`, `plan_cancellations` and `users.own_keys_fallback`. Migration 49 added `subscriptions.plan`
(which plan a Stripe subscription pays for). Migration 83 dropped `plan_subscriptions` with the pretend card payment (#963). Migration 59 added the indexes the
former data export pages and housekeeping read through (#625, #626). Migration 60 (#614–#620) added the indexes the
housekeeping sweeps and deletions read through: `quiz_history(answered_at)`, `game_activity(played_at)`,
`courses(state, updated_at)`, `courses(state) WHERE notes <> ''`, `projects(course_id)`,
`users(active_course)` (partial, set values only);
`db::tests::every_foreign_key_is_indexed` keeps every foreign key indexed, so deleting an account never
reads a whole table. Migration 61 (v61 in
`src/db.rs`) added `signup_invites` (#634); migration 62
`users.must_change_password` (#635, 0 for every existing account); migration 63 the
`lower(email)` index on `users` (#604, #605, #606, #622); migration 64 the game score and round indexes and `game_rounds` (#592–#601). What each table holds, and how long, is in the
[data inventory](data-inventory); a table with user data must be listed in
`privacy::USER_TABLES` (a test checks every table in the schema).

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

Environment variables, all optional on a mini (a hosted server needs the providers, below). Every name starts `SKRONGLE_`. For one release `crate::env` also reads the
older `LEARN_<name>` and `SPANISH_<name>` (#584): the `SKRONGLE_` one wins when both are set, and
startup logs one warning naming each old variable and its new name.

**Skrongle mini** (#632, #641, #578) is the self-hosted release: the same
binary with `SKRONGLE_MINI=1`, the one setting that makes a server a mini,
plus the defaults `deploy/skrongle.service` sets, marked *mini* in the table
below: `SKRONGLE_SIGNUP=invite`,
and no mail provider (#633). `shape.rs` reads `SKRONGLE_MINI` once
at startup (`Shape::Mini` or `Shape::Hosted`, `shape::current()`,
`crate::is_mini()`); nothing else reads the variable (a test checks).
A mini has none of the hosted site's production features and no second
switch for any of them: `crate::env` reads the settings in `shape::HOSTED_ONLY` (the mail
provider) as unset, so those
features are off wherever they are read; the startup log names any that
are set and ignored. Dev mode's stand-in for it (the dev mailbox) is off
in a mini too. `SKRONGLE_PLANS` is
gone; a server that still sets it logs a warning. The startup
line and `/healthz` (`ok skrongle mini <version>`) say which shape runs.
Self-hosting the full build is not supported: outside mini and dev mode the
server refuses to start until e-mail (`SKRONGLE_MAIL_PROVIDERS`) is set
(`require_providers` in `main.rs`).
It also refuses to start without `SKRONGLE_SECURE_COOKIES=1` (the hosted site
is served over HTTPS) and with `SKRONGLE_PROXY_AUTH_*` set (proxy sign-in is
a mini feature; #980). The dev server's mailbox stands in for it. The
release tarballs are `skrongle-<version>-<arch>.tar.gz`
(`routes::docs::RELEASE_TARBALL`, `scripts/package-release.sh`); The pitch and the security
posture are in `docs/self-hosting.md`, "Skrongle mini".

| Variable | Default | Meaning |
|---|---|---|
| `SKRONGLE_BIND` | `0.0.0.0:8080` | Listen address; a bare port (`9000`) means every interface. A value that doesn't parse stops the server |
| `SKRONGLE_SIGNUP` | `open` (*mini:* `invite`) | Who may make an account (`src/signup.rs`, #634): `open` (anyone), `invite` (only through a link an owner makes on `/manage`) or `closed`. Under `invite` and `closed` the first account on a server with none is made only through the owner link the server logs at startup (a new one each start), and is made an owner. Read as is, with no older spelling; any other value stops startup. `deploy/skrongle.service` sets `invite`, `deploy/aws` nothing |
| `SKRONGLE_PROXY_AUTH_HEADER`, `SKRONGLE_PROXY_AUTH_FROM` | unset | Sign-in by a reverse proxy (`src/proxy_auth.rs`, #642): the header holding the person's e-mail address, and the proxy's addresses (IPs or CIDRs, comma-separated) it is trusted from, judged by the TCP peer, never `X-Forwarded-For`. Both or neither (one alone stops startup). A trusted request with the header is signed in without password, two-factor or rate limit; an unknown address follows `SKRONGLE_SIGNUP` (`open` makes a password-less account with its address confirmed). `/login` and `/register` only say the proxy signs in. Read as is, no older spelling. Skrongle mini only: the hosted server refuses to start with either set |
| `SKRONGLE_DATA_DIR` | `data` | Directory containing `courses/` |
| `SKRONGLE_DB` | `learn.db` | SQLite file (WAL + SHM files are created next to it) |
| `SKRONGLE_COURSES_PER_USER` | `50` | Courses one account may make (no limit in dev mode unless set) |
| `SKRONGLE_COURSE_BYTES` | `8388608` | Total size of one user-made course's files |
| `SKRONGLE_COURSE_BYTES_PER_USER` | `33554432` | Total size of the files of one account's courses |
| `SKRONGLE_RETAIN_HISTORY_DAYS` | `730` | Delete answers in `quiz_history`, finished rounds in `game_activity` and the daily goal's `activity_log` older than this many days (at startup and hourly). `0`: keep them while the account exists. A placeholder until the retention periods are decided (TODO.md) |
| `SKRONGLE_KEEP_NOTES` | `0` | `1`: keep the notes of an AI course once it is ready (revisions then see them). Default: emptied within the hour |
| `SKRONGLE_FAILED_COURSE_RETENTION_DAYS` | `30` | Delete an AI course whose generation failed (with its notes and files) once it has gone this many days without a resume or edit (at startup and hourly, `privacy::apply_retention`). `0`: keep them |
| `SKRONGLE_MIN_AGE` | `14` | The age the sign-up forms' age checkbox names (#976) |
| `SKRONGLE_LOG` | `info` | One level: `error`, `warn`, `info`, `debug` or `trace`. Per-module filters like `skrongle=debug` are not supported; an unrecognised value logs one warning and runs at `info`. |
| `SKRONGLE_SECURE_COOKIES` | `0` | Set `1` behind a TLS proxy to add `Secure` to cookies. Required outside mini and dev mode: the hosted server refuses to start without it |
| `SKRONGLE_BASE_PATH` | *(empty)* | URL prefix when mounted behind a reverse proxy, e.g. `/spanish` |
| `SKRONGLE_PUBLIC_URL` | — | Public URL, for the links in e-mail (unset: links use the request's host, `X-Forwarded-Host` behind a proxy) |
| `SKRONGLE_MINI` | unset (*mini:* `1`) | `1`: the server is a Skrongle mini (`shape.rs`); unset: the hosted site. Anything but `1`, `true`, `0`, `false` or unset refuses to start. Read once at startup, directly (no older spelling) |
| `SKRONGLE_AI_PRICES` | see `prices::DEFAULT_PRICES` | `model=in/out[/cached],…` in dollars per million tokens, added to (or replacing) the built-in table of Anthropic's list prices; `*` prices unknown models (default: the dearest rate) |
| `SKRONGLE_SUPPORT_EMAIL` | — | The help address, the `Reply-To` of every e-mail, so a reply to the no-reply sender reaches support. Unset: mail has no `Reply-To` |
| `SKRONGLE_MAIL_FROM` | — | Sender of e-mail, e.g. `Skrongle <noreply@example.org>`; needed with `SKRONGLE_MAIL_PROVIDERS` |
| `SKRONGLE_MAIL_PER_HOUR` | `3000` (none in dev mode) | E-mails of one purpose (`email-verify`, `login-code`, `course-share`, …) due per window; each purpose has its own budget. Past it a message is queued for the next window with room (`mail_outbox.scheduled_at`), at most `mail::MAX_WAIT_WINDOWS` (12) ahead; `mail::NO_WAIT` purposes (sign-in codes) are not queued. `0`: no ceiling. |
| `SKRONGLE_MAIL_WINDOW` | `3600` | The ceiling's window in seconds; windows start at multiples of it (clock hours). A short one (`60`) lets a test watch a capped message wait and leave |
| `SKRONGLE_MAIL_PROVIDERS` | — | Provider names in fallback order, e.g. `ses,backup`; each is configured by `SKRONGLE_MAIL_<NAME>_*` (`_KIND`, and for SMTP `_HOST`, `_PORT`, `_TLS`, `_USER`, `_PASS`). Unset: e-mail is logged, not sent (in dev mode it goes to the dev mailbox), and the features that would send it switch off (`AppState::caps`, #633); Skrongle mini ignores it (`docs/self-hosting.md`, "Without e-mail or text messages") |
| `SKRONGLE_MAIL_<NAME>_API_KEY` / `_API_URL` | — / `https://api.resend.com` | For `_KIND=resend` (Resend's HTTP API): the API key (required) and where calls go (a mock in tests) |
| `SKRONGLE_MAIL_<NAME>_API_KEY` / `_API_URL` / `_STREAM` | — / `https://api.postmarkapp.com` / `outbound` | For `_KIND=postmark` (Postmark's HTTP API): the Server API token (required), where calls go (a mock in tests) and the message stream |
| `SKRONGLE_MAIL_<NAME>_API_KEY` / `_API_URL` | — / `https://api.brevo.com/v3/smtp/email` | For `_KIND=brevo` (Brevo's HTTP API): the API key (required) and the endpoint (tests point it at a fake server) |
| `SKRONGLE_OPENAI_BASE_URL` | `https://api.openai.com` | Where the OpenAI provider sends requests: any OpenAI-compatible server (Ollama, LM Studio) |
| `SKRONGLE_ANTHROPIC_BASE_URL` | `https://api.anthropic.com` | Where the Anthropic provider sends requests (a proxy, the dev AI provider) |
| `SKRONGLE_GOOGLE_BASE_URL` | `https://generativelanguage.googleapis.com/v1beta/openai` | Where the Google provider sends requests |
| `SKRONGLE_DEEPSEEK_BASE_URL`, `SKRONGLE_MISTRAL_BASE_URL`, `SKRONGLE_XAI_BASE_URL`, `SKRONGLE_OPENROUTER_BASE_URL` | `https://api.deepseek.com`, `https://api.mistral.ai`, `https://api.x.ai`, `https://openrouter.ai/api/v1` | Where those providers send requests (#24). An Ollama-compatible server has no variable: its base URL is the one the owner saves on Manage Users |
| `SKRONGLE_AI_TIMEOUT_SECS` | — | How long one model request may take, in place of 900 s for generation and 120 s for a chat turn (`scripts/e2e.sh` sets 8) |
| `SKRONGLE_SQL_TRACE` | — | A file: every SQL statement the server runs is appended to it as a JSON line with its time, duration, SQLite's step and scan counters and the filled-in SQL (docs/testing.md, "Query trace") |
| `SKRONGLE_AI_JOB_BUDGET_SECS` | `3600` | How long one AI job (a generation, Only add, Edit lessons, Allow overall changes) may run in all; past it the job stops before its next request, and no request runs past it |
| `SKRONGLE_ALLOW_PRIVATE_AI_URLS` | `0` | `1`: a base URL typed into an AI form may point at loopback, private or link-local addresses (a self-hosted Ollama on the LAN), on any port. Off, such URLs are refused, and only ports 443 and 80 are allowed, so learners cannot make the server call its own network (dev mode always allows them). `SKRONGLE_OPENAI_BASE_URL` is never checked |
| `SKRONGLE_SECRET_KEY` | *(generated)* | 64 hex characters; encrypts stored API keys. Unset: read from or created at `<SKRONGLE_DB>.key` (mode 0600) |
| `SKRONGLE_NEW_SECRET_KEY` | `0` | `1`: the secret key is lost for good; create a new one, delete the saved API keys it cannot read, and turn off the authenticator apps it cannot read (two-factor off for those accounts, #1068). Without it the server refuses to start when the database has saved keys but neither `SKRONGLE_SECRET_KEY` nor `<SKRONGLE_DB>.key` is there, or the key there opens none of them |
| `SKRONGLE_DEV_SMTP` | `1` | Dev mode without `SKRONGLE_MAIL_PROVIDERS`: `1` sends e-mail to the dev mailbox on a free port, a number pins the port, `0` only logs it ([testing](testing#e-mail-the-dev-mailbox)) |
| `SKRONGLE_DEV` | `0` | `1`: dev mode (`/dev/login`, `/dev/reset`, `/dev/mail`, `/dev/mail/send`, `/dev/jobs`, `/dev/inbox` and its SMTP mailbox, `/dev/ai` (the dev AI provider, which the default base URLs point at); sent e-mail keeps its body; the dev mailbox needs no sign-in). Refused unless `SKRONGLE_BIND` is loopback, `SKRONGLE_SECURE_COOKIES` is not `1` and `SKRONGLE_PUBLIC_URL` is unset or local; the `/dev/*` routes also refuse proxied or non-loopback-`Host` requests. Set by `./.dev-start`, never by `deploy/` |
| `SKRONGLE_DEV_RELOAD` | `0` | Dev mode only: `1` (set by `./.dev-start --watch`) appends a script to every HTML page that reloads it after the server restarts; see [testing](testing#live-reload---watch) |
| `TEST_COURSE` | `0` | `1`: also offer the courses under `data/test-courses/` (the test course, for automated tests). `SKRONGLE_TEST_COURSE` works too. Never set in production |
| `SKRONGLE_DEV_SEED` | `0` | Dev mode only: at boot, create the dev user with a course open and some progress |
| `SKRONGLE_DEV_SEED_USERS` | `0` | Dev mode only: at boot, a crowd of this many made-up learners with weeks of history (`dev_crowd.rs`, [testing](testing#a-crowd-of-learners-with-history)); `SKRONGLE_DEV_SEED_DAYS` (90) is how far back it goes |
| `SKRONGLE_RNG_SEED` | — | Dev mode only: a number; quizzes, sessions and games draw from it instead of the OS, so a browser test replays exactly (`scripts/e2e.sh` sets it from `E2E_SEED`) |
| `SKRONGLE_LLM_MODE` | (empty) | Dev mode only. Empty (or `dev`): the providers' default base URLs point at the dev AI provider (`/dev/ai`, [testing](testing#the-dev-ai-provider)), which answers from the fixtures over their wire protocols; `capture`: the same, holding each request for a reply typed at the bottom of `/dev/ai` (Manual replies); `live`: the real providers (and the seed saves no keys). `fixtures` is read as the default |
| `SKRONGLE_LLM_DIR` | `./dev-llm` | Dev mode only: where held requests are written and their replies read |
| `SKRONGLE_LLM_FIXTURES` | `data/test-fixtures/llm` | Dev mode only: the dev AI provider's canned replies, one `<purpose>.json` per call site |
| `SKRONGLE_LLM_TIMEOUT_SECS` | `600` | Dev mode only: how long a held request waits for its reply |
| `SKRONGLE_LLM_DELAY_MS` | `0` | Dev mode only: how long a canned reply takes without its own `delay_ms`; `./.dev-start` sets 500 |

Dev mode and the dev AI provider are described in [testing](testing).

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
material inside `<material>` tags. It also carries the rules for one kind of
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

## Deploying

`deploy/deploy.env` (gitignored; template `deploy/deploy.env.example`
documents every variable) holds the settings for the deploy; a variable
exported from the shell overrides the file (`scripts/deploy-env.sh`).
`scripts/deploy-all.sh` runs the tests, cross-compiles once for aarch64 and
calls the per-target script:

- **Pi / DietPi:** `scripts/release.sh` → `scripts/deploy.sh`: rsync + systemd
  unit + nginx snippets, `/etc/spanish-learning/env` for the public URL. Read-only install, database in `/var/lib/spanish-learning`.

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
  codes count against sign-in, and a blocked sign-in blocks it); password reset: 10 link requests per
  address per hour, 3 links (and 3 "you already have an account"
  messages) per e-mail address per hour, past which nothing is sent and the
  page says the same, and 10 new passwords per address per 10 minutes. The address is the peer, or
  the last `X-Forwarded-For` entry when the peer is a proxy on loopback;
  an IPv6 address counts by its /64. Each limit keeps its own table of at
  most 64 keys (`caps::MAX_RATE_LIMIT_KEYS`); a full table drops ended
  windows and otherwise refuses new keys (as over the limit) rather than
  evicting a running one, so a flood cannot reset an account's
  wrong-password count.
  Changing the password or two-factor sign-in
  and *Sign out everywhere else* sign out the account's other sessions; a
  reset link signs out all of them. Registration never says whether an
  address has an account (#379).
- Two-factor sign-in (`src/two_factor.rs`, [features](features#two-factor-sign-in)):
  with a method on, the password only leaves a challenge named
  by a 256-bit token in the `twofa` cookie (HttpOnly, SameSite=Lax, 10
  minutes; the table keeps its SHA-256); the session is made after the
  code. E-mailed codes are 6 random digits, 10 minutes, 5 tries
  per sign-in, kept across *Send a new code* (in the database, so on in
  dev mode too), stored as
  SHA-256(salt:token:code): a 6-digit code can't be protected by hashing
  alone, but without the cookie's token a copy of the database can't be
  searched for it. Wrong codes (the last one too) also count as wrong
  passwords for the account's limit, and while that limit is reached
  `/login/code` and its resend refuse too; *Send a new code* is limited to
  5 per account per 15 minutes. Authenticator codes: RFC 6238, ±1 step, a step accepted once
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
  of them (`courses::MAX_STORES`, one active course per account), least
  recently used out first. Lesson Markdown is rendered by pulldown-cmark; raw HTML in
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
- Account data: *Profile → Your data* deletes the account (`POST /profile/delete`: the password,
  or for an account without one the e-mail address typed out plus its
  second step's code, if it has one). Deletion stops the account's
  generation jobs, tutor chats and games in memory, removes the `users` row
  (every user table cascades from it) and the caches of its courses, and clears the cookie.
- Logs name users by id, never by name or address; model errors are scrubbed of
  the key before they are logged or shown (`ai::redact_key`).
