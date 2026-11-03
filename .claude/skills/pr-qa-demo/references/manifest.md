# The demo folder and its manifest

Every PR's demo lives in one folder on the QA Mac:

```
~/skrongle-demos/pr-<N>/
  manifest.json                      # what is in the folder (below)
  01-before.png, 01-after.png, ...   # one before/after pair per item
  demo.mp4                           # only when the demo is a video
```

pr-qa-demo writes it. The Review Desk, pr-fix, backlog-sweep and mergeme read
it. This page is the one place the format is written down. Don't add a second
copy in another skill: link here.

Rules for the folder:
- One folder per PR, named by the PR number only (`pr-1103`), never by issue.
- A new QA run replaces everything in it. `scripts/demo-folder.sh publish`
  does that in one step, after checking the manifest, so a reader never sees
  a half-written folder.
- Only the files the manifest names go in it. No `.webm`, persona, report or
  log files.
- It stays on this Mac. Nothing in it is committed or uploaded.

## manifest.json

| Field | Required | Type | What it holds |
| --- | --- | --- | --- |
| `schema` | yes | number | Always `1`. A reader treats a missing `schema` as `1`. |
| `pr` | yes | number | The PR number. Same as the folder's `<N>`. |
| `issue` | yes | number or `null` | The first issue number in the PR title, `null` if it has none. |
| `title` | yes | string | The PR title, as on GitHub. |
| `why` | yes | string | One line in plain words: why this change matters to a learner or to us. |
| `verdict` | yes | `"merge"` or `"not ready"` | The QA verdict, the same as the QA comment's. "Merge after X" is `"merge"`, with X in `verdict_note`. |
| `verdict_note` | no | string | The condition or reason, one line: "after renumbering migration v88", "CI red on cargo test". |
| `checked_at` | yes | string | The commit QA checked, 8 hex characters, exactly as on the QA comment's `**Checked at**` line. It is the PR head after any fix QA pushed. |
| `qa_comment_url` | yes | string | The `html_url` of the `## QA: PR #N` comment this run posted. |
| `qa_page_url` | no | string | The claude.ai QA page section for this PR (`…#pr-<N>`), when the run made one. |
| `items` | yes | array | One entry per thing the change shows, in the order to look at them. Can be empty only when `video` is set. |
| `items[].caption` | yes | string | One short sentence: what to compare. |
| `items[].before` | no | string | File name in the folder, e.g. `01-before.png`, shot on main. Left out when main has nothing to show (a new script with no page). |
| `items[].after` | yes | string | File name in the folder, e.g. `01-after.png`, shot on the PR branch. |
| `video` | no | string | `demo.mp4` when the run recorded a video. |
| `made_at` | no | string | When the folder was written, ISO 8601 with time zone. |

File names in `items` and `video` are plain names inside the folder, never
paths. Items are numbered from `01` in order.

How a reader uses it:
- The demo is current when `checked_at` is a prefix of the PR's head commit,
  or when every commit after it is only a merge of main (see "Stale").
  Otherwise the PR has new work since QA, and the demo may be out of date.
- A PR with no `pr-<N>/` folder is from before this format. Its demo, if any,
  is a file in `~/skrongle-demos/` with `pr-<N>-` in its name, or the path in
  the PR body's `## Demo` section.

## Stale

A PR is stale when it has its own new work since `checked_at`. Merging main
into the branch (a main-merge sweep, often with `[skip ci]`) is not new work.
Every tool uses this rule; `scripts/find-prs.sh` is the reference
implementation, with REST calls only:

1. If the PR head starts with `checked_at`, it is not stale. No calls.
2. `own` = the commits in `compare/<base>...<head>`: the PR's commits that
   its base branch (main, or the branch it is stacked on) doesn't have.
3. `new` = the commits in `compare/<checked_at>...<head>`. If GitHub can't
   find `checked_at` there (a rebase or force push), it is stale.
4. A **base merge** is an `own` commit with two parents whose second parent
   is not in `own`: it pulled main (or the base branch) in.
5. Stale = some commit is in both `own` and `new` and is not a base merge.

Commits from main that a merge brought in are not in `own`, so they never
count. Merging another PR's branch in does count: its second parent is in
`own`. A report from before the `Checked at` line has no `checked_at`; for
it, `new` is the `own` commits whose committer date is after the report.

## Example

```json
{
  "schema": 1,
  "pr": 1103,
  "issue": 879,
  "title": "[879] Verb Drill draws the person slots with profile pictures",
  "why": "Learners see who each verb form is about, without reading yo/tú/él.",
  "verdict": "merge",
  "checked_at": "4c748861",
  "qa_comment_url": "https://github.com/alexbeeken/pooki/pull/1103#issuecomment-6476126153",
  "qa_page_url": "https://claude.ai/artifact/MCNMD32h5Md7oxGBLQyd4i#pr-1103",
  "items": [
    {"caption": "The drill setup: each slot now shows its people.", "before": "01-before.png", "after": "01-after.png"},
    {"caption": "The saved setting shows on the card.", "before": "02-before.png", "after": "02-after.png"}
  ],
  "video": "demo.mp4",
  "made_at": "2026-10-28T14:05:00-07:00"
}
```

## Checking one

```sh
.claude/skills/pr-qa-demo/scripts/demo-folder.sh check <dir>
```

It prints what is wrong (a missing field, a wrong type, a file the manifest
names that isn't there) and exits 1, or prints `ok`.
