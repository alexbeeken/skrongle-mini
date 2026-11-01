# Data inventory

What the app stores about a learner, where, why, whether it leaves the
server, and for how long. It is the record of processing behind the
[privacy policy](privacy), taken from the schema in `src/db.rs`
(migration v25). Not legal advice; the promises made from it are in the
[privacy policy](privacy) (sections 9, 15 to 19).

`src/privacy.rs` lists every user table in `USER_TABLES`: the data export
reads them, account deletion removes them, and a test fails when a table is
added to the schema without being listed there. Add new tables here too.
The export copies only the columns `privacy::EXPORTED` names for each table;
the ones it keeps out on purpose (secrets, codes, links, message text) are in
`privacy::WITHHELD`, and a test fails when a column is in neither (#429).

## In the database (SQLite, `SKRONGLE_DB`)

Every row below is removed when the account is deleted (`ON DELETE CASCADE`
from `users(id)`, directly or through the owning course<!-- themes --> or theme<!-- /themes -->), and is
included in the export unless noted. The exceptions are `sms_sends` and
`privacy_actions` (`privacy::KEPT_ON_DELETE`): their rows lose their
account and stay (`sms_sends` until purged). `ai_spend_days` (`privacy::SHARED_TABLES`) belongs to no account: it is the
server's daily AI spend, `jobs` (also shared) is the background job queue, and `content_check_worker` (also shared) is when the content-check worker last checked in.

| Table | Columns with personal data | Why | Leaves the server? | Kept |
|---|---|---|---|---|
| `users` | `email`, `first_name`, `last_name`, `avatar`, <!-- themes -->`theme` (no longer read since #577: a theme is a course's setting), <!-- /themes -->`active_course`, `created_at`, `google_sub`, `age_confirmed`, `developer` (a developer account, v24; nothing reads it since #298, and its `mock_ai` flag was dropped in v40), `ai_off` ("Hide all AI features", v27), `email_verified_at` (when the address was confirmed, v29), `superuser` (the Superuser section, v30), `avatar_color` (the colour behind the picture, v34), `review_quiet` (the review of AI changes does not open by itself, v38), `phone` (the profile's phone number, E.164) and `phone_verified_at` (when a texted code confirmed it), `two_factor` (the second sign-in step in use: '', `sms`, `email` or `totp`; v39), `lang` (the language the app is shown in, '' = the browser's; v42), `billing_ref` (a random reference Stripe's subscription metadata carries instead of the account id) and `stripe_customer` (the account's Stripe customer id; v45), `motivator` (the motivator worn, '' = the default; v48; since v51 only what a newly opened course starts with), `ai_credits` (prepaid Skrongle AI credit, micro-dollars, v53), `signup_credit_at` (when a new account got its one-time Skrongle AI credit, v87, #931), `own_keys_fallback` (on Plus, "use my keys when my Skrongle AI limit is hit", v54), `must_change_password` (1 after the owner set a temporary password, until the account picks its own; v62, #635), `phone_opted_out_at` (when a text to the number was refused because it replied STOP; cleared when a text gets through or the number changes; v69, #969), `suspended_at` (when a superuser suspended the account for a breach of the Terms; v70, #967), `beta` (set by the removed waitlist; nothing reads it; v86, #934), `delete_at` (when an account whose owner asked to delete it is deleted: 14 days after asking, or the end of a cancelled plan's paid time if later; v88, #970); `ai_effort` (the effort *Generate the course* last ran at: `low`, `medium` or `high`; v92, #918); A password account whose address was never confirmed is deleted, with everything in it, when the address is registered again `SKRONGLE_UNCONFIRMED_DAYS` (default 7) days or more after it was made (#379). `password_hash` (argon2id; `''` for Google-only accounts) | the account and profile | no; `google_sub`/`email` came from Google at sign-in | until deleted. The export leaves out `password_hash` |
| `sessions` | `session_token` (the SHA-256 of the cookie's random 256-bit token; the token itself is never stored, since v36), `created_at`, `expires_at` | staying signed in | no | 30 days, purged hourly; deleted on logout. No IP address or user agent is stored. The export leaves out the token |
| `user_progress` | mastery per lesson, `read_at` | adaptive practice | no | until deleted |
| `quiz_history` | every answer: `selected_answer` (the learner's text), `was_correct`, `answered_at` | spaced practice, history page | no | until deleted; answers older than `SKRONGLE_RETAIN_HISTORY_DAYS` (default 730 days, two years; `0` keeps them) are deleted hourly |
| `journey_progress` | Guide steps, stars (its `xp` column is no longer written, #558) | the Guide | no | until deleted |
| `user_vocab` | collected words, review counts, when each is due next | vocabulary review | no | until deleted |
| `lesson_passes` | seen/passed per lesson, `evidence` (an agent's note) | one "passed" for every mode | no | until deleted |
| `game_scores` | scores per game board | leaderboards: **other learners of the course see the first name and last initial next to the score** | no | until deleted |
| `game_activity` | finished rounds: durations, counts (its `xp` column is no longer written, #558) | the home page and history | no | until deleted; rounds older than `SKRONGLE_RETAIN_HISTORY_DAYS` (as `quiz_history`) are deleted hourly, so streaks and the games' recent bests reach back that far (round counts are kept in `game_rounds`) |
| `activity_log` | `kind` of activity (an answer, a game item, a flashcard, a lesson read…), how many (`n`), `at` | the daily goal's effort points and the streak (`src/activity.rs`, v47) | no | until deleted; rows older than `SKRONGLE_RETAIN_HISTORY_DAYS` are deleted hourly, as `quiz_history` |
| `lesson_touches` | per lesson: the `mode` it was last opened or worked on in and when (`at`) | where every mode resumes, and in which mode *Continue* opens it (v47); Social counts the learners on a lesson in the last hour, anonymously (#501) | no | until deleted; one row per lesson touched |
| `lesson_notes` | per lesson: the learner's own note (`body`, plain text up to 500 characters) and when it last changed | "My note" on the lesson page (`src/notes.rs`, v93, #336) | no | until deleted; emptying the note deletes its row |
| `game_rounds` | per course and game: how many rounds the learner finished | "rounds played" on the Pictures, Listening, Numbers and Dates pages without counting `game_activity` (`src/games.rs`, v64) | no | until deleted; one row per course and game |
| `earnings` | per course: effort `points` ever logged, daily `goals` reached, the local `day` last active, its `day_points` and the day `streak` | XP, Skrongle Cash and the streak, without reading the whole activity log (`src/earnings.rs`, v56) | no | until deleted; one row per course |
| `motivator_purchases` | per course: the Skrongle Cash shop badges bought (`item`), the `price` paid and when | the badges on the SKC hero and the SKC balance, earned less spent (`src/motivators/shop.rs`, v68) | no | until deleted; one row per badge per course |
| `projects` | courses opened, when, the level started at<!-- themes -->, the theme<!-- /themes -->, profile picture and motivator worn in each, and whether its first-visit settings were done | the course picker | no | until deleted |
| `user_api_keys` | `provider`, `ciphertext` (ChaCha20-Poly1305, bound to user and provider), `updated_at` | AI features with the learner's own key | the decrypted key goes to that provider with each call | until removed or deleted. The export lists the providers only |
| `subscriptions` | Stripe `customer_id`, `subscription_id`, `status`, `price_id`, period end, the `plan` it pays for (v55) | paid plan | the customer id is Stripe's; card details never reach the server | until deleted. Deletion is refused while a subscription would keep billing; the Stripe customer itself stays at Stripe |
| `plan_grants` | Plus without paying, `note` | grants | no | until deleted |
| `plan_cancellations` | the plan cancelled, the `reason` picked (one of four) and the `feedback` typed, when (v54) | why learners leave | no | until deleted |
| `ai_usage` | one row per model call: `purpose`, `provider`, `model`, token counts, `cost_micros`, `credit_micros`, `key_source` (`hosted` or `own`), the course and job | hosted AI allowances and credits (`src/metering.rs`); own-key calls are logged at cost 0 | no (the call itself went to the provider) | until deleted |
| `ai_usage_months` | per calendar month (UTC): the hosted `cost_micros`, the `credit_micros` part, `hosted_calls` and `own_calls`, the running totals of the learner's `ai_usage` rows (v65) | the monthly allowance without adding up the month's calls (`src/metering.rs`, #624) | no | until deleted; one row per month with AI use |
| `blocked_ips` | `cidr` (an address or range a superuser blocked), `created_at`; no account (v70, #967) | refusing requests from it (`src/moderation.rs`) | no | until a superuser removes it. Not exported (it is about no account) |
| `ai_spend_days` | per UTC day: everyone's hosted `cost_micros`, no account (v65) | the daily spending cap (#623) | no | kept; one row per day. Not exported (it is about no one); deleting an account lowers that day's figure |
| `jobs` | `kind`, `payload_json`, state, run and lease times, `last_error`; no account (v72) | background jobs (`src/jobs.rs`, #872): today only the hourly housekeeping, whose payload is empty (one of them, `housekeeping.deletions`, deletes the accounts whose `delete_at` has come, #970) | no | done rows deleted after a day, failed ones after 30 days (hourly housekeeping). Not exported (it is about no one) |
| `ai_credit_purchases` | Stripe Checkout `session_id`, `payment_intent`, `price_id`, amount paid, credit added and refunded | credit packs: applied once, refunds take back their share | the payment is Stripe's | until deleted |
| `terms_acceptances` | Terms `version` accepted, `accepted_at` (no IP address) | the record that the learner accepted the Terms | no | until deleted; one row per version accepted |
| `terms_announcements` | Terms `version`, `mailed_at` | that the account was e-mailed about a material change of the Terms 30 days ahead, so it is mailed once (#966) | no (the message itself is a `mail_outbox` row) | until deleted; one row per announced version |
| `breach_notices` | `notice` (a hash of the notice's subject and body), `mailed_at` | that the account was e-mailed a breach notice (#1068); nothing writes it any more | no (the message itself is a `mail_outbox` row) | until deleted; one row per notice |
| `consents` | an optional consent's `purpose` (`ai`: AI features), `granted_at`, `withdrawn_at` (v82) | the record behind the profile's Privacy and consent card (`src/consent.rs`, #974): when each consent was given and taken back | no | until deleted; a new row each time a consent is given again |
| `mail_outbox` | `to_addr`, `subject`, `body_text`/`body_html`, `purpose`, delivery state, `provider` | e-mail the app sends (`src/mail.rs`) | yes: the address and message go to the mail provider that sends it | while queued, a sign-in or two-factor code (`login-code`, `two-factor-confirm`) or a confirmation link (`email-verify`) is in the body in clear: that is the message being sent. Codes are never in the subject. Once a row is sent or has failed for good its bodies are emptied, and so is the subject of those code- and link-bearing purposes (`mail::SECRET_PURPOSES`); the sender also empties settled rows when it starts (#361). All of this is skipped in dev mode, whose outbox page shows them. Sent and failed rows deleted after 30 days. Deleting an account deletes its rows, except its address-confirmation e-mails of the last 24 hours, which stay without body or account (and unsent) until that day is over so the per-address cap still counts them (#395). The export has purpose, recipient, state and dates, never subject or body. Rows about no account (`user_id` NULL) are not in any export. While the job runner sends a row it holds a lease on it (`locked_by`, the server's process id and start time; `locked_until`), cleared once the row is settled; never exported (#873) |
| `sms_outbox` | `to_number`, `body_text`, `purpose`, delivery state, `provider` | text messages the app sends (`src/sms.rs`); the send limits count `sms_sends` | yes: the number and text go to the SMS provider that sends it | while queued the text, often a code, is in clear; it is emptied once sent or failed for good, and at start for older settled rows (kept in dev mode); sent and failed rows deleted after 30 days. The export has purpose, number, state and dates, never the text; rows about no account are not in any export. The send lease (`locked_by`, `locked_until`, #873) is cleared once settled and never exported |
| `sms_sends` | `number_key` (SHA-256 of the number; the number itself is not stored), `kind` (`signin` or `other`), `created_at`, `user_id` | the send limits (#362): per account, per number and for the server | no | two days, purged hourly. Kept when the account is deleted, with `user_id` set to NULL, so a new account gets no allowance back |
| `email_verifications` | `email` the link was sent to, `token_hash` (SHA-256 of the link's token; the token itself is never stored), `created_at`, `expires_at`, `used_at` | confirming the account's e-mail address (`src/verify.rs`) | no (the link itself leaves in a `mail_outbox` message) | a link works for 24 hours, once; unused ones are deleted when a new one is sent or the address changes; every row is deleted 7 days after it was made (hourly housekeeping). The export leaves out `token_hash` (the link itself is never in it: `mail_outbox` bodies are not exported) |
| `password_resets` | `email` the link was sent to, `token_hash` (SHA-256 of the link's token; the token itself is never stored), `created_at`, `expires_at`, `used_at` | choosing a new password from `/reset` (`src/reset.rs`, v66) | no (the link itself leaves in a `mail_outbox` message) | a link works for an hour, once; unused ones are deleted when a new one is sent or one is used; every row is deleted 7 days after it was made (hourly housekeeping). The export leaves out `token_hash` |
| `totp_keys` | `secret` (the authenticator app's TOTP secret, encrypted at rest like `user_api_keys`: ChaCha20-Poly1305 under the server's master secret, bound to the user; codes are worked out from it, so it can't be hashed), `created_at`, `confirmed_at`, `last_step` (the last code's time step, so a code works once) | two-factor sign-in with an authenticator app (`src/two_factor.rs`, v39) | no (shown once to the learner while setting the app up) | until removed, another method is chosen, or deleted; an unconfirmed set-up is deleted after a day. The export leaves out `secret` (encrypted or not, it would let anyone make codes) and `last_step` |
| `phone_verifications` | `phone` the code was texted to, `code_hash` and `salt` (SHA-256 of the code; the code itself is never stored), `attempts`, `created_at`, `expires_at` | confirming the profile's phone number | no (the code itself leaves in an `sms_outbox` text) | 10 minutes, five tries; deleted when used, replaced or the number changes, expired rows hourly. The export leaves out `code_hash`, `salt` and `attempts` |
| `two_factor_challenges` | `token_hash` (SHA-256 of the `twofa` cookie's token), `method`, `code_hash` and `salt` (SHA-256 of the texted or e-mailed code with the token), `attempts`, `created_at`, `expires_at`, `sent_by` (`email` when a texted code went by e-mail because the number replied STOP, v69) | a sign-in waiting for its second step | no (the code leaves in a `mail_outbox` or `sms_outbox` message) | 10 minutes, five tries; deleted when used, cancelled or replaced, expired rows hourly. The export leaves out `token_hash`, `code_hash`, `salt` and `attempts` |
| `two_factor_confirmations` | `method`, `code_hash` and `salt` (SHA-256 of the code with a random salt), `attempts`, `created_at`, `expires_at` | a Google-only account (no password) turning its texted or e-mailed second step off, switching it, or deleting the account: the code sent to prove it | no (the code leaves in a `mail_outbox` or `sms_outbox` message) | 10 minutes, five tries; deleted when used or replaced, expired rows hourly. The export leaves out `code_hash`, `salt` and `attempts` |
| `courses` | `title`, `brief` (what the learner asked for), `notes` (pasted material), `outline`, `error`, `taken_down_at` (when a superuser took it down, #967), `owner_emailed_at` (when a superuser last e-mailed the owner why it was flagged or taken down, #1035) | user-made and AI courses | `brief`, `notes` and the course go to the chosen LLM provider while generating or revising | until deleted; `notes` emptied once an AI course is ready (`SKRONGLE_KEEP_NOTES=1` keeps them); an AI course whose generation failed is deleted, notes and files with it, after `SKRONGLE_FAILED_COURSE_RETENTION_DAYS` (30) days without a resume or edit |
| `course_files` | the course's JSON and SVG files | the course | sent to the provider when revising or adding a lesson | until deleted |
| `course_undo`, `course_undo_files` | earlier versions (ten steps) | Undo | no | until deleted, ten steps per course |
| `course_ai_runs`, `course_ai_run_files` | the files the latest AI run changed, before and after, what the learner left out of it (`excluded`, v38) and their own text for lines of it (`edits`, v41) | Review AI changes | no | until deleted; one run per course, replaced by the next |
| `course_shares` | `email` (an address the owner typed to share a course with), `token_hash` (SHA-256 of the invitation link's token; the token itself is never stored), `user_id` (the account that opened the link), `created_at`, `opened_at` | sharing a course with other learners (`src/share.rs`) | the address gets the invitation through the mail provider (a `mail_outbox` message) | until the owner removes the address or deletes the course; a deleted recipient account unlinks the row (the address stays on the owner's list). In the export of the owner (their shares) and of the recipient (the shares opened by their account). The export leaves out `token_hash` |
| `course_flags` | `state` (clear, flagged or overridden), `source` (which check), `matches_json` (the listed words found, with the file, field, lesson and the text around them), `reason`, `content_hash` (SHA-256 of what was checked), `checked_at`, `override_note`, `overridden_by` and `overridden_at` (a superuser's decision), `terms_version` and `ai_version` (the rules each check ran under, v78, #1039) | the content check of made courses (`src/course_flags.rs`, v74, #1033) | no | as long as the course. In the owner's export, without `content_hash` and `overridden_by` |
| `content_scores` | per part of a made course (`chunk`: the lesson file, `#2`… for a long one's later parts): `lesson` (its title), `hash` (SHA-256 of the text sent), `high` (the categories it scored high on, v80, #1046), `model`, `input_tokens`, `cost_usd`, `review` (queued for the AI check's stage 2, then its verdict: fine or harmful), `scanned_at`, `ai_version` (the rules it was scored under, v78, #1039) | the AI content check, stages 1 and 2 (`src/content_check.rs`, v75, #1036; `content_check/review.rs`, #1037) | the lesson's text goes to TypeSafe (Jev) to be scored; a part that scored high goes to the hosted AI provider for review | as long as the course. In the owner's export, without `hash` |
| `content_category_scores` | per part of a made course and category of the AI check (misinformation, violence, self-harm, weapons, firearms, hate, tragedy, adult, minors, sexual crimes, terms): `score` (Jev's probability) | the AI content check, stage 1 (`src/content_check.rs`, v80, #1046) | no (the text it scores is sent with `content_scores`' part) | as long as the course. In the owner's export |
| `content_checks` | `checked_at` (when the content check last read the course's files) | the nightly content check (`src/content_check/nightly.rs`, v77, #1038): a course edited since is checked again at midnight | no | as long as the course. In the owner's export |
| `content_check_worker` | the worker's name (process id and start time) and `seen_at`; no account (v79) | `skrongle content-check` checks in every minute (`src/content_check/worker.rs`, #1041); the course table on /superuser shows it | no | one row, overwritten. Not exported (it is about no one) |
| `signup_invites` | `token_hash` (SHA-256 of the invitation link's token; the token itself is never stored), `created_by` (the superuser who made it; empty for the owner link, whose token is also written to the server log at startup while there is no account), `created_at`, `expires_at`, `used_at`, `used_by` (the account that registered with it), `email` (set by the removed waitlist; nothing reads it; v86, #934) | sign-up by invitation (`SKRONGLE_SIGNUP=invite`, `src/signup.rs`, v61, #634) | no (the superuser copies the link and sends it themselves) | a link works once, for 7 days (the owner link for a year, replaced at each start, deleted at startup once an account exists); rows are deleted 30 days after they expire, when revoked, or with the account that made them; a deleted invited account unlinks the row. In the export of the maker and of the invited account. The export leaves out `token_hash` |
| `privacy_actions` | `superuser_id` (who acted), `action` (`export`, `forget`, `delete-account`, `sign-out` or `notice`), `reason` (typed by the superuser), `created_at` | the log of privacy requests answered (v85, #981); nothing writes it any more. It names no learner | no | kept as a record; when the superuser's account is deleted its rows stay without `superuser_id` (`privacy::KEPT_ON_DELETE`). In the superuser's own export |
<!-- themes -->
| `user_themes`, `user_theme_files` | themes the learner made | themes | no | until deleted |
<!-- /themes -->
| `data_exports` | `state` (queued, building, ready, downloaded, expired, failed), `token_hash` (SHA-256 of the mailed download link's token; the token itself is never stored), `file` (the file's name in the exports folder), `link_base` (the site the link points to), `bytes`, `error`, `created_at`, `ready_at`, `expires_at`, `downloaded_at` | "Download my data" built in the background (`src/exports.rs`, v44, #430) | no (the link leaves in a `mail_outbox` message with purpose `data-export`) | the link works once, for 24 hours; the file is deleted when downloaded, when the link expires (hourly housekeeping) or with the account; rows are deleted 30 days after they were made. The export leaves out `token_hash`, `file`, `link_base` and `error` |

The memo in #74 (written before migrations 15–22) said there was no Stripe
code; there is (`src/billing.rs`, migration 15). It also predates
<!-- themes -->`user_themes` (v20), <!-- /themes -->`course_undo` (v21) and `age_confirmed` (v22).

## On disk besides the database

- **Data exports** (`<SKRONGLE_DB>.exports/`, e.g. `/var/lib/skrongle/learn.db.exports/`):
  the JSON file of a "Download my data" export, written a page at a time by
  the `export` job while it is built (`<id>-<random>.json.part`), then
  renamed. It holds what the export holds (`privacy::EXPORTED`). Deleted
  when downloaded, when its link expires after 24 hours, or when the
  account is deleted; files no row names are deleted by the hourly
  housekeeping.

## In memory only

| What | Why | Leaves the server? | Kept |
|---|---|---|---|
| AI tutor chats (`tutor::Sessions`): the transcript, the provider with its decrypted key | AI tutoring | the whole transcript goes to the provider each turn | 2 hours idle, or a restart; dropped at account deletion |
| Generation jobs (`ai::Jobs`): the request incl. notes and the decrypted key, the progress log | AI courses | yes, to the provider | 6 hours after finishing, or a restart; cancelled at account deletion |
| Timed game sessions (`games::Sessions`) | Word Match timing | no | 30 minutes; dropped at account deletion |
| Rate-limit counters (after #121): per address or account id, also learning writes per account (#381) | password guessing, scripted floods | no | minutes to an hour |
| Profile trips to Google (`oauth::StepUps`): the `state`, the account id and what for (link, confirm); accounts that just confirmed with Google | linking Google, a Google-only account proving it is its owner (#360, #376) | no | 10 minutes, used once, or a restart |

## In the browser

- Cookies: `sid`, the session (HttpOnly, SameSite=Lax, 30 days, `Secure`
  with `SKRONGLE_SECURE_COOKIES=1`); `oauth`, the Google sign-in state (10
  minutes); `twofa`, a sign-in waiting for its two-factor code (10
  minutes). All strictly necessary. The [cookie notice](cookies) lists
  them all for learners, with `signup`, `signup_invite`, `lang` and `tz`. No analytics, no third-party scripts,
  fonts or stylesheets: every asset is served by the app.<!-- themes --> A theme may name
  its pictures by `https://` URL, which the browser then fetches from that
  host.<!-- /themes -->
- `localStorage`: the voice and audio switch (`speech:*`), game and
  listening settings (`numbers:`, `dates:`, `listening:`),
  `sprint-sound`, `vocab-side`, `term`. Never sent to the server.
- Read aloud uses the browser's speech synthesis; some browsers' online
  voices send the text to the browser's vendor.

## Leaving the server

| To | What | When |
|---|---|---|
| Anthropic, OpenAI or Google (Gemini), or a custom OpenAI-compatible base URL | the learner's key, the system prompt, their notes/brief/feedback, course files, tutor transcripts, their first name (tutor prompt) | only when the learner uses an AI feature with their key. The forms say so (`data-testid="ai-disclosure"`) |
| The hosted AI provider (Skrongle AI's key, `SKRONGLE_AI_PROVIDER`) | the parts of a made course that Jev scored high, with the course title, languages and lesson list, other lessons the model asks for, and a superuser's override note; no account details | the AI content check's stage 2 (#1037, hosted only): only for parts that scored high |
| Google | the OpenID Connect sign-in | "Continue with Google" |
| The mail provider(s) in `SKRONGLE_MAIL_PROVIDERS` (e.g. Amazon SES) | the recipient's address, the subject and the message | each e-mail the app sends; none while unset (messages are only logged) |
| Resend (a `resend` kind in `SKRONGLE_MAIL_PROVIDERS`) | the recipient's address, the subject and the message, over its HTTPS API | each e-mail that provider sends |
| Postmark (a `postmark` kind in `SKRONGLE_MAIL_PROVIDERS`) | the recipient's address, the subject and the message, over its HTTPS API (plus the purpose as its tag and the outbox id as metadata) | each e-mail that provider sends |
| Brevo (a `brevo` kind in `SKRONGLE_MAIL_PROVIDERS`) | the sender and recipient addresses, the subject, the text and HTML body, and the message's purpose as a tag, over its HTTPS API | each e-mail that provider sends |
| The SMS provider(s) in `SKRONGLE_SMS_PROVIDERS` (e.g. Twilio, or the operator's webhook) | the phone number and the text | each text the app sends: the code confirming the profile's phone number, and sign-in codes with text-message two-factor sign-in; none while unset (texts are only logged) |
| Stripe | the account id as `client_reference_id`, e-mail for Checkout | Subscribe / Manage subscription |
| TypeSafe (Jev, `TYPESAFE_API_KEY`) | the text of a made course, a lesson at a time, with the course and lesson titles; no account details | the AI content check (#1036, hosted only): when an AI generation or revision finishes, before a share, and each night at midnight for courses edited since their last check (#1038), for the lessons that changed |
| Hosting | everything above, at rest | always: on the operator's own server; the privacy policy (section 8) says the United States |

## Logs

`tracing` to stdout (journald under systemd). Lines name users by **id**,
never by name or address (sign-in, registration, profile, keys, courses, games, tutor
errors, export, deletion). Model errors are scrubbed of the key
(`ai::redact_key`, tested). Mail lines name the outbox id and purpose, never the address, subject or text (also when no provider is set and mail is only logged, #361); provider errors quoting an address are scrubbed (`mail::scrub`). Text message lines name the outbox id, purpose and the number's last two digits; digit runs in provider errors are scrubbed (`sms::scrub`). Generation progress lines carry titles and
counts ("Building from your material (1200 characters)"), never the notes.
The app logs no IP addresses (#121's rate-limit warnings do log the
address). The operator sets the journal's retention and the proxy's
access log, which records IPs.

## Backups

A deleted account stays in backups until they expire. On the hosted site
no backup is kept longer than 31 days (#972; the nightly copies are #964).
On Skrongle mini the operator makes the backups and decides how long a
deleted account stays in them.

Deleting an account deletes its rows; nothing is kept in anonymized form.
So after a deletion nothing personal stays on the hosted site longer than
31 days, the limit the privacy notice gives.
