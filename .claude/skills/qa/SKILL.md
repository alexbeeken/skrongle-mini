---
name: qa
description: General QA of the Skrongle site with the repo's own Jev scripts. Runs the explore personas (scripts/e2e-explore.sh), the course and test-course playthroughs (scripts/jev-playthrough-*.sh), the Jev journeys and the content checks, reads every report, verifies each finding by hand on the dev server, and files one "[qa]" issue (labelled needs grooming) per real bug or rough edge that isn't filed yet. Use this for "/qa", and whenever the user asks to "QA the site", "find little bugs", "find new bugs", "run Jev over the app", "do a QA sweep", "explore the site for problems", "look for UX rough edges", or wants an exploratory or nightly QA pass, even when no PR or issue is named. /rotate starts it twice a day; it never stops to ask, a doubtful finding is filed for grooming on the Review Desk.
---

# QA the site with the existing scripts

You run the scripts, read what they found, check it, and file what is real.
Nothing here tests a particular PR (that is `pr-qa-demo`) or works an issue
(`backlog-sweep`). The scripts and their reports are documented in
`docs/testing.md` ("Exploratory testing", "Playthrough", "Fact check") and
`docs/qa.md`; this skill is the order to run them in and what to do with
the output.

`$BASE` is this checkout. `$SK` is `$BASE/.claude/skills/qa`. `$S` is
your scratchpad.

`.claude/skills/rotate/references/factory-rules.md` holds the rules every
factory session follows: the model for each agent, the per-run agent
total, keeping the session small, and commands that never stop for a
permission prompt (section 4). Read it before you start.

Say in your first message which groups you are running (see step 2) and
start. Never wait for a go-ahead.

Nobody watches this session. `/rotate` starts it twice a day with no
instructions, and running it by hand works the same way: the full sweep,
unless the user asked for a quick look. You never stop to ask. Where you
are unsure whether something is a bug, file it anyway as a `[qa]` issue
labelled `needs grooming` and say what you doubt in the body: the user
grooms these on the Review Desk's Issues mode, which is the place for that
question. A question that isn't about one finding (how a whole class of
findings should be filed, a script that is broken and needs a choice to
fix) goes to the desk's Decisions mode with `scripts/review-desk/ask.py`
(the rules are in `.claude/skills/backlog-sweep/references/decisions.md`),
and the sweep goes on. The 2026-10-29 run stopped with "security findings
need the user to choose how to file"; the rule now is below, in step 5.

## 1. Get ready

- **Jev key**: `zsh -ic 'echo ${#TYPESAFE_API_KEY}'` prints a number above
  0, or `./.env` / `e2e/.env` has `TYPESAFE_API_KEY=`. Without it every
  script here but the replay stops: write the log line (step 8) with
  `stopped: no Jev key` and stop.
- **Build once**: `cargo build` in `$BASE`, and `npm --prefix e2e ci` if
  `e2e/node_modules` is missing. The scripts build too, but one build up
  front keeps five parallel runs from compiling at once.
- **Disk**: `df -h /`. Playthroughs write a database checkpoint per lesson
  and screenshots; a full disk takes the CI runners down. The other three
  factory sessions share the disk with you. Under 30 GB free, run one group
  at a time; under 16 GB, write the log line with `stopped: <free> GB
  free` and stop.
- **What is already filed**: `$SK/scripts/known-issues.sh > $S/known.tsv`.
  Every open issue and the ones closed in the last two weeks, one per line.
  You grep this before filing anything. Also read the ranges a previous
  sweep filed (the memory notes name them: #396–#428 UX, #465–#491 content
  and bugs) so nothing is filed twice.
- **Cost**: well under $5 for everything. A profile's goals cost under a
  cent, a full-course playthrough a dollar or two per mode. Say the rough
  total in the report.

## 2. Run the scripts

```sh
$SK/scripts/run-all.sh $S/logs
```

It starts the groups in the background, each on its own server, port and
database, with a log per run in `$S/logs/`. The explore runs and the
journeys share Playwright's `e2e/results/report`, so they run one after
another in a single chain (`playwright-chain.log`). The two playthrough
scripts share `./playthrough-<mode>.db`, so they are a second chain
(`playthrough-chain.log`); the content checks run beside both. The groups, and what each is for:

