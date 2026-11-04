# Data inventory

What the app stores about a learner, where, why, whether it leaves the
server, and for how long. It is the record of processing, taken from the
schema in `src/db/migrations/v001_schema.sql`. Not legal advice.

`src/db/tables.rs` lists every user table in `USER_TABLES`: a test fails when a table is
added to the schema without being listed there. Add new tables here too.

## In the database (SQLite, `SKRONGLE_DB`)

Every row below belongs to an account (`ON DELETE CASCADE`
from `users(id)`, directly or through the owning course). `jobs`,
`server_api_keys` and `settings` (`db::tables::SHARED_TABLES`) belong to no
account: the background job queue and the server's AI keys and settings.

| Table | Columns with personal data | Why | Leaves the server? | Kept |
|---|---|---|---|---|
| `users` | `email`, `first_name`, `last_name`, `created_at`, `email_verified_at` (when the address was confirmed: only a proxy-made account and the dev user have one), `age_confirmed`, `owner` (Manage Users in the account menu: the person who runs the server), `must_change_password` (1 after the owner set a temporary password, until the account picks its own), `two_factor` (the second sign-in step in use: '' or `totp`), `active_course`, `lang` (the language the app is shown in, '' = the browser's), `review_quiet` (the review of AI changes does not open by itself), `ai_effort` (the effort *Generate the course* last ran at: `low`, `medium` or `high`), `delete_at` (unused, nothing reads it); `password_hash` (argon2id; `''` for an account a trusted proxy made) | the account and profile | no | until deleted. |
| `sessions` | `session_token` (the SHA-256 of the cookie's random 256-bit token; the token itself is never stored), `created_at`, `expires_at` | staying signed in | no | 30 days, purged hourly; deleted on logout. No IP address or user agent is stored. |
| `totp_keys` | `secret` (the authenticator app's TOTP secret, encrypted at rest: ChaCha20-Poly1305 under the server's master secret, bound to the user; codes are worked out from it, so it can't be hashed), `created_at`, `confirmed_at`, `last_step` (the last code's time step, so a code works once) | two-factor sign-in with an authenticator app (`src/two_factor.rs`) | no (shown once to the learner while setting the app up) | until removed, another method is chosen, or deleted; an unconfirmed set-up is deleted after a day. |
| `two_factor_challenges` | `token_hash` (SHA-256 of the `twofa` cookie's token), `attempts`, `created_at`, `expires_at` | a sign-in waiting for its second step | no | 10 minutes, five tries; deleted when used, cancelled or replaced, expired rows hourly. |
| `signup_invites` | `token_hash` (SHA-256 of the invitation link's token; the token itself is never stored), `created_by` (the owner who made it; empty for the owner link, whose token is also written to the server log at startup while there is no account), `created_at`, `expires_at`, `used_at`, `used_by` (the account that registered with it) | sign-up by invitation (`SKRONGLE_SIGNUP=invite`, `src/signup.rs`) | no (the owner copies the link and sends it themselves) | a link works once, for 7 days (the owner link for a year, replaced at each start, deleted at startup once an account exists); rows are deleted 30 days after they expire, when revoked, or with the account that made them; a deleted invited account unlinks the row. |
| `user_progress` | mastery per lesson, `read_at` | adaptive practice | no | until deleted |
| `quiz_history` | every answer: `selected_answer` (the learner's text), `was_correct`, `answered_at` | spaced practice, history page | no | until deleted; answers older than `SKRONGLE_RETAIN_HISTORY_DAYS` (default 730 days, two years; `0` keeps them) are deleted hourly |
| `journey_progress` | Guide steps, stars | the Guide | no | until deleted |
| `user_vocab` | collected words, review counts, when each is due next | vocabulary review | no | until deleted |
| `lesson_passes` | seen/passed per lesson, `evidence` (an agent's note) | one "passed" for every mode | no | until deleted |
| `game_scores` | scores per game board | leaderboards: **other learners of the course see the first name and last initial next to the score** | no | until deleted |
| `game_activity` | finished rounds: durations, counts | the home page and history | no | until deleted; rounds older than `SKRONGLE_RETAIN_HISTORY_DAYS` (as `quiz_history`) are deleted hourly, so streaks and the games' recent bests reach back that far (round counts are kept in `game_rounds`) |
| `activity_log` | `kind` of activity (an answer, a game item, a flashcard, a lesson read…), how many (`n`), `at` | the daily goal's effort points and the streak (`src/activity.rs`) | no | until deleted; rows older than `SKRONGLE_RETAIN_HISTORY_DAYS` are deleted hourly, as `quiz_history` |
| `lesson_touches` | per lesson: the `mode` it was last opened or worked on in and when (`at`) | where every mode resumes, and in which mode *Continue* opens it | no | until deleted; one row per lesson touched |
| `lesson_notes` | per lesson: the learner's own note (`body`, plain text up to 500 characters) and when it last changed | "My note" on the lesson page (`src/notes.rs`) | no | until deleted; emptying the note deletes its row |
| `game_rounds` | per course and game: how many rounds the learner finished | "rounds played" on the Pictures, Listening, Numbers and Dates pages without counting `game_activity` (`src/games.rs`) | no | until deleted; one row per course and game |
| `earnings` | per course: effort `points` ever logged, daily `goals` reached, the local `day` last active, its `day_points` and the day `streak` | XP and the streak, without reading the whole activity log (`src/earnings.rs`) | no | until deleted; one row per course |
| `projects` | courses opened, when, the level started at, the motivator worn in each, and whether its first-visit settings were done | the course picker | no | until deleted |
| `courses` | `title`, `brief` (what the learner asked for), `notes` (pasted material), `outline`, `error` | user-made and AI courses | `brief`, `notes` and the course go to the chosen LLM provider while generating or revising | until deleted; `notes` emptied once an AI course is ready (`SKRONGLE_KEEP_NOTES=1` keeps them); an AI course whose generation failed is deleted, notes and files with it, after `SKRONGLE_FAILED_COURSE_RETENTION_DAYS` (30) days without a resume or edit |
| `course_files` | the course's JSON and SVG files | the course | sent to the provider when revising or adding a lesson | until deleted |
| `course_undo`, `course_undo_files` | earlier versions (ten steps) | Undo | no | until deleted, ten steps per course |
| `course_ai_runs`, `course_ai_run_files` | the files the latest AI run changed, before and after, what the learner left out of it (`excluded`), their own text for lines of it (`edits`) and the run's notes on each lesson (`why`) | Review AI changes | no | until deleted; one run per course, replaced by the next |
| `course_shares` | `email` (a label the owner typed for the link), `token_hash` (SHA-256 of the link's token; the token itself is never stored), `user_id` (the account that opened the link), `created_at`, `opened_at` | sharing a course with other learners (`src/share.rs`) | no (the owner passes the link on) | until the owner removes it or deletes the course; a deleted recipient account unlinks the row (it stays on the owner's list). |
| `server_api_keys` | `provider`, `ciphertext` (ChaCha20-Poly1305, bound to the provider), `set_at`; no account | the AI features of every account, with the keys an owner saved on Manage Users | the decrypted key goes to that provider with each call | until an owner removes it |
| `settings` | `name`, `value`: server-wide settings (`ai_off`: the owner's "Hide AI features for everyone"); no account | what the owner set on Manage Users | no | until changed |
| `jobs` | `kind`, `payload_json`, state, run and lease times, `last_error`; no account | background jobs (`src/jobs.rs`): today only the hourly housekeeping, whose payload is empty | no | done rows deleted after a day, failed ones after 30 days (hourly housekeeping). It is about no one |


## On disk besides the database

## In memory only

| What | Why | Leaves the server? | Kept |
|---|---|---|---|
| AI tutor chats (`tutor::Sessions`): the transcript, the provider with its decrypted key | AI tutoring | the whole transcript goes to the provider each turn | 2 hours idle, or a restart|
| Generation jobs (`ai::Jobs`): the request incl. notes and the decrypted key, the progress log | AI courses | yes, to the provider | 6 hours after finishing, or a restart |
| Timed game sessions (`games::Sessions`) | Word Match timing | no | 30 minutes |
| Rate-limit counters (after #121): per address or account id, also learning writes per account (#381) | password guessing, scripted floods | no | minutes to an hour |

## In the browser

- Cookies: `sid`, the session (HttpOnly, SameSite=Lax, 30 days, `Secure`
  with `SKRONGLE_SECURE_COOKIES=1`); `twofa`, a sign-in waiting for its two-factor code (10
  minutes). All strictly necessary. Also set: `signup`, `signup_invite`, `lang` and `tz`. No analytics, no third-party scripts,
  fonts or stylesheets: every asset is served by the app.
- `localStorage`: the voice and audio switch (`speech:*`), game and
  listening settings (`numbers:`, `dates:`, `listening:`),
  `sprint-sound`, `vocab-side`, `term`. Never sent to the server.
- Read aloud uses the browser's speech synthesis; some browsers' online
  voices send the text to the browser's vendor.

## Leaving the server

| To | What | When |
|---|---|---|
| Anthropic, OpenAI or Google (Gemini), or a custom OpenAI-compatible base URL | the learner's key, the system prompt, their notes/brief/feedback, course files, tutor transcripts, their first name (tutor prompt) | only when the learner uses an AI feature with their key. The forms say so (`data-testid="ai-disclosure"`) |
| Hosting | everything above, at rest | always: on the operator's own server |

## Logs

`tracing` to stdout (journald under systemd). Lines name users by **id**,
never by name or address (sign-in, registration, profile, keys, courses, games, tutor
errors, deletion). Model errors are scrubbed of the key
(`ai::redact_key`, tested). Generation progress lines carry titles and
counts ("Building from your material (1200 characters)"), never the notes.
The app logs no IP addresses (#121's rate-limit warnings do log the
address). The operator sets the journal's retention and the proxy's
access log, which records IPs.

## Backups

Backups hold the database as it was when they were made. The operator makes
them and decides how long they are kept.
