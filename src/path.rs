//! The learner path's ordering rule: every learning activity gets a score,
//! the number of days until it is due, and lists are sorted by it, lowest
//! first. Rule and settings: `docs/features-learner-path.md`.
//!
//! Days are local day numbers in the learner's time zone
//! (`earnings::day_of(CurrentUser::tz, at)`), so a review due tomorrow turns
//! up at the learner's midnight.
//!
//! Each attempt at a lesson's Study, Review or Play is stored in
//! `path_activity` by [`record`]. A learner's list, and hiding an activity
//! from it, is `path/list.rs`.

use rusqlite::{params, Connection, OptionalExtension};

mod list;
pub use list::{hide, list, unhide, Item};

/// The numbers the rule reads. `SETTINGS` holds the ones the app uses;
/// every function takes a `&Settings` so a test can pass its own.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    /// The gap after the first attempt, in days.
    pub first_gap_days: f64,
    /// What the gap is multiplied by per clean attempt while learning.
    pub growth: f64,
    /// Clean attempts in a row that make an activity mastered.
    pub clean_to_master: u32,
    /// Steps a clean attempt adds to the streak. The streak counts steps, so
    /// a step smaller than a clean attempt can be stored as a whole number.
    pub clean_steps: u32,
    /// Steps a pass below the clean line adds: half of a clean attempt.
    pub pass_steps: u32,
    /// The gap once mastered, in days.
    pub rest_days: f64,
    /// What the rest gap is multiplied by per clean attempt after mastery.
    pub rest_growth: f64,
    /// An attempt at or above this percentage is clean.
    pub clean_pct: i64,
    /// An attempt below this percentage fails.
    pub fail_pct: i64,
    /// How many days ahead of a review due today a never-tried activity sorts.
    pub head_start_days: f64,
}

/// The settings the app uses. The only place these numbers are written.
pub const SETTINGS: Settings = Settings {
    first_gap_days: 1.0,
    growth: 2.0,
    clean_to_master: 2,
    clean_steps: 2,
    pass_steps: 1,
    rest_days: 14.0,
    rest_growth: 2.0,
    clean_pct: 90,
    fail_pct: crate::progress::PASS_PCT,
    head_start_days: 0.0,
};

/// The last attempt at an activity: its local day and the streak after it, in steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tried {
    pub day: i64,
    pub streak: u32,
}

/// The streak, in steps, that makes an activity mastered: `clean_to_master` clean attempts.
pub fn master_steps(s: &Settings) -> u32 {
    s.clean_to_master.saturating_mul(s.clean_steps)
}

/// Whether a streak makes an activity mastered.
pub fn mastered(streak: u32, s: &Settings) -> bool {
    streak >= master_steps(s)
}

/// A streak in clean attempts, rounded down: what the course home shows.
pub fn clean_attempts(streak: u32, s: &Settings) -> u32 {
    streak.checked_div(s.clean_steps).unwrap_or(streak)
}

/// The days between an attempt and the next time the activity is due. The
/// gap grows by `growth` per clean attempt's worth of steps, so a half step
/// grows it by the square root.
pub fn gap(streak: u32, s: &Settings) -> f64 {
    let per_clean = f64::from(s.clean_steps.max(1));
    if mastered(streak, s) {
        s.rest_days * s.rest_growth.powf(f64::from(streak - master_steps(s)) / per_clean)
    } else {
        s.first_gap_days * s.growth.powf(f64::from(streak) / per_clean)
    }
}

/// The streak after an attempt scoring `pct`: `clean_steps` more when clean,
/// `pass_steps` more when it passes, back to 0 when it fails. A first attempt
/// leaves it at 0.
pub fn next_streak(streak: u32, pct: i64, first_attempt: bool, s: &Settings) -> u32 {
    if first_attempt || pct < s.fail_pct {
        0
    } else if pct >= s.clean_pct {
        streak.saturating_add(s.clean_steps)
    } else {
        streak.saturating_add(s.pass_steps)
    }
}

/// Days until the activity is due; `None` when it is off the list for good
/// (tried, and it does not repeat). `today` is a local day number.
pub fn score(tried: Option<Tried>, repeats: bool, today: i64, s: &Settings) -> Option<f64> {
    match tried {
        None => Some(-s.head_start_days),
        Some(t) if repeats => Some(gap(t.streak, s) - (today - t.day) as f64),
        Some(_) => None,
    }
}

/// The three kinds of activity on the path, as `path_activity.kind` stores them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// A lesson's Study, walked to its last step.
    Study,
    /// A Review session on one lesson.
    Review,
    /// A game played on one lesson.
    Play,
}

