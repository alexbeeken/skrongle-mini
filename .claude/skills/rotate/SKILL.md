---
name: rotate
description: Start one turn of the Skrongle software factory - four Orca workspaces working at once, each a Claude session running one skill. /mergeme lands the PRs marked mergeme, /backlog-sweep builds PRs for the workable issues and puts the questions it can't answer on the Review Desk, /pr-qa-demo QAs every open PR that needs it, and /qa explores the site with Jev and files "[qa]" issues. Use this for "/rotate", "run the rotation", "start the factory", "kick off the twice-a-day run", or whenever the user wants all four started together. The rotate session is the rotator: the one Fable 5.1 coordinator of the fleet, which picks the sessions' models, checks in on them and unblocks them until they finish.
model: fable
---

# One rotation: four skills at once

The user runs this by hand twice a day. Between rotations they work on the
Review Desk: review PRs (mergeme, needs work), groom the `[qa]` issues
(Issues mode) and answer the factory's questions (Decisions mode). A
rotation does the rest:

| Workspace | Skill | Does |
|---|---|---|
| `mergeme-<date>` | `/mergeme` | lands every open PR labelled `mergeme` (or with a "mergeme" comment) as one combined PR |
| `backlog-<date>` | `/backlog-sweep` | ranks the workable issues, opens a PR for as many as it can, and asks about the stuck ones |
| `pr-qa-<date>` | `/pr-qa-demo` | QA report and before/after screenshots for every open PR that lacks them or changed |
| `qa-<date>` | `/qa` | Jev explores the site; one `[qa]` issue per real bug, with screenshots |

You are the **rotator**: the one coordinator for the whole fleet, and the
only Fable 5.1 in the factory (`references/factory-rules.md`, section 1).
You pick each session's model, start the four sessions, then check in on
them until they finish and unblock any that are stuck (section 3). You
don't do their work: you don't edit code, merge, QA or file issues. Each
workspace is its own Orca session with its own worktree and starts its own
agents. If `/model` shows you aren't on Fable 5.1, say so in your first
line and carry on.

The prompt each session gets is only `/<skill>`. The four skills are written
for this shape, so there is nothing to tell them: no stages to skip, no
limits to pass, no "nobody is watching". Running one by hand gives the same
behaviour. What makes them fit together is their own defaults:

- **Nobody but the rotator is watching, so nobody asks.** The rotator
  unblocks a session but never answers for the user. A question only the user can
  answer goes to the Review Desk's Decisions mode as one file in
  `~/skrongle-workflow/decisions/`, written with `scripts/review-desk/ask.py`
  (the rules are in `../backlog-sweep/references/decisions.md`). The skill
  goes on with everything else. A PR that turns out not ready goes back to
  the desk's PR queue; an issue that isn't clear goes to its Issues mode
  (`needs grooming`).
- **backlog-sweep builds, pr-qa-demo QAs.** The sweep never QAs its own
  PRs. pr-qa-demo takes the PRs that are open when it starts, so a PR built
  in this rotation gets its QA in the next one.
- **pr-qa-demo leaves approved PRs alone.** A PR labelled `mergeme` is only
  QA'd again when it changed since the QA the user approved.
- **`[qa]` issues wait for grooming.** `/qa` labels them `needs grooming`,
  which backlog-sweep skips. The user takes the label off on the desk's
  Issues mode to make one workable.
- **One mergeme at a time.** If a combined `alexbeeken/mergeme-*` PR is still
  open, the script doesn't start another one: two would race each other into
  main.
- **Disk is shared.** Every skill reads its agent limit from
  `scripts/budget.sh` in this folder, which assumes all four sessions are
  running, so no session takes the whole disk.
- **One rule book for usage and prompts.** `references/factory-rules.md`
  says which model each kind of agent runs on (you on Fable, the sessions
  on Sonnet, Opus only for code, Qwen first for chores), how many agents one run may
  start in all (`budget.sh total`, 10), how a coordinator stays small
  (5-line replies, `/compact` after each wave, hand off past ~150k), and
  how to write commands that never stop for a permission prompt
  (`scripts/drop-worktree.sh` for worktrees, no `rm` on a variable or a
  glob). Every factory skill and brief points at it. It came from
  October's usage (#1314): runs that started 30–45 Opus agents were most
  of the month, and cleanup commands waited up to 15 hours for an approval
  nobody was there to give.
- **Main moves under them.** mergeme landing a batch puts open PRs behind
  main. That's expected; the builders branch off `origin/main` and the next
  mergeme merges main in.

## 1. Start it

```sh
$BASE/.claude/skills/rotate/scripts/start.sh
```

`$BASE` is this checkout. The script checks, then launches:

- not a cloud session (the runners, Orca and `~/skrongle-demos` are on the
  Mac);
- at least 50 GB free (`scripts/budget.sh` prints the builder and QA agent
  limits the sessions will use);
- the Jev key is set (pr-qa-demo and qa need it);
- Orca answers (`orca open` if not);
- no combined mergeme PR is open.

Then it checks each part has work, and skips the ones that don't (each
skip is a note on the log line):

