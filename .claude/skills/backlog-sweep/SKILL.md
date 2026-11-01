---
name: backlog-sweep
description: Work the Skrongle issue backlog. Rank the open GitHub issues into one distinct order (skipping blocked, needs-grooming, in-progress and already-finished ones, ties broken at random), start one agent per issue in its own worktree to open a PR, and put every question only the user can answer (an ambiguous issue, a stuck or old one, one that looks like a bad idea, a builder blocked on a choice) on the Review Desk's Decisions mode instead of stopping. QA is pr-qa-demo's job, not this skill's. Use this whenever the user asks to "work the backlog", "sweep the issues", "prioritize the open issues", "pick the next issues and open PRs", "spin up agents on the issues", "what should we work on next", or wants issues turned into PRs in bulk, even if they only ask for the ranking and not the PRs.
---

# Sweep the backlog: rank, ask, build, unslop

You coordinate. Agents do the building. The sweep has five stages, and the
user can ask for a prefix of them: ranking only ("what should we do next"),
or ranking plus PRs. Say which stages you are doing in your first message,
then start. Never wait for a go-ahead.

Nobody watches this session. `/rotate` starts it twice a day with no
instructions, and running it by hand works the same way. So you never stop
to ask: a question only the user can answer becomes one file on the Review
Desk's Decisions mode (`references/decisions.md`, written with
`scripts/review-desk/ask.py`), and you go on with the rest. QA is not your
job either: `/pr-qa-demo` runs beside you and QAs your PRs in the next
rotation.

`$BASE` is this checkout (the repo root). `$SK` is
`$BASE/.claude/skills/backlog-sweep`. `$S` is your scratchpad.

Read `$BASE/.claude/skills/rotate/references/factory-rules.md` before you
start. It covers the model for each kind of agent, the per-run agent
total, keeping this session small, and commands that never stop for a
permission prompt. This skill follows it everywhere.

## In a cloud session

A cloud session (claude.ai/code, `claude --cloud`) has `CLAUDE_CODE_REMOTE=true`.
Check it before you start (`echo $CLAUDE_CODE_REMOTE`). CI runs on the
laptop runners and the decisions folder is on the Mac:

- Stages 1 to 3 run as usual. Stage 2's questions go in the report instead
  of the folder; say so.
- Stage 4 runs with `In a cloud session: open the PR and stop; don't watch
  CI.` added to every builder's prompt. Record each PR as "PR #N, CI not
  watched" on the state table.
- Stage 5 runs as usual. No log line (the log is on the Mac).
- The VM is Linux: clone `target` with `cp -R --reflink=auto`, not `cp -Rc`.

## 1. Collect the issues and strike the ones not to touch

```sh
python3 $SK/scripts/issues.py candidates > $S/candidates.tsv
```

One line per open issue: number, status, labels, PR references, the day it
last changed, title. The status column already applies the skip rules, so
you don't have to re-derive them:

- `skip:blocked`, `skip:on-hold`, `skip:wontfix`: the labels say no. Stage 2
  asks about the old ones.
- `skip:needs-grooming`: the `needs grooming` label **or** `[needs grooming]`
  in the title. Both mean the same thing here. The user grooms these on the
  Review Desk's Issues mode; don't ask about them in Decisions mode too.
- `skip:in-progress`: the `in progress` label, or an **open** PR whose title
  carries `[N]` or whose body says `Closes #N`. Most in-progress issues have
  no label, only the PR, so the PR scan is what catches them. A PR labelled
  `needs work` counts too, and shows as `needs-work:<PR>` in the prs column:
  the Review Desk sent it to a fix agent (pr-fix), which works on that PR.
  Don't build the issue again. List these as "being fixed" in the report.
- `check-done:<PR>`: a **merged** PR claims this issue (`[N]` in its title
  or `Closes`/`Fixes`/`Resolves #N` in its body) but the issue stayed open.
  Open the issue and the PR. If the work is on main, ask whether to close it
  (stage 2, "looks finished"), and leave it out of the ranking. Don't close
  issues yourself. A `Part of #N` is not a claim of completion.
- `design`: a design, research or RFC issue (the title starts with
  "Design", "Designs for", "Design proposals", "Exploration", "Research" or
  "RFC", or the body says the outcome is an artifact). **These are built,
  not asked about.** The deliverable is a claude.ai artifact linked on the
  issue; the builder brief's "A design issue" section says how. An issue
  the script missed but whose body asks for a document, a proposal or a
  study is a `design` row too; treat it as one.
- `skip:designed`: the `designed` label. The artifact is on the issue and
  the user decides what to build from it. Don't make a second one.
- `candidate`: everything else.

Strike a few more by reading, because labels can't see them. Each of these
is a question for stage 2, not a silent skip:

- Decisions and legal items ("Decide", "consult a lawyer"). They end in a
  choice only the user can make. A document the user wants written is a
  `design` row, not a question: never ask "should an agent write the design
  as an artifact?". If it is a design ticket, the output is an artifact.
- Parent or umbrella issues that other issues implement.
- Issues that need hardware or an account you don't have (a real Pi, a
  DietPi submission, the AWS root user).
