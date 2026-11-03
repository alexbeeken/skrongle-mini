---
name: pr-qa-demo
description: QA every open Skrongle PR that has no QA report or demo yet, or that changed since its last QA (an approved one, labelled mergeme, only when it changed). One agent per PR checks its "How to QA it" boxes with Jev and exact Playwright checks, posts a "## QA: PR #N" comment with a verdict and the commit it checked, and saves before/after screenshots (and a captioned video with music, when asked) with a manifest.json in ~/skrongle-demos/pr-<N>/, the folder the Review Desk reads. A choice only the user can make goes to the Review Desk's Decisions mode. Use this whenever the user asks to QA PRs, "QA and demo the open PRs", "record demos", "run Jev on the PR checklists", "check which PRs are ready", or wants a QA sweep or merge verdicts, even for a single PR or if they don't say "demo".
---

# QA + demo for the open PRs

You coordinate. One subagent per PR does the work, following
`references/agent-brief.md`. Your job is to pick the PRs, keep enough of
them running without filling the disk, pass on what agents learn, and report
at the end.

Nobody watches this session. `/rotate` starts it twice a day with no
instructions, and running it by hand works the same way. Never wait for a
go-ahead. A choice only the user can make goes to the Review Desk's
Decisions mode (the agent brief says how); a PR that isn't ready gets that
verdict and shows red on the desk. Neither stops the run.

Every PR's demo lives in one folder on this Mac, `~/skrongle-demos/pr-<N>/`:
a `manifest.json`, one before/after screenshot pair per thing the change
shows, and `demo.mp4` when there is a video. `references/manifest.md` is the
format. The Review Desk, pr-fix, backlog-sweep and mergeme all read it, so
don't change it in one place only. A new QA run replaces the folder's
contents. Nothing in it goes to GitHub; the PR gets only the folder's path.

The demo is before/after screenshots unless the user asks for videos (the
2026-10-28 run: a page of pairs is quicker to review than a video). A video
gets a random song from a small library (`scripts/songs/`: "cringe" from
PR #656, "gato" from PR #832, and
"last-round-turbo", a sad boxing-game orchestral medley at double speed
with a key change every third beat, and "silent-night-drift", Silent Night
at double speed in four parts, the upper two in C and the lower two in F#,
whose note lengths differ so the voices drift apart, and
"anxious-heart-reharm", the Final Fantasy VII slums theme (tune and form
taken from fan MIDI files) at twice the speed over a bluegrass boom-chick,
with new chords under every bar the way Jacob Collier would do it:
clusters, lydian upper structures, chromatic-mediant majors, a harp through
the new voicings, and a jazz modulation every four bars, up a major third,
up a major third, up a minor third, through all twelve keys, each reached
by a dominant on the last beat).

Labels: never add or remove `mergeme` or `needs work`, and tell no agent
to. The user and the Review Desk set them; pr-fix removes `needs work`.