| Part | Skipped when |
|---|---|
| mergeme | no open PR is labelled `mergeme` or has a "mergeme" comment |
| backlog | fewer than 2 issues are `candidate` in `issues.py candidates` |
| pr-qa | no open non-draft PR shows `both`, `qa`, `demo` or `stale` in `find-prs.sh` (this check takes about 3 minutes) |
| qa | the last `qa.log` line is under 20 hours old (`QA_EVERY=<hours>`) |

`FORCE=1` starts every part anyway. Then it runs one `orca worktree create
--base-branch main --agent claude --model <model> --prompt "/<skill>"` per
workspace. Every session defaults to `sonnet`: the sessions coordinate,
and their `opus` agents write the code. You pick the models: raise one
session with `MODEL_MERGEME`, `MODEL_BACKLOG`, `MODEL_PR_QA` or `MODEL_QA`
only for a reason you can name (a mergeme batch of more than 8 PRs or with
stacks, say), and put the reason in your report. Never `fable`.
Passing the model means a `/model` switch elsewhere can't move the factory
onto another model. It also starts the local Qwen server if it isn't up
(`QWEN=off` skips it), so the sessions' first Qwen call doesn't wait for
the model to load; the log line says `qwen on` or `qwen off`. Then it
appends one line to
`~/skrongle-workflow/log/rotate.log`. Orca adds `-2` to a name already
taken, so the afternoon run is `qa-2026-10-28-2`.

`ONLY="qa mergeme"` starts some of them (`mergeme backlog pr-qa qa`).
`DRY=1` prints the commands and launches nothing. Use it when the user asks
what a rotation would do. `BASE_BRANCH=<branch>` starts the sessions from a
branch instead of main, to try changes to these skills before they merge;
the builders and mergeme still work off `origin/main`.

When a check fails, the script writes `stopped: <why>` to the log and starts
nothing. A skipped part isn't a failed check: the others start. Tell the user why in one line, and what to do (free some disk,
`orca open`, set the key). Don't work around a check.

"Orca could not confirm the launch outcome" has meant the agent started
anyway (the desk's log, 2026-10-28). Check with
`orca worktree list --repo path:<main checkout> --json` before starting it
again.

## 2. Say what started

Two or three lines, plain words: which workspaces started on which model,
any that were skipped and why, the builder and QA limits, and whether Qwen
is on. Then supervise.

## 3. Supervise until they finish

Keep `$S/rotate-state.md`: one row per started workspace with its model,
its terminal handle, its log file, the last check, its state (working /
nudged / done / stuck) and what you sent it. It is your memory if you are
compacted. Find each handle with
`orca terminal list --worktree name:<workspace> --json`; the sessions also
show in `ListAgents` by workspace name.

Check in every 20 minutes: start `sleep 1200` as a background Bash
command; it wakes you when it ends. On each check, for each workspace not
yet done:

1. **Done?** Its log (`~/skrongle-workflow/log/mergeme.log`,
   `backlog.log`, `pr-qa.log`, `qa.log`) has a line written after the
   rotation started. Mark it done; don't read further.
2. **Look**: `orca terminal read --terminal <handle> --screen --limit 40`.
   The screen only, never the history: your context is what every later
   check pays for.
3. **Stuck?** Any of these:
   - a permission prompt or dialog is waiting;
   - it asked a question and is waiting for an answer;
   - it says it hit a rate or spend limit, or Qwen or the runners are down;
   - the same error twice in a row, or the same screen as your last check
     with nothing running;
   - it's idle at the prompt with no log line written.
4. **Unblock** with one short message (`SendMessage` to its name, or
   `orca terminal send --terminal <handle> --text "<message>" --enter`),
   by the rules it already has:
   - a held command: decline the prompt, never approve it, and tell it to
     skip that command and go on (`factory-rules.md`, section 4);
   - a question only the user can answer: "Write it to the desk with
     `scripts/review-desk/ask.py` and go on." You never answer for the user;
   - a choice the rules settle (which model an agent gets, whether to rerun
     a known flaky spec once, a limit from `budget.sh`): answer it;
   - a coding job its agent keeps failing at on `sonnet`: "Start an `opus`
     agent for it." Never a `fable` one;
   - a limit or an outage it can't wait out: "Write your log line with
     `stopped: <why>` and end";
   - idle without a log line: "Finish: write your log line and report."
   Write what you sent in the state table. A session you've nudged twice
   for the same thing is `stuck`: leave it, and name it in the report.

Don't open its agents' transcripts, don't do its steps for it, and don't
close any workspace. Stop supervising when every workspace is done or
stuck, or after 10 hours. Then append one line to
`~/skrongle-workflow/log/rotate.log`:
`supervised: done mergeme backlog pr-qa · stuck qa (rate limit) · 3 nudges`.

## 4. Report

A few lines, plain words:

- which workspaces finished, which got stuck and why, and what you sent
  each one;
- each workspace's own log line, as it is;
- the models you chose and why, if any wasn't `sonnet`;
- the `qwen <n>` parts from the log lines, added up;
- the PRs show up on the Review Desk, and the questions in its Decisions
  mode.

End with:

```
Open the Review Desk: `review`
```

(or "run `scripts/review-desk/review`" if `zsh -ic 'command -v review'`
prints nothing).

Workspaces from earlier rotations stay in Orca until the user closes them.
Don't close them yourself, this rotation's or older ones.