- Anything whose title is a whole product area ("overhaul", "redesign the
  editor") with no concrete steps in the body. An agent given that opens a
  PR nobody can review.
- Issues that read like a bad idea: against the product focus (customisation
  lives in the course JSON, the modes stay opinionated), against a rule in
  `CLAUDE.md` (a third shape between hosted and mini, a heavy dependency on
  the Pi), or something a learner would be worse off with. Say why, and
  recommend closing or changing it.
- Issues main has quietly fixed. Issues from a QA sweep are often fixed by a
  PR that never mentions them (#668 and #669 were fixed by #529 the day
  after they were filed), so the script can't see it. For each candidate,
  spend a minute on main: open the file or page the issue names and check
  whether the behaviour is still there (`git log --oneline origin/main
  --since=<issue date> -- <path>` shows what touched it). A fixed one is a
  "looks finished" question with the commit as evidence. With more than
  about five candidates, give these checks to `haiku` agents, five issues
  each, that reply one line per issue (`#N fixed by <commit>` or `#N still
  there`). They don't count toward the run's agent total.

## 2. Ask about what's stuck

Everything struck above that waits on the user becomes one question on the
desk, so the backlog can't silt up with issues nobody acts on. Read
`references/decisions.md` first: what makes a good question, the fixed slugs
for the recurring kinds, and the cap. Then, for each:

- **Looks finished** (a merged PR or a commit did it):
  `ask.py --issue N --slug looks-finished --question "PR #M landed this on <date>; close #N?" --option "Close it" --option "Keep it open: something is missing" --recommended "Close it"`,
  with what you checked in `--context`.
- **Needs the user** (legal, hardware, umbrella, vague): one question that
  names the smallest useful next step and offers it: "Do the x86_64 CI half
  now and leave the Pi for you?", "Split this into the three issues listed
  in the context?", options "Yes, next sweep" / "I'll do it myself" /
  "Close it". Not for a design, research or RFC issue: that is a `design`
  row and gets built (stage 4).
- **Old and stuck**: an issue labelled `blocked` or `on hold`, or a
  `[needs grooming]` one, with no comment or label change in 30 days (the
  `updated` column). Slug `still-wanted`: "No movement since
  <date>. Still wanted?", options "Yes, unblock it: I'll say how" / "Keep
  waiting" / "Close it", recommended from what you read. Say in `--context`
  what it waits on, if the issue says.
- **Looks like a bad idea**: say why in one or two plain sentences in the
  question itself, and offer "Build it as written" / "Change it to: …" /
  "Close it", with your recommendation.

`ask.py` refuses to ask an open question twice and a dismissed one again
within 30 days, so run it freely; but read `ask.py list N` before asking a
new question on a number that already has one in other words. Write at most
8 new questions per sweep, oldest issue first, and say in the report how
many more are waiting. The user answers them in Decisions mode, and the
desk's prompt sends an agent to act on each answer.

## 3. Rank the rest into one distinct order

Give each remaining issue a tier. The tiers, in order:

1. **Broken for a learner now.** Wrong answers marked right, a page that
   errors, lost progress, a button that does nothing, a flow that can't be
   finished.
2. **Small, concrete UX fixes.** A clear repro, a clear fix, one or two
   files. Text that lies, a phone overflow, a misplaced control.
3. **Developer and docs fixes.** Flaky specs, a broken script, docs that
   are wrong or unreadable on a phone.
4. **Specified features, and the `design` rows.** The body says what the
   learner should see and how to check it; or it asks for a design,
   research or RFC document, and the artifact is the deliverable.
5. **Big or loosely specified work.** Don't build these; they are stage 2
   questions ("groom it, or build it as the agent reads it?").

Why tiers and not a score: a learner hitting a bug today outranks any new
feature, and within a tier the issues are close enough that arguing over the
order wastes more than it saves. That is what the random tie-break is for.
Write the tiers to a file, one `tier number` pair per line, and let the
script shuffle inside each tier:

```sh
python3 $SK/scripts/issues.py order --tiers $S/tiers.txt --seed 20261024 > $S/order.txt
```

Use a seed and write it in the report, so the user (or you, tomorrow) can
reproduce the same order. Any integer works; the date is a fine seed. The
output is a numbered list with no ties. Show the order with one line per
issue (tier, number, title). If the user only asked for the ranking, stop
here (still write the log line, step 6).

## 4. One agent per issue, one PR per agent

Get ready first:

- `cargo build` in `$BASE` so the agents' APFS clones of `target` start warm,
  and make sure `$BASE/e2e/node_modules` exists (`npm --prefix e2e ci`).
- The builder limit is `$BASE/.claude/skills/rotate/scripts/budget.sh builders`.
  It assumes the other three factory sessions are running beside you and
  sharing the disk, which is the normal case. `0` means under 16 GB free: a
  full disk takes the CI runners down, so write the log line with
  `stopped: <free> GB free` and stop after the ranking.
- The run's total is `budget.sh total` (10 unless `FACTORY_MAX_AGENTS`
  says otherwise). Start no more builders than that in this run, however
  long the order is. The rest are `waiting: next run` on the state table.
