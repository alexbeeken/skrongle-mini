# Developer documentation

For working on Skrongle's code. Start with `CLAUDE.md` in the repository.

- [Architecture](architecture): how the server is built, the database
  schema, configuration, deployment.
- [Testing locally](testing): dev login, seed data, answering the model's
  requests by hand or from fixtures, `data-testid` hooks, browser tests.
- [QA-testing a pull request](qa): clicking through a PR, leaving e2e specs
  behind, writing the merge verdict.
- [E2E coverage audit](e2e-audit): every flow the browser tests walk, set
  against every route and surface, with the gaps and what to add next
  (a snapshot of 2026-10-17).
- [Languages (i18n)](i18n): the app's own text in the learner's language,
  one `i18n/<code>.yaml` per language.
- [Security review](security-review): what was checked before going
  public, what was fixed, and the risks accepted.
- [Scalability](scalability): what breaks first if 10,000 people sign
  up in one day. It has measured numbers, ranked bottlenecks, the first five fixes.
  [On AWS](scalability-aws) asks the same for the cloud stack alone.
- [Load testing](load-testing): a proposal for spike-testing the app
  with k6 on the Mac, in a sized Docker container and on a throwaway AWS
  stack, so bottlenecks show up before a crowd does.
- [AWS setup](aws-setup): the account, IAM user, keys and tools needed
  before `scripts/deploy-all.sh aws`.

The contract between content and code is the [course format](course-format)
and its [JSON Schema](course.schema.json): `src/ai.rs` embeds the format in
the generator's prompt, and `cargo test` checks the schema against the test
course. Change a lesson field and you change both. The user guides
(`getting-started`, `course-structure`, `making-courses`, <!-- themes -->`using-themes`,<!-- /themes -->
`faq`) link to those pages instead of copying field tables; keep them true
when a feature changes. The sidebar is `SECTIONS` in `src/routes/docs.rs`;
`/llms.txt` is `docs/llms.txt`.

## Working with Claude

Most changes here are made by Claude Code sessions. A session reads into one
context window, and a window filled with file dumps forgets the start of the
task. These habits keep it small:

- One task or issue per session. Each orca workspace is its own worktree
  with its own session; start a new workspace for a new issue. The
  `backlog-sweep` and `qa` skills already give each issue or finding
  its own agent.
- Explore in a subagent. Questions like "where is this handled" or "which
  files read this field" go to a subagent, which reads the files and returns
  the answer. The main window keeps the change and the test output.
- Run `/clear`, or open a new workspace, when you switch to an unrelated
  task. Old file contents from the last task only take up room.
- Name sessions after the issue (`/rename 913 session habits`), so you can
  find and resume the right one later.
- Read the module layout in [Architecture](architecture) before grepping.
  It names the file for most areas, which saves a search.
- Keep the auto-memory index short. `MEMORY.md` loads into every session
  and stops loading past 200 lines or 25 KB; on 2026-10-25 it was at 101
  lines and 19.5 KB. Write each entry as one line pointing at its own file,
  and merge old entries on the same topic into one.

`CLAUDE.md` asks compaction to keep the issue number, the files changed and
the test or spec commands used, so a long session can carry on after
`/compact`.

## Rotation

