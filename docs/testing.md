# Testing locally

How to drive and check the app on their own machine:
one command to a logged-in session, a simulated AI provider whose replies
come from canned files (or that you answer by hand), and markup with stable hooks.

**Everything here is local only.** It is compiled only with the `dev` cargo
feature, off by default: `./.dev-start`, `scripts/smoke.sh`, `scripts/e2e.sh`
and CI build with `--features dev`, and a release build (`cargo build
--release`, `scripts/build-pi.sh`, the release workflow) has no `/dev`
routes, no simulated provider and no test course, and does not know the
settings below. Unit tests that need it run under `cargo test --features
dev`; plain `cargo test` leaves them out. In a dev build it needs `SKRONGLE_DEV=1`, which
`./.dev-start` sets and `deploy/` never does, and the server refuses it
(logging why) unless it binds a loopback address, `SKRONGLE_SECURE_COOKIES`
is not `1` and `SKRONGLE_PUBLIC_URL` is unset (or local). It also refuses it
when the database holds any account besides `dev@example.com`, since that
looks like a deploy's database, unless `SKRONGLE_DEV_FORCE=1` says it is a
development one; `./.dev-start` sets that, and `scripts/e2e.sh`,
and `scripts/smoke.sh` start from a fresh database. In dev
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
login: dev@example.com / dev-password   (or http://127.0.0.1:8214/dev/login)
pr:    https://github.com/alexbeeken/skrongle-mini/pull/12   ([11] Remove the plagiarism check)
```

`pr` is the branch's open pull request, so a reviewer can start on its QA steps; `gh` looks it up
during the build, and the line is left out without `gh`, a login, the network or an open PR.
`login` is left out when dev mode is
(`--lan`, which prints `phone:` with the LAN address instead). The build's output shows only if it
fails, and the server log goes to `dev-server.log`, in the foreground too (Ctrl-C stops it); a server
that exits at boot prints the log's last lines. `DEV_VERBOSE=1 ./.dev-start …` shows everything: the
build, the settings, and in the foreground or with `--watch` the server log in the terminal.

| Flag | Does |
|---|---|
| `--reset` | Delete `./dev.db`, its key file and the held LLM requests first |
| `--seed` | Create the dev user with Spanish open, two lessons passed and one seen (`SKRONGLE_DEV_SEED=1`), as an owner (`users.owner`), with [the dev AI provider's](#the-dev-ai-provider) keys for Anthropic, OpenAI and Gemini (not with `--llm live`) |
| `--port N` | Listen on a fixed port. Without it (or with `--port auto`) the server takes a random free port in 8100-8999, so two worktrees can run at once |
| `--llm capture` / `--llm live` | Model calls go to [the dev AI provider](#the-dev-ai-provider) (below) without `--llm`; `capture` starts it with Manual replies on (each request waits for your answer at the bottom of `/dev/ai`); `live` sends them to the real providers instead |
| `--test-course` | Also offer the [test course](#the-test-course) (`TEST_COURSE=1`) |
| `--background` | Build, start detached, wait for `/healthz`, print the links. Pid in `.dev-server.pid`, URL in `.dev-server.url`, log in `dev-server.log` |
| `--watch` | Rebuild and restart on every change, keeping the port and `dev.db`; open pages reload themselves ([below](#live-reload---watch)). With `--background` the watcher runs detached and `--stop` ends it |
| `--stop` | Stop the background server (or watcher) |
| `--lan` | Bind `0.0.0.0` for phones; this turns dev mode **off** |

`GET /healthz` answers `ok` with no login; wait on it instead of sleeping.

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
AI prompt) are compiled in, and `data/` (courses) is read at boot.
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

The dev pages (`/dev/jobs`, `/dev/authenticator`, `/dev/ai` and every
route under them, JSON and actions too) need no sign-in: they are for
whoever is testing and hold every account's authenticator entries. Like
every dev route they exist in dev mode only, for requests made on this
machine (`dev::local_only`). `e2e/lib/totp.ts` reads the authenticator with
a request context of its own, so the learner a spec drives is not touched.
There is no dev mailbox: the app sends no e-mail (#10).

| Route | Does |
|---|---|
| `GET /dev/login` | Log in as `dev` (`dev@example.com`, password `dev-password`), an owner (`users.owner`: the Manage Users page, `/manage`). Every dev-mode boot creates the account if it is missing, so the login form takes those credentials too, and `/login` and `/register` show a **Log in as dev** button. `?open=spanish` opens a course (`?open` alone: Spanish) and counts it as set up, so its first-visit settings wizard does not open (add `&wizard=1` to see the wizard on the course home again), `?keys=dev` (or the older `?keys=fake`) stores [the dev AI provider's](#the-dev-ai-provider) keys for Anthropic, OpenAI and Google as the server's (#25: one set for every account), so the AI forms are unlocked and offer each provider, `?next=/courses` says where to land. It skips [two-factor sign-in](features#two-factor-sign-in); `/login` does not |
| `GET /login/code/device` | Dev mode only: the dev device the pending sign-in's code went to, for the frame at the bottom of `/login/code` (see [the dev device](#two-factor-sign-in-the-dev-device)); needs the `twofa` cookie, not a session. `?format=json` for the rows |
| `GET /dev/reset` | Wipe the dev user's progress, projects, own courses (drafts and generated) and saved app language, keeping the account and keys. `?next=` as above |
| `GET /dev/jobs` | The newest 100 rows of the background `jobs` table (`src/jobs.rs`), newest first: `{"jobs": [{id, kind, payload, state, priority, run_at, attempts, max_attempts, locked_by, locked_until, last_error, created_at, finished_at}]}`. For a browser (`Accept: text/html`, or `?format=html`) a page (`dev-jobs`) with a row per job (`dev-jobs-row[data-id][data-kind][data-state][data-run-at][data-attempts]`, `dev-jobs-state`, `dev-jobs-attempts`, `dev-jobs-error`), **Run now** (`dev-jobs-run-now`) on queued and running rows, **Queue a failing job** (`dev-jobs-queue-fail`) and an (i) (`dev-jobs-info`). No sign-in, like the other dev pages. `?format=json` forces JSON |
| `POST /dev/jobs/fail` | Queue a `dev.fail` job due now (it always fails, so its attempts climb through `jobs::RETRY_DELAYS` to `failed`), then back to `/dev/jobs` |
| `POST /dev/jobs/{id}` | Form `do=run_now`: a queued row is due now; a running row's lease ends now, as if its server had stopped. The runner takes it within a moment. 404 for a row that is done, failed or gone |
| `GET /dev/ai` | [The dev AI provider](#the-dev-ai-provider)'s page, the dev pages' **AI** section (`dev-box-kind-ai`): each key profile's card (`dev-ai-profile[data-who=platform\|learner][data-over]`: its keys as copy buttons in the provider's colours `dev-ai-key[data-provider][data-copy-key]`, usage `dev-ai-usage`, limit `dev-ai-limit` + `dev-ai-measure` + `dev-ai-limit-save`, `dev-ai-reset`, **Hit the limit now** `dev-ai-exhaust` / `dev-ai-lift`, failure `dev-ai-fault` + `dev-ai-hang` + `dev-ai-once`/`dev-ai-sticky` + `dev-ai-inject`, `dev-ai-injected`, `dev-ai-clear-inject`), the call log (`dev-ai-call[data-id][data-who][data-format][data-purpose][data-status][data-fixture]`, `dev-ai-call-open`, `dev-ai-call-status`, `dev-ai-call-outcome`; polls `GET /dev/ai/rows` every 2 s), the fixture files (`dev-ai-fixture[data-name][data-edited]`) and **Add a scenario** (`dev-ai-scenario-*`). `?format=json`: `{mode, simulating, holding, base_urls, client_timeout_secs, profiles, calls, fixtures, held}` (`held`: the held requests, newest first, each `{stem, pending, request, response}`) |
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
| `GET /dev/authenticator` | [The dev authenticator](#two-factor-codes-the-dev-authenticator)'s accounts with their current codes. JSON `{"now", "accounts": [{id, issuer, account, code, remaining, period, digits, algorithm, secret}]}`, or for a browser the dev pages' **Authenticator** section (`dev-box-kind-auth`): an add form (`dev-authenticator-uri`, `dev-authenticator-add`, `dev-authenticator-error`) and a row per account (`dev-authenticator-item[data-id][data-issuer][data-account][data-code][data-expires]`) with the code (`dev-authenticator-code`, `123 456`), the seconds left (`dev-authenticator-remaining`), **Copy** (`dev-authenticator-copy`) and **Remove** (`dev-authenticator-remove`). `?uri=otpauth://totp/…` is the "scan": the form opens filled in with it; without it the form holds an example account. The page counts down and polls `GET /dev/authenticator/rows` (`{"html", "now"}`) |
| `POST /dev/authenticator` | Form `uri`: an `otpauth://totp/…` link or a base32 setup key; adds the account (the same secret and name again replaces it), back to `/dev/authenticator` (204 for the page's own posts, `{"id"}` with `Accept: application/json`), 400 with why |
| `POST /dev/authenticator/{id}` | Form `do=delete`: remove that account; 404 if it has none |
| `POST /dev/vocab` | JSON `{"lesson": "<slug>", "days_ago": n}`: collect that lesson's words for the signed-in learner as if `n` days ago, so they are due today (#1360). Answers `{"collected": n}`, the words that were new |
| `GET /dev/boot` | This process's boot id, which changes on every restart; the `--watch` reload script polls it ([Live reload](#live-reload---watch)) |

## Two-factor codes: the dev authenticator

The dev pages' **Authenticator** section (`<url>/dev/authenticator`),
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
  (`authenticator()`, `authenticatorCode(account)`, with a request context of its own).
  `e2e/specs/authenticator.spec.ts` covers add, scan, codes, rollover and remove.
- **Two-factor sign-in** ([features](features#two-factor-sign-in)): the
  profile's set-up dialog has *Dev: add it to the dev authenticator*, the
  scan link, which opens the dev authenticator in a tab of its own.
  `/dev/login` skips the second step, so sign in through `/login` to try it.
  `e2e/specs/two-factor.spec.ts` covers turning it on and authenticator
  sign-in, with the codes from the dev device below; it runs in CI with
  the exact specs.

### Two-factor sign-in: the dev device

In dev mode, a small window floats at the bottom of `/login/code`
(`login-code-dev`, a `<details>` you can fold) framing
`/login/code/device` (`login-code-dev-frame`): the dev device this sign-in's
code comes from, and nothing else: the dev authenticator's entry for the
account's address with its code counting down. Each row (`login-code-device-item`,
`data-code`) has **Use** (`login-code-device-use`), which types the code into
the form above, so a sign-in is two clicks. The frame re-reads itself every
two seconds (`static-dev/devbox.js`).

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
| DeepSeek, Mistral, xAI (OpenAI's format, #24) | `<url>/dev/ai/deepseek`, `/mistral`, `/xai` | `POST /v1/chat/completions`, answered as OpenAI's (`sk-dev-…` keys; xAI's `xai-dev-…`) |
| OpenRouter | `<url>/dev/ai/openrouter/api/v1` | `POST /chat/completions` (`sk-or-dev-…` keys) |
| Ollama-compatible | `<url>/dev/ai/ollama` (type it as the Ollama row's base URL) | `POST /v1/chat/completions`, no key |

**How calls get there: the keys do the routing.** In dev mode these are
the providers' default base URLs (unless `SKRONGLE_ANTHROPIC_BASE_URL`,
`SKRONGLE_OPENAI_BASE_URL` or `SKRONGLE_GOOGLE_BASE_URL` say otherwise), and the
server is seeded with one simulator key per provider as its keys (`--seed`,
and `/dev/login?keys=dev` or `?keys=fake` saves them too; every account uses
them, #25). A key the simulator
did not issue gets the provider's own 401, as a real provider answers a bad
key. There is no keyless stand-in and no client-side switch: a server
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
| Learner's own keys | `sk-ant-dev-anthropic`, `sk-dev-openai`, `AIza-dev-google` (seeded), and any `sk-ant-dev-…`, `sk-dev-…` or `AIza-dev-…` key you type (the old `?keys=fake` key `sk-ant-dev-fake-key` among them) | The server's keys, on Manage Users |

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

`cargo test --features dev` runs these scenarios through the simulator on loopback
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

## Release QA

The smoke test and the browser specs run a dev build. The release tarball
(`docs/self-hosting.md`, "What a release contains") is a different binary:
no `/dev` routes, cross-compiled for machines nobody on the project owns.
`scripts/release-qa/` checks that package (the plan is #47):

```sh
scripts/release-qa/package-smoke.sh dist/skrongle-0.2.0-aarch64.tar.gz   # one tarball, on a Linux box
scripts/release-qa/matrix.sh 0.2.0        # every architecture, each in a Debian container of that architecture (Docker)
scripts/release-qa/matrix.sh dist         # the same for a candidate you built
```

`package-smoke.sh` (POSIX sh + curl) checks the tarball's layout and
checksum, the binary (`--version`, `doctor`, what it links against), then
a first day over HTTP with no dev routes: the owner link from the log, the
owner, an invitation and a learner, the built-in course's pages, a copied
course shared by link, a clean stop and a second start on the same
database. `matrix.sh` runs it per architecture through
`scripts/release-qa/compose.yaml`, which also serves every architecture's
build side by side for a look in a browser (`docker compose up`, the owner
link in `docker compose logs`). The README in that folder has the
walkthrough, and what a container does not cover: systemd, `update.sh` and
rollback (the VM layer, #42), and anything timed (emulated speed means
nothing).

`scripts/release-qa.sh <version | tarball dir>` is the one command: it runs
`scripts/release-check.sh` (for a version) and then `matrix.sh`, and ends
with one summary and the log paths. The emulated matrix is the check for
each architecture; [Release QA: the hardware checklist](release-qa) is the
list a person walks on a Pi Zero 2 W before announcing a release.

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
no optional settings (no AI key, no
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
| every page (`base.html`) | `nav-home`, `nav-course-switch`, `nav-courses`, `nav-account-menu`, `nav-account-toggle`, `nav-profile`, `nav-switch-course`, `nav-docs`, `nav-logout-form`, `nav-logout`, `nav-login`, `nav-register`; mode bars `nav-study`/`nav-play` with tabs `nav-study-<tab>`, `nav-play-<tab>`, and `nav-study-mode`, `nav-study-to-sprint`, `nav-study-to-play`, `nav-study-menu` (and `nav-play-…` likewise) | `nav-account-toggle[data-email]`, `nav-course-switch[data-course-id]` (the course open now), `a[aria-current=page]` on links to the current page |
| `/` (home) | `home-stats` with `home-stat-passed`, `home-stat-mastered`, `home-stat-streak`, `home-skipped`; the map row `home-map` with `home-map-title`, `home-map-unit`, `home-map-progress`; `home-mode` (each card), `home-<mode>-open` (its heading link), `home-info-<mode>` (its (i)) opening `home-info-<mode>-dialog` (closed by `home-info-close`), `home-<mode>-<target>` (each link in it: `home-study-lessons`, `home-guide-stages`, `home-guide-next`, `home-ai-api-keys`, …), `home-guide-stat`, `home-start-sprint`/`-study`/`-play`/`-guide`/`-tutor`; with nothing to run the tutor on, `home-ai-blocked` (the red !, in place of `home-info-ai`) and `home-start-tutor` open `home-tutor-gate` (`home-tutor-gate-close`, `home-tutor-gate-add-key`) | `home-mode[data-mode=sprint\|study\|play\|guide\|ai]`, `home-map-progress[data-passed][data-total]` (the current unit), `home-guide-stat[data-completed][data-total]` |
| `/sprint/play` | `sprint-play-main` (the `<main>`: stage, answer sheet and results), `sprint-play-stage`, `sprint-play-result`, `sprint-play-verdict`, `sprint-play-next`, `sprint-play-done`, `sprint-play-done-home`, `sprint-play-again`, `sprint-play-check`, `sprint-play-exit`, `sprint-play-audio`, `sprint-play-sound`, `sprint-play-progress` | `sprint-play-stage[data-kind][data-exercise-id][data-pos]` (the exercise up: its kind, its id as in `src/sprint.rs`, its place; `data-kind=done` at the end), `sprint-play-result[data-state=right\|wrong]`, `sprint-play-done[data-saved=true]` once the session is recorded; tiles and options: `[aria-pressed]` when picked, `[data-state=right\|wrong\|used\|done]`; options and speakers carry `[data-value]` |
| `/login`, `/register` | `login-form`, `login-email`, `login-password`, `login-submit`, `login-error`, `login-register-link`, `login-notice`; dev mode, in the `login-dev` card: `login-dev-login`,  `register-form`, `register-email`, `register-password`, `register-submit`, `register-error`, `register-login-link`, `register-owner-note` (the first account on an invite/closed server), `register-invite` (the hidden invitation token); `register-closed` (the sign-ups-closed page), `register-closed-message`, `register-closed-login`; behind a signing-in proxy (`SKRONGLE_PROXY_AUTH_*`) `/login`, `/register` and `/logout` are `proxy-signin` with `proxy-signin-note` (or `proxy-signin-signed-out` after logout) and `proxy-signin-home`; dev mode, in the `register-dev` card: `register-dev-login` | |
| `/login/code` | `login-code`, `login-code-sent-to`, `login-code-form`, `login-code-input`, `login-code-submit`, `login-code-error`, `login-code-cancel`; dev mode: `login-code-dev` (the floating window), `login-code-dev-frame` (its frame of `/login/code/device`: `login-code-device[data-method]`, `login-code-device-to`, `login-code-device-item[data-code]`, `login-code-device-code`, `login-code-device-remaining`, `login-code-device-use`, `login-code-device-empty`) | `login-code[data-method]`, `login-code-device-item[data-code]` | `login-code[data-method]` |
| `/password/change` | `password-change-gate`, `password-change-info` (the (i)) and `password-change-dialog`, `password-change-form`, `password-change-new`, `password-change-confirm`, `password-change-submit`, `password-change-error`, `password-change-logout` | |
| `/courses` | sections `courses-projects`, `courses-mine`, `courses-builtin`, `courses-make`; each card `courses-card` with `courses-open`, `courses-resume` (stopped with its outline saved), `courses-retry` (stopped before it), `courses-delete-form`, `courses-delete` (stopped cards), `courses-card-stopped`, `courses-watch`, `courses-cancel`, `courses-generating`, `courses-card-phase`, `courses-card-revising`, `courses-card-error`, `courses-card-error-detail`, `courses-settings` (the gear of an opened course) opening `courses-settings-dialog[data-course-id][data-step]` (on a course not opened yet, `[data-wizard][data-start]`: `courses-open` opens it, #587) with `courses-settings-step`, `courses-settings-level[data-unit]` (a level card; `courses-settings-level-radio` its radio, `courses-settings-level-info` its (i)), `courses-settings-prev`, `courses-settings-next`, `courses-settings-save` (Get Started!), `courses-settings-close`, and at the gear dialog's top right (not in the wizard) `courses-edit` (the pencil to the editor, own courses) and `courses-copy` in `courses-copy-form` (#629); `courses-notice`, `courses-create-form`, `courses-create` (Create a course); `?generate=<id>`: the Generate dialog of a stopped course (testids as on `/courses/{id}`) | `courses-card[data-course-id][data-course-status=generating\|revising\|ready\|failed]`, project cards `[data-active]` |
| `/courses/{id}` Generate dialog (a draft or a stopped generation; open on `?generate=1`) | `generate` (the `<dialog>`), `generate-close`, `generate-named` (the names the AI keeps), `generate-own`, `generate-form`, `generate-error`, `generate-wish`, `generate-wish-readonly` (resume), `generate-notes`, `generate-sensitive-note` (the "don't paste sensitive personal data" line under it), `generate-files`, `generate-kind` (each radio), `generate-depth-choice` (the Full course / Preview / Outline fieldset) with `generate-depth` (each radio), `generate-depth-info` and `generate-depth-info-text`, `generate-provider` (each radio: only with two or more choices;), `generate-provider-only` (one choice: a hidden input, no choice shown), `generate-no-key` (with,, ; the same on `tutor-no-key` and `home-tutor-gate`) (no key: the line above the form) with `generate-profile-link`, `generate-disabled` (the form's fieldset, disabled then), `generate-instructions`, `generate-submit` | on `generate`: `data-course-id`, `data-resume`; `[open]` while shown |
| `/courses/generate/{job}` | none: redirects to `/courses/{id}?job={job}` | |
| `/courses/{id}` (the AI job modal: a generation or Add content with AI) | `course-ai-modal-backdrop`, `course-ai-modal`, `course-ai-modal-title`, `course-ai-modal-what`, `course-ai-modal-phase`, `course-ai-modal-bar`, `course-ai-modal-count`, `course-ai-modal-cancel-form`, `course-ai-modal-cancel`, `course-ai-modal-refresh` (no JavaScript), `course-ai-modal-error`, `course-ai-modal-error-detail`, `course-ai-modal-close` | on `course-ai-modal`: `data-job`, `data-state` (`running` \| `failed`; an Only add / Allow overall changes job that changed the course opens `course-review-modal` instead), `data-done`, `data-error`, `data-phase`, `data-adding`, `data-revision`, `data-lessons-done`, `data-lessons-total`, `data-poll-url`, `data-done-url` |
| `/courses/{id}?review=1` (Review AI changes; also opens when an Only add / Allow overall changes job finishes, unless "Don't show this automatically" is ticked) | `course-ai-review` (the *Review changes* button next to Submit), `course-review-backdrop`, `course-review-modal`, `course-review-label`, `course-review-overview`, `course-review-error` (Accept refused), `course-review-close`, `course-review-expand-all`, `course-review-collapse-all`, `course-review-raw` (show the files as JSON), `course-review-list`, `course-review-file`, `course-review-details` (its ▸), `course-review-file-link` (the lesson's row: the summary that opens it), `course-review-position`, `course-review-file-title`, `course-review-tag`, `course-review-summary`, `course-review-exclude-file` (the lesson's −), `course-review-split`, `course-review-whole`, `course-review-row` (a changed line), `course-review-exclude-line` (its −), `course-review-del`, `course-review-add`, `course-review-line` (a line of a one-column file), `course-review-edit-input` (the box a line opens into on click, #287: a textarea holding the raw JSON line), `course-review-history` (the modal's Undo / Redo of edits in the footer, `hidden` until the first edit) with `course-review-undo` and `course-review-redo` (`disabled` with nothing to undo / redo), `course-review-edit-value` (the hidden `edit:<path>:<key>` field Accept sends), `course-review-gap`, `course-review-form`, `course-review-quiet`, `course-review-left-out`, `course-review-accept` (absent once the run is undone), `course-review-undone` (the *Undone* tag of a run undone since, #281) with `course-review-undone-info` (its (i)) and `course-review-undone-text` | `course-review-modal[data-files][data-after-job][data-undone][data-labels]` (`data-labels`: the plain names of JSON keys, for the script), a dimmed id-only cell's `td.rv-quiet`, `course-review-file[data-path][data-kind][data-added][data-removed]` (`kind`: `added` \| `removed` \| `changed`), `course-review-row[data-key]`, `course-review-exclude-line[data-group]` (0: not left out; rows left out together share a number), an editable cell's `td[data-edit]` (any cell on the right of a JSON file, `tr.rv-row.same` ones too; focusable, Enter opens it) with `data-ai` (the line as the AI's file has it) and class `rv-is-edited` while edited (blue; its row `tr.rv-edited-row`, its `course-review-exclude-line` and its lesson's `course-review-exclude-file` `disabled`) |
| `/courses/{id}` (the feature usage report, #328) | `course-report-open` (the round bar-chart button beside the gear; a `#course-report` link without JavaScript), `course-report` (the `<dialog>`), `course-report-close`, `course-report-unit` (a unit, a `<details>`, the first `open`), `course-report-unit-counts` ("2 lessons · 7 of 21 features · 3 of 6 question types"), `course-report-lesson` (a lesson, a nested `<details>`), `course-report-question` (a question type of its quiz), `course-report-feature` (a lesson feature) | `course-report-unit[data-unit]`, `course-report-lesson[data-lesson]`, `course-report-question[data-key][data-used]` (`key`: the quiz type, e.g. `true-false`), `course-report-feature[data-key][data-used]` (`key` from `FEATURES` in `src/report.rs`, e.g. `text`, `vocabulary`, `listening`; `used`: `true` \| `false`) |
| `/courses/{id}` | `course-edit`, `course-edit-locked` (the workspace, `inert`, under a running job's modal; no `course-edit-cancel` or busy banner then), `course-edit-cancel-form`, `course-edit-cancel`, `course-edit-open-form`, `course-edit-open`, `course-edit-notice`, `course-edit-generating`, `course-edit-error`, `course-edit-files`, `course-edit-file`, `course-edit-file-link`, `course-edit-file-delete-form`, `course-edit-file-delete`, `course-edit-new-file`, `course-edit-manifest`, `course-edit-export`, `course-edit-delete-form`, `course-edit-delete`, `course-edit-revising`, `course-ai`, `course-ai-form`, `course-ai-error`, `course-ai-text`, `course-ai-sensitive-note`, `course-ai-mode-choice` (the Add / Edit / Add and edit choice, a `<fieldset>`, #1241), `course-ai-mode` (each of its radios, `name="mode"`, `[value=add\|edit\|change]`; `change` is `disabled` on a course that cannot be revised; `e2e/lib/app.ts` `aiMode(page, value)`), `course-ai-mode-info` (the "i", a `<details>`), `course-ai-mode-info-text`, `course-ai-mode-finished-only` (in it: why Add and edit is greyed out), `course-ai-outline` (the *Outline only* switch, #1119: the lessons it writes are outlines; `disabled` while Edit is chosen), `course-ai-outline-info`, `course-ai-outline-info-text`, `course-ai-submit`, `course-ai-provider` (the provider named in `ai-disclosure`, with two or more saved keys only), `course-ai-generate` (a draft: opens the Generate dialog), `course-ai-no-key`, `course-ai-profile-link`, `course-history` (the Undo and Redo icons, in `course-history-row` under the table), `course-undo` (always there; `disabled` with nothing to undo; `data-discard` while the table holds changes that cannot be saved, when it takes those back instead, #288), `course-undo-form` (only when there is something to undo), `course-undo-label` (inside it, not shown: the change it takes back), `course-redo`, `course-redo-form`, `course-redo-label` (the same for Redo), `course-edit-title`, `course-edit-rename` (the title as a button, renames on click), `course-edit-rename-input`, `course-edit-save-status` (the circle by the title; hidden without scripts and while a job locks the page), `course-edit-save-error` (why something is not saved), `autosave-leave-dialog`, `autosave-leave`, `autosave-stay`, `course-lessons`, `course-lessons-list`, `course-lessons-settings`, `course-lessons-status`, `course-lessons-unit`, `course-lessons-unit-title`, `course-lessons-fold`, `course-lessons-count`, `course-lessons-add-here`, `course-lessons-gear` (the gear on the Course settings row: opens `course-lessons-prefs`, the Editor settings dialog, with `course-lessons-pref-json` and `course-lessons-pref-lines` rows and `course-lessons-prefs-close`), `course-lessons-add-unit` (+ Add unit, on the Course settings row right of the gear), `course-lessons-unit-rename` (a new unit's name box), `course-lessons-unit-delete` (a unit's ✕, hidden on the last unit), `course-lessons-unit-dialog` (asks before a unit with lessons goes; `course-lessons-unit-cancel`, `course-lessons-unit-confirm`), `course-lessons-row`, `course-lessons-grip`, `course-lessons-position`, `course-lessons-expand`, `course-lessons-title`, `course-lessons-rename`, `course-lessons-edit`, `course-lessons-duplicate` (⧉, copies the lesson below it), `course-lessons-ai-edit` (✦, Edit with AI on that lesson, #1242: only with a key, not on an unsaved row; opens `course-lessons-ai-dialog[data-reopen]`, with `course-lessons-ai-form`, `course-lessons-ai-title`, `course-lessons-ai-lesson` (the hidden file field), `course-lessons-ai-text`, `course-lessons-ai-count`, `course-lessons-ai-provider` (with two or more keys the provider's name in it is `course-lessons-ai-provider-name`), `course-lessons-ai-error` (a refused request), `course-lessons-ai-cancel`, `course-lessons-ai-submit`), `course-lessons-delete`, `course-lessons-drawer`, `course-lessons-json`, `course-lessons-json-status` (the drawer's status: valid, or its problem in the words of the line under the title), `course-lessons-json-goto` (*Go to line N*, while the drawer has a problem with a line), `course-lessons-format`, `course-lessons-lines` (the box's line numbers, while the Line numbers switch is on), `course-lessons-bad-line` (the problem's number in it), `course-lessons-json-highlight` (the tint over the problem's line in the box), `course-lessons-json-switch` and `course-lessons-lines-switch` (in the gear's dialog), `course-form`, `course-form-field`, `course-form-section`, `course-form-list`, `course-form-item`, `course-form-add`, `course-form-up`, `course-form-down`, `course-form-remove`, `course-form-correct`, `course-form-kind`, `course-form-map-key`, `course-form-kept`; the lesson editor (#841) in a lesson's `course-form`: `course-form-layout` (`[data-tab]` at phone width), `course-form-tab-outline` / `-edit` / `-preview`, `course-outline`, `course-outline-section` (`[data-sec]`, `[aria-expanded]` on a section with items, `[aria-pressed]` on a single one), `course-outline-item` (`[data-sec][data-sub][data-idx]`, `[aria-pressed]`; a `.le-warn` child when the rules would refuse it), `course-outline-add` (`[data-path]`: the list it adds to), `course-outline-json`, `course-item` (the form: `[data-sec][data-sub][data-idx][data-problem]`), `course-form-field` / `-item` / `-list` / `-add` / `-remove` / `-up` / `-down` / `-correct` with `data-path` as before, plus `course-form-type` (the type switch; its buttons `[data-value][aria-pressed]`), `course-form-check` (a field's own checks: `.le-check[data-check][data-level]`, a ✓ or ! in its corner that opens `course-form-check-dialog` with `course-form-check-list` and `course-form-check-close`), `course-form-new-option`, `course-form-add-teaches`, `course-form-add-prereq`, `course-form-prev` / `-next` / `-duplicate`, `course-form-bucket-prev` / `-next`, `course-form-add-sort-item`, `course-form-add-bucket`, `course-form-remove-bucket`, `course-form-words`, `course-form-cards`, `course-form-uses`, `course-form-step`, `course-form-add-step` (`[data-value]`: the kind), `course-form-guide-by-hand`, `course-form-guide-derive`, `course-form-say`, `course-form-schema`, `course-form-mode` (the preview's Study / Sprint / Guide buttons, `[data-value]`); the preview `course-preview` (`[data-mode]`), `course-preview-body` (`[data-key]`: what it shows, drawn again 120 ms after the last keystroke; wait for it before answering in the preview), `course-preview-phone` (`[data-mode]`), `course-preview-none`, `course-preview-question`, `course-preview-option`, `course-preview-answer`, `course-preview-tile`, `course-preview-bucket`, `course-preview-chip` (a word sorted into its bucket), `course-preview-word`, `course-preview-check`, `course-preview-feedback` (`[data-state=right\|wrong]`), `course-preview-again`, `course-preview-flashcard`, `course-preview-stage` | `course-edit[data-course-id][data-course-status][data-loads]` (`data-loads` follows each save), `course-edit-save-status[data-state]` (`saved`, `saving`, `error`), `course-ai[data-course-id][data-has-key]`, `course-edit-file[data-path]`, `course-lessons[data-started][data-folders][data-version]` (the files' version the page's next save names), `course-lessons-unit[data-unit]`, `course-lessons-row[data-lesson-id][data-unit][data-path]` (an added row has an empty id and path until saved), `course-lessons-row[data-problem]` and `course-lessons-settings[data-problem]` (`json`, `rules` or `refused`: the drawer's problem, until the next edit or save; its box is `aria-invalid`), `course-undo-form[data-earlier]`, `course-lessons-drawer[data-mode]` (`json` or `forms`), `course-lessons-json-switch[aria-checked]`, `course-lessons-lines-switch[aria-checked]`; every `course-form-*` carries `data-path`, the field's place in the file joined by dots (`quiz.3.options.1`; on add, the list's path) |
| `POST /api/courses/{id}/preview-answer` | none: JSON `{question, answer}` → `{correct, note, correct_answer, explanation}`, the lesson editor's Try it (owner only, nothing recorded) | |
| `POST /api/courses/{id}/answer-key` | none: JSON `{question}` → `{answer}`, the first answer of a numeric or expression question the checker cannot read (`null` for none), the lesson editor's red check before a save (owner only) | |
| `/courses/{id}/file` | `course-file-form`, `course-file-path`, `course-file-content`, `course-file-status`, `course-file-save`, `course-file-cancel`, `course-file-error` | `course-file-form[data-new]` |
| `/lessons` | `lessons-filter`, `lessons-filter-status`, `lessons-unit`, `lessons-list`, `lessons-item`, `lessons-item-link`, `lessons-item-practise` | `lessons-item[data-lesson-id][data-passed][data-read][data-status][data-search]`, `lessons-filter-status[data-matches]` (set once something is typed) |
| `/lessons/{slug}` | `lesson`, `lesson-head`, `lesson-mastery`, `lesson-read-form`, `lesson-read`, `lesson-practise`, `lesson-listen`, `lesson-tutor`, `lesson-actions`, `lesson-more` with `lesson-more-toggle`, `lesson-guide`, `lesson-sprint`, `lesson-info` (the (i) by the status) and `lesson-pass-info` (the one in the footer) opening `lesson-info-dialog` with `lesson-pass-rule` and `lesson-info-close`, `lesson-test`, `lesson-prerequisite`, `lesson-content`, `lesson-vocabulary`, `lesson-vocab-item`, `lesson-conjugations`, `lesson-conversation`, `lesson-conversation-form`, `lesson-conversation-start`, `lesson-check`, `lesson-check-form`, `lesson-check-question`, `lesson-check-option`, `lesson-check-answer`, `lesson-check-feedback`, `lesson-check-submit`, `lesson-check-result`, `lesson-check-retry`, `lesson-pass` | `lesson[data-lesson-id][data-passed][data-read][data-seen][data-status]`, `lesson-check[data-done]`, `lesson-check-feedback[data-state=right\|wrong]`, `lesson-check-result[data-state=passed\|failed]`, `lesson-pass[data-passed]`, `lesson-prerequisite[data-lesson-id]` |
| `/profile` | `profile-nav`, `profile-nav-link`, `profile-form`, `profile-notice`, `profile-display-name`, `avatar-blank` (the gear in the top-right corner), `profile-save-status`, `profile-save-error`, `profile-first-name-error`, `profile-last-name-error`, `autosave-leave-dialog`, `autosave-leave`, `autosave-stay`, `profile-password-open`, `profile-password-dialog`, `profile-password-rule` (the length rule), `profile-password-cancel`, `profile-first-name`, `profile-last-name`, `profile-email` (read-only), `email-verified` (the check inside a confirmed address), `profile-privacy-info` (the (i) button), `profile-privacy-note` (the dialog it opens), `profile-privacy-close`, `profile-public-name` (the name others see, live), `profile-password-form`, `profile-password-current`, `profile-password-new`, `profile-password-confirm`, `profile-password-submit`, `profile-password-error`, `profile-sign-in` (how the account signs in), `profile-sign-out-others-form`, `profile-sign-out-others`, `profile-sign-out-others-info` and `profile-sign-out-others-info-dialog` (`profile-sign-out-others-info-close`), `profile-role` (the word Owner under the name, owners only), `profile-api-keys`, `profile-api-key` (a `<details>` row), `profile-api-key-toggle`, `profile-api-key-status`, `profile-api-key-error`, `profile-api-key-input`, `profile-api-key-remove-form`, `profile-api-key-remove`, `email-verified` (the ✓) (the address's state), `profile-two-factor` (the card), `profile-two-factor-info`, `profile-two-factor-info-dialog`, `profile-two-factor-form`, `profile-two-factor-choice` (a chip), `profile-two-factor-error`, `two-factor-totp-dialog`, `two-factor-totp-qr` (the inline SVG; `data-modules` its width), `two-factor-totp-secret`, `two-factor-totp-qr` (the QR's wrapper; `data-uri` is the `otpauth://` link it encodes), `profile-2fa-totp-qr-info` (the (i) by the dialog's title) and `profile-2fa-totp-qr-info-dialog`, `two-factor-totp-dev` (dev mode), `two-factor-totp-form`, `two-factor-totp-code`, `two-factor-totp-confirm`, `two-factor-totp-cancel`, `two-factor-totp-error`, `profile-two-factor-totp` (the added app's line), `profile-two-factor-totp-remove`, `two-factor-confirm-dialog` (Confirm it's you: any two-factor change), `two-factor-confirm-what`, `two-factor-confirm-form`, `two-factor-confirm-choice`, `two-factor-confirm-password` (or `two-factor-confirm-code` for an account without a password), `two-factor-confirm-submit`, `two-factor-confirm-cancel`, `two-factor-confirm-error` | `profile-save-status[data-state]` (`saved`, `saving`, `error`), `[data-saved]` on a saved field's `.field-wrap`, label or chip, `profile-api-key[data-provider][data-key-set][data-unreadable]`, `profile-nav-link[data-section]` (the id it jumps to), `profile-two-factor[data-on]` (`true`, `false`), `profile-two-factor-choice[data-on]` |
| `/manage` (owners only; `nav-manage` in the account menu) | `manage-page`, `manage-info` + `manage-info-dialog` (`manage-signup-mode[data-mode]`), `manage-notice`, `manage-error`; in invite mode `manage-invites-card`, `manage-invite-form`, `manage-invite-make`, `manage-invite-new`, `manage-invite-link`, `manage-invite-copy`, `manage-invites`, `manage-invite-row`, `manage-invite-revoke`; `manage-accounts-card`, `manage-accounts`, `manage-account-row[data-manage]`, `manage-account-temp-password`, `manage-account-two-factor-off`, `manage-account-make-owner` (rows that are not owners; each `[data-account-email]`), `manage-password-dialog` / `-form` / `-confirm`, `manage-two-factor-dialog` / `-form` / `-confirm`, `manage-owner-dialog` / `-form` / `-confirm`, `manage-temp-password-dialog`, `manage-temp-password-new`, `manage-temp-password`, `manage-temp-password-copy`, `manage-temp-password-close` | `manage-account-row[data-manage]`, `manage-signup-mode[data-mode]` |

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
creation flow or lesson tests use to the table. What the browser specs
rely on:

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
  runs together or is a symbol: "Open Study mode", 
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
quiz type, each Sprint mode exercise, Listening, the paradigm drill, Date Dash, the Guide (hand-ordered and derived) and the legacy field names. A browser test can
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
step (new enum variants don't compile until they are named there). Features with no field of their
own are listed in its `AUTHORING.md`.

## Browser tests

```sh
scripts/e2e.sh                          # all specs
scripts/e2e.sh --grep "reading check"   # any Playwright option works with e2e.sh
npx --prefix e2e playwright show-report e2e/results/report   # after a failure: steps, trace, screenshot
```

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
| `e2e/specs/profile-autosave.spec.ts` | The profile saves as you edit: one request a second after typing stops, the circle's states, a field that needs attention (red at once, not sent, the leave dialog and the browser's prompt), a refusal from the server, a lost connection that saves once back online, no API keys (they are the server's, `manage-keys.spec.ts`), and the green check on a saved field |
| `e2e/specs/manage-keys.spec.ts` | The server's AI API keys on Manage Users (#25, #24): one row per provider, folded, each Set or Not set, saved on paste or 3 s after typing, a key for the wrong box refused; a key the owner saves serves another account, and with none that account is told to ask the owner; a DeepSeek key answers the tutor through the dev provider; an Ollama row takes a base URL and a model. Puts the seeded keys back at the end |
| `e2e/specs/pathways.spec.ts` | Exact specs for the essential flows of #36: switch course (Spanish, a generated course, back) by `nav-course-switch[data-course-id]` and `courses-card[data-active]`; generate from notes and uploaded files (`generate-notes`, `generate-files`); cancel a slow generation and resume it to a ready course; resume a run stopped on a lesson (the resumed job keeps what was written), then write that lesson by hand; open the editor between the outline and lesson 1 and see its modal follow the job to the end; remove an API key (`profile-api-key[data-key-set]` flips back). Runs in CI with the exact specs |
| `e2e/specs/editor-autosave.spec.ts` | The course editor saves as you edit (#151, #190): no Save button; a lesson's JSON sent 3 s after the last keystroke (on Playwright's clock) or when its drawer loses focus, or at once when the tab is hidden; red with the reason for JSON that doesn't parse and for a table the server refuses (`422`), nothing stored, the text kept, the leave dialog; a failed save retried; a tab that is behind another refused (`409`) and asked to reload; Use AI saves what waits first and the locked editor sends nothing. Other specs make the waits short with `fastAutosave(page)` (`e2e/lib/app.ts`: it sets `window.AUTOSAVE_TEST_MS` before the page's scripts run) and wait with `editorSaved(page)`. Runs in CI with the exact specs |
| `e2e/specs/forms.spec.ts` | Forms mode in the lesson table's drawer: a lesson in the lesson editor (add a word from the outline, a multiple-choice question with the right-answer radio, the dashed new-option box, reorder, remove, the type switch), see them as JSON, a JSON edit shown back in the form, saved as typed, reload, the table and the file; a field the editor doesn't know kept; the JSON switch; the course settings as a schema form (a new unit becomes a folder) at phone width. Runs in CI with the exact specs |
| `e2e/specs/lesson-editor.spec.ts` | The lesson editor (#841), against a copy of the test course's "Everything at once" lesson: the outline's sections, items and badges and which preview modes show each kind; a multiple-choice question (picked answer, dashed new-option box, retyping keeps the answer, the type switch to true-false and numeric and back, ↑ ↓, saved to the file with the rest untouched); checks as a mark in the field's corner (a modal holds the text; clearing an explanation moves nothing below it, and the mark stays on its label's row in a two-column row, #1320), in the outline and from the server's refusal (a word renamed carries into the questions that teach it; a heading renamed breaks a Guide step, and the line under the title opens the Guide on it); Try it in Study, Guide and Sprint through `/api/courses/{id}/preview-answer`; the Sprint exercises (buckets and ◂ ▸, the wrong word by a click, tiles and distractors, the sort played in the preview); words, listening sentences, conversation lines and a conjugation table; add, duplicate and delete from the outline, Guide steps by hand and back to derived, prerequisites; the Outline / Edit / Preview tabs at phone width. Runs in CI with the exact specs |
| `e2e/specs/lesson-editor-rules.spec.ts` | The lesson editor's forms against the course rules (#1200), each path ending on a save the server accepts: a type switch to numeric or expression with an answer the checker cannot read (`/api/courses/{id}/answer-key`), the option that was the answer deleted, a word a question names deleted, a question the Guide names deleted, a duplicated word, question and conjugation, a sort's bucket and item limits and a deleted bucket's items, a spot's word that appears twice and a word in brackets, a conversation's empty line and its two-line least, a Guide word count that is no whole number, an optional box of spaces. Runs in CI with the exact specs |
| `e2e/specs/words-due.spec.ts` | Words due (#1360), seeded through `POST /dev/vocab`: no card until words are due; the card's count, three words, (i) and button; the due round (a miss comes back once at the end, the summary's "Back tomorrow" and "Next" lines); the card gone after; the card stacked on a phone. Runs in CI with the exact specs |
| `e2e/specs/dev-ai.spec.ts` | [The dev AI provider](#the-dev-ai-provider): a course generated from the dialog with the seeded key shows in `/dev/ai`'s log; an edited fixture, a used-up limit, a request limit and an injected timeout reach the learner; the wire API refuses unknown keys; Manual replies holds a request, folded at the bottom of `/dev/ai`, for an answer typed there |
| `e2e/specs/docs.spec.ts` | The docs (#97): `/docs` logged out, its two sidebar sections (`docs-section[data-section=using|working]`), every user guide as HTML and raw Markdown, `/llms.txt` naming the course format and schema (every link it lists answers 200), no page wider than a phone, every in-app link and `#heading` on every page resolves. Runs in CI with the exact specs |
| `e2e/specs/i18n.spec.ts` | [Languages](i18n): the picker on `/register` sets the `lang` cookie and the page comes back in Spanish, the account keeps the choice (it wins over a cleared cookie at the next sign-in), the profile's picker (`profile-lang`) saves at once and reloads in English; a browser asking for `es-MX` gets Spanish before any pick, `fr` gets English, `POST /lang` refuses an unknown code. Runs in CI with the exact specs |
| `e2e/specs/lesson-tables.spec.ts` | Lesson pages fit a phone (390 px, #465): the test course's lessons with tables and five Spanish lessons with wide paradigm tables are no wider than the screen; Markdown tables (`table-scroll`), the vocabulary (`lesson-vocabulary-scroll`) and the paradigm table (`lesson-conjugations-scroll`) scroll inside their own box. Runs in CI with the exact specs |
| `e2e/specs/course-copy.spec.ts` | Copy a course (#629): Copy on the Spanish card opens *Copy of Spanish* in the editor with every file of the folder, lists it beside the untouched original, and a copy of the copy is *Copy of Copy of Spanish*; a fresh account copying Spanish until its 32 MB are full gets the "no room for a copy" notice and no extra course |
| `e2e/specs/course-translate.spec.ts` | Copy and translate (#630), from `translate-course.json`: a fresh draft translated to Español through the dialog behind the icon in the course's settings (what is translated behind its (i)) is followed to its end in the editor and comes out as *Untitled course (Español)*, ids kept and the answer still one of the options, the original untouched; the Spanish card's dialog offers no English |
| `e2e/specs/modals.spec.ts` | Closing by clicking outside: a Sprint mode stop's sheet (`sprint-sheet`) closes on a tap outside the stop and on Escape, a tap inside keeps it (#233); modals close on a click on their backdrop (#226, `static/modal.js`): the profile's dialogs (not on a drag out of them, not with a password typed), the Generate dialog (not with a wish typed), not a running job's box (`data-modal-static`), Review AI changes by its Close link. Runs in CI with the exact specs |
| `e2e/specs/smoke/` | [Smoke specs](#smoke-specs): a new account passes lessons in every mode, answering from the course files; the whole test course, right and wrong |
| `e2e/lib/app.ts` | Exact helpers: `devLogin`, `devReset`, `correctAnswers`, `playSprintSession`; `TEST_COURSE` and `courseDir` for [the test course](#the-test-course) |
| `e2e/lib/key.ts` | `AnswerKey`: the right answer to a question id or a Sprint mode exercise id, read from the course files |
| `e2e/lib/answer.ts` | `answerQuestion` (Study and Play, by `data-qid`), `passReadingCheck`, `playPractice`, `playStage` |
| `e2e/lib/sprint-play.ts` | `playSprintRight`: a Sprint mode session played with the right answers; reports the exercise kinds it met |
| `e2e/lib/learner.ts` | A fresh account (`newAccount`; `signUp` fills `/register` and waits for the signed-in page; `register` signs up and opens a course; `login`, `logout`), `/api/progress`, and the check that every page agrees with it |
| `e2e/lib/fixtures.ts` | The smoke specs' `test`: seeded `Math.random` and a fake voice in every page; prints the seed on failure |
| `e2e/results/` | Last run: `last-run.json`, the HTML report, traces (untracked) |

## Smoke specs

```sh
scripts/e2e.sh specs/smoke                  # just these (they also run in every scripts/e2e.sh)
E2E_SEED=7 scripts/e2e.sh specs/smoke       # another seed; a failure prints the one to replay with
```

Cheap, deterministic browser tests of the essential paths, right and wrong, run on
every change (and by `.github/workflows/smoke.yml` on every PR, on GitHub-hosted runners, as separate checks: `cargo test`, `scripts/smoke.sh`, the exact specs and the smoke specs, each e2e suite in shards (see "CI runners" below)): no model, no
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
- `test-course-games.spec.ts`: the games and the tutor, on
  the test course.
- `phone-width.spec.ts` (#201): as the seeded dev user at 390×844 (`isMobile`),
  the main pages, a lesson in each mode and the course editor are exactly
  390 px wide (`documentElement.scrollWidth`): nothing scrolls sideways.
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
`src/sprint.rs`), and the key looks it up, old field names included. So a spec survives edited, reordered and new questions,
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
a reason to add it to the template ([Markup hooks](#markup-hooks)). 

## Releases (maintainers)

Not for self-hosters: how a release is made. A release is a tag that matches the `version` in
`Cargo.toml` (bump it, merge, then tag main; the workflow refuses a tag
that doesn't match). Pushing `v<version>` runs
`.github/workflows/release.yml`, which builds both architectures with
`scripts/build-pi.sh` (cargo-zigbuild, glibc 2.31), packs each with
`scripts/package-release.sh` and attaches the tarballs and checksums to the
GitHub release of that tag (created with generated notes if it doesn't
exist yet). The binary reports the tag's version (`skrongle --version`).

Before the tag, on the commit you will tag:

1. Bump `version` in `Cargo.toml`, and add a `## 0.2.0 (<date>)` section to
   `CHANGELOG.md` with what changed and what to back up. The release
   notes are that section; the workflow fails without it.
2. Run `scripts/release-check.sh 0.2.0`. It checks the Cargo version and the
   changelog section, and on Linux with a tarball in `dist/` (from
   `scripts/package-release.sh`) it unpacks the tarball and checks that
   `skrongle --version` prints the version. Without a tarball it skips that
   part and says so.
3. Check the tarballs on every architecture: `scripts/release-qa/matrix.sh dist`
   runs each in a Debian container of that architecture ([Release QA](#release-qa)).
   After the tag, `scripts/release-qa/matrix.sh 0.2.0` does the same with the
   published tarballs, before you announce it.

```sh
git tag v0.2.0 origin/main && git push origin v0.2.0
```

*Actions → release → Run workflow* builds the same tarballs as workflow
artifacts without making a release. To check the layout locally:
`scripts/build-pi.sh aarch64 && scripts/package-release.sh 0.0.0 aarch64`.

## CI runners

CI runs on GitHub-hosted `ubuntu-latest` runners. A PR's `checks` workflow
(`.github/workflows/smoke.yml`) is eight jobs: `cargo test` (which checks
that a build without the `dev` feature compiles, `cargo check
--no-default-features`, lints with `cargo clippy --features dev --all-targets
-- -D warnings` (lint levels are in `Cargo.toml` under `[lints.clippy]`; a
warning fails the job), runs `cargo test --features dev` and then
`scripts/smoke.sh`), three exact-spec shards and four smoke-spec shards.

- **Shards**: the e2e specs run one at a time (they share the dev user), so
  each suite is split with Playwright's `--shard`, one job per shard with
  its own server (`.github/actions/e2e`). Playwright splits by spec file,
  so a file always runs whole, and files are dealt out in name order by test
  count, not time: after adding a long spec file, check the split with
  `npx playwright test specs/smoke --list --shard 1/4` (in `e2e/`). The
  specs' type check and unit tests run once, in exact shard 3. A failed
  shard keeps its report as `playwright-report-<suite>-<n>`. To rerun one
  shard locally: `scripts/e2e.sh specs/smoke --shard 2/3`. In CI a shard's
  database is in RAM (`E2E_DB=/dev/shm/e2e.db`).
- **Gate jobs**: `e2e exact specs`, `e2e smoke specs` and `server smoke test
  (scripts/smoke.sh)` pass only when every shard (or smoke.sh) passed. They
  carry the names the ruleset requires, so a shard count can change without
  touching the ruleset. The `pr-title` and `release` workflows are not
  required.
- **One build per job**: every job that needs the server gets it from
  `scripts/ci-build.sh` (`SERVER_BIN` makes `e2e.sh` and `smoke.sh` skip
  their build). Each job builds once.
- **Caches**: the Rust build uses `Swatinem/rust-cache` and the npm
  install uses `setup-node`'s npm cache; both are GitHub's caches.
- **No run on pushes to main**: `checks` runs only on PRs. A PR's run tests
  it merged into `main`, and the ruleset requires a PR to be up to date
  with `main` before it merges. A PR that falls behind shows "This branch
  is out of date"; update it, which runs its checks again.
- **CI finds failures; debug them locally.** A job's output is kept short:
  the pass/fail lines, the failing spec's error and the server log tail, and
  a Playwright report as an artifact when a shard fails. Builds carry only
  line tables (`CARGO_PROFILE_DEV_DEBUG=line-tables-only` in `smoke.yml`),
  which compiles and links much faster; panics still name file and line.
  To dig into a failure, rerun the one spec or test in your own terminal
  (`scripts/e2e.sh specs/smoke/x.spec.ts --grep "..."`, `cargo test name`),
  where the dev profile has full debug info. For the rare failure only CI
  shows, turn full debug info on for CI with
  `gh variable set CI_DEBUGINFO --body full`, rerun, and
  `gh variable delete CI_DEBUGINFO` afterwards.