impl Kind {
    /// The kinds in the order a lesson's activities come in.
    pub const ALL: [Kind; 3] = [Kind::Study, Kind::Review, Kind::Play];

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Study => "study",
            Kind::Review => "review",
            Kind::Play => "play",
        }
    }

    pub fn parse(s: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.as_str() == s)
    }

    /// Whether the activity comes back after it is tried: only a Review does.
    pub fn repeats(self) -> bool {
        self == Kind::Review
    }
}

/// Store one finished attempt scoring `pct` at `now` (Unix seconds). The
/// first attempt starts the row with a streak of 0; a later one moves the
/// streak by [`next_streak`] and counts the attempt.
pub fn record(conn: &Connection, user_id: i64, course_id: &str, lesson_id: &str, kind: Kind, pct: i64, now: i64) -> rusqlite::Result<()> {
    let key = params![user_id, course_id, lesson_id, kind.as_str()];
    let streak: Option<u32> = conn
        .query_row("SELECT streak FROM path_activity WHERE user_id = ?1 AND course_id = ?2 AND lesson_id = ?3 AND kind = ?4", key, |r| {
            r.get(0)
        })
        .optional()?;
    match streak {
        None => conn.execute(
            "INSERT INTO path_activity (user_id, course_id, lesson_id, kind, first_at, last_at, last_pct, streak, attempts)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6, 0, 1)",
            params![user_id, course_id, lesson_id, kind.as_str(), now, pct],
        )?,
        Some(streak) => conn.execute(
            "UPDATE path_activity SET last_at = ?5, last_pct = ?6, streak = ?7, attempts = attempts + 1
             WHERE user_id = ?1 AND course_id = ?2 AND lesson_id = ?3 AND kind = ?4",
            params![user_id, course_id, lesson_id, kind.as_str(), now, pct, next_streak(streak, pct, false, &SETTINGS)],
        )?,
    };
    Ok(())
}

/// An activity with its score and its place in the course.
#[derive(Debug, Clone, PartialEq)]
pub struct Scored<T> {
    pub score: Option<f64>,
    pub order: i64,
    pub item: T,
}

