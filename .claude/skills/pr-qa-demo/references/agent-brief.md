# Brief: QA one pull request with Jev, post the report, save the demo

You are QA for ONE pull request of the Skrongle repo (alexbeeken/skrongle-mini, a Rust/axum
learning app). Your prompt names the PR, the coordinator's checkout (`$BASE`), what
the PR still lacks (the QA report, the demo, or both), and what kind of demo to make.
Do only what it lacks.

1. **QA report**: check every box in the PR's own "How to QA it" list with Jev and exact
   checks, look a little beyond it, and post the report as a PR comment with a verdict
   and the commit you checked.
2. **Demo**: before/after screenshots of the change (and a captioned video with music,
   when the prompt asks), with a `manifest.json`, in `~/skrongle-demos/pr-<N>/` on this
   Mac. The format is `$SK/references/manifest.md`; read it. The Review Desk shows this
   folder to the user. Only the folder's path goes into the PR. Nothing in it is ever
   committed or uploaded.

"Both again: it has new commits since the last QA" means the PR got its own new work
after the last report (the rule is in `$SK/references/manifest.md`, "Stale"). Do the whole job again at today's head. Your folder replaces the old one.

Work only on your PR. Don't re-plan the task, don't QA other PRs, and don't spawn agents:
the work is sequential. Finish the whole job (setup, QA, report posted, demo saved, PR body
updated, cleanup), then report back.

The factory's rules are in `$BASE/.claude/skills/rotate/references/factory-rules.md`.
Read section 4 ("Commands that never wait for a person") before your first command:
nobody watches this session, so a removal that needs approval stalls your run for hours.
In short: don't delete scratch files, send test output to a new folder, write paths out in
full, and remove your worktree with `drop-worktree.sh` (section 6).

Never add or remove a label, `mergeme` and `needs work` above all. The user and the
Review Desk set them.

The skill folder is `$SK=$BASE/.claude/skills/pr-qa-demo`. Its `assets/` and `scripts/`
hold the demo recorder, an example persona, the script that finishes a video, and
`demo-folder.sh`, which checks and publishes the demo folder.

**In a cloud session** (`CLAUDE_CODE_REMOTE=true`): skip step 4 (the demo),
the demo folder and the `## Demo` section of the PR body, and don't wait for CI after
pushing a fix. The demos and the CI runners are on the Mac. Say "no demo
(cloud)" in the report. Clone `target` with `cp -R --reflink=auto` instead
of `cp -Rc`.

## Read first (ten minutes that save an hour)

Read only what this PR needs. Everything you read stays in your context for every later
step, so a page you didn't need costs you all run.

- `CLAUDE.md` is already in your context; don't read it again. Read the
  `.claude/rules/*.md` files for the areas the PR touches, not all of them (`e2e.md`
  when you write a persona or spec, `i18n.md` when the PR adds text, `content.md` for
  course files, `ui.md` for icons and modals).
- `docs/qa.md` sections 1 (claims), 3 (Jev) and 5 (fix, file or note), and its "Summary
  template" (`sed -n '/^## Summary template/,/^## Efficiency/p' docs/qa.md`). Skip
  section 4, the spec-writing pass: write a spec only to guard a bug you fix. This brief
  replaces the rest.
- The PR: `gh api repos/{owner}/{repo}/pulls/<N> --jq '{title, body, base: .base.ref, head: .head.ref}'`
  and its files, `gh api --paginate repos/{owner}/{repo}/pulls/<N>/files --jq '.[].filename'`.
- The diff: `git diff origin/main...origin/<head>`. Read it, because the body never says
  everything. For a stacked PR, diff against its base branch instead of main.
