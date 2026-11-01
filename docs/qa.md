# QA-testing a pull request

How to take on the role of QA for one PR: click through it locally, find
what is broken or unfinished, leave deterministic e2e specs behind, and say
whether it is ready to merge. This page comes from QA runs on real PRs.
Update it when a run teaches something new.

A QA run always produces three things:

1. Specs added to the PR's branch (deterministic, fixtures, no key).
2. Fixes for small bugs found, in the same branch, and issues for the rest.
3. A summary posted as a comment on the PR, with a coverage audit and a merge
   verdict ([template](#summary-template)). It lives on the PR, not in the repo.

Bugs live in issues, nowhere else. Run `gh issue list` first (a bug may be
filed already; link it instead of filing again). Don't commit PR-specific
reports, and don't commit `test.fixme` or other tests for bugs that aren't
fixed. Put the repro steps, and the spec to add with the fix, in the issue.

## 0. Before you start (five minutes, saves an hour)

| Check | How | If it fails |
|---|---|---|
| The Jev key is in your environment | `zsh -ic 'echo ${#TYPESAFE_API_KEY}'` (the key lives in `~/.zshrc`; a non-interactive shell does not see it) | Say so and ask. Don't silently do only the exact checks. |
| Run everything through `zsh -ic '…'` | `zsh -ic 'scripts/e2e.sh specs/x.spec.ts'` | Without it the Jev specs skip, and you won't notice. |
| The PR's base | `gh pr view N --json baseRefName,files,body`. A **stacked** PR (base is another PR's branch) is reviewed against that base: `git diff origin/<base>...origin/<head>` | Merge order goes in the verdict. |
| Check the PR out as a tracking branch | `git fetch origin <head> && git switch -c <local> --track origin/<head>` in your worktree | Never `git stash` bare: the stash stack is shared by every worktree. Use a WIP commit, or a tagged stash and apply by SHA. |

## 1. Read, then write the claims down

Read the PR body and the diff (not the whole repo). From the body, list
every claim: each thing the PR says a learner can now do, each rule it
says holds, each "blocked while…" state. That list is the audit table in the
summary. A claim with no test is a gap.

Then read the code for what the body does not say. These found the real
problems so far:

- **Every consumer of a new state.** Adding an enum value or a course state
  (`revising`)? `grep` every place that branches on the enum and on its
  string form (templates, list pages, JS). Each must handle the new value.
- **What the functions the PR calls do to state.** The best find on #52 was
  not in its diff. The new Save button calls `revalidate`, which flips a
  stopped run to `ready` and so kills Resume. For each existing function a new
  button, route or script calls, read its body for side effects (`UPDATE … SET
  state`, cache invalidation, redirects) and ask which other screen depends
  on the old state.
- **Every state the new UI can appear in**, not just the happy one: running,
  done, stopped (failed, cancelled), no JavaScript, phone width.
- **Every other way into the same data.** Locked while X runs? Try each
  write path: editor save, upload API, delete, Undo, a second start, Cancel.
- **Races.** A check followed by an unconditional write (`if ready … UPDATE`)
  is a race. Fire 3 requests at once.
- **Text the JS rewrites.** Server-rendered text that a poller overwrites
  (the "rewritten or added" label became "written").
- **Owner-only.** New routes must 404/400 for another user.
- **Failure ends.** Bad input, a model that returns junk, cancel, a restart.
  Does the page stay usable and say what happened in plain words?
- **Project rules** in `CLAUDE.md` and `.claude/rules/`: docs updated, `data-testid` on new
  markup, a journey for a new essential flow, fixtures for a new model call.

## 2. Baseline

```sh
zsh -ic 'cargo test 2>&1 | grep "^test result"'
zsh -ic 'scripts/smoke.sh | tail -3'
zsh -ic 'scripts/e2e.sh'            # all exact + smoke specs, Jev too
```

Record the numbers. A failure here is either the PR's or already on `main`;
settle which before going on (see [pre-existing](#pre-existing-or-caused-by-the-pr)).

## 3. Explore with Jev (cheap, leads only)

Write a throwaway persona (`e2e/personas/<name>.ts`, goals start at
`/dev/login?open=spanish&keys=fake&next=<page>` so AI forms are unlocked) and
run `zsh -ic 'scripts/e2e-explore.sh <name> --max-calls 150'`. A run costs
about half a cent and takes three minutes.

What to expect, from experience:

- Jev is good at finding unlabelled buttons, error text not in `role=alert`,
  overflow on a phone, state only in a class, and pages that never say what
  happened.
- Jev is bad at long flows with a background job. It clicks away while a job
  runs, hits Cancel, wanders to unrelated pages, and the page polls (4 s) so
  the end state arrives after it has left. 0 of 3 goals reached is normal.
- "No effect" findings are usually the harness: a `confirm()` dialog
  Playwright dismisses, or a `required` box left empty. Reproduce by hand
  before believing one.
- Findings on pages the PR didn't touch (the tutor 404) go in "not this PR".
- A Jev journey needs a short goal with an exact `check` and a start state
  that already exists. A "generate, then revise" journey can't have either, so
  it was left out. Say that in the summary, and don't ship a flaky one.
- A run covers one viewport. A persona that lists `["desktop", "phone"]` ran
  only the first. Run the phone size as a second pass (`viewports: ["phone"]`).
  The overflow check is always on at phone width.
- When the flow has no background wait (a panel on an existing page), Jev
  reaches the goal in a few steps for a tenth of a cent. Its findings were
  low-severity wording and testability. Run it, but don't trust it.

Delete the persona unless it is reliable. Never commit a profile that reaches
0 goals.

## 4. Write the deterministic specs (the real deliverable)

One file per feature: `e2e/specs/<feature>.spec.ts`, exact, no model in the
loop. This pattern worked:

- **Helpers at the top** (`makeCourse`, `revise`, `lessonTitles`), so each spec
  reads as the user's story.
- **Drive scenarios with fixtures.** `[fixture:NAME]` in the typed text picks a
  reply from `data/test-fixtures/llm/<purpose>.json`. Need a state that is too
  brief to catch (a job that finishes in 1 s)? Add a `slow` scenario with
  `delay_ms` instead of racing it.
- **Wait for state, not time.** Assert on `data-*` attributes, or
  `expect.poll` the JSON status API. Never `waitForTimeout`. Poll the API (not
  the DOM) once you've navigated away from the page that updates itself.
- **Refusals.** Use `request.post(..., { maxRedirects: 0 })` and assert the
  `303` and the `Location` (the notice text), so you test the server and not
  just the UI.
- **Dialogs.** Call `page.once("dialog", d => d.accept())` before the click.
  Test accept and dismiss, and read `d.message()` for the warning wording.
- **Progress through the API.** `POST /api/progress/seen` then read
  `/api/progress/lessons/<id>`. This is faster than clicking a lesson.
- **A helper that clicks a `<details>` toggle closes it if it is already
  open.** After `goBack()` or a reload the panel may be restored open. Load
  the URL fresh (`page.goto(statusUrl)`) before calling the helper.
- **No JavaScript.** Use `test.describe(…, () => { test.use({ javaScriptEnabled: false }); … })`.
  Use `page.request` (the page's session), not the `request` fixture, which has
  none. Without scripts nothing polls, so reload until `data-done` is `true`.
- **Each spec makes its own data** (unique wish text), and `afterAll` resets.
- Cover, per PR: the happy path, each failure end, each lock, the second time
  (do it twice in a row), undo/back, and owner-only.

### Tell a spec bug from a product bug

When a new spec fails, read the code before changing anything. In the first
run, the spec caused three of three failures: a fixture that legitimately
returned "nothing changed" the second time, a page that renders in place (no
redirect), a check that never reloaded. Fix the spec if the product does what
its code and body say. File or fix the product if a user would be surprised.

### Prove the spec can fail

Copy the spec file to the scratchpad, break an assertion (or remove the fix),
rerun, and copy it back. A spec that passes in half a second looks suspicious
but may just be fast, and the break test settles it. Temporarily remove the
fix (or break the behaviour) and restore it. A spec that stays green is not
guarding what you think. Say so in the summary if you can't make it fail (a
double-submit spec once stayed green because another check already blocked
the race, so the fix there was defence in depth).

## 5. Fix, file, or note

| Finding | Action |
|---|---|
| Small, clearly wrong, inside the PR's area | Fix in the PR's branch, with a spec that guards it |
| Real but separate, or a judgement call (copy, UX) | `gh issue create`, with where, why, repro steps in the browser, fix idea, the spec to add with the fix |
| Blocks merge | Say it first in the summary; fix it if it is small |
| Not the PR's | List under "not this PR"; file only if new |

### Pre-existing or caused by the PR?

Check what the PR touched (`git diff --stat <base>...<head>`) and whether the
failing text or code is in it. A Jev-judged spec scoring the same low value on
every run, on wording the diff doesn't touch, is pre-existing. When it is
cheap, check it against the base branch in a second worktree.

## 6. Re-run and push

```sh
zsh -ic 'scripts/e2e.sh'            # everything, not only your spec
zsh -ic 'cargo test 2>&1 | grep "^test result"'
zsh -ic 'scripts/smoke.sh | tail -1'
```

Commit specs and fixes together with a message that lists what is covered, and
push to the PR's branch. Specs for new fixtures and scenarios update
`docs/testing.md` in the same commit.

## Shell gotchas (macOS)

- `sed -i` needs an empty suffix argument (`sed -i ''`); edit with Python or the
  Edit tool instead.
- A pipe through `grep` after `zsh -ic` hides the build output; add
  `grep -E "✓|✘|passed|failed|skipped|Expected|Received|^ +>"` to read results
  and open `e2e/results/artifacts/<test>/error-context.md` for the page at the
  failure (it shows the accessibility tree, which usually explains it).
- Stacked PRs: specs pushed to the base PR's branch reach the upper PR when it
  is rebased or merged. Say so in the summary so a conflict in `e2e/specs/`
  doesn't surprise anyone.

## Summary template

```markdown
## QA: PR #N <title>
**Verdict:** merge / merge after <X> / not ready — one sentence why.
**Checked at** `<8-character commit>`
**Stack:** base branch, merge order.
**Checks:** cargo test n · smoke ✓ · e2e a passed, b skipped, c failed (why).
**Bugs:** fixed (with spec) · filed (#issue) · none.
**Not this PR:** pre-existing findings.
**Coverage audit:**
| Claim in the PR | Covered by | Gap |
**Left uncovered, and why:** (flaky by nature, needs a real provider, …)
```

Post it with `gh pr comment N --body-file …`. A verdict names the blockers
and says "none" when there are none. The author reads the verdict first.
"Checked at" is the PR head after your own last push
(`git rev-parse --short=8 HEAD`), in backticks, with nothing else on the line
before it. It must match the demo manifest's `checked_at`. pr-qa-demo's
`find-prs.sh` compares it with the PR head to tell when QA is out of date.

## Efficiency notes

- Read the body and `files` first; they tell you which `e2e/specs` and
  templates matter. Skip the rest of the repo.
- Run one spec at a time while writing it (`scripts/e2e.sh specs/x.spec.ts -g "name"`),
  the whole suite once at the end. Each run builds and starts a server.
- Write all specs for a PR in one pass from the claims table, run, then
  triage failures together.
- One exploration run is enough. More runs of the same persona repeat themselves.
- Don't spawn agents for this. The work is sequential and each step needs the last result.
