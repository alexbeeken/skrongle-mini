---
name: pr-fix
description: Fix one Skrongle PR that the Review Desk sent back with "needs work". Switch the workspace to the PR's code, read alexbeeken's newest "## Needs work" comment on the PR (that comment is the task), fix it, push to the same PR, wait for CI, re-run pr-qa-demo for that PR so the QA report and ~/skrongle-demos/pr-<N>/ are fresh, then remove the `needs work` label and comment "## Fixed". When it can't finish, it leaves the label on and comments "## Fix stopped" with why. Use this for "/pr-fix N", "fix PR N", "do the needs work on PR N", "the needs work on PR N", or whenever a PR labelled `needs work` should be fixed, even if the user doesn't say "needs work".
---

# Fix a PR sent back with "needs work"

The Review Desk starts you in a fresh Orca workspace with
`orca worktree create --name fix-pr-<N> --pr <N> --agent claude --prompt "/pr-fix <N>"`.
`<N>` is the PR number. Nobody is watching: don't stop to ask questions.
When something needs the user's decision, stop the way "Stopping" says, so
the question ends up on the PR.

The task is not in your prompt. It is the newest comment on the PR by
alexbeeken that looks exactly like this (the desk posts it):

```markdown
## Needs work

<what the user wants fixed>

<!-- review-desk:needs-work -->
```

Your own comments start `## Fixed` or `## Fix stopped`, so they are never
read as a request.

Labels: you only ever remove `needs work`, and only when the fix is done.
Never add or remove `mergeme`.

`$SK` is `.claude/skills/pr-fix` in this checkout. `$QA` is
`.claude/skills/pr-qa-demo`. The repo is `alexbeeken/skrongle-mini`.

`.claude/skills/rotate/references/factory-rules.md` holds the rules every
factory session follows: the model for each agent, the per-run agent
total, keeping the session small, and commands that never stop for a
permission prompt (section 4). Read it before you start.

## In a cloud session

A cloud session (claude.ai/code, `claude --cloud`) has `CLAUDE_CODE_REMOTE=true`.
Check it first (`echo $CLAUDE_CODE_REMOTE`). CI runs on the Mac's runners
and the demos live on the Mac, so don't start: say pr-fix needs the Mac,
and stop. Post nothing and leave the label alone.

## 1. Read the request

```sh
$SK/scripts/request.sh <N>
```

It only reads GitHub. It prints the PR's state, head branch, base branch,
whether it conflicts, whether `needs work` is on it, and the newest request
with its text after a `---` line. `answered yes` means a `## Fixed` comment
came after that request. A `## Fix stopped` doesn't count, so running
`/pr-fix <N>` again after a stop tries again.

Stop here when:
- **`state` is `merged` or `closed`.** Post nothing and leave the label.
  Tell the user in the terminal.
- **`request` is `none`.** If the label is on, post a `## Fix stopped`
  comment: "No needs-work comment found on this PR, so there was nothing to
  fix." Otherwise just say so in the terminal.
- **`answered` is `yes`.** The newest request was already fixed. Say so in
  the terminal and post nothing. A new request needs a new comment from the
  desk.

A draft PR is fixed like any other.

Keep the request's URL. Every comment you post links to it.

## 2. Switch to the PR's code

Orca made this workspace on a new branch off main. Switch it to the PR:

```sh
git status --short                 # must be empty
git fetch origin
gh pr checkout <N> --detach        # detached: the branch may be checked out in another worktree
git log --oneline -1               # should be head_sha from request.sh
```

Never check out the PR's branch by name, and leave the branch Orca made
alone. You push by refspec in step 5.

## 3. Fix it

