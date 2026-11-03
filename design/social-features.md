# Social feature ideas (#300)

Issue #300 asks for features that make learning feel shared across the whole
community, without building a social network. This report lists ideas to
pick from. Each one keeps to the rules that exist: nobody is named, every
count hides below `MIN_CROWD` (5 learners, `src/motivators/social.rs`), and
everything is per course.

## What exists today

- The Social motivator (#563) puts up to four anonymous lines on the course
  home: effort this week against others, how many study today, how many kept
  their streak (#505), and how many worked on the next lesson in the last
  hour (#501). Its reward line says "You're one of 41 who passed this lesson
  this week." It stores nothing and caches its crowd for five minutes.
- Its data: `activity_log` (effort points), `lesson_passes` (seen and passed
  times), `lesson_touches` (who opened what), the `earnings` ledger (streak,
  points, goals; `src/earnings.rs`). `quiz_history` keeps every answer with
  `question_id`, `selected_answer` and `was_correct`.
- Word Match has the only named board (`game_scores`, "Ana L." from
  `profile::public_name`). Picture Hunt is gone (#842).
- #501 and #505 are closed: both shipped as Social lines.

## Rules every idea follows

- No names, no profiles, no free text, no messages. A share group is where
  names may live (#1000, separate report).
- A count or percentage appears only when at least 5 learners are in its
  window. On a small Skrongle mini server most of these simply stay hidden.
  That is fine: a lesson or server without the data shows nothing.
- Lines on the course hero and the reward line belong to the Social
  motivator; wearing it is the switch. Lines inside a lesson (ideas 1, 2, 4,
  5) are facts about the question or lesson, so I'd show them to everyone.
  That is a judgment call; see the open questions.

## The ideas

### 1. Hard for everyone
| Learner sees | After a wrong answer: "63 % of learners miss this one on the first try." After a right one on the same question: "Most learners miss this one. Nice." |
|---|---|
| Data | `quiz_history`, first answer per learner and question, per course. New index on `(course_id, question_id)`. Cached an hour per question. |
| Privacy | Only after 10 first answers from at least 5 learners, and only when the miss rate is 30 % or more. |
| Size, issues | S. Absorbs the useful half of #506 (miss rate). |

### 2. The usual slip
| Learner sees | After a wrong typed answer: "The usual slip here is 'soy' for 'estoy'." |
|---|---|
| Data | `quiz_history.selected_answer`, most common wrong answer per question, normalised the way the drill grades. |
| Privacy | Typed answers can hold anything. Show one only when at least 5 different learners gave the same wrong answer, and only for questions with a short expected answer. Never show a whole sentence. |
| Size, issues | M. New; sits next to #506. |

### 3. "This tripped me up too" vote
| Learner sees | A button after a wrong answer; "41 learners said this tripped them up." |
|---|---|
| Data | New table `question_votes(user_id, course_id, question_id, voted_at)`. |
| Privacy | Count only. The vote row is personal data, so `privacy.rs` export and delete must cover it. |
| Size, issues | S. The other half of #506. I'd drop it: idea 1 says the same thing from data we already keep, with no new table and no button. |

### 4. Usual time to pass
| Learner sees | On the lesson card or page: "Learners usually pass this in about 12 minutes." |
|---|---|
| Data | `lesson_passes`: median of `passed_at - seen_at` per lesson, capped at a day so a pass weeks later doesn't count. |
| Privacy | Median of 5 or more passes. |
| Size, issues | S. New. |

### 5. Most need two tries
| Learner sees | After a failed run: "Most learners need more than one try at this lesson. Go again." |
|---|---|
| Data | New: the runs a pass took. `lesson_passes.runs` counts every qualifying run, also the ones after the pass, so it can't say how many tries the pass needed. One counter column (`runs_before_pass`) set by `progress::record_run` when the lesson is first passed. |
| Privacy | Share of 5 or more learners who passed. |
| Size, issues | M (a migration and a change in `progress.rs`). New. |

### 6. Weekly course goal
| Learner sees | On the hero: "Together this week: 18,400 / 25,000 effort points. You added 140." When met: "We did it! This week's goal is reached." |
|---|---|
| Data | This week's effort is already in `Crowd` (`activity_log`). Target: the week before's total times 1.1, rounded to a friendly number. `Crowd` holds only the last eight days, so the target is one more range read on `idx_activity_log_course_at` (days 8 to 14), cached with the crowd. Nothing stored. |
| Privacy | Hidden below 5 active learners. On a shared course see #1000. |
| Size, issues | M. Replaces #504. Per course instead of server-wide, effort points instead of `quiz_history` + `game_activity`, a praise line instead of a reward item. |

### 7. Bands instead of a board
| Learner sees | "This week you're in the top 25 % of this course." Four bands: top 10 %, top 25 %, top half, getting started. |
|---|---|
| Data | `Crowd::others_effort`, the same as the "more effort than 70 %" line. |
| Privacy | No names, no rank number. Hidden below 5. The bottom band never says "bottom". |
| Size, issues | S. Replaces #502 community-wide. A named weekly board fits a share group (#1000). |

### 8. Pace through the course
| Learner sees | "You've passed more lessons than 40 % of the learners who started this course in the last month." |
|---|---|
| Data | `lesson_passes` count per learner, for learners whose first pass is in the last 30 days. |
| Privacy | Hidden below 5. Compares with people who started at about the same time, so a newcomer is not set against veterans. |
| Size, issues | S. New. |

### 9. Unit finished with others
| Learner sees | Reward line when a run finishes a unit: "You're one of 12 who finished A1 this month." |
|---|---|
| Data | `lesson_passes` and the unit list (the `units` count in `earnings::between`). |
| Privacy | Hidden below 5 finishers. |
| Size, issues | S. New. |

### 10. Welcome back
| Learner sees | First visit after 7 or more quiet days: "Welcome back. 31 learners came back to this course this week." |
|---|---|
| Data | `earnings.day` and `activity_log`: learners active this week after a gap of a week or more. |
| Privacy | Count only, hidden below 5. |
| Size, issues | S. New. |

### 11. Yesterday in this course
| Learner sees | A small card, once a day: "Yesterday: 9 units finished, 2 learners reached a 30-day streak, 140 lessons passed." |
|---|---|
| Data | Counts from `lesson_passes` and `earnings.streak` for the previous day. Nothing stored. |
| Privacy | A day-old total can't be matched to one person by timing, unlike a live ticker. Each count hidden below 5. |
| Size, issues | S. Replaces #507. A live ticker on a small site points at the one person who just passed. |

### 12. A cheer for the crowd
| Learner sees | One button on the hero, once a day: "Send a cheer to everyone studying today." Later: "12 learners sent this course a cheer today." |
|---|---|
| Data | New: one counter per course and day (`social_cheers(course_id, day, n)`); who sent it is not stored. |
| Privacy | Not aimed at anyone, so nobody can be found by it. Once a day per learner, kept in memory. |
| Size, issues | S. Replaces #508. Cheers aimed at one learner belong in a share group. |

### 13. Studying now, by mode
| Learner sees | On the Sprint page: "7 others are in a Sprint right now." |
|---|---|
| Data | `lesson_touches` and `activity_log` of the last 10 minutes, split by mode. |
| Privacy | Hidden below 5. Rounded down to 5, 10, 20 so a small number doesn't track one person. |
| Size, issues | S. Extends #501, which is per lesson. |

### 14. Words the course collected
| Learner sees | "This course's learners collected 3,200 words this week. Most collected: la cuenta." |
|---|---|
| Data | `user_vocab` (`src/vocab.rs`), new words per course this week. |
| Privacy | Totals only, the top word only when 5 or more learners collected it. |
| Size, issues | S. New. |

### 15. Game times without names
| Learner sees | After Numbers or Date Dash: "Your time beats 70 % of this week's players." |
|---|---|
| Data | `game_scores` (`games::save_score`), which only Word Match writes today. |
| Privacy | No names, unlike the Word Match board. Hidden below 5 players. |
| Size, issues | M. Replaces #503 for the games that are left (Picture Hunt is gone). |

### 16. Most missed, for authors
| Learner sees | Not learners: the owner of a course sees in the editor "Most missed in this lesson: question 3 (61 %)." |
|---|---|
| Data | `quiz_history` per course and question, as idea 1. |
| Privacy | The owner sees rates, never who answered. Hidden below 5 learners. On a built-in course this is for us only. |
| Size, issues | M. The follow-up named in #506. It fits the product focus: it makes the course content better. |

## Recommended first three

1. Hard for everyone (1). Mistakes are where learners feel most alone, and
   this speaks right then. The data is already kept, it needs one index,
   and a question without enough answers shows nothing.
2. Weekly course goal (6). The only cooperative idea: nobody loses. It
   reuses Social's crowd for this week's total; the target needs
   one more read of the week before, cached with it. Built per course, it
   also gives share groups their group goal (#1000) for free, with a group
   minimum in place of 5.
3. Bands instead of a board (7). It gives a gentle "how am I doing" without
   ranks or names, and it is a small change to a line Social already makes.

After these: idea 16 for authors, then 9 and 10 as reward and hero lines.
Park 2 (moderation risk of typed text) and 5 (needs a migration). Close #503,
#507 and #508 in favour of 15, 11 and 12, or as not wanted.

## Open questions

1. Lines inside a lesson (1, 2, 4, 5): shown to everyone, or only to those
   wearing Social?
2. Keep 5 as the minimum for everything, or raise it for percentages (say
   10) so "63 %" isn't 3 of 5 people?
3. Close #502, #503, #507 and #508 now, or keep them parked?
4. Should the weekly goal reset Monday in each learner's time zone, or run
   as "the last seven days" like the other Social lines?
