---
name: mergeme
description: Land every open Skrongle PR marked `mergeme` (the label, or a comment from alexbeeken that just says "mergeme") as one combined PR off main, so CI runs once instead of once per PR. A PR that can't go in waits, goes back to the Review Desk's queue, or is asked about in its Decisions mode; the run never stops to ask. Use this whenever the user says "mergeme", "merge the mergeme PRs", "land the labelled PRs", "merge everything that's ready", "batch these PRs", or otherwise asks to get a set of PRs into main together, even if they don't name the label. Covers picking the order (stacks bottom-up), dry-running the merges, resolving conflicts (migration renumbering, smoke.yml spec list, i18n), opening the combined PR with every issue number, watching CI, merging, and checking every original PR shows as merged.
---

# Land the `mergeme` batch as one PR

The main ruleset is strict: every merge into main puts the other open PRs
BEHIND, and each re-merge costs a full CI run on the laptop runners. So the
batch goes into one branch off main, gets one CI run, and merges once.
GitHub then marks each original PR MERGED by itself, because its head commit
reached main inside the combined merge. That's how #825 landed twelve PRs on
2026-10-23.

Nobody watches this session. `/rotate` starts it twice a day with no
instructions, and running it by hand works the same way. So you never stop
to ask. A PR that can't go in this batch is left out and the batch lands
without it; what happens to it is one of three things, each written down
below: it **waits** (the next run picks it up by itself), it **goes back to
the desk** (its `mergeme` label comes off, with a comment saying why, so the
user sees it again in the Review Desk's PR queue), or it is **asked about**
on the desk's Decisions mode (`scripts/review-desk/ask.py`; the rules are
in `../backlog-sweep/references/decisions.md`). However the run ends,
landed or stopped, it ends with one line in the run log (step 7).

Taking `mergeme` off a PR is the one label change this skill makes, and only
in the "back to the desk" cases. Nothing here ever adds `mergeme` or touches
`needs work`.

`.claude/skills/rotate/references/factory-rules.md` holds the rules every
factory session follows: the model for each agent, the per-run agent
total, keeping the session small, and commands that never stop for a
permission prompt (section 4). Read it before you start.

## In a cloud session

A cloud session (claude.ai/code, `claude --cloud`) has `CLAUDE_CODE_REMOTE=true`.
Check it before you start (`echo $CLAUDE_CODE_REMOTE`). Landing the batch
means waiting for the combined PR's CI run on the laptop runners, so in a
cloud session don't start: say that mergeme needs to run on the Mac, and
stop. (No log line: the log lives on the Mac.)

## 1. Collect the batch

A PR is in the batch when either mark is on it:
- the `mergeme` label, or
- a conversation comment by `alexbeeken` whose whole text is the single
  word `mergeme` (any case, surrounding whitespace allowed, nothing else).
  A comment that says more than that word does not count, and a comment by
  anyone else does not count.

Find the comment-marked PRs first and give them the label, so GitHub shows
the same batch you are about to land and the rest of this skill only has to
look at the label:

```sh
repo=$(gh repo view --json nameWithOwner --jq .nameWithOwner)
for n in $(gh pr list --state open --limit 200 --json number --jq '.[].number'); do
  if gh api "repos/$repo/issues/$n/comments" --paginate \
       --jq '.[] | select(.user.login == "alexbeeken") | .body' \
     | grep -qixE '[[:space:]]*mergeme[[:space:]]*'; then
    gh pr edit "$n" --add-label mergeme
    echo "labelled #$n from a mergeme comment"
  fi
done
```

Then collect the batch:

```sh
gh pr list --label mergeme --state open \
  --json number,title,headRefName,baseRefName,mergeable,body,statusCheckRollup,labels
```

Note for the report which PRs came in by comment rather than by label.

**Drop every PR that also carries `needs work`.** The Review Desk adds that
label when the user wants changes, and a fix agent is working on the PR. It
isn't ready, even with `mergeme` on it. Leave both labels as they are (only
pr-fix removes `needs work`), leave it out of the batch, and name it in the
report and the log line: "dropped #1103 (needs work)". It waits: pr-fix
takes the label off when the fix is in, and the next run lands it.