Read before you change anything:
- `CLAUDE.md`, and the `.claude/rules/*.md` file for each area you touch
  (they don't load by themselves in this worktree).
- The PR: its body (`gh api repos/alexbeeken/skrongle-mini/pulls/<N> --jq .body`),
  its newest `## QA: PR #<N>` comment, and the diff against its base:
  `git diff origin/<base>...HEAD`.

Then fix what the request asks, and only that. If it can be read two ways,
pick the reading that fits the PR's own intent, and say which one you
picked in the `## Fixed` comment. Stop instead (see "Stopping") when the
request needs a decision you can't make: two ways that each drop someone's
work, a provider key or secret you don't have, or a change far outside
what the PR is for.

Follow CLAUDE.md as for any change:
- Don't run `cargo test`, `scripts/smoke.sh` or `scripts/e2e.sh` before
  pushing; CI runs them. Locally, `cargo build` (if you changed Rust,
  templates or i18n) and the one test or spec you are debugging are enough.
- Disk: check `df -h /` before building. A worktree's build takes 3–6 GB,
  and a full disk takes the CI runners down. With under 10 GB free, stop.
- Keep the docs true. If the fix changes what a "How to QA it" box says or
  adds a step, update those boxes in the PR body, changing nothing else in
  it, because pr-qa-demo checks the boxes as written:
  `gh api repos/alexbeeken/skrongle-mini/pulls/<N> --jq .body > $S/body.md`, edit,
  then `gh api -X PATCH repos/alexbeeken/skrongle-mini/pulls/<N> -F body=@$S/body.md --silent`.
  (`$S` is your scratchpad.)

Commit with the PR title's first issue number:

```sh
git commit -m "[<issue>] <what you fixed>" -m "Co-Authored-By: <the line your system reminder gives>"
```

Never put `[skip ci]` in the message. It keeps CI from running, and step 6
waits for CI.

## 4. Conflicts with the base

A PR that conflicts with its base gets no CI run at all, so settle this
before you push. Check it locally (GitHub's `mergeable` is often `UNKNOWN`):

```sh
git fetch origin
git merge-tree --write-tree HEAD origin/<base> >/dev/null && echo clean || echo conflicts
```

`<base>` is `main` for most PRs. For a stacked PR it's the lower PR's
branch: merge that, not main.

If it conflicts, merge the base in with a real merge, never a rebase:
`git merge --no-ff origin/<base>`. Resolve hunk by hunk. Never
`git checkout --theirs` or `--ours` a whole file: that once dropped #243's
Undo/Redo row without anyone noticing. After each file,
`git diff HEAD -- <file>` should show only what you meant. The usual
conflict spots (migration numbers, the smoke.yml spec list, `i18n/*.yaml`,
generated files) and the fix for each are in `.claude/skills/mergeme/SKILL.md`,
step 3. Run `cargo build` after resolving, and the one test behind anything
you renumbered (`cargo test db` after migrations, `cargo test i18n` after
yaml).

If both sides built the same thing in different ways, that's a decision,
not a conflict: `git merge --abort` and stop.

## 5. Push

```sh
git fetch origin
git merge-base --is-ancestor origin/<head> HEAD || git merge --no-ff origin/<head>   # someone pushed meanwhile
git push origin HEAD:<head>                                                          # never --force
git rev-parse HEAD                                                                   # the commit CI runs on
```

`<head>` is the `head` line from request.sh. Push to the same PR. Never open
a new one.

## 6. Wait for CI

Run this as a background Bash command. It sleeps between looks, and you
are told when it ends:

```sh
$SK/scripts/wait-ci.sh <N> <commit you pushed>
```

What it prints last:
- **`pass`**: go to step 7.
- **`fail`**, then one line per failed check with its link. Read the
  failure (`gh run view <run-id> --log-failed | tail -80`; the run id is the
  number after `/runs/` in the link).
  - A known flaky spec (the list is in `.claude/skills/mergeme/SKILL.md`,
    step 5) failing on its own: rerun once with
    `gh run rerun <run-id> --failed` and wait again. Two failures on the
    same commit means it's real.
  - A real failure: rerun that one test or spec locally
    (`cargo test <name>`, or `scripts/e2e.sh specs/<file>.spec.ts -g "<name>"`),
    fix it, commit, push (step 5) and wait again.
  - A failure the PR didn't cause (main's latest CI run fails the same
    way): say so in the `## Fixed` comment and go on to step 7.
- **`none`**: no check started. Check that the commit message has no
  `[skip ci]` and the branch doesn't conflict (step 4), fix that, and push
  again. If checks still don't start, look at `scripts/ci-runners.sh status`
  and stop: the runners are down.
- **`timeout`**: CI ran for 90 minutes. The runners are stuck or
  overloaded. Stop.

Give up after three pushes that each came back red. Stop then, and say
what still fails.

## 7. Re-QA with pr-qa-demo

Run the pr-qa-demo skill for this one PR (the Skill tool, `pr-qa-demo`,
with the argument `#<N> only`). A named PR is QA'd even when `find-prs.sh`
shows it as `skip` because of the label, so you don't remove the label
first. It posts a fresh `## QA: PR #<N>` comment with a `**Checked at**`
line, and replaces `~/skrongle-demos/pr-<N>/`. Its QA agent may push a small
fix of its own; that's fine.

When it's done, check it worked:

```sh
$QA/scripts/find-prs.sh | awk -F'\t' -v n=<N> '$1 == n'   # needs is "skip" (label still on), checked at is the head
$QA/scripts/demo-folder.sh check ~/skrongle-demos/pr-<N>   # prints ok
git fetch origin && git rev-parse --short=8 origin/<head>   # matches the manifest's checked_at
```

Then read the verdict:
- **merge**, or **merge after X**: go to step 8.
- **not ready** because what the request asked for still isn't right: go
  back to step 3 once more. A second "not ready" on the same point: stop.
- **not ready** for another reason (a migration number taken by another
  PR, a bug that was already there): the fix is still done. Go to step 8,
  and name the reason in the `## Fixed` comment. The desk shows the PR on a
  red card.

If pr-qa-demo can't run (no Jev key, the disk is too full), stop: the fix
is pushed, but the PR has no fresh QA.

## 8. Finish

Remove the label first, then comment:

```sh
gh api -X DELETE "repos/alexbeeken/skrongle-mini/issues/<N>/labels/needs%20work" --silent || true
gh api repos/alexbeeken/skrongle-mini/issues/<N>/comments -F body=@$S/fixed.md --jq .html_url
```

`$S/fixed.md`, in plain words and short sentences:

```markdown
## Fixed

<what you changed, in one to four plain lines. Say how you read the request if it could be read two ways.>

- Asked in: <request URL>
- Commits: `<8-character commit>`, …
- CI: green <or: red on X, which fails on main too>
- QA: <merge / merge after X / not ready: why>, checked at `<8-character commit>` (<QA comment URL>)
```

Stop any dev server you started (`./.dev-start --stop`). Leave the Orca
workspace; the user closes it.

## Stopping

When you can't finish, leave the `needs work` label on and post:

```markdown
## Fix stopped

<why, in one or two plain lines>

- Asked in: <request URL>
- Done so far: <commits pushed, or "nothing pushed">
- What would unblock it: <the decision, key or fix it needs>
```

Post it with the same `gh api …/comments` call as step 8. Don't stop
quietly: the desk counts the PR as "being fixed" for as long as the label
is on, and this comment is how the user finds out. The exceptions are in
step 1 (a merged or closed PR, a request that was already answered) and
the cloud session, where you only tell the terminal.

When what stopped you is a choice the user has to make (two ways that each
drop someone's work, a change far outside what the PR is for), also put it
on the Review Desk's Decisions mode, so it is answered there and not only
read on the PR:

```sh
$BASE/scripts/review-desk/ask.py --pr <N> --slug design-choice --asked-by pr-fix \
  --title "<PR title>" --question "<the choice, one plain sentence>" \
  --option "<A>" --option "<B>" --recommended "<A>" --context "<what each leads to; the request URL>"
```

The rules are in `.claude/skills/backlog-sweep/references/decisions.md`.
Name the question id in the `## Fix stopped` comment. A missing key or
secret is not a choice; the comment alone is enough.

## Report

End in the terminal with a few plain lines: fixed or stopped, and why;
the commits pushed; CI; the QA verdict and checked-at commit; the links to
your comment and the QA comment. The demo is in `~/skrongle-demos/pr-<N>/`,
and the Review Desk shows it.