- Let Qwen look first, free (`factory-rules.md`, section 1; never call `qwen_stop`). Save
  the diff to `$S/diff-<N>.patch` (skip this for one over 150 KB) and call
  `mcp__qwen__qwen_ask` with it: "List what this change does that the PR body below
  doesn't mention, and anything that looks like a bug, as `file:line | what`. Nothing
  else.", with the body in the prompt. Its items are places to look, not findings: one
  goes in the report only after you see it on the server. For a failed Playwright run or
  CI job, Qwen can find the failure ("Which test failed, and the first error line, as
  `test | line`. Nothing else."); `grep -n` that line and read around it. If Qwen
  errors, skip it.

## 1. Set up an isolated worktree

Never work in another session's checkout. Your scratchpad is given in your environment;
call it `$S`. Everything below happens in `$WT=$S/wt-pr<N>`.

```sh
cd $BASE && git fetch origin
git worktree add "$WT" origin/<head-branch> -b qa/pr<N>   # push by refspec later, if at all
cp -Rc $BASE/target "$WT/target"        # APFS clone of a warm cargo cache: instant, no disk until files change
cp -R  $BASE/e2e/node_modules "$WT/e2e/node_modules"
cd "$WT" && . ~/.cargo/env && cargo build 2>&1 | tail -2
```

Disk on this Mac is tight, and a full disk takes the CI runners down. Make only this one
worktree, don't run `cargo clean` anywhere else, and remove it at the end (section 6).
Write `$S`, `$WT` and `$D` out as full paths in any command that removes something.

For clicking through, run `./.dev-start --reset --seed --background` in `$WT`. It picks a
random port and prints `site:`. Add `--test-course`, `--mini` etc. if the PR's QA steps
say so; environment variables set before the command reach the server. Each worktree has
its own `dev.db`, so yours doesn't disturb anyone. `./.dev-start --stop` stops it.

## 2. QA the checklist with Jev

Turn the "How to QA it" boxes into a throwaway persona `e2e/personas/_qa-pr<N>.ts`. The
`_` keeps it out of `--list`, and it is never committed. `$SK/assets/qa-persona-example.ts`
is a real one to copy from. Its shape:
- one `goal` per checkbox step that happens in the browser;
- Jev navigates toward the goal from `start`, and `check` tests the end state exactly;
- `then(page, look)` makes the exact Playwright checks for that step. For each check it
  appends one JSON line `{step, ok, note}` to `process.env.QA_OUT`, and it calls
  `look("…")` so the report gets a screenshot.

Do the shell steps yourself (starting the server with a setting, `curl`, reading the log,
running a script) and record each outcome the same way. The helpers are in
`e2e/lib/learner.ts`, `e2e/lib/app.ts` and `e2e/lib/sprint-play.ts`. The persona type is
in `e2e/lib/persona.ts`.

```sh
cd $WT && QA_OUT=$S/qa-pr<N>.jsonl zsh -ic 'scripts/e2e-explore.sh --persona-file e2e/personas/_qa-pr<N>.ts --max-calls 150'
# a server setting the PR needs:  SKRONGLE_MINI=1 QA_OUT=… zsh -ic 'scripts/e2e-explore.sh …'
# one goal again (0-based):       … --goal 3
```

The run writes `e2e/results/explore/<id>-<stamp>/report.md` (with `report.json` and
`shots/`): Jev's findings, its judgements, the route and the cost. Read it. A run costs
about half a cent; keep it to a few runs.

If the steps need a server the explore script can't start (a plain binary, a
`SKRONGLE_*`/`SKRONGLE_*` setting that `scripts/e2e.sh` overrides, two browsers), start it
yourself and point the explore spec at it:

```sh
cd $WT/e2e && E2E_BASE_URL=<url> JEV_EXPLORE_FILE=$WT/e2e/personas/_qa-pr<N>.ts JEV_EXPLORE_MAX_CALLS=150 QA_OUT=… \
  zsh -ic 'npx playwright test --grep @explore --reporter list'
```

Beyond the checklist, spend a little time on what docs/qa.md lists: the states the body
doesn't mention (owner-only, no JavaScript, phone width, doing it twice) and every consumer
of a new state. Run the PR's own specs or tests (`scripts/e2e.sh specs/<file>`,
`cargo test <name>`), so the verdict rests on more than the happy path. Also check the
branch against main: `git merge-tree --write-tree HEAD origin/main` for conflicts, and
whether main gained i18n languages the branch doesn't cover (`cargo test i18n`).

## 3. Bugs

- **Small, clearly wrong, inside the PR's area, fixable in minutes**: fix it on the PR's
  branch with a spec or test that guards it, and say so in the report.
