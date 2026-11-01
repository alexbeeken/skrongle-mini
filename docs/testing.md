# Testing locally

How a person or an AI agent drives and checks the app on their own machine:
one command to a logged-in session, a simulated AI provider whose replies
come from canned files (or that you answer by hand), and markup with stable hooks.

**Everything here is local only.** It needs `SKRONGLE_DEV=1`, which
`./.dev-start` sets and `deploy/` never does, and the server refuses it
(logging why) unless it binds a loopback address, `SKRONGLE_SECURE_COOKIES`
is not `1` and `SKRONGLE_PUBLIC_URL` is unset (or local). It also refuses it
when the database holds any account besides `dev@example.com`, since that
looks like a deploy's database, unless `SKRONGLE_DEV_FORCE=1` says it is a
development one; `./.dev-start` sets that, and `scripts/e2e.sh`,
`scripts/smoke.sh` and the playthroughs start from a fresh database. In dev
mode **every** route answers only requests made to `localhost`,
`127.0.0.1` or `[::1]` (any port) and gives anything else `421 Misdirected
Request`, so a DNS-rebinding page cannot reach the dev server under its own
name; open the dev server by one of those names, as the scripts do. On top
of that the `/dev/*` routes refuse (404) requests with proxy headers
(`X-Forwarded-For`, `X-Real-IP`, `Forwarded`). Without dev mode the `/dev/*` routes are 404,
`SKRONGLE_LLM_MODE` is ignored and model calls go to the real providers; tests check all of this.

## One command to a logged-in session

```sh
./.dev-start --reset --seed --background   # prints the site, inbox, login and PR (below)
open "$(cat .dev-server.url)/dev/login"                # logged in as dev, Spanish open
./.dev-start --stop
```

Once the server answers, `./.dev-start` prints only this:

```
site:  http://127.0.0.1:8214
inbox: http://127.0.0.1:8214/dev/inbox
login: dev@example.com / dev-password   (or http://127.0.0.1:8214/dev/login)
pr:    https://github.com/alexbeeken/pooki/pull/264   ([263] Profile picture: no own-colour swatch)
```