`/rotate` (#1136) starts four Orca workspaces at once, each with the bare
prompt `/<skill>`: `/mergeme` lands the PRs marked mergeme, `/backlog-sweep`
opens PRs for workable issues, `/pr-qa-demo` QAs the other open PRs, and
`/qa` has Jev explore the site and file `[qa]` issues (labelled
`needs grooming`, with screenshots). Run it twice a day. The four skills are
written for this shape (#1192): nobody is watching, so none of them stops
to ask. A question only you can answer goes to the Review Desk's Decisions
mode as one file written with `scripts/review-desk/ask.py`; a PR that isn't
ready comes back to the desk's PR queue; a doubtful finding is filed for
grooming in its Issues mode. The backlog sweep also asks about what is
stuck: issues a merged PR already did, old blocked ones, explorations and
hardware items, and issues that read like a bad idea. Each session's agent
limit comes from `.claude/skills/rotate/scripts/budget.sh`, which assumes
all four share the disk. Each skill adds a line to its log in
`~/skrongle-workflow/log/` (`rotate.log`, `mergeme.log`, `backlog.log`,
`pr-qa.log`, `qa.log`) when it ends.

`/rotate` skips a part with nothing to do (no mergeme PRs, fewer than two
buildable issues, no PR needing QA, a `/qa` run in the last 20 hours) and
starts each session on its own model, `sonnet` unless the rotator picks
otherwise (#1314, #1367). The rotator, the session running `/rotate`, is
the one Fable 5.1 in the factory: it starts the four sessions, checks in
on them every 20 minutes and unblocks any that are stuck. Opus is for the
agents that write code; the local Qwen model takes the chores whose answer gets
checked (finding the failure in a CI log, boilerplate a test checks, a
first look at a diff, long reports; never merge conflicts or unslop),
and `QWEN=off` starts a rotation without it. `FORCE=1` starts all four. Every factory
skill follows `.claude/skills/rotate/references/factory-rules.md`: the model
for each kind of agent, at most 10 agents per run (`budget.sh total`),
5-line agent replies, and commands that never stop for a permission prompt.
Agents remove worktrees with `.claude/skills/rotate/scripts/drop-worktree.sh`,
never with `rm -rf` on a variable, because Claude Code's removal check holds
that for a person and nobody watches a factory session.

## Review Desk

A small page on your Mac that walks through the PRs waiting for review, one
at a time (#1121). Start it from the workspace you are in:

```sh
scripts/review-desk/review                       # opens the page in your browser
REVIEW_DESK_DRY_RUN=1 scripts/review-desk/review # prints GitHub and Orca changes instead of making them
```

It runs that workspace's copy of the desk and reads the repo from it, not
from the main checkout. To type just `review` in any workspace, add this to
`~/.zshrc`:

```sh
unalias review 2>/dev/null
function review { "$(git rev-parse --show-toplevel)/scripts/review-desk/review" "$@"; }
```

`unalias` clears an older `review` alias; without it zsh expands the alias
inside the function line and stops with a parse error.

It shows every open PR with a `## QA: PR #N` report that isn't a draft and
isn't labelled `mergeme` or `needs work`. A red card names what is wrong (CI
red or still running, QA says not ready, conflicts with main, changed since
QA). Press **M** to label `mergeme`, **W** for needs work, **S** to skip,
**U** to undo for a few seconds, and the arrow keys to move without marking.
**C** closes the PR on GitHub after a yes on the page
(`gh api -X PATCH repos/OWNER/REPO/pulls/N -f state=closed`, #1239); reopen it
on GitHub if you change your mind.

A **design PR** (title `[N] Design: ...`, or a body whose first line is
`Design: https://claude.ai/artifact/<id>`) shows without a QA report and opens
in design mode: a big **Open the design** link to the artifact where the
screenshots would be, and two buttons. **I** (Make issues) asks which pieces
you picked and gives a prompt to copy: an agent reads the PR and its
artifact, files one issue per piece, then closes the PR. **C** closes it now.
claude.ai refuses to be shown in a frame (`frame-ancestors 'self'`), so the
artifact opens in a new tab.

**W (needs work)** asks "What should the agent fix?" and needs at least 3
words. Start runs, in order: make the `needs work` label if the repo has none,
add it, post your text as a `## Needs work` comment, then open an Orca
workspace that runs `/pr-fix N`:
`orca worktree create --name fix-pr-N --pr N --agent claude --prompt "/pr-fix N"`,
with `--model` and `--effort` from `config.json` (`fix_agent_model`,
`fix_agent_effort`). Your text only goes to GitHub; the agent reads it there.
If a step fails, the desk stops and says which one. When `fix_agent_limit`
(3) `fix-pr-*` workspaces are open, it warns first. A needs work can't be
undone here: close the Orca workspace to stop the agent.

**Run mergeme** shows after the last PR, with a tally. It asks "Start mergeme
for N PRs?" and then opens a `mergeme-<date>` workspace that runs `/mergeme`.
It is greyed out while a `mergeme-*` workspace is open. The header shows the
last line of `~/skrongle-workflow/log/mergeme.log`.

If Orca isn't running, the desk runs `orca open` first. If that fails, it
shows the command to copy.

**Issues** (the switch next to the name, or `#issues` at the end of the URL)
walks through the open issues waiting for grooming, oldest first: a `[qa]`
or `[needs grooming]` tag in the title (any case), or the `needs grooming`
label (#1139). Each card shows the issue's text and a Title box. Edit the
title and press **Enter** to save it. **D** swaps the text for a box with
its Markdown, so you can edit the description too (#1194); **Cmd-Enter**
saves from there, and **D** again shows it rendered, with your unsaved
changes. One save sends what changed (`gh issue edit N --title=...
--body-file -`). **X** takes the tags out of the title box for you, **E**
puts the cursor in it, **C** closes the issue (`gh issue close N`), **S**
skips, and **U** undoes for a few seconds: it reopens a closed issue, or
puts the old title and description back. Saving changes only the issue's
text; the `needs grooming` label stays, so an issue with the label shows
again after Reload.

**Decisions** (the third switch, or `#decisions`) lists the questions that
block work, one per issue or PR (#1163). The factory skills write them to
`~/skrongle-workflow/decisions/` with `scripts/review-desk/ask.py`, one
JSON file each (the format and the rules are in
`.claude/skills/backlog-sweep/references/decisions.md`). Pick an option if
the question offers some, and write your answer in the box; it saves as you
type. **Build prompt** turns every answered question into one prompt for an
agent: a short paragraph saying these are your decisions and which skill's
method to follow for each kind of answer (build it, change the PR, close it,
label it), then the issue or PR number, the question and your answer for
each. Paste it into any Claude session on the Mac. **Copy** copies it and
marks those questions sent, so they don't show again. **Drop** hides a
question that no longer needs an answer.

**Usage** (the fourth switch, or `#usage`) shows the Claude tokens each
rotation used (#1366). `scripts/review-desk/usage.py` reads the transcripts of
the rotation workspaces (`~/.claude/projects/*-{mergeme,backlog,pr-qa,qa}-<date>`,
the session and its agents), adds up the tokens the API reported per model,
and rewrites `~/skrongle-workflow/log/usage.jsonl`, one line per workspace.
The desk runs it on every load (about a second). A workspace belongs to the
`rotate.log` line that started it.

- **Cached** tokens are cache reads: the conversation so far, re-read on
  every step. They are most of it (about 90 %). **Not cached** is the rest:
  new input, new context saved to the cache, and output. Every workspace,
  session and agent in the log carries `tokens`, `cached` and `uncached`.
- **Total** chart: one bar per rotation, split cached vs not cached, or by
  task, model or kind of token.
- **Per task**: one chart each for mergeme, backlog, pr-qa and qa, on its own
  scale, cached vs not cached.
- **Not cached only** rescales every chart to the tokens that aren't cache
  reads, so their trend is readable.
- The table shows each task's tokens, % cached and tokens per unit of work:
  PRs landed (mergeme), PRs opened (backlog), PRs QA'd (pr-qa), issues filed
  (qa). Click a rotation to see its workspaces, their models and their
  biggest agents.

Jev calls and the local Qwen model aren't counted. Run
`python3 scripts/review-desk/usage.py` to print the totals without the desk.

- Code: `scripts/review-desk/` (Python standard library only). Tests:
  `cd scripts/review-desk && python3 -I -B -m unittest`.
- Settings and logs: `~/skrongle-workflow/` (`REVIEW_DESK_HOME` picks
  another folder). `config.json` is made on the first run.
  `log/actions.log` has every action and command.
- Demos: read from `~/skrongle-demos/pr-<N>/` (the format is in
  `.claude/skills/pr-qa-demo/references/manifest.md`).
- It listens on `127.0.0.1` only, with a new random key in the URL each
  start. Stop it with Ctrl-C.
