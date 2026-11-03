# Documentation

Skrongle is a learning app with lessons, quizzes, phone sessions and games,
all driven by JSON course files.

## Guides

- [Getting started](getting-started): what the app is, the five ways to
  study (Sprint, Study, Play, Guide, AI tutoring) and when a lesson counts as passed.
- [How a course is built](course-structure): a course folder, `course.json`,
  lesson files and which field switches on which feature, with examples
  from the Spanish course.
- [Building and generating courses](making-courses): the editor, adding
  files, generating a course with AI, sharing a course, fixing validation
  errors.
- [FAQ](faq) — API keys, missing games, sharing, AI agents.

## Reference

- [Course format](course-format): every field and rule of `course.json` and
  lesson files. Give it to an LLM and it can write a course. Also as
  [raw Markdown](course-format.md) and a [JSON Schema](course.schema.json).
- [Features](features): what each part of the app does and which lesson
  fields it reads: Lessons, Quizzes, Guide, Sprint mode, Games, Map, Vocabulary,
  History, Skrongle AI.
- [Progress across modes](progress): the pass rule, and the API (and brief)
  for AI agents that teach the learner.
- [Languages (i18n)](i18n): the app's own text in the learner's language:
  one `i18n/<code>.yaml` per language, the pickers, how code reads it, and
  how a course gets translated (a translated copy).
- [Data inventory](data-inventory): what is stored about a learner, where,
  why, what leaves the server and for how long. The
  [privacy policy](privacy) (served at `/privacy`) is written from it.

## For LLMs

[`/llms.txt`](/llms.txt) lists the pages an LLM needs to write a course.
Every page here is also raw Markdown. Add `.md` to its address.

## For developers

Working on the code? See [Developer documentation](dev): architecture,
testing, QA and the security review.
