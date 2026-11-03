# Progress across modes

The same material can be studied many ways: properly at a desk in Study mode, in
five-minute bursts on the bus in Sprint mode, as games in Play mode, one-to-one with
the AI tutor, or with any AI agent you talk to outside the app. All of them
move the same progress. This page holds the rule they share and the API an
outside agent uses to report what the learner showed it. Give this page to
an agent before it teaches (it is also at [`/docs/progress.md`](progress.md)
as raw Markdown).

## When a lesson counts as learned

A lesson is **passed** when both of these are true, in any order, in any
mode:

1. **Seen.** The learner has been taught the lesson's material. That means
   reading the lesson page and passing its short reading check, playing a
   Sprint mode lesson session or a Guide stage (both open with the lesson's
   teaching cards), or being taught it one-to-one by a tutor.
2. **Shown.** In one sitting, the learner answered at least **3** of the
   lesson's exercises and got at least **70 %** right. A lesson with fewer
   than 3 quiz questions needs all of them, and one with none needs nothing
   more than being seen. Sprint mode can't draw a protractor, so there
   `measure` questions don't count. Hints are fine. Answers given away or skipped count
   as wrong. The exercises must test this lesson's own focus.

Passing is a milestone and stays: a bad round later never takes it away.

## Starting at a later level

In a course whose units are levels (`"levels": true` in `course.json`), a
learner may start at a later level. They pick it the first time they open
the course, or later on the lessons page. The lessons of the levels below
are **skipped**, not passed: nothing is recorded for them and nothing is
scored. Recommended practice, the review mix, the lightning round, a mixed
listening round, the AI tutor and the "read next" hint never pick a skipped
lesson, and the Sprint mode path and the Guide start at the first lesson of
the chosen level. The lists show it as *Skipped*, and the "passed" totals
leave it out. A skipped lesson the
learner studies anyway (seen, read or answered) counts like any other from
then on.