For each PR, note:
- **Its base.** A base that isn't `main` makes it a stack. Find the PR that
  owns that base branch. If that PR is in the batch, fine. If it isn't, the
  batch would bring in work the user hasn't approved (#651 carried #646 this
  way). Leave the upper PR out (it waits) and ask once:
  `$BASE/scripts/review-desk/ask.py --pr <upper> --slug base-not-approved --asked-by mergeme --question "PR #<upper> sits on PR #<base>, which isn't marked mergeme. Land both?" --option "Land both: mark #<base> mergeme" --option "Wait until I've reviewed #<base>" --recommended "Wait until I've reviewed #<base>"`.
- **Its issues.** Collect the `Closes #N` / `Part of #N` lines and the
  `[N]` numbers in the title. The combined PR needs all of them.
- **Its QA steps.** The `- [ ]` boxes under "How to QA it", for the combined
  body.
- **Whether it changed since QA.** The user marks a PR `mergeme` after
  watching its QA demo, so the batch should land the code QA checked. Run
  `.claude/skills/mergeme/scripts/qa-fresh.sh <N> ...` on the whole batch. For
  each PR it finds the commit the latest QA report checked (the
  `**Checked at**` line of the `## QA: PR #N` comment, or `checked_at` in
  `~/skrongle-demos/pr-<N>/manifest.json`, see
  `../pr-qa-demo/references/manifest.md`) and prints a state. It uses the
  same rule as pr-qa-demo's `find-prs.sh`, written down under "Stale" in
  that manifest page:
  - `current` or `main-only`: fine. `main-only` means the new commits only
    merged main (or the PR's base branch) in, which doesn't count as a
    change.
  - `stale`: the PR has its own new work since QA, listed in the last
    column (`rewritten` when GitHub can't find the checked commit any more).
    Merging another PR's branch in counts as new work. It waits: leave it
    out, and post one comment on the PR so the user knows why it didn't
    land: "Left out of the mergeme batch of <date>: it changed since the QA
    you approved (<commits>). pr-qa-demo checks it again and the next
    mergeme lands it." pr-qa-demo QAs a stale PR even when it is labelled
    `mergeme`, so no label change is needed.
  - `none`: no QA report says which commit it checked (older reports, or no
    QA at all). Don't block on it. Say so in the report: "#1090 has no
    Checked at, so I couldn't tell if it changed since QA".
- **Its QA verdict and when it was approved.** Run
  `.claude/skills/mergeme/scripts/approval.sh <N> ...` on the batch. It
  prints the latest QA verdict and whether the `mergeme` label came before
  or after that report:
  - `label-after-qa`: the user approved it knowing this report and this CI
    (the desk confirms a red card before labelling). Land it as they asked,
    even with "not ready" or red checks on it.
  - `qa-after-label` with verdict `not ready`: a re-QA (pr-fix's, or
    pr-qa-demo's after a change) found something the user hasn't seen. It
    goes back to the desk (below). A `merge` verdict here is fine.
- **Two PRs that build the same thing in different ways.** Read the titles
  and bodies against each other. That's a choice, not a conflict to resolve:
  leave both out (they wait) and ask once:
  `ask.py --pr <newer> --slug same-thing-twice --asked-by mergeme --question "PR #A and PR #B both build <what>, in different ways. Which one lands?" --option "#A" --option "#B" --option "Both, merged by hand" --recommended "<the one that matches the issue better>"`.

**Back to the desk.** A PR goes back when its fresh QA says not ready
(`qa-after-label`), or when its own code makes the combined CI red for real
(step 5). Take the label off and say why on the PR, in that order:

```sh
gh api -X DELETE "repos/{owner}/{repo}/issues/<N>/labels/mergeme" --silent
gh api repos/{owner}/{repo}/issues/<N>/comments -f body="Back to the Review Desk from the mergeme run of <date>: <why, one plain sentence, with the link to the QA comment or the failed run>. Mark it mergeme again or send it to a fix agent there."
```

The desk shows it again as a red card with the reason, and the user decides
there. Name it in the report and the log line: "back to desk #1104 (QA not
ready)".

Order: each stack from the bottom up, then the rest by PR number. Merging a
stack's lower PR first means the upper PR's merge only brings in its own
commits.

## 2. Dry-run before touching anything

Make one detached scratchpad worktree off main. PR branches are usually
checked out in other orca worktrees, so never check them out by name.

```sh
git fetch origin
git worktree add --detach <scratchpad>/mergeme origin/main
```

In it, merge each `origin/<head>` in order with `git merge --no-ff --no-commit`.
Note which ones conflict and in which files, then `git merge --abort`. Or check
each step with `git merge-tree --write-tree HEAD origin/<head>`. Write the
plan down in a few short lines for the report: the order, which merges
conflict, and where.

## 3. Merge for real and resolve conflicts

Merge each PR in order. Use real merges (`git merge --no-ff origin/<head>`),
never squash or cherry-pick. The original head commits have to end up in
main, or GitHub won't mark those PRs merged.

Conflicts are resolved by one `opus` agent, never by Qwen
(`factory-rules.md`, section 1). Give it the conflicted files, the PRs'
numbers and this section; it resolves them in the scratchpad worktree and
reports one line per conflict. Check each with `git diff HEAD -- <file>`:
it shows only what both PRs meant, and the build and the one test behind it
pass (step 4).

Resolve every conflict hunk by hunk. Never `git checkout --theirs/--ours` a
whole file: that silently drops the other side's clean changes, and CI often
doesn't notice. After each file, `git diff HEAD -- <file>` should show only
what you meant.

The usual conflict spots, and the fix for each:
- **Migrations** (`MIGRATIONS` in `src/db/migrations.rs`): the version is the position
  in the list. Two PRs that both added "v61" become v61 and v62 in merge
  order. Append after main's last one. Fix every "vNN" / "Migration NN"
  mention in comments, tests and docs (`docs/architecture.md`,
  `docs/scalability.md`, `docs/data-inventory.md`):
  `git grep -n "v6[0-9]\|Migration 6[0-9]"`. If two PRs create an index with
  the same name or the same columns, keep one.
- **`.github/workflows/smoke.yml`** exact-spec list: keep every entry from
  both sides.
- **`i18n/*.yaml`**: keep both sides' keys. A key one PR adds to `en.yaml`
  must be in every other language file (`cargo test i18n` catches a missing
  one).
- **A PR that carries older copies of other PRs in the batch** (an
  integration branch): the newer PR's version wins. Keep only that PR's own
  commits' changes.

If two PRs turn out to build the same thing in different ways only now,
in the merge, that's a choice, not a conflict to resolve: `git merge
--abort` that one, leave the later PR out, and ask as step 1 says
(`same-thing-twice`).

Keep notes as you go. Each resolved conflict gets one line in the PR body.

## 4. Build, then open the combined PR

Per CLAUDE.md, CI runs the full checks. Locally, `cargo build` is enough,
plus the single test behind anything you changed while resolving (for
example `cargo test db` after renumbering migrations, `cargo test i18n`
after merging yaml). Other worktrees share `CARGO_TARGET_DIR`, so first
`find src templates i18n -exec touch {} +`. Otherwise cargo can reuse a
stale binary and you get false passes.

Push by refspec from the detached worktree:

```sh
git push origin HEAD:refs/heads/alexbeeken/mergeme-<YYYY-MM-DD>
```

Open it against main with `gh pr create --base main`:
- **Title**: every issue number in brackets, then a short summary:
  `[592][593][633] mergeme batch: Skrongle mini, sign-up, query indexes`.
- **Body**, in this order:
  - `## What this PR is for`: one bullet saying it lands the `mergeme` PRs
    (list them as #N) in one CI run. Add a bullet for each thing a reviewer
    needs to know (a stack merged bottom-up, a PR that came along with
    another). Short sentences.
  - `## How to QA it`: `- [ ]` steps starting from
    `./.dev-start --reset --seed --background`. Take the most important step
    from each original PR's QA list, not all of them, and end with
    `./.dev-start --stop`.
  - `## Conflicts resolved along the way`: one bullet per conflict, saying
    what each side did and what was kept.
  - The `Closes #N` lines for every issue the originals closed.
  - The attribution line.

## 5. Watch CI and merge

Watch in the background with `gh pr checks <N> --watch` (via a background
Bash command, or a watcher subagent with `model: "haiku"` that replies in
one line), so you aren't polling by hand. Don't read the full CI log into
this session; `gh run view <id> --log-failed | tail -40` shows the part
that failed.

- **Known flaky specs**: profile-nav "marks the section in view", the
  authenticator TOTP period. If one of these fails on
  its own, rerun it once: `gh run rerun <run-id> --failed`. Two failures on
  the same commit means it's real.
- **A real failure in the merge itself** (a migration number, a missing
  i18n key, a spec list): find the failing line first (`gh run view
  <run-id> --log-failed > $S/ci.log`, ask Qwen "Which test failed, and the
  first error line, as `test | line`. Nothing else.", and `grep -n` that
  line). The `opus` agent that resolved the conflicts reruns that one test or spec locally,
  fixes it on the branch and pushes; add a line to "Conflicts resolved".
- **A real failure in one PR's own code**, with no mechanical fix: that PR
  goes back to the desk (step 1, "Back to the desk", with the failed run's
  link). Then rebuild the batch without it: a fresh detached worktree off
  main, the same merges in the same order minus that PR, push to a new
  `alexbeeken/mergeme-<date>-2` branch, open the combined PR again with
  the old one closed, and watch CI again. Two rebuilds in one run is the
  limit; after that, stop with the log line saying so.
- **Checks stuck at Queued** for an hour usually means the runners are
  down (`scripts/ci-runners.sh status`). Stop with the log line saying so
  ("in CI #A #B · stopped: runners down"); the combined PR stays open and
  the next run finds it and doesn't start another.

When it's green, `gh pr merge <N> --merge --auto`. Use `--merge`, never
`--squash`: a squash puts new commits on main, so the original PRs never
show as merged. `--auto` is safe here because the base is main, where the
ruleset holds the merge until checks pass.

## 6. Check it landed

```sh
git fetch origin
git merge-base --is-ancestor <combined-merge-commit> origin/main && echo landed
for n in <batch>; do gh pr view $n --json number,state --jq '"\(.number) \(.state)"'; done
```

Every original should say MERGED. `gh pr close` refuses on those, so there's
nothing to close. If one is still OPEN, its head didn't make it into the
combined branch: find out why before you report. Then remove the worktree with
`<BASE>/.claude/skills/rotate/scripts/drop-worktree.sh <the full path of <scratchpad>/mergeme>`,
written out in full, never `rm -rf`.

## 7. Write the log line

Every run that got past the cloud check appends one line to
`~/skrongle-workflow/log/mergeme.log`, also a run that stopped part way
(CI failed for real, the runners down, nothing to land). The Review Desk
shows the newest line as it is, so keep to this shape exactly:

```
2026-10-28 14:02  PR #1130: landed #1101 #1102 · dropped #1103 (needs work)
```

- Local date and time (`date '+%Y-%m-%d %H:%M'`), then two spaces.
- `PR #<combined>: ` once the combined PR exists, otherwise nothing.
- Then what happened, parts joined by ` · `:
  - `landed #A #B` when it merged;
  - `in CI #A #B` if you stop before CI finishes;
  - `stopped: <why, a few words>` for a run that ended without landing
    ("stopped: CI red on cargo test", "stopped: runners down",
    "stopped: no mergeme PRs");
  - `dropped #N (<reason>)`, one part per PR left out that waits, reasons
    in plain words: `needs work`, `changed since QA`, `base not approved`,
    `same as #M`;
  - `back to desk #N (<reason>)`, one part per PR sent back: `QA not
    ready`, `CI red on its code`;
  - `asked <id>` for each question written;
  - `qwen <n>` (Qwen calls, or `qwen down`).
- One line, no trailing ` · `. Leave out parts that are empty.

```sh
mkdir -p ~/skrongle-workflow/log
printf '%s  %s\n' "$(date '+%Y-%m-%d %H:%M')" \
  'PR #1130: landed #1101 #1102 · dropped #1103 (needs work)' \
  >> ~/skrongle-workflow/log/mergeme.log
```

A run that lands nothing because every PR was left out still writes its
line ("stopped: nothing to land · dropped #1104 (changed since QA)").

## Report

End with a short report in plain words. Say which PRs landed (or the
combined PR's number if it's still in CI), which PRs wait and why (`needs
work`, changed since QA, base not approved), which went back to the desk
and why, which questions you wrote for the desk's Decisions mode, which had
no Checked at to compare, what conflicted and how each one was settled, and
anything that flaked and was rerun. Use short sentences and no jargon the
user didn't use first. End with:

```
Open the Review Desk: `review`
```
