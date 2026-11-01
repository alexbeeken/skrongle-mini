# Factory rules: models, agent limits, context, safe commands

Every factory skill (mergeme, backlog-sweep, pr-qa-demo, qa, pr-fix) and
every agent they start follows these rules. They exist because of October
2026's usage (#1314): sessions that started 10 or more agents used 56% of
the month, 92% of the agents ran on Opus, coordinators sat at 150–260k
context for thousands of calls, and about 40 cleanup commands waited hours
for a permission prompt nobody was there to answer.

## 1. The model for each job

There is one coordinator for the whole fleet: the **rotator**, the session
running `/rotate`, on **Fable 5.1**. It is the only Fable in the factory.
It starts the four sessions, picks each one's model, checks in on them and
unblocks them (`rotate/SKILL.md`, "Supervise"). Nothing else runs on
Fable: no session, no agent. Never start a `fable` agent.

Below the rotator, Opus is for writing code and little else. Everything
else runs on Sonnet or Haiku, and mechanical text work goes to the local
Qwen model first.

| Job | Model |
|---|---|
| The rotator (`/rotate`) | Fable 5.1, the only one |
| The four sessions (mergeme, backlog, pr-qa, qa) | `sonnet`, unless the rotator picks otherwise for a run (`MODEL_*` in `start.sh`) |
| A builder that writes code for an issue (backlog-sweep), a pr-fix session | `opus` |
| A design issue's artifact (backlog-sweep): mockups in the app's look | `opus`; `sonnet` for a research report or RFC that is text and examples |
| A QA agent checking a PR's "How to QA it" boxes (pr-qa-demo) | `sonnet` |
| Resolving merge conflicts, renumbering migrations (mergeme) | an `opus` agent; never Qwen |
| Finding the failure in a CI or Playwright log | Qwen, and a grep confirms the line it names; `grep`/`tail` when Qwen is down |
| Boilerplate a build or test checks (fixtures, `test.fixme` outlines, data rows) | Qwen drafts, the build or test checks it |
| A first look at a diff (places to check, not a review) | Qwen |
| Reading a long report (over about 300 lines) into one line per finding | Qwen, then skim for `bug` lines; a `haiku` agent when Qwen is down |
| The unslop pass over a body | whoever wrote it, with a grep for the characters; never Qwen |
| An issue or PR body | whoever files it; never Qwen |
| Watching CI, polling a check | a background Bash command, or a `haiku` agent |
| Checking whether an issue is already fixed, looking for duplicates, closing old issues | `haiku` |
| Translating i18n chunks | `sonnet` |

`opus` means the newest Opus (5 to 5.5 are all fine). Move one agent up a
level when its job needs it, and say why on the state table. A QA agent
goes up to `opus` when the PR changes migrations, auth, billing or sign-in,
or more than about 300 lines of Rust. `/rotate` starts each session on its
own model (`rotate/scripts/start.sh`), so a `/model` switch in another
session can't change it.

### Qwen first for the chores

Qwen3-Coder 30B runs on this Mac and costs nothing, but it is a smaller
model: it misses things and flags things that are fine. So use it only
where something else checks its answer, or where a miss costs nothing:

- **Finding the failure in a log.** "Which test failed, and the first
  error line, as `test | line`. Nothing else." Confirm it with a `grep` for
  that line, then read only around it. Logs run to thousands of lines, so
  this saves the most Claude reading, and a wrong answer shows at once.
- **Boilerplate the build or a test checks.** Test fixtures, `test.fixme`
  outlines from a proposed test, data rows. A wrong one fails the build.
- **A first look at a diff**, as places to check: the author reads the
  diff anyway, so a miss costs nothing.
- **A long report** (over about 300 lines) into one line per finding, and
  then skim it for `bug` lines it left out: a missed bug never becomes an
  issue, and nobody notices.

Never Qwen for:

- **Merge conflicts.** Not a first look, not a proposal, not a
  renumbering. On 2026-10-31 it failed a plain two-migration conflict
  twice: it reused an existing version, then dropped one PR's migration.
  A wrong resolution can pass CI and lose a PR's change, so conflicts go
  to an `opus` agent.
- **Unslop and text a person reads.** In a test the same day it flagged 3 of 4
  slop lines in a sloppy PR body and missed the curly quotes; on a clean
  body it flagged 7 lines, none of them real, and one rewrite (`Closes #N`
  to `Closes issue #N`) would have broken GitHub's issue link. Checking
  its flags cost more than reading the body. Issue and PR body drafts save
  little for the same reason: every claim still has to be checked.
- Rankings, QA verdicts, whether a finding is real, anything that has to
  be complete.

