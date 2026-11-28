# The learner path

The course home (`/`) is a list of what to do next. Every lesson has up to
three activities: its **Study** (the lesson in small steps), its **Review**
(a Review session on that lesson) and its **Play** (Word Match on its
terms). One rule gives each activity a score, the number of days until it
is due, and the list is sorted by it, lowest first (#318).

The rule is `src/path.rs`. A learner's list is `src/path/list.rs`. The page
is `src/routes/home.rs`.

## The rule

Days are whole local days in the learner's time zone
(`earnings::day_of(tz, at)`), so a review due tomorrow turns up at the
learner's midnight.

| The activity | Its score | On the list |
|---|---|---|
| Never tried | 0 (less `head_start_days`) | Yes, in course order |
| A Study or a Play, tried | none | No. It does not come back |
| A Review, tried | its gap, less the days since the last attempt | Once the score is 0 or less |

`path::sort` keeps the activities with a score of 0 or less, lowest first.
An overdue review therefore comes before new work, and a review due today
ties with new work. A tie goes to the activity that comes first in the
course.

**The streak.** Each activity has a streak, counted in steps so that half
of a clean attempt can be stored as a whole number (`path::next_streak`):

- the first attempt leaves it at 0, whatever the score;
- a clean attempt (90 % or more) adds 2 steps;
- a pass below that (70 % to 89 %) adds 1 step;
- a fail (below 70 %, the pass line of `progress::PASS_PCT`) puts it back
  to 0.

**Gaps.** The gap is the days between an attempt and the next time the
activity is due (`path::gap`). It starts at 1 day and doubles with each
clean attempt: 1 day, then 2. A pass is half a step, so it grows the gap by
the square root of 2. A part of a day counts as a whole day: a gap of 1.4
days is due after 2.

**Mastery.** A streak of 4 steps masters the activity (`path::mastered`):
two clean reviews, four passes, or one clean review and two passes. From
then on the gap is 14 days and still doubles with each clean attempt: 14,
28, 56. A fail at any point puts the streak back to 0 and the gap to 1 day.

A review answered clean each time it is due comes back on days 1, 3, 17, 45
and 101 after the first attempt, and is mastered on day 3.

## Attempts

`path::record` stores one finished attempt in `path_activity`: one row per
learner, course, lesson and kind, with the first and last time, the last
score, the streak and the number of attempts.

| Kind | Recorded when | Where | Its score |
|---|---|---|---|
| Study | The lesson's Study is walked to its last step | `routes/guide.rs` (`POST /api/guide/step`) | Its quick checks right, of those answered |
| Review | A Review session on one lesson finishes with something answered | `routes/sprint.rs` (`POST /api/sprint/finish`) | Right answers of those answered |
| Play | A Word Match round on one lesson's board finishes | `routes/games.rs` (`POST /api/games/matching/finish`) | Pairs, of pairs plus wrong tries |

The review mix, the lightning round and Word Match on all terms are not on
one lesson, so they record nothing here. The same requests still report to
`progress` as before: the path does not change what "passed" means
([Progress across modes](progress)).

Which kinds a lesson has is `path::list::kinds_of`. A lesson has a Study
when it has Study steps, a Review when a session on it has something to
answer, and a Play when it has at least 2 vocabulary terms
(`games::MIN_WORDS`). A lesson without the data for a kind has no such
activity.

Migration v10 seeds the table from earlier progress, once. A lesson whose
Study was walked to the end, or that is passed, has its Study tried. A
passed lesson also has its Review tried and mastered, last tried at the
time of the migration, so it comes back after 14 days.

## The list

`path::list(conn, user, course, start, now, tz)` returns a `Path` with
three lists. It walks the lessons in course order and, for each lesson, its
kinds in the order Study, Review, Play:

- **`hidden`**: the activity is hidden by the learner, or its lesson is one
  the starting level skipped and the learner has not touched. In course
  order.
- **`done_today`**: its last attempt was today. Oldest first.
- **`next`**: everything else that is due, in the order of `path::sort`.

A Play is a reward. It is in `next` only once today's goal is met
(`activity::Today`). Before that it is on none of the lists.

## Hide and bring back

The eye beside an activity hides it: `POST /path/hide` with the form fields
`lesson` (the lesson's id) and `kind` (`study`, `review` or `play`).
*Bring back* under "Hidden (n)" is `POST /path/unhide` with the same
fields. Both go back to `/`. An unknown kind is a 400, a lesson that is not
in the course a 404 (`src/routes/path.rs`).

Hiding stores a row in `path_hidden` and changes nothing else. The
activity's attempts stay, so one brought back has the score it would have
had anyway.

Bringing back one activity of a lesson the starting level skipped makes the
lesson the learner's (`activity::touch`). Its other activities are then
hidden one by one, so only the one asked for comes back.

## The course home

`GET /` for a signed-in learner (`routes/home.rs`, `templates/home.html`,
`static/home.css`). On a wide screen the list is on the left and the
Progress panel on the right. On a narrow one the panel comes first.

| Part | What it shows | `data-testid` |
|---|---|---|
| Up next | The first activity of `next` as a card: the kind, the lesson's title, one line about it and **Start**, the page's only filled button | `path-next`, `path-start` |
| Then | The three activities after it, one row each. A row opens its activity | `path-then`, `path-item` |
| Nothing due | "Nothing is due. Come back tomorrow.", or that every lesson is passed | `path-empty` |
| Done today | What was done today, with "Back in N days" on a Review | `path-done` |
| Progress | The worn motivator's panel ([Motivators](features#motivators)): today's goal, the streak and what the motivator counts | `home-today` (with `data-motivator`) |
| Words due | In the Progress panel, on a day collected words are due: the count, an (i) with the ladder and *Review N words* (`/vocabulary/study?due=1`, 20 at most) | `home-due`, `home-info-due`, `home-due-review` |
| Whole course | A link to the Study list at `/study` | `path-whole` |
| Hidden (n) | Folded. Each hidden activity with *Bring back* | `path-hidden-toggle`, `path-hidden-item`, `path-unhide` |

Each activity on the list has the eye that hides it (`path-hide`) and
carries `data-lesson` and `data-kind`.

The line about an activity: a Study says "5 steps", or "Step 3 of 5" when
it is partly done. A Review that is new has no line. One that came back
says "Clean reviews: 1 of 2", or "Mastered. A check after 14 days". A Play
names its game, Word Match.

Start opens `/study/{slug}` for a Study, `/review/play?lesson={slug}` for a
Review and `/play/matching/play?lesson={slug}` for a Play.

On a course's first visit its settings wizard opens over this page
([A course's settings](features#a-courses-settings)).

## Back to the course home

An activity started from the list ends where the list is. A finished Study,
a finished Review session on one lesson and a finished Word Match board on
one lesson each end with one main button, **Back to {course}**
(`data-testid="activity-back-home"`), which opens `/`. The mode's own next
step (the next lesson's Study, *Play again*) is an outlined second choice.
The ✕ of a Review session on one lesson also goes to `/`.

A review mix, a lightning round and Word Match on all terms end as before,
on their own mode's page.

## Functions

| Function | What it does |
|---|---|
| `path::score(tried, repeats, today, &settings)` | Days until the activity is due. `None` once a Study or a Play is tried |
| `path::gap(streak, &settings)` | Days between an attempt and the next time it is due |
| `path::next_streak(streak, pct, first_attempt, &settings)` | The streak after an attempt |
| `path::mastered(streak, &settings)`, `path::master_steps(&settings)` | Whether a streak is mastered, and the steps that takes |
| `path::clean_attempts(streak, &settings)` | A streak in whole clean attempts, for the page |
| `path::sort(items)` | The due activities, lowest score first, ties to course order |
| `path::record(conn, user, course, lesson, kind, pct, now)` | Store one attempt and move the streak |
| `path::list(conn, user, course, start, now, tz)` | The learner's `Path { next, done_today, hidden }` |
| `path::hide(…)`, `path::unhide(…)` | Hide one activity, or bring it back |
| `Kind::repeats()` | Whether a kind comes back after it is tried. Only a Review does |

The pure functions take a `&Settings`, so a test can pass its own. The ones
that read or write the database use `SETTINGS`.

## Settings

`path::SETTINGS` is the only place these numbers are written. They are
constants of the app, not course data and not a setting of the server.

| Setting | Value | What it is |
|---|---|---|
| `first_gap_days` | 1 | The gap after the first attempt, in days |
| `growth` | 2 | What the gap is multiplied by per clean attempt while learning |
| `clean_to_master` | 2 | Clean attempts in a row that master an activity |
| `clean_steps` | 2 | Steps a clean attempt adds to the streak |
| `pass_steps` | 1 | Steps a pass below the clean line adds |
| `rest_days` | 14 | The gap once mastered, in days |
| `rest_growth` | 2 | What the rest gap is multiplied by per clean attempt after mastery |
| `clean_pct` | 90 | An attempt at or above this percentage is clean |
| `fail_pct` | 70 (`progress::PASS_PCT`) | An attempt below this percentage fails |
| `head_start_days` | 0 | How many days ahead of a review due today a never-tried activity sorts |