`$BASE` is this checkout (the repo root of the session's working directory).
`$SK` is `$BASE/.claude/skills/pr-qa-demo`.

Read `$BASE/.claude/skills/rotate/references/factory-rules.md` before you
start: the model for each agent, the per-run agent total, keeping this
session small, and commands that never stop for a permission prompt.

## In a cloud session

A cloud session (claude.ai/code, `claude --cloud`) has `CLAUDE_CODE_REMOTE=true`.
Check it before you start (`echo $CLAUDE_CODE_REMOTE`). The demos have to
end up in `~/skrongle-demos` on the Mac, and CI runs on the Mac's runners,
so a cloud session records no demos and waits for no CI run:

- If the user asked for demos only ("record demos"), don't start. Say it
  needs the Mac and stop.
- Otherwise do the QA report only. Add `In a cloud session: QA report only,
  no demo, don't wait for CI.` to every agent's prompt. No demo folder is
  made, and the PR body's `## Demo` section is left alone, so `find-prs.sh`
  still shows the PR needs a demo for the next run on the Mac. Say so in the
  report.
- The VM is Linux: clone `target` with `cp -R --reflink=auto`, not `cp -Rc`.

## 1. Find the PRs

```sh
$SK/scripts/find-prs.sh
```

It only reads GitHub, and prints one line per open PR. The columns are:
number, head, base, mergeable, checks, has QA, has demo, needs, draft,
checked at, title.

- A QA report is a comment starting `## QA: PR #<n>`. The latest one counts.
  Its `**Checked at**` line names the commit it checked (the `checked at`
  column, `-` for an older report without one).
- A demo is `~/skrongle-demos/pr-<n>/manifest.json`. From before that
  folder, a `## Demo` section in the body or a `<!-- qa-page -->` link line
  counts too.
- `needs` says what to do:

| needs | Meaning | Do |
| --- | --- | --- |
| `both` | no QA report, no demo | QA + demo |
| `qa` | no QA report | QA report |
| `demo` | no demo | demo |
| `stale` | the PR has its own new work since the commit the latest report checked (a main merge alone doesn't count; see `references/manifest.md`, "Stale") | QA + demo again |
| `skip` | labelled `needs work`: its fix agent (pr-fix) runs QA itself when it's done | nothing |
| `approved` | labelled `mergeme` and not stale: the user approved it on the desk and a mergeme run lands it | nothing |
| `-` | QA'd at the current head, demo saved | nothing |

Take every PR whose `needs` is `both`, `qa`, `demo` or `stale`. A run on
PRs that haven't changed does nothing, and an approved PR is QA'd again only
when it changed since the QA the user watched (then it shows `stale`, not
`approved`). If the user named PRs (pr-fix names one), take only those, and
redo them even if they show `-`, `skip` or `approved`. Leave out drafts,
unless the user names one.

Don't filter on checks or mergeable. After a `[skip ci]` main-merge sweep,
most PRs show `checks none`, and GitHub reports `mergeable UNKNOWN` until
something asks. A PR that is red on its own code or conflicts with main
still gets QA'd: the agent says so in the verdict. Note these in your table
so the final report can name them. A main-merge sweep doesn't make a PR
`stale`: only the PR's own commits do.

Tell the user the list in a few lines (PR, what it lacks, anything odd)
and start. No need to wait for a go-ahead.

## 2. Get ready

- **Warm cache**: every agent clones `$BASE/target` (an APFS clone, so it's
  instant). Run `cargo build` in `$BASE` first, so the clones start warm.
  `$BASE/e2e/node_modules` must exist (`npm --prefix e2e ci` if not).
- **Jev key**: `zsh -ic 'echo ${#TYPESAFE_API_KEY}'` should print a number
  above 0. If it doesn't, stop and tell the user. Without it, the Jev half
  of every report quietly disappears.
- **Disk**: the agent limit is
  `$BASE/.claude/skills/rotate/scripts/budget.sh qa`. It assumes the other
  three factory sessions are running beside you and sharing the disk, which
  is the normal case. `0` means under 16 GB free: a full disk takes the CI
  runners down, so write the log line (step 6) with `stopped: <free> GB
  free` and stop.
- **Total**: start no more than `budget.sh total` QA agents in this run
  (10 unless `FACTORY_MAX_AGENTS` says otherwise). Take the PRs oldest
  first; the rest are `waiting: next run`, and the next rotation's
  `find-prs.sh` still shows them.
- **State table**: keep `$S/qa-state.md` in your scratchpad, with one row
  per PR: needs, status (waiting / running / done / blocked / waiting: next
  run), verdict, checked at, issues filed. It is your memory when the
  session is compacted.

## 3. Run the agents

Start each agent with the Agent tool in the background, with
`model: "sonnet"`. Use `model: "opus"` instead when the PR changes
migrations, auth, billing or sign-in, or more than about 300 lines of Rust
(`gh api repos/{owner}/{repo}/pulls/<N> --jq '.additions'` and the file
list), and note why on the state table. Give it a short prompt, because the
brief holds the method:

```
Read <SK>/references/agent-brief.md and follow it exactly.
PR: #<N> "<title>", head <head>, base <base>.
It lacks: <the QA report and the demo | only the QA report | only the demo |
  both again: it has new commits since the last QA (checked at <sha>)>.
Demo: <before/after screenshots | before/after screenshots and a side-by-side video, as in <SK>/references/side-by-side-video.md>.
$BASE is <BASE>. Your scratchpad is your own; use it as $S.
Notes from agents earlier in this run: <new harness facts, or "none">
```

Fill the number, title, head and base from `find-prs.sh`'s output, not by
hand: a hand-typed number once gave an agent the wrong title.

An agent replies in at most 5 lines. Don't open its transcript. When an
agent reports back, update the table, start the next PR, and read its last
line (what the next agents should know). A new harness fact (a
quirk, a flaky spec, a shared-file clash) goes into the next agents'
"Notes" line right away. At the end, add the facts that will still hold
next time to the brief's "Harness facts" section, so the next run doesn't
pay for them again.

After each wave (the limit's worth of agents have come back), run
`/compact`. If your context still passes about 150k, hand off as the rules
file says.

If an agent comes back without finishing (no comment posted, no video),
check what's missing (`find-prs.sh`, `ls ~/skrongle-demos/pr-<N>`). Send a message
to that agent to finish the job, rather than starting a new one: it still
has its context. Make sure its worktree was removed (`git worktree list`).

## 4. Check the work

For each PR the agent says is done:
- `find-prs.sh` shows `needs -` for it.
- `$SK/scripts/demo-folder.sh check ~/skrongle-demos/pr-<N>` prints `ok`,
  and the manifest's `checked_at` is the `checked at` column.
- Look at one before/after pair with the Read tool: both show the same
  place, and the difference is the change.
- A video, if there is one, plays:
  `ffprobe -v error -show_entries format=duration -of csv=p=0 ~/skrongle-demos/pr-<N>/demo.mp4`.
- Nothing stray was committed: `git log origin/main..origin/<head> --stat`
  shows no `.mp4`, `.png`, `manifest.json`, `_qa-pr`, `_demo-pr`, `_shots-pr`
  or `e2e/results` files.

## 5. A QA page on claude.ai (only when the user asks)

The folders are the demo; the Review Desk shows them. If the user also wants
one page for the run, as on 2026-10-28:
- Build one Artifact titled with the date ("PR QA 2026-10-28") from the
  manifests: one section per PR with `id="pr-<N>"` (highlighted when the
  URL's hash matches), its `why`, the verdict, and the pairs with captions.
  Publish the images through the Artifact `files` map.
- Just before publishing, refresh each PR's CI and merged state (the latest
  run per check only).
- Write `qa_page_url` (`<url>#pr-<N>`) into each manifest.
- At the top of each PR body, add or replace one line marked
  `<!-- qa-page -->`, linking to that section. Change nothing else in the
  body.

## 6. Write the log line, then report

Every run that got past the cloud check appends one line to
`~/skrongle-workflow/log/pr-qa.log`, also a run that stopped early:

```sh
mkdir -p ~/skrongle-workflow/log
printf '%s  %s\n' "$(date '+%Y-%m-%d %H:%M')" \
  'QA #1156 #1160 #1179 · not ready #1164 · asked 1 · about $0.40' \
  >> ~/skrongle-workflow/log/pr-qa.log
```

Parts joined by ` · `: `QA #A #B` (every PR a report was posted for, or
`nothing to QA`), `not ready #N` for each one, `asked <n>` when questions
were written, `<n> waiting` when the total held PRs back, `stopped: <why>` for a run that ended early, the rough Jev
cost, `qwen <n>` (Qwen calls made by the agents, or `qwen down`).

Then end with a short report in plain words, short sentences:
- a table with one row per PR: verdict (merge / merge after X / not
  ready), checklist tally, issues filed, checked at;
- the PRs that are not ready, and why, first;
- the questions written for the desk's Decisions mode (PR, one line each);
- shared problems across PRs (for example two PRs taking the same
  migration number, or a flaky spec several agents hit);
- what you added to the brief;
- to add a song to the library, the user drops a script that takes
  `<out.wav> <seconds>` into `scripts/songs/` (say so if they ask);
- PRs you left out, and why (`skip`, `approved`, draft, already `-`,
  `waiting: next run` because of the total).

The demos are in `~/skrongle-demos/pr-<N>/`, and the Review Desk shows them.
Say so once rather than repeating the folder in every row. End with:

```
Open the Review Desk: `review`
```
