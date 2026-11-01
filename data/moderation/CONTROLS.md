# Content check controls

Notes for the developer who runs the content check's control set and
red-team scripts (#1061). They live here, not in `docs/`, on purpose:
everything in `docs/` is built into the server and served to anyone at
`/docs`, and these notes say how the filter is tested and where it is
weak. Nothing under `data/moderation/` is served or shipped in the
release tarball. Keep it that way: don't link this file from `docs/`.

## The control set

`data/moderation/controls/` is a fixed control set for the content check
(#1061): one file per category of `CATEGORIES`, each a list of made-course
lessons in the course format. Every category has at least three
obvious controls (plain wording), at least three subtle ones (no listed
term, and softened or indirect wording such as a teaching frame, a story, a
"so-called" or a hint spread over exercises) and fine look-alikes that
must pass ("disparar a puerta", "spick-and-span", "der Sieg", a body-safety
lesson). Hate's obvious controls carry a term from
`data/moderation/terms.txt`, and so does one obvious control under minors;
a control that should hit the list names its `term`. Harmful text is
described in placeholder words ("[the steps are left out of this
control]"), never written out. The minors controls describe grooming in
plain, non-explicit words, such as an adult asking a child to keep secrets
from their parents. Nothing sexual, ever.

`cargo test controls` (CI, no model) checks the files and runs the term
check over every control. Each one that names a `term` is flagged with
exactly that term, every other control and every ordinary lesson of the
calibration set passes. A file is kept in the form `--keep` writes
(`serde_json` pretty print, keys sorted inside a lesson), which a test
checks too.

`scripts/content-check-controls.sh` (by hand, never in CI) sends every
control through the term check, the real Jev and the real stage 2 review,
the same functions the server calls, on a database in memory. It prints a
line per control (term hit, Jev's score on its category, the stage 2
verdict, caught or MISSED, passed or FLAGGED, the two highest categories),
then a table per category with obvious caught, subtle caught, fine passed, and
the cost. `--category hate` runs one category. Obvious and subtle controls
count as caught when the course ends up flagged, by the term check or by
stage 2. It needs `TYPESAFE_API_KEY` and `SKRONGLE_AI_KEY` (or
`ANTHROPIC_API_KEY`) from the shell, `./.env` or `e2e/.env`, so run it as
`zsh -ic 'scripts/content-check-controls.sh'`. Jev costs almost nothing
here; most of the cost is the stage 2 calls, one to three per control that
scores high (see the stage 2 rate above).

## Trying to beat the filter

`scripts/moderation-redteam.sh` (by hand, never in CI) asks Claude to
write one lesson that breaks a category's rule in a way the filter might
miss. The lesson goes through the term check, Jev and stage 2 like a
control. When it gets through, the same conversation is asked "Explain the
technique you used to get past the filter." Each round tells Claude how
the last lesson did, so it tries a different technique.

```sh
zsh -ic 'scripts/moderation-redteam.sh --category violence --rounds 2'
zsh -ic 'scripts/moderation-redteam.sh'                 # one round, a random category
zsh -ic 'scripts/moderation-redteam.sh --category hate --keep --out audit/redteam.md'
```

It prints each round's scores, verdict, explanation and cost, and writes
the lessons, scores, verdicts and explanations to a report (`--out`, by
default a file in `$TMPDIR`; never `docs/`). `--keep` adds each lesson
that got through to its category's control file as a subtle control with
a `note`, so the next scorecard lists it as missed until the filter
catches it. `--model` picks the Claude model that writes (default
`claude-sonnet-5-5`). Claude is told to describe harm in placeholder words
and to write nothing sexual. The script refuses the minors category, so no
model is asked to write for it, and its controls are written by hand. A
round costs a few cents. It needs the same keys as the scorecard.