| Group | Script | Finds |
|---|---|---|
| `explore` | `scripts/e2e-explore.sh <profile> --wander 1`, one run per profile in `e2e/personas/` (`impatient`, `accurate`, `classroom`, `testing-bot`) | exact bugs on the pages a persona reaches (HTTP ≥ 400, failed requests, console errors, phone overflow, dead taps), moments Jev judges unclear or frustrating, content it doubts, markup a test can't see |
| `playthrough` | `scripts/jev-playthrough-all.sh` | the whole Spanish course in Sprint, Study and Play as a new account: broken pages, progress pages that disagree, lessons a learner can't pass, phases that get stuck |
| `test-course` | `scripts/jev-playthrough-test-course.sh` | the same for the test course, so every quiz type, exercise and Guide step is met once |
| `journeys` | `scripts/e2e-jev.sh` | the essential flows in `e2e/journeys.ts` and the judged specs; a failing journey is a regression |
| `content` | `scripts/fact-check.sh` | wrong facts in course content (ERROR with a fix, CHECK to verify), copied text |

`ONLY="explore journeys"` picks groups; `LESSONS=5` caps the playthroughs
for a quick pass; `PROFILES="impatient"` picks personas. When the user asks
for a quick look, run `explore` and `journeys` with `LESSONS=5` playthroughs.
When they ask for a full or nightly sweep, run everything.

The scripts take from a few minutes (explore) to an hour or two (a full
playthrough). Don't poll the logs every minute; check back when a `wait`
loop ends or use a Monitor. While they run, do step 3's first half: open the
dev server and click through what the scripts don't reach.

## 3. Walk the site yourself

The scripts walk routes a persona or a mode knows about. Things they miss:
the profile and course settings dialogs, the course editor, sharing, the
docs pages, the landing page, error pages, and anything behind an (i) or !
icon. Spend twenty to forty minutes here, phone width first.

```sh
./.dev-start --reset --seed --test-course --background    # prints the URL
```

Open `<url>/dev/login` (and `/dev/login?open=test-course`) in Chrome at
desktop width, and for phone width run a throwaway Playwright script from
`e2e/` with the iPhone 13 device (Chrome's `resize_window` is ignored). Look
for: text that runs off the screen, a button with no effect, two pages that
disagree about what is passed, English in the app's own text (it is all
translated; a hard-coded string is a bug), a status message sitting on the
page instead of behind an icon, a destructive button styled like a primary
one, a modal that can't be closed, a 404 from a link. Take a screenshot of
each (`$S/shots/`); it goes in the issue.

Stop the server with `./.dev-start --stop` when you are done.

## 4. Read the reports

- **Explore**: `e2e/results/explore/<profile>-<timestamp>/report.md`.
  `bug` findings (high) first, then `unclear` and `frustrating` with their
  p and screenshot. The "Proposed e2e tests" section is for step 6.
