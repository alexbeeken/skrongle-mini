# Questions for the user: the decisions folder

The factory runs with nobody watching. A question only the user can answer
goes to the Review Desk's Decisions mode (#1163), never into a report alone
and never into a "stopped, waiting for the user". The user answers it there,
and the desk turns the answers into one prompt for an agent, which acts on
each answer (build it, change the PR, close it, label it).

Every factory skill asks the same way, with one script:

```sh
$BASE/scripts/review-desk/ask.py --issue <N> | --pr <N> \
  --question "<one question, plain words>" --asked-by <skill> \
  [--option "<A>" --option "<B>" ...] [--recommended "<A>"] \
  [--context "<markdown>" | --context-file <file>] [--slug <word-word>] [--title "<issue or PR title>"]
```

It writes one JSON file per question in `~/skrongle-workflow/decisions/`
(`$REVIEW_DESK_HOME/decisions/` when that is set), named by the question's
id: `issue-<N>-<slug>.json` or `pr-<N>-<slug>.json`. One file each means
two sessions can write at the same time. It prints:

- `wrote <id>`: the question is on the desk.
- `open <id>`: the same question is already waiting for the user (or
  answered but not yet sent). Nothing written. Don't ask it again in other
  words either: `ask.py list <N>` shows every question on a number.
- `dismissed <id>`: the user dropped that question in the last 30 days.
  Nothing written. The answer is "not now"; act as if they said so.

A question the user answered and sent frees its id, so the same question
can be asked again later if things change (the old file is kept under a
dated name).

The desk shows the file as a card with the question, the options (the
recommended one marked), the context and the number it unblocks:

```json
{
  "issue": 1163,
  "title": "A decision mode on the review desk app",
  "question": "Should the desk mark a question sent when you copy the prompt?",
  "options": ["Mark it sent on Copy", "Keep it until I remove it"],
  "recommended": "Mark it sent on Copy",
  "context": "Markdown. The two readings, what each one leads to, and links.",
  "asked_by": "backlog-sweep",
  "asked_at": "2026-10-29T08:00:00-07:00"
}
```

The desk adds `answer`, `sent_at` and `dismissed_at`; never write those. A
file with `sent_at` or `dismissed_at` no longer shows on the desk. Leave it;
it is the record of what was decided.

## What makes a good question

- **One question per file**, answerable without opening GitHub. Put what
  the user needs to decide in `--context` (the two readings, what each one
  leads to, the evidence, a link), not in the question.
- **Options whenever there is a choice.** Two to four short answers, and
  `--recommended` set to the one you would pick. The user can always write
  their own answer as well; a question with no options is for "which
  server?", "what does Basic give?", where the answer is a fact only they
  have.
- **Say what each answer leads to**, so the agent that gets the answer
  can act without coming back: "Close it" closes the issue; "Build it as
  written" opens a PR in the next sweep; "Change it to: …" means the user
  writes the new reading in the text box.
- **A bad idea is said plainly.** When you think the issue is wrong (it
  breaks a rule in `CLAUDE.md`, works against the product focus, or would
  make the app worse for a learner), say so in the question in one or two
  plain sentences and recommend closing or changing it. Don't build it to
  be polite, and don't skip it in silence.
- **Plain words, short sentences**, the unslop rules
  (`~/.claude/skills/unslop/SKILL.md`). The user reads these one after
  another on the desk.

## Fixed slugs for the recurring kinds

A fixed slug lets `ask.py` recognise the same question across sweeps, so a
dismissed one stays quiet for 30 days and an open one isn't duplicated:

| Slug | Asked by | When |
| --- | --- | --- |
| `looks-finished` | backlog-sweep | a merged PR or a commit on main did the work, and the issue is still open |
| `next-step` | backlog-sweep | the issue needs the user (exploration, legal, hardware, umbrella, vague); the question offers the smallest useful next step |
| `still-wanted` | backlog-sweep | a `blocked`, `on hold` or `[needs grooming]` issue with no movement in 30 days |
| `bad-idea` | backlog-sweep, a builder | the issue reads as wrong; the question says why |
| `which-reading` | a builder | the issue can be read two ways that lead to different work |
| `base-not-approved` | mergeme | a `mergeme` PR sits on a PR that isn't marked mergeme |
| `same-thing-twice` | mergeme, pr-qa-demo | two PRs build the same thing in different ways |
| `design-choice` | pr-qa-demo, pr-fix | the verdict or the fix hinges on a product choice |

Any other question gets a slug of its own (`--slug`), or one made from the
question's first words.

## The cap

A backlog sweep writes at most 8 new questions per run, oldest issue first,
and says how many are waiting. Eight is about what the user answers in one
sitting; forty at once would be ignored. The other skills ask only what
their run turned up, which is rarely more than one or two.

## In a cloud session

The folder isn't there (it is on the Mac). Put each question in your
report in the same shape (number, question, options, recommendation), so
the user or the next Mac session can write it.
