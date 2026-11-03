# Harness facts

What earlier runs learned about the scripts and the personas. Add what will
still hold next time; delete what has been fixed.

- **Jev's limits are not bugs.** The playthrough player fills a text box
  one word at a time from the lesson page's word pool and never touches
  speakers or accent helpers. Study and Play lessons that need a conjugated
  form not on the page, or a listening answer, fail for that reason. Read
  the wrong answers and their confidence in the report before filing.
- **A stale wander goal after a course switch** gave a 404 on
  `/guide/<spanish slug>` once (2026-10-20): the bot had switched to the
  test course. Not a bug.
- **Disk.** A nightly run died with ENOSPC until stale `target/` directories
  under other worktrees were deleted. Check `df -h /` first.
- **Phone screenshots.** Chrome's `resize_window` tool is ignored and
  iframes are refused; use a throwaway Playwright script from `e2e/` with
  the iPhone 13 device.
- **The dev server can be shut down from outside the session.** Restart it
  with `./.dev-start --test-course --port <same> --background` to keep the
  DB.
- **Replay a Play (Guide) stage finding.** A finished Guide stage could not
  be replayed (#490, 2026-10-20): the server kept `step=total` and the
  first POST was "stale". Check whether that is fixed before reading a
  "try 2 stuck" in Play as new.
- **Never run the two playthrough scripts side by side** (2026-10-25).
  `scripts/jev-playthrough.sh` writes `./playthrough-<mode>.db` in the
  checkout and registers `jev-<mode>-<MMDDhhmmss>@example.com`; two runs
  started in the same second share the file and the second registration
  fails with "An account with that e-mail address already exists", then
  every mode fails at `/courses`. `run-all.sh` chains them for this reason.
- **Playthroughs fail at `/courses` until #895 is fixed** (2026-10-25):
  `scripts/jev-playthrough.sh` runs a mini server but never sets
  `E2E_MINI=1`, so `signUp` waits for a "check your mail" page that never
  comes. Until then run them as `E2E_MINI=1 scripts/jev-playthrough-all.sh`
  if the script still doesn't export it, and don't read the fatal
  `toBeVisible` at `/courses` as a site bug.
- **The `start-at-a2` journey fails by design of its budget** (#896): the
  Start wizard has six screens and the journey allows eight steps. Not a
  regression of the wizard itself.
- **Jev's soft findings that were false alarms on 2026-10-25**: `accurate`
  doubting the practice feedback (it had pressed Check with an empty box:
  "Type an answer first" is right); `testing-bot` calling the accent
  helper buttons symbol-only labels (they carry `aria-label="Type é"`);
  `accurate` doubting accents on `/` (p=0.45, nothing wrong). The exact
  `labels` check found nothing; trust it over the judged statement.
- **Fact-check false alarms** are recorded in each course's
  `fact-check-ok.txt` (`--why` prints the line). Jev proposes "tu cherches"
  → "cherche" after an English "you", and an accent on the echo question
  "¿Que si…?"; both were right as written.
- **Phone screenshots in bulk**: ffmpeg on this Mac has no `drawtext`;
  tile with `-f concat` + `scale,pad,tile=4x3` and read the sheet.
  `document.documentElement.scrollWidth - innerWidth` was 0 on all 131
  Spanish pages, 19 test-course pages and 29 docs pages (the #465 overflow
  is fixed); `<pre>` blocks in docs scroll inside their own box.
- **Course-card testids**: the stat chips on `/courses` are
  `courses-card-stat`; `courses-stat` is the modal's content. The editor's
  "new file" link (`course-edit-new-file`) is not on the lesson editor page.
- **Jev wanders on `start-at-a2`** (2026-10-29): a fresh account on
  `/courses` clicked "Create a course" (conf 0.27) before "Start learning" and
  burnt its 8 steps in the Generate dialog. The wizard itself was fine (the
  `change-level` journey passed). Rerun once before reading it as #896 back.
- **`accurate`'s "Browse the vocabulary list" goal can't be reached** on the
  seeded dev account: no terms are collected, so `/vocabulary` is the empty
  state and Jev loops between it and lesson pages for 20 steps. Not a bug.
- **Sprint bucket (sort) exercises**: Jev's first try often ends "out of
  steps" in a put-in / take-out loop ("Take hice out of the bucket"); the
  second try passes. A `stuck` on a bucket card with a ✓ on try 2 is Jev.
- **Fact check "could not be checked … reading 'usage'"** on one unit is a
  Jev API hiccup for that call, not a content fault; rerun that lesson.
- **zsh loops**: `for a in "spanish c2/x"; do set -- $a` does not word-split
  in zsh, so `$1 --lesson $2` is empty. Use `read -r c l <<< "$a"` or run the
  commands one per line.
- **A "full" Sprint playthrough stops at 8,000 Jev calls** (`--max-calls`,
  2026-10-29): 66 of 101 lessons, about $0.53, 45 min; Study (3,800 calls,
  17 min) and Play finish the course. The lessons after the cap are not
  played; pass `--max-calls` higher if the B2–C2 Sprint lessons matter.
- **The playthrough counter's denominator is the current lessons page**
  (`18/9`, `30/12`): the A1 list paginates 12 + 9. Cosmetic; the `#` column
  in `report.md` is the real index.
- **Study reading-check failures** (24 of 101 on 2026-10-29, all with 7–8/11
  practice answers right) are Jev's one-word fills: "media" for "y media",
  "verlo" for "leerla", "Había marchado" for "se había marchado". The check
  wants both questions right, so the lesson stays "Not read yet". Not a bug.
- **Test-course Study playthrough "stuck ×6 at `/practice?lesson=sort`"**
  (2026-10-29): the sort lesson has no quiz, so Practice is the empty "no
  questions available" page and the player idles there. Filed as #1197 (the
  lesson page offers quiz buttons it can't honour); not a playthrough bug.
- **A viewport screenshot of `/sprint` can show the tab bar's labels cut
  off** (2026-10-29): the first shot of the Sprint path at iPhone 13 size
  had "Path · Review · …" clipped at the bottom, but the DOM (nav bottom
  = innerHeight, label text ends 12 px above it) and a second shot were
  fine. Re-shoot before reading it as a layout bug.
- **`/dev/login?open=<course>` switches the dev user's active course for
  every later run on the same database** (2026-10-29): a walk of the test
  course left Spanish lesson URLs returning 404 in the next script. Pass
  `open=spanish` on each login, or read a 404 after a course switch as
  your own artefact.
- **Full-page screenshots of the course editor can show the lesson table
  floated over the "Lessons" heading** (2026-10-29) after a click that
  scrolled the page; the box is `position: static` and a viewport shot is
  fine. Not a bug.
- **The `start-at-a2` journey still fails** (#896 closed, groomed, not
  built): step 1 "Select the option A language" times out and the rest
  goes nowhere. Skip it in the journeys' step log.
- **Fact-check false alarms on 2026-10-29** (all on the ok-lists now):
  "aun así" without an accent (RAE: *aun* = *incluso*), "Ni rara tú ni raro
  él" where *tú* is a woman, "Vos tenés razón" sorted to the speaker from
  across the sea, "a lo mejor = maybe (always with the indicative)", the
  subjunctive fix "quiere que explores", and two English-with-French
  narrative lines ("«and» the radio says", "démissionné «of» the mines").
  Rerun one lesson with `scripts/fact-check.sh <course> --lesson <path>
  --why` to get the ok-list line: the answers are cached, so it costs
  nothing. In zsh, `set -- $l` does not split the string; use bash.
- **Jev's explore soft findings on 2026-10-29 were the 2026-10-25 ones
  again**: `accurate` doubting accents on `/` (p=0.48) and the practice
  feedback after an empty Check; `impatient` calling a lesson's opening
  explanation long; `testing-bot`'s "visit every page" goal never finishes.
- **TypeSafe credits can run out mid-sweep** (2026-10-29): the playthrough
  then logs `!! stuck: … 402 Your organization has no available TypeSafe
  API credits` once and every later lesson "fails" in 1 s with `0 calls`,
  `0/0 right`, "fudged (not seen …)". That is not the site; kill the
  playthrough chain (`pkill -f jev-playthrough`) and read only the groups
  that finished. A full sweep spends about $1.40 on the fact check alone,
  so check the balance at console.typesafe.ai before a full run.
- **`scripts/fact-check.sh --why` is free after a run**: every answer comes
  from `e2e/results/fact-check/cache.json` (0 calls, 5 s), so get the
  ok-list lines that way instead of re-asking Jev.
- **Jev's soft findings on 2026-10-29 (third)** were the 2026-10-25 set
  again (accents on `/`, feedback explains, long explanation, symbol-only
  labels, hidden menus) plus `accurate` not "browsing the vocabulary list"
  because a fresh account has no terms and the page says so. None is a bug.
- **Check the Jev balance before starting the scripts** (2026-10-30): the
  key being set says nothing about credits. One tiny call from `e2e/`
  (`new TypeSafeClient({ apiKey })`, `systemOne` on a one-line state) costs
  nothing worth counting and answers `402 … no available TypeSafe API
  credits` when the balance is empty; then every group fails in seconds and
  the playthrough "fails" each lesson in 1 s with 0 calls. With no credits,
  do step 3's manual walk (phone-width crawl over the GET routes, the
  `POST /lang lang=de back=/` text pass, the dialogs behind every (i)), file
  what it finds, put the top-up question on the desk against the credits
  issue, and write `stopped: no Jev credits` in the log line.
- **The dev LLM fixtures speak English**: the tutor's "Hello! Let's practise
  this lesson together." in a German session comes from
  `data/test-fixtures/llm/tutor-turn.json`, not from a template. Not an i18n
  bug. Docs pages (`/docs/*`), the Terms and the AI prompt under
  "Advanced" are English by design too.
- **Lesson titles, summaries and category labels ("Vocabulary",
  "Grammar") are course content** and stay English in every app language;
  strip them (the lesson JSON `title`/`summary`/`focus` fields) before
  grepping a German dump for English, or the Sprint ladder and the map
  drown the real hits.