- **Anything else**: search first with
  `gh api -X GET search/issues -f q='repo:alexbeeken/skrongle-mini is:issue <words>'`. If it's new, open
  one with `gh api repos/{owner}/{repo}/issues -f title="…" -F body=@<file>`, saying where it happens, steps in the browser, expected vs seen, and the
  spec to add with the fix. Link it from the report. Never commit `test.fixme` or a report
  file.
- **Already on main** (the diff doesn't touch it): list it under "Not this PR", and file
  it only if it's new.
- Don't rewrite or refactor the PR. You are QA, not the author.
- **A choice only the user can make** (the PR builds the issue one way and
  another reading is as good; two open PRs build the same thing; the PR's
  premise looks wrong for the product): don't decide it in the verdict. Put
  it on the Review Desk's Decisions mode and say so in the report:

  ```sh
  $BASE/scripts/review-desk/ask.py --pr <N> --slug design-choice --asked-by pr-qa-demo \
    --title "<PR title>" --question "<the choice, one plain sentence>" \
    --option "<A>" --option "<B>" --recommended "<A>" --context "<what each leads to>"
  ```

  (`same-thing-twice` as the slug for two PRs; the rules are in
  `.claude/skills/backlog-sweep/references/decisions.md`.) The verdict is
  then "not ready: waiting on a decision (<id>)" when the choice changes
  what the PR should do, or "merge" with the question noted when it doesn't.

## 4. Make the demo

Build the demo in a staging folder, `$D=$S/pr<N>`, and publish it at the end of
section 5. The reviewer should see what the PR changed next to what it replaced, without
reading the diff. Commit and push any fix first (the top of section 5), so the after
shots show the commit your report names.

**Pick the items.** One item per thing the change shows: a page, a dialog, a message, a
phone layout. Usually two to five. Each gets a one-sentence caption for someone who
hasn't read the PR ("The drill setup: each slot now shows its people").

**After first, on the PR branch.** Write `$WT/e2e/specs/_shots-pr<N>.spec.ts` only now
(an explore run loads every file in `e2e/specs`): one test that reaches each item's state
and saves a screenshot. `$SK/assets/demo-template.spec.ts` shows the imports and helpers.

```ts
import { test } from "@playwright/test";
import { devLogin } from "../lib/app.ts";
const OUT = process.env.SHOTS_OUT!;   // absolute: $D
const SIDE = process.env.SHOTS_SIDE!; // "before" or "after"
test.use({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: 2 });
test("shots", async ({ page }) => {
  await devLogin(page, "open=spanish");
  await page.goto("/lessons");
  // ... reach item 1's state with locators that exist on main too (text, role)
  await page.screenshot({ path: `${OUT}/01-${SIDE}.png` });
  // ... item 2, 3, ...
});
```

```sh
mkdir -p $D && cd $WT && ./.dev-start --reset --seed --background
SHOTS_OUT=$D SHOTS_SIDE=after E2E_BASE_URL=$(cat .dev-server.url) \
  npx --prefix e2e playwright test --config e2e/playwright.config.ts specs/_shots-pr<N>.spec.ts --reporter list
```

**Then before, on main, in the same worktree.**

```sh
cd $WT && ./.dev-start --stop
git checkout --detach $(git merge-base origin/main HEAD)
cargo build 2>&1 | tail -1 && ./.dev-start --reset --seed --background
SHOTS_OUT=$D SHOTS_SIDE=before E2E_BASE_URL=$(cat .dev-server.url) \
  npx --prefix e2e playwright test --config e2e/playwright.config.ts specs/_shots-pr<N>.spec.ts --reporter list
git checkout qa/pr<N>
```

The spec file is untracked, so it survives the checkout. Same seed, same clicks, same
viewport on both sides: a pair differs only by the change. For a PR about phones, use
390×844 on both sides.

- Where the PR adds something main lacks (a new link, a new page), the before shows the
  place it would be. The caption says it isn't there on main. Don't skip the before.
- Where main has no place at all (a new script, a CI job), leave `before` out of that
  item. Show the result on a text page (`page.setContent`) as the after.
- A docs-, template- or static-only PR needs no second build: see the harness facts.
- Don't stage or fake anything. If a claim can't be shown, leave it out and say so in
  the report.
- Look at every pair with the Read tool. A blank page, a login screen or a cut-off
  element means the shot is wrong; take it again.

**A video, only when the prompt asks.** Record 30–90 seconds with captions:
1. Copy `$SK/assets/demo-template.spec.ts` to `$WT/e2e/specs/_demo-pr<N>.spec.ts`. It
   throws without `DEMO_OUT`, so copy it only now.
2. Replace the body of `walkthrough` with the PR's story: one `say` per visible claim,
   `click` / `type` for each action, and an `expect` where the result matters. A failed
   `expect` ends the recording, so the video never shows a broken claim as working.
3. Start from the state the step needs (`devLogin`, `/dev/login?open=…`, a fresh account,
   the API), not by clicking through unrelated pages.
4. For a change with no browser UI, show what a browser can show: the docs page it
   changed, a dev JSON endpoint, or real command output on a `textPage` with a caption.

```sh
DEMO_OUT=$S/demo-pr<N>.webm DEMO_TITLE="<PR title>" DEMO_SUBTITLE="PR #<N> · issue #<I>" E2E_BASE_URL=$(cat .dev-server.url) \
  npx --prefix e2e playwright test --config e2e/playwright.config.ts specs/_demo-pr<N>.spec.ts --reporter list
$SK/scripts/finish-demo.sh $S/demo-pr<N>.webm --out $D/demo.mp4
```

`finish-demo.sh` adds a song picked at random from `$SK/scripts/songs/` (720p, about
1 MB a minute). It names the song on stderr; mention it on the report's Demo line. Use
only the songs in that folder, and don't write a new one: the user picks what goes in the
library. It leaves two frames, `$S/demo-pr<N>-f1.png` and `-f2.png`. Look at both with
the Read tool to confirm the captions and the page are visible.

## 5. Push fixes, post the report, publish the demo, update the PR body

If you fixed a bug, commit only that fix and its spec or test. Leave the untracked
persona and shot specs where they are (`git add` only your fix's files); the worktree goes
in section 6, and they go with it. Never commit the persona,
the shots or demo spec, a screenshot, a video, the manifest, `e2e/results`, `dev.db`,
`e2e.db`, `node_modules` or `target`.

```sh
git commit -m "[<I>] <what was fixed>" -m "Co-Authored-By: <the line your system reminder gives>"
git fetch origin && git rebase origin/<head-branch>   # another session may have pushed meanwhile
git push origin HEAD:<head-branch>                     # never --force
```

**The commit you checked.** After your last push, `git rev-parse --short=8 HEAD` is the
commit your report covers. Put it on the report and in the manifest as `checked_at`.
`find-prs.sh` looks at the PR's commits after it: if any is the PR's own work, not just a
merge of main, the PR shows as `stale` and gets QA'd again. If the author pushed while you worked and you didn't check their
commits, give the commit you did check, and say so.

Post the report with
`gh api repos/{owner}/{repo}/issues/<N>/comments -F body=@$S/report-pr<N>.md --jq .html_url`
and keep the URL it prints. Use docs/qa.md's summary template, starting with the title
line `## QA: PR #<N> <title>` (the coordinator finds reports by it). Right under the
verdict, add the line

```markdown
**Checked at** `<8-character commit>`
```

with nothing else on it before the commit. Then add these sections:

- **Checklist**: every "How to QA it" box, in order. Mark each one ✅ passed, ❌ failed
  (what you saw) or ⚠️ not checked (why), and say how it was checked: Jev reached it,
  exact check, or shell.
- **Beyond the checklist**: what you tried outside it, one bullet each.
- **Jev**: goals reached, calls, cost, and its findings worth a human's eye, one line each.
- **Demo**: `Saved on the QA Mac: ~/skrongle-demos/pr-<N>/`, how many before/after
  pairs, and the video's length and song if there is one. Add one line on what it shows.

Write in plain words and short sentences; the author reads the verdict first.

**Write the manifest** to `$D/manifest.json` with every required field from
`$SK/references/manifest.md`: `schema` 1, `pr`, `issue` (the first issue number in the
title, or `null`), `title`, `why` (one plain line: why the change matters), `verdict`
(`"merge"` or `"not ready"`; "merge after X" is `"merge"` with X in `verdict_note`),
`checked_at`, `qa_comment_url` (the URL from posting), `items` (caption, before, after
per item, in order), `video` only if you made one, and `made_at` (`date +%Y-%m-%dT%H:%M:%S%z`).
Then publish it:

```sh
$SK/scripts/demo-folder.sh publish <N> $D
```

It checks the manifest and the files it names, then replaces `~/skrongle-demos/pr-<N>/`
with them. If it prints a problem, fix the manifest and run it again.

Then add a section at the end of the PR body, changing nothing else in it:

```markdown
## Demo

Saved on the QA Mac (not uploaded): `~/skrongle-demos/pr-<N>/` (<k> before/after pairs<, a <s> s video>)
```

To add it, run `gh api repos/{owner}/{repo}/pulls/<N> --jq .body > $S/body-pr<N>.md`, append the
section, then `gh api -X PATCH repos/{owner}/{repo}/pulls/<N> -F body=@$S/body-pr<N>.md --silent`. If the body already has a
`## Demo` section from an older run, replace that section instead of adding a second one.
Leave a `<!-- qa-page -->` line at the top alone; the coordinator owns it.

## 6. Clean up (always, even after a failure)

```sh
<BASE>/.claude/skills/rotate/scripts/drop-worktree.sh <the full path of $WT> --branch qa/pr<N>
```

Write the paths out in full (`/private/tmp/claude-501/.../scratchpad/wt-pr1275`), not as
variables. The script stops the dev server and binaries, removes the worktree, prunes and
deletes the `qa/pr<N>` branch. Never `rm -rf` it yourself.

Keep `~/skrongle-demos/pr-<N>/`. Leave `$D` and the `.webm` files in `$S`: the scratchpad is
temporary, and deleting them is the kind of removal that waits for a person.

## What to report back to the coordinator

At most 5 lines:

1. `PR #<N>: <merge | merge after X | not ready: why>`, checked at `<commit>`.
2. Checklist: `<passed>/<failed>/<not checked>`; bugs fixed (commit) and issues filed, or `none`.
3. Demo: `<k> pairs` (and the video's length), or why there is none.
4. Jev: calls and cost; `qwen <n>` (or `qwen down`).
5. One line the coordinator should pass to the other agents (a harness quirk, a flaky
   spec, a shared file), or `none`.

The details are in the QA comment. The coordinator keeps every reply in its context, so a
long one costs every later turn.

## Harness facts from earlier QA runs (trust them)

**Shell and paths**
- The Jev key is in `~/.zshrc`, so run anything that needs Jev through `zsh -ic '…'`. In
  a non-interactive shell the Jev parts skip silently. zsh gotcha: `"$T:src/x"` applies a
  modifier to `$T`; write `"${T}:src/x"`.
- Playwright's working directory is `e2e/`. From a persona, read repo files with
  `new URL("../../data/…", import.meta.url)`. `QA_OUT` must be an absolute path.

**Jev's limits**
- Jev can't click hidden (`sr-only`) radios such as the motivator chips, can't
  tick the age and terms boxes on `/register`, can't log out through the account menu,
  and gets stuck in the course start wizard on `/courses`. Do those steps yourself in
  `then`, with `signUp`, `logout` and `openCourse` from `e2e/lib/learner.ts`.
- Jev often can't find a lesson by topic on `/lessons`. Give every goal a `start` URL and
  an exact `check`. A goal whose `check` is already true at its `start` hands straight to
  `then` with zero Jev calls: that's how to do exact Playwright work inside the run.
- Jev's "no effect" findings are mostly clicks that rightly did nothing, or a `confirm()`
  that Playwright dismissed (use `page.once("dialog", …)`). Reproduce one by hand before
  believing it.

**Course state**
- Guide stages and Sprint stops stay locked until the earlier lessons are passed, and
  `/guide/<slug>` silently redirects to `/guide`. Unlock with `POST /api/progress/pass`.
  `/sprint/play?lesson=<slug>` (the slug, not the id) works even when locked.
- The harness seeds the random number generator, so a Sprint session is the same on every
  run, and a session doesn't always include every exercise kind. Start a new session if
  the kind you need hasn't come up.
- The explore harness calls `/dev/reset` between goals and after the run. That wipes the
  dev user's active course and a `--seed` server's progress. Start pages with
  `/dev/login?open=<course>&next=<page>`, and restart with `--reset --seed` before each
  explore run. Give a `/courses` goal `start: "/courses"`.
- `e2e/specs/explore.spec.ts` always does a dev login, so it can't run against a plain
  non-dev binary. Patch it locally (don't commit the patch), or run the exact checks as a
  plain Playwright spec.

**Pages**
- "Create a course" opens the Generate dialog over the editor, and it blocks every click.
  Go to the course URL without `?generate=1` first.
- The dev user can prepare one data export a day; use a fresh account for export steps.
- Picking a language on the profile saves and reloads; wait for the load before you
  navigate.
- `page.reload()` after a form POST resends the form. Use `page.goto(url)` instead.
- Give tutor goals a `check` that waits for the first `.msg.tutor` reply.

**Demo recording (video)**
- The template opens `/healthz` before the title card, because `sessionStorage` throws on
  `about:blank`. It keeps the caption inside an open `<dialog>` and centres spotted
  elements above the caption bar.
- For a PR that is only about phones, set `viewport` and `video.size` both to 390×844.
  Otherwise the frame stays 1280×720, with grey beside a narrow page.
- Don't run two Playwright runs in one worktree (they share `results/artifacts`).
- If you broke the code to prove a spec can fail, run `cargo build` again before
  recording.
- Clearing cookies and going to `/login` switches the recorded page to another account.

**Database and migrations**
- `SKRONGLE_SQL_TRACE=<file>` logs every query (docs/testing.md "Query trace"). On the small
  dev database it shows `scan 0` even for queries that would scan a big table, so fill
  the table first. `dev.db` is encrypted (`dev.db.key`), so the `sqlite3` CLI can't open
  it; `e2e.db` it can.
- Several open PRs often add the same next migration number. Name the collision in the
  verdict; don't fix it. It's not the PR's bug.

**Docs-only and content PRs (QA run, 2026-10-29)**
- A PR that only changes a markdown or skill file needs no worktree, build or server:
  render the file at main and at the PR head on a page with Playwright, the new lines
  in green, and shoot that for the before/after pairs.
- `scripts/fact-check.sh` has no `--no-color` option; set `NO_COLOR=1`. Its cache is
  per checkout at `e2e/results/fact-check/cache.json`, so a fresh worktree starts empty:
  copy a warm one from another workspace and the runs cost nothing.
- In the explore harness, start test-course goals at
  `/dev/login?open=test-course&next=/courses`. Started at `/courses`, Jev copied the
  Spanish course instead.
- Any PR that adds a spec to the exact-spec line of `.github/workflows/smoke.yml`
  conflicts with the next PR that does the same (main's `notation.spec.ts` line, for
  one). Name it in the verdict; the fix is to keep both.

**Other**
- Other sessions' worktrees also live under `/private/tmp/claude-501/…`. Touch only your
  own `$WT`. If your scratchpad turns out to be shared, prefix your files with `pr<N>-`.

**Before/after screenshots (QA run, 2026-10-28)**
- After shots on the PR branch, then before shots with `git checkout --detach` of the
  merge base in the same worktree, rebuilt: one worktree per PR is enough.
- Disk was the limit: with under 16 GB free, run one agent at a time.

**Before/after demos (backlog-sweep, 2026-10-25)**
- The two recordings come out different lengths. Stack them with plain `hstack` (no
  `tpad`, no `shortest`): it holds the shorter side on its last frame. Padding with
  `tpad` and `shortest=1` made the video about 10 s too long.
  `$SK/references/side-by-side-video.md` has the command.
- `-deadline realtime -cpu-used 8` encodes the side-by-side in seconds; the
  default settings take 5+ minutes.
- For a docs- or content-only PR, one worktree is enough. Swap in main's copy
  of the file for the "before" (rust-embed reads `docs/` from disk in debug;
  restart with `SKRONGLE_RNG_SEED=1` for a lesson file).
- For a terminal-only change, no server is needed. Load the captured output
  with `page.setContent` and record that. A static server must serve
  `index.html`; a `/healthz` with no extension makes `goto` start a download.
- A still of an element lower on the page loses the fixed "Before/After"
  label. Move the label to the top of the screenshot area before the shot.
- Do the setup in `test.beforeAll` with a plain `browser.newContext()` to keep
  it off camera.

**Before/after demos (backlog-sweep, 2026-10-27)**
- Hold every scripted step to a fixed length (wait until a set time has
  passed since the step began). Then both halves stay in step without padding.
- Setup that plays Sprint uses up random numbers. Restart both servers with
  the same `SKRONGLE_RNG_SEED` right before recording.
- `page.route` set before `/dev/login?next=<page>` missed the page the
  redirect lands on. Log in first, then `goto` the page.
- A template-only PR needs one worktree. Swap in main's template, rebuild
  (askama compiles templates into the binary), record, then restore.
- A change to `i18n/*.yaml` shows only after a server restart.
- A PR that changes how Sprint uses the random numbers matches main only for
  the first session after a restart. Record that first session. To reach one
  exercise, answer one at a time (`playSprintRight(..., { answers: 1 })`)
  until it appears, rather than counting answers.
- Jev presses submit without filling the form, and the browser rightly
  refuses. Do form submissions as exact steps in `then`.
- `gh api repos/{owner}/{repo}/…` fills the placeholders only inside a
  checkout. Elsewhere, write `alexbeeken/skrongle-mini`.
- For a script that calls AWS, put a fake `aws` first on `PATH` that logs its
  arguments. Never use real credentials.

**QA run, 2026-10-30 (34 PRs, no Jev credits, a loaded Mac)**
- When TypeSafe answers 402 ("no available TypeSafe API credits"), Jev is out for the
  whole run. Don't retry it: do every checklist step as an exact Playwright check and
  say so in the report. Jev then costs $0.
- Look at CI once and move on. Runners were killed mid-compile (signal 9, exit 137),
  dropped jobs after 10 minutes, and once could not check out code ("could not read
  Username for 'https://github.com'"). A red check for a runner reason is not the
  PR's fault; say so in the verdict. One agent waited ten hours on a rerun.
- On a loaded Mac (load average 20–76 with the other rotation sessions and the CI
  runners), a cold `cargo build` in a fresh worktree takes about 4 minutes even with
  the cloned `target`, `cargo test` with one filter over 3, and Playwright's first
  browser launch has stalled up to 15 minutes. Run builds and tests in the
  background, and rerun a timed-out spec before believing it. `cargo test` takes one
  name filter only. Bash calls over 180 s get moved to the background on their own,
  and the `timeout` command is not installed.
- Run spec files with `--workers 1` against one dev server. Parallel workers share
  the dev user and log each other out. Start the server with `run_in_background`,
  not with `&` in the Bash call that then runs Playwright.
- `gh api` and `git fetch` sometimes hang for over two minutes or get reset by
  GitHub; background or retry them. `gh api …/actions/jobs/<id>/logs` needs
  `--allow-escape-sequences`.
- The scratchpad is shared with the other PR agents (their worktrees live there
  too). Prefix your files with `pr<N>-`, delete only your own, and never touch
  `/tmp/x`. `pkill -f <pattern>` kills your own command when the pattern matches it.
- **Language.** A language saved on the profile beats the `lang` cookie and survives
  `/dev/reset`. A shots run that dies partway can leave the dev user in another
  language, so set it back to English at the start of every shots spec (`POST /lang`
  with the form field `lang=en` on a dev server). After picking a language on the
  profile, wait for `html[lang]` to change on the profile's own reload before
  navigating; a fixed 1.5 s wait and the `profile-save-status` pill both failed under
  load (`actionTimeout` about 30 s). A change to `i18n/*.yaml` shows only after a
  restart.
- **Before shots with one worktree.** `docs/`, `static/` and course files are read
  from disk in a debug build, so swapping in the merge base's copy and restarting
  (no rebuild) gives the before shots; `git checkout <merge-base> -- <files>`, then
  `git checkout HEAD -- <files>`. Templates and `i18n/` need a rebuild. A design PR's
  "before" is today's screen on the dev server and its "after" is the mockup; a
  design artifact's local copy may sit in a `backlog-*` scratchpad, the artifact
  server wants the UUID or the full claude.ai link, its sections use
  `aria-labelledby` not `id`, and `read_file` with `paths` saves its images to disk
  to serve as `file://`. The Playwright config needs `E2E_BASE_URL` even for a
  `page.setContent`-only spec (a dummy `http://127.0.0.1:1` works). No markdown
  renderer is installed (no pandoc, no `marked`).
- A docs page can pass `toContainText` and still be cut off in the browser by a raw
  tag in the text (`<title>` hid the rest of two pages, fixed in #1285). Check that
  the text is visible. Docs pages curl apostrophes, so match `/isn.t/`, and a docs
  page's `main` element includes the sidebar. `:has-text()` is not valid inside
  `document.querySelector` in `page.evaluate`. BSD `sed` has no `\b`; use `perl -pi`.
- **Editor.** The editor remembers JSON or Forms mode in `localStorage` (`lt-mode`);
  set it back to `json` before editing JSON again. A question added through the JSON
  drawer needs a `question_id` or the save returns 422. Generate refuses a short wish
  ("Your prompt is too short"); use a full sentence. Closing the Generate dialog
  leaves the page scrolled; scroll to the top before a shot. The test course has 12
  course lints and one advice badge ("Old field names") before any change. On
  `/courses` a generated course has no editor link; read its id from the
  `form[action*='/courses/open/']` actions. A copied course has no gear on `/courses`
  until it has been opened once.
- **Sprint and Guide.** Read the session JSON in `<script id="session">` (its
  `exercises` array) right after `goto` and reload until the kind you need comes
  early; don't play whole sessions under load (`/sprint/play?lesson=notation`, then
  Check until `data-kind` matches, gets a build card fast). Sprint's page width reads
  411–414 px for about a second while a card slides in; measure after it settles. A
  spot card fades in (wait about 1.5 s), and after Check scroll `.sprint-kicker` into
  view. French listening needs `seedPage(page, ["fr", "en"])` from
  `e2e/lib/fixtures.ts`, or the exercise stalls with no voice. Guide quick checks use
  the `stage-option`, `stage-check` and `stage-next-step` testids; `/guide/<slug>`
  resumes at the last step reached, so start from the visible step, and scope
  "continue" clicks to it. In Practice the ear questions come in random order; reach
  one by its `data-qid`. Once a lesson is passed, `/lessons/<slug>` hides its check.
  `POST /api/progress/pass` with `{answered: 20, correct: 20, evidence}` passes a
  lesson. The test course's no-quiz lessons are build, conversation, listening,
  notation, read-only, sort and spot; `/practice?lesson=numeric` and `?lesson=expression`
  show today's maths questions.
- **Stacked PRs.** A fix pushed to a base by its QA agent reaches the stacked PRs
  only when they merge the base in; check `git merge-tree` against the base as it is
  now, and say so. #1268's `cd058af9` added a strict one-staff locator in
  `notation.spec.ts` that any PR adding staff questions to the notation lesson has to
  make `.first()`. A Review Desk PR needs no Rust build: `scripts/review-desk/review
  --no-open --port <p>` with a fresh `REVIEW_DESK_HOME`; without `--no-open` it opens
  a browser tab. `?terms=minor` shows only the newest Terms entry in effect.
  Any code that draws with abcjs `responsive: "resize"` into a box with a max-width
  gets the same blank gap under the staff (185 px on a wide page) unless it draws into
  an inner box, as `notation.js` does since `cd058af9`; `fill-bar.js` (#1275) needed
  the same fix. A second visit to `/guide/<slug>` in one session hides the earlier
  cards and their Continue buttons time out: go through the cards on the first visit.
  A saved language on the dev user beats the `lang` cookie: once a QA step saves one
  (profile language, or `POST /lang`), every spec that sets the cookie fails on that
  server, and `/dev/reset` does not clear it (#1363). Start the server again with
  `--reset` before trusting such a failure.

Talk to GitHub with `gh api` (REST), not `gh pr` or `gh issue`. Those use GraphQL, which
the GitHub proxy in a cloud session refuses with a 403.
