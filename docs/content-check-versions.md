# Content check versions

The content check of made courses has two sets of rules, each with its own
version number. Every check saves the version it ran under, so the courses
checked under older rules can be found and checked again. This is for the
people who run the hosted site; Skrongle mini has no content check.

- **Terms** (`TERMS_VERSION` in `src/course_flags/terms.rs`): the term list,
  `data/moderation/terms.txt`. Saved on each course's flag row
  (`course_flags.terms_version`). When it goes up, the next nightly run
  checks every made course against the list again. That is plain string
  work, so there is no cap.
- **AI** (`AI_CHECK_VERSION` in `src/content_check.rs`): the Jev questions
  and their thresholds (stage 1) and the review prompt (stage 2), the blocks
  between `// moderation-criteria(ai)` markers. Saved on each part's score
  (`content_scores.ai_version`) and on the flag row when the AI wrote the
  verdict (`course_flags.ai_version`). A stored score counts only at the
  current version, so a part scored under an older one goes to Jev again
  the next time its course is scanned. A course nobody edits is scanned
  again once its AI check is 3 or more versions behind (#1040): the
  nightly run rechecks it within its cap, oldest check first.
  `SKRONGLE_CONTENT_CHECK_RECHECK_BEHIND` changes the 3.

They are separate because a term recheck is free and an AI recheck costs
money: a new term list shouldn't make every course go to Jev again.

The superuser course table shows each course's versions under **Rules**,
and highlights the ones older than the current rules. Its filter "AI
check 3 or more versions behind" lists the courses waiting for a recheck
and says how many there are.

## Bumping a version

Add 1 for a change that could flip a verdict: a term added or taken out, a
change to how terms match, a new or reworded question, a new threshold, a
new prompt. A typo or a comment needs no bump. Then add a line to the table
below with the date and what changed.

CI checks it (`scripts/moderation-version-lint.sh`): a PR that changes the
term list or a `moderation-criteria(ai)` block fails unless it raises the
matching version, or its body says `moderation-version: unchanged`. A change
to the matching code in `src/course_flags/terms.rs` is not caught by the
lint; bump `TERMS_VERSION` by hand when it could change what matches.
`cargo test` checks that the table has a line for each current version.

## Changelog

| Check | Version | Date | What changed |
|-------|---------|------|--------------|
| Terms | 1 | 2026-10-27 | The first term list: slurs and hate phrases (#1033). |
| AI | 1 | 2026-10-27 | Jev asks two questions (Terms of Use, self-harm), threshold 0.5 (#1036); the first review prompt (#1037). |
| AI | 2 | 2026-10-27 | Eleven categories instead of two questions, each with its own threshold (all 0.5): misinformation, violence, self-harm, weapons, firearms, hate speech, mass tragedies, adult content, minors, sexual crimes, Terms of Use. The review prompt names them (#1046). |