**Changing level** later (the level in the course's settings, #333,
#587) changes only where the learner is. Every pass, mastery score
and answer stays, and nothing is passed for them. Lessons below the new level
they never touched become skipped; ones they studied stay theirs. Every
mode continues from the first open lesson of the new level, even when a
lesson below it was begun and not passed. Going back down un-skips the
lessons below, with their progress intact.

**Mastery** is separate. It is a 0–100 score per lesson that rises with each
right answer, falls with wrong ones and decays with time (see Mastery in
[Features](features)).
Mastery says how fresh a lesson is; a pass says it has been learned once.

A good round before the reading counts the moment the lesson is seen, so a
learner who tests themselves first and reads afterwards passes as soon as
they read.

## What each mode reports

| Mode | Seen | A run |
|---|---|---|
| Study mode | the reading check at the end of the lesson page (*I've read it* on a lesson without questions) | a practice round: each lesson it asked about is one run; a listening round on one lesson |
| Sprint mode | a lesson session (it opens with the lesson's bites) | the same session: every graded exercise |
| Play mode | a Guide stage (its concept cards) | the same stage: its quick checks; Word Match on one lesson's terms (each pair right, each wrong try a miss) |
| AI tutoring | the tutor taught the lesson | the session's answers on that lesson, skips as misses, once there are enough of them |
| AI agents | `POST /api/progress/seen` or `/pass` | `POST /api/progress/pass` |

Review mixes, the lightning round, the paradigm drill, Numbers by ear and
Date Dash are not about one lesson. They move mastery and
vocabulary but do not pass lessons.

Passing a lesson anywhere completes its stop on the Sprint mode path and its
Guide stage, unlocks the next stop of both, unlocks its items in the
paradigm drill, and shows a ✓ on the Study mode lessons list, lesson page and
map. The home page counts lessons passed.

## The daily goal

Every mode fills one daily goal: the ring on the course's home page and in
Sprint mode. It counts *effort points*, so reading, a game or a hand of
flashcards moves it as well as a quiz does. Something that takes longer
earns more. A typed quiz answer (about 15 seconds) is one point:

| What | Points |
|---|---|
| A quiz question, in any mode (Study, a Guide check, Sprint mode, an agent's answer) | 1 |
| A question from the AI tutor | 1.5 |
| A Sprint mode exercise that is not a quiz question | 0.6 |
| One item of a game (a pair, a number, a date, a picture, a drill form) | 0.4 |
| One sentence of a listening round | 1 |
| One flashcard | 0.3 |
| One card of a Guide stage | 0.3 |
| Reading a lesson to the end (its check passed, or *I've read it*) | 4 |

The goal is 20 points a day, in the learner's time zone, per course. Any of
these keeps the day streak going too. One running row per course (`earnings`,
`src/earnings.rs`) holds the streak, the days whose goal was reached and the
points ever logged. Each activity updates it, and the app never adds it up
from the whole log (#589). The table is `activity::Kind::points`
and `activity::DAILY_GOAL` in `src/activity.rs`. XP, which only
Sprint mode and the Guide used to give, is gone.

## Where you left off

Every mode resumes at the same lesson: the home page's *Continue*, the
current stop of Sprint mode's path, the Guide's current stage, the map's
"you are here" and Study mode's *Continue*. It is the lesson the learner
last opened or worked on, in any mode, unless it is passed; then the next
lesson after it that is neither passed nor skipped (from the start of the
course when none is left after it). A new learner starts at the first
lesson that is not skipped. It is open in Sprint mode and the Guide even
when the lesson before it is not passed.

Only something about one lesson moves it: its page, its Guide stage, a
Sprint mode session on it, a practice, listening or Word Match round on it
alone, a tutor session on it, or an agent's `seen` or `pass`. A review mix,
the lightning round or recommended practice draws on many lessons and does
not. *Continue* on the home page opens the lesson in the mode last used:
Sprint mode, the Guide, or the lesson page for the rest.

## For AI agents

You are teaching a learner who uses this app. Teach well and report
honestly, so the app shows the same progress whichever way they study.
Follow these rules:

- **Teach before testing.** Work from the lesson's own text and vocabulary
  (`GET /api/progress/lessons/{lesson}`), not from what you remember of the
  topic. Report `seen` once you have actually taught it.
- **Report every graded answer** with `POST /api/progress/answers`, right or
  wrong. That keeps mastery honest. Prefer the lesson's own quiz questions
  and let the app grade them. When you set your own exercise, grade it
  strictly, as the lesson's explanations would.
- **Declare a pass only when the rule holds**: the lesson was taught, and in
  this sitting the learner answered at least the lesson's `required_answers`
  of its exercises with at least 70 % right. Count skipped and given-away
  answers as wrong. Say in `evidence` what the learner did. The app refuses
  a pass whose numbers do not meet the rule.
- **Never pass a lesson for effort, for asking, or because a later lesson
  went well.** If the learner is not there yet, say what to practise and
  stop.

### Signing in

The API uses the learner's own session, as the browser does. Sign in once
and keep the cookie:

```sh
curl -c jar -d 'email=ann@example.org&password=…' https://host/login
curl -b jar https://host/api/progress
```

Everything works in the learner's active course. To change it, `POST
/courses/open/{id}` with the cookie.

### Endpoints

All requests and replies are JSON. A lesson can be named by its `id` or its
`slug`. Errors come back as HTTP 400 with a message saying what was wrong.

**`GET /api/progress`**: the rules (`rules.text`, `rules.pass_pct`,
`rules.min_answers`), the course, and every lesson in course order with
`id`, `slug`, `title`, `unit`, `focus`, `summary`, `quiz_questions`,
`required_answers`, `seen`, `passed`, `status` (in words), `passed_via`
(`sprint`, `study`, `play`, `tutor`, `agent` or `legacy`),
`best_pct`, `mastery` (0–100), `mastery_status` and `skipped` (below the
level the learner is at; see above), plus `start_unit`, the level they
chose ('' for the beginning). Start here to choose what to teach. Usually
pick the first lesson that is neither passed nor skipped. Teach a skipped
lesson only when the learner asks for it.

**`GET /api/progress/lessons/{lesson}`**: the same status for one lesson,
the rules, and the whole lesson file under `lesson`: its text in Markdown
(`content`), `vocabulary`, `quiz` (with `correct_answer`,
`accepted_answers`, `explanation` and `hints`), conversations and listening
sentences. The format is the [course format](course-format).

**`POST /api/progress/seen`** `{ "lesson": "ser-vs-estar" }`: you taught
it. The reply is the lesson's pass outcome (below).

**`POST /api/progress/answers`**: one answer. Either a quiz question for the
app to grade:

```json
{ "question_id": "a1-ser-vs-estar-03", "answer": "está" }
```

or an exercise you set and graded yourself:

```json
{ "lesson": "ser-vs-estar", "prompt": "Translate: the soup is cold", "answer": "la sopa está fría", "correct": true }
```

The reply has `lesson_id`, `correct`, `mastery_before` and `mastery`, and,
for a quiz question, the app's grading under `graded` (the right answer,
the explanation, any vocabulary collected). Answers appear in the learner's
History and count towards the daily goal and the streak.

**`POST /api/progress/pass`**:

```json
{ "lesson": "ser-vs-estar", "answered": 6, "correct": 5,
  "evidence": "Explained ser vs estar in their own words; 5 of 6 gap-fills right, missed one location." }
```

`answered` and `correct` are this sitting's exercises on this lesson. The
call marks the lesson seen and records the run. If the numbers meet the
rule the lesson is passed, credited to "an AI agent", and `evidence` is
kept with it. If they do not, the reply is a 400 that says why.

**Pass outcome** (the reply to `seen` and `pass`): `lesson_id`, `seen`,
`passed`, `passed_now` (this call passed it), `best_pct`, `run_pct`,
`run_counted`, `required_answers` and `summary` (in words, as the app shows
it).

**Limits.** Every write above, like an answer or finished round in the
app's own modes, counts towards a per-account limit of 60 in 10 seconds
and 3600 an hour; past it the reply is a 429 with `{ "error": "…" }`, so
wait a few seconds and send it again. Exercises you graded yourself (the
second form of `answers`) are also capped at 1000 per account in 24 hours;
quiz questions of the course still count after that.

## Under the hood

`src/activity.rs` holds the daily goal and the resume point: every mode
calls `activity::record` (a kind and how many, table `activity_log`) and,
for a lesson-focused page or round, `activity::touch` (table
`lesson_touches`); `activity::resume` is the one rule the home page, Sprint
mode, Study mode, the Guide and the map apply.

`src/progress.rs` holds the rule (`PASS_PCT`, `MIN_ANSWERS`,
`required_answers`, the `RULES` text the tutor and agents receive) and the
table `lesson_passes`. Per (user, course, lesson) it stores when and where
the lesson was first seen, the best counted run, when and where it was
passed, the number of counted runs, and an agent's evidence note. The
starting level is `projects.start_unit`; `Project::start` carries it as a
unit index, and `progress::is_skipped` (with `quiz::is_skipped` and
`mastery::Status::of_lesson` on top) is the one rule every picker and list
applies. Every mode calls `mark_seen`, `record_run` or `record_taught_run`;
none writes the table itself. Seen and Study mode's *read*
(`user_progress.read_at`, which lets a lesson into recommended practice) are
one thing: `mark_seen` sets both, so a lesson taught in Sprint mode or by a
tutor joins recommended practice too. Guide and Sprint mode completions from
before the table existed carried over as seen, and as passed where they had
earned two stars or more (70 %). Lessons already read count as seen.