- Start `$S/sweep-state.md`: one row per issue in order, with status
  (waiting / building / PR #N / asked <id> / blocked / waiting: next run)
  and the PR. Update it on every message from an agent. It is your memory
  when the session is compacted.

Start the agents in the background with the Agent tool, `model: "opus"`,
working down the order, never more than the limit at once and never more
than the total in all. A docs-only or content-only issue (no Rust, no
template logic) gets `model: "sonnet"`. A `design` row gets `opus` when
the issue asks for mockups, screens or how something looks and feels (the
artifact has HTML mockups in the app's look), and `sonnet` for a research
report or an RFC that is text and examples. Its builder follows the
brief's "A design issue" section: no worktree, no PR, an artifact linked
on the issue and the `designed` label. The brief holds the method, so the
prompt is short:

```
Read <SK>/references/builder-brief.md and follow it.
Issue: #<N> "<title>".
$BASE is <BASE>. Your scratchpad is your own; use it as $S.
Notes from agents earlier in this run: <new facts, or "none">
```

A builder replies in at most 5 lines (the brief says what goes in them).
Don't open its transcript; put the reply on the state table. After each
wave of builders (the limit's worth have come back), run `/compact`, and
hand off as the rules file says if your context still passes about 150k.

A builder comes back with a PR number (or, for a `design` row, "design
#<N> <artifact link>"), or with "asked: <id>" when the issue
could be read two ways or looked wrong once it was in the code (the brief
has it write the question itself with `ask.py`), or blocked on something
mechanical (a key it doesn't have, the runners down). Put each outcome on the
state table and move on to the next issue; never guess a reading for it. A
builder's harness note (a flaky spec, a shared-file clash, a migration
number everyone is taking) goes into the next agents' "Notes" line at once,
and what will still hold next month goes into the brief's "Harness facts".

Two builders can take the same next migration number or add the same key to
`i18n/en.yaml`. That is settled when the PRs merge (the mergeme skill
renumbers), not now. Do tell each builder which other issues are in flight,
so one doesn't rebuild what another is doing.

Labels: no agent in the sweep adds or removes `mergeme`, `needs work` or
`needs grooming`. The user sets them on the Review Desk.

## 5. Unslop everything a person will read

Every PR body and every user-visible string the builders wrote gets the
unslop pass: the text an issue reporter reads in the PR, the i18n strings in
`i18n/en.yaml` (and the other language files, where the agent wrote them),
template text, docs pages. The builder brief asks each builder to do this
before pushing. You do a last pass over each PR body yourself before you
report, and over the questions you wrote in stage 2 (`cat` the files).

Do it yourself, never with Qwen (`factory-rules.md`, section 1): it
flags fine lines and misses real ones. Grep each body for the character
tells first (`gh pr view <N> --json body -q .body | grep -nE '—|“|”|‘|’'`),
then read it against the rules and fix it with `gh pr edit <N> --body-file`.

The rules are the `unslop` skill. It is marked `disable-model-invocation`,
so the Skill tool won't run it for you; read the rules file directly at
`~/.claude/skills/unslop/SKILL.md` and apply them. The patterns it hunts
(em dashes, "not just X but Y", filler, chatbot phrases, bold-label lists,
curly quotes) are exactly the ones agents produce in PR bodies, so skipping
this step is how slop reaches the user.

## 6. Write the log line, then report

Every run that got past the cloud check appends one line to
`~/skrongle-workflow/log/backlog.log`, also a run that stopped early:

```sh
mkdir -p ~/skrongle-workflow/log
printf '%s  %s\n' "$(date '+%Y-%m-%d %H:%M')" \
  'PRs #1143 #1150 · asked 5 · 3 more to ask · seed 20261029' \
  >> ~/skrongle-workflow/log/backlog.log
```

Parts joined by ` · `: `PRs #A #B` (or `no PRs`), `designs #C #D` when
design artifacts were made, `asked <n>` (questions
written), `<n> more to ask` when the cap held some back, `<n> waiting`
when the agent total held issues back, `stopped: <why>`
for a run that ended early, the seed, `qwen <n>` (Qwen calls, or `qwen down`).

Then end with a short report in plain words and short sentences:

- the order you used and the seed;
- how many builders the run started out of its total, and the issues left
  `waiting: next run`;
- a table, one row per issue worked: PR, CI state, the artifact link for a
  `design` row, or the question id;
- the questions you wrote (issue, one line each) and how many are waiting
  for a later sweep; the user answers them on the Review Desk's Decisions
  mode;
- issues skipped because their PR is being fixed (`needs-work:<PR>`);
- what the builders learned that will hold next time, and which of it you
  added to `references/builder-brief.md`.

The PRs get their QA and demo from the next `/pr-qa-demo` run; don't list
demo paths. End the report with this line, as the last thing the user reads:

```
Open the Review Desk: `review`
```

If the `review` command isn't set up on this Mac
(`zsh -ic 'command -v review'` prints nothing), write "Open the Review Desk: run `scripts/review-desk/review`"
instead.
