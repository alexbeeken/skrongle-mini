---
paths:
  - "data/**"
  - "src/content.rs"
  - "docs/course-format.md"
  - "docs/course.schema.json"
---

# Course content and the course format

- `docs/course-format.md` is the contract between content and code **and**
  the text given to LLMs that generate courses (`src/ai.rs` embeds it). If
  you add or rename a lesson field in `src/content.rs`, update this page and
  `docs/course.schema.json` in the same change, and keep the old spelling as
  a `#[serde(alias)]`.
- **The test course** (`data/test-courses/test-course/`, offered only with
  `TEST_COURSE=1`) uses every feature a course file can switch on, one
  lesson per feature, for browser tests. Adding a feature course files can
  use (a field, a quiz type, an exercise, a Guide step, a game reading
  course data, a theme, an alias) MUST add it to the test course and to the
  e2e specs that use it, in the same change. `cargo test test_course` fails
  until the course covers the schema, aliases, quiz types, exercise kinds and
  themes; its `AUTHORING.md` has the feature map and what the tests can't see.
- One course = `course.json` + `lessons/**/*.json`. Built-in courses are in
  `data/courses/<id>/` (folder name = manifest id). User-made courses are the
  same files stored in SQLite (`courses`, `course_files`). Both load through
  `ContentStore::from_files`; validation errors must name the file and field.
- Tests: `cargo test`. Content tests load every course in `data/courses/`
  (Spanish and French) and the test course; keep them all valid. Each course folder
  may carry an `AUTHORING.md` brief with its syllabus and quality bar.
  Fact-check content with `scripts/fact-check.sh` (Jev, cached, cents for
  every course) instead of auditing it with an LLM. An ERROR comes with
  evidence (usually the fix); a CHECK is a doubt to verify. A finding that
  is right after all goes in `data/courses/<id>/fact-check-ok.txt`. After
  changing the questions or `JEV_MODEL`, rerun `--calibrate`.
- `docs/testing.md` "Plagiarism check" — `scripts/plagiarism-check.sh`: briefs may
  not tell an agent to imitate a product; run it after changing course content.

```sh
scripts/fact-check.sh [course…]    # after content changes: Jev judges every lesson item, prints what is likely wrong
scripts/plagiarism-check.sh [course…]   # after content changes: briefs lint, shingle overlap, Jev recognises published text (--prefilter-only needs no key)
```