- **Playthroughs**: `$S/logs/playthrough-chain.log`, then
  `e2e/results/playthrough/<timestamp>-all/summary.md`
  and `-test-course/summary.md`, then each mode's `report.md`. A `bug` or
  `invariant` is the app's fault. A `stuck` phase or a failed lesson may be
  Jev's limit (it fills text boxes from the page's word pool and skips
  listening, so a subjunctive drill or a dictation often fails): read the
  wrong answers and the confidence before calling it a bug. A reproducible
  stuck is a bug (#490 was found this way).
- **Journeys**: the step logs at the end of `$S/logs/playwright-chain.log`;
  a failing journey names its step.
- **Content**: an ERROR comes with evidence and a fix; a CHECK is a doubt.
  A finding that is right after all goes in
  `data/courses/<id>/fact-check-ok.txt`, not an issue.

Read the logs with `grep` and `tail`, not whole: a playthrough chain log
runs to thousands of lines, and everything you read stays in this session
for the rest of the run. For a long report (over about 300 lines, or the
playthroughs' per-mode reports), give it to Qwen first:
`mcp__qwen__qwen_ask` with the report and the prompt "One line per
finding: severity | where | what, in plain words | evidence path. Leave out
passes. Nothing else." Skim the report for `bug` and `invariant` lines Qwen
left out. When Qwen is down, give it to an agent with `model: "haiku"`
that returns the same lines. Those agents don't count toward the run's
total. Step 5 reproduces every line anyway. Write the issue bodies
yourself, not with Qwen.

Write every candidate into `$S/findings.md` as one line: where, what, which
script, evidence path. Then dedupe: two personas hitting the same overflow
is one finding; the same table overflowing on 29 lessons is one finding
(name the worst five and the count).

## 5. Verify, then file

A Jev flag can be wrong, and an exact signal can be an artefact of the run
(a stale goal after a course switch gave a 404 once). Before filing:

1. Reproduce it yourself on the dev server, or with
   `scripts/e2e-explore.sh --replay <report.json>` (no Jev, no cost), or
   with `scripts/jev-playthrough.sh serve <checkpoint.db>` for a playthrough
   finding at lesson N.
2. `grep -i <a word of it> $S/known.tsv`. Filed already, open or recently
   closed: skip it, or add a comment to the issue if you have new evidence.
3. Decide the kind: a bug (something broken), a rough edge (works but
   confusing or ugly), a content fault (a lesson file), a security finding
   (a limit that isn't enforced, a check the browser can skip, a route
   that trusts its caller), or a testability gap (markup a test can't see).
   A testability gap is not an issue; it is a markup fix for step 6. A
   security finding is filed like a bug, one issue each, with the `security
   fix` label as well, and no exploit text beyond what reproduces it on the
   dev server.

File one issue per finding with `gh issue create`, following
`$SK/references/issue-template.md`: title `[qa] <what is wrong, in plain
words>`, label `needs grooming` (plus `bug` for something broken),
body of at most four short paragraphs: what is wrong, how to see it (start
from the dev-start line, then the URL and clicks), what you expected, and
the evidence (report path, which persona or mode found it), then the
screenshots.

**Screenshots.** Every bug gets two or three still screenshots in the issue
body that show the problem: the broken state, plus the step before it or
the same place at the other width when that makes it clearer. No videos.
Crop or pick the viewport so the problem is in view; a full page where the
fault is a speck at the bottom doesn't help. A rough edge gets one when a
picture says it faster than words. Take them on the dev server (or reuse
the explore report's shot when it shows the problem), save them as
`$S/shots/<slug>-1.png`, `-2.png`, then:

```sh
$SK/scripts/issue-shots.sh "$(date +%F)-<slug>" $S/shots/<slug>-*.png >> $S/issue-<slug>.md
```

It puts the images on the `qa-screenshots` branch and appends one
`![…](…)` line per image to the body, so the issue shows them inline. Never
commit `e2e/results` or `$S/shots` to a working branch.

Don't fix anything in this sweep. The user grooms the issues later and
decides. The `needs grooming` label keeps `backlog-sweep` off an issue until
the user takes it off, so a Jev false alarm never becomes a PR. The `[qa]`
title says where the issue came from and stays after grooming.

## 6. Leave tests behind

Qwen can draft the `test.fixme` outlines from a report's "Proposed e2e
tests" (`mcp__qwen__qwen_ask` with the report: "One Playwright
`test.fixme` per proposed test, titled with the issue number. Nothing
else."); the spec still has to compile, so run that one spec once.

The explore reports propose tests. For each verified bug, paste its
regression outline into `e2e/specs/exact.spec.ts` as a `test.fixme` with the
issue number in its title, so the fix has a test to turn on. A reached
goal that is an essential flow with no journey yet goes into
`e2e/journeys.ts`. Markup fixes from testability findings go in as they
are (an `aria-label`, a `data-state`, a `role="alert"`), following
"Markup hooks" in `docs/testing.md`. Put these on a branch
`alexbeeken/<issue>-qa-specs` with a PR titled `[<issue>] …` only when there
is something to commit; most sweeps file issues and leave no PR.

## 7. Report

End with a short report in plain words, short sentences:

- what ran, how long, roughly what it cost;
- a table with one row per issue filed: number, kind, where, found by;
- findings you did not file, and why (Jev's limit, already filed, false
  alarm), in one line each;
- questions you wrote for the desk's Decisions mode, if any;
- the counts the scripts printed (goals reached, lessons failed per mode,
  ERRORs and CHECKs), only where they changed what you did;
- tests or markup fixes left behind, with the PR if any;
- what to run next time and what to skip, if something was a waste.

Write anything that will still hold next time (a flaky persona goal, a
script quirk, a persona that always gets lost on one page) into
`$SK/references/harness-facts.md`, so the next run doesn't pay for it again.

## 8. Write the log line

Every run appends one line to `~/skrongle-workflow/log/qa.log`, also a run
that stopped part way (no Jev key, disk full):

```sh
mkdir -p ~/skrongle-workflow/log
printf '%s  %s\n' "$(date '+%Y-%m-%d %H:%M')" \
  'filed #1140 #1141 #1142 · 4 not filed · about $3' \
  >> ~/skrongle-workflow/log/qa.log
```

Parts joined by ` · `: `filed #A #B` (or `filed nothing`), `<n> not filed`,
`PR #N` for a specs PR, `asked <n>` when questions were written for the
desk, `stopped: <why, a few words>`, the rough cost, `qwen <n>` (Qwen calls, or `qwen down`).

End the report with:

```
Open the Review Desk: `review`
```
