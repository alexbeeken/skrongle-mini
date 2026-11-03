# Brief: take one issue to a pull request

You build ONE GitHub issue of the Skrongle repo (a Rust/axum/SQLite/askama
learning app) into a pull request that a reviewer can QA from its body. Your
prompt names the issue, the coordinator's checkout (`$BASE`) and what other
issues are in flight. Finish the whole job (worktree, change, tests you
touched, unslop pass, PR open, CI watched once), then report back in at most
5 lines. Don't take on other issues, and don't spawn agents, except one
`haiku` agent to watch CI if you like.

The factory's rules are in `$BASE/.claude/skills/rotate/references/factory-rules.md`.
Read section 4 ("Commands that never wait for a person") before your first
command: nobody watches this session, so a removal that needs approval
stalls your run for hours.

**In a cloud session** (`CLAUDE_CODE_REMOTE=true`): do everything up to
opening the PR, then skip step 6 (watching CI). CI runs on the laptop
runners, and the Mac session watches it. Say "CI not watched (cloud)" in
your report. Clone `target` with `cp -R --reflink=auto` instead of `cp -Rc`.

## Read first

- `CLAUDE.md` is already in your context; don't read it again. Read the
  `$BASE/.claude/rules/*.md` files for the areas your issue touches only
  (they are per area and don't load on their own in your worktree). The parts that bite most often are the course-format contract
  (a new lesson field means `docs/course-format.md`,
  `docs/course.schema.json`, the test course and its e2e specs in the same
  change), "no English in a template" (every string is an `i18n/*.yaml` key in
  every language file), "information behind an icon, not on the page", and
  the two shapes (hosted or Skrongle mini, nothing in between).
- The issue: `gh api repos/{owner}/{repo}/issues/<N> --jq .body` and its comments,
  `gh api repos/{owner}/{repo}/issues/<N>/comments --jq '.[].body'`. Then the code it talks about.
  Issues are often written before the previous PR landed, so check whether
  main already does part of it (`git log --oneline -20 -- <files>`,
  `gh api -X GET search/issues -f q='repo:alexbeeken/skrongle-mini is:pr is:merged <words>' --jq '.items[] | "\(.number) \(.title)"'`).
- The `docs/` sections for the area, not whole pages: `grep -n '^#'
  docs/<page>.md` and read the sections that name your files or feature
  (`features.md`, `architecture.md`, `testing.md`).

## A design issue: the artifact is the deliverable

Some issues end in a document, not code: the title starts with "Design",
"Designs for", "Design proposals", "Exploration", "Research" or "RFC", or
the body says the outcome is an artifact (`issues.py` marks them `design`).
Never ask whether the user wants the artifact; if it is a design ticket,
the output is an artifact. Skip steps 1, 3, 5, 6 and 7: no worktree, no
build, no PR, no `design/*.md` on main (PR #1207 was closed for adding
one). Do this instead:

- Read the issue, its comments and the code or pages it talks about, so
  the design is about the app as it is today. Issue bodies name the UI as
  it was when they were filed (the editor's AI box, for one, now has a
  single "Allow changes to existing content" switch plus Outline only);
  say so in the page where the issue's words are out of date.
- If a question on the issue really can be read two ways, don't ask; write
  the page for the reading you would pick and give the other one a short
  section with what it would change.
- Make the artifact with the claude-artifacts tools: `guide` with topic
  "pages" first, and "design" before drawing mockups, then
  `publish_version` (or `publish_files` for images). One page, titled after
  the issue: a short intro, what the issue asks and what today's app does,
  then each proposal as a section with a mockup drawn in HTML/CSS in the
  app's look (read `static/style.css` for its tokens), what it answers and
  its costs. End with your recommended pick and why. A research or RFC
  issue gets findings, real examples and a recommendation instead of
  mockups. Where screenshots of today's state help, take them from the dev
  server with Playwright and upload them as artifact assets. Keep the page
  under a few MB and check it at phone width (two of five pages needed a
  sideways-scroll fix). Mini stays on a Pi Zero 2 W, so say what a design
  costs there, unless the issue or the user said to leave mini out.
- Unslop the page text (step 4's rules) before you publish.
- Put the link on the issue, as a comment headed `## Design artifact`:
  the link on its own line, then three to six lines with each proposal and
  your pick, then "What to do next: pick one and file the build issue, or
  say what to change." Then add the `designed` label
  (`gh api -X POST repos/{owner}/{repo}/issues/<N>/labels -f 'labels[]=designed'`).
  The issue stays open; the user closes it when a build issue exists.
- Report `design #<N> <link>` on line 1, `none` for CI, migrations and
  files, and the one-line note as usual.

## 1. An isolated worktree off main

Never work in another session's checkout. Your scratchpad is `$S`; the
worktree is `$WT=$S/wt-<N>`.

```sh
cd $BASE && git fetch origin
git worktree add "$WT" -b alexbeeken/<N>-<short-slug> origin/main
cp -Rc $BASE/target "$WT/target"           # APFS clone of the warm cargo cache
cp -R  $BASE/e2e/node_modules "$WT/e2e/node_modules"
cd "$WT" && . ~/.cargo/env && cargo build 2>&1 | tail -2
```

Disk is tight on this Mac and a full disk takes the CI runners down. One
worktree, no `cargo clean` elsewhere, and remove it at the end (step 7).
Write `$S` and `$WT` out as full paths in commands that remove things.

For clicking through your change: `./.dev-start --reset --seed --background`
prints the site URL; `./.dev-start --stop` stops it. Each worktree has its
own `dev.db` and port.

## 2. Decide the reading of the issue before you code

If the issue can be read two ways and they lead to different work, don't
guess, and don't stop to ask in the terminal: nobody is watching. Put the
question on the Review Desk's Decisions mode, clean up (step 7), and report
"asked: <id>":

```sh
$BASE/scripts/review-desk/ask.py --issue <N> --slug which-reading --asked-by backlog-sweep \
  --title "<issue title>" --question "<the choice, in one plain sentence>" \
  --option "<reading A, a few words>" --option "<reading B>" --recommended "<the one you would pick>" \
  --context "<what each reading leads to, and where in the code you looked>"
```

The same when the issue turns out to be a bad idea once you are in the code
(it breaks a rule in CLAUDE.md, or a learner would be worse off): slug
`bad-idea`, say why in the question, options "Build it as written" /
"Change it to: ..." / "Close it", and your recommendation. `open <id>`
printed instead of `wrote <id>` means the question is already waiting;
`dismissed <id>` means the user said "not now" lately. Report the id either
way. The rules are in `$SK/references/decisions.md`.

Small judgment calls (naming, which page a link goes on) you make yourself
and state in the PR body.

## 3. Make the change

- The smallest change that resolves the issue as written. No refactors
  around it, no fixing of neighbours (file an issue if you see one:
  search first with `gh api -X GET search/issues -f q='repo:alexbeeken/skrongle-mini is:issue <words>'`,
  it may exist).
- New markup gets `data-testid`s. A new learner-visible flow gets an exact
  spec in `e2e/specs/` and, if it is in `.github/workflows/smoke.yml`'s
  exact-spec list, an entry there. A new model call gets an `ai::Call` and a
  fixture file.
- A new migration is appended at the end of `MIGRATIONS` in `src/db/migrations.rs`.
  Another builder in this run may take the same number; that's settled at
  merge time, not by you. Mention it in the PR body.
- New text: add the key to `i18n/en.yaml` **and** every other
  `i18n/<code>.yaml` (`cargo test i18n` fails on a missing key). Write the
  other languages plainly; a reviewer who speaks them fixes nuance.
- Run only what you touched locally: `cargo build`, then the one test or
  spec behind the change (`cargo test <name>`,
  `scripts/e2e.sh specs/<file>.spec.ts`). Rerunning a spec doesn't need its
  old results deleted; Playwright replaces them, or pass `--output` a new
  folder in `$S`. CI runs everything once after the
  push. Other worktrees share `CARGO_TARGET_DIR`, so before trusting a
  local test run `find src templates i18n -exec touch {} +`.

## 4. Unslop what a person will read

Before you commit, read `~/.claude/skills/unslop/SKILL.md` and apply it to
every string you added for a learner (yaml values, template text, docs
prose) and to the PR body you are about to write. The Skill tool won't run
`unslop` for you (it is user-only), so read the file. The tells it lists are
the ones agents produce: em dashes, "not just X but Y", "ensuring…",
bold-label lists, chatbot phrases, curly quotes. Fix them before a reviewer
sees them.

Grep for the character tells too: `grep -nE '—|“|”|‘|’' $S/body-<N>.md`.
Do the unslop pass yourself, never with Qwen (`factory-rules.md`, section 1).

Then let Qwen take a first look at your diff, free (never call
`qwen_stop`; skip it if Qwen errors): `git add -A && git diff --cached
$(git merge-base origin/main HEAD) > $S/diff-<N>.patch` (in `$S`, not the
worktree; skip it for a patch over 150 KB), and call `mcp__qwen__qwen_ask`
with it: "List anything that looks like a bug, a hard-coded English string
a learner sees, a debug leftover or a missing test, as `file:line | what`.
Nothing else." It is a pointer, not a review: still read your diff. Qwen
can draft test fixtures and data rows too; the test run checks them.

## 5. Commit, push, open the PR

```sh
git add -A && git status --short            # no dev.db, e2e.db, target, node_modules, e2e/results
git commit -m "[<N>] <what changed, in plain words>" -m "Co-Authored-By: <the line your system reminder gives>"
git push -u origin alexbeeken/<N>-<short-slug>
```

Open it with `gh api repos/{owner}/{repo}/pulls -f base=main -f head=alexbeeken/<N>-<short-slug> -f title="[<N>] <short summary>" -F body=@$S/body-<N>.md --jq .html_url`.
The title must start with the issue number in square brackets; the
`pr-title` workflow fails without it. The body, in this order:

```markdown
## What this PR is for

- One bullet per intent. Short sentences, one to three per bullet. What a
  learner can now do, or what no longer happens.

## How to QA it

- [ ] `./.dev-start --reset --seed --background`, open the printed URL, sign in at /dev/login.
- [ ] One action per box, and what you should see. In the order to do them.
- [ ] Include the "before" state where it helps: "On main this row read X; now it reads Y."
- [ ] `./.dev-start --stop`

Closes #<N>

🤖 Generated with [Claude Code](https://claude.com/claude-code)
```

The QA boxes matter more than they look: the QA agent turns each one into a
check, and the before/after demo is scripted from them. A box that names
the page, the action and the visible result makes both possible; "test the
feature" makes neither.

## 6. Watch CI once, fix what's yours

Watch with a loop that polls `.claude/skills/pr-qa-demo/scripts/find-prs.sh | awk -F'\t' '$1==<PR>'`
until its checks column is no longer `pending`
(there is no `timeout` command on this Mac, and `--watch` has exited while
two checks were still pending). Known flaky specs: the authenticator TOTP
period. If one fails alone,
`gh run rerun <run-id> --failed` once. To find the failure, save the
failed job's log (`gh run view <run-id> --log-failed > $S/ci-<N>.log`) and
ask Qwen "Which test failed, and the first error line, as `test | line`.
Nothing else."; `grep -n` that line in the log and read around it. A real
failure: rerun that one test or spec locally, fix, push.
Checks stuck at Queued usually means the runners are down; say so and move
on. A job that fails with "the self-hosted runner lost communication" or is
cancelled at its time limit before any step ran is the runner (the Mac
slept, or a runner restarted), not your change: rerun it once. Its logs
can't be downloaded afterwards; the check-run annotations say why.

## 7. Clean up

```sh
<BASE>/.claude/skills/rotate/scripts/drop-worktree.sh <the full path of $WT>
```

Write both paths out in full (`/private/tmp/claude-501/.../scratchpad/wt-1241`),
not as variables. The script stops the dev server and the binaries, removes
the worktree and prunes. Never `rm -rf` the worktree yourself, and leave
the rest of `$S` alone: the scratchpad is temporary. The branch stays on
origin; the PR needs it.

## Report back (at most 5 lines)

1. `PR #<n> <title>`, or `asked: <id>`, or `blocked: <why, a few words>`.
2. CI: green / rerun green / red: why / not watched.
3. Migration number and i18n keys added, or `none`; `qwen <n>` (or `qwen down`).
4. Issues you filed, or `none`.
5. One line the other builders should know (a harness quirk, a shared file
   you touched), or `none`.

The details go in the PR body, not the reply. The coordinator keeps every
reply in its context, so a long one costs every later turn.

## A design-proposal issue

Some issues end in a document, not code ("Design proposals: …", "design
proposal for …"). Sweeps on 2026-10-29 made these PRs #1189, #1193, #1203,
#1204 and #1207; do the same:

- Make a private claude.ai artifact with the proposals (the claude-artifacts
  tools: `guide` with topic "pages" first, then `publish_version` or
  `publish_files`). One page: a short intro, then each proposal as a section
  with a mockup drawn in HTML/CSS that matches the app's look (read
  `static/style.css` for its tokens), what it answers and its trade-offs. End
  with your recommended pick and why. Where the issue asks for screenshots
  of today's state, take them from the dev server with Playwright and upload
  them as artifact assets; keep the page well under 16 MB, and check it at
  phone width (two of the five pages needed a sideways-scroll fix).
- The PR adds one file, `design/<slug>.md`: the artifact link on line 3, a
  few lines per proposal and the pick. No app code, so no `target/` clone
  and no build.
- The QA boxes open the artifact and say what to see, then open the design
  file and check the link matches. Say `Part of #<N>`, not Closes: the issue
  closes when a design is picked and built.
- Unslop the artifact text and the design file too.
- Issue bodies name the UI as it was when they were filed. The course
  editor's AI box, for one, no longer has Only add / Allow overall changes
  but one switch, "Allow changes to existing content", plus Outline only.
  Say so in the PR when the issue's words are out of date.

## A design-proposal issue

Some issues end in a document, not code ("Design proposals: …", "design
proposal for …"). Sweeps on 2026-10-29 made these PRs #1189, #1193, #1203,
#1204 and #1207; do the same:

- Make a private claude.ai artifact with the proposals (the claude-artifacts
  tools: `guide` with topic "pages" first, then `publish_version` or
  `publish_files`). One page: a short intro, then each proposal as a section
  with a mockup drawn in HTML/CSS that matches the app's look (read
  `static/style.css` for its tokens), what it answers and its trade-offs. End
  with your recommended pick and why. Where the issue asks for screenshots
  of today's state, take them from the dev server with Playwright and upload
  them as artifact assets; keep the page well under 16 MB, and check it at
  phone width (two of the five pages needed a sideways-scroll fix).
- The PR adds one file, `design/<slug>.md`: the artifact link on line 3, a
  few lines per proposal and the pick. No app code, so no `target/` clone
  and no build.
- The PR title is `[<N>] Design: <title>`, and the body's very first line,
  before "What this PR is for", is `Design: https://claude.ai/artifact/<id>`.
  The Review Desk finds design PRs by those two and opens them in design
  mode (#1239): the artifact link instead of screenshots, and two buttons,
  Make issues (an agent files one issue per piece the user picked, then
  closes the PR) or Close PR. Such a PR needs no QA report to show there.
- The QA boxes open the artifact and say what to see, then open the design
  file and check the link matches. Say `Part of #<N>`, not Closes: the issue
  closes when a design is picked and built.
- Unslop the artifact text and the design file too.
- Issue bodies name the UI as it was when they were filed. The course
  editor's AI box, for one, no longer has Only add / Allow overall changes
  but one switch, "Allow changes to existing content", plus Outline only.
  Say so in the PR when the issue's words are out of date.

## Harness facts (trust them)

- The dev server is for development, not a third product shape. Don't add
  fallbacks "for a self-hoster who skipped SMTP"; mini leaves provider
  features out, the hosted site has them all.
- Run anything that needs the Jev key through `zsh -ic '…'`; a plain shell
  doesn't see `~/.zshrc`.
- Playwright's working directory is `e2e/`; read repo files from a spec with
  `new URL("../../data/…", import.meta.url)`.
- `page.reload()` after a form POST resends the form; use `page.goto`.
- Several open PRs at once usually carry the same next migration number.
  Name it in the body; don't try to avoid it.
- `cargo test i18n` fails on a key missing from any language **and** on a
  key that exists in one language but not `en.yaml`.
- The stash stack is shared by every worktree: never bare `git stash`.
- To make a model call wait so you can test "left mid-turn": `/dev/ai/hold`
  (`do=on`), then `POST /dev/ai/held/{stem}` answers it (PR #847 did this).
- A dev server never has zero accounts (it always creates the dev user). A
  step that needs an empty server starts the plain binary:
  `SKRONGLE_SIGNUP=invite SKRONGLE_MINI=1 SKRONGLE_DB=/tmp/x.db target/debug/spanish-learning`,
  which comes up as Skrongle mini.
- Several builders in one run touch `static/style.css` and `e2e/specs/exact.spec.ts`
  (new imports at the top). Keep to your own block; the conflicts are small.
- The scratchpad is shared with the other agents of the run. Prefix your
  files with `<N>-`, and don't `pkill` by a pattern wider than your worktree.
- Flaky in CI, pass on one rerun after the run ends: `motivator-reward.spec.ts`
  (60 s timeout), `create.spec.ts:179` (generation finishes before Cancel
  lands).
- With several builders in flight, CI queues about 20 minutes before a run starts.
- A docs-only issue needs no `target/` clone and no build; `cargo test docs`
  covers the links. `rust-embed` reads `docs/` from disk in debug builds.
- Test modules live in `src/<mod>/tests.rs` (#909), and `scripts/panic-check.sh`
  skips files named `tests.rs`. A lint or test that walks `src/` should too.
- Talk to GitHub with `gh api` (REST), not `gh pr` or `gh issue`. Those use
  GraphQL, which the GitHub proxy in a cloud session refuses with a 403.
- Check for an open PR on your issue again right before you open yours.
  Another session may have started on it since the sweep began (#1023 and
  #1031 both fixed #1016 on 2026-10-27; on 2026-10-31 two sweeps ran on the
  same backlog, and #1236 and #1237 each got a PR from the other one first).
- With one `CARGO_TARGET_DIR` shared by several worktrees, the binary is
  whichever worktree built last. Copy `target/debug/spanish-learning` out
  right after your own build and run the copy. If it looks stale, run
  `cargo clean -p spanish-learning` and build again.
- A response that sets two cookies must append both `Set-Cookie` headers.
  Inserting into the header map keeps only the last one (#1028, PR #1053).
- A server-opened `<dialog open>` is drawn far down the page with JavaScript
  off. Place it in the page flow (`dialog[open]:not(:modal)`), as PR #1017
  did; #1057 covers the rest.
- Also flaky until PR #1030 lands:
  `routes::tests::a_phone_needs_a_confirmed_address_and_a_number_of_its_own`.
- Since PR #1160 a Sprint sort grades every bucket tap at once and opens its
  sheet when the pile is empty; there is no Check. A spec that taps the first
  bucket until Check appears hangs. Play a sort by tapping each word's right
  bucket (`e2e/lib/sprint-play.ts` does).
- `src/content.rs`, `src/sprint.rs`, `src/ai.rs`, `static/style.css` and
  `static/courses.js` sit at their size baselines (`scripts/file-size-lint.sh`).
  A line added there needs a line removed, or the new code goes in a new
  small file (`src/notation/<type>.rs`, `src/sprint/spot.rs`,
  `src/ai/check.rs`, `static/<feature>.js` with its own `.css`).
- `docs/architecture.md` must name every folder under `static/`
  (`cargo test docs` checks). A new `static/vendor/` needs a line there.
- Flaky too: `course-copy.spec.ts:220` (generation timing, #1144); it passed
  on a rerun.
- Content-only issues: "Spot the mistake" splits a sentence on spaces and the
  wrong word must be one word that appears once, so two words can't be one
  tile without a code change. The validator is in `cargo test course`.
- A `cargo test` job can die with the compiler killed (SIGKILL) before any
  test runs. That is the runner, not your change: rerun the job once (PR
  #1203).
- `i18n/en.yaml` quotes a title or a learner's own text inside a sentence
  with curly quotes (“{title}”), 34 places on main. Keep that convention in
  yaml values even though the unslop rules ask for straight quotes in prose.
- There are six app language files: `i18n/en.yaml`, `es.yaml`, `fr.yaml`,
  `de.yaml`, `pt.yaml` and `zh.yaml`. A new key goes in all six.
- After `createCourse` in a spec, the Generate dialog opens on load and
  blocks clicks. Click `generate-close` first (PR #1307).
- When the four rotation sessions build at once, the Mac's load average
  climbs past 30 and whole CI runs fail with "the self-hosted runner lost
  communication", rerun after rerun (2026-10-30: PRs #1293, #1303, #1304).
  Check `uptime` and rerun once the load is under about 15. A job that lost
  its runner has no log; its annotation says why.
- `gh run rerun` sometimes dies on a dropped connection without starting the
  rerun. `gh api -X POST repos/{owner}/{repo}/actions/runs/<id>/rerun-failed-jobs`
  does the same job; check `run_attempt` went up.
- Flaky too, passed on a rerun: `smoke/wrong-answers.spec.ts:98` (a 5 s click
  timeout on the quiz Check button) and `two-factor.spec.ts:251` (the TOTP
  period).
- From the sweep of 2026-10-29 (fourth run, 36 PRs):
  - A runner job that ends with "could not compile `spanish-learning`" and no
    `error[...]` line, or a Playwright "worker process exited unexpectedly
    (SIGKILL)", is the runner VM killing the process under load, not your
    change. Rerun once after the run ends. "Abandoned" jobs mean the same.
  - In a spec, never switch the app language with `POST /lang` while signed
    in as the dev user: it is saved on the account, `devReset` keeps it, and
    every later spec in the shard gets that language (`dev-ai.spec.ts:334`
    failed five times on PR #1282). Set the `lang` cookie instead.
  - Every question-type lesson in the test course needs at least 3 questions
    that ask only that type (`cargo test test_course`, wrong-answers.spec.ts).
    A new lesson changes the seed draw of `complete-test-course`; prefer
    adding items to an existing lesson. Items in the notation lesson need
    exactly 4 options (the Sprint step of the #1218 test).
  - A Guide stage right after `devReset` is locked and redirects to `/guide`;
    open `/lessons/<slug>` first.
  - Specs in one CI shard share the dev user. A describe that leaves the
    user mid-course (a Practice run, a Sprint) makes the next spec's Guide
    stage locked; end it with `devReset` (#1225's `notation.spec.ts:11`).
  - The i18n test reads `t("editor.js." + key)` as a literal key; build keys
    with a `PREFIX` variable as `lesson-editor.js` does.
  - Chrome has a built-in `window.Keyboard`; the piano component is
    `window.PianoKeys`.
  - abcjs: Playwright needs `{ force: true }` to click a note (the staff lines
    cover it); don't overwrite `data-index` on abcjs elements; a rest is
    `.abcjs-rest`, not `.abcjs-note`, and a signature-only staff draws no note.
  - `cargo fmt -- <file>` formats the whole crate here; don't commit that.
  - `gh pr edit` and `find-prs.sh` can hang when GitHub is slow; `gh api`
    (REST) kept working.
  - A design PR's title is `[N] Design: <title>` and its body's first line
    `Design: https://claude.ai/artifact/<id>` (PR #1249 adds the section
    above with the details).