/// The activities on the list (a score of 0 or less), lowest score first,
/// ties to the lower order number.
pub fn sort<T>(items: Vec<Scored<T>>) -> Vec<Scored<T>> {
    let mut on: Vec<Scored<T>> = items.into_iter().filter(|i| i.score.is_some_and(|v| v <= 0.0)).collect();
    on.sort_by(|a, b| {
        // Adding 0.0 turns -0.0 (a new activity's score) into 0.0, so it ties with a review due today.
        let (x, y) = (a.score.unwrap_or(0.0) + 0.0, b.score.unwrap_or(0.0) + 0.0);
        x.total_cmp(&y).then(a.order.cmp(&b.order))
    });
    on
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: &Settings = &SETTINGS;

    fn item(score: Option<f64>, order: i64, name: &'static str) -> Scored<&'static str> {
        Scored { score, order, item: name }
    }

    fn names(items: Vec<Scored<&'static str>>) -> Vec<&'static str> {
        sort(items).into_iter().map(|i| i.item).collect()
    }

    #[test]
    fn gaps_at_the_starting_values() {
        let gaps: Vec<f64> = (0..9).step_by(2).map(|n| gap(n, S)).collect();
        assert_eq!(gaps, vec![1.0, 2.0, 14.0, 28.0, 56.0], "whole clean attempts");
        assert!((gap(1, S) - 2f64.sqrt()).abs() < 1e-9, "a half step grows the gap by the square root");
        assert!((gap(5, S) / 14.0 - 2f64.sqrt()).abs() < 1e-9);
    }

    #[test]
    fn a_clean_review_each_time_it_is_due() {
        let mut t = Tried { day: 0, streak: next_streak(0, 100, true, S) };
        let mut days = vec![0];
        let mut mastered_on = None;
        while days.len() < 6 {
            // Due on the first day its score reaches 0.
            let due = (t.day..).find(|&d| score(Some(t), true, d, S).is_some_and(|v| v <= 0.0)).unwrap();
            assert_eq!(score(Some(t), true, due - 1, S), Some(1.0), "not due a day early");
            t = Tried { day: due, streak: next_streak(t.streak, 100, false, S) };
            if mastered(t.streak, S) && mastered_on.is_none() {
                mastered_on = Some(due);
            }
            days.push(due);
        }
        assert_eq!(days, vec![0, 1, 3, 17, 45, 101]);
        assert_eq!(mastered_on, Some(3), "two clean reviews master it");
    }

    #[test]
    fn a_first_attempt_leaves_the_streak_at_zero() {
        assert_eq!(next_streak(0, 100, true, S), 0);
        assert_eq!(next_streak(2, 100, false, S), 4);
    }

    #[test]
    fn a_pass_is_half_a_step_and_a_fail_resets_it() {
        assert_eq!(S.clean_steps, 2 * S.pass_steps, "a pass is half a clean attempt");
        assert_eq!(next_streak(2, 80, false, S), 3);
        assert_eq!(next_streak(2, 95, false, S), 4);
        assert_eq!(next_streak(6, 50, false, S), 0);
        assert_eq!(gap(next_streak(6, 50, false, S), S), 1.0);
        assert_eq!(next_streak(4, S.fail_pct, false, S), 5, "the pass line itself passes");
        assert_eq!(next_streak(4, S.clean_pct, false, S), 6, "the clean line itself is clean");
        assert_eq!(next_streak(4, S.clean_pct - 1, false, S), 5);
    }

    #[test]
    fn two_clean_reviews_or_four_passes_master_it() {
        let run = |pct: i64, n: usize| (0..n).fold(next_streak(0, pct, true, S), |st, _| next_streak(st, pct, false, S));
        assert!(!mastered(run(100, 1), S));
        assert!(mastered(run(100, 2), S));
        assert!(!mastered(run(80, 3), S));
        assert!(mastered(run(80, 4), S));
        // One clean and two passes.
        assert!(mastered(next_streak(next_streak(next_streak(0, 100, false, S), 80, false, S), 80, false, S), S));
        assert_eq!((clean_attempts(3, S), clean_attempts(4, S)), (1, 2), "a half shows once it makes a whole");
    }

    #[test]
    fn never_tried_scores_zero_and_ties_go_to_the_lower_order() {
        assert_eq!(score(None, true, 10, S), Some(0.0));
        assert_eq!(score(None, false, 10, S), Some(0.0));
        let due_today = score(Some(Tried { day: 9, streak: 0 }), true, 10, S);
        assert_eq!(due_today, Some(0.0));
        assert_eq!(names(vec![item(Some(0.0), 5, "late"), item(due_today, 2, "review"), item(None, 1, "done")]), vec!["review", "late"]);
    }

    #[test]
    fn a_head_start_sorts_new_work_between_due_and_overdue() {
        let s = Settings { head_start_days: 1.0, ..SETTINGS };
        let new = score(None, true, 10, &s);
        let due_today = score(Some(Tried { day: 9, streak: 0 }), true, 10, &s);
        let overdue = score(Some(Tried { day: 7, streak: 0 }), true, 10, &s);
        assert_eq!((new, due_today, overdue), (Some(-1.0), Some(0.0), Some(-2.0)));
        let list = names(vec![item(due_today, 1, "due today"), item(new, 3, "new"), item(overdue, 2, "overdue")]);
        assert_eq!(list, vec!["overdue", "new", "due today"]);
    }

    #[test]
    fn a_tried_activity_that_does_not_repeat_has_no_score() {
        assert_eq!(score(Some(Tried { day: 0, streak: 0 }), false, 30, S), None);
    }

    /// (last_at, last_pct, streak, attempts) of one row.
    fn row(c: &Connection, kind: Kind) -> (i64, i64, u32, i64) {
        c.query_row(
            "SELECT last_at, last_pct, streak, attempts FROM path_activity WHERE user_id = 1 AND course_id = 'c' AND lesson_id = 'l' AND kind = ?1",
            [kind.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap()
    }

    #[test]
    fn record_starts_a_row_then_moves_its_streak() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            crate::auth::create_user(c, "ann@example.org", "x")?;
            record(c, 1, "c", "l", Kind::Review, 100, 10)?;
            assert_eq!(row(c, Kind::Review), (10, 100, 0, 1), "a first attempt, even a clean one, leaves the streak at 0");
            let first: i64 = c.query_row("SELECT first_at FROM path_activity", [], |r| r.get(0))?;
            record(c, 1, "c", "l", Kind::Review, 95, 20)?;
            assert_eq!(row(c, Kind::Review), (20, 95, 2, 2), "clean: a whole step");
            record(c, 1, "c", "l", Kind::Review, 80, 30)?;
            assert_eq!(row(c, Kind::Review), (30, 80, 3, 3), "a pass: half a step");
            record(c, 1, "c", "l", Kind::Review, 40, 40)?;
            assert_eq!(row(c, Kind::Review), (40, 40, 0, 4), "a fail resets it");
            let after: i64 = c.query_row("SELECT first_at FROM path_activity", [], |r| r.get(0))?;
            assert_eq!((first, after), (10, 10));
            // Each kind has its own row.
            record(c, 1, "c", "l", Kind::Play, 50, 50)?;
            assert_eq!(row(c, Kind::Play), (50, 50, 0, 1));
            assert_eq!(row(c, Kind::Review), (40, 40, 0, 4));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn sort_drops_what_is_not_due_yet() {
        let not_yet = score(Some(Tried { day: 10, streak: 2 }), true, 11, S);
        assert_eq!(not_yet, Some(1.0));
        assert!(names(vec![item(not_yet, 1, "later")]).is_empty());
    }
}