How to use it:

- **How**: call `mcp__qwen__qwen_ask` with `files` (exact paths, about
  160 KB in total; Qwen reads them itself) and a `prompt` that says the
  output shape and ends "Nothing else.". Qwen sees nothing but what you send.
- **Check it**: grep for the line it names, run the build or the test,
  open the evidence for a finding before you use it.
- **One server for everyone**: it answers one request at a time (`-np 1`),
  and four sessions and their agents share it. Give it the few jobs above,
  one file per call, not a stream of small asks. Never call
  `mcp__qwen__qwen_stop`; another session may be using it. `/rotate`
  starts it, the user stops it.
- **When it isn't there** (an `ERROR`, a timeout, the tool missing in a
  cloud session), use the fallback in the table and go on. Retry at most
  once. The first call after idle takes about 20 seconds.
- **Count it**: each skill's log line gets a `qwen <n>` part (calls made,
  or `qwen down`), so the user can see whether it pays.
- **Memory**: the Mac has 48 GB and Qwen holds about 20 GB. If builds get
  killed (`signal: 9`), say so in the report; `QWEN=off start.sh` runs a
  rotation without it.

## 2. A small coordinator

Each turn of the coordinator re-reads its whole conversation, so what it
keeps in context is what every later call pays for.

- **Agents reply in at most 5 lines**: the PR or issue number, the outcome
  (PR opened, verdict, asked <id>, blocked: why), CI state, and one line of
  facts the next agents need. Everything else goes where it belongs: the PR
  body, the QA comment, the demo folder, or a file in the scratchpad.
- **Never read an agent's transcript or a full log.** Read the state file
  and the 5-line replies. When you need a detail, `grep` or `tail` for it.
- **The state file is your memory** (`sweep-state.md`, `qa-state.md`,
  `mergeme-state.md`). Update it on every reply, so nothing is lost when the
  session is compacted.
- **Compact after each wave** of agents: run `/compact`, keeping the state
  file's path and the run's seed and limits.
- **Hand off rather than grow.** If your context still passes about 150k
  tokens, write `handoff:` and what's left at the top of the state file,
  write the log line with `stopped: handed off`, and end. The next run
  starts from the state file.

## 3. A total per run, not only at once

`rotate/scripts/budget.sh builders` and `budget.sh qa` say how many agents
may run **at the same time** (from the free disk). `budget.sh total` says
how many a run may start **in all** (10 by default, `FACTORY_MAX_AGENTS` to
change it). Stop starting agents when you reach it. Mark the rest
`waiting: next run` on the state table and in the report. The next
rotation picks them up. Small `haiku` jobs (watching CI, checking whether an
issue is fixed) don't count toward the total.

## 4. Commands that never wait for a person

Claude Code has a built-in safety check on removals that no permission rule
or bypass mode turns off. It holds a command for a person's approval when it
can't tell for sure what a removal will delete:

- a **shell variable** in the target: `rm -rf "$WT"`, `rm -f $S/*.webm`;
- a **relative path or a glob after a `cd`** in the same command:
  `cd $WT; rm -rf e2e/results/artifacts/*`, `rm -f ../pr1275/*`;
- a home, system or workspace directory.

Nobody watches a factory session, so a held command waits until someone
comes back: up to 15 hours in October. Write commands that the check has
nothing to ask about:

- **Don't delete scratch files.** The scratchpad is temporary. Leave old
  files, or write the next run's output to a new name (`qa-pr1273-2.jsonl`).
- **Send test output to a new folder** instead of emptying an old one:
  Playwright `--output <scratchpad>/results-<pr>-<n>`, not
  `rm -rf e2e/results/artifacts/*`.
- **Remove a worktree with the script**, never with `rm`:

  ```sh
  <BASE>/.claude/skills/rotate/scripts/drop-worktree.sh <absolute worktree path> [--branch qa/pr<N>]
  ```

  It refuses an empty path or one outside a scratchpad, stops the
  worktree's dev server and binaries, removes the worktree, prunes, and
  deletes the local branch you name. Write the path out in full; don't pass
  it through a variable.
- **When a removal is really needed**, use one literal absolute path: no
  variable, no glob, no `..`, and no `cd` in the same command.
- **Push without `--force`.** When a branch must be rewritten, use
  `git push --force-with-lease=<branch>:<sha> origin HEAD:refs/heads/<branch>`
  with the sha you expect.
- **No `git reset --hard` or `git clean` in a shared checkout.** Start a
  fresh detached worktree instead.
- **A command that is refused, or whose prompt times out, is skipped.** Note
  it on the state table (what and why) and go on. Never wait for it, and
  never get around it another way.
