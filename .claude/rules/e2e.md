---
paths:
  - "e2e/**"
  - "scripts/e2e*.sh"
  - "scripts/jev-*.sh"
  - "templates/**"
  - "src/ai.rs"
  - "docs/testing.md"
---

# Browser tests and Jev

`docs/testing.md` covers dev login, answering LLM requests by hand
(`--llm capture`, Manual replies on `/dev/ai`) or from `data/test-fixtures/llm/`, and the
`data-testid` convention, and "Browser tests" covers `e2e/`: exact specs,
and the journeys list (`e2e/journeys.ts`) that Jev navigates. Add a journey
for a new essential flow. A new model call site passes an `ai::Call` with
its own purpose and gets a fixture file; new markup gets testids.

```sh
scripts/e2e.sh specs/smoke   # just the smoke specs: a new learner passes lessons in every mode, and the whole test course
scripts/e2e-record.sh --start <path> --goal "…" --id <id>   # Jev walks a goal once and drafts a smoke spec to review
scripts/e2e-jev.sh [id…] # just the Jev journeys and judged specs (needs the key in ./.env or e2e/.env)
scripts/e2e-explore.sh <profile>   # Jev explores as a user profile (e2e/personas/), reports bugs + proposed tests
scripts/jev-playthrough-all.sh     # Jev plays the whole course in Sprint, Study and Play as a new account: bugs, broken progress, lessons it can't pass (also -sprint/-study/-play)
scripts/jev-playthrough-test-course.sh   # the same for the test course (TEST_COURSE=1)
```
