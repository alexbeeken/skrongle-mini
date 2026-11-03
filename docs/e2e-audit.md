# E2E coverage audit

What the browser automation in `e2e/` walks through, set against every
production route and `data-testid` hook, and what to do about the gaps.
Refreshed 2026-10-22 and re-counted at the merge of main on 2026-10-23 (the
first snapshot was PR #117, 2026-10-17, hand-written: `git show
c955cd1e:docs/e2e-audit.md`). How the suite works:
[Testing locally](testing#browser-tests).

The numbers below the line are generated: `python3 scripts/e2e-audit.py
--write` refreshes them in place. Re-run it before acting on a count.

## Findings

- **33 % of the keyless tests never run in CI.** The exact-spec job in
  `.github/workflows/smoke.yml` passes a fixed list of 18 files
  (`specs: specs/exact.spec.ts …`); the smoke job passes the folder
  `specs/smoke`. A spec file not on the list runs only when someone runs
  `scripts/e2e.sh` by hand. Missing today, 15 files and 79 tests, none of
  them needing a key: `ai-off`, `avatar-colour`, `course-report`,
  `dev-payments`, `editor-errors`, `home`, `hosted-ai`, `lessons`,
  `profile-autosave`, `prompt-limits`, `review-ai-changes`,
  `review-and-revise`, `revise`, `share`, `subscription`. That is most of the
  course editor (lessons, revise, review), the home page, sharing,
  profile autosave and the paid plans. `create` joined the list in #422,
  `docs` and `course-copy` since, one file at a time: new spec files keep
  landing outside it. `CLAUDE.md` says "the exact and smoke e2e specs
  run on every PR", which reads as all of them.
- **The learner's paths are covered, in CI.** The smoke specs play every
  mode, question type, Sprint exercise kind and game on the test course,
  right and wrong, as a fresh account, and check `/api/progress` after;
  Spanish lesson 1 and French get the same in each mode.
- **Nothing with a model in the loop runs unattended.** The 14 Jev journeys,
  2 judged specs, 4 personas (`scripts/e2e-explore.sh`) and the course
  playthroughs need `TYPESAFE_API_KEY` and no workflow has a `schedule`.
- **Routes.** Of 137 production routes, 3 are reached by no flow:
  `POST /login/code/resend`, `POST /login/code/cancel`,
  `POST /profile/two-factor/code`. The script lists five more that are
  reached in ways a grep cannot see: `GET /profile/export/download` (the link in the
  export e-mail, in `smoke/export.spec.ts`), `/courses/shared/{token}` (from the invitation
  e-mail, in `share.spec.ts`, local only), `POST /api/courses/{id}/lessons`
  (the editor's autosave, the URL built by concatenation in
  `static/courses.js`), `/pictures/{*path}` (a course's SVG pictures),
  `/play/conjugation/play` (`drill-setup-start`).
- **Testids.** 69 % of the hooks are referenced. The learner-facing gaps
  are the settings of Numbers by ear, Listening and Date
  Dash (rate, range, replay, reveal), the tutor's provider picker and
  no-key state, and the Matching leaderboard. Most of the rest are `-form`
  wrappers, dialogs' close buttons and the dev tools.

## Flows by area

The per-test list is below the line. In short:

| Area | Specs (CI unless marked) |
|---|---|
| Sign-up, sign-in, e-mail, 2FA | `smoke/verify-email`, `mail`, `two-factor`, `authenticator`, `terms`, `i18n` |
| Learning, every mode and game | `smoke/happy-paths`, `smoke/complete-test-course`, `smoke/test-course-games`, `smoke/wrong-answers`, `smoke/wrong-answers-games`, `smoke/start-level`, `smoke/french`, `smoke/daily-goal`, `smoke/motivator*`, `modals`, `lesson-tables` |
| Home page | `home` (local only) |
| Profile | `profile-autosave`, `avatar-colour`, `ai-off` (all local only); `profile-nav`; `smoke/privacy`, `smoke/export` (export, delete) |
| Plans and payments | `subscription`, `dev-payments`, `hosted-ai` (all local only) |
| Generating a course | `exact`, `pathways`, `dev-ai`, `create`; `prompt-limits` (local only) |
| Editing a course | `editor-autosave`, `forms`, `course-copy`; `lessons`, `editor-errors`, `course-report`, `revise`, `review-ai-changes`, `review-and-revise` (local only) |
| Sharing | `share` (local only) |
| Docs, self-hosting, layout | `smoke/self-host`, `smoke/phone-width`, `docs` |
| Jev, with a key | `journeys` (14 goals from `e2e/journeys.ts`), `judged` (2), `explore` (a persona from `e2e/personas/`), `record` (drafts a spec) |

Journeys (`e2e/journeys.ts`): open a lesson, find the generate form, start
Sprint on a phone (impatient), find "ser vs estar" (classroom), visit every
nav section, share a course, add a lesson with AI, leave an AI lesson out in
the review, edit a lesson as a form, add a lesson to a unit by hand, name a
new course, turn off AI features, start at A2, change the level.

Personas (`e2e/personas/`): `accurate` (reads everything, Study mode,
patience 20), `classroom` (wants one topic now, 10), `impatient` (a
teenager on a phone in Sprint, 6), `testing-bot` (sees only markup, 12).

## Recommendations

1. **Run every keyless spec in CI.** Pass `specs` (the whole folder) to the
   exact-spec job instead of a list; the Jev specs already skip themselves
   without a key (`--grep-invert @jev` stays). Expect some of the 79 to
   fail at first: they have not run since they were written. Fix or delete
   each, then the list can never go stale again.
2. **Make the docs say what CI runs.** One line in `CLAUDE.md` and
   `docs/testing.md`: CI runs every file under `e2e/specs/` except the Jev
   ones (after 1), or, until then, only the files listed in `smoke.yml`.
3. **Cover the three unreached routes** with exact specs: resend and
   cancel on `/login/code`, and the profile's two-factor code change.
4. **Spec the learner surfaces with no testid referenced**: home submenu
   links, game settings (rate, range, replay), tutor without a key, the
   Matching leaderboard.
5. **Schedule the model-driven checks**: a nightly workflow that runs
   `scripts/e2e-jev.sh` and one persona when `TYPESAFE_API_KEY` is set,
   reporting, not gating.

## Generated numbers

<!-- e2e-audit:generated -->
<!-- generated by scripts/e2e-audit.py at f2279d0 -->

| What | Count |
|---|---|
| Spec files | 95 (79 run in CI) |
| Tests | 428: 341 run in CI, 76 keyless but local only, 11 need a Jev key |
| Production routes (`src/routes/mod.rs`, no `/dev`) | 144: 94 named by an e2e file, 41 more reached through a page, 9 not reached |
| `data-testid` hooks (templates, static JS) | 1326, 1072 referenced (80 %) |
| Jev journeys / personas | 14 / 4 |

### Spec files

| File | Tests | CI | Needs a key |
|---|---|---|---|
| `specs/accessibility.spec.ts` | 7 | yes |  |
| `specs/advice-badge.spec.ts` | 1 | yes |  |
| `specs/ai-examples.spec.ts` | 3 | yes |  |
| `specs/ai-off.spec.ts` | 2 | **no** |  |
| `specs/algebra-tiles.spec.ts` | 4 | yes |  |
| `specs/authenticator.spec.ts` | 4 | yes |  |
| `specs/chord-spell.spec.ts` | 5 | yes |  |
| `specs/clock.spec.ts` | 3 | yes |  |
| `specs/consent.spec.ts` | 2 | yes |  |
| `specs/course-card-stats.spec.ts` | 3 | yes |  |
| `specs/course-copy.spec.ts` | 5 | yes |  |
| `specs/course-lints.spec.ts` | 2 | yes |  |
| `specs/course-report.spec.ts` | 3 | **no** |  |
| `specs/course-translate.spec.ts` | 2 | yes |  |
| `specs/create.spec.ts` | 15 | yes |  |
| `specs/dev-ai.spec.ts` | 11 | yes |  |
| `specs/docs.spec.ts` | 10 | yes |  |
| `specs/editor-autosave.spec.ts` | 5 | yes |  |
| `specs/editor-errors.spec.ts` | 4 | **no** |  |
| `specs/editor-fixes.spec.ts` | 3 | **no** |  |
| `specs/exact.spec.ts` | 34 | yes |  |
| `specs/explore.spec.ts` | 3 | **no** | yes |
| `specs/fact-check.spec.ts` | 1 | yes |  |
| `specs/fill-bar.spec.ts` | 5 | yes |  |
| `specs/forms.spec.ts` | 4 | yes |  |
| `specs/home.spec.ts` | 4 | **no** |  |
| `specs/i18n.spec.ts` | 3 | yes |  |
| `specs/journeys.spec.ts` | 2 | **no** | yes |
| `specs/judged.spec.ts` | 3 | **no** | yes |
| `specs/keyboard.spec.ts` | 4 | yes |  |
| `specs/landing-try.spec.ts` | 4 | yes |  |
| `specs/lesson-ai-edit.spec.ts` | 5 | yes |  |
| `specs/lesson-conversations.spec.ts` | 2 | yes |  |
| `specs/lesson-duplicate.spec.ts` | 1 | yes |  |
| `specs/lesson-editor-rules.spec.ts` | 10 | yes |  |
| `specs/lesson-editor.spec.ts` | 13 | yes |  |
| `specs/lesson-header.spec.ts` | 1 | yes |  |
| `specs/lesson-tables.spec.ts` | 2 | yes |  |
| `specs/lessons.spec.ts` | 17 | **no** |  |
| `specs/mail.spec.ts` | 6 | yes |  |
| `specs/measure.spec.ts` | 2 | yes |  |
| `specs/mistakes.spec.ts` | 1 | yes |  |
| `specs/modals.spec.ts` | 5 | yes |  |
| `specs/music-sound.spec.ts` | 10 | yes |  |
| `specs/notation.spec.ts` | 14 | yes |  |
| `specs/owner-resets.spec.ts` | 3 | yes |  |
| `specs/pathways.spec.ts` | 7 | yes |  |
| `specs/privacy.spec.ts` | 3 | yes |  |
| `specs/profile-autosave.spec.ts` | 7 | **no** |  |
| `specs/profile-nav.spec.ts` | 1 | yes |  |
| `specs/prompt-limits.spec.ts` | 5 | **no** |  |
| `specs/record.spec.ts` | 3 | **no** | yes |
| `specs/review-ai-changes.spec.ts` | 14 | **no** |  |
| `specs/review-and-revise.spec.ts` | 4 | **no** |  |
| `specs/revise.spec.ts` | 11 | **no** |  |
| `specs/scale-ladder.spec.ts` | 3 | yes |  |
| `specs/share-edges.spec.ts` | 8 | yes |  |
| `specs/share-removed.spec.ts` | 1 | yes |  |
| `specs/share-sign-in.spec.ts` | 2 | yes |  |
| `specs/share.spec.ts` | 2 | **no** |  |
| `specs/smoke/complete-test-course.spec.ts` | 3 | yes |  |
| `specs/smoke/daily-goal.spec.ts` | 1 | yes |  |
| `specs/smoke/drill-first-set.spec.ts` | 2 | yes |  |
| `specs/smoke/export.spec.ts` | 2 | yes |  |
| `specs/smoke/french.spec.ts` | 5 | yes |  |
| `specs/smoke/happy-paths.spec.ts` | 10 | yes |  |
| `specs/smoke/mini.spec.ts` | 6 | yes |  |
| `specs/smoke/motivator-reward.spec.ts` | 1 | yes |  |
| `specs/smoke/motivator-skc.spec.ts` | 1 | yes |  |
| `specs/smoke/motivator-xp.spec.ts` | 1 | yes |  |
| `specs/smoke/motivator.spec.ts` | 1 | yes |  |
| `specs/smoke/password-reset.spec.ts` | 2 | yes |  |
| `specs/smoke/phone-width.spec.ts` | 2 | yes |  |
| `specs/smoke/placement.spec.ts` | 2 | yes |  |
| `specs/smoke/practice-empty-check.spec.ts` | 1 | yes |  |
| `specs/smoke/practice-keys.spec.ts` | 2 | yes |  |
| `specs/smoke/practice-nothing-yet.spec.ts` | 2 | yes |  |
| `specs/smoke/practice-result-order.spec.ts` | 1 | yes |  |
| `specs/smoke/practice-summary.spec.ts` | 2 | yes |  |
| `specs/smoke/privacy.spec.ts` | 3 | yes |  |
| `specs/smoke/self-host.spec.ts` | 4 | yes |  |
| `specs/smoke/skc-shop.spec.ts` | 2 | yes |  |
| `specs/smoke/sprint-check-error.spec.ts` | 2 | yes |  |
| `specs/smoke/sprint-ladder.spec.ts` | 2 | yes |  |
| `specs/smoke/sprint-sort.spec.ts` | 2 | yes |  |
| `specs/smoke/start-level.spec.ts` | 4 | yes |  |
| `specs/smoke/test-course-games.spec.ts` | 6 | yes |  |
| `specs/smoke/verify-email.spec.ts` | 3 | yes |  |
| `specs/smoke/wrong-answers-games.spec.ts` | 5 | yes |  |
| `specs/smoke/wrong-answers.spec.ts` | 7 | yes |  |
| `specs/staff-note.spec.ts` | 5 | yes |  |
| `specs/terms.spec.ts` | 5 | yes |  |
| `specs/two-factor.spec.ts` | 2 | yes |  |
| `specs/word-list.spec.ts` | 3 | yes |  |
| `specs/words-due.spec.ts` | 2 | yes |  |

### Routes reached only through a page

No e2e file names them; a spec drives the form, link (its testid) or script that calls them.

- `POST /api/practice/finish`: `static/quiz.js`
- `POST /api/listening/check`: `static/listening.js`
- `POST /api/listening/finish`: `static/listening.js`
- `POST /courses/{id}/file/delete`: `course-edit-file-delete-form`
- `POST /courses/{id}/cancel`: `course-ai-modal-cancel-form`, `course-edit-cancel-form`, `courses-cancel`
- `POST /courses/{id}/undo`: `course-undo-form`
- `POST /courses/{id}/redo`: `course-undo-form`
- `POST /courses/{id}/share/remove`: `course-share-remove-form`
- `GET /cookies`: `footer-cookies`
- `GET POST /terms/accept`: `terms-accept-check`
- `GET POST /password/change`: `password-change-form`
- `POST /api/sprint/check`: `static/sprint.js`
- `POST /api/sprint/finish`: `static/sprint.js`
- `POST /api/guide/step`: `static/guide.js`
- `POST /api/guide/restart`: `static/guide.js`
- `POST /api/games/matching/start`: `static/matching.js`
- `POST /api/games/matching/finish`: `static/matching.js`
- `POST /api/games/conjugation/deal`: `static/conjugation.js`
- `POST /api/games/conjugation/check`: `static/conjugation.js`
- `POST /api/games/conjugation/finish`: `static/conjugation.js`
- `POST /api/games/numbers/finish`: `static/numbers.js`
- `POST /api/games/dates/finish`: `static/dates.js`
- `POST /api/vocabulary/review`: `static/vocab.js`
- `GET /api/vocabulary/due`: `static/vocab.js`
- `GET POST /login/code`: `login-code-input`
- `POST /login/code/resend`: `login-code-cancel`
- `POST /login/code/cancel`: `login-code-cancel`
- `POST /account/deletion/cancel`: `account-deletion-cancel`
- `POST /profile/password`: `profile-password-form`
- `POST /profile/sessions/end-others`: `profile-sign-out-others-form`
- `POST /profile/api-keys/remove`: `profile-api-key-remove-form`
- `POST /profile/ai`: `static/profile.js`, `consent-ai-form`
- `POST /profile/export`: `profile-export-form`
- `GET POST /profile/export/download`: `profile-export-download-form`
- `POST /profile/delete`: `profile-delete-form`
- `POST /profile/verify-email/resend`: `verify-email-resend`
- `POST /profile/two-factor/totp/confirm`: `two-factor-totp-code`
- `POST /superuser/invites`: `superuser-invite-form`
- `POST /superuser/invites/revoke`: `superuser-invite-revoke`
- `POST /superuser/terms-announce`: `superuser-terms-form`
- `POST /api/lesson-check`: `static/lesson-check.js`

### Routes no e2e flow reaches

Not named by an e2e file, and no driven form, link or script calls them.

- `POST /api/courses/{id}/preview-answer`
- `POST /api/courses/{id}/answer-key`
- `GET /play/conjugation/play`: called from `templates/conjugation_setup.html`
- `GET /pictures/{*path}`
- `GET /badges/{file}`
- `GET /account/deletion`
- `POST /profile/two-factor/code`: called from `templates/profile.html`
- `GET /static/{*path}`
- `GET /manifest.webmanifest`: called from `templates/base.html`, `templates/sprint.html`, `templates/sprint_play.html`, `templates/tutor.html`

### Testids per template

| File | Hooks | Referenced | Not referenced |
|---|---|---|---|
| `templates/dev_ai.html` | 42 | 14 | `dev-ai-base`, `dev-ai-clear`, `dev-ai-clear-inject`, `dev-ai-detail`, `dev-ai-fixture`, `dev-ai-fixture-edited`, `dev-ai-fixture-open`, `dev-ai-fixtures`, `dev-ai-fixtures-info`, `dev-ai-fixtures-info-dialog`, `dev-ai-hold-info`, `dev-ai-hold-info-dialog` (+16) |
| `templates/numbers.html` | 23 | 7 | `numbers-again`, `numbers-card`, `numbers-form`, `numbers-max`, `numbers-min`, `numbers-novoice`, `numbers-range`, `numbers-rate`, `numbers-replay`, `numbers-reveal`, `numbers-settings`, `numbers-setup` (+4) |
| `templates/listening_play.html` | 20 | 7 | `listen`, `listen-empty`, `listen-form`, `listen-novoice`, `listen-rate`, `listen-replay`, `listen-reveal`, `listen-setup`, `listen-slow`, `listen-status`, `listen-stop`, `listen-unsupported` (+1) |
| `templates/home.html` | 34 | 22 | `home-ai-blocked`, `home-ai-open`, `home-guide-open`, `home-play-open`, `home-sprint-open`, `home-start-play`, `home-start-sprint`, `home-start-study`, `home-study-open`, `home-tutor-gate`, `home-tutor-gate-add-key`, `home-tutor-gate-close` |
| `templates/profile.html` | 109 | 98 | `email-no-mail-close`, `email-no-mail-dialog`, `email-unverified-close`, `two-factor-confirm-code`, `two-factor-confirm-form`, `two-factor-confirm-method`, `two-factor-confirm-send`, `two-factor-confirm-sent`, `two-factor-confirm-sent-why`, `two-factor-totp-error`, `two-factor-totp-form` |
| `templates/lesson.html` | 54 | 45 | `lesson-check-form`, `lesson-head`, `lesson-note-error`, `lesson-pass`, `lesson-prerequisite`, `lesson-read-form`, `lesson-scale-play`, `lesson-scale-staff`, `quiz-play` |
| `templates/dev_box.html` | 18 | 9 | `dev-box-info`, `dev-box-info-dialog`, `dev-box-off`, `dev-inbox-clear`, `dev-inbox-filter`, `dev-inbox-off`, `dev-inbox-to`, `dev-mail-providers`, `dev-mail-send-form` |
| `templates/dev_inbox_message.html` | 11 | 3 | `dev-inbox-back`, `dev-inbox-headers`, `dev-inbox-link`, `dev-inbox-message`, `dev-inbox-message-archive`, `dev-inbox-message-delete`, `dev-inbox-raw`, `dev-inbox-text` |
| `templates/dev_mail_message.html` | 11 | 3 | `dev-mail-back`, `dev-mail-detail`, `dev-mail-error`, `dev-mail-headers`, `dev-mail-html`, `dev-mail-message-archive`, `dev-mail-message-delete`, `dev-mail-text` |
| `templates/dates.html` | 16 | 9 | `dates-again`, `dates-card`, `dates-ear`, `dates-hud`, `dates-replay`, `dates-score`, `dates-setup` |
| `templates/tutor.html` | 11 | 4 | `tutor-add-key`, `tutor-exit`, `tutor-form`, `tutor-no-key`, `tutor-provider`, `tutor-provider-form`, `tutor-score` |
| `templates/conjugation_setup.html` | 13 | 7 | `drill-setup-count`, `drill-setup-empty`, `drill-setup-form`, `drill-setup-hide`, `drill-setup-locked-lesson`, `drill-setup-locked-list` |
| `templates/matching.html` | 11 | 5 | `matching-again`, `matching-board-area`, `matching-leaderboard`, `matching-leaderboard-row`, `matching-status`, `matching-timer` |
| `templates/dev_ai_held.html` | 9 | 4 | `dev-ai-answer-error`, `dev-ai-answer-form`, `dev-ai-held-empty`, `dev-ai-reply`, `dev-ai-request-state` |
| `templates/dev_ai_call.html` | 8 | 3 | `dev-ai-back`, `dev-ai-call-detail`, `dev-ai-call-headers`, `dev-ai-call-purpose`, `dev-ai-call-result` |
| `templates/dev_jobs.html` | 10 | 5 | `dev-jobs-attempts`, `dev-jobs-empty`, `dev-jobs-row`, `dev-jobs-state`, `dev-jobs-table` |
| `templates/generate_form.html` | 32 | 27 | `generate-depth-choice`, `generate-depth-info`, `generate-depth-info-text`, `generate-instructions`, `generate-kind` |
| `templates/landing.html` | 6 | 1 | `landing-course`, `landing-privacy`, `landing-register`, `landing-terms`, `landing-try` |
| `templates/lookup.html` | 10 | 5 | `lookup-back`, `lookup-collected`, `lookup-hits`, `lookup-more`, `lookup-term` |
| `templates/register.html` | 10 | 5 | `register-dev`, `register-dev-login`, `register-form`, `register-invite`, `register-owner-note` |
| `templates/reset.html` | 13 | 8 | `reset-again`, `reset-ask-form`, `reset-choose-form`, `reset-error`, `reset-login` |
| `templates/dev_authenticator.html` | 5 | 1 | `dev-authenticator-add-form`, `dev-authenticator-info`, `dev-authenticator-info-dialog`, `dev-authenticator-list` |
| `templates/listening.html` | 4 | 0 | `listening-lesson`, `listening-lesson-mastery`, `listening-lesson-play`, `listening-mixed` |
| `templates/login.html` | 12 | 8 | `login-dev`, `login-dev-login`, `login-form`, `login-privacy-link` |
| `templates/dev_ai_fixture.html` | 7 | 4 | `dev-ai-fixture-form`, `dev-ai-fixture-key`, `dev-ai-fixture-save` |
| `templates/conjugation.html` | 11 | 8 | `drill-again`, `drill-again-misses`, `drill-form` |
| `templates/guide.html` | 9 | 6 | `guide-continue`, `guide-stages`, `guide-three-star` |
| `templates/lessons.html` | 9 | 6 | `lessons-bar`, `lessons-list`, `lessons-shown` |
| `templates/login_code.html` | 10 | 7 | `login-code-form`, `login-code-notice`, `login-code-resend` |
| `templates/register_closed.html` | 3 | 0 | `register-closed`, `register-closed-login`, `register-closed-message` |
| `templates/terms.html` | 4 | 1 | `terms-privacy-link`, `terms-raw`, `terms-version` |
| `templates/verify_email.html` | 7 | 4 | `verify-email`, `verify-email-continue`, `verify-email-resend-form` |
| `static/devbox.js` | 14 | 12 | `dev-jobs`, `login-code-device` |
| `templates/dev_ai_rows.html` | 5 | 3 | `dev-ai-call-outcome`, `dev-ai-empty` |
| `templates/docs.html` | 9 | 7 | `docs`, `docs-llms` |
| `templates/games.html` | 3 | 1 | `games-list`, `games-play` |
| `templates/lang_picker.html` | 3 | 1 | `lang-picker-form`, `lang-picker-submit` |
| `static/lesson-note.js` | 4 | 2 | `lesson-note`, `lesson-note-save-error` |
| `templates/login_code_device_rows.html` | 5 | 3 | `login-code-device-code`, `login-code-device-empty` |
| `templates/login_code_device.html` | 2 | 0 | `login-code-device-off`, `login-code-device-to` |
| `templates/map.html` | 5 | 3 | `map-lesson-link`, `map-practice` |
| `templates/matching_pick.html` | 2 | 0 | `matching-board`, `matching-board-play` |
| `templates/terms_accept.html` | 6 | 4 | `terms-accept-error`, `terms-accept-form` |
| `templates/terms_notice.html` | 5 | 3 | `terms-notice-change`, `terms-notice-form` |
| `static/landing.js` | 4 | 2 | `try-clock`, `try-line` |
| `templates/terms_check.html` | 5 | 3 | `{{ tid }}-age-row`, `{{ tid }}-terms-row` |
| `templates/dev_authenticator_rows.html` | 7 | 6 | `dev-authenticator-account` |
| `templates/goal_message.html` | 1 | 0 | `goal-message` |
| `templates/history.html` | 4 | 3 | `history-games-scroll` |
| `static/lessons.js` | 3 | 2 | `lessons-to-top` |
| `static/measure.js` | 2 | 1 | `measure-handle` |
| `templates/mistakes.html` | 9 | 8 | `mistakes-scroll` |
| `static/music-sound.js` | 5 | 4 | `music-chip` |
| `templates/vocabulary.html` | 4 | 3 | `vocabulary-study-all` |
| `templates/vocabulary_lesson.html` | 4 | 3 | `vocabulary-study` |
| `templates/ai_switch.html` | 4 | 3 | `{{ tid }}-no-ai-row` |
| `templates/account_deletion.html` | 6 | 6 |  |
| `templates/ai_form.html` | 15 | 15 |  |
| `static/notation.js` | 2 | 2 |  |
| `static/autosave.js` | 3 | 3 |  |
| `templates/base.html` | 17 | 17 |  |
| `static/chord-spell.js` | 5 | 5 |  |
| `static/sprint.js` | 6 | 6 |  |
| `static/profile.js` | 24 | 24 |  |
| `templates/lessons_panel.html` | 61 | 61 |  |
| `static/courses.js` | 28 | 28 |  |
| `templates/ai_examples.html` | 12 | 12 |  |
| `templates/ai_job_modal.html` | 14 | 14 |  |
| `templates/course_edit.html` | 21 | 21 |  |
| `templates/course_file.html` | 6 | 6 |  |
| `templates/lessons_modes.html` | 1 | 1 |  |
| `static/lesson-editor.js` | 4 | 4 |  |
| `templates/lessons_row.html` | 9 | 9 |  |
| `templates/lessons_ai_dialog.html` | 10 | 10 |  |
| `static/word-list.js` | 10 | 10 |  |
| `templates/course_report.html` | 12 | 12 |  |
| `templates/undo_revision.html` | 5 | 5 |  |
| `templates/course_review_modal.html` | 32 | 32 |  |
| `templates/course_share_modal.html` | 13 | 13 |  |
| `templates/courses.html` | 36 | 36 |  |
| `templates/course_shared_dot.html` | 3 | 3 |  |
| `templates/course_copy.html` | 2 | 2 |  |
| `templates/course_settings.html` | 14 | 14 |  |
| `static/course-settings.js` | 5 | 5 |  |
| `static/placement.js` | 4 | 4 |  |
| `templates/course_wizard.html` | 3 | 3 |  |
| `templates/course_stat.html` | 8 | 8 |  |
| `templates/course_translate.html` | 11 | 11 |  |
| `templates/dev_box_kinds.html` | 3 | 3 |  |
| `templates/dev_box_rows.html` | 6 | 6 |  |
| `templates/error.html` | 5 | 5 |  |
| `static/fill-bar.js` | 6 | 6 |  |
| `templates/vocab_study.html` | 12 | 12 |  |
| `static/keyboard.js` | 2 | 2 |  |
| `static/lesson-check.js` | 1 | 1 |  |
| `templates/lesson_pager.html` | 3 | 3 |  |
| `static/conjugation.js` | 1 | 1 |  |
| `templates/nav_play.html` | 12 | 12 |  |
| `templates/nav_study.html` | 12 | 12 |  |
| `templates/pager.html` | 4 | 4 |  |
| `templates/password_change.html` | 9 | 9 |  |
| `templates/placement_test.html` | 10 | 10 |  |
| `templates/proxy_signin.html` | 4 | 4 |  |
| `templates/quiz.html` | 13 | 13 |  |
| `static/quiz.js` | 2 | 2 |  |
| `templates/register_sent.html` | 2 | 2 |  |
| `static/say.js` | 1 | 1 |  |
| `static/scale-ladder.js` | 1 | 1 |  |
| `templates/self_host.html` | 15 | 15 |  |
| `templates/shop.html` | 10 | 10 |  |
| `templates/sprint.html` | 24 | 24 |  |
| `templates/sprint_play.html` | 13 | 13 |  |
| `static/staff-note.js` | 3 | 3 |  |
| `templates/stage.html` | 12 | 12 |  |
| `static/guide.js` | 1 | 1 |  |
| `templates/superuser.html` | 44 | 44 |  |
| `static/algebra-tiles.js` | 1 | 1 |  |
| `templates/algebra_tiles.html` | 4 | 4 |  |
| `static/landing-kit.js` | 2 | 2 |  |
| `templates/landing_try.html` | 8 | 8 |  |
| `static/tutor.js` | 6 | 6 |  |
| `static/wrong-note.js` | 3 | 3 |  |
| `templates/pick_tabs.html` | 2 | 2 |  |

### Every test, by file

Titles as Playwright lists them (from the source: run `npm ci` in e2e/ for loops expanded).

- **`specs/accessibility.spec.ts`** (7, CI): pages fit a phone at 200 % text size (#342) · the root font size follows the browser's text size (#342) · the system's reduce-motion setting stops animations and transitions (#342) · the first Tab reaches a skip link that jumps to the content (#342) · /sprint has one page heading (#342) · status pills and links in text have 4.5:1 contrast (#342) · inline code keeps its size with the % root font size (#342)
- **`specs/advice-badge.spec.ts`** (1, CI): a question without an explanation puts a yellow ! on its row, and the list opens its field
- **`specs/ai-examples.spec.ts`** (3, CI): a click on an example adds it to the box; a second goes on a new line · a course that teaches no language has no listening or conversation example · at phone width the cards stack and nothing scrolls sideways
- **`specs/ai-off.spec.ts`** (2, local only): the sign-up box hides every AI feature, and the profile turns them back on · the profile's switch is a negative, hides the AI features at once, and asks first when a key would be deleted
- **`specs/algebra-tiles.spec.ts`** (4, CI): practice draws the pile, tiles drag, snap, move with keys and turn · the typed sides are graded: two sides right, the expanded form gets a note · the reading check and the Guide show the board too · a lesson without tiles loads no board
- **`specs/authenticator.spec.ts`** (4, CI): a pasted otpauth link is added, shows the RFC 6238 code with a countdown, copies it, and is removed (#291) · the next code shows by itself when the period runs out, and a link's settings are honoured · a scan link opens the form filled in, a setup key works too, and a bad key says why · when the list goes empty, an empty add box offers the example account again (#291)
- **`specs/chord-spell.spec.ts`** (5, CI): Practice: tap tiles into the slots, take one out, hear about a wrong spelling, and spell the others in any order · a wrong set that isn't the same sounds gets no spelling note · the reading check, Sprint mode and the Guide take the chord in any order · a lesson without a chord-spell question loads no chord-spell script · the lesson editor has the answer and tiles boxes, and its preview plays the question
- **`specs/clock.spec.ts`** (3, CI): Practice: drag the minute hand, use the keys, read a clock · the Guide and Sprint mode show the clock too · the tutor draws a clock to read, with its times to tap
- **`specs/consent.spec.ts`** (2, CI): the consent card shows each consent, and Do Not Show Me AI Features hides them and shows them again across a reload · with a key on file the card's AI switch asks first, and the key line goes
- **`specs/course-card-stats.spec.ts`** (3, CI): a course card shows passed and mastered as a bar and chips; a chip opens its stat in a modal and its name fits a phone (#684) · a chip's modal fetches what is next as it opens: the next lesson to pass and each unit, the lessons to practise, and opens the course at one (#900) · with the app in French, when a course was last opened fits its card at 1280 and 390 px, without an ellipsis (#1182)
- **`specs/course-copy.spec.ts`** (5, CI): the copy icon in Spanish's settings opens Copy of Spanish in the editor, with every file, and lists it · copies count against the account's course bytes: past the limit the copy is refused with a notice · no copy or translate icon in the settings of a course shared with you; copying or translating it, or another user's private course, is a 404 · no copy icon while a course is being generated, nor after it stopped; a POST says why · at phone width the copy icon sits beside the X, inside the dialog and clear of its title
- **`specs/course-lints.spec.ts`** (2, CI): a prerequisite after its lesson shows up as a thing to check · the lesson editor shows a lint as an amber ! on its field after the save, opens it in a modal, and drops it after the fix
- **`specs/course-report.spec.ts`** (3, local only): the report shows which features each lesson uses · the report fits a phone · the report follows a save
- **`specs/course-translate.spec.ts`** (2, CI): Copy and translate to Español makes a translated copy; the original stays as it was · the dialog does not offer the language a course is already explained in
- **`specs/create.spec.ts`** (15, CI): create, close the dialog, name the course and its lessons, then generate into it · a new course's card counts in the singular: 1 lesson · 1 unit, 1 Lektion · 1 Einheit (#1254) · editing a lesson's content takes Generate away · a refused generate form comes back open with what was typed · the effort beside Generate opens its own dialog and the account keeps the last level (#918) · the old pages lead to the new flow · the dialog asks which provider only when two or more keys are saved · the progress modal can't be dismissed while it runs; stopped, the course waits on the list with Resume · a generation that fails before its outline: Retry on the card, the dialog filled in, and a retry writes the same course · a stopped course can be deleted from its card · a stopped course's red ! does not widen the course list on a phone (#675) · a stopped course's red ! tip stays on a phone screen while it shows (#864) · the old status page leads to the editor · the dialog shows the lesson count, the time and the cost before generating (#443) · in German, a new course starts with German names and description
- **`specs/dev-ai.spec.ts`** (11, CI): a course generated through the dev AI provider shows in /dev/ai; limits, a timeout and an edited fixture reach the learner · the wire API refuses unknown keys and answers the OpenAI format; the learner profile has its limit · Manual replies: a request waits, folded at the bottom of /dev/ai, for the answer typed there; an ERROR: answer is the provider's error · leaving the tutor while its first turn waits on the model does not lock the tutor (#659) · a learner who leaves the tutor mid-turn twice has one call with the model, not two (#848) · coming back while the tutor is still answering, Cancel lets go of the old reply and starts again (#890) · a slow reply gets a Cancel after ten seconds, and the learner can carry on (#890) · in another language, coming back while the tutor is still answering also offers Cancel (#890) · coming back to the tutor in the same tab after a failed first turn starts again (#932) · each profile's keys are copy buttons in their provider's colours
- **`specs/docs.spec.ts`** (10, CI): the docs index is reachable logged out, with user and developer pages apart · every user page is served as HTML and as raw Markdown · /llms.txt lists the course format and its schema, and both are served · no docs page scrolls sideways at phone width · a long bare link in docs prose wraps at phone width · every in-app link on every docs page opens, and its #heading exists · a docs page's heading is on the first screen, the page list after it · the page list is not cut short, and its last page opens · on a desktop the page list stays beside the page · on a desktop every page in the list can be reached from the middle of a long page
- **`specs/editor-autosave.spec.ts`** (5, CI): typing in a lesson's JSON saves 3 s after the last keystroke, or when the drawer loses focus · a hidden tab sends what waits at once · what does not validate turns the circle red, says why, and is not stored but stays on the page · a save the server fails is tried again, and a page behind another tab is told to reload · an AI job locks the editor: what was typed is saved first, then nothing is saved until it ends
- **`specs/editor-errors.spec.ts`** (4, local only): a parse error, an unknown prerequisite and a duplicate id each say what, where and how to fix it · a problem in another lesson opens that lesson's drawer · the file page words a parse error as the editor does · Undo takes back changes that cannot be saved, after saving what can (#288)
- **`specs/editor-fixes.spec.ts`** (3, local only): the file page's path box checks the path and logs no console error (#273) · on a phone, Course settings and + Add unit do not overlap (#274) · closing a rename box by clicking elsewhere leaves focus where the click went (#275)
- **`specs/exact.spec.ts`** (34, CI): generate a course from the fixtures and open it · revise a generated course with feedback, then undo it · Use AI's Add / Edit / Add and edit choice: the box follows it, and an Edit run rewrites lessons in place (#1241) · the lesson table: move, rename, delete, add, edit JSON, each saved as it is made, then undo each page's edits · a course someone makes stays private to them · a failed generation explains itself in the dialog; its card keeps the raw detail behind a red ! · a key the provider does not know is refused by the provider, not by the app · the course list fills a desktop row and stacks on a phone (#428) · filter the lessons list, and say when nothing matches · look a word up from the lessons filter: its meaning and the lesson that teaches it (#348) · lessons show one unit at a time, with a tab per unit and page links for a long unit (#837) · the verb drill setup shows one set at a time, and its locked lessons a page at a time (#837) · pass a lesson's reading check · a note written on a lesson is there when the lesson opens again (#336) · a lesson's tappable words look tappable and take the keyboard (#403) · tapping a word shows its meaning in a bubble that fits the screen · finish a Sprint mode session · Sprint home fits a phone in every app language (#670) · on a phone the History table scrolls in its box, not the page (#424) · on a phone the Vocabulary term table scrolls in its box, not the page (#933) · on a phone the Study and Play mode bars take two rows and scroll sideways (#423) · the modes' old paths redirect, keeping the query string · the mode bars link to Study's Lessons, not to the /study redirect · the profile has no Dev section for anyone; the seeded keys answer generate and the tutor · the profile's e-mail field: a Verified check, and an (i) dialog with the name others see · Start asks for a course's settings before it opens, then keeps them behind its gear; the password changes in a dialog · a page that does not exist is a 404 inside the normal layout, with ways back (#397) · a form-only URL opened by address is a 405 inside the normal layout, with a way home (#1253) · the site installs as an app: every layout links a manifest that opens at the home page (#345) · #1154 a lesson without questions offers no practice · #1253 a form-only URL opened directly shows the error page, not a blank one · #1254 the course card says 1 lesson · 1 unit, not 1 lessons · 1 units · the Guide list folds locked stages under one lock per unit (#1213) · #1306 no Practise link for a lesson without quiz questions
- **`specs/explore.spec.ts`** (3, needs a key): explore ${replayPath ? "replay" : who}
- **`specs/fact-check.spec.ts`** (1, CI): a fact check shows a why line under the fixed lesson and the Check lines of the rest
- **`specs/fill-bar.spec.ts`** (5, CI): Practice: tap tiles into the gap, undo one, overfill it and hear how many beats went in · on a wide page the bar has no blank gap below it and the answer stays its own width · the reading check, Sprint mode and the Guide take any tiles that fill the gap · a lesson without a fill-bar question loads no fill-bar script · the lesson editor has the tiles and the what-counts switch, and its preview plays the question
- **`specs/forms.spec.ts`** (4, CI): Forms mode: add a word and a question in the lesson editor, see them as JSON, and find them in the file as saved · the JSON switch: a drawer reopened after a flip follows it, and quick flips end where they point · a new lesson is a whole form before it is saved · Forms mode edits the course settings too, and fits a phone
- **`specs/home.spec.ts`** (4, local only): the home page has a map row and the Guide as a mode of its own · each mode's explanation sits behind its (i) · the home page fits a phone · the Guide card is a plain link
- **`specs/i18n.spec.ts`** (3, CI): a visitor picks Spanish on the register page, the account keeps it, the profile changes it back · the browser's language is used before any pick, and an unknown one falls back to English · /dev/reset forgets the dev user's saved language, so the lang cookie decides again (#1363)
- **`specs/journeys.spec.ts`** (2, needs a key): journey ${journey.id}
- **`specs/judged.spec.ts`** (3, needs a key): a lesson page teaches a topic and ends with a check · a failed generation explains itself in plain words
- **`specs/keyboard.spec.ts`** (4, CI): Practice: keys sound on a tap, the last one counts, any spelling and octave unless the key is exact · the reading check, Sprint mode and the Guide take the key tapped · a lesson without a keyboard question loads no keyboard script · the lesson editor has the key, octaves and what-counts switches, and its preview plays the question
- **`specs/landing-try.spec.ts`** (4, CI): a visitor plays the language sample of every mode · maths: shade, the number line, plot a point, a formula, graphs · music: read the staff, hear it, play the keys, place a note · the try-it stage fits a phone in every subject and mode
- **`specs/lesson-ai-edit.spec.ts`** (5, CI): Edit with AI on a row rewrites that lesson only, and Undo takes it back · a reply that renames the lesson is refused and the course stays as it was · a refused request opens the dialog again with why · a lesson gone since the page loaded: the dialog opens again and says so · a lesson copied in the editor gets its ✦ once it is saved
- **`specs/lesson-conversations.spec.ts`** (2, CI): a lesson with a conversation shows both speakers' lines · tapping a conversation line shows its meaning and reads it aloud
- **`specs/lesson-duplicate.spec.ts`** (1, CI): duplicate a lesson: the copy sits below the original, with new ids, and the course still loads
- **`specs/lesson-editor-rules.spec.ts`** (10, CI): a type switch to numeric or expression says when the checker cannot read the answer · deleting the option that is the answer leaves no answer to pick, and a question keeps two options · a deleted word leaves the questions that named it; a deleted question leaves the Guide · a word typed through another word's term keeps its questions (#1248) · a word left on another word's term gets its questions back when the other word is renamed (#1248) · a duplicated word, sentence or conjugation says to change one; a duplicated question gets its own id · a sort keeps 2 to 4 buckets and 3 to 10 items; a deleted bucket's items move to the one before · a spot's wrong word: one the sentence has twice cannot be picked, and punctuation round a word does not stop it · a conversation keeps two lines and says when one is empty · a Guide's word count is a whole number, and an optional box of spaces is left out
- **`specs/lesson-editor.spec.ts`** (13, CI): the outline lists every section of the lesson, opens its items, and the preview knows which modes show them · a multiple-choice question: the right answer is picked, a new option grows from the dashed box, the type switch reshapes the form, and it all lands in the file · checks appear on the field as you type, in the outline, and from the server · a field's own check is a mark in its corner that opens a modal, so typing moves nothing below it (#1320) · a field's mark sits on its label's row in a two-column row too (#1320) · Try it: the preview grades a question with the course's own checker, in Study and in Sprint · Sprint exercises: buckets and items, the wrong word by a click, tiles and distractors, played in the preview · words, sentences, conversations and tables have their own forms · items are added and deleted from the outline, the Guide's steps are ordered by hand, and the lesson's prerequisites are picked · on a phone the three columns are tabs, and nothing overflows · the gear's Language features switch hides what only a language course fills, and is kept in the course settings · with the language features off, + Build is offered in a music course only (#1232) · the gear's Levels switch holds the Placement test switch, which adds or takes out placement.json under the course settings (#1106)
- **`specs/lesson-header.spec.ts`** (1, CI): on a phone the lesson text starts under 420 px down a 390x844 screen: one main button, the rest under More ways, the details behind an (i) (#999)
- **`specs/lesson-tables.spec.ts`** (2, CI): the test course's lessons with tables fit a phone · Spanish paradigm tables scroll in their box, not the page
- **`specs/lessons.spec.ts`** (17, local only): the server refuses a table it cannot use, and changes nothing · the last lesson cannot be deleted, and Escape keeps a lesson's name · a stopped run keeps its Resume, and has no editor until it is finished · while an AI lesson is being written, every way to change the course is refused · three saves at once leave a course that loads · a folded unit hides its lessons and still takes new ones · Add unit: named in place, saved in the settings, it takes lessons, an empty one goes at once and one with lessons asks first (#232) · course settings are the first row: pinned, kept, and their units reshape the table as they are typed · add-lesson refusals: short brief, and a reply that cannot be used leaves the course alone · Use AI: Only add appends a lesson each time; Allow overall changes can replace the whole course · a hand-made course can be changed overall too · the download is every file by path, with a format version · Delete this course is a red button that warns first · without JavaScript the table still lists the lessons, and Edit opens one · the lesson table fits a phone · a course with progress warns before a delete, and the table is for the owner's own courses only · a lesson page links the lessons before and after it in course order, below the lesson only, and neither past the ends (#402, #999)
- **`specs/mail.spec.ts`** (6, CI): a new account's confirmation e-mail opens in the dev inbox, and its link confirms the address · the outbox's test message arrives in the inbox and opens from the outbox · the dev mailbox is a mail client's table: bold until opened, archive strikes out and sinks, delete removes, all without a reload (#223) · a superuser resets another account's e-mail verification, and the new link confirms it again · only a superuser sees the Superuser page · the dev mailbox needs no sign-in: signed out and as a learner, every section and its JSON open
- **`specs/measure.spec.ts`** (2, CI): drag and turn the protractor, read the angle, type it · the reading check and the Guide draw it; Sprint mode leaves it out
- **`specs/mistakes.spec.ts`** (1, CI): a wrong quiz answer shows on the mistakes list, and a right one clears it (#338)
- **`specs/modals.spec.ts`** (5, CI): a Sprint mode stop's sheet closes on a tap outside it and on Escape (#233) · profile dialogs close on a click outside; one with something typed in its form stays (#226) · the Generate dialog closes on a click outside until a wish is typed; a running job's box stays (#226) · the /dev/jobs (i) opens its dialog; OK and a click outside close it (#872) · /dev/jobs fits a desktop window: Run now is in view, in German too (#1110)
- **`specs/music-sound.spec.ts`** (10, CI): Practice: the speaker plays the interval, the staff comes after the answer, and staves are options · no sound a second after the tap: the speaker says so · Can't listen now: off for an hour, the notes to read instead, and a tap turns it back on · Sprint mode: the speaker, staves as options and speaker tiles in Sort it · a wrong melody: the answer line shows the right one as a staff, not as ABC · a lesson without play loads no sound code · the lesson editor has the Sound box, the options switch and a sort item's sound · Guide: the options are three staves, a wrong melody grades wrong and the right one is marked · speaker tiles, a wrong tap keeps the chord on the pile to hear again, each placed chord replays · a percussion bar plays one pitch, the options are one-bar percussion staves, the right bar grades right
- **`specs/notation.spec.ts`** (14, CI): an abc block in the lesson text is drawn as a staff · on a wide page a staff is as tall as its drawing, and the lesson's staff stays wider than a question's · a lesson without notation loads no notation script · a music course's Sprint session has the circle-of-fifths build, graded exactly (#1232) · a question's abc is drawn as a staff above its prompt in Practice, the reading check, Sprint and the Guide · the lesson editor has the question's Music box and previews the staff · an empty and a non-ABC block in the lesson text keep their text · a question's abc that isn't music shows as text in the editor's preview · shows two notes on a staff and grades the interval · the learner taps the wrong note, then picks its fix; both must be right · dragging the wrong note down a step (or the down arrow) picks the tile of the note it lands on · the editor's spot form has the Music box, the note number and the fix, and previews the staff · shows a stacked chord on a staff and grades its name · shows a key signature or a tune and grades the key
- **`specs/owner-resets.spec.ts`** (3, CI): a superuser sets a temporary password; the learner signs in with it and must choose a new one · a superuser turns off another account's two-factor; others get 404 · reloading after a temporary password is set does not set another
- **`specs/pathways.spec.ts`** (7, CI): switch course: Spanish, then a generated course, then back · generate from notes and uploaded files, with no wish · cancel a slow generation, then resume it to a ready course · resume a run that stopped on a lesson: the resumed job keeps what was written · saving into a stopped run keeps it resumable, and Resume keeps the edit (#81) · the editor opened between the outline and lesson 1 still follows the job to its end · remove an API key: the profile says no key, and generating no longer offers it
- **`specs/privacy.spec.ts`** (3, CI): the privacy notice is public, dated, and no longer a draft · the notice has one heading per section, the summary list and the US-rights table · at phone width the notice does not scroll sideways and the table can be scrolled on its own
<!-- themes -->
- **`specs/profile-autosave.spec.ts`** (7, local only): the green checks come a moment after the save, fade after 5 s, and come back with the next edit · names save a second after typing stops, in one request, and the heading follows · a field that needs attention turns red at once, is not sent, and leaving asks first · the server refusing a field marks it red with the server's reason · without a connection the circle turns red, leaving asks, and it saves once back online · the profile has no picture and no theme · API keys: folded away, a pasted key saves at once, a typed one 3 s after typing stops
<!-- /themes -->
- **`specs/profile-nav.spec.ts`** (1, CI): the profile nav marks the section in view
- **`specs/prompt-limits.spec.ts`** (5, local only): the Generate dialog counts the wish and the notes, red outside 20 to the maximum · the server refuses Generate prompts outside the limits, in the same words · Use AI counts its text, red outside 20 to the maximum, and the server agrees · a Generate form that is too short is refused in place, calmly · short notes are refused in place too, and the notes box is the one marked
- **`specs/record.spec.ts`** (3, needs a key): record ${id}
- **`specs/review-ai-changes.spec.ts`** (14, local only): a generated course reviews as new lessons, listed like the lesson table · Only add shows the new lesson as one column; what was there keeps no red · Use AI runs on the course page: the modal comes up in place, holds the page, then the review opens · a refused Use AI request comes back on the same page, text kept · a revision shows red and green side by side, and later hand edits do not change it · leaving out a line and a lesson: Accept writes only the rest, and the review remembers · editing a line in place as text: it turns blue and stays in, Undo/Redo step through edits, Accept writes them · after a lesson-table edit, leaving out part of a lesson still goes in (#280) · after Undo of the AI run the review says so and offers no Accept (#281) · the review labels lines in plain words, not JSON keys (#282) · Don't show this automatically next time: the review waits for its button · Review changes is a button next to Submit · a course nobody used AI on has no review button · without JavaScript the review is a page whose boxes Accept sends
- **`specs/review-and-revise.spec.ts`** (4, local only): while a revision runs the modal covers the locked workspace; closing the review shows the new lessons · a lesson edited by hand survives a revision that keeps it · undo puts back the files from before the revision and drops later hand edits · a hand edit that breaks the course rules does not block a revision of a course that loads again
- **`specs/revise.spec.ts`** (11, local only): revise a course twice in a row, and undo goes back one step · undo and redo icons right after a revision, from the course page the review closes onto (#231) · Undo and Redo: rows the step adds fade in, rows it takes away fade out, the rest stay still; none under reduced motion (#251) · a generated course is changed with Only add or Allow overall changes, not generated again · a course with content of its own sends you to the Use AI box, not to the old Only add buttons (#1245) · feedback that is too short is refused on the form and kept · a plan that removes a lesson drops it, keeps the others' progress, and undo brings it back · a rewritten lesson keeps its progress · a plan the AI cannot give leaves the course alone and offers neither undo nor resume · while a revision runs the course still opens but its files are locked; cancel releases it · two revisions asked for at the same moment start only one job
- **`specs/scale-ladder.spec.ts`** (3, CI): Sprint mode: tap the notes of D major onto the ladder; graded at Check · Study: the lesson page shows the scale on a staff with a speaker · the lesson editor's Scale form checks the notes and decoys and previews the scale
- **`specs/share-edges.spec.ts`** (8, CI): a used link turns a second account away; the invited address, confirmed later, has the course with or without the link · unknown, odd and padded tokens: the link says it no longer works (#1082), the sign-in pages carry it · removed and invited again: a new link, the old one dead, the learner's progress back · an old link after the share was removed says it no longer works (#1082) · every mode opens on a shared course with the learner's own progress, kept when the owner edits the lesson · the share removed mid-session: the next page goes to the course list, the API answers 410 · the owner deletes the course, or the whole account: the course leaves the learner's list and session · the invitation is in the owner's language; the learner's pages stay in theirs
- **`specs/share-removed.spec.ts`** (1, CI): the share removed mid-Sprint: the page goes to the course list instead of 'try again'
- **`specs/share-sign-in.spec.ts`** (2, CI): a share link survives the two-factor code step, and cancelling it keeps the link · an old link after the share was removed says it no longer works, signed out and signed in (#1082)
- **`specs/share.spec.ts`** (2, local only): an invited address registers from the link and has the course read only, until the owner removes the share · a shared course's circle tip wraps inside a phone screen while it shows, even with a long owner address (#954)
- **`specs/smoke/complete-test-course.spec.ts`** (3, CI): K: read and practise every lesson · D: every lesson's session, every exercise kind, the review mix and lightning · G: every Guide stage, in order
- **`specs/smoke/daily-goal.spec.ts`** (1, CI): every mode fills one daily goal and resumes where the learner left off
- **`specs/smoke/drill-first-set.spec.ts`** (2, CI): the drill opens on the first set with unlocked verbs, not Present · Start drill works without JavaScript
- **`specs/smoke/export.spec.ts`** (2, CI): a learner asks for their data, gets the link by e-mail and downloads the JSON once
- **`specs/smoke/french.spec.ts`** (5, CI): the course is on /courses, offers its four levels, and starts at greetings · Study: read greetings, pass its check, practise it; numbers' numeric questions grade · Sprint: play greetings' session right at phone width · Play: play the first stage · games: the verb drill (être, avoir) and Date Dash
- **`specs/smoke/happy-paths.spec.ts`** (10, CI): 1. a new learner sees lesson 1 as next · 1b. a stage read in Study but never played says Start in the Guide, not Try again (#417) · 1c. a Guide stage played to the end under 70 % says Try again, not Continue past its last step (#417) · 2. Study: read lesson 1, pass its check, practise it: passed everywhere · 3. Sprint: play lesson 1's session right: passed everywhere · 4. Play: play stage 1: passed everywhere · 5. a lesson passed in one mode is passed in the other two · 6. progress survives logging out and back in · 7. a lesson whose every answer was right is not listed as needing practice (#681) · 8. a Guide stage's quick checks mix their options, so the right one is not always first (#962)
- **`specs/smoke/mini.spec.ts`** (6, CI): a learner registers, signs in again and passes a lesson with nothing configured · there is no dev mailbox · the server says it is a Skrongle mini · the profile has no red ! on the e-mail · sharing shows the link once, and a reload goes back to the dialog without inviting again
- **`specs/smoke/motivator-reward.spec.ts`** (1, CI): every mode's end screen shows the worn motivator's reward line
- **`specs/smoke/motivator-skc.spec.ts`** (1, CI): Skrongle Cash draws the hero and reward line, and keeps counting while another motivator is worn
- **`specs/smoke/motivator-xp.spec.ts`** (1, CI): XP counts what was earned under any motivator and levels the learner up
- **`specs/smoke/motivator.spec.ts`** (1, CI): the motivator worn draws the course home, and switching keeps progress
- **`specs/smoke/password-reset.spec.ts`** (2, CI): registering a taken address looks the same and mails its owner · a forgotten password is reset from the login page through the mailed link
- **`specs/smoke/phone-width.spec.ts`** (2, CI): the main pages and a lesson in each mode fit a 390 px phone · the course editor fits a 390 px phone
- **`specs/smoke/placement.spec.ts`** (2, CI): the placement test suggests the first unit with a miss, shows a report card, and the course starts there · the marking is the server's: answers come back with each unit's tally and no question carries its answer
- **`specs/smoke/practice-empty-check.spec.ts`** (1, CI): Check with no answer asks for one inline, on a choice and on a typed question
- **`specs/smoke/practice-keys.spec.ts`** (2, CI): number keys pick an option and Enter checks, then advances · digits typed into a fill-in answer stay in the text box
- **`specs/smoke/practice-nothing-yet.spec.ts`** (2, CI): a fresh learner's practice shows Read next and no counter · a round with questions still shows the counter
- **`specs/smoke/practice-result-order.spec.ts`** (1, CI): the result box sits above Next, and Hint goes once the answer is graded
- **`specs/smoke/practice-summary.spec.ts`** (2, CI): the mastery change is shown as (+N), and left out when nothing changed · a round too short to count says what it needs the answers for
- **`specs/smoke/privacy.spec.ts`** (3, CI): the privacy policy is served to visitors · deleting the account signs out and waits 14 days; signing in can cancel it (#970) · the AI forms say which provider gets the notes
- **`specs/smoke/self-host.spec.ts`** (4, CI): the self-hosting guide is linked from the docs and readable on a phone · the guide is only linked from the docs, not the landing page or the footer · the self-hosting FAQ is linked from the technical guide and points at the diagnostics
- **`specs/smoke/skc-shop.spec.ts`** (2, CI): the shop sells a badge once and keeps it across motivators · without JavaScript a refused buy says why
- **`specs/smoke/sprint-check-error.spec.ts`** (2, CI): a failing check shows one error line, replaced on retry, cleared on success · a typed answer stays editable after a failed check (issue #672)
- **`specs/smoke/sprint-ladder.spec.ts`** (2, CI): fast taps all land on the rungs, a filled rung gives its form back, all right grades right · one form on a wrong rung grades wrong, shows the key, and History agrees with its ✗
- **`specs/smoke/sprint-sort.spec.ts`** (2, CI): a wrong bucket shakes the word and keeps it; the sort checks itself, wrong, and History says why · a clean sort, by the keyboard: Enter on the right bucket drops the word, and the sort checks itself right
- **`specs/smoke/start-level.spec.ts`** (4, CI): start Spanish at A2: the A1 lessons are skipped and never offered · change level mid-course (#333): progress stays, every mode moves to the chosen level · the test course offers each of its units as a level · a course without levels never asks for one
- **`specs/smoke/test-course-games.spec.ts`** (6, CI): every game on /play is played here · K Listening: a lesson's round, then a mixed round · Vocabulary: collect words in Sprint mode, then the vocabulary page and the flashcards · Word Match: the lesson board and the all-lessons board · the paradigm drill, Numbers by ear, Date Dash (each drill) · Tutor: a tutored lesson over the fixtures, one tapped and one typed answer
- **`specs/smoke/verify-email.spec.ts`** (3, CI): a new learner confirms their e-mail address from the link · sending the e-mail again replaces the first link
- **`specs/smoke/wrong-answers-games.spec.ts`** (5, CI): Guide: a stage of wrong quick checks isn't passed and the next stays locked; a replay passes it · Word Match: wrong tries are counted and keep the lesson from passing · the games: a wrong answer is marked wrong and History counts it · flashcards: a card marked not known comes back in the missed deck · Tutor: a wrong answer gets a wrong verdict
- **`specs/smoke/wrong-answers.spec.ts`** (7, CI): K: every question type, answered wrong, is marked wrong and doesn't pass · K: one lesson answered wrong reads as \"1 lesson needs practice\" on the course home and map · K: the 70 % line, 3 of 5 doesn't pass and 4 of 5 does · near misses are wrong, and the answers they nearly are stay right · a reading check with one wrong answer fails, the lesson stays unread, and a retry passes · D: every exercise kind, answered wrong, is marked wrong, and no lesson passes on wrong answers · D: the session's score is the answers given, on the screen, in History and in progress
- **`specs/staff-note.spec.ts`** (5, CI): Practice: tap, drag and the keyboard place the note; the octave counts unless any octave does · a note a step off is wrong with no octave note · the reading check, Sprint mode and the Guide take the note · a lesson without a staff-note question loads no staff-note script · the lesson editor has the note, clef and any-octave boxes, and its preview plays the question
- **`specs/terms.spec.ts`** (5, CI): the terms page is public and says when it was last updated · registration needs the terms checkbox, and records the acceptance · a material change: the notice cannot be dismissed and blocks the API until accepted · a minor change: the notice shows what changed and Continue closes it for good · the profile's read link shows the terms in a dialog and closes back to the profile (#152)
- **`specs/two-factor.spec.ts`** (2, CI): e-mail sign-in codes are not offered until the address is verified; then the code comes by e-mail and a wrong one is refused (#292) · an authenticator app is added through the dev authenticator and its codes sign in, each once (#292)
- **`specs/word-list.spec.ts`** (3, CI): a pasted word list previews, flags what it can't read, and becomes a lesson that opens with the words · a paste past the cap is refused in words, not with a body-limit error · the button follows the Language features switch
- **`specs/words-due.spec.ts`** (2, CI): due words show on the course home, a due round reviews them, and the card goes (#1360) · on a phone the card stacks: the button runs the full width under the words (#1360)
