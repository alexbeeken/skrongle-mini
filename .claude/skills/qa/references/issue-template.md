# Issue format for a QA finding

One finding per issue. Title and body in plain words, no jargon the user
has to look up. The user grooms these one by one the next morning, so the
body must let them see the problem in under a minute.

```
gh issue create \
  --title "[qa] Phone: conjugation tables run off the screen on 29 lesson pages" \
  --label "needs grooming" --label bug \
  --body-file $S/issue-NNN.md
```

Body, at most four paragraphs, each starting with a bold lead:

```
**What is wrong.** At phone width (390 px) the lesson page is up to 377 px
wider than the screen on 29 of the 100 Spanish lessons. The offenders are
the paradigm tables and the markdown tables inside the lesson content. The
whole page scrolls sideways, and the top bar moves with it.

**How to see it.** `./.dev-start --reset --seed --background`, open
`/dev/login?open=spanish&next=/lessons/ser-vs-estar` in a 390 px wide
viewport (Chrome device mode, iPhone 13). The conjugation table runs 230 px
off the right edge. Same on `/lessons/proofreading` (377 px) and
`/lessons/imperfect` (319 px).

**What I expected.** The table fits the screen, or scrolls inside its own
box while the page stays put.

**Evidence.** Found by the `classroom` explore persona
(`e2e/results/explore/classroom-20261020-013412/report.md`, finding 1) and
confirmed on all 100 lessons with a Playwright sweep.

![tables-1](https://github.com/alexbeeken/pooki/blob/qa-screenshots/2026-10-20-phone-tables/tables-1.png?raw=true)
![tables-2](https://github.com/alexbeeken/pooki/blob/qa-screenshots/2026-10-20-phone-tables/tables-2.png?raw=true)
```

Rules:

- `[qa]` first in the title, then where and what. "Phone:" or
  "Study mode:" as a prefix when the place matters. `[qa]` says a QA sweep
  found it; the label, not the title, is what keeps it waiting for grooming.
- The `needs grooming` label always; `bug` when something is broken, not
  for a rough edge; `accessibility` when it is one.
- Steps start from how to run it and end at what you see. One action per
  sentence.
- Numbers only when they help reproduce it (a width, a lesson count).
- Name the script, persona or mode that found it and the report path, so
  the finding can be replayed. Do not paste the whole report.
- A bug gets two or three still screenshots at the end of the body (no
  video), each showing the problem in view. `scripts/issue-shots.sh` puts
  them on the `qa-screenshots` branch and prints the `![…](…)` lines.
  A rough edge gets one when it helps.
- Never commit screenshots, `e2e/results/` or `e2e.db` to a working branch.
- A content fault names the file and the field
  (`data/courses/spanish/lessons/a1/ser-vs-estar.json`, `quiz[3]`) and the
  fix, the way `scripts/fact-check.sh` prints it.
