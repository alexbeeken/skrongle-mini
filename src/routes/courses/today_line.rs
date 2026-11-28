//! The course list's line about today: on each started course's
//! card, today's goal ("12 of 20 today", or a ✓ once it is met), the day
//! streak and the words due, the same numbers the course home shows.
//!
//! The list reads them for every course at once, in three queries
//! ([`Days::load`]): today's effort points (`activity_log`), the earnings
//! ledger (streak, last day active) and the words due (`user_vocab`), each
//! grouped by course. No course's content is built for it.

use std::collections::HashMap;

use rusqlite::Connection;

use crate::activity::Today;
use crate::earnings::Ledger;

/// A course whose last activity is this many days back or more shows no line.
pub const QUIET_AFTER_DAYS: i64 = 30;

/// One card's line and the numbers in it (the card's `data-*`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodayLine {
    pub today: Today,
    /// The day streak, when the course's motivator shows it on the home and it is running.
    pub streak: Option<i64>,
    /// Words due by the end of today, when there are any.
    pub due: Option<i64>,
    /// "12 of 20 today · 5-day streak · 8 words due".
    pub text: String,
}

/// What the learner did in each course, read for the whole list.
pub struct Days {
    /// The learner's local day number now (`earnings::day_of`).
    day: i64,
    points: HashMap<String, i64>,
    ledgers: HashMap<String, Ledger>,
    /// course id -> words due.
    due: HashMap<String, i64>,
}

impl Days {
    /// Three queries, whatever the number of courses (inside `Db::call`).
    /// `tz` is the learner's offset from UTC in seconds (`CurrentUser::tz`).
    pub fn load(conn: &Connection, user_id: i64, tz: i64, at: i64) -> rusqlite::Result<Days> {
        let points = crate::activity::points_since_by_course(conn, user_id, crate::sprint::local_day_start(tz, at))?;
        let ledgers = Ledger::of_user(conn, user_id)?;
        let due = crate::vocab::due_by_course(conn, user_id, tz, at)?;
        Ok(Days { day: crate::earnings::day_of(tz, at), points, ledgers, due })
    }

    /// The line of the course `id`, worn with the motivator `motivator`
    /// (`projects.motivator`). None for a course with no activity in the
    /// last [`QUIET_AFTER_DAYS`] days, or none at all.
    pub fn line(&self, id: &str, motivator: &str) -> Option<TodayLine> {
        let ledger = self.ledgers.get(id)?;
        let last = ledger.day?;
        if self.day - last >= QUIET_AFTER_DAYS {
            return None;
        }
        let today = Today::of_points(self.points.get(id).copied().unwrap_or(0));
        let streak = if crate::motivators::get(motivator).shows_streak() { Some(ledger.streak_at(self.day).0) } else { None };
        let streak = streak.filter(|n| *n > 0);
        let due = self.due.get(id).copied().filter(|n| *n > 0);
        Some(TodayLine { today, streak, due, text: text(today, streak, due) })
    }
}

/// The line's words: the goal part, then the streak and words due when there are any.
fn text(today: Today, streak: Option<i64>, due: Option<i64>) -> String {
    let goal = if today.done() {
        crate::t("courses.picker.today_done")
    } else {
        crate::t2("courses.picker.today_goal", "n", today.points, "goal", today.goal)
    };
    let mut parts = vec![goal];
    parts.extend(streak.map(|n| crate::tn("courses.picker.today_streak", n)));
    parts.extend(due.map(|n| crate::tn("courses.picker.today_due", n)));
    parts.join(" · ")
}

#[cfg(test)]
mod tests;
