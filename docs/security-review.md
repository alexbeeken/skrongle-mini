# Security review (October 2026)

Two deliberate passes over the code before it goes public and the site takes
payments: the first for issue #57 (2026-10-17), the second on 2026-10-19
(issues #356–#395 and #429–#431, fixes merged 2026-10-21). It records what
was checked, what was fixed, and what is accepted as a known risk, so the
next review starts from here. The sections after the two Fixed tables
describe main after both. How to report a problem:
[SECURITY.md](https://github.com/alexbeeken/pooki/blob/main/SECURITY.md).

Each fix has a test; the test names are given so they can be found with
`cargo test <name>`.

## Fixed in the first review (2026-10-17)

| Area | Problem | Fix | Test |
|---|---|---|---|
| Stored XSS | Lesson Markdown passed raw HTML and `javascript:` links through to pages rendered with `\|safe`. Course text is untrusted: uploads, and AI output written from pasted material that can carry instructions. | `content::render_markdown` shows raw HTML as text and points links/images whose scheme is not http(s) or mailto at `#`. No built-in course used either. | `markdown_never_renders_raw_html_or_script_links` |
| Stored XSS | `sprint.js` put the manifest's `terms.label` (and the `chars` buttons) into `innerHTML` unescaped. | `esc()` around both. | none automated (a one-line JS change; the rest of `static/*.js` was audited and escapes everything it inserts) |
| SSRF | The custom base URL on the AI forms made the server POST to any address, including loopback, the LAN and cloud metadata (`169.254.169.254`), and showed part of the reply. | `ai::check_base_url`: http(s) only, no credentials in the URL, and every address the host resolves to must be public, unless dev mode or `SKRONGLE_ALLOW_PRIVATE_AI_URLS=1`. Model calls no longer follow redirects. | `custom_base_urls_must_be_public`, `internal_addresses_are_recognised`, `base_url_hosts_parse` |
| CSRF | Only `SameSite=Lax` protected the HTML forms; it does not stop login CSRF (logging a victim into the attacker's account), sibling-subdomain posts or old browsers. | `security::csrf` refuses every non-GET request a browser marks as cross-site (`Sec-Fetch-Site`, else `Origin` vs `Host`/`X-Forwarded-Host`) with a 403. The Stripe webhook is exempt (signed). Requests without either header (curl, agents) pass. | `security_headers_and_cross_site_posts`, `cross_site_requests_are_recognised` |
| Headers | No CSP, framing protection or referrer policy. | `security::headers` on every response: CSP (`security::CSP`, `frame-ancestors 'none'`, `object-src 'none'`, `base-uri 'self'`), `X-Frame-Options: DENY`, `nosniff`, `Referrer-Policy: same-origin`; HSTS when `SKRONGLE_SECURE_COOKIES=1`. Routes that set their own CSP (pictures; in dev mode `/login/code/device`, framed by `/login/code`, with `frame-ancestors 'self'`) keep theirs. | `security_headers_and_cross_site_posts` |
| Brute force | No rate limits on sign-in, registration or password change; each attempt costs the Pi an argon2 hash. | `security::RateLimits` (in memory, bounded, off in dev mode): 30 sign-in attempts per address per 10 min, 10 wrong passwords per account (e-mail address) per 15 min, 20 registrations per address per hour, 10 password changes per account per 15 min. 429 with the form and a message. The address is the peer, or the last `X-Forwarded-For` entry when the peer is a loopback proxy. | `account_forms_are_rate_limited`, `rate_limits_count_per_key_and_window`, `rate_limit_memory_is_bounded`, `client_ip_trusts_only_a_local_proxy` |
| Sessions | Changing the password left every other session signed in (whoever knew the old password stayed in). | The change ends the account's other sessions. | `account_forms_are_rate_limited` |
| Dev routes | Dev mode was refused only for a non-loopback bind or `SKRONGLE_SECURE_COOKIES=1`, but deploys bind loopback behind nginx/Caddy, so `SKRONGLE_DEV=1` on a Pi without secure cookies would have exposed `/dev/login` to the internet. | Dev mode is also refused when `SKRONGLE_PUBLIC_URL` names a non-local host, and the `/dev/*` routes answer only requests with a loopback `Host` and no proxy headers. | `dev_mode_needs_a_loopback_bind_and_no_secure_cookies`, `dev_routes_refuse_proxied_and_non_local_requests`, `dev_routes_404_unless_dev_mode_is_on` |
| Secrets | `ai::Provider` and `billing::StripeConfig` derived `Debug`, so a future `{:?}` in a log would print API keys and secrets. | Hand-written `Debug` that prints `<redacted>`. | `debug_output_never_shows_secrets` |
| Secrets | A `<db>.key` copied in by hand or restored from a backup could be world-readable. | At startup it is set back to 0600, with a warning. | `master_hex_roundtrip_and_key_file` |
| Self-hosting | No warning when passwords and cookies cross a network without TLS. | Startup warns for a plain-HTTP non-loopback bind, and for an https `SKRONGLE_PUBLIC_URL` without `SKRONGLE_SECURE_COOKIES=1`. | `warns_about_plain_http` |
| Dependencies | `cargo audit`: rustls 0.23.45 (RUSTSEC-2026-0346 to -0349, low/medium; reached through ureq's TLS client). | Updated to 0.23.46. `cargo audit` is clean. | `cargo audit` |

## Fixed in the second review (2026-10-19)

Thirteen PRs, merged into main on 2026-10-21. Each row names the issue, the
PR and its merge commit.

| Issue | Area | Problem | Fix | PR | Test |
|---|---|---|---|---|---|
| #358 | Terms gate redirect | `next=/%09/evil.example` passed `safe_next`; browsers strip the tab and follow `//evil.example`. | `terms::is_app_path` rejects whitespace and control characters too; shared with `/dev/login`. | #432 (`ea764f5`) | `next_stays_in_the_app` |
| #359 | Second sign-in step | "Send a new code" reset the 5 tries; `/login/code` ignored the per-account block; the last wrong code was not counted. | Tries kept across resends; `/login/code` and the resend refuse while `LOGIN_FAILURES_PER_USER` is reached; every wrong code counts. | #432 (`ea764f5`) | `a_resend_keeps_the_wrong_codes_already_typed`, `sign_in_codes_count_against_the_account_limit` |
| #371 | Rate-limit table | At 20,000 keys the oldest half was evicted, including running per-account counters; IPv6 keyed per address; `/login` made a key before the cheap length check. | One table per limit (10,000 keys each); a full table refuses new keys rather than evicting; IPv6 keyed by /64; length check first. | #432 (`ea764f5`) | `a_flood_of_new_keys_cannot_reset_an_account_counter`, `ipv6_clients_are_keyed_by_their_64`, `rate_limit_memory_is_bounded` |
| #364 | Billing | `users.id` had no `AUTOINCREMENT`, so a deleted account's id went to the next registrant, and Stripe events naming it (metadata `user_id`) attached the old subscription and customer to them. | `users` rebuilt with `AUTOINCREMENT` (migration v45); Checkout metadata carries a random per-account `billing_ref`; `apply_event` ignores events whose ref names no account, whose customer differs from the account's, or whose subscription predates the account; legacy `user_id` metadata counts only with the account's own customer. | #433 (`154190c`) | `a_deleted_accounts_subscription_never_reaches_the_next_account`, `users_rebuild_keeps_everything_and_never_reuses_ids` |
| #394 | Billing | Two concurrent Checkouts made two Stripe customers and two paying subscriptions; only one was tracked. | The account's Stripe customer is created (idempotency key) and stored before the first Checkout and named by every one; Checkout refuses while a subscription is `incomplete`; a second paying subscription is logged as a warning and the first kept. | #433 (`154190c`) | `checkouts_share_one_customer_and_wait_for_an_incomplete_payment`, `a_second_paying_subscription_keeps_the_first` |
| #361 | Mail | Sign-in and two-factor codes were in the mail subject, which was kept for 30 days and logged by `LogProvider`. | Codes only in the body. Code- and link-bearing rows lose subject and bodies once sent or failed, older rows are blanked at start, and `LogProvider` logs only the purpose. | #434 (`3932297`) | `code_mail_is_blanked_after_sending_and_on_failure`, `settled_rows_with_codes_are_blanked_at_start` |
| #361 | SMS | Failed texts kept their code in `sms_outbox.body_text`. | Blanked on final failure and at server start. | #434 (`3932297`) | `failed_and_settled_texts_keep_no_code` |
| #429 | Export | `SELECT *` minus a deny-list exported mail and text subjects and bodies (live codes and links). | Per-table allow-list `privacy::EXPORTED` plus `WITHHELD`; a test fails on an undecided column. | #434 (`3932297`) | `every_column_has_an_export_decision`, `export_has_every_table_and_no_secrets` |
| #367 | AI generation size | AI-written courses bypassed the per-course caps: any outline length, ~10 MB replies, no per-file or file-count check on the AI write path. | Outline capped at `MAX_LESSONS`; `check_file_caps` (`MAX_FILE_BYTES`, `MAX_FILES`) on every AI write path; replies read up to 1 MiB. | #435 (`5dc0f6d`) | `ai_generation_keeps_to_the_course_caps` |
| #368 | AI job slots | Capacity check and insert under separate locks; a deleted account's job freed its slot while its thread ran on; no time budget; no per-user start quota; failed jobs kept 6 h each without a limit. | Check and insert under one lock; slot held until the thread exits; 1 h job budget that also caps each request's timeout; 10/h and 40/day starts per account; 5 finished jobs kept per user. | #435 (`5dc0f6d`) | `ai_job_slots_are_taken_atomically_and_held_until_the_thread_exits`, `finished_ai_jobs_per_user_are_capped` |
| #372 | Saved API keys | An empty key box with a custom base URL sent the saved key to that URL. | A custom base URL requires a pasted key. | #435 (`5dc0f6d`) | `ai_features_with_saved_keys_answered_by_the_simulator` |
| #373 | Custom AI base URL ports | Any port on a public host was allowed, so the server could be used as a relay. | Only 443 and 80 unless `SKRONGLE_ALLOW_PRIVATE_AI_URLS=1` or dev mode. | #435 (`5dc0f6d`) | `custom_base_urls_use_only_web_ports`, `ai_features_with_saved_keys_answered_by_the_simulator` |
| #362 | SMS limits | Limits counted `sms_outbox` rows that cascade away with the account; no server cap or country filter (text bombing, toll fraud). | `sms_sends` ledger (migration v46: hashed number, `user_id` SET NULL, 2 days) counted by every limit; `SKRONGLE_SMS_MAX_PER_HOUR`/`_PER_DAY` server caps; `SKRONGLE_SMS_COUNTRIES` allow-list. | #436 (`d9ab14f`) | `the_limits_outlive_the_account`, `the_server_wide_cap_holds_for_each_kind`, `only_listed_countries_are_texted` |
| #363 | SMS lock-out | Anyone could put a victim's number on their profile and use up its per-number quota, blocking the victim's SMS sign-in codes. | Sign-in codes have their own per-number and server allowance; a number verified on one account is refused (unique partial index) when saved, texted or verified elsewhere. | #436 (`d9ab14f`) | `sign_in_codes_have_their_own_allowance`, `a_number_verified_on_one_account_is_refused_on_another`, `the_sms_ledger_migration_keeps_one_verification_per_number` |
| #431 | Phone numbers | Unconfirmed accounts could add any number and have it texted. | Only accounts with a confirmed e-mail address may add a phone or request a code; the profile locks the field with an (i). | #436 (`d9ab14f`) | `a_phone_needs_a_confirmed_address`, `a_phone_needs_a_confirmed_address_and_a_number_of_its_own` |
| #366 | Course storage | No total size cap per course or account, no course count cap, no rate limit on course writes; every save re-parses the course and the course cache was unbounded. | `courses::Limits` (8 MiB/course, 32 MiB/account, 50 courses; `SKRONGLE_COURSE_BYTES`, `SKRONGLE_COURSE_BYTES_PER_USER`, `SKRONGLE_COURSES_PER_USER`) checked by `room_for`/`room_for_course` inside the writing `Db::call` on create, upload, file save and the lesson table; `security::COURSE_WRITES_PER_USER` (120 per 5 min); the custom-course cache is LRU-bounded at 24 MiB of files. | #437 (`f72c56a`) | `course_and_account_size_limits`, `the_course_cache_drops_the_least_recently_used`, `course_writes_stop_at_the_limits` |
| #380 | Course ids | A course whose manifest id differs from its row id still opened and wrote under the chosen id, reaching other courses' leaderboards. | `Courses::get` returns `None` and caches nothing when `store.id() != id`. | #437 (`f72c56a`) | `a_course_claiming_another_id_does_not_open` |
| #383 | Active course | `Project` trusted `users.active_course` without re-checking ownership. | The session query also selects `courses.owner_id`; `Project` opens a non-built-in course only for its owner (or, since #451, a learner it is shared with) and clears the active course otherwise. | #437 (`f72c56a`) | `course_writes_stop_at_the_limits` |
| #374 | Two-factor on | Turning two-factor on (own phone/app) needed no password; the owner was not told. | `needs_proof` is any change; the account is e-mailed (`two-factor-changed`). | #438 (`5ad30a3`) | `every_change_of_method_needs_proof`, `account_changes_need_proof_and_sign_out_other_devices` |
| #375 | Sessions | Only a password change ended other sessions; the mail advised a non-existent remedy. | "Sign out everywhere else" (`POST /profile/sessions/end-others`); two-factor changes end other sessions; mail text fixed. | #438 (`5ad30a3`) | `account_changes_need_proof_and_sign_out_other_devices`, `the_mails_say_how_to_sign_out_everywhere_else` |
| #376 | Deleting without a password | Deleting needed only the e-mail address shown on the page. | `prove_owner`: the second step's code. | #438 (`5ad30a3`) | `privacy_page_export_and_account_deletion_over_http`, `account_changes_need_proof_and_sign_out_other_devices` |
| #378 | Profile password checks | Password change and account deletion neither checked nor hit `LOGIN_FAILURES_PER_USER`. | Both go through `prove_owner`. | #438 (`5ad30a3`) | `account_forms_are_rate_limited` |
| #390 | Deploy files | The owner's DuckDNS host and the Pi's LAN address were tracked in `deploy/` and `scripts/`. | Placeholders in the unit and nginx snippets, rendered by `deploy.sh` from `PI_PUBLIC_URL`; `PI_HOST` / `PI_PUBLIC_URL` required from `deploy/deploy.env`. | #439 (`da6d26e`) | `git grep` (manual) |
| #392 | Self-hosting docs | No tarball checksum; backup tarball world-readable. | `sha256sum -c` of the published `.sha256`; backup under `umask 077`. | #439 (`da6d26e`) | docs |
| #393 | Mail providers | Brevo accepted any `http://`; Postmark matched loopback by prefix. | Shared `api_url_is_safe` checks the parsed host. | #439 (`da6d26e`) | `brevo_api_url_must_be_https_or_this_machine`, `postmark_api_url_loopback_is_checked_on_the_parsed_host`, `config_reads_the_key_and_url_and_names_missing_variables` |
| #365, #430 | Export | `/profile/export.json` built the whole account as one JSON value in memory inside one `Db::call`, holding the only connection, with no rate limit (OOM and stall risk on the Pi). | Built in the background: a queued row, then the `export` job on the shared `jobs` runner (#875) writes the file page by page (short calls, a pause between them, 50 pages a run). A one-time, hashed, 24 h link that only works for the signed-in owner is mailed. One in flight and one a day per account. The file is deleted on download, on expiry and with the account. | #456 (`fdc18d2`) | `one_export_in_flight_and_one_a_day`, `an_export_is_built_in_pages_mailed_and_downloaded_once`, `expired_exports_and_strays_are_deleted`, `a_deleted_account_leaves_no_export`, `privacy_page_export_and_account_deletion_over_http` |
| #384 | Inline session JSON | Only `</` was escaped in the Sprint and Listening session JSON; a comment opener plus a script tag could swallow the closing tag and kill the page. | `routes::script_json` escapes `<` `>` `&` U+2028 U+2029 as `\u` escapes, used by both routes. | #457 (`fcf2443`) | `script_json_escapes_markup_characters` |
| #385 | `?notice=` | Free text from the URL on `/courses` and `/courses/{id}` was shown as a status alert (content spoofing). | The parameter is a key mapped to server-side text; unknown keys show nothing. | #457 (`fcf2443`) | `notices_are_keys_not_text` |
| #386 | Lesson Markdown images | `![](https://host/…)` loaded from any host (tracking pixel). | Only relative or `data:image/` image URLs; others become plain links<!-- themes --> (CSP unchanged for theme art)<!-- /themes -->. | #457 (`fcf2443`) | `markdown_never_renders_raw_html_or_script_links` |
| #387 | Delete confirms, picture paths | A template value inside an inline `onsubmit`; `is_picture_path` allowed any character. | `data-confirm` handled by `modal.js`; `is_picture_path` uses `valid_path`'s character set. | #457 (`fcf2443`) | `notices_are_keys_not_text`, `pictures_are_validated_and_served` |
| #381 | Learning APIs | Every answer and finished round appended a row with no limit, kept forever. | Per-account limits on the JSON write endpoints (60/10 s, 3600/h, 429 JSON; off in dev mode), 1000 agent-graded answers per account per 24 h; `SKRONGLE_RETAIN_HISTORY_DAYS` defaults to 730 and also sweeps `game_activity`. | #458 (`6c51077`) | `learning_writes_are_rate_limited`, `retention_drops_old_answers_rounds_and_finished_notes` |
| #382 | Tutor | Each `/api/tutor/start` made a new session and thread, so one account could hold many 120 s model calls at once. | One call in flight per user, enforced in `Sessions::start` and `begin` (429 shown inline) and, for a call whose request has gone (the learner left mid-turn), by the tutor thread's slot until the provider answers; at most 4 tutor threads server-wide. Cancel (`/api/tutor/cancel`) frees the learner but not the thread, and only one cancelled call per learner may run, so a learner holds at most two threads. | #458 (`6c51077`), #848, #890 | `one_tutor_call_in_flight_per_user`, `tutor_threads_are_capped`, `one_tutor_thread_per_learner`, `a_cancelled_call_frees_the_learner_not_the_thread` |
| #357 | Math checker | A deeply nested `expression` answer overflowed the stack and aborted the server. | `parse` refuses inputs over 200 characters, nesting past 64 levels and trees deeper than 64; `eval` has a depth guard. | #459 (`d8ac79e`) | `deep_nesting_is_an_error_not_a_crash` |
| #369 | Listener | `axum::serve` had no timer (so no header timeout) and no connection limit. | `src/server.rs`: 15 s header read timeout, 30 s body idle timeout, `SKRONGLE_MAX_CONNECTIONS` (256); `LimitNOFILE=1024` in the units; self-hosting says to use a proxy. | #459 (`d8ac79e`) | `slow_clients_are_cut_off_and_connections_are_capped` |
| #370 | Request bodies | Every route read up to 2 MiB before any check; `/login` parsed the body before its rate limit. | Global 64 KiB `DefaultBodyLimit`; 2 MiB only on session-gated editor/AI routes and dev tools, 256 KiB on the webhook; `/login` and `/register` check the per-IP limit before reading the body. | #459 (`d8ac79e`) | `request_bodies_are_capped` |
| #389 | Panics | `panic = "abort"`: any reachable panic kills the server. | Abort kept (binary size; a catch-panic layer cannot catch a stack overflow); `scripts/panic-check.sh` in CI fails when a file gains unwrap/expect/panic!/indexing outside tests. | #459 (`d8ac79e`) | `scripts/panic-check.sh` (CI step in `cargo test`) |
| #388 | Dev mode | `SKRONGLE_DEV=1` on a deploy behind a minimal nginx block (loopback `Host`, no forwarding headers) exposed `/dev/login` and `/dev/inbox`. | Dev mode is refused when the database holds accounts besides the dev user, unless `SKRONGLE_DEV_FORCE=1` (`./.dev-start` sets it). | #460 (`f95b90a`) | `dev_mode_refuses_a_database_with_other_accounts` |
| #388 | Dev mode | Only `/dev/*` checked `Host`, so a DNS-rebinding page could use the dev server's ordinary routes with the public dev password. | In dev mode every route answers 421 to a `Host` other than localhost/127.0.0.1/[::1]. | #460 (`f95b90a`) | `dev_mode_answers_only_local_hosts_on_every_route` |
| #395 | Mail | No per-address cap: registering a stranger's address sent them 5 mails an hour indefinitely, and re-registering reset it. | At most 10 verification mails per address per day, counted from outbox rows that outlive account deletion for that day. | #460 (`f95b90a`) | `verification_mail_is_capped_per_address_even_across_account_deletion` |
| #395 | Mail | No global outbound ceiling. | `SKRONGLE_MAIL_PER_HOUR` (default 100) caps everything queued per hour. Since #579: per purpose, default 3000, and capped mail waits for the next hour instead of being dropped. | #460 (`f95b90a`) | `over_the_hourly_ceiling_mail_waits_for_the_next_hour` |
| #395 | Mail | The verification mail promised "nothing else will be sent". | Reworded to state the daily cap. | #460 (`f95b90a`) | `verify::tests::the_message_carries_the_link` |

Migrations were renumbered when these PRs merged: #433's `users` rebuild
landed as v45 (its PR body says v42) and #436's `sms_sends` ledger as v46.

Still open from this review:

- **#379, PR #461** (registration never reveals whether an address has an
  account; password reset): not merged. Its "same answer for every
  address, no session" registration conflicts with course sharing's
  invite link that registers and lands the learner in the shared course
  (#492), so it waits for a product decision.
- **#356** (CI runners): blocked until the repository is public; see
  Accepted risks.

## Checked and found sound

- **Passwords**: argon2id (8 MiB, 3 passes, per-user salt); unknown addresses
  verify against a dummy hash so timing does not reveal them; 8–128
  characters.
- **Session cookies**: 256-bit random token, `HttpOnly`, `SameSite=Lax`,
  `Path` set, `Secure` with `SKRONGLE_SECURE_COOKIES=1`; fixed 30-day expiry,
  checked in SQL on every request, purged hourly; logout deletes the row and
  clears the cookie; a new token on every login (no fixation).
- **Private courses**: every `/courses/{id}…` and `/api/courses/{id}…` handler
  that changes a course goes through `owned()` (404 or 400 for anyone but the
  owner); opening a course checks `accessible()` (built-in, owned, or shared
  with the account, #451), and `Project` checks the same again on every
  request (#383), so every learning route and `/pictures` only ever loads a
  built-in course, one the user owns, or one shared with them (read-only). A
  share by e-mail address matches only an account that has confirmed that
  address.<!-- themes --> Made themes are owner-only (`made_themes_are_private_over_http`).<!-- /themes -->
- **Generation jobs**: `/api/generate/{job}` (and the old
  `/courses/generate/{job}` redirect) check the job's user (`job_of`), and
  the editor's modal shows only the owner's job on their own course; cancel, resume, undo and AI changes go
  through `owned()`.
- **Progress API** (`/api/progress…`): agents use the learner's own session
  cookie; every handler takes `Project`, so it reads and writes only that
  user's rows in their active course.
- **In-memory sessions**: tutor chats and timed games are looked up by id
  *and* user id.
- **Stripe**: the webhook checks `Stripe-Signature` (HMAC-SHA256, constant-time
  compare, 5-minute tolerance) before parsing the event; the plan changes only
  through it; the secret key stays on the server; no secret is in templates or
  JS.
- **Learner API keys** (`src/secrets.rs`): ChaCha20-Poly1305 with a fresh
  nonce, user id and provider as associated data, master secret from
  `SKRONGLE_SECRET_KEY` or a 0600 file created with `create_new`; never logged,
  never sent back (last four characters only). Keys pasted for one job live
  in memory only.
- **Uploads and the editor**: paths must be `course.json`,
  `lessons/<name>.json` or `pictures/<name>.svg` (no `..`, no leading slash,
  a safe character set, 120 characters); 1 MiB per file, 400 files per course,
  100 per upload, 8 MiB per course and 32 MiB per account in all, 50 courses
  per account, 120 course writes per 5 minutes (#366); request bodies are
  capped at 64 KiB, 2 MiB only on the session-gated editor and AI routes and
  the dev tools, 256 KiB on the Stripe webhook (#370); JSON parsing is
  serde_json with its recursion limit (128). AI-written courses go through
  the same per-file and file-count caps (#367). SVGs are served with
  `default-src 'none'` and `nosniff`, and shown through `<img>`.
- **Templates**: askama escapes by default. The `|safe` uses are: lesson
  and stage Markdown (now sanitised, above), the Sprint and Listening session
  JSON inside `<script type="application/json">` (`routes::script_json`
  escapes `<`, `>`, `&`, U+2028 and U+2029, #384), and `/docs` (the repository's own files). All of
  `static/*.js` was read for `innerHTML`/`insertAdjacentHTML` sinks: every
  `esc()` escapes `& < > " '`, and apart from the Sprint label above every
  course or user value is escaped; there is no `eval` or `new Function`.
  Tutor replies are escaped before the small bold/italic formatting.
- **Open redirects**: the redirects that take a target from the request
  (`/terms/accept?next=`, `/dev/login?next=`) accept only an app path
  through `terms::is_app_path`: it starts with one `/` and has no `\`,
  whitespace or control character, so a tab cannot turn it into
  `//evil.example` (#358). Sign-in after a share link lands on a path the
  server builds from the token. One weaker check remains: the language
  picker's `POST /lang` `back` refuses `//` but not `/\` or a tab, so it
  could send the browser off-site; a cross-site form cannot post it (the
  CSRF check), so only a same-site page could use it.
- **Codes and links at rest**: sign-in, two-factor and phone codes and the
  verification and export links are stored only as SHA-256 hashes (codes
  with a random salt). Their plain text exists only in the message itself:
  a `mail_outbox` or `sms_outbox` row keeps it while queued and loses it
  (and a code mail its subject) once sent or failed, and it is never logged
  or exported (#361, #429). An authenticator app's secret is sealed like
  learner API keys.
- **Prompt injection**: pasted material is wrapped in `<material>` tags and the
  model is told to treat it as content. What a hostile file can still make the
  model write is lesson text: wrong or misleading content, or an http(s) link.
  It cannot add script (Markdown is sanitised, SVGs cannot run), cannot reach
  another account (the job writes only the owner's course), and every file is
  validated against the course format before it is saved.

## Accepted risks

- **Superusers act on any account** (#220). The only action so far,
  *Reset email verification*, unconfirms an account and e-mails it a new
  link. Until the owner clicks it, the account cannot add a phone number or
  turn on e-mail as its second step (#431), and courses shared to its
  address do not open for it; sign-in, including an e-mail second step
  already on, keeps working. So the harm is an unwanted e-mail and a
  temporary loss of those features. The flag is set only by hand (or on the
  dev user in dev mode), every action is logged with both user ids, and
  anyone else's POST is a 404. Actions added there later need the same
  review.

- **DNS rebinding** on custom base URLs: the host is checked when the form is
  submitted and resolved again by the HTTP client when it connects, so a DNS
  server that changes its answer in between could still reach an internal
  address. Closing it needs a resolver hook in ureq (its API for that is
  unstable).
- **CSP allows inline scripts** (`'unsafe-inline'`): the templates still have
  three inline `<script>` blocks and a few `onsubmit`/`onchange` attributes.
  The policy still blocks scripts from other origins, framing, plugins and
  `<base>` tricks; XSS is prevented at the sinks (above). Moving the inline
  code into `static/` would allow `script-src 'self'`.
- **`Secure` cookies and HSTS are opt-in in Skrongle mini**
  (`SKRONGLE_SECURE_COOKIES=1`), because a Pi on a home network is usually
  reached over plain HTTP. The startup warning says when to turn it on. The
  hosted site refuses to start without it (#980).
- **Rate limits live in memory**: they reset on restart and are per process.
  Behind a proxy that does not set `X-Forwarded-For` every client shares one
  address bucket (the per-account limit still applies).
- **Proxy sign-in trusts the proxy completely** (`SKRONGLE_PROXY_AUTH_*`,
  #642, off unless both are set): a request whose TCP peer is in
  `SKRONGLE_PROXY_AUTH_FROM` is signed in as whatever e-mail address its
  header names, with no password, two-factor step or rate limit, and in
  `open` mode an unknown address gets a new account. The peer is the
  socket's own address, never `X-Forwarded-For`; the header from anywhere
  else is ignored and logged. So anything that can connect from a trusted
  address (every local program when it is `127.0.0.1`) can sign in as
  anyone, and a proxy that passes on a client's own copy of the header lets
  the client choose. Self-hosting says to bind to loopback and have the
  proxy overwrite the header; a range wider than /8 is warned about at
  startup. `/dev/*` is never signed in this way. It is a Skrongle mini
  feature: the hosted server refuses to start with either setting (#980).
- **Root on the server can read saved API keys** (the database and the key
  file are on the same box), as `src/secrets.rs` says.
- **Dev mode** (`SKRONGLE_DEV=1`) creates a `dev@example.com` / `dev-password` account and
  must never run against a real database. Besides the loopback-bind,
  secure-cookie and `SKRONGLE_PUBLIC_URL` guards above, it is now refused on a
  database that holds other accounts unless `SKRONGLE_DEV_FORCE=1`, and in dev
  mode every route answers 421 to a `Host` that is not this machine (#388).
  That makes it hard to switch on by accident in a deploy, not impossible:
  `SKRONGLE_DEV_FORCE=1` copied into a deploy's environment would do it.
- **CI runners** (#356, open): pull-request jobs run as root on the
  self-hosted runners beside a repository-admin token, on persistent shared
  volumes. While the repository is private only the owner opens pull
  requests; it must be fixed before the repository goes public, and is
  blocked until then.
- **Registration is open** to anyone who can reach the server (documented in
  self-hosting); there is no CAPTCHA. E-mail addresses are verified by a
  one-time link (`src/verify.rs`: 256-bit token, only its SHA-256 stored,
  24 hours, single use, voided by a newer link or an address change; resend
  limited to 5 per account per hour, and at most 10 per address per day
  across account deletion, #395). Only a few things need a confirmed
  address: adding a phone number (#431), e-mail
  as the second sign-in step, and opening a course shared to that address.
  Otherwise an unconfirmed account can do everything a confirmed one can.
  Registration still says when an address already has an account, and there
  is no password reset, so someone who registers a victim's address first
  keeps it: #379, open (PR #461, above).
- **The verification link's host** comes from `SKRONGLE_PUBLIC_URL`, or, when
  that is unset, from the request's `Host` (or `X-Forwarded-Host`). Without
  `SKRONGLE_PUBLIC_URL` someone registering with another person's address
  through a forged `Host` header can make the link in that person's mail
  point at their own site; if the person clicks it, the token leaks and the
  address can be confirmed on an account they don't own. Set
  `SKRONGLE_PUBLIC_URL` on a public deploy (Stripe needs it
  anyway).
- **Course and AI quotas are per account** (#366, #367): 50 courses, 8 MiB
  per course, 32 MiB per account, 10 AI generations an hour and 40 a day.
  Registration is open, so many accounts can still add up; the server-wide
  AI slots and the hourly mail ceiling bound the expensive parts, not the
  disk.

## Git history scan

`gitleaks detect` over the full history (121 non-merge commits) on
2026-10-17 reported four "generic API key" matches, all false positives: dummy
values in a test (`debug_output_never_shows_secrets`), a sentence in this
repository's docs, and a constant name in `static/dates.js`. No API keys,
tokens, private keys, `.env` files, database files or `.key` files were ever
committed; the only env files are the `*.example` templates, whose secret
variables are empty.

Personal data a fresh public history would want to drop or replace:

- **Commit metadata**: the author's personal e-mail address and a
  machine-local address (`<user>@<laptop>.local`) on every commit. A fresh
  history (or a mailmap plus rewrite) should use a noreply address.
- **Infrastructure names in old commits**: the Pi's public DuckDNS host name
  and its LAN address were in `deploy/` and `scripts/` (`deploy.env.example`,
  the nginx snippets, the systemd unit, `deploy-env.sh`, `release.sh`). They
  are not secrets, but they point at the owner's home server. HEAD has
  placeholders since #390 (PR #439); the history still has them.

Rewriting history is the owner's decision (TODO.md); this review did not.

## Not done here

- Moving inline scripts and handlers into `static/` so the CSP can drop
  `'unsafe-inline'`.
- A resolver-level SSRF check (DNS rebinding, above).
- An automated browser test for the Sprint label escaping.
- Registration that does not reveal an address, and a password reset
  (#379, PR #461 open).
- CI runner isolation (#356).
- The language picker's `back` check (`POST /lang`, above) should use
  `terms::is_app_path`.
