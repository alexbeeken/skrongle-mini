# Side-by-side before/after video

The reviewer should see what the PR changed next to what it replaced, both
on screen at once, labelled, so the difference is obvious without reading
the diff.

The demo is before/after screenshot pairs by default (`agent-brief.md`,
section 4, makes them in a staging folder `$D`; section 5 writes
`manifest.json` and publishes the folder to `~/skrongle-demos/pr-<N>/`
with `demo-folder.sh publish`; the folder format is `manifest.md`). Follow
those as written. This page adds only one thing: when the user asks for a
video, the video is a side-by-side of main and the PR, not a single-screen
walkthrough. It goes into the same folder as `demo.mp4`, next to the
screenshot pairs.

`$SK_QA` is `$BASE/.claude/skills/pr-qa-demo` (this folder). `$D` is the
staging folder from the brief (`$S/pr<N>`).

## Two servers, same steps

The screenshots can share one worktree (after shots, then check out the
merge base and rebuild). A side-by-side video is easier with both servers up
at once. "Before" is main at the PR's merge base; "after" is the PR branch.
Each runs from its own worktree, because each worktree has its own `dev.db`
and picks its own port.

```sh
cd $BASE && git fetch origin
MB=$(git merge-base origin/main origin/<head>)
git worktree add "$S/wt-before-<N>" --detach "$MB"
cp -Rc $BASE/target "$S/wt-before-<N>/target"      # $BASE is on main, so this cache is already warm for "before"
cp -R  $BASE/e2e/node_modules "$S/wt-before-<N>/e2e/node_modules"
(cd "$S/wt-before-<N>" && cargo build 2>&1 | tail -1 && ./.dev-start --reset --seed --background)
BEFORE_URL=$(cat "$S/wt-before-<N>/.dev-server.url")
AFTER_URL=$(cat "$WT/.dev-server.url")              # the PR worktree you already have
```

A change confined to `static/` or `i18n/` needs no second build: debug
builds read `static/` from disk and load `i18n/` at startup. Check out
main's copies of those files into the PR worktree, restart the server, and
that is "before"; then `git checkout HEAD -- static i18n`, restart, "after".

## Drive both with one spec

Write the walkthrough once as a function of `(page, url, label)` and run it
twice: the demo template's `walkthrough` body against `BEFORE_URL`, then
`AFTER_URL`. Same steps, same `say` captions, same hold times, so the two
recordings line up frame for frame. The caption bar on each side says the
label: `say(page, "Before (main): the row reads 4 of 4 correctly")`.

Rules that keep the comparison honest (the screenshot rules in the brief
apply too):

- Locators must work on both sides. Don't rely on a `data-testid` the PR
  adds; use text, role or a selector that exists on main too.
- Where the PR adds something that main lacks (a new link, a new page), the
  "before" shows the place it would be, with a caption saying it isn't
  there. Don't skip the before side; the gap is the point.
- An `expect` on the after side where the result matters, as in the brief,
  so the video can't show a broken claim as working. On the before side,
  expect the old behaviour (or just `say` what is on screen if the old
  behaviour is an error).
- The seeded dev user must be in the same state on both servers. Both ran
  `--reset --seed`, so they start equal; do the same clicks in the same order.

Record two `.webm` files (`DEMO_OUT=$S/before-<N>.webm`, then
`$S/after-<N>.webm`), each with the same `DEMO_TITLE`.

## Stack them into one frame

Side by side for desktop (two 1280×720 recordings scaled to 960 wide, 1920
total), stacked on top of each other for phone-only PRs (390×844 each).

The pane labels ("Before (main)", "After (PR #N)") are drawn by the page,
not by ffmpeg: this Mac's ffmpeg has no `drawtext` filter. Add a second
element to the template's overlay, a small box pinned to the top left
(`#__demo_label`, text from `sessionStorage.__demo_label`, same font as the
caption bar, `position:fixed;left:12px;top:12px;padding:6px 12px;background:
rgba(20,20,24,.86);color:#fff;border-radius:6px;z-index:2147483646`), and
set it once per recording with `page.evaluate` before the title card. It
survives navigations the same way the caption does, so every frame of each
side carries its label.

```sh
ffmpeg -y -loglevel error -i $S/before-<N>.webm -i $S/after-<N>.webm -filter_complex "
  [0:v]scale=960:-2[l];[1:v]scale=960:-2[r];[l][r]hstack=inputs=2[v]" \
  -map "[v]" -c:v libvpx -b:v 2M -deadline realtime -cpu-used 8 $S/demo-pr<N>.webm
$SK_QA/scripts/finish-demo.sh $S/demo-pr<N>.webm --out $D/demo.mp4
```

For phone-only PRs drop the `scale` steps and use `vstack`. If the two
recordings differ in length, `hstack` holds the shorter one on its last
frame until the longer one ends (checked 2026-10-28). Better still, hold
every step to a fixed length so they match.

Then set `"video": "demo.mp4"` in the manifest and publish as the brief
says. The screenshot pairs stay the main demo: the Review Desk shows them
first and the video under them. Don't stack the pairs into one image; the
desk puts each pair side by side itself.

## In the PR

Nothing extra. The brief's `## Demo` section names the folder, the number of
pairs and the video's length, and the QA comment's Demo line says the same.
If a before side couldn't be recorded (main errors on the page the PR
fixes), record the error as the before, caption it, and say so in the
comment.

## Lessons from the 2026-10-24 run

- The static-files shortcut works as described: swap main's copy of the
  CSS file into the one worktree, record, restore. The debug server picks
  the change up without a restart.
- The template's overlay script also runs inside iframes (the dev inbox on
  `/login/code`), so a second caption and label appear there. Start
  `OVERLAY` with `if (window !== window.top) return;`. Give the label box
  its own inline style, because `textPage` rebuilds the overlay.
- Headless recordings have no address bar. When the PR is about a URL, draw
  the page's URL in the overlay and update it after `history.replaceState`.
- Same `SKRONGLE_RNG_SEED` on both servers (`.dev-start` passes it through)
  gives identical Sprint sessions, so a seed that shows the bug on main shows
  the fix on the PR with the same taps.
- Generate course content in a non-recording browser context and copy its
  cookies into the recording one, so the recordings line up frame for frame.
- A change with no UI (a flaky spec, a script): run the thing many times on
  both sides under load and show the two result summaries on text pages,
  then the page it touches on both sides so the reviewer sees behaviour is
  unchanged.
- Main's error pages (a 405) can have an empty body; the before side then
  relies on the caption plus a status assert.

The 2026-10-25 and 2026-10-27 lessons are in the brief's harness facts.

## Clean up

```sh
<BASE>/.claude/skills/rotate/scripts/drop-worktree.sh <the full path of $S/wt-before-<N>>
```

Write the path out in full, not as a variable. Leave the `.webm` files in
`$S`: the scratchpad is temporary, and an `rm` on `$S/*.webm` waits for a
person to approve it (`.claude/skills/rotate/references/factory-rules.md`,
section 4). Then the brief's own clean-up (section 6) for the PR worktree.