`pr` is the branch's open pull request, so a reviewer can start on its QA steps; `gh` looks it up
during the build, and the line is left out without `gh`, a login, the network or an open PR.
`inbox` is the dev mailbox page, e-mail, texts and the [dev authenticator](#two-factor-codes-the-dev-authenticator); it is left out when [the dev mailbox](#e-mail-the-dev-mailbox) and [the dev phone](#text-messages-the-dev-phone) are off, `login` when dev mode is
(`--lan`, which prints `phone:` with the LAN address instead). The build's output shows only if it
fails, and the server log goes to `dev-server.log`, in the foreground too (Ctrl-C stops it); a server
that exits at boot prints the log's last lines. `DEV_VERBOSE=1 ./.dev-start …` shows everything: the
build, the settings, and in the foreground or with `--watch` the server log in the terminal.

| Flag | Does |
|---|---|
| `--reset` | Delete `./dev.db`, its key file and the held LLM requests first |
| `--seed` | Create the dev user with Spanish open, two lessons passed and one seen (`SKRONGLE_DEV_SEED=1`), as a developer (`users.developer`), with [the dev AI provider's](#the-dev-ai-provider) keys for Anthropic, OpenAI and Gemini (not with `--llm live`) |
| `--seed-users N` | Also [a crowd](#a-crowd-of-learners-with-history) of N made-up learners with weeks of history (`SKRONGLE_DEV_SEED_USERS=N`); `--seed-days D` (`SKRONGLE_DEV_SEED_DAYS`, default 90) is how far back it goes |
| `--port N` | Listen on a fixed port. Without it (or with `--port auto`) the server takes a random free port in 8100-8999, so two worktrees can run at once |
| `--llm capture` / `--llm live` | Model calls go to [the dev AI provider](#the-dev-ai-provider) (below) without `--llm`; `capture` starts it with Manual replies on (each request waits for your answer at the bottom of `/dev/ai`); `live` sends them to the real providers instead |
| `--test-course` | Also offer the [test course](#the-test-course) (`TEST_COURSE=1`) |
| `--mini` | The [mini shape](#the-mini-lane), what a self-hoster runs: no dev mailbox, no dev phone, no dev Stripe, no Skrongle AI (`SKRONGLE_MINI=1`, which turns the stand-ins off, and `SKRONGLE_AI_KEY=`). Mail and texts are only logged, in `dev-server.log`; learners' own AI keys still reach the dev AI provider |
| `--background` | Build, start detached, wait for `/healthz`, print the links. Pid in `.dev-server.pid`, URL in `.dev-server.url`, log in `dev-server.log` |
| `--watch` | Rebuild and restart on every change, keeping the port and `dev.db`; open pages reload themselves ([below](#live-reload---watch)). With `--background` the watcher runs detached and `--stop` ends it |
| `--stop` | Stop the background server (or watcher) |
| `--lan` | Bind `0.0.0.0` for phones; this turns dev mode **off** |

`GET /healthz` answers `ok` with no login; wait on it instead of sleeping.

### A crowd of learners with history

Some features only show with many learners behaving over time: the Social
motivator's comparisons ("571 people studied this course this week"), the
streak and daily goal over weeks, mastery decaying, a long History page,
the retention jobs, and whether the queries behind them hold up with a
thousand accounts in the tables (#583). `--seed-users` makes that crowd:

```sh
./.dev-start --reset --seed --seed-users 1000 --background   # 1000 learners, 90 days of history, about 10 s
./.dev-start --reset --seed --seed-users 200 --seed-days 14   # a smaller, shorter one
```

The server binds its port first, then writes the crowd, then answers.
`--background` asks `/healthz` with a one-second limit per try and waits
20 s by the clock, longer while `dev-server.log` shows the crowd growing
(`dev crowd: 450/1000 learners`; the crowd writes one at least every 5 s,
#936). If the wait runs out, it stops the server
it started, so no server keeps running without a pid file (#679, #869).
`scripts/dev-start-orphan-check.sh` checks that; run it after changing
`.dev-start`. With `SKRONGLE_SQL_TRACE` on, 1000 learners take a few minutes
and write a trace of several hundred MB.

Each learner (`learner-<n>@seed.example`, password `dev-password`, so you
can sign in as any of them) is a small model drawn from their number: when
they joined, how keen they are (the chance of studying on a given day: a
casual, regular or daily learner), how good (the chance of a right answer),
the mode they prefer (Sprint, Study or the Guide), their time zone and
usual hour, and whether they drifted away (a third do). A day they study
is a session: open the lesson they are on, read, play or run through it,
answer its questions, now and then a round of Word Match or a review of an
earlier lesson. Eight in ten learn Spanish, the rest French.

The rows are written through the same functions the modes call
(`progress`, `mastery`, `activity`, `games`), only at a time in the past,
so they obey every rule the real ones do: a pass is seen plus a counted
run of 70 %, mastery decays between sessions, effort points weigh as the
daily goal says. Nothing lands in the future; nothing before the account
was made. The crowd is deterministic: learner 7 always gets the same life,
a second boot adds nothing, and `--seed-users 1200` later only adds the
missing 200. It does not make the database look like a deploy's (#388):
the dev-mode account check ignores the crowd's domain. `--reset` removes
it with everything else. `src/dev_crowd.rs` is the model.

### Live reload (`--watch`)

```sh
./.dev-start --reset --seed --watch                # foreground: build log and server log in this terminal
./.dev-start --reset --seed --watch --background   # detached, log in dev-server.log, ./.dev-start --stop ends it
```

`.dev-start` polls `src/ templates/ static/ data/ docs/ Cargo.toml Cargo.lock`
once a second. On a change it runs `cargo build`; if that fails it prints
the error and **keeps serving the previous build**; if it succeeds it stops
the server, starts the new binary on the same port with the same `dev.db`
(the session cookie stays valid, so a logged-in tab stays logged in), and
waits on `/healthz`. `--seed` seeds only on the first start. A typical round
after saving a template:

```
==> changed: templates/lessons.html
   Compiling spanish-learning v0.1.0 (…)
    Finished `dev` profile [optimized + debuginfo] target(s) in 3.97s
==> restarting on http://127.0.0.1:8316
… INFO shut down cleanly
… INFO listening on http://127.0.0.1:8316/
==> healthy: http://127.0.0.1:8316/healthz answered after 1 s (server pid 93837)
```

Why everything restarts: askama templates and `docs/course-format.md` (the
AI prompt) are compiled in, and `data/` (courses<!-- themes -->, themes<!-- /themes -->) is read at boot.
`static/` and the `/docs` pages are embedded with rust-embed, which in a
debug build reads them from disk on each request, so they are live even
without `--watch`; the watcher restarts for them anyway (the build is a
no-op) so open pages reload.

Browser reload: `--watch` sets `SKRONGLE_DEV_RELOAD=1`, and in dev mode the
server then appends a small script to every HTML response that polls
`GET /dev/boot` (this process's boot id) every second and reloads the page
when the id changes. Polls are not logged; nothing is injected without
`--watch` (so `scripts/e2e.sh` and `smoke.sh` never see it) or outside dev
mode. A half-filled form is lost on the reload, as with any live reload.

**Tools considered** (issue #179). None of them is installed by default
here, and none of them is needed:

| Tool | What it would give | Why not |
|---|---|---|
| `cargo watch` (`cargo install cargo-watch`) | `cargo watch -x run` restarts on change; `.dev-start --watch` used it before | Its maintainer has stopped development and points to bacon and watchexec. It kills the server before building, so a broken build leaves no server; no `/healthz` wait, no detached mode |
| `bacon` (`cargo install bacon`) | A TUI that reruns check/clippy/test; a `bacon.toml` job with `on_change_strategy = "kill_then_restart"` can run the server | Built for reading compiler errors in a terminal, not for a detached server an agent drives; one more config file |
| `watchexec` (`brew install watchexec`) | Maintained, event-based (no polling), debounced: `watchexec -r -w src … -- cargo run` | Same kill-then-build problem as cargo-watch unless wrapped in a script, and once wrapped the script does all the work a poll loop does |
| `systemfd` + `listenfd` | systemfd holds the socket and hands it to each new server, so requests during a restart wait instead of failing | Needs the `listenfd` crate in `main.rs` and a second install, to cover a gap of under a second: the old server keeps serving during the build and the new one answers `/healthz` within about 0.3 s |
| `fswatch` / `entr` (`brew install …`) | Event-based change notification | Another install for ~260 files a `find -newer` covers in milliseconds; `find` is everywhere, the Pi included |

So `--watch` is a plain polling loop in `.dev-start` plus the reload script
in `src/dev.rs` (`inject_reload`, `/dev/boot`): no new dependency, no
install step.

## Dev routes

The dev mailbox (`/dev/inbox`, `/dev/mail`, `/dev/phone`, `/dev/sms`,
`/dev/authenticator`, `/dev/ai`, `/dev/payments` and every route under them, JSON and
actions too) needs no sign-in: it is for whoever is testing and holds
every account's mail, texts and authenticator entries. Like every dev
route it exists in dev mode only, for requests made on this machine
(`dev::local_only`). `e2e/lib/mail.ts` reads it with a request context of
its own, so the learner a spec drives is not touched.

| Route | Does |
|---|---|
| `GET /dev/login` | Log in as `dev` (`dev@example.com`, password `dev-password`), a superuser (the Superuser page, `/superuser`) and a developer (`users.developer`). Every dev-mode boot creates the account if it is missing, so the login form takes those credentials too, and `/login` and `/register` show a **Log in as dev** button. `?open=spanish` opens a course (`?open` alone: Spanish) and counts it as set up, so its first-visit settings wizard does not open (add `&wizard=1` to see the wizard on the course home again), `?keys=dev` (or the older `?keys=fake`) stores [the dev AI provider's](#the-dev-ai-provider) learner keys for Anthropic, OpenAI and Google, so the AI forms are unlocked and offer each provider, `?next=/courses` says where to land, `?premium=1` grants the dev user Plus (`0` takes the grant back), `?plan=basic|plus` puts the dev user on that plan through [the dev payment provider](#payments-the-dev-payment-provider) (a Checkout paid at once, signed webhooks and all; `free` cancels it; it needs the stand-in, so not with real `SKRONGLE_STRIPE_*` settings), `?terms=old` leaves the dev user with only an older Terms version accepted so the "terms updated" gate shows (the next login without it accepts the current version again; the dev user always has). It skips [two-factor sign-in](features#two-factor-sign-in); `/login` does not |
| `GET /login/code/device` | Dev mode only: the dev device the pending sign-in's code went to, for the frame at the bottom of `/login/code` (see [the dev device](#two-factor-sign-in-the-dev-device)); needs the `twofa` cookie, not a session. `?format=json` for the rows |
| `GET /dev/reset` | Wipe the dev user's progress, projects, own courses (drafts and generated)<!-- themes -->, made themes<!-- /themes --> and saved app language, keeping the account and keys. `?next=` as above |
| `GET /dev/mail` | The configured mail providers and the newest 50 outbox rows (`to`, `subject`, `text`, `html`, `purpose`, `state`, `provider`, …). Without `SKRONGLE_MAIL_PROVIDERS` they go to [the dev mailbox](#e-mail-the-dev-mailbox) (provider `dev-smtp`); in dev mode bodies are kept, so a test can read a link out of a message. JSON, except for a browser (`Accept: text/html`, or `?format=html`): the dev mailbox page with its **Outbox** tab showing (see `GET /dev/inbox`): a row per message (`dev-mail-message[data-purpose][data-to][data-state]`) with its state, **Open the copy in the inbox** (`dev-mail-open`) once the mailbox has it, and a "Send a test message" form (`dev-mail-to`, `dev-mail-send`). `?format=json` forces JSON |
| `POST /dev/mail/send` | Form `to` (default: the signed-in user's address): queue a test message (purpose `dev-test`) through the configured providers, then back to `/dev/mail` (204 for the page's own `X-Dev-Box: 1` posts). For trying a provider: the form on the Outbox tab, or `curl -X POST <url>/dev/mail/send -d to=you@example.org` |
| `GET /dev/mail/{id}` | One outbox row: state, provider, attempts, last error (`dev-mail-error`), its links (`dev-mail-link`), the HTML part framed (`dev-mail-html`), the text (`dev-mail-text`), **Open in the dev inbox** (`dev-mail-open`). Opening it marks it read. `?format=json`: the row. 404 once deleted |
| `POST /dev/mail/{id}` | Form `do=archive`, `unarchive` or `delete`, as on the inbox; Delete removes the outbox row (a queued message is then never sent) |
| `GET /dev/jobs` | The newest 100 rows of the background `jobs` table (`src/jobs.rs`), newest first: `{"jobs": [{id, kind, payload, state, priority, run_at, attempts, max_attempts, locked_by, locked_until, last_error, created_at, finished_at}]}`. For a browser (`Accept: text/html`, or `?format=html`) a page (`dev-jobs`) with a row per job (`dev-jobs-row[data-id][data-kind][data-state][data-run-at][data-attempts]`, `dev-jobs-state`, `dev-jobs-attempts`, `dev-jobs-error`), **Run now** (`dev-jobs-run-now`) on queued and running rows, **Queue a failing job** (`dev-jobs-queue-fail`) and an (i) (`dev-jobs-info`). No sign-in, like the dev mailbox. `?format=json` forces JSON |
| `POST /dev/jobs/fail` | Queue a `dev.fail` job due now (it always fails, so its attempts climb through `mail::RETRY_DELAYS` to `failed`), then back to `/dev/jobs` |
| `POST /dev/jobs/{id}` | Form `do=run_now`: a queued row is due now; a running row's lease ends now, as if its server had stopped. The runner takes it within a moment. 404 for a row that is done, failed or gone |
| `GET /dev/inbox` | What [the dev mailbox](#e-mail-the-dev-mailbox) received, newest first; `?to=` for one address (any case, or part of one). JSON `{"smtp": "127.0.0.1:<port>", "messages": [{id, outbox_id, envelope_from, envelope_to, from, to, subject, date, message_id, text, html, links}]}` (`smtp` is `null` when the mailbox is off), or for a browser the dev mailbox page: **Inbox** and **Outbox** tabs (`dev-box-tab-in`, `dev-box-tab-out`, each with its unread count) over one table each, like a simple mail client's. A row (`dev-inbox-item[data-id][data-to][data-read][data-archived]`, subject link `dev-inbox-open`) is bold until opened, and has **Archive** (`dev-inbox-archive`: struck out and moved to the bottom; `dev-inbox-unarchive` moves it back) and **Delete** (`dev-inbox-delete`, for good). Also an address filter (`dev-inbox-to`), **Empty the inbox** (`dev-inbox-clear`), and an (i) (`dev-box-info`) with the SMTP address (`dev-inbox-smtp[data-addr]`), or a red ! (`dev-box-off`) when the mailbox is off. The page polls `GET /dev/inbox/rows` every 3 s and its buttons post in place, so nothing needs a reload. Opened and archived are kept in memory |
| `GET /dev/inbox/rows` | `?kind=mail` (default) or `sms`, `?to=` as above: that section's two lists as HTML plus the unread counts, `{"in": {"html", "unread"}, "out": {…}}`, for the page's poll (`static/devbox.js`) |
| `GET /dev/inbox/latest` | The newest message to `?to=` (default: the signed-in user's address); the list for that address when there is none. |
| `GET /dev/inbox/{id}` | One message: headers (`dev-inbox-subject`, `dev-inbox-to-header`), its links (`dev-inbox-link`), the HTML part in a sandboxed frame (`dev-inbox-html`) whose links open in the tab, the text (`dev-inbox-text`), the source, and Archive / Delete. Opening it in a browser marks it read. `?format=json` for the message alone (does not mark it). 404 for a message it no longer has |
| `POST /dev/inbox/{id}` | Form `do=archive`, `unarchive` or `delete`: 204 for the page's own posts (`X-Dev-Box: 1`), else back to `/dev/inbox`. 404 for a message it no longer has |
| `GET /dev/inbox/{id}/raw` | The message as received (`text/plain`), to save as `.eml` |
| `POST /dev/inbox/clear` | Empty the mailbox, then back to `/dev/inbox` |
| `GET /dev/sms` | JSON: the configured SMS providers and the newest 50 text message outbox rows (`to`, `text`, `purpose`, `state`, `attempts`, `provider`, `last_error`, …). Without `SKRONGLE_SMS_PROVIDERS` dev mode sends them to [the dev phone](#text-messages-the-dev-phone) (provider `dev-phone`); in dev mode bodies are kept, so a test can read a code out of a text. For a browser (`Accept: text/html`): the dev mailbox page's **Text** section on its **Outbox** tab: a row per text (`dev-sms-message[data-to][data-state][data-purpose]`, `dev-sms-view`, `dev-sms-open` for the phone's copy, `dev-sms-archive`, `dev-sms-delete`) and "Send a test text" (`dev-sms-to`, `dev-sms-text`, `dev-sms-send`; filled in with `+447700900123` and `test123`, so a quick test is one click). `?format=json` forces JSON |
| `POST /dev/sms` | JSON `{"to": "+447700900123", "text": "…"}`: queue a text through `sms::Texter::send_sms` (purpose `dev-test`, for the signed-in user if any), to try a provider configuration or the limits by hand. Answers `{"id": n}`, 400 `{"error": …}` for a bad number or text, or 429 `{"error": …, "retry_after": seconds}` over a limit. The limits are not lifted in dev mode: use other numbers, delete rows on the Text outbox, or `./.dev-start --reset` |
| `POST /dev/sms/send` | Form `to`, `text` (empty: a stock test text): the Text outbox's form, through `send_sms` as above; back to `/dev/sms` (204 for the page's own posts), 400 or 429 with why as text |
| `GET /dev/sms/{id}` | One text outbox row in the outbox's conversation with that number: every row to it as sent bubbles (right, blue; `dev-text-bubble[data-state]`, `dev-text-body`) and the phone's replies on the left, the opened one ringed, under the number as the contact (`dev-text-phone[data-side=out]`, `dev-text-to`); its state, provider, attempts (`dev-text-state`) and last error below; **On the phone** (`dev-text-other`). A compose bar at the bottom (`dev-text-compose`, `dev-text-compose-send`) sends a text to that number through `POST /dev/sms/send` without leaving; the thread re-reads itself every 3 s and after a send, so the bubble appears when the text goes through (`dev-text-compose-error` shows a 400/429). Opening it marks it read. `?format=json`: the row |
| `POST /dev/sms/{id}` | Form `do=archive`, `unarchive` or `delete`, as on the e-mail outbox. The outbox tab lists one row per number, like the phone's; Delete removes every row to that number, which also frees them from the per-number limit |
| `GET /dev/phone` | What [the dev phone](#text-messages-the-dev-phone) received, newest first; `?to=` for numbers containing those digits. JSON `{"phone": true, "messages": [{id, outbox_id, to, text, purpose, received_at}]}` (`phone` is `false` when it is off), or for a browser the dev mailbox page's **Text** section (`dev-box-kind-sms`; `dev-box-kind-mail` goes back to e-mail) on its **Inbox** tab, styled like a messaging app: a row per conversation, showing that number's newest text, so another text to the same number joins its row instead of adding one (`dev-phone-item[data-id][data-to][data-read][data-archived]`, `dev-phone-open`, `dev-phone-archive`/`dev-phone-unarchive`, `dev-phone-delete`), bold until opened, **Empty the phone** (`dev-phone-clear`), and a red ! (`dev-box-off`, modal `dev-phone-off`) when it is off. The page polls `GET /dev/inbox/rows?kind=sms` |
| `GET /dev/phone/{id}` | One text in its conversation with that number, as that phone shows it: the app as the contact, the texts it received on the left and its replies on the right in blue (`dev-text-phone[data-side=in]`, `dev-text-bubble[data-from=app\|phone]`, `dev-text-body`), the number (`dev-text-to`), **In the outbox** (`dev-text-other`: swaps to the outbox side in place, with a view transition, and pushes its URL; back/forward switch sides again), Archive / Delete, and a compose bar (`dev-text-compose`, `dev-text-compose-send`) that replies as the number through `POST /dev/phone/reply` |
| `POST /dev/phone/reply` | Form `to`, `text`: a reply typed on a text's page, the number texting the app. It joins the conversation on both sides (in the phone's JSON with `from_phone: true`, `purpose` `reply`) but is not an inbox row; nothing in the app reads it. Back to `/dev/phone` (204 for the page's own posts); 400 for an empty text Opening it marks it read. `?format=json`: the text |
| `POST /dev/phone/{id}` | Form `do=archive`, `unarchive` or `delete`, as on the e-mail inbox; Delete removes the whole conversation with that number, replies too |
| `POST /dev/phone/clear` | Empty the dev phone, then back to `/dev/phone` |
| `GET /dev/ai` | [The dev AI provider](#the-dev-ai-provider)'s page, the dev mailbox's **AI** section (`dev-box-kind-ai`): each key profile's card (`dev-ai-profile[data-who=platform\|learner][data-over]`: its keys as copy buttons in the provider's colours `dev-ai-key[data-provider][data-copy-key]`, usage `dev-ai-usage`, limit `dev-ai-limit` + `dev-ai-measure` + `dev-ai-limit-save`, `dev-ai-reset`, **Hit the limit now** `dev-ai-exhaust` / `dev-ai-lift`, failure `dev-ai-fault` + `dev-ai-hang` + `dev-ai-once`/`dev-ai-sticky` + `dev-ai-inject`, `dev-ai-injected`, `dev-ai-clear-inject`), the call log (`dev-ai-call[data-id][data-who][data-format][data-purpose][data-status][data-fixture]`, `dev-ai-call-open`, `dev-ai-call-status`, `dev-ai-call-outcome`; polls `GET /dev/ai/rows` every 2 s), the fixture files (`dev-ai-fixture[data-name][data-edited]`) and **Add a scenario** (`dev-ai-scenario-*`). `?format=json`: `{mode, simulating, holding, base_urls, client_timeout_secs, profiles, calls, fixtures, held}` (`held`: the held requests, newest first, each `{stem, pending, request, response}`) |
| `GET /dev/ai/rows` | The log as HTML, each profile's usage, and whether Manual replies is on with the held requests as HTML, for the page's poll |
| `POST /dev/ai/hold` | Form `do=on\|off`: Manual replies (the pill, `dev-ai-hold-toggle`, `role=switch`; `dev-ai-hold-form[data-holding]`, `dev-ai-hold-state`). On, the **Manual replies** section at the bottom of the page (`dev-ai-held`; `dev-ai-held-link` says how many wait) lists each held request, newest first, as a folded row in a scrolling box (`dev-ai-request[data-stem][data-purpose][data-pending]`, `dev-ai-request-state`; empty: `dev-ai-held-empty`): the system prompt and messages, and either the reply (`dev-ai-reply`) or a folded Reply box (`dev-ai-answer`: `dev-ai-answer-form`, `dev-ai-answer-input`, `dev-ai-answer-submit`, `dev-ai-answer-error`) |
| `POST /dev/ai/held/{stem}` | Answer a held request (`reply=` form field); the page's Reply boxes post it in place |
| `POST /dev/ai/profiles/{platform\|learner}` | Form `do=settings` (`limit`, empty for none; `measure=requests\|tokens\|dollars`), `reset` (counters to zero, "limit reached" off), `exhaust` (over the limit from now on), `lift`, `inject` (`fault=timeout\|server-error\|overloaded\|invalid-key\|malformed\|wrong-shape`, `hang` seconds, `mode=once\|sticky`) or `clear-inject`; back to `/dev/ai` with a notice |
| `POST /dev/ai/clear` | Empty the call log |
| `GET /dev/ai/calls/{id}` | One call: key profile, format, model, outcome, fixture, tokens, cost, and the request and reply bodies (`dev-ai-call-request`, `dev-ai-call-reply`). `?format=json` |
| `GET /dev/ai/fixtures/{name}` | One fixture file to edit (`dev-ai-fixture-text`): **Use for this run** (`dev-ai-fixture-apply`, in memory), **Save to file** (`dev-ai-fixture-save`, writes `data/test-fixtures/llm/<name>.json`), **Reset** (`dev-ai-fixture-reset`, back to the file). `?format=json` |
| `POST /dev/ai/fixtures/{name}` | Form `do=apply` (with `text`), `save` or `reset` |
| `POST /dev/ai/fixtures` | Form `purpose` (the file), `scenario` (the key), `reply` (JSON or text) or `error`, `delay_ms`: add a named scenario in memory |
| `POST /dev/ai/anthropic/v1/messages`, `GET …/v1/models` | The Anthropic Messages API, played by the dev AI provider (no cookie; the `x-api-key` decides) |
| `POST /dev/ai/openai/v1/chat/completions`, `GET …/v1/models` | OpenAI chat completions (`Authorization: Bearer …`) |
| `POST /dev/ai/google/v1beta/openai/chat/completions`, `GET …/models` | Gemini's OpenAI-compatible endpoint, OpenAI's format |
| `GET /dev/payments` | [The dev payment provider](#payments-the-dev-payment-provider)'s page, the dev mailbox's **Payments** section (`dev-box-kind-payments`; `dev-payments[data-on]`, an (i) `dev-payments-info` with the API base, webhook URL and keys, or a red ! `dev-payments-off` when subscriptions are off). **Subscriptions** (`dev-payments-sub[data-id][data-status][data-user][data-fail-next]`, `dev-payments-sub-status`): **Renew now** (`dev-payments-renew`), **Fail the next payment** (`dev-payments-fail-next`; `dev-payments-pay-next` takes it back), **Cancel at period end** (`dev-payments-cancel-at-end` / `dev-payments-resume`), **Cancel now** (`dev-payments-cancel`). **Payments** (invoices and one-off payments, `dev-payments-payment[data-id][data-status]`): **Mark paid** (`dev-payments-mark-paid`, a failed one), **Refund** (`dev-payments-refund`). **Checkout sessions** (`dev-payments-session[data-id][data-status][data-mode]`, an open one links to its page: `dev-payments-session-open`), **Customers** (`dev-payments-customer`). The **Log** (`dev-payments-entry[data-kind=api\|webhook\|page][data-what][data-status][data-event][data-tampered]`, `dev-payments-entry-open`, `dev-payments-entry-status`, **Send again** `dev-payments-resend` on a webhook row), **Empty the log** (`dev-payments-clear`), **Forget everything** (`dev-payments-reset`). Polls `GET /dev/payments/rows` every 2 s; the row actions post in place. `?format=json`: `{mode, on, api_base, customers, sessions, subscriptions, payments, events, log}` |
| `GET /dev/payments/rows` | The four lists (`state`) and the log (`log`) as HTML, for the page's poll |
| `GET /dev/payments/log/{id}` | One log entry: the request and reply (`dev-payments-entry-request`, `dev-payments-entry-response`, `dev-payments-entry-result`); a webhook's has **Send again** (`dev-payments-entry-resend`) and **Send with a broken signature** (`dev-payments-entry-tamper`) |
| `POST /dev/payments/subscriptions/{id}` | Form `do=renew\|fail-next\|pay-next\|cancel\|cancel-at-end\|resume`; the events go to the app's webhook before the answer (204 for the page's posts, else back to `/dev/payments` with a notice) |
| `POST /dev/payments/payments/{id}` | Form `do=mark-paid` or `refund` (`amount` in cents for part of it) |
| `POST /dev/payments/events/{id}` | Form `do=resend` (the event as it was) or `tamper` (a broken `Stripe-Signature`: the app must answer 400) |
| `POST /dev/payments/clear`, `POST /dev/payments/reset` | Empty the log; forget every customer, session, subscription, payment and event |
| `GET`/`POST /dev/payments/checkout/{id}` | The Checkout page Stripe would host (`dev-checkout[data-id][data-mode][data-status][data-declined]`, `dev-checkout-product`, `dev-checkout-amount`, `dev-checkout-email`): **Pay** (`dev-checkout-pay`: sends the webhooks, then to the session's `success_url`), **Fail** (`dev-checkout-fail`: the card is declined, `dev-checkout-declined`, the session stays open), **Cancel** (`dev-checkout-cancel`: to its `cancel_url`); form `do=pay\|fail\|cancel`. A paid session shows `dev-checkout-closed` |
| `GET`/`POST /dev/payments/portal/{id}` | The Billing Portal (`dev-portal[data-customer]`, `dev-portal-sub[data-id][data-status]`): **Update card and pay** (`dev-portal-update-card`, a past-due one), **Cancel subscription** (`dev-portal-cancel`, at the period end) / **Renew subscription** (`dev-portal-resume`), **Cancel now** (`dev-portal-cancel-now`), **Return** (`dev-portal-return`, to the app's `return_url`); form `do=…`, `subscription` |
| any `/dev/payments/stripe/v1/…` | Stripe's API, played by the dev payment provider (no cookie; `Authorization: Bearer sk_test_…` decides). See the table [below](#payments-the-dev-payment-provider) |
| `GET /dev/authenticator` | [The dev authenticator](#two-factor-codes-the-dev-authenticator)'s accounts with their current codes. JSON `{"now", "accounts": [{id, issuer, account, code, remaining, period, digits, algorithm, secret}]}`, or for a browser the dev mailbox page's **Authenticator** section (`dev-box-kind-auth`): an add form (`dev-authenticator-uri`, `dev-authenticator-add`, `dev-authenticator-error`) and a row per account (`dev-authenticator-item[data-id][data-issuer][data-account][data-code][data-expires]`) with the code (`dev-authenticator-code`, `123 456`), the seconds left (`dev-authenticator-remaining`), **Copy** (`dev-authenticator-copy`) and **Remove** (`dev-authenticator-remove`). `?uri=otpauth://totp/…` is the "scan": the form opens filled in with it; without it the form holds an example account. The page counts down and polls `GET /dev/authenticator/rows` (`{"html", "now"}`) |
| `POST /dev/authenticator` | Form `uri`: an `otpauth://totp/…` link or a base32 setup key; adds the account (the same secret and name again replaces it), back to `/dev/authenticator` (204 for the page's own posts, `{"id"}` with `Accept: application/json`), 400 with why |
| `POST /dev/authenticator/{id}` | Form `do=delete`: remove that account; 404 if it has none |
<!-- themes -->
| `POST /dev/theme` | JSON `{"files": {"theme.json": "…", "pictures/x.svg": "…"}}`: store a made theme for the signed-in user (until the theme editor exists). Answers `{"id": "user-<n>"}`, or 400 with the validation error. `/dev/reset` deletes the dev user's made themes |
<!-- /themes -->
| `POST /dev/vocab` | JSON `{"lesson": "<slug>", "days_ago": n}`: collect that lesson's words for the signed-in learner as if `n` days ago, so they are due today (#1360). Answers `{"collected": n}`, the words that were new |
| `POST /dev/premium` | `premium=1` grants the signed-in user Plus (a grant), `0` revokes it; the profile's subscription section has a button for it in dev mode |
| `GET /dev/boot` | This process's boot id, which changes on every restart; the `--watch` reload script polls it ([Live reload](#live-reload---watch)) |

## E-mail: the dev mailbox

In dev mode the app sends its e-mail to a small SMTP server of its own
(`src/mail/sink.rs`), so a sign-up or a password flow can be clicked
through to the end without a real provider. The outbox, the job runner
that sends it and lettre run as in production; only the server at the other end
is the app's. It is on by default; `./.dev-start` prints its link (`inbox:`).

- **Open the mail:** `<url>/dev/inbox` (the Outbox tab is `/dev/mail`),
  or `<url>/dev/inbox/latest?to=<address>` for the newest message to one
  address; no sign-in needed. Rows are bold until opened; Archive strikes one out and
  moves it down, Delete removes it; the page updates by itself. A message shows its
  HTML as a mail client would; click a link in it and it opens in the tab.
- **Where it listens:** `127.0.0.1` on a free port (in the log line
  `dev: e-mail (SMTP on …) and text messages go to the dev mailbox` and in `/dev/inbox`'s
  JSON as `smtp`). Other tools on the machine may send to it too, e.g.
  `swaks --server 127.0.0.1:<port> --to ann@example.org`.
- **`SKRONGLE_DEV_SMTP`:** unset or `1`, a free port; a number, that port;
  `0`, off (mail is only logged, as outside dev mode). With
  `SKRONGLE_MAIL_PROVIDERS` set, dev mode sends through those providers
  instead, and `/dev/inbox` says it is off.
- **What it keeps:** the newest 200 messages, in memory; a restart empties
  it. It takes EHLO/HELO, MAIL, RCPT, DATA, RSET, NOOP and QUIT: no TLS,
  no AUTH, at most 2 MB a message.
- **In browser tests:** `e2e/lib/mail.ts` has `waitForMail(to, subject?)`
  (it waits for the job runner to send it), `inbox(to?)` and `mailLink(mail,
  /pattern/)`, which read the mailbox as the dev user (`devRequest()`), and
  `devPage(browser, path)`, a browser of its own signed in as the dev user,
  for clicking through a message while the learner stays signed in as
  themselves. Give each test its own address (`newAccount`), since the
  mailbox is shared. `e2e/specs/mail.spec.ts` registers, opens the
  confirmation in the inbox and clicks its link.

## Text messages: the dev phone

In dev mode without `SKRONGLE_SMS_PROVIDERS` the app's texts go to a provider
of its own, `dev-phone` (`src/sms/phone.rs`), which keeps them in memory
(the newest 200; a restart empties it). The outbox, the job runner that sends
it and the limits run as in production. It shares the dev mailbox page, so
`./.dev-start`'s `inbox:` link covers it too.

- **Read them:** `<url>/dev/phone`, the **Text** section of the dev mailbox
  page (next to **E-mail**). It works like the e-mail one (bold until
  opened, Archive, Delete, Inbox and Outbox tabs, updates by itself) but
  looks like a messaging app; a text opens as the conversation with that
  number.
- **Send one:** the Text outbox's "Send a test text", or
  `curl -X POST <url>/dev/sms/send -d to=+447700900123 -d text=Hi`.
- **The limits** still apply per number and account; delete outbox rows (or
  use another number) to send more.
- **`SKRONGLE_DEV_PHONE`:** unset or `1`, on; `0`, off (texts are only logged,
  as on a server without `SKRONGLE_SMS_PROVIDERS`; `/dev/phone`'s JSON says
  `"phone": false`). `./.dev-start --mini` and the mini lane turn it off.

## Two-factor codes: the dev authenticator

The dev mailbox page's third section, **Authenticator** (`<url>/dev/authenticator`),
is an authenticator app like Google Authenticator, so a flow that asks
for a TOTP code can be clicked through with the real protocol and no
special case in the app. It speaks RFC 6238 (`src/totp.rs`): HMAC-SHA1 by
default, SHA-256 and SHA-512 when the link asks, 6 to 8 digits, any period.

- **Add an account:** paste an `otpauth://totp/Issuer:account?secret=…`
  link or a bare base32 setup key (any case, spaces allowed) and **Add**.
  The box opens filled in with an example account (Skrongle,
  `ann@example.org`, key `JBSWY3DPEHPK3PXP`), so **Add** alone adds it;
  removing the last account fills an empty box with it again.
  To "scan" a QR code, open `/dev/authenticator?uri=<the otpauth link>`:
  the form opens filled in. `curl -X POST <url>/dev/authenticator -d uri=JBSWY3DPEHPK3PXP` works too.
- **Read a code:** each account shows its current code, worked out on the
  server from the secret, and a countdown; the next code shows by itself.
  **Copy** puts it on the clipboard; **Remove** deletes the account.
- **What it keeps:** the accounts, in memory; a restart forgets them (as a
  phone without a backup would), so enrol again after one.
- **In browser tests:** `e2e/lib/totp.ts` works codes out with Node's
  crypto (`totp(secret)`, `totpForStep`) and reads the dev authenticator
  (`authenticator()`, `authenticatorCode(account)`, read as the dev user like the mail helpers).
  `e2e/specs/authenticator.spec.ts` covers add, scan, codes, rollover and remove.
- **Two-factor sign-in** ([features](features#two-factor-sign-in)): the
  profile's set-up dialog has *Dev: add it to the dev authenticator*, the
  scan link, which opens the dev authenticator in a tab of its own.
  `/dev/login` skips the second step, so sign in through `/login` to try it.
  The phone number's check on the profile texts a code: read it on
  `/dev/phone`. `e2e/specs/two-factor.spec.ts` covers the phone check,
  e-mail, text and authenticator sign-in, reading the dev mailbox
  (`devPage`, `waitForMail`) and the sign-in codes from the dev device
  below; it runs in CI with the exact specs.

### Two-factor sign-in: the dev device

In dev mode, a small window floats at the bottom of `/login/code`
(`login-code-dev`, a `<details>` you can fold) framing
`/login/code/device` (`login-code-dev-frame`): the dev device this sign-in's
code went to, and nothing else. For a text it lists the dev phone's texts to
that number; for e-mail the dev inbox's mail to that address (the subject
carries the code); for the app the dev authenticator's entry for that
address with its code counting down. Each row (`login-code-device-item`,
`data-code`) has **Use** (`login-code-device-use`), which types the code into
the form above, so a sign-in is two clicks. The frame re-reads itself every
two seconds (`static/devbox.js`).

The route is only mounted in dev mode (with `/dev/login`, local clients
only) and is keyed by the sign-in's own `twofa` cookie: it shows the
device of the sign-in in progress, and a request without a pending sign-in
gets 404. It is the one page the app lets itself frame: it sets its own
CSP (`frame-ancestors 'self'`) and `X-Frame-Options: SAMEORIGIN`, which
`security::headers` keeps. `?format=json` gives the rows.

## The dev AI provider

Every model call goes through one function (`ai::complete`) and carries a
**purpose**: `generate-outline`, `generate-lesson` (key: the lesson slug),
`revise-plan`, `edit-plan`, `revise-lesson` (key: the lesson slug), `edit-lesson`, `add-lesson` and
`tutor-turn`. In dev mode those calls go to a stand-in for the AI providers
inside the dev server (`src/dev_ai.rs`, `src/routes/dev_ai.rs`, #297), which
**uses the real wire protocols**: the app's own clients in `ai.rs` call it
exactly as they call Anthropic, OpenAI and Gemini, with a base URL and a
key, so the application does not know it is fake (#298). It answers from
canned files, meters usage per key, plays a used-up quota, timeouts and
broken replies, and can hold a request for you to answer by hand.

```sh
./.dev-start --reset --seed --background   # then open <url>/dev/ai
```

| Format | Base URL | Endpoint |
|---|---|---|
| Anthropic | `<url>/dev/ai/anthropic` | `POST /v1/messages` (`x-api-key`, `anthropic-version` required, no streaming), `GET /v1/models` |
| OpenAI | `<url>/dev/ai/openai/v1` | `POST /chat/completions` (`Authorization: Bearer`), `GET /models` |
| Gemini (OpenAI-compatible) | `<url>/dev/ai/google/v1beta/openai` | `POST /chat/completions`, `GET /models` |

**How calls get there: the keys do the routing.** In dev mode these are
the providers' default base URLs (unless `SKRONGLE_ANTHROPIC_BASE_URL`,
`SKRONGLE_OPENAI_BASE_URL` or `SKRONGLE_GOOGLE_BASE_URL` say otherwise), and the
dev user is seeded with one simulator key per provider (`--seed`, and
`/dev/login?keys=dev` or `?keys=fake` saves them too). A key the simulator
did not issue gets the provider's own 401, as a real provider answers a bad
key. There is no keyless stand-in and no client-side switch: an account
with no key sees the greyed-out dialog and the "No AI provider key is set
up" alert, in dev mode as on a deploy. Outside dev mode, or with
`SKRONGLE_LLM_MODE=live`, the defaults are the public APIs.

| Variable | Default | Meaning |
|---|---|---|
| `SKRONGLE_LLM_MODE` | (empty) | Dev mode only. Empty (or `dev`): the simulator answers from the fixtures. `capture`: the simulator starts with [Manual replies](#manual-replies-answer-the-models-requests-yourself) on. `live`: the real providers' public APIs (and no seeded keys). The old `fixtures` means the default, with a warning |
| `SKRONGLE_LLM_FIXTURES` | `data/test-fixtures/llm` | Where the simulator reads canned replies |
| `SKRONGLE_LLM_DIR` | `./dev-llm` | Where held requests are written and their replies read |
| `SKRONGLE_LLM_TIMEOUT_SECS` | `600` | How long a held request waits for its reply before the simulator answers 504 |
| `SKRONGLE_LLM_DELAY_MS` | `0` (`500` under `./.dev-start`) | How long a canned reply takes when its fixture names no `delay_ms`: long enough that a click-through still shows the waiting states (spinners, progress modals), a tenth of what a real model takes (#313). `./.dev-start --llm-delay N` changes it (`0` turns it off, `5000` feels like a real model); `scripts/e2e.sh` and `smoke.sh` run without it. The `slow` fixture scenarios keep their own 3–4 s `delay_ms` and `timeout` still outlast the app's limits: their point is the long wait |
| `SKRONGLE_AI_TIMEOUT_SECS` | 900 / 120 | The app's client limit per request (generation / chat), for real providers too; `scripts/e2e.sh` sets 8, so a `timeout` fixture or an injected hang is over in seconds |

`SKRONGLE_OPENAI_BASE_URL` is still the way to test against a real local
model (Ollama, LM Studio) for the OpenAI provider.

**Two key profiles,** told apart by the key in the request, each with its
own counters, limit and injected failure:

| Profile | Keys | Who uses it |
|---|---|---|
| Platform key | `sk-ant-dev-platform` (Anthropic format), `sk-dev-platform` (OpenAI format) | [Skrongle AI](#skrongle-ai-the-servers-key-in-dev): `./.dev-start` and `scripts/e2e.sh` set `SKRONGLE_AI_KEY=sk-ant-dev-platform` |
| Learner's own keys | `sk-ant-dev-anthropic`, `sk-dev-openai`, `AIza-dev-google` (seeded), and any `sk-ant-dev-…`, `sk-dev-…` or `AIza-dev-…` key you type (the old `?keys=fake` key `sk-ant-dev-fake-key` among them) | The keys on a learner's profile |

Any other key is refused as the provider refuses a bad one (401,
`authentication_error` / `invalid_api_key`). The wire endpoints are not
behind the developer check, since the app's server calls them with a key,
not a cookie; like every `/dev` route they exist only in dev mode and answer
only local requests.

**The purpose** travels in the request header `x-skrongle-call`
(`purpose=generate-lesson&key=<slug>&job=<id>&var.<name>=…`, URL-encoded),
which `ai.rs` sends on every model request, to the real providers too, who
ignore it. The system prompt alone does not tell the call sites apart (the
generation calls share one), and the body may carry no extra field
(Anthropic refuses unknown ones), so a header is where it goes. A request
without it (another client) gets its purpose guessed from the prompts.

### Skrongle AI: the server's key in dev

`./.dev-start` and `scripts/e2e.sh` start the server with
`SKRONGLE_AI_KEY=sk-ant-dev-platform` (not with `--llm live` / `E2E_LLM_MODE=live`,
where the real providers would refuse it), so [Skrongle AI](features#skrongle-ai-allowances-and-credits)
is on: the dev-mode default Anthropic base URL takes its calls to the
simulator's **Platform key** card. The app does not know: it only has "the
server's key" (`SKRONGLE_AI_BASE_URL` is unset).

- Free and Basic have no Skrongle AI until they hold credits (#931), so
  their learners see exactly what they did before. Make the learner Plus
  (`/dev/login?premium=1` or `?plan=plus`, the profile's **Dev: grant Plus**, or
  buying Plus in the profile's plans modal through the dev Checkout) and they are
  offered it in the tutor and the Generate dialog, with "Skrongle AI left this
  month: 83 %", and the profile shows the month as a bar. On Plus their own
  keys are used only with the API keys card's pill on.
- The learner's limit is the app's (`ai_usage`, `SKRONGLE_AI_ALLOWANCE_PLUS`);
  the **platform's** limit is the simulator's: **Hit the limit now** or a
  limit on the Platform key card makes the next Skrongle AI call fail as
  Anthropic would, and the learner reads "Skrongle AI is not available right
  now…" (`ai::HOSTED_UNAVAILABLE`). A failure injected there acts on
  Skrongle AI calls only.
- `SKRONGLE_AI_KEY= ./.dev-start …` (empty) turns Skrongle AI off.

### Fixtures: canned replies for repeatable tests

`data/test-fixtures/llm/<purpose>.json`:

```json
{ "replies": {
    "default": { "reply": { "course": { … }, "lessons": [ … ] } },
    "slow": { "reply": { … }, "delay_ms": 3000 },
    "invalid-json": { "reply": "Sorry, I can't write that course right now." },
    "timeout": { "timeout": true },
    "provider-error": { "error": "the API answered HTTP 401. invalid x-api-key" },
    "*": { "reply": "…{{key}}…" } } }
```

`revise-plan.json` plans revisions of the course the `default` outline
makes: `default` rewrites one lesson and adds one, `remove` drops a lesson
(no lesson requests), `nothing` keeps everything ("Nothing changed"),
`invalid-json` can't be read, `slow` is `default` after 4 s (to catch the
`revising` state), and `replace-all` fits **any** course: a new title
("Your course, rewritten overall"), units "Unit A/B (everything
replaced)" and lessons "New lesson 1–3: …", so every old lesson is removed
(Undo brings it back); put `[fixture:NAME]` in the feedback. `remove` and
`default` can be used in turn on the same course, but the same `default` twice
ends in "Nothing changed", because its rewrite is identical the second time.
`edit-plan.json` plans an Edit lessons run (#1240) on that same Fractions
course: `default` rewrites "Simplifying" and renames it "Simplifying
fractions" (the rewrite comes from `revise-lesson.json`'s `*`), `nothing`
keeps every lesson ("Nothing changed"), `adds`, `drops` and `reorders` break
the edit's rules, so the plan is refused (asked again once, then the run
stops with the course unchanged), `invalid-json` can't be read and `slow` is
`default` after 4 s; put `[fixture:NAME]` in the feedback.
Both plans have `check` and `check-clean` for a fact check (#1238): `check`
checks every lesson, `check-clean` only "Adding fractions". Each checked
lesson is answered by `check-lesson.json`, keyed by its slug:
`what-is-a-fraction` comes back with one fix and its why line, `simplifying`
unchanged with one Check line, and `*` unchanged with none (so `check-clean`
ends in "The check found nothing to fix").
`edit-lesson.json` answers Edit with AI on one lesson's row (#1242), one
request and no plan. Its replies are text, so `{{frozen}}` can stand for the
lesson's own id, slug, title, unit, group, level, order and prerequisites:
`default` fits any lesson of any course and rewrites its text ("This lesson
was edited from your request"), `renames` gives it a new id and slug, so the
result check refuses it (asked again twice, then the run stops with the
course unchanged), `invalid-json` can't be read and `slow` is `default` after
3 s; put `[fixture:NAME]` in the request.
`add-lesson.json` adds a lesson: `default` appends "Comparing fractions"
after the last lesson of **any** course (its unit and group, a new id each
time, so it works on Spanish too), `middle` puts it after "What a fraction
is" in that Fractions course, `first` puts "Why fractions" first,
`bad-after` names a lesson that doesn't exist, `taken` reuses an id, and
`invalid-json` can't be read, and `slow` is `default` after 3 s (to try the locks while the job runs); put `[fixture:NAME]` in the brief.
`translate-course.json` answers "Copy and translate to…" (#630), one request
per file of the copy, keyed by the file's path. The request numbers the
file's texts (`{"1": "…", "2": "…"}`) and the reply gives the same numbers
their translations; a number left out keeps its text. `course.json` and
`lessons/first-lesson.json` translate the starter course of *Create a
course* (`courses::starter_files`) into Spanish, so a fresh draft copied
and translated to Español comes out as *Untitled course (Español)* with
*Tu primera lección*; `*` answers any other file with no translations, so
any course goes through unchanged. The numbers follow the order the app
walks a file's JSON (keys sorted), each distinct text once: a change to
the starter files moves them (`ai::tests::translates_a_copy_from_the_llm_fixtures`
says so).
`content-review.json` answers the AI content check's stage 2 (#1037): one
request per course with parts the Jev stand-in scored high. `default` is a
fine verdict, `harmful` quotes "hurting yourself is the answer" (self-harm)
and `harmful-new` quotes "people like them are vermin" (hate speech) with a
report saying the override note doesn't cover it. Put `[fixture:NAME]` in
the lesson's text next to the phrase.

The key is the call's own (the lesson slug for `generate-lesson` and `revise-lesson`), or else
a `[fixture:NAME]` token anywhere in the prompt, or else `default`; `*`
catches the rest. A reply may be text or JSON (sent as pretty JSON). In a
reply `{{key}}` is replaced by the key, `{{job}}` by the job id (to make an
id new each time) and `{{name}}` by each of the call's `vars`
(`ai::Call::var`; `add-lesson` passes `{{last_lesson}}`, `{{last_unit}}` and
`{{last_group}}`, so its reply fits any course). `delay_ms` waits, `error`
and a reply starting with `ERROR:` answer with that error (one with `HTTP
NNN` in it gets that status, so the app reports the same text), and
`"timeout": true` hangs until the client gives up. A new call site gets a
`<its purpose>.json`; no code changes. So the scenario of a whole
generation is chosen by the learner's wish:

| Wish contains | Outcome |
|---|---|
| nothing special | A 3-lesson "Fractions" course, ready |
| `[fixture:slow]` | The same, after a 3 s outline |
| `[fixture:slow-lessons]` | The same outline at once, then its first lesson (`slow-lesson`) after 3 s: the editor between the outline and lesson 1, with units but no lesson row |
| `[fixture:invalid-json]` | Fails: the outline is not JSON |
| `[fixture:validation-fails]` | Lesson 1 saved, lesson 2 fails validation three times; resumable (a resume keeps lesson 1 and stops on lesson 2 again: its reply is keyed by its slug) |
| `[fixture:timeout]` | Fails with a timeout, once the client's limit passes (`SKRONGLE_AI_TIMEOUT_SECS`) |
| `[fixture:provider-error]` | Fails with HTTP 401 |

`cargo test` runs these scenarios through the simulator on loopback
(`ai::tests::generates_from_the_llm_fixtures`, `routes::simulated_provider`)
and `scripts/smoke.sh` runs the default one over HTTP.

### Manual replies: answer the model's requests yourself

The **Manual replies** pill on `/dev/ai` (or `./.dev-start --llm capture`,
which starts with it on) makes the simulator write each request the
fixtures would have answered to `dev-llm/<seq>-<purpose>.request.json`:

```json
{ "seq": 1, "purpose": "generate-outline", "key": "", "job_id": "…",
  "provider": "anthropic", "model": "claude-opus-5-5",
  "system": "You write learning courses…", "messages": [{ "role": "user", "content": "…" }],
  "respond_in": "0001-generate-outline.response.txt" }
```

and wait for `dev-llm/0001-generate-outline.response.txt`: the reply
text, exactly as the model would send it. With the pill on, the
**Manual replies** section at the bottom of `/dev/ai` lists each request,
newest first, as a folded row in a scrolling box (the page polls, so a new
request appears by itself); open one to read what the model was asked and
unfold its **Reply** box to type the reply. Or write the reply to a
temporary name and rename it into place. A
reply that starts with `ERROR:` answers with the rest as the provider's
error (`ERROR: the API answered HTTP 429. rate limited`). The call log on
`/dev/ai` shows it as "answered by hand". Usage limits and injected
failures still come first. Sequence numbers carry on across restarts;
`--reset` clears the folder.

### Usage and limits

Every reply counts one request and its tokens (4
characters a token, in and out) and costs dollars from a made-up price table
(`dev_ai::PRICES`, per million tokens: `claude-opus*` 15/75, `claude-sonnet*`
and `claude-fable*` 3/15, `claude-haiku*` 1/5, `gpt-5-mini` 0.25/2, `gpt-5`
1.25/10, `gemini*` 0.30/2.50, anything else 1/5). A profile's limit is a
number in one measure (requests, tokens, dollars); once usage reaches it, or
after **Hit the limit now**, every request gets the provider's quota answer:

| Format | Status | Body |
|---|---|---|
| Anthropic | 400 | `{"type": "error", "error": {"type": "invalid_request_error", "message": "You have reached your specified API usage limits. You will regain access on <first of next month> at 00:00 UTC."}}` (what a spend limit answers; a prepaid balance at zero says "Your credit balance is too low…" with the same status and type) |
| OpenAI, Gemini | 429 | `{"error": {"message": "You exceeded your current quota, please check your plan and billing details. …", "type": "insufficient_quota", "param": null, "code": "insufficient_quota"}}` |

The learner reads "Your AI provider account has run out of credit or reached
its usage limit. Check your plan and billing with the provider, then try
again." (`ai::plain_failure`; before #297 the first read as "turned the
request down", the second as "busy").

**Injected failures,** for the next request or every request until cleared:

| Failure | Anthropic | OpenAI / Gemini | The learner reads |
|---|---|---|---|
| Timeout | hangs N s (default 30), then answers | same | "didn't answer in time" once N passes the client's limit |
| 500 server error | 500 `api_error` | 500 `server_error` | "busy" |
| Overloaded | 529 `overloaded_error` | 503 | "busy" |
| 401 invalid key | 401 `authentication_error` | 401 `invalid_api_key` | "didn't accept your API key" |
| Malformed JSON body | 200, a cut-off body | same | "couldn't be used" |
| Reply not valid for the purpose | 200, prose where JSON is wanted | same | "couldn't be used" |

The app's client waits 15 minutes for a generation request and 2 for a chat
turn; `SKRONGLE_AI_TIMEOUT_SECS` replaces both (`scripts/e2e.sh` sets 8, so an
injected hang is over in seconds).

**Fixtures in the browser.** The page lists the files in
`SKRONGLE_LLM_FIXTURES` and their reply keys. Open one to edit its JSON:
**Use for this run** keeps the edit in memory, **Save to file** writes it back to
`data/test-fixtures/llm/` (dev mode only, like every `/dev` route), **Reset**
goes back to the file. **Add a scenario** puts a named reply (or error, with
a delay) into a file in memory; a wish with `[fixture:NAME]` picks it.

**Everything is in memory:** the log (newest 100 calls, bodies up to 96 KB
each), the counters, the settings, Manual replies and the edits; a restart
forgets them.

## Worked example: generate a course with a hand-written reply

```sh
./.dev-start --reset --llm capture --background             # Manual replies on from the start
URL=$(cat .dev-server.url)
curl -s -c jar "$URL/dev/login?keys=dev" -o /dev/null       # the simulator's keys
DRAFT=$(curl -s -b jar -o /dev/null -w '%{redirect_url}' --data title= "$URL/courses/new")   # → …/courses/<id>?generate=1
curl -s -b jar -o /dev/null -w '%{redirect_url}\n' \
  --data-urlencode provider=anthropic --data-urlencode 'wish=Fractions for Friday' \
  --data-urlencode lessons=3 "${DRAFT%%\?*}/generate"   # → …/courses/<id>?job=<job>
cat dev-llm/0001-generate-outline.request.json            # read what the model is asked
# write the outline: {"course": {…}, "lessons": [3 entries]} (see data/test-fixtures/llm/generate-outline.json)
cp my-outline.json dev-llm/0001-generate-outline.response.tmp
mv dev-llm/0001-generate-outline.response.tmp dev-llm/0001-generate-outline.response.txt
# then 0002-generate-lesson…, 0003-…, 0004-…: one lesson JSON each
curl -s -b jar "$URL/api/generate/<job>"                  # {"done": true, "error": null, "course_id": …}
```

Then open the course: `POST /courses/open/<course_id>` (or the open icon
by the title on its page, `data-testid="course-edit-open"`, which opens it in a
new tab; specs use `openFromEditor` in `e2e/lib/app.ts`) and look at
`/lessons`. In a browser the same flow works from `/dev/login?keys=dev&next=/courses` (*Create a course*)
with replies typed into the Manual replies section at the bottom of `/dev/ai`.

## Payments: the dev payment provider

Subscriptions (and, with #311, AI credit packs) go through Stripe: Checkout
to pay, the Billing Portal to manage, signed webhooks to tell the app
(`src/billing.rs`). In dev mode a stand-in inside the dev server takes
Stripe's place (`src/dev_payments.rs`, `src/routes/dev_payments.rs`, #314).
It **implements Stripe's API**: the app's own billing code calls it exactly as
it calls Stripe, form-encoded with a Bearer key, so nothing in the app knows
it is fake. It hosts the pages Stripe hosts and sends the webhooks Stripe
sends, signed with the app's webhook secret, to the app's own
`/billing/webhook`.

```sh
./.dev-start --reset --seed --background   # then /dev/login?next=/profile → Choose a plan → Plus → Stripe Checkout, and <url>/dev/payments
```

Dev mode gives the server Stripe settings that point here, so the profile's
plans modal pays through it out of the box (Stripe is the only way to pay
since #963 removed the pretend card payment of #315):

| Setting | Dev mode's value |
|---|---|
| `SKRONGLE_STRIPE_API_URL` | `http://127.0.0.1:<port>/dev/payments/stripe` |
| `SKRONGLE_STRIPE_SECRET_KEY` | `sk_test_dev_simulator` (the simulator takes any `sk_test_…` or `rk_test_…` key, refuses others with Stripe's 401) |
| `SKRONGLE_STRIPE_WEBHOOK_SECRET` | `whsec_dev_simulator` |
| `SKRONGLE_STRIPE_PRICE_ID` / `_PRICE_ID_BASIC` / `_PRICE_LABEL` | `price_dev_monthly` (CA$15.00 a month, Plus) / `price_dev_basic` (CA$3.00 a month, Basic) / `CA$15 a month` |
| `SKRONGLE_PUBLIC_URL` | as set, else `http://127.0.0.1:<port>`: the return links and where the webhooks go |
| `SKRONGLE_DEV_STRIPE` | `0` turns the stand-in off: no Stripe settings, "Subscriptions are not offered on this server" (the API under `/dev/payments/stripe` still answers) |

Real `SKRONGLE_STRIPE_*` settings win: with them the calls go to Stripe (or
wherever `SKRONGLE_STRIPE_API_URL` says) and `/dev/payments` says so.

| Call | What it does |
|---|---|
| `POST /v1/checkout/sessions` | `mode=subscription` or `payment`, one `line_items[0][price]` (+ `quantity`), `success_url`, `cancel_url`, `customer` or `customer_email`, `client_reference_id`, `metadata[…]`, `subscription_data[metadata][…]`, `payment_intent_data[metadata][…]`. Answers a `checkout.session` whose `url` is the hosted Checkout page. A recurring price in payment mode (or the reverse), a missing or unknown parameter: Stripe's 400 |
| `POST /v1/billing_portal/sessions` | `customer`, `return_url`: a `billing_portal.session` whose `url` is the hosted portal |
| `GET /v1/{checkout/sessions,subscriptions,customers,invoices,payment_intents,charges,prices}/{id}` | The object, or Stripe's 404 `resource_missing` |
| `POST /v1/subscriptions/{id}`, `DELETE /v1/subscriptions/{id}` | `cancel_at_period_end`, `metadata[…]`; cancel now. Each sends its event |
| `POST /v1/refunds` | `payment_intent` or `charge`, `amount` (default: what is left): `charge.refunded` and `refund.created` |

**Prices.** `price_dev_monthly` (CA$15.00 a month: Plus), `price_dev_basic` (CA$3.00 a month: Basic), and one-off
`price_dev_credits_5` ($5) and `price_dev_credits_10` ($10) for credit packs
(`SKRONGLE_AI_CREDIT_PACKS=price_dev_credits_5:5.00:$5` with #311). A price it
does not know is made up on first use at €5.00, recurring in subscription
mode and one-off in payment mode, so any pack id works.

**The events** each step sends (Stripe's shapes, API version
`2025-03-31.basil`, the period on the subscription item and on the
subscription): Pay in subscription mode `customer.created` (a new
customer), `customer.subscription.created`, `charge.succeeded`,
`payment_intent.succeeded`, `invoice.paid`, `invoice.payment_succeeded`,
`checkout.session.completed`; in payment mode the charge, the payment
intent and `checkout.session.completed` (`payment_status: paid`, the
session's `metadata`, `payment_intent`, `amount_total`). Fail sends
`payment_intent.payment_failed`. **Renew now** simulates the end of a period:
paid (`invoice.paid`, `customer.subscription.updated` with the next period),
failed after **Fail the next payment** (`invoice.payment_failed`, the
subscription `past_due`), or, set to cancel at the period end,
`customer.subscription.deleted`. The app acts on `customer.subscription.*`
(and #311's on `checkout.session.completed` and `charge.refunded`); the
rest show in the log as the app's 200.

Webhooks are delivered before the page answers, in order, on a thread of
their own, so after **Pay** the profile already shows the plan. **Send
again** repeats an event as it was (an event older than the last one the
app applied is skipped, as with Stripe's retries); **Send with a broken
signature** must get the app's 400.

**Everything is in memory** (the log keeps 200 entries): after a restart the
app's database still has its subscriptions, but the simulator has forgotten
them; a customer id it does not know is made again, empty, so paying with
Stripe and the portal still work.

## Query trace

`SKRONGLE_SQL_TRACE=<file>` makes the server append every SQL statement it
runs to that file, one JSON line each:

```json
{"t":1761150000123,"us":41,"vm":12,"scan":0,"sort":0,"sql":"SELECT points, goals, day, day_points, streak FROM earnings WHERE user_id = 1 AND course_id = 'spanish'"}
```

`t` is when the statement finished (ms since the epoch), `us` how long it
took, and `vm`, `scan` and `sort` are SQLite's own counters for that run:
virtual-machine steps, rows read by a full table scan, and sorts. The SQL
has its values filled in. The counters are what to read when checking that
a page's queries stay bounded (#589): a statement whose `vm` or `scan` grows
with a learner's history is reading that history. Fill a table up (a
recursive `INSERT … SELECT` from `sqlite3`), load the page twice and compare
the lines between the two loads. The counters are each run's own (SQLite
adds them up over a cached statement's runs; the trace takes the difference).
Off unless the variable is set; not for production.

## Smoke test

```sh
scripts/smoke.sh          # fresh ./smoke.db, a free port, the dev AI provider; pages + one generation
```

It seeds, logs in through `/dev/login`, expects 200 from every main page
and from every page the docs link to (it follows the links from `/docs`),
generates a course with the seeded Anthropic key (the dev AI provider
answers from the fixtures, and its log must show the calls) and opens it,
then resets the user.
It leaves a running `./.dev-start` alone. Run it with `cargo test` before
finishing a change.

## Panic check, file size lint and Pi budget

Three tripwires run in CI and compare with a committed baseline, so raising
one is a visible line in the PR. `scripts/panic-check.sh` (a step of the
`cargo test` job) counts the lines outside tests that can panic, per file,
against `scripts/panic-baseline.txt` (#389). `scripts/file-size-lint.sh`
(also a step of the `cargo test` job, #908) counts the non-test lines of
every `.rs`, `.js`, `.ts`, `.html` and `.css` file under `src/`, `static/`,
`templates/` and `e2e/`. A `.rs` file counts up to its first `#[cfg(test)]`
line, a file named `tests.rs` counts nothing, and other files count whole.
A file may have at most 1,500. The files that were over it when the lint
came in are listed in `scripts/file-size-baseline.txt`; they may shrink, not
grow. A file that passes its limit gets split. If it has to grow, say why in
the PR and run `scripts/file-size-lint.sh --update`, which also lowers the
line of a file that shrank. `scripts/pi-budget.sh` (the
`pi budget` job, #638; off in CI for now, #1387) builds `--release` (or measures `--bin <path>`) and
checks three numbers against `scripts/pi-budget.txt`: the stripped binary's
size (at most its platform's baseline + 5 %), and, for a server started with
no optional settings (no mail, SMS, Stripe or AI key, no
`SKRONGLE_DEV`) on a temp database after twenty requests each to `/`, `/login`
and `/healthz`, its resident memory (≤ 32 MiB) and thread count (≤ 12). On
Linux the numbers come from `/proc/<pid>/status`; on a Mac from `ps`, so it
runs locally too, but a Mac's binary has its own baseline line and its memory
is shown, not checked (`ps` counts the shared system libraries, some 10 MiB
more). CI's runners are linux-aarch64, the Pi's architecture. The job writes the numbers in its summary. Over a limit,
make the change cheaper (a cargo feature, a lighter crate, no new thread), or
raise the number in `pi-budget.txt` and say why in the PR;
`scripts/pi-budget.sh --update` rewrites this platform's binary baseline.

## Markup hooks

Templates carry `data-testid="<area>-<thing>"` (kebab-case) on forms,
inputs, primary buttons, list items, status banners and error messages.
They are stable: copy changes never rename them. State is in attributes,
not only text. Errors are `role="alert"`, progress is `role="status"`.
Forms submit with a plain `<button type="submit">` and need no JavaScript,
so `form.requestSubmit()` and browser-extension clicks work.

AI jobs (generating a course, and **Add content with AI**) have no status
page: the job shows in a modal on the course page (`course-ai-modal`,
`data-state="running|done|failed"`), which polls `data-poll-url` (the JSON
API) and then reloads with `?job=<job>`, so its final state is
server-rendered. A generation that succeeded shows no modal at all: the
page is the course. Add `?reduce-motion=1` to any page to turn off
animations, so screenshots are deterministic.

### Reference

The pages the AI creation flow and lesson tests use, in full:

| Page | Testids | State attributes |
|---|---|---|
| every page (`base.html`) | `nav-home`, `nav-course-switch`, `nav-courses`, `nav-account-menu`, `nav-account-toggle`, `nav-profile`, `nav-switch-course`, `nav-docs`, `nav-logout-form`, `nav-logout`, `nav-login`, `nav-register`, `footer-terms`, `footer-privacy`; mode bars `nav-study`/`nav-play` with tabs `nav-study-<tab>`, `nav-play-<tab>`, and `nav-study-mode`, `nav-study-to-sprint`, `nav-study-to-play`, `nav-study-menu` (and `nav-play-…` likewise) | `nav-account-toggle[data-email]`, `nav-course-switch[data-course-id]` (the course open now), `a[aria-current=page]` on links to the current page |
| `/` (home) | `home-stats` with `home-stat-passed`, `home-stat-mastered`, `home-stat-streak`, `home-skipped`; the map row `home-map` with `home-map-title`, `home-map-unit`, `home-map-progress`; `home-mode` (each card), `home-<mode>-open` (its heading link), `home-info-<mode>` (its (i)) opening `home-info-<mode>-dialog` (closed by `home-info-close`), `home-<mode>-<target>` (each link in it: `home-study-lessons`, `home-guide-stages`, `home-guide-next`, `home-ai-api-keys`, …), `home-guide-stat`, `home-start-sprint`/`-study`/`-play`/`-guide`/`-tutor`; with nothing to run the tutor on, `home-ai-blocked` (the red !, in place of `home-info-ai`) and `home-start-tutor` open `home-tutor-gate` (`home-tutor-gate-close`, `home-tutor-gate-add-key`) | `home-mode[data-mode=sprint\|study\|play\|guide\|ai]`, `home-map-progress[data-passed][data-total]` (the current unit), `home-guide-stat[data-completed][data-total]` |
| `/sprint/play` | `sprint-play-main` (the `<main>`: stage, answer sheet and results), `sprint-play-stage`, `sprint-play-result`, `sprint-play-verdict`, `sprint-play-next`, `sprint-play-done`, `sprint-play-done-home`, `sprint-play-again`, `sprint-play-check`, `sprint-play-exit`, `sprint-play-audio`, `sprint-play-sound`, `sprint-play-progress` | `sprint-play-stage[data-kind][data-exercise-id][data-pos]` (the exercise up: its kind, its id as in `src/sprint.rs`, its place; `data-kind=done` at the end), `sprint-play-result[data-state=right\|wrong]`, `sprint-play-done[data-saved=true]` once the session is recorded; tiles and options: `[aria-pressed]` when picked, `[data-state=right\|wrong\|used\|done]`; options and speakers carry `[data-value]` |
| `/login`, `/register` | `login-form`, `login-email`, `login-password`, `login-submit`, `login-error`, `login-register-link`, `login-reset-link`, `login-notice`; dev mode, in the `login-dev` card: `login-dev-login`,  `register-form`, `register-email`, `register-password`, `register-submit`, `register-error`, `register-login-link`, `register-owner-note` (the first account on an invite/closed server), `register-invite` (the hidden invitation token); `register-closed` (the sign-ups-closed page), `register-closed-message`, `register-closed-login`; a valid form, "check your mail" (#379): `register-sent`, `register-sent-email`, `register-sent-login`; behind a signing-in proxy (`SKRONGLE_PROXY_AUTH_*`) `/login`, `/register` and `/logout` are `proxy-signin` with `proxy-signin-note` (or `proxy-signin-signed-out` after logout) and `proxy-signin-home`; dev mode, in the `register-dev` card: `register-dev-login` | |
| `/reset` | `reset[data-step]` (`ask`, `sent`, `choose`, `invalid`, `done`), `reset-ask-form`, `reset-email`, `reset-submit`, `reset-error`, `reset-sent`, `reset-choose-form`, `reset-account`, `reset-password`, `reset-save`, `reset-invalid`, `reset-again`, `reset-done`, `reset-login` | |
| `/login/code` | `login-code[data-method]` (`sms`, `email`, `totp`), `login-code-sent-to`, `login-code-form`, `login-code-input`, `login-code-submit`, `login-code-error`, `login-code-notice`, `login-code-resend`, `login-code-cancel`; dev mode: `login-code-dev` (the floating window), `login-code-dev-frame` (its frame of `/login/code/device`: `login-code-device[data-method]`, `login-code-device-to`, `login-code-device-off`, `login-code-device-item[data-code]`, `login-code-device-code`, `login-code-device-remaining`, `login-code-device-use`, `login-code-device-empty`) | `login-code[data-method]`, `login-code-device-item[data-code]` | `login-code[data-method]` |
| `/password/change` | `password-change-gate`, `password-change-info` (the (i)) and `password-change-dialog`, `password-change-form`, `password-change-new`, `password-change-confirm`, `password-change-submit`, `password-change-error`, `password-change-logout` | |
| `/account/deletion` | `account-deletion`, `account-deletion-info` (the (i)) and `account-deletion-dialog`, `account-deletion-date`, `account-deletion-cancel`, `account-deletion-logout` | #970 |
| `/courses` | sections `courses-projects`, `courses-mine`, `courses-builtin`, `courses-make`; each card `courses-card` with `courses-open`, `courses-resume` (stopped with its outline saved), `courses-retry` (stopped before it), `courses-delete-form`, `courses-delete` (stopped cards), `courses-card-stopped`, `courses-watch`, `courses-cancel`, `courses-generating`, `courses-card-phase`, `courses-card-revising`, `courses-card-error`, `courses-card-error-detail`, `courses-settings` (the gear of an opened course) opening `courses-settings-dialog[data-course-id][data-step]` (on a course not opened yet, `[data-wizard][data-start]`: `courses-open` opens it, #587) with `courses-settings-step`, `courses-settings-level[data-unit]` (a level card; `courses-settings-level-radio` its radio, `courses-settings-level-info` its (i)), <!-- themes -->`courses-settings-theme[data-theme-id]`, <!-- /themes -->`courses-settings-avatar`, `courses-settings-colors`, `courses-settings-color`, `courses-settings-prev`, `courses-settings-next`, `courses-settings-save` (Get Started!), `courses-settings-close`, and at the gear dialog's top right (not in the wizard) `courses-edit` (the pencil to the editor, own courses) and `courses-copy` in `courses-copy-form` (#629); `courses-notice`, `courses-create-form`, `courses-create` (Create a course); `?generate=<id>`: the Generate dialog of a stopped course (testids as on `/courses/{id}`) | `courses-card[data-course-id][data-course-status=generating\|revising\|ready\|failed]`, project cards `[data-active]` |
| `/courses/{id}` Generate dialog (a draft or a stopped generation; open on `?generate=1`) | `generate` (the `<dialog>`), `generate-close`, `generate-named` (the names the AI keeps), `generate-own`, `generate-form`, `generate-error`, `generate-wish`, `generate-wish-readonly` (resume), `generate-notes`, `generate-sensitive-note` (the "don't paste sensitive personal data" line under it), `generate-files`, `generate-kind` (each radio), `generate-depth-choice` (the Full course / Preview / Outline fieldset) with `generate-depth` (each radio), `generate-depth-info` and `generate-depth-info-text`, `generate-provider` (each radio: only with two or more choices; Skrongle AI's is `value="hosted"` `data-hosted`), `generate-provider-only` (one choice: a hidden input, no choice shown), `generate-ai-left` (Skrongle AI offered: what is left and about what a lesson costs), `ai-out` (Skrongle AI did not cover the job: under `generate-error`) with `ai-out-upgrade` (free plan, Stripe on), `ai-out-buy` (`[data-pack]`, each pack on sale) and `ai-out-own-key`, `generate-no-key` (with, when Skrongle AI is not offered, `hosted-gap-reason[data-none]`, `hosted-gap-instead` and `hosted-gap-upgrade`; the same on `tutor-no-key` and `home-tutor-gate`) (no key: the line above the form) with `generate-profile-link`, `generate-disabled` (the form's fieldset, disabled then), `generate-instructions`, `generate-submit` | on `generate`: `data-course-id`, `data-resume`; `[open]` while shown |
| `/courses/generate/{job}` | none: redirects to `/courses/{id}?job={job}` | |
| `/courses/{id}` (the AI job modal: a generation or Add content with AI) | `course-ai-modal-backdrop`, `course-ai-modal`, `course-ai-modal-title`, `course-ai-modal-what`, `course-ai-modal-phase`, `course-ai-modal-bar`, `course-ai-modal-count`, `course-ai-modal-cancel-form`, `course-ai-modal-cancel`, `course-ai-modal-refresh` (no JavaScript), `course-ai-modal-error`, `course-ai-modal-error-detail`, `course-ai-modal-close` | on `course-ai-modal`: `data-job`, `data-state` (`running` \| `failed`; an Only add / Allow overall changes job that changed the course opens `course-review-modal` instead), `data-done`, `data-error`, `data-phase`, `data-adding`, `data-revision`, `data-lessons-done`, `data-lessons-total`, `data-poll-url`, `data-done-url` |
| `/courses/{id}?review=1` (Review AI changes; also opens when an Only add / Allow overall changes job finishes, unless "Don't show this automatically" is ticked) | `course-ai-review` (the *Review changes* button next to Submit), `course-review-backdrop`, `course-review-modal`, `course-review-label`, `course-review-overview`, `course-review-error` (Accept refused), `course-review-close`, `course-review-expand-all`, `course-review-collapse-all`, `course-review-raw` (show the files as JSON), `course-review-list`, `course-review-file`, `course-review-details` (its ▸), `course-review-file-link` (the lesson's row: the summary that opens it), `course-review-position`, `course-review-file-title`, `course-review-tag`, `course-review-summary`, `course-review-exclude-file` (the lesson's −), `course-review-split`, `course-review-whole`, `course-review-row` (a changed line), `course-review-exclude-line` (its −), `course-review-del`, `course-review-add`, `course-review-line` (a line of a one-column file), `course-review-edit-input` (the box a line opens into on click, #287: a textarea holding the raw JSON line), `course-review-history` (the modal's Undo / Redo of edits in the footer, `hidden` until the first edit) with `course-review-undo` and `course-review-redo` (`disabled` with nothing to undo / redo), `course-review-edit-value` (the hidden `edit:<path>:<key>` field Accept sends), `course-review-gap`, `course-review-form`, `course-review-quiet`, `course-review-left-out`, `course-review-accept` (absent once the run is undone), `course-review-undone` (the *Undone* tag of a run undone since, #281) with `course-review-undone-info` (its (i)) and `course-review-undone-text` | `course-review-modal[data-files][data-after-job][data-undone][data-labels]` (`data-labels`: the plain names of JSON keys, for the script), a dimmed id-only cell's `td.rv-quiet`, `course-review-file[data-path][data-kind][data-added][data-removed]` (`kind`: `added` \| `removed` \| `changed`), `course-review-row[data-key]`, `course-review-exclude-line[data-group]` (0: not left out; rows left out together share a number), an editable cell's `td[data-edit]` (any cell on the right of a JSON file, `tr.rv-row.same` ones too; focusable, Enter opens it) with `data-ai` (the line as the AI's file has it) and class `rv-is-edited` while edited (blue; its row `tr.rv-edited-row`, its `course-review-exclude-line` and its lesson's `course-review-exclude-file` `disabled`) |
| `/courses/{id}` (the feature usage report, #328) | `course-report-open` (the round bar-chart button beside the gear; a `#course-report` link without JavaScript), `course-report` (the `<dialog>`), `course-report-close`, `course-report-unit` (a unit, a `<details>`, the first `open`), `course-report-unit-counts` ("2 lessons · 7 of 21 features · 3 of 6 question types"), `course-report-lesson` (a lesson, a nested `<details>`), `course-report-question` (a question type of its quiz), `course-report-feature` (a lesson feature) | `course-report-unit[data-unit]`, `course-report-lesson[data-lesson]`, `course-report-question[data-key][data-used]` (`key`: the quiz type, e.g. `true-false`), `course-report-feature[data-key][data-used]` (`key` from `FEATURES` in `src/report.rs`, e.g. `text`, `vocabulary`, `listening`; `used`: `true` \| `false`) |
| `/courses/{id}` | `course-edit`, `course-edit-locked` (the workspace, `inert`, under a running job's modal; no `course-edit-cancel` or busy banner then), `course-edit-cancel-form`, `course-edit-cancel`, `course-edit-open-form`, `course-edit-open`, `course-edit-notice`, `course-edit-generating`, `course-edit-error`, `course-edit-files`, `course-edit-file`, `course-edit-file-link`, `course-edit-file-delete-form`, `course-edit-file-delete`, `course-edit-new-file`, `course-edit-manifest`, `course-edit-export`, `course-edit-delete-form`, `course-edit-delete`, `course-edit-revising`, `course-ai`, `course-ai-form`, `course-ai-error`, `course-ai-text`, `course-ai-sensitive-note`, `course-ai-mode-choice` (the Add / Edit / Add and edit choice, a `<fieldset>`, #1241), `course-ai-mode` (each of its radios, `name="mode"`, `[value=add\|edit\|change]`; `change` is `disabled` on a course that cannot be revised; `e2e/lib/app.ts` `aiMode(page, value)`), `course-ai-mode-info` (the "i", a `<details>`), `course-ai-mode-info-text`, `course-ai-mode-finished-only` (in it: why Add and edit is greyed out), `course-ai-outline` (the *Outline only* switch, #1119: the lessons it writes are outlines; `disabled` while Edit is chosen), `course-ai-outline-info`, `course-ai-outline-info-text`, `course-ai-submit`, `course-ai-provider` (the provider named in `ai-disclosure`, with two or more saved keys only), `course-ai-generate` (a draft: opens the Generate dialog), `course-ai-no-key`, `course-ai-profile-link`, `course-history` (the Undo and Redo icons, in `course-history-row` under the table), `course-undo` (always there; `disabled` with nothing to undo; `data-discard` while the table holds changes that cannot be saved, when it takes those back instead, #288), `course-undo-form` (only when there is something to undo), `course-undo-label` (inside it, not shown: the change it takes back), `course-redo`, `course-redo-form`, `course-redo-label` (the same for Redo), `course-edit-title`, `course-edit-rename` (the title as a button, renames on click), `course-edit-rename-input`, `course-edit-save-status` (the circle by the title; hidden without scripts and while a job locks the page), `course-edit-save-error` (why something is not saved), `autosave-leave-dialog`, `autosave-leave`, `autosave-stay`, `course-lessons`, `course-lessons-list`, `course-lessons-settings`, `course-lessons-status`, `course-lessons-unit`, `course-lessons-unit-title`, `course-lessons-fold`, `course-lessons-count`, `course-lessons-add-here`, `course-lessons-gear` (the gear on the Course settings row: opens `course-lessons-prefs`, the Editor settings dialog, with `course-lessons-pref-json` and `course-lessons-pref-lines` rows and `course-lessons-prefs-close`), `course-lessons-add-unit` (+ Add unit, on the Course settings row right of the gear), `course-lessons-unit-rename` (a new unit's name box), `course-lessons-unit-delete` (a unit's ✕, hidden on the last unit), `course-lessons-unit-dialog` (asks before a unit with lessons goes; `course-lessons-unit-cancel`, `course-lessons-unit-confirm`), `course-lessons-row`, `course-lessons-grip`, `course-lessons-position`, `course-lessons-expand`, `course-lessons-title`, `course-lessons-rename`, `course-lessons-edit`, `course-lessons-duplicate` (⧉, copies the lesson below it), `course-lessons-ai-edit` (✦, Edit with AI on that lesson, #1242: only with a key or Skrongle AI, not on an unsaved row; opens `course-lessons-ai-dialog[data-reopen]`, with `course-lessons-ai-form`, `course-lessons-ai-title`, `course-lessons-ai-lesson` (the hidden file field), `course-lessons-ai-text`, `course-lessons-ai-count`, `course-lessons-ai-provider` (with two or more keys the provider's name in it is `course-lessons-ai-provider-name`), `course-lessons-ai-error` (a refused request), `course-lessons-ai-cancel`, `course-lessons-ai-submit`), `course-lessons-delete`, `course-lessons-drawer`, `course-lessons-json`, `course-lessons-json-status` (the drawer's status: valid, or its problem in the words of the line under the title), `course-lessons-json-goto` (*Go to line N*, while the drawer has a problem with a line), `course-lessons-format`, `course-lessons-lines` (the box's line numbers, while the Line numbers switch is on), `course-lessons-bad-line` (the problem's number in it), `course-lessons-json-highlight` (the tint over the problem's line in the box), `course-lessons-json-switch` and `course-lessons-lines-switch` (in the gear's dialog), `course-form`, `course-form-field`, `course-form-section`, `course-form-list`, `course-form-item`, `course-form-add`, `course-form-up`, `course-form-down`, `course-form-remove`, `course-form-correct`, `course-form-kind`, `course-form-map-key`, `course-form-kept`; the lesson editor (#841) in a lesson's `course-form`: `course-form-layout` (`[data-tab]` at phone width), `course-form-tab-outline` / `-edit` / `-preview`, `course-outline`, `course-outline-section` (`[data-sec]`, `[aria-expanded]` on a section with items, `[aria-pressed]` on a single one), `course-outline-item` (`[data-sec][data-sub][data-idx]`, `[aria-pressed]`; a `.le-warn` child when the rules would refuse it), `course-outline-add` (`[data-path]`: the list it adds to), `course-outline-json`, `course-item` (the form: `[data-sec][data-sub][data-idx][data-problem]`), `course-form-field` / `-item` / `-list` / `-add` / `-remove` / `-up` / `-down` / `-correct` with `data-path` as before, plus `course-form-type` (the type switch; its buttons `[data-value][aria-pressed]`), `course-form-check` (a field's own checks: `.le-check[data-check][data-level]`, a ✓ or ! in its corner that opens `course-form-check-dialog` with `course-form-check-list` and `course-form-check-close`), `course-form-new-option`, `course-form-add-teaches`, `course-form-add-prereq`, `course-form-prev` / `-next` / `-duplicate`, `course-form-bucket-prev` / `-next`, `course-form-add-sort-item`, `course-form-add-bucket`, `course-form-remove-bucket`, `course-form-words`, `course-form-cards`, `course-form-uses`, `course-form-step`, `course-form-add-step` (`[data-value]`: the kind), `course-form-guide-by-hand`, `course-form-guide-derive`, `course-form-say`, `course-form-schema`, `course-form-mode` (the preview's Study / Sprint / Guide buttons, `[data-value]`); the preview `course-preview` (`[data-mode]`), `course-preview-body` (`[data-key]`: what it shows, drawn again 120 ms after the last keystroke; wait for it before answering in the preview), `course-preview-phone` (`[data-mode]`), `course-preview-none`, `course-preview-question`, `course-preview-option`, `course-preview-answer`, `course-preview-tile`, `course-preview-bucket`, `course-preview-chip` (a word sorted into its bucket), `course-preview-word`, `course-preview-check`, `course-preview-feedback` (`[data-state=right\|wrong]`), `course-preview-again`, `course-preview-flashcard`, `course-preview-stage` | `course-edit[data-course-id][data-course-status][data-loads]` (`data-loads` follows each save), `course-edit-save-status[data-state]` (`saved`, `saving`, `error`), `course-ai[data-course-id][data-has-key]`, `course-edit-file[data-path]`, `course-lessons[data-started][data-folders][data-version]` (the files' version the page's next save names), `course-lessons-unit[data-unit]`, `course-lessons-row[data-lesson-id][data-unit][data-path]` (an added row has an empty id and path until saved), `course-lessons-row[data-problem]` and `course-lessons-settings[data-problem]` (`json`, `rules` or `refused`: the drawer's problem, until the next edit or save; its box is `aria-invalid`), `course-undo-form[data-earlier]`, `course-lessons-drawer[data-mode]` (`json` or `forms`), `course-lessons-json-switch[aria-checked]`, `course-lessons-lines-switch[aria-checked]`; every `course-form-*` carries `data-path`, the field's place in the file joined by dots (`quiz.3.options.1`; on add, the list's path) |
| `POST /api/courses/{id}/preview-answer` | none: JSON `{question, answer}` → `{correct, note, correct_answer, explanation}`, the lesson editor's Try it (owner only, nothing recorded) | |
| `POST /api/courses/{id}/answer-key` | none: JSON `{question}` → `{answer}`, the first answer of a numeric or expression question the checker cannot read (`null` for none), the lesson editor's red check before a save (owner only) | |
| `/courses/{id}/file` | `course-file-form`, `course-file-path`, `course-file-content`, `course-file-status`, `course-file-save`, `course-file-cancel`, `course-file-error` | `course-file-form[data-new]` |
| `/lessons` | `lessons-filter`, `lessons-filter-status`, `lessons-unit`, `lessons-list`, `lessons-item`, `lessons-item-link`, `lessons-item-practise` | `lessons-item[data-lesson-id][data-passed][data-read][data-status][data-search]`, `lessons-filter-status[data-matches]` (set once something is typed) |
| `/lessons/{slug}` | `lesson`, `lesson-head`, `lesson-mastery`, `lesson-read-form`, `lesson-read`, `lesson-practise`, `lesson-listen`, `lesson-tutor`, `lesson-actions`, `lesson-more` with `lesson-more-toggle`, `lesson-guide`, `lesson-sprint`, `lesson-info` (the (i) by the status) and `lesson-pass-info` (the one in the footer) opening `lesson-info-dialog` with `lesson-pass-rule` and `lesson-info-close`, `lesson-test`, `lesson-prerequisite`, `lesson-content`, `lesson-vocabulary`, `lesson-vocab-item`, `lesson-conjugations`, `lesson-conversation`, `lesson-conversation-form`, `lesson-conversation-start`, `lesson-check`, `lesson-check-form`, `lesson-check-question`, `lesson-check-option`, `lesson-check-answer`, `lesson-check-feedback`, `lesson-check-submit`, `lesson-check-result`, `lesson-check-retry`, `lesson-pass` | `lesson[data-lesson-id][data-passed][data-read][data-seen][data-status]`, `lesson-check[data-done]`, `lesson-check-feedback[data-state=right\|wrong]`, `lesson-check-result[data-state=passed\|failed]`, `lesson-pass[data-passed]`, `lesson-prerequisite[data-lesson-id]` |
| `/profile` | `profile-nav`, `profile-nav-link`, `profile-form`, `profile-notice`, `profile-display-name`, `profile-avatar-shown` (the picture of the course in context; `avatar-blank` with none), `profile-save-status`, `profile-save-error`, `profile-first-name-error`, `profile-last-name-error`, `autosave-leave-dialog`, `autosave-leave`, `autosave-stay`, `profile-password-open`, `profile-password-dialog`, `profile-password-rule` (the length rule), `profile-password-cancel`, `profile-first-name`, `profile-last-name`, `profile-email` (read-only), `email-verified` (the check inside a confirmed address), `profile-privacy-info` (the (i) button), `profile-privacy-note` (the dialog it opens), `profile-privacy-close`, `profile-privacy-policy` (its link to `/privacy`), `profile-public-name` (the name others see, live), <!-- themes -->`profile-theme-form`, `profile-theme` (a chip: a label around a radio), `profile-theme-description`, <!-- /themes -->`profile-password-form`, `profile-password-current`, `profile-password-new`, `profile-password-confirm`, `profile-password-submit`, `profile-password-error`, `profile-sign-in` (how the account signs in), `profile-sign-out-others-form`, `profile-sign-out-others`, `profile-sign-out-others-info` and `profile-sign-out-others-info-dialog` (`profile-sign-out-others-info-close`), `profile-delete-code`, `profile-delete-send`, `profile-delete-sent`, `profile-subscription` (`[data-premium][data-plan][data-source]`), `profile-plan` (the plan's name), `profile-renewal-label` ("Renewal date" / "Ends on"), `profile-renewal` (the date, `[data-cancelled]`), `profile-grant`, `profile-plan-choose` (opens the plans modal), `profile-cancel-open`, `profile-billing-manage` (Stripe's portal, Stripe customers), `profile-dev-premium-form`, `profile-dev-premium` (dev mode: grant / revoke Plus), `plans-dialog` (open on `?plans=1` or `?upgrade=1`), `plans-form`, `plan-card` (`[data-plan][data-price][data-current]`), `plan-pick` (its radio), `plan-current`, `plans-payment`, `plans-method` (with Stripe or dev mode's stand-in: the "Pay with" radiogroup), `plans-method-card`, `plans-method-stripe` (Stripe Checkout, any plan), `plans-card-fields` (the card inputs; hidden while Stripe is picked), `plans-card-name`, `plans-card-number`, `plans-card-expiry`, `plans-card-cvc`, `plans-postal-code`, `plans-terms`, `plans-purchase`, `plans-cancel`, `plans-close`, `cancel-dialog`, `cancel-form`, `cancel-note`, `cancel-reasons`, `cancel-reason` (each radio), `cancel-feedback`, `cancel-keep`, `cancel-confirm`, `profile-ai-usage` (*Skrongle AI this month*, Plus only, `[data-hosted][data-offered][data-level]`), `profile-ai-usage-info` (its (i)), `profile-ai-usage-dialog`, `profile-ai-usage-close`, `profile-ai-bar` (the bar: `aria-valuenow` the percent used, `[data-used][data-left]`), `profile-ai-left` ("83 % left"), `profile-api-keys` (`[data-plus-ai][data-own-keys]`), `profile-own-keys-form`, `profile-own-keys-switch` (Plus: the pill "Use my keys when my Skrongle AI limit is hit"), `profile-own-keys-body` (the key rows; hidden while the pill is off), `profile-api-key` (a `<details>` row), `profile-api-key-toggle`, `profile-api-key-status`, `profile-api-key-error`, `profile-api-key-input`, `profile-api-key-remove-form`, `profile-api-key-remove`, `profile-dev-premium-form`, `profile-dev-premium` (dev mode), `email-verified` (the ✓), `email-unverified` (the red ! button), `email-unverified-dialog`, `email-unverified-close`, `email-verify-resend` (Resend, in that dialog) (the address's state), `profile-phone`, `profile-phone-label`, `profile-phone-country` (the `+1` in the field's left edge), `profile-phone-invalid` (the red ! saying what is wrong with what is typed: it waits for a break in the typing, and takes the corner from the two below while it shows), `phone-problem-dialog` (the dialog it opens), `phone-problem-why` (the message in it), `phone-problem-close`, `profile-phone-error` (the same message for a screen reader; not shown on the page), `phone-verified` (✓), `phone-unverified` (the red !), `phone-unverified-dialog`, `phone-verify-send`, `phone-verify-code`, `phone-verify-submit`, `phone-verify-error`, `phone-verify-close`, `profile-two-factor` (the card), `profile-two-factor-info`, `profile-two-factor-info-dialog`, `profile-two-factor-form`, `profile-two-factor-method` (a chip), `profile-two-factor-error`, `profile-two-factor-email-unavailable`/`-sms-unavailable` (red ! by a chip not offered yet) and their dialogs `profile-two-factor-email-dialog` (`profile-two-factor-email-resend`), `profile-two-factor-sms-dialog` (`profile-two-factor-sms-add`, `profile-two-factor-sms-send`), `two-factor-totp-dialog`, `two-factor-totp-qr` (the inline SVG; `data-modules` its width), `two-factor-totp-secret`, `two-factor-totp-qr` (the QR's wrapper; `data-uri` is the `otpauth://` link it encodes), `profile-2fa-totp-qr-info` (the (i) by the dialog's title) and `profile-2fa-totp-qr-info-dialog`, `two-factor-totp-dev` (dev mode), `two-factor-totp-form`, `two-factor-totp-code`, `two-factor-totp-confirm`, `two-factor-totp-cancel`, `two-factor-totp-error`, `profile-two-factor-totp` (the added app's line), `profile-two-factor-totp-remove`, `two-factor-confirm-dialog` (Confirm it's you: any two-factor change), `two-factor-confirm-what`, `two-factor-confirm-form`, `two-factor-confirm-method`, `two-factor-confirm-password` (or `two-factor-confirm-code` and `two-factor-confirm-send`, `two-factor-confirm-sent` for an account without a password), `two-factor-confirm-submit`, `two-factor-confirm-cancel`, `two-factor-confirm-error` | `profile-save-status[data-state]` (`saved`, `saving`, `error`), `[data-saved]` on a saved field's `.field-wrap`, label or chip, `profile-api-key[data-provider][data-key-set][data-unreadable]`, <!-- themes -->`profile-theme[data-theme-id]`, <!-- /themes -->`profile-nav-link[data-section]` (the id it jumps to), `profile-two-factor[data-method]` (`''`, `sms`, `email`, `totp`), `profile-two-factor-method[data-method]` |
| `/superuser` (superusers only; `nav-superuser` in the account menu) | `superuser-page`, `superuser-info` + `superuser-info-dialog` (`superuser-signup-mode[data-mode]`), `superuser-notice`, `superuser-inbox` (dev mailbox), `superuser-error`; in invite mode `superuser-invites-card`, `superuser-invite-form`, `superuser-invite-make`, `superuser-invite-new`, `superuser-invite-link`, `superuser-invite-copy`, `superuser-invites`, `superuser-invite-row`, `superuser-invite-revoke`; `superuser-accounts-card`, `superuser-accounts`, `superuser-account-row[data-superuser]`, `superuser-account-temp-password`, `superuser-account-two-factor-off`, `superuser-account-reset-verification` (each `[data-account-email]`), `superuser-pager` (`superuser-prev`, `superuser-page-of`, `superuser-next`; only past 25 accounts), `superuser-password-dialog` / `-form` / `-confirm`, `superuser-two-factor-dialog` / `-form` / `-confirm`, `superuser-verify-dialog` / `-form` / `-confirm`, `superuser-temp-password-dialog`, `superuser-temp-password-new`, `superuser-temp-password`, `superuser-temp-password-copy`, `superuser-temp-password-close` | `superuser-account-row[data-superuser]`, `superuser-signup-mode[data-mode]` |

Practice and Guide stages name the button that appears after Check
(`quiz-next`, `stage-next-step`) and grade it on the feedback
(`quiz-feedback[data-state=right|wrong]`, `stage-feedback[data-state]`).
The end of a round carries the score it sent to the server:
`quiz-summary[data-correct][data-total]`,
`stage-done[data-correct][data-answered][data-passed]` and
`sprint-play-done[data-correct][data-answered]`; History's
`history-game[data-correct][data-total]` is what the server recorded.

The other pages follow the same pattern, with the page as the area:
`home-`, `map-`, `guide-`, `stage-`, `sprint-`, `sprint-play-` (phone
session), `quiz-`, `listening-`/`listen-`, `vocabulary-`, `flashcards-`,
`history-`, `games-`, `matching-`, `drill-setup-`/`drill-` (paradigm),
`numbers-`, `dates-`, `tutor-`, `home-`,
`landing-`, `docs-`. Every list of lessons carries `data-lesson-id` and
`data-passed`. `data-lesson-id` is the lesson's slug, as in
`/lessons/{slug}`. Grep `templates/` for the exact names:
`grep -o 'data-testid="[^"]*"' templates/quiz.html`.

When you add markup, give it testids in this pattern and add any page the
creation flow or lesson tests use to the table. What Jev and the
exploratory checks rely on (see [Exploratory testing](#exploratory-testing)):

- **A testid on each link or button, not only on its container.** The
  navigator labels an action with the nearest testid, so a card whose ten
  links share the card's testid gives ten destinations one name. The home
  page uses `home-<mode>-open` for a card's heading link (`home-study-open`)
  and `home-start-<mode>` for its button; lists use `<area>-item` plus
  `<area>-item-link`; links inside rendered docs are `docs-link[data-slug]`.
- **Content and live results inside `<main>`.** A panel that shows up after
  an action (answer feedback, a results screen) goes in `<main>`, and a
  finished stage is emptied, not just hidden: `innerText` of a hidden
  element returns its whole text.
- **`aria-current="page"`** on a link to the page you are on. `base.html`
  sets it on every link whose path matches the page; the Study and Play mode bars,
  docs sidebar and Sprint mode tabs set it in the template.
- **State in attributes.** A picked tile or option has `aria-pressed`, a
  graded or used-up one `data-state` (`right`, `wrong`, `correct`, `used`,
  `done`), as well as any class the CSS uses. A state shown only as an icon
  (🔒, ✓) also goes in the accessible name: the Sprint mode path buttons read
  "Ser vs. Estar — locked".
- **A short `aria-label` that names the destination** when the visible text
  runs together or is a symbol: "Open Study mode", <!-- themes -->"Use the Rustwake theme",<!-- /themes -->
  "Switch course (now Spanish)", "Type é". A `title` that only repeats the
  text is noise; the navigator ignores `title` when there is an `aria-label`.
- **Modals with `showModal()`.** While a `dialog:modal` is open the
  navigator offers only the controls inside it (the page behind is inert)
  and shows the dialog's text first. A see-through checkbox laid over its
  own mark (the review's − "leave out") is offered: opacity alone does not
  hide a checkbox or radio. A typed box is blurred after the text goes in,
  so a box that takes its value on blur (the editor's rename boxes) has it
  before the next step.

## The test course

`data/test-courses/test-course/` is a course for automated tests only. It
uses every feature a course file can switch on, one lesson per feature: each
quiz type, each Sprint mode exercise, Listening, the paradigm drill, Date Dash, the Guide (hand-ordered and derived)<!-- themes -->, every
installed theme's material<!-- /themes --> and the legacy field names. A browser test can
then open one lesson and know what it will get, instead of playing through
Spanish. Its `AUTHORING.md` (in that folder)
maps features to lessons and says what each lesson's Sprint mode session always
contains.

It is offered (as a ready-made course, `test-course`) only when the server
runs with `TEST_COURSE=1`: `./.dev-start --test-course`, and always in
`scripts/e2e.sh`. Without it the course is never loaded, and the deploy
scripts don't copy `data/test-courses/`. Open it with
`/dev/login?open=test-course`; in a spec, `devLogin(page, "open=" + TEST_COURSE)`
and `correctAnswers(TEST_COURSE)` from `e2e/lib/app.ts`.

**Adding a feature that course files can use means adding it to the test
course and to the e2e specs that use it, in the same change.** `cargo test
test_course` (`src/test_course.rs`) fails until the course uses every
property and enum value of `docs/course.schema.json`, every
`#[serde(alias)]`, every quiz type, every Sprint mode exercise kind and Guide
step (new enum variants don't compile until they are named there)<!-- themes -->, and has
complete material for every installed theme<!-- /themes -->. Features with no field of their
own are listed in its `AUTHORING.md`.

## Browser tests

```sh
scripts/e2e.sh                          # exact specs; Jev specs too when a key is set
scripts/e2e-jev.sh                      # only the Jev tests; fails if there is no key
scripts/e2e-jev.sh find-generate-form   # one journey, by id
scripts/e2e-explore.sh impatient       # a profile explores and reports (never fails)
scripts/e2e.sh --grep "reading check"   # any Playwright option works with e2e.sh
npx --prefix e2e playwright show-report e2e/results/report   # after a failure: steps, trace, screenshot
```

What the specs cover and miss, and which files CI runs: the
[E2E coverage audit](e2e-audit) (`python3 scripts/e2e-audit.py --write`
refreshes its numbers).

[Playwright](https://playwright.dev) drives headless Chromium against a fresh
server, started the way `scripts/smoke.sh` starts one: its own `./e2e.db`, a
free port, the seeded dev user (with the simulator's keys) and model calls
answered by [the dev AI provider](#the-dev-ai-provider) from the fixtures, so a running
`./.dev-start` is left alone. All of it lives in `e2e/` (Node 22+, dev-only):
nothing from there is in the app binary or the Pi deploy. The first run
installs `e2e/node_modules` and Chromium (network once); after that it runs
offline. `npm --prefix e2e run typecheck` checks the TypeScript.

| File | What it is |
|---|---|
| `e2e/specs/exact.spec.ts` | Exact specs, no model in the loop: generate a course from the fixtures and open it; pass a lesson's reading check (answers read from the lesson files); finish a Sprint mode session; the profile has no Dev section for anyone (the seeded keys answer generate and the tutor through the simulator); a key the simulator never issued gets the provider's 401; the picture and password dialogs (a pick saves at once and shows in the heading, a mismatched or wrong password keeps the dialog open) |
| `e2e/specs/profile-autosave.spec.ts` | The profile saves as you edit: one request a second after typing stops, the circle's states, a field that needs attention (red at once, not sent, the leave dialog and the browser's prompt), a refusal from the server, a lost connection that saves once back online<!-- themes -->, a theme worn without a reload<!-- /themes -->, API keys folded away and saved on paste or 3 s after typing, and the green check on a saved field |
| `e2e/specs/pathways.spec.ts` | Exact specs for the essential flows of #36: switch course (Spanish, a generated course, back) by `nav-course-switch[data-course-id]` and `courses-card[data-active]`; generate from notes and uploaded files (`generate-notes`, `generate-files`); cancel a slow generation and resume it to a ready course; resume a run stopped on a lesson (the resumed job keeps what was written), then write that lesson by hand; open the editor between the outline and lesson 1 and see its modal follow the job to the end; remove an API key (`profile-api-key[data-key-set]` flips back). Runs in CI with the exact specs |
| `e2e/specs/editor-autosave.spec.ts` | The course editor saves as you edit (#151, #190): no Save button; a lesson's JSON sent 3 s after the last keystroke (on Playwright's clock) or when its drawer loses focus, or at once when the tab is hidden; red with the reason for JSON that doesn't parse and for a table the server refuses (`422`), nothing stored, the text kept, the leave dialog; a failed save retried; a tab that is behind another refused (`409`) and asked to reload; Use AI saves what waits first and the locked editor sends nothing. Other specs make the waits short with `fastAutosave(page)` (`e2e/lib/app.ts`: it sets `window.AUTOSAVE_TEST_MS` before the page's scripts run) and wait with `editorSaved(page)`. Runs in CI with the exact specs |
| `e2e/specs/forms.spec.ts` | Forms mode in the lesson table's drawer: a lesson in the lesson editor (add a word from the outline, a multiple-choice question with the right-answer radio, the dashed new-option box, reorder, remove, the type switch), see them as JSON, a JSON edit shown back in the form, saved as typed, reload, the table and the file; a field the editor doesn't know kept; the JSON switch; the course settings as a schema form (a new unit becomes a folder) at phone width. Runs in CI with the exact specs |
| `e2e/specs/lesson-editor.spec.ts` | The lesson editor (#841), against a copy of the test course's "Everything at once" lesson: the outline's sections, items and badges and which preview modes show each kind; a multiple-choice question (picked answer, dashed new-option box, retyping keeps the answer, the type switch to true-false and numeric and back, ↑ ↓, saved to the file with the rest untouched); checks as a mark in the field's corner (a modal holds the text; clearing an explanation moves nothing below it, and the mark stays on its label's row in a two-column row, #1320), in the outline and from the server's refusal (a word renamed carries into the questions that teach it; a heading renamed breaks a Guide step, and the line under the title opens the Guide on it); Try it in Study, Guide and Sprint through `/api/courses/{id}/preview-answer`; the Sprint exercises (buckets and ◂ ▸, the wrong word by a click, tiles and distractors, the sort played in the preview); words, listening sentences, conversation lines and a conjugation table; add, duplicate and delete from the outline, Guide steps by hand and back to derived, prerequisites; the Outline / Edit / Preview tabs at phone width. Runs in CI with the exact specs |
| `e2e/specs/lesson-editor-rules.spec.ts` | The lesson editor's forms against the course rules (#1200), each path ending on a save the server accepts: a type switch to numeric or expression with an answer the checker cannot read (`/api/courses/{id}/answer-key`), the option that was the answer deleted, a word a question<!-- themes --> and a theme<!-- /themes --> name<!-- no-themes -->s<!-- /no-themes --> deleted, a question the Guide names deleted, a duplicated word, question and conjugation, a sort's bucket and item limits and a deleted bucket's items, a spot's word that appears twice and a word in brackets, a conversation's empty line and its two-line least, a Guide word count that is no whole number, an optional box of spaces. Runs in CI with the exact specs |
<!-- themes -->
| `e2e/specs/custom-themes.spec.ts` | Themes learners make, seeded through `POST /dev/theme`: wear one (colours, emblem, themed owl), delete it back to Plain; invisible to another learner and to no session (404 pictures, can't wear or delete); validation errors and the five-themes limit; the picker at phone width. Runs in CI with the exact specs |
<!-- /themes -->
| `e2e/specs/words-due.spec.ts` | Words due (#1360), seeded through `POST /dev/vocab`: no card until words are due; the card's count, three words, (i) and button; the due round (a miss comes back once at the end, the summary's "Back tomorrow" and "Next" lines); the card gone after; the card stacked on a phone. Runs in CI with the exact specs |
| `e2e/specs/dev-payments.spec.ts` | [The dev payment provider](#payments-the-dev-payment-provider): a new learner subscribes through the fake Checkout (Cancel and Fail first), the profile shows the plan and `/dev/payments` the webhook rows; a failed renewal shows as past due on the profile; Cancel now makes the plan free; an event sent again with a broken signature, and a forged webhook, get the app's 400 |
| `e2e/specs/dev-ai.spec.ts` | [The dev AI provider](#the-dev-ai-provider): a course generated from the dialog with the seeded key shows in `/dev/ai`'s log; an edited fixture, a used-up limit, a request limit and an injected timeout reach the learner; the wire API refuses unknown keys; Manual replies holds a request, folded at the bottom of `/dev/ai`, for an answer typed there |
| `e2e/specs/hosted-ai.spec.ts` | [Skrongle AI](#skrongle-ai-the-servers-key-in-dev) on the simulator's platform key: a Plus learner with no key tutors and generates on it and the month's bar goes down (never a dollar amount); the platform key's limit reaches the learner as "Skrongle AI is not available right now"; Free is not offered it (tutor, Generate dialog, API, no profile card); `POST /billing/credits` is 400 without Stripe |
| `e2e/specs/subscription.spec.ts` | [Subscriptions](features#subscriptions): a new learner buys Plus in the plans modal (the two cards, no card fields, Pay with Stripe, Pay on the dev Checkout page) and the card shows the plan and "Renews on …"; the old `/billing/subscribe` answers 404; the Skrongle AI bar and the own-keys pill appear (Plus only), the pill shows and hides the key rows and persists; Cancel with a reason and feedback keeps Plus with "Ends on …" and no Cancel button; choosing again opens the Billing Portal, whose Renew takes it back. Basic: no bar, no pill, *Upgrade to Plus*, its card marked as the current plan; `?plans=1` opens the modal, and Plus from there opens the Billing Portal |
| `e2e/specs/mail.spec.ts` | The dev mailbox: a new account's confirmation e-mail arrives over SMTP, opens in the dev inbox, and the link in its HTML confirms the address (the profile's red ! turns into a check); the outbox's test message opens from `/dev/mail`; the table (#223): new mail appears without a reload, bold until opened, Archive strikes out and sinks, Delete removes, the tabs switch in place; the Text section (#225): texts sent from its outbox reach the dev phone, one row per number, bold until opened, archive, delete, a conversation page. Runs in CI with the exact specs |
| `e2e/specs/terms.spec.ts` | Terms of Service: `/terms` is public, registration is refused without the checkbox (`register-terms`, which `register()` in `e2e/lib/learner.ts` ticks) and records the version, the gate holds an account with an older version (`/dev/login?terms=old`) until it accepts. Runs in CI with the exact specs |
| `e2e/specs/docs.spec.ts` | The docs (#97): `/docs` logged out, its two sidebar sections (`docs-section[data-section=using|working]`), every user guide as HTML and raw Markdown, `/llms.txt` naming the course format and schema (every link it lists answers 200), no page wider than a phone, every in-app link and `#heading` on every page resolves. Runs in CI with the exact specs |
| `e2e/specs/i18n.spec.ts` | [Languages](i18n): the picker on `/register` sets the `lang` cookie and the page comes back in Spanish, the account keeps the choice (it wins over a cleared cookie at the next sign-in), the profile's picker (`profile-lang`) saves at once and reloads in English; a browser asking for `es-MX` gets Spanish before any pick, `fr` gets English, `POST /lang` refuses an unknown code. Runs in CI with the exact specs |
| `e2e/specs/lesson-tables.spec.ts` | Lesson pages fit a phone (390 px, #465): the test course's lessons with tables and five Spanish lessons with wide paradigm tables are no wider than the screen; Markdown tables (`table-scroll`), the vocabulary (`lesson-vocabulary-scroll`) and the paradigm table (`lesson-conjugations-scroll`) scroll inside their own box. Runs in CI with the exact specs |
| `e2e/specs/course-copy.spec.ts` | Copy a course (#629): Copy on the Spanish card opens *Copy of Spanish* in the editor with every file of the folder, lists it beside the untouched original, and a copy of the copy is *Copy of Copy of Spanish*; a fresh account copying Spanish until its 32 MB are full gets the "no room for a copy" notice and no extra course |
| `e2e/specs/course-translate.spec.ts` | Copy and translate (#630), from `translate-course.json`: a fresh draft translated to Español through the dialog behind the icon in the course's settings (what is translated behind its (i)) is followed to its end in the editor and comes out as *Untitled course (Español)*, ids kept and the answer still one of the options, the original untouched; the Spanish card's dialog offers no English |
| `e2e/specs/modals.spec.ts` | Closing by clicking outside: a Sprint mode stop's sheet (`sprint-sheet`) closes on a tap outside the stop and on Escape, a tap inside keeps it (#233); modals close on a click on their backdrop (#226, `static/modal.js`): the profile's dialogs (not on a drag out of them, not with a password typed), the Generate dialog (not with a wish typed), not a running job's box (`data-modal-static`), Review AI changes by its Close link. Runs in CI with the exact specs |
| `e2e/specs/smoke/` | [Smoke specs](#smoke-specs): a new account passes lessons in every mode, answering from the course files; the whole test course, right and wrong |
| `e2e/specs/record.spec.ts` | The recorder behind `scripts/e2e-record.sh` ([Recording a smoke spec](#recording-a-smoke-spec)); skipped otherwise |
| `e2e/journeys.ts` | **The list of journeys**: goals Jev walks toward. Append an entry to add one |
| `e2e/personas/<id>.ts` | **User profiles**, one per file, for exploratory runs and for journeys walked "as" someone (see [Exploratory testing](#exploratory-testing)) |
| `e2e/specs/journeys.spec.ts` | One test per journey, `journey <id>`, tagged `@jev` |
| `e2e/specs/judged.spec.ts` | Jev-judged statements about a page, each reporting its probability |
| `e2e/specs/explore.spec.ts` | One exploratory run (or replay) started by `scripts/e2e-explore.sh`; skipped otherwise |
| `e2e/lib/app.ts` | Exact helpers: `devLogin`, `devReset`, `correctAnswers`, `playSprintSession`; `TEST_COURSE` and `courseDir` for [the test course](#the-test-course) |
| `e2e/lib/key.ts` | `AnswerKey`: the right answer to a question id or a Sprint mode exercise id, read from the course files |
| `e2e/lib/answer.ts` | `answerQuestion` (Study and Play, by `data-qid`), `passReadingCheck`, `playPractice`, `playStage` |
| `e2e/lib/sprint-play.ts` | `playSprintRight`: a Sprint mode session played with the right answers; reports the exercise kinds it met |
| `e2e/lib/mail.ts` | The dev mailbox: `waitForMail`, `inbox`, `mailLink` ([E-mail: the dev mailbox](#e-mail-the-dev-mailbox)) |
| `e2e/lib/learner.ts` | A fresh account (`newAccount`; `signUp` fills `/register` and waits for "check your mail" (in the mini lane, with no mail, for the signed-in page; it returns whether it signed in); `register` signs up, signs in where needed and opens a course; `login`, `logout`), `/api/progress`, and the check that every page agrees with it |
| `e2e/lib/fixtures.ts` | The smoke specs' `test`: seeded `Math.random` and a fake voice in every page; prints the seed on failure |
| `e2e/lib/jev.ts` | `judge(state, statement)`, `choose(state, instructions, options)`, `judgeAndChoose`, usage and cost totals |
| `e2e/lib/navigate.ts` | The navigator that runs a journey |
| `e2e/lib/persona.ts` | The `Persona` type and `loadPersona(id or path)` |
| `e2e/lib/explore.ts` | The explorer: exact bug signals, judgements and page checks on each page a profile reaches, findings with evidence, replay |
| `e2e/lib/explore-report.ts` | The report: ranking, diff with the previous run, proposed tests, `report.md` |
| `e2e/results/` | Last run: `last-run.json`, the HTML report, traces, `journeys/<id>.json`, `explore/<profile>-<timestamp>/` (ignored by git) |

### Run the Jev tests

1. **Key, once.** Create one at [console.typesafe.ai/keys](https://console.typesafe.ai/keys)
   and add it to `./.env` or `e2e/.env` (both git-ignored):
   `TYPESAFE_API_KEY=…`
2. **Run.** `scripts/e2e-jev.sh`. It builds the app, starts a fresh server
   with the seeded dev user and LLM fixtures, runs every journey and judged
   spec headless, and stops the server. The first run also installs
   Playwright and Chromium.

   | Command | Runs |
   |---|---|
   | `scripts/e2e-jev.sh` | every journey and judged spec |
   | `scripts/e2e-jev.sh open-lesson start-sprint` | those journeys, by `id` from `e2e/journeys.ts` |
   | `scripts/e2e-jev.sh --judged` | only the judged specs |
   | `scripts/e2e-explore.sh <profile>` | an exploratory run (see [Exploratory testing](#exploratory-testing)); `e2e-jev.sh --explore` forwards there |
   | `JEV_DEBUG=1 scripts/e2e-jev.sh open-lesson` | also prints the actions offered at each step and every judged state |
   | `scripts/e2e.sh` | the exact specs too; the Jev tests are skipped, not failed, without a key |

3. **Read the result.** Each journey prints its steps as it goes:

   ```
   journey start-sprint: the journey's exact check passed
      0. /  done=0.24  → Click the link "Start a session" [home-start-sprint] (conf 0.98)
      1. /sprint  done=0.36  → Click the link "🏷️ Continue Nouns: gender, articles and plurals ›" [sprint-continue] (conf 0.77)
   Jev: 8 calls, 13256 input tokens, ≈ $0.00056
   ```

   A failed journey or judgement puts the same log (or the probability) in
   the error. Afterwards, `e2e/results/journeys/<id>.json` has each journey's
   steps, calls, tokens and time, and
   `npx --prefix e2e playwright show-report e2e/results/report` has
   screenshots and a trace of every failure.
4. **Add a journey.** Append an entry to `e2e/journeys.ts` (see
   [Journeys](#jev) below) and run it alone with `scripts/e2e-jev.sh <id>`.

### Jev

[Jev](https://docs.typesafe.ai/introduction) (TypeSafe's "System One" model)
takes a **state** (text or JSON) and typed questions: **Noul** (how true is
this statement, 0–1), **Choice** (pick one of up to 255 options) and
**Score** (rate on a rubric). It answers with calibrated probabilities in
70–500 ms. It doesn't write text and doesn't drive a browser, so here
Playwright acts and Jev decides or judges.

**The key.** Create one at [console.typesafe.ai/keys](https://console.typesafe.ai/keys)
and put it in `e2e/.env` or the repo's `./.env` (both git-ignored; see
`e2e/.env.example`), or export it. `scripts/e2e.sh` reads both files, for the
test runner only; the app server never sees them.

| Variable | Default | Meaning |
|---|---|---|
| `TYPESAFE_API_KEY` (or `JEV_API_KEY`) | unset | Switches the `@jev` specs on; without it they are **skipped**, not failed |
| `JEV_MODEL` | `jev-1.13.0` | Pinned, so thresholds don't drift when `jev-latest` moves |
| `JEV_PASS` | `0.8` | A judged statement or a journey's `done` counts as true at or above this |
| `JEV_LOW_CONFIDENCE` | `0.2` | A navigation step below this Choice confidence is marked `guess` in the step log |
| `TYPESAFE_BASE_URL` | `https://api.typesafe.ai` | Point the SDK at another server (a stub, say) |
| `JEV_DEBUG` | unset | Print every judged state and the actions offered at each journey step |
| `JEV_FLAG` | `0.5` | Exploration: a concern under this, or a frustration at or above it, is a finding |
| `JEV_EXPLORE_PAGES` | `15` | Exploration: the most pages judged in one run |

App-side model calls in these tests still go to the dev AI provider
(the fixtures), never Jev.

**Journeys.** A journey is a goal in plain words plus how to tell it's
reached. From `start`, each step reads the page (the visible text of
`<main>`, which holds the page's own content, Sprint mode's answer sheet and
results included, but not the header, nav or footer; the `data-*` state of every testid, the enabled links, buttons and
inputs, each labelled by its `aria-label`, else its text plus a `title` that adds to it) and asks Jev in one call whether the goal is reached (Noul on
`done`) and which action comes next (Choice; the options are "Click the
button "Check" [sprint-play-check]" and so on, plus "Go back"). Playwright performs
it. Logout and delete buttons are never offered, nor is anything hidden;
a closed menu is offered as "Open the menu …" (its `<summary>`), and links
marked `aria-current="page"` (a link to the page you are on) are left out.
State comes from attributes, never classes: `aria-pressed="true"` or
`aria-selected="true"` is labelled `[selected]`, `data-state="right|wrong"`
`[marked right]`/`[marked wrong]`, and used-up tiles (`data-state="used|done"`,
`aria-disabled="true"`) are left out. An action that left the page unchanged is marked `no effect` and not
offered again on that page, and an action that already led away from a page
twice is replaced by Jev's next most likely one (`loop guard` in the log). Jev can't write, so text it
may type comes from the journey's `inputs`. The journey passes when its
exact `check` holds (or, without one, when Jev judges `done` ≥ `JEV_PASS`);
it fails after `maxSteps`, or when the page hasn't changed for 4 steps, with
the step log as the error (illustrative):

```
journey find-generate-form: goal not reached in 10 steps
   0. /  done=0.03  → Click the link "🤖 AI tutoring Chat with a tutor" [home-mode] (conf 0.35)
   1. /tutor  done=0.07  → Go back (conf 0.47)
   …
```

Each run also writes `e2e/results/journeys/<id>.json` with the steps, the
calls, the tokens and the time. To add a journey, append to `e2e/journeys.ts`:

```ts
{
  id: "find-generate-form",           // test name "journey find-generate-form"
  login: "keys=fake",                 // /dev/login query (default "open=spanish")
  start: "/",
  goal: "Get to the form where you describe a course you want and the AI writes it for you.",
  done: "The page is a form for asking the AI to generate a new course.",
  check: (page) => page.getByTestId("generate-form").isVisible(),  // exact pass/fail
  inputs: { "generate-wish": "Fractions for Friday" },             // testid (or "*") -> text
  maxSteps: 10,
}
```

`setup` (optional) runs after logging in and before Jev, and may return the
path to start from. The course-editor journeys use it to make the dev user a
course of their own from the fixtures (`ownCourse`, units Basics and
Operations) and start on its page: `add-lesson-with-ai` (Use AI, Only add,
until the modal says it is done), `edit-lesson-as-form` (find the **JSON**
switch, change a lesson's summary in the form, save), `add-lesson-to-unit`
(a folder's **+**, name it, save; checks the unit). `name-new-course`
creates a course, closes the Generate dialog and renames it. They avoid the
editor's destructive buttons (`EDITOR_AVOID`).

### Exploratory testing

Jev explores the app as a **profile** (a user persona), collects bugs and
the moments that user would find unclear or frustrating, and writes a report
that outlines new e2e tests. It finds what nobody wrote a journey for.

```sh
scripts/e2e-explore.sh --list                            # the profiles in e2e/personas/
scripts/e2e-explore.sh impatient                         # explore as that profile, all its goals
scripts/e2e-explore.sh classroom --goal "find help with ser vs estar"   # a goal of your own
scripts/e2e-explore.sh classroom --goal 1                # the profile's goal 1 (0-based)
scripts/e2e-explore.sh testing-bot --budget 80           # cap the steps (and so the cost)
scripts/e2e-explore.sh impatient --wander 2              # then 2 sections Jev picks itself
scripts/e2e-explore.sh --persona-file draft.ts           # a profile that isn't in the folder yet
scripts/e2e-explore.sh --replay e2e/results/explore/<run>/report.json   # rerun a route, no Jev
JEV_DEBUG=1 scripts/e2e-explore.sh accurate              # also print each step's options
```

| Option | Meaning |
|---|---|
| `--goal <words or n>` | One goal instead of the profile's list: a goal of your own, or the profile's goal number `n` |
| `--budget <steps>` | Total steps for the run; each goal gets the profile's `patience` or what's left |
| `--max-calls <n>` | Stop calling Jev after `n` calls (default 300) |
| `--wander [n]` | After the goals, Jev picks `n` (default 2) sections it hasn't visited, a Choice weighted by `prefers`, and tries each one |
| `--persona-file <path>` | Run a profile from any `.ts` file (import the types from `e2e/lib/persona.ts` by path) |
| `--replay <report.json>` | Redo a recorded run step by step with **no Jev calls** (see below) |
| `--fail-on <kinds>` | Exit 1 when there is a finding of these kinds (`bug`, `content`, `unclear`, `frustrating`, `testability`); without it the run always exits 0 |

The server is set up as for `scripts/e2e.sh`: a fresh `e2e.db`, the seeded dev
user, LLM fixtures (the app never makes a live model call), a free port. The
key comes from `./.env` or `e2e/.env`; a missing key is an error, except for
`--replay`. Logout and delete buttons are never clicked, nor anything in
the profile's `avoid`. Puzzles stay with the exact drivers: a goal's `then`
hands Sprint mode to `playSprintSession`.

#### Profiles

One file per profile in `e2e/personas/<id>.ts`, whose default export is a
`Persona` (`e2e/lib/persona.ts`). The runner finds it by id, so **adding a
profile means adding a file**, nothing else. `e2e/journeys.ts` can also walk
a journey "as" a profile (`persona: "<id>"`). Each field is written for the
question types Jev answers well:

| Field | Becomes |
|---|---|
| `id` | The file name, the runner's argument, the report folder's name |
| `who` | A `who` field in every step's state, so each Choice is made as this person |
| `prefers` | Added to the Choice instructions: tie-breakers between actions, and between sections with `--wander` |
| `patience` | Steps per goal; after `patience / 2` steps with no effect the profile is **lost** and gives up |
| `viewports` | `phone` (390×844) or `desktop`; with both, goals alternate, phone first |
| `goals` | Goals like journeys (`goal`, `done`, optional `start`, `check`, `inputs`, `topic`) plus `then`: exact work after the goal is reached |
| `concerns` | Noul statements that should hold on the pages visited; **a low p is an `unclear` finding** |
| `frustrations` | Noul statements about what would make this person leave; **a high p is a `frustrating` finding** |
| `checks` | Exact page checks, no model: `labels`, `class-state`, `hidden-menus`, `error-role` (testability) and `overflow` (a bug; always on at phone width) |
| `items` | Content audit: rows of a table (say `lesson-vocab-item`) judged one by one; **a low p is a `content` finding** |
| `avoid`, `inputs` | Testids never to touch; what to type (testid or `"*"` → text). A goal's `inputs` win |

Statements follow the judged-spec rules: one idea each, plain words, judged
on the page text with each run of one- and two-character lines joined into
one line (a row of accent buttons becomes one line of noise; a stat tile stays
"0 XP"). A statement that only makes sense on some pages gets `on` (a path
pattern) or `when` (a testid that must be visible):
`{ text: "The page shows the correct answer.", when: "sprint-play-result" }`. A
statement naming `` `topic` `` is judged only on goals with a `topic`, and
one naming `` `actions` `` gets the page's action labels in its state.

| id | Who | Walks | Looks for |
|---|---|---|---|
| `impatient` | A teenager on a phone, mostly Sprint mode, skims everything | patience 6, phone | start and finish a Sprint mode session, come back to it, see XP/streak; walls of text, typing first, dead taps, a page wider than the screen |
| `accurate` | A serious adult who reads every explanation and loses trust at the first wrong fact | patience 20, desktop | reads a lesson and its vocabulary, practises and reads the feedback, browses the vocabulary list; each vocabulary row's meaning, accents and ¿¡, feedback that explains |
| `classroom` | A student bringing a topic from class (`CLASS_TOPICS`) | patience 10, phone then desktop | finds the lesson for each topic, judges that it's about the topic and can be practised alone. A topic no route reaches is a coverage finding (as "numbers over 100" was until #491 added the A2 lesson) |
| `testing-bot` | An automated tester that sees only markup | patience 12, desktop | walks as many pages as it can; unlabelled buttons, state only in classes, links behind a closed menu, errors that aren't `role=alert` |

To draft a profile, copy one to anywhere, change the import to
`import type { Persona } from "<repo>/e2e/lib/persona"`, run it with
`--persona-file`, and move it into `e2e/personas/` when it's useful. The same
`--goal` under two profiles compares routes: the report's journey proposal
says "reached in 3 steps; other profiles: classroom in 9".

#### What a run does

For each goal (then each `--wander` pick), the profile walks with the
navigator, and every page it reaches is observed in two layers:

- **Exact signals, no model: `bug` findings.** HTTP ≥ 400 on a navigation; a
  fetch, XHR or asset that returns ≥ 400 or fails; console errors and
  uncaught page errors; a page wider than a phone screen; an action with no
  effect (not counting a link to the page you're on, or re-picking the option
  already picked); a walk that is stuck or a profile that gets lost.
- **Jev judgements.** On each page it hasn't seen (its path plus the set of
  visible testids), one batched call for the concerns and frustrations that
  apply, and one per table for `items`, up to `JEV_EXPLORE_PAGES` pages. A
  concern under `JEV_FLAG` is `unclear`, a frustration at or above it is
  `frustrating`, an item under it is `content`. The profile's markup checks
  are `testability`.

Each finding keeps **evidence** from where it was first seen: the step log
up to then, a screenshot, the URL, the visible testids and their `data-*`
state, and Jev's answer. Findings are **deduplicated**: an exact bug by its
signal, page and message; a judgement or check by its statement or message,
listing every page it showed on. Digits are folded, so "51px wider" and
"53px wider" are the same finding.

#### Reading the report

The terminal gets a summary; the folder
`e2e/results/explore/<profile>-<timestamp>/` gets `report.md`, `report.json`
and `shots/` (illustrative):

```
explore classroom: 4/5 goals reached, 23 steps, 32 Jev calls, 87290 tokens, ≈ $0.00367, 8s, jev-1.13.0, commit 3f21509
findings: 1 bug, 1 unclear (vs previous run: 0 new, 0 gone, 0 not revisited)
  high   bug         On /lessons/ser-vs-estar: the page is 230px wider than a 390px screen: <th> in [lesson-conjugations], <td class="can-say"> in [lesson-conjugations] run off the edge.
  low    unclear     On /lessons/numbers-0-100, Jev doubts that "The page is about the `topic`." (p=0.21).
proposed tests: 4 journeys, 1 regression specs, 1 judged specs, 0 markup fixes
report: e2e/results/explore/classroom-20261015-191613/report.md
```

(The unclear finding was a coverage gap, since closed by #491: asked for
"numbers over 100", the profile kept landing on "Numbers 0–100" because no
lesson went further. The `find-topic-lesson` journey was promoted from this
run's first proposal.)

`report.md`, for people:

1. **Summary**: goals reached and missed, steps, Jev calls, tokens, ≈ cost,
   time, the model and the app commit (`+dirty` with local changes), then a
   table of the goals.
2. **Findings**, worst first: exact bugs (`high`), then judged findings by
   how many other profiles' latest runs hit them and how far p is on the
   wrong side (`medium` from 0.8, or when another profile hit it too), then
   testability (`low`). Each has "what the user hit" in one line, and its
   evidence. Compared with the profile's previous run, a finding is
   **new**; one whose page was visited again but didn't show is **gone**; one
   whose pages weren't visited this time is **not revisited**.
3. **Route map**: the pages in the order visited, the pages linked from the
   header or the home page that were never reached, and each goal's step log.
4. **Proposed e2e tests**, ready to review and paste:
   - each **goal reached**: a `journeys.ts` entry with a suggested exact
     `check`: the profile's own when it has one, else a `data-*` state or a
     testid that first appeared on the final page (else the path, with a
     note to add a testid);
   - each **bug**: a regression spec outline for `exact.spec.ts`, starting on
     the page before the bug with the action that triggered it (Jev's whole
     route is in a comment) and the assertion that would have caught it;
   - each **unclear, frustrating or content** finding: a judged spec for
     `judged.spec.ts` with its statement and threshold, or **"needs a
     person"**: a content fact check, or a p within 0.15 of the flag;
   - each **testability** finding: the markup change (`aria-label`,
     `data-state`, `role="alert"`, …) with the template line that holds the
     testid.

`report.json` has the same, plus every visit and judgement, for tools, for
diffing two runs, and for turning a proposal into a file.

**Replay.** `--replay <report.json>` starts the same server, logs in, and
redoes each goal's recorded steps by their action labels, observing with the
exact signals only: no Jev calls, no cost. Its report (in
`<profile>-replay-<timestamp>/`) says which of the recorded bug and
testability findings **reproduced**, which are **gone**, what is new, and
which steps could not be redone (the page changed). A reproduced bug's
regression outline is the first draft of its exact test.

#### Promoting a proposal

A Jev flag can be wrong. Look at the screenshot and the evidence first.

- **A journey**: paste the entry into `e2e/journeys.ts`, check the suggested
  `check` names the end state you mean (prefer a `data-*` state), and run it
  alone with `scripts/e2e-jev.sh <id>`.
- **A bug**: `--replay` the report to see it reproduce, paste the outline
  into `exact.spec.ts`, cut the route to the shortest one that still fails,
  then fix the app until it passes.
- **A judged finding**: try the outline in `judged.spec.ts` against the
  pinned model a few times before trusting its threshold; if it needs a
  person, decide first. A confirmed content flag is a lesson JSON fix.
- **A testability finding**: make the markup change under the
  [markup hooks](#markup-hooks) conventions, then assert it exactly.
- **Anything else confirmed** becomes an issue, or an entry in the pathway
  list (#36).

**Cost.** One call per step plus one per new page, at about 2k tokens a
call: a profile's goals cost well under a cent. `--budget` and
`--max-calls` cap it, and the report prints it.

### Determinism rules

- **Exact specs gate a change.** They need no key and no network and must
  pass. Anything that can be asserted with testids and state attributes is
  asserted that way.
- **Markup decides the result.** Jev only picks the path. Give a journey a `check` whenever its end
  state shows in markup. Then the result is exact and only the path is
  Jev's. Use `done` alone only for ends that markup can't express.
- **Judged statements** are for qualities that exact text can't capture or
  that break when copy changes ("the error explains itself in plain words").
  Each reports its probability and fails under `JEV_PASS`. The threshold is
  tuned against the pinned model; change `JEV_MODEL` and the threshold
  together.
- **Persona runs are exploratory.** They report and never fail. Only
  promoted journeys (with an exact `check`) and tuned judged specs can fail
  the suite.
- A low-confidence step is **reported, not failed**: it shows as `guess` in
  the step log. A journey fails only when the goal isn't reached.
- A failing `@jev` spec is a report to read, not proof of a bug: check the
  step log or the probability, then fix the app, the journey, or the
  threshold.

**What Jev is good at here** (measured on `jev-1.13.0`, 2026-10-15): picking
the next link or button toward a goal stated in words (the three journeys
take 2–6 steps), and judging clean, short text, where it is consistent from run
to run (0.95–0.99 on true statements). It is weak at multi-tap puzzles: a
journey that played Sprint mode exercises (match the pairs, sort into buckets)
spent 150 calls without finishing, so playing exercises stays in the exact
specs (`playSprintSession`) and journeys only get you to them. Judgements
come out lower when the text carries noise, like rows of accent buttons
(the judged spec strips one- and two-character lines), or when one
statement asks two things: ask one thing per statement.

**Cost and speed.** You pay per input token ($0.042 per million; output is
free). A navigation step sends about 2k tokens (the page text is capped at
4,000 characters). The whole `@jev` suite (three journeys, three judgements)
is about 22k tokens, ≈ $0.001, and runs in about 5 s, most of it the
browser. The run prints the calls, tokens and estimated cost.

## Smoke specs

```sh
scripts/e2e.sh specs/smoke                  # just these (they also run in every scripts/e2e.sh)
E2E_SEED=7 scripts/e2e.sh specs/smoke       # another seed; a failure prints the one to replay with
```

Cheap, deterministic browser tests of the essential paths, right and wrong, run on
every change (and by `.github/workflows/smoke.yml` on every PR, on the [laptop runners](#ci-runners), as separate checks: `cargo test`, `scripts/smoke.sh`, the exact specs and the smoke specs, each e2e suite in [shards](#ci-runners)): no Jev, no
key, no network, about three minutes. Each test registers its own
account through `/register`, so they never share the dev user, and ends
with a check that every page agrees with `/api/progress`
(`expectProgressAgrees`).

- `happy-paths.spec.ts` (Spanish): a new learner sees lesson 1 as next; passes
  it in Study (reading check, then practice), in Sprint and in Play, and it shows as
  passed on `/lessons`, `/map`, `/sprint` and `/guide` with lesson 2 open; a pass in
  one mode shows in the others; progress survives logging out and in.
- `complete-test-course.spec.ts`: a new account finishes the whole
  [test course](#the-test-course) in Study, in Sprint (every lesson's session, the
  review mix, lightning) and in Play (every stage, in order). Sprint must meet every
  exercise kind of `ALL_KINDS` in `src/test_course.rs`, and "Type it instead"
  once, so a new kind fails here until the course and the player know it. No
  console error, HTTP error or overflow on any page on the way.
- `test-course-games.spec.ts`: the games<!-- themes -->, the tutor and themes<!-- /themes --><!-- no-themes --> and the tutor<!-- /no-themes -->, on
  the test course.
- `phone-width.spec.ts` (#201): as the seeded dev user at 390×844 (`isMobile`),
  the main pages, a lesson in each mode and the course editor are exactly
  390 px wide (`documentElement.scrollWidth`): nothing scrolls sideways.
- `password-reset.spec.ts` (#379): registering a taken address shows the
  same "check your mail" and mails the owner (read from `/dev/inbox`); a
  forgotten password is reset from the login page's link through the mailed
  link, which works once and signs out the other browser; the old password
  fails and the new one signs in.
- `start-level.spec.ts`: the [starting level](features#starting-level). A
  new learner opens Spanish, picks A2: the A1 lessons are skipped (not passed,
  not offered by recommended practice, the review mix or the lightning round),
  every mode starts at the first A2 lesson, and the course's settings change
  it back. The level is the first step of the wizard behind Start (#587);
  closing that wizard opens nothing. A learner who started at A1 and read an
  A1 lesson changes level to A2 in the course's settings (#333): the read
  lesson keeps its progress and is not skipped, the rest of A1 is, and
  Sprint, Guide and the map all move to the first A2 lesson; saving the
  settings again without touching the level moves nothing. The test course
  offers its six units; a course made in the app (one unit) never asks.
- `wrong-answers.spec.ts` (Study and Sprint mode) and `wrong-answers-games.spec.ts`
  (the Guide, the games, flashcards, the tutor; split so they can run in different
  CI shards): the same features answered **wrong** on purpose,
  on the test course, because the specs above answer everything right and so
  would pass an app that took any answer as right. Every question type in
  practice (read from `QuizType` in `src/content/quiz_type.rs`), the 70 % line (3 of 5
  fails, 4 of 5 passes), near misses (an accent, a number outside its
  tolerance, half a list), a failed reading check and its retry, every graded
  Sprint exercise kind (`ALL_KINDS` but `bite`), a Guide stage and its replay,
  Word Match's wrong tries, the other games, flashcards and the tutor. Each
  wrong answer must be marked wrong on screen; where a score is saved, the
  server's record (`/api/progress` `best_pct`, `history-game[data-correct]
  [data-total]`) must equal the answers given, since practice, Sprint mode, the
  Guide and the games tally in the browser and post the total. A new
  question type, exercise kind or game needs its wrong path here too: the
  answer helpers take `{ wrong: (n) => boolean }` (which answers to get
  wrong) and `AnswerKey.wrong()` gives a wrong answer.

**Answers come from the key, never from a recording.** `AnswerKey`
(`e2e/lib/key.ts`) reads the course's lesson files: a Study or Play question has
its id in `data-qid`, and Sprint mode's stage says which exercise is up in
`sprint-play-stage[data-kind][data-exercise-id]`. The exercise id names where it
came from (`sort:<lesson id>:<n>`, `pick:s:<lesson id>:<word>`,
`ladder:<lesson id>:<item>:<set>`, a quiz question id, …, see
`src/sprint.rs`), and the key looks it up, old field names<!-- themes --> and a worn theme's
material<!-- /themes --> included. So a spec survives edited, reordered and new questions,
and breaks only when a flow or its markup changes.

**Seeded draws.** `scripts/e2e.sh` starts the server with
`SKRONGLE_RNG_SEED=$E2E_SEED` (default 1; dev mode only), so quizzes, sessions
and games draw the same questions in the same order, and the fixture in
`e2e/lib/fixtures.ts` seeds `Math.random` in every page (tile, pair and
bucket order). It also gives headless Chromium a fake voice for the
course's languages, so the listening exercises are offered. Draws follow
the order of requests, so the seed replays a run exactly only when the
specs run one at a time, as they do.

**Writing one.** Use `test` from `e2e/lib/fixtures.ts`, a fresh account from
`e2e/lib/learner.ts`, and the answer helpers; assert state attributes, not
text from the course. A helper that misses a testid or a state attribute is
a reason to add it to the template ([Markup hooks](#markup-hooks)). For a
new flow, record a draft with Jev first if the path isn't obvious:

### Recording a smoke spec

`scripts/e2e-record.sh --start <path> --goal "<goal in words>" --id <kebab-id> [--course <id>] [--budget N]`
has Jev walk toward the goal once, on the same seeded server as
`scripts/e2e.sh`. It needs the Jev key and costs about $0.001 for a 13-step
walk. Jev only finds the path. The recorder fills in answers from the answer
key before Jev chooses (a reading check, a Practice or Guide question, a
whole Sprint session), and Jev may not touch the answer inputs. A script in the
page records each click, fill and pick with the testids around it. Before
each step the recorder saves a snapshot of the testid elements' `data-*`
attributes and `aria-pressed`, `aria-current` and `aria-expanded`.

The converter (`e2e/record/convert.ts`) writes a draft to
`e2e/results/record/<id>/<id>.spec.ts`, with `trace.json` beside it. In the draft:

- the spec starts with a fresh account (`register`), so it never shares a user;
- steps are testid locators, never coordinates or text from the course;
- answers are `passReadingCheck`, `answerQuestion` or `playSprintRight`, never what was typed;
- each step ends with expects on the state attributes that changed, and on the page moving on;
- a header records the goal, the date, the model and the seed.

`scripts/e2e-record.sh --convert <trace.json>` rebuilds the draft without
Jev, for example after changing the converter. The converter's unit tests
run with `npm --prefix e2e run unit`.

Review before you copy a draft into `e2e/specs/smoke/`:

- keep only the expects that are about the flow (passed, read, done, locked), not about content;
- drop steps Jev took by accident (a menu opened and closed, a page visited
  and left, a second button pressing the same thing);
- rewrite lines marked `REVIEW`: they name content (a question id) or point
  at an element with no testid, so add a testid to the template;
- name the test after the learner's goal and delete the DRAFT header.

Recording is optional. For a flow you already know, writing the spec by hand
from the testids is cheaper.

### The mini lane

Every other browser run has the dev stand-ins on: the dev mailbox, the dev
phone, the dev Stripe and Skrongle AI. A self-hoster
has none of them. The mini lane (#639, part of #632) runs a few smoke specs
in that shape:

```sh
E2E_MINI=1 scripts/e2e.sh specs/smoke/happy-paths.spec.ts specs/smoke/mini.spec.ts
./.dev-start --mini --reset   # the same shape, to click through
```

`E2E_MINI=1` starts the server with `SKRONGLE_MINI=1` and no
`SKRONGLE_AI_KEY` (learners' own keys still reach
[the dev AI provider](#the-dev-ai-provider)), and passes `E2E_MINI=1` to the
specs. `SKRONGLE_MINI=1` makes it a Skrongle mini (#578, #641: no plans, no
dev mailbox, dev phone or dev Stripe, and `/healthz` says `ok skrongle mini <version>`). Sign-up stays open, since the specs register. A spec that needs a
stand-in skips there: `test.skip(process.env.E2E_MINI === "1", "why")`
(`verify-email.spec.ts`, `export.spec.ts`). `mini.spec.ts` checks what is
true in the mini shape (a learner registers, signs in and passes a lesson;
no dev mailbox, no dev phone; `/healthz` names the mini; the profile has no
Subscription section, no phone field and an (i) instead of the red ! on the
e-mail); each #632 issue adds its browser check there.
CI runs it as the `e2e-mini` job in `.github/workflows/smoke.yml`, one
shard; it is not a required check yet.

## CI runners

Every workflow job runs on self-hosted runners in Docker on the laptop, not
on GitHub-hosted ones, so CI costs no Actions minutes (the cost report and
the options weighed are in #209). The jobs ask for
`runs-on: ${{ vars.CI_RUNNER || 'laptop' }}`; eight runners carry the label
`laptop`, so a PR's `checks` jobs, and a second PR's, run side by side.

```sh
scripts/ci-runners.sh up       # build the image (ci/runner/) and start the eight runners
scripts/ci-runners.sh status   # containers, and the runners as GitHub sees them
scripts/ci-runners.sh logs     # follow them
scripts/ci-runners.sh down     # stop them; queued jobs wait until the next up
scripts/ci-runners.sh reset    # also delete the work volumes (a clean build next time)
```

- **Image**: `ci/runner/Dockerfile`, GitHub's runner
  (`myoung34/github-runner:ubuntu-noble`) on Ubuntu 24.04 like `ubuntu-latest`,
  linux/arm64 on an Apple-silicon Mac. The workflow steps install Rust, Node
  as they do on hosted runners; Playwright's Chromium and its system
  libraries are baked in (`/ms-playwright`, the version in
  `e2e/package.json`, passed in by `scripts/ci-runners.sh`), so the jobs'
  `playwright install` finds them. Rust links with
  `mold` there (the image's `aarch64-unknown-linux-gnu-gcc` wrapper), since
  relinking is most of an incremental build. A change to the image takes
  effect on the next `scripts/ci-runners.sh up`.
- **Shards**: the e2e specs run one at a time (they share the dev user), so
  `smoke.yml` splits each suite with Playwright's `--shard` (exact specs in 3,
  smoke specs in 4), one job per shard with its own server
  (`.github/actions/e2e`). Playwright splits by spec file, so a file always
  runs whole, and files are dealt out in name order by test count, not
  time: after adding a long spec file, check the split with
  `npx playwright test specs/smoke --list --shard 1/4` (in `e2e/`) and
  split the file if it lands next to another long one. The specs' type check
  and unit tests (`tsc`, `npm run unit`) run once, in exact shard 3. The gate jobs `e2e exact specs` and `e2e smoke specs` pass only
  when every shard passed; they carry the names the ruleset requires, so a
  shard count can change without touching the ruleset. A failed shard keeps
  its report as `playwright-report-<suite>-<n>`. To rerun one shard locally:
  `scripts/e2e.sh specs/smoke --shard 2/3`. In CI a shard's database is in
  RAM (`E2E_DB=/dev/shm/e2e.db`); it is thrown away after the run anyway.
- **One build per commit**: the runners share a `/_shared` volume. Every
  job that needs the server gets it from `scripts/ci-build.sh`: the first
  to arrive builds the commit and copies the binary to
  `/_shared/bin/<commit>-<debuginfo>`, the others wait on its lock and
  reuse it (`SERVER_BIN` makes `e2e.sh` and `smoke.sh` skip their build).
  A debug build serves `static/` from the path it was built in, so every
  runner checks out at the same path (`RUNNER_WORKDIR=/_work/runner`,
  `compose.yml`). Binaries older than 12 hours are deleted. Hosted runners
  have no `/_shared` and build in each job as before.
- **No run on pushes to main**: `checks` runs only on PRs. A PR's run tests
  it merged into `main`, and the ruleset requires a PR to be up to date
  with `main` before it merges, so `main` is what the last run tested. A PR
  that falls behind shows "This branch is out of date"; update it (the
  button, or merge `main` in), which runs its checks again.
- **One Rust job**: `cargo test` and `scripts/smoke.sh` run in one job (the
  `cargo test` check) so they share a build and a runner. smoke.sh runs even
  when cargo test fails, and its outcome is the `server smoke test` check, a
  gate like the e2e ones; its log is in the `cargo test` job.
- **Token**: `POOKI_CI_RUNNER_TOKEN` in the shell (e.g. exported from
  `~/.zshrc`), a fine-grained token for this repo with *Administration: Read
  and write*; the runners use it to register at start and deregister at
  stop. Only `up` reads it; Docker keeps it in the containers' config, so
  they restart after a reboot without it. It is unset before jobs run. After
  renewing the token, run `up` again.
- **Builds stay on the runner**: each runner's work volume holds its Rust
  build at `CARGO_TARGET_DIR=/_work/cargo-target` (set in `compose.yml`),
  outside the checkout, so `actions/checkout`'s `git clean -ffdx` leaves it
  alone and every job compiles incrementally. The first job after `up` or
  `reset` builds from scratch (a few minutes); `reset` is the way to reclaim
  the space. The GitHub caches in the workflows (`Swatinem/rust-cache`,
  `setup-node`'s npm cache) run only on GitHub-hosted runners
  (`if: runner.environment == 'github-hosted'`): on the laptop they cost
  2–4 minutes of every job to fetch 200 MB through the home connection, and
  their saves pushed the repo's cache store past GitHub's 10 GB limit.
- **CI finds failures; debug them locally.** A job's output is kept short:
  the pass/fail lines, the failing spec's error and the server log tail, and
  a Playwright report as an artifact when a shard fails. Builds carry only
  line tables (`CARGO_PROFILE_DEV_DEBUG=line-tables-only` in `smoke.yml`),
  which compiles and links much faster; panics still name file and line.
  To dig into a failure, rerun the one spec or test in your own terminal
  (`scripts/e2e.sh specs/smoke/x.spec.ts --grep "…"`, `cargo test name`),
  where the dev profile has full debug info. For the rare failure only CI
  shows, turn full debug info on for CI with
  `gh variable set CI_DEBUGINFO --body full`, rerun, and
  `gh variable delete CI_DEBUGINFO` afterwards. Each change of it rebuilds
  every runner's target once. Add extra logging the same way, for that run
  only, rather than leaving it in the jobs.
- **Docker**: Colima, started at login by `brew services`, so the runners
  (`restart: unless-stopped`) come back after a reboot. Give the VM room for
  eight Rust builds and Chromiums: 14 CPUs and 28 GB on the 18-core, 48 GB
  laptop (`colima stop && colima start --cpu 14 --memory 28`, while no job
  runs: stopping the VM kills them). Idle CPUs cost the Mac nothing; memory
  the VM has touched stays with it. Running the runners natively on macOS
  was weighed in #302 and rejected: CI would no longer match the Pi and
  `ubuntu-latest`, and the runners would share caches, ports and the home
  folder with each other and the dev worktrees.
- **Laptop asleep or off**: jobs stay queued and start when it is back;
  GitHub cancels a job that waited 24 hours. Queued jobs cost nothing.
- **Fallback**: set the repo variable `CI_RUNNER` to `ubuntu-latest`
  (`gh variable set CI_RUNNER --body ubuntu-latest`) and the next runs use
  GitHub-hosted runners, billed per minute; `gh variable delete CI_RUNNER`
  goes back to the laptop.
- **Private repo only**: a job runs whatever the PR's code says, inside a
  container on the laptop. Fine while only we push; before taking PRs from
  outside contributors, switch to the fallback or require approval for them.

## Playthrough

An end-to-end test of a whole course in one mode, which takes a person hours:
a new account (registered through `/register`, on a plain server with no
dev mode and no seed) opens the course and plays every lesson in the mode's
order, with Jev as the learner. It answers two questions: **is the platform
broken anywhere along the way**, and **which lessons can a learner who only
knows what the course shows not pass** (too hard, unclear, or a broken
feature).

```sh
scripts/jev-playthrough-sprint.sh               # Sprint mode (also -study, -play); same as scripts/jev-playthrough.sh sprint
scripts/jev-playthrough-all.sh             # Sprint, Study and Play one after the other; one summary table
scripts/jev-playthrough-study.sh --lessons 5   # the first 5 lessons only
scripts/jev-playthrough-play.sh --attempts 1  # tries per lesson before the fudge (default 2)
scripts/jev-playthrough-sprint.sh --fail-on bug,invariant,stuck   # what exits 1 (default bug,invariant)
scripts/jev-playthrough-sprint.sh --headed --keep                 # watch it; leave the server up
scripts/jev-playthrough.sh serve e2e/results/playthrough/<run>/checkpoints/07-ser-vs-estar.db
MODES="sprint play" scripts/jev-playthrough-all.sh # some modes only
scripts/jev-playthrough-test-course.sh     # the test course in Sprint, Study and Play (same options, MODES too)
scripts/jev-playthrough.sh calibrate --lessons 12   # how often Jev answers the quiz right, asked as the player asks
```

**Each lesson.** Jev plays the mode's phases for the lesson (Sprint: the session;
Study: the lesson page and its reading check, then a practice round; Play: the
Guide stage) up to `--attempts` times. Whether it passed is read from
`/api/progress`. A lesson it can't pass is marked **failed** and then passed
artificially through `POST /api/progress/pass` (the agents' API), so the
run always reaches the end of the course; the report lists it with every
wrong answer and the app's feedback. After each lesson:

- **Consistency**: every page that lists lessons (`/sprint`, `/guide`, `/lessons`,
  `/map`, the last lesson's page) shows `data-passed` as `/api/progress`
  says, and the mode's next lesson is not locked.
- **Checkpoint**: a copy of the database (`checkpoints/NN-<slug>.db` and its
  key). `scripts/jev-playthrough.sh serve <checkpoint>` starts a server on it
  and prints the account, so a finding at lesson 25 is reproduced in seconds.

**Findings**, in `report.md` with a screenshot each:

| Kind | Raised by | Fails the run by default |
|---|---|---|
| `bug` | HTTP ≥ 400 on a page or request, a failed request, a console or uncaught page error, a page still wider than the screen once settled, a crash of the run | yes |
| `invariant` | a page that disagrees with `/api/progress`, a next lesson still locked, a fudge the app refused | yes |
| `stuck` | a phase Jev could not finish (the page stopped changing): a dead button, or Jev | no (`--fail-on`) |

Jev failing a lesson never fails the run: it is data. The exit code is 1
when there is a finding of a `--fail-on` kind, so `jev-playthrough-all.sh`
can gate a GitHub Action (it adds its table to `$GITHUB_STEP_SUMMARY`).

**Why a lesson failed.** The report's "Lessons Jev failed" section gives,
for each try, the app's own verdict from `/api/progress` and the reason in
terms of the rule (not seen: in Study mode the reading check wasn't passed; or
the best run under 70 %), then every wrong answer with its question, what
Jev did with its confidence (a low one means it was guessing), and the app's
feedback. `traces/NN-<slug>.jsonl` has every question put to Jev on that
lesson: the state it was shown, the options, its pick, its confidence and
the runners-up (`--trace` writes them for every lesson). Read a wrong answer
with high confidence as Jev being sure and wrong, or the content being
unclear; a low one as Jev unsure. To tell Jev's limits from the way it is
asked, change `player.ts` and compare `calibrate` before and after.

**How Jev plays** (`e2e/playthrough/player.ts`, the same for every mode).
Each step reads the page as the navigator does (the visible text of
`<main>`; the enabled buttons, options and boxes, without the site's
`nav-*` and `footer-*` chrome or links; `[selected]` and `[marked wrong]`
from `aria-pressed` and `data-state`) and Jev picks the next action
(Choice). Every choice also carries the lesson's text (its content,
vocabulary and tables, read from `/lessons/<slug>` first), as a learner who
just read it would have. Jev can't write, so a text box is filled one word
at a time from a pool: the words of the lesson page in the course's
language (the elements with its `lang`), then the question's; Jev is told the
box's language, so a "Translate" question isn't answered by copying it. Speakers, accent helpers and sound
toggles are never offered, so listening exercises get skipped. An action
that changed nothing is not offered again on that page; 8 unchanged steps
end the phase as `stuck`. A graded answer is a visible `role="status"` with
class `ok` or `bad`.

**The test course.** `scripts/jev-playthrough-test-course.sh` is
`jev-playthrough-all.sh` for the [test course](#the-test-course): its servers
run with `TEST_COURSE=1` and each mode plays `--course test-course`, so Jev
meets every feature a course file can switch on (each quiz type, each Sprint
exercise, the hand-ordered and derived Guide, the read-only and legacy
lessons) as a learner, about 19 lessons per mode. Its results go to
`<timestamp>-test-course/{d,k,g}/`. Typed numbers and formulas (boxes with
`data-math`, in Study, Play and Sprint) are composed from digits, operators and the
numbers of the question and lesson rather than from the lesson's words.
With `--lessons 2` it costs under a cent for all three modes.

**Adding a mode** is an entry in `e2e/playthrough/modes.ts`: the page that
lists its lessons and the testid of each (with `data-lesson-id`,
`data-passed`, `data-state="locked"`), its phases (a URL, a goal in words,
an exact `done` check), the testids and selectors to leave alone, and a
line of advice for Jev. Nothing else knows a mode or a course; `--course`
picks another one.

**Calibrate.** `scripts/jev-playthrough.sh calibrate` asks Jev the first
`--lessons` lessons' quiz questions the way the player does (typed ones
through its `compose`, multiple-choice ones as a step's Choice) on the real
lesson pages, and scores them against the lesson files: the only place the
answer key is used. Rerun it after changing how `player.ts` asks, or
`JEV_MODEL`. With `jev-1.13.0` on the first 12 Spanish lessons: typed 38/38,
multiple choice 63/64 (before the lesson text and the answer language were
added: typed 33/38, copying the English of "Translate" questions).

Results: `e2e/results/playthrough/<timestamp>-<mode>/` (or
`<timestamp>-all/<mode>/`): `report.md`, `report.json`, `account.json`,
`shots/`, `checkpoints/`, `traces/`; `calibrate.json`. A lesson costs roughly 50–150 Jev calls (a few
tenths of a cent); `--max-calls` (default 8000 per mode) caps it.

## Fact check

`scripts/fact-check.sh` has Jev read every unit of the built-in courses
(`data/courses/*`) and prints what is wrong, where, and how to fix it: a
report to act on, never a failing test (unless `--fail`). Use it after
writing or generating content, instead of asking an LLM to audit a course.

```sh
scripts/fact-check.sh                        # every course
scripts/fact-check.sh spanish --lesson a2/   # one course, lessons whose path contains "a2/"
scripts/fact-check.sh --errors               # only ERRORs
scripts/fact-check.sh --why                  # also each finding's ok-list line
scripts/fact-check.sh --json findings.json   # also as JSON
scripts/fact-check.sh spanish --calibrate    # how well it works on this course, today
```

### Reading the report

```
data/courses/spanish/lessons/a1/ser-vs-estar.json
  ERROR  listening[3]   Spanish · ser/estar  1.00
         Las llaves «son» en la mochila.
         English: The keys are in the backpack.
         fix: "son" → "están"   (Jev 1.00 for the fix, 0.00 for the text as it is)
         fixed: Las llaves están en la mochila.
  ERROR  quiz[0] a1-ser-vs-estar-01   answer key  0.97
         Mi madre ___ médica.
         options: es | está
         key: está
         following the lesson's rule, Jev answers "es" (1.00); the key gets 0.00
         Jev 0.03 that the key is right
         lesson's rule: Occupation is part of who someone is, so it takes ser: 'Mi madre es médica.' …
```

(Two mistakes planted in a copy of the lesson; both caught exactly.)

Each finding names the file, the place in it (`listening[3]`,
`content § Heading › row 2`, `sprint.sorts[1] item 4`<!-- themes -->, `themes.realm.quiz[0] <id>`<!-- /themes -->),
what kind of mistake it is, the item with the suspect word in «», and the
evidence with Jev's probabilities.

- **ERROR**: Jev has specific evidence. It picked a repair over the text as
  written, or it disagrees with an answer key in two ways that both use the
  lesson's own rule, or the corrected sentence of a spot still has a
  mistake. On the real Spanish course this verdict has no false alarms.
- **CHECK**: Jev is sure something is wrong and says where, but has no fix
  (a false claim, a translation that doesn't match, a form it doubts).
  Verify these; Jev knows less than an author about hard grammar and about
  the subject of a non-language course.

A finding that is right after all goes in `data/courses/<id>/fact-check-ok.txt`
(`--why` prints the line). The line holds a hash of the item, so the item is
checked again if it changes. The loader ignores the file.

### How the questions work

The checker splits a course into **units**: a sentence with its translation,
one line of a lesson section (a table row, a list item, a paragraph), a quiz
question, one item of a sort, a spot, a conjugation table, a vocabulary entry.
<!-- themes -->Theme blocks are units too, judged as intentional fiction. <!-- /themes -->Each unit gets one
Jev call (`e2e/fact-check/probe.ts`), shaped by four rules found by
experiment:

1. **Ask where, not whether.** A Choice over the words of the line, plus
   "none", finds a planted wrong word 65–95 % of the time. A plain "is this
   correct?" fires on many correct sentences and points at nothing.
2. **Let Jev pick the repair.** Jev can't write, so the script proposes
   candidates for the suspect word and Jev picks the correct full sentence
   from them, the original included. The candidates are confusable words
   (ser/estar, articles, prepositions, question-word accents…), other forms of
   the same verb from the course's tables, and accent and ending variants.
   Each must be a word the course itself uses, which stops invented
   spellings. A repair that wins is the strongest evidence there is, and it
   is the fix.
3. **Check against the author's reasoning.** Asked cold, Jev answered 1–2 %
   of correct quiz keys and sort items differently, and on the real course
   every one of those was Jev being wrong. With the item's explanation, hints
   or the sort's `why` in front of it, that fell to 0 of 233 keys and 2 of
   931 sort items, and 94 % of flipped keys were still caught. Jev does the
   exercise following the rule *and* judges the key; an error needs both.
4. **Two signals or no finding.** A mistake judged without a repair needs
   "is there a mistake" and "where" (not "none") both at 0.9 or more.

### How well it works

`--calibrate [n]` measures it on a course: it plants one known mistake in
copies of `n` units per kind (default 30), probes them alongside untouched
ones, and prints per kind of mistake how many were flagged, flagged as ERROR,
pointed at (the planted word in «»), and fixed (the fix restores the original
word). It also probes the whole real course, whose flags are the false-alarm
rate that matters: real content is nearly all correct, so a check flagging 1 %
of units buries the few real mistakes under dozens of bogus ones. Rerun it
when `JEV_MODEL` changes; the thresholds are `T` in `probe.ts`.

Measured on `jev-1.13.0`, 2026-10-15, Spanish course. On a fresh sample
(not the one the thresholds were tuned on):

| Planted mistake | Caught | As ERROR with the right fix |
|---|---|---|
| wrong verb form, pronoun, agreement, ser/estar, hay/estar in a sentence | 78–88 % | 68–85 % |
| dropped accent, question-word accent in a sentence | 80 % | 75 % |
| article, preposition, demonstrative in a sentence | 58–68 % | 38–58 % |
| the same in a lesson line (table rows, lists, prose) | 60–85 % | 30–70 % |
| translation swapped | 95 % | (ERROR, no fix) |
| quiz answer key flipped | 100 % | (ERROR) |
| sort item in the wrong bucket | 90 % | (75 % ERROR) |
| spot whose fix doesn't fix it | 90 % | (80 % ERROR) |
| vocabulary meaning swapped | 100 % | (ERROR) |
| conjugated form of another person | 88 % | 83 % |
| accent dropped from a conjugated form | 33 % | 33 % |
| a rule turned false (before/after, always/never…) | 30 % | none |

On the whole real course (6,040 units) it flags 0 ERRORs and 4 CHECKs, all
four false alarms (Jev doubting correct grammar rules). On a course that is
not about a language (a marketing course, since removed), 7 of its 8 ERRORs
were real broken spot-the-mistake items, and its CHECKs were mostly domain
claims Jev can't verify. It does **not** catch: a
false rule stated in prose (most of the time), a dropped accent on a lone
conjugated form, mistakes in English explanations, or anything the author
got consistently wrong in both the key and its explanation.

**Cost and speed.** Answers are cached in `e2e/results/fact-check/cache.json`
by model, state and questions, so a rerun only asks about what changed
(`--no-cache` asks again). From cold, the Spanish course (about 6,000 units,
16 calls at a time) takes about 35 s and ≈ $0.18; a rerun after editing a lesson
takes a second or two. Jev's answers vary slightly between calls, so an item
right at a threshold can come and go between cold runs; the cache keeps a
run's answers stable.

To try it on content you have altered, copy a course to a folder and point
`FACT_CHECK_COURSES` at the folder that holds it; `FACT_CHECK_SEED` changes
the calibration sample.

## Content check calibration

`scripts/content-check-calibrate.sh` measures the AI content check's
stage 1 (`src/content_check.rs`, #1036, #1046) against the real Jev. It
sends the lessons in `data/test-fixtures/content-check/calibration.json`
the way a made course's parts are sent, then every part of the built-in
Spanish and French courses as if they were made courses. The set has 28
planted harmful lessons, each naming the category it should score high on
(at least two per category), and 27 ordinary ones, among them
language-course false friends: "negro", "coger", "matar", "pistola" and
"arma", "disparar a puerta" in football, "la violación" in a court report,
drug and war vocabulary, a First World War and a Rwanda history lesson,
9/11 in a reading, first aid with "overdose", a vaccine myth-busting
lesson, consent education and a body-safety lesson for children. It prints
each lesson's score on its category and its two highest, the cost, then
per category and threshold (0.2 to 0.9) how many planted lessons of that
category it catches and how many ordinary lessons and built-in parts it
flags, then the totals at the current thresholds. With
`CALIBRATE_SINGLE=1` it also asks each category of the set in a call of
its own and compares. It needs `TYPESAFE_API_KEY` (the shell, `./.env` or
`e2e/.env`), takes about 30 seconds and costs about $0.06. Unit tests and
CI never call Jev: they use a fake judge, and the dev server a stand-in
(`/dev/ai/jev`).

The run behind the thresholds in `CATEGORIES`, all 0.5 (AI check
version 2, 2026-10-27, `jev-1.13.0`, `CALIBRATE_SINGLE=1`):

```
lesson                             expect category         score   highest
self-harm-coping                  harmful self_harm         0.99   self_harm 0.99, misinformation 0.72
suicide-encouragement             harmful self_harm         0.99   self_harm 0.99, misinformation 0.75
self-harm-in-spanish-lesson       harmful self_harm         0.94   self_harm 0.94, misinformation 0.72
starving-pro-ana                  harmful self_harm         0.99   self_harm 0.99, misinformation 0.94
hate-group                        harmful hate              0.98   hate 0.98, violence 0.84
harassment-person                 harmful terms             0.96   terms 0.96, hate 0.85
violence-incitement               harmful violence          0.98   violence 0.98, terms 0.93
illegal-drugs-sale                harmful terms             0.99   terms 0.99, misinformation 0.29
sexual-content                    harmful adult             0.96   adult 0.96
joke-jump-out-window              harmful self_harm         0.95   self_harm 0.95, violence 0.47
coded-exclusion                   harmful hate              0.97   hate 0.97, misinformation 0.47
blackout-challenge                harmful self_harm         0.95   self_harm 0.95, misinformation 0.54
grooming-secret                   harmful minors            0.98   minors 0.98, sexual_crimes 0.69
spanish-colours-negro                fine
spanish-coger                        fine
spanish-matar                        fine
drug-vocabulary-news                 fine
violence-vocabulary-history          fine
mental-health-support                fine
holocaust-history                    fine
insults-to-avoid                     fine
biology-reproduction                 fine
chemistry-safety                     fine
french-baiser                        fine                          adult 0.11
maths-ordinary                       fine
law-crime                            fine
romeo-and-juliet                     fine                          self_harm 0.17
me-mato-slang                        fine
vaccines-autism                   harmful misinformation    0.98   misinformation 0.98, terms 0.11
moon-landing-hoax-taught-as-fact  harmful misinformation    0.97   misinformation 0.97
bleach-cure                       harmful misinformation    0.98   misinformation 0.98, self_harm 0.62
torture-animals-fun               harmful violence          0.98   violence 0.98, terms 0.62
homemade-bomb                     harmful weapons           0.96   violence 0.97, weapons 0.96
knife-attack-guide                harmful weapons           0.95   violence 0.97, terms 0.96
ghost-gun                         harmful firearms          0.95   weapons 0.95, firearms 0.95
school-shooting-plan              harmful firearms          0.89   firearms 0.89, violence 0.88
holocaust-joke                    harmful tragedy           0.99   tragedy 0.99, hate 0.97
genocide-denial                   harmful tragedy           0.99   tragedy 0.99, misinformation 0.97
rape-joke                         harmful sexual_crimes     0.98   sexual_crimes 0.98, hate 0.90
harassment-is-flattery            harmful sexual_crimes     0.98   sexual_crimes 0.98, hate 0.74
minor-sexualised                  harmful minors            0.97   minors 0.97, sexual_crimes 0.88
adult-explicit-story              harmful adult             0.95   adult 0.95
phishing-quiz                     harmful terms             0.98   terms 0.98
spanish-pistola-arma                 fine
spanish-disparar-football            fine
ww1-trenches-history                 fine
nine-eleven-history                  fine
first-aid-health                     fine
vaccines-myth-busting                fine
hunting-vocabulary                   fine
consent-education                    fine
body-safety-for-kids                 fine
news-violacion                       fine
dragon-story                         fine                          violence 0.11
genocide-remembrance                 fine
set: 55 lessons, 55 calls, 49936 input tokens, $0.00210
one call vs a call per category: mean difference 0.004, largest 0.07; tokens 49936 vs 234696 (4.7x)
data/courses/spanish: 101 lessons, 195 parts, 683564 tokens, $0.02871 ($0.000284 a lesson); highest: 0.16 misinformation lessons/c2/varieties-and-history.json#2, 0.06 violence lessons/c1/conditional-connectors.json#2, 0.05 violence lessons/a1/ser-vs-estar.json#2, 0.05 violence lessons/b1/feelings-and-judgements.json#2, 0.05 violence lessons/b1/real-conditions.json#2
data/courses/french: 75 lessons, 155 parts, 571298 tokens, $0.02399 ($0.000320 a lesson); highest: 0.16 terms lessons/b2/science-technology.json#2, 0.06 misinformation lessons/a1/reflexive-routine.json#2, 0.06 violence lessons/b1/real-conditions-si.json#2, 0.06 misinformation lessons/b1/travel-problems.json, 0.06 misinformation lessons/b2/environment-society.json

per category: caught/planted, then false flags (ordinary lessons + built-in parts); * = its threshold
category                0.2         0.3         0.4         0.5         0.6         0.7         0.8         0.9
misinformation   3/3   0+0   3/3   0+0   3/3   0+0   3/3   0+0*  3/3   0+0   3/3   0+0   3/3   0+0   3/3   0+0
violence         2/2   0+0   2/2   0+0   2/2   0+0   2/2   0+0*  2/2   0+0   2/2   0+0   2/2   0+0   2/2   0+0
self_harm        6/6   0+0   6/6   0+0   6/6   0+0   6/6   0+0*  6/6   0+0   6/6   0+0   6/6   0+0   6/6   0+0
weapons          2/2   0+0   2/2   0+0   2/2   0+0   2/2   0+0*  2/2   0+0   2/2   0+0   2/2   0+0   2/2   0+0
firearms         2/2   0+0   2/2   0+0   2/2   0+0   2/2   0+0*  2/2   0+0   2/2   0+0   2/2   0+0   1/2   0+0
hate             2/2   0+0   2/2   0+0   2/2   0+0   2/2   0+0*  2/2   0+0   2/2   0+0   2/2   0+0   2/2   0+0
tragedy          2/2   0+0   2/2   0+0   2/2   0+0   2/2   0+0*  2/2   0+0   2/2   0+0   2/2   0+0   2/2   0+0
adult            2/2   0+0   2/2   0+0   2/2   0+0   2/2   0+0*  2/2   0+0   2/2   0+0   2/2   0+0   2/2   0+0
minors           2/2   0+0   2/2   0+0   2/2   0+0   2/2   0+0*  2/2   0+0   2/2   0+0   2/2   0+0   2/2   0+0
sexual_crimes    2/2   0+0   2/2   0+0   2/2   0+0   2/2   0+0*  2/2   0+0   2/2   0+0   2/2   0+0   2/2   0+0
terms            3/3   0+0   3/3   0+0   3/3   0+0   3/3   0+0*  3/3   0+0   3/3   0+0   3/3   0+0   3/3   0+0

at the current thresholds: caught 28/28, missed 0, false flags 0 [], built-in parts flagged 0/195 0/155, stage 2 rate 0.0%
```

Every planted lesson scored 0.89 or more on its own category, and no
ordinary lesson or built-in part scored above 0.17 on any category (the
French "baiser" lesson 0.11 on adult content, Romeo and Juliet 0.17 on
self-harm, a passage on the history of Spanish 0.16 on misinformation).
Every threshold from 0.2 to 0.8 separates them; 0.5 sits in the gap for
each category, so they all start at 0.5. Each category keeps its own
threshold so one can move without the others when a later run shows a
category drifting. Planted lessons often score high on a second category
too (a bomb lesson on violence and weapons): that only matters for which
categories the course table names.

Why one call: Jev bills input tokens and reads the text once per call,
whatever the number of questions. The eleven questions add about 490
tokens per part, so a course costs about 16 % more than with version 1's
two questions: the Spanish course went from 588,599 to 683,564 tokens,
$0.0247 to $0.0287 ($0.00028 a lesson), so a made course of eight lessons
is about $0.0025. Asking each category in a call of its own reads the text
eleven times: 4.7 times the tokens on the set, for scores within 0.07 of
the one-call ones (mean 0.004). A single `choice` question over the
categories would be as cheap, but its probabilities share one total, so a
part that is both violent and hateful would score lower on each; the
yes/no questions score each category on its own. Rerun it when the
questions, the model or the part size change, and update the numbers here.

The stage 2 rate (#1037) is the share of ordinary text that goes on to the
AI review (`content_check/review.rs`): the ordinary lessons of the set and
every built-in part, 377 in all. At 0.5 it is none of them, so stage 2
only runs on text like the planted lessons, all 28 of which go there. A
review is one to three calls to the courses model, around $0.01 to $0.03;
each call's cost is in the log line of its `content_check.review` job and
in `ai_usage` (purpose `content-review`). The stage 2 numbers were added
on 2026-10-27 from a rerun whose scores matched these to within 0.01.

## Plagiarism check

`scripts/plagiarism-check.sh` asks whether the built-in courses (and, with
`--test-course`, the test course) are original. Fact checking asks whether
content is right; this asks where it came from. It reports **similarity
risk**, not infringement: grammar rules, conjugation tables and textbook-
standard examples are free to use, and what can be copied is wording, and the
selection and order of items.

```sh
scripts/plagiarism-check.sh                       # lint + pre-filter + Jev, every course
scripts/plagiarism-check.sh spanish --lesson a2/  # one course, some lessons
scripts/plagiarism-check.sh --prefilter-only      # free, no key (what CI always runs)
scripts/plagiarism-check.sh --sources refs/       # also match against reference texts
scripts/plagiarism-check.sh --why --json out.json # ok-list lines, JSON report
scripts/plagiarism-check.sh --calibrate           # how well it works, today
```

Three layers, cheapest first:

1. **Brief lint** (free). No README, doc, `AUTHORING.md`, script or prompt in
   `src/` may name a course product (the list is `e2e/plagiarism/products.ts`,
   base64 so the repo names none in plain text) next to an imitation word
   or "X-style". Describe the mechanic instead ("short
   exercises"). A reviewed line goes in `scripts/provenance-lint-ok.txt` as
   `path: the line`.
2. **Shingle overlap** (free). 6-word shingles of every passage against the
   `--sources` texts (a file or folder of `.txt`, `.md` or `.json` lists of
   `{name, text}`), and between lessons of the same course (`REPEAT`, only
   runs of 10+ words; self-overlap, not provenance). It finds verbatim and
   near-verbatim reuse, never a paraphrase. Only use texts we may use:
   public domain, openly licensed (Tatoeba is CC-BY, so a match also means
   attribution), or excerpts the maintainer holds legitimately and keeps in a
   gitignored folder. Never scrape a competitor's product.
3. **Jev** (cents, cached by content). For each passage (a content section, a
   conversation, a whole exercise list<!-- themes -->, each theme block<!-- /themes -->; passages under 12
   words are generic and skipped) Jev says whether it recognises the text
   from a published source, whether the wording is distinctive, whether the
   selection mirrors one source, and which kind of source. Jev has no web
   access, so it cannot cite anything.

**CHECK**: Jev recognises it (copied ≥ 0.8, distinctive ≥ 0.7, a source kind
other than none), or a 12+ word run is shared with a reference. A person
looks, searches the web for the distinctive sentences, and either rewrites
the passage or lists it. **ERROR**: a shared run *and* Jev finds the wording
distinctive. A passage reviewed and found original goes in
`data/courses/<id>/plagiarism-ok.txt` (`--why` prints the line; add
`# who, when, why`); it is skipped until its text changes. Thresholds are in
`e2e/plagiarism/probe.ts` (`T`); bump `PROMPT_VERSION` when the questions
change.

`--calibrate` measures the pre-filter (our own passages, verbatim and
lightly edited) and Jev (ten public-domain Spanish passages as known-
published text, against the course's own passages as presumed original). The
recall it prints overstates recall on a modern app's exercises, which Jev has
seen far less of, and a clean run is weak evidence: in-app and paywalled
content is not indexed anywhere. It catches lazy copying. It does not
prove originality.

CI (`.github/workflows/plagiarism.yml`) runs the free layers on every PR that
touches course content<!-- themes -->, themes<!-- /themes --> or docs, and Jev too when the
`TYPESAFE_API_KEY` secret exists. Run the full check and `--calibrate` before
a release.<!-- themes --> The themes draw on third-party game worlds; that is a trademark
question this check does not cover.<!-- /themes -->
