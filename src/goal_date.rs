//! A date to aim for (#235): an optional day a learner sets in a course's
//! settings (`projects.goal_date`, `YYYY-MM-DD`, '' for none). With one set,
//! the course home's chip row gains one quiet chip: the date and either "on
//! pace" or "about N lessons a week"; after the date, "Date passed" in grey.
//!
//! It is arithmetic on passes (`progress.rs`) and nothing else: the lessons
//! not passed (skipped ones left out) divided by the weeks left, rounded up,
//! against the lessons passed in the last [`RECENT_DAYS`] days. Days are the
//! learner's local days, like the daily goal (`earnings::day_of`). It never
//! touches the daily goal, the streak or passing, and nothing is sent or
//! scheduled. Only the learner's own course home reads it.

use std::collections::HashMap;

use rusqlite::{params, Connection};

use crate::content::ContentStore;
use crate::progress::LessonPass;

/// "On pace" looks at the lessons passed in this many local days, today included.
pub const RECENT_DAYS: i64 = 14;
/// The pace needed is out of reach above this many times the weekly pace of
/// the last [`RECENT_DAYS`] days: the opened chip then suggests a later date.
pub const OUT_OF_REACH: f64 = 2.5;
/// With less than this many days left the pace is still counted over one
/// week, so the chip never asks for more lessons than are left.
const MIN_DAYS: i64 = 7;

/// What the chip says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pace {
    /// No chip: every lesson is passed.
    None,
    /// The lessons a week needed, and the last weeks were at least that.
    OnPace(i64),
    /// The lessons a week needed, more than lately.
    PerWeek(i64),
    /// The lessons a week needed, far more than lately ([`OUT_OF_REACH`]).
    OutOfReach(i64),
    /// The date is behind today.
    Passed,
}

/// The pace for `left` lessons still to pass, `today` and `goal` as local day
/// numbers, and `recent` lessons passed in the last [`RECENT_DAYS`] days.
pub fn pace(left: i64, today: i64, goal: i64, recent: i64) -> Pace {
    if left <= 0 {
        return Pace::None;
    }
    if goal < today {
        return Pace::Passed;
    }
    let days = (goal - today).max(MIN_DAYS);
    // left / (days / 7), rounded up.
    let per_week = (left * 7 + days - 1) / days;
    let lately = recent as f64 * 7.0 / RECENT_DAYS as f64;
    if lately >= per_week as f64 {
        Pace::OnPace(per_week)
    } else if recent > 0 && per_week as f64 > OUT_OF_REACH * lately {
        // With nothing passed lately there is nothing to compare with: no
        // later date is suggested.
        Pace::OutOfReach(per_week)
    } else {
        Pace::PerWeek(per_week)
    }
}

/// The day number (days since 1970-01-01) of a `YYYY-MM-DD` date, as
/// `<input type="date">` sends it; None for anything else.
pub fn parse(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let num = |r: std::ops::Range<usize>| -> Option<i64> {
        let part = &s[r];
        if part.bytes().all(|c| c.is_ascii_digit()) { part.parse().ok() } else { None }
    };
    let (y, m, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    if !(2000..=2999).contains(&y) || !(1..=12).contains(&m) || d < 1 {
        return None;
    }
    let day = days_from_civil(y, m, d);
    // 31 February comes back as a day in March: not a date.
    (crate::profile::civil(day * 86_400) == (y, m as usize, d)).then_some(day)
}

/// Days since 1970-01-01 of a calendar date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Lessons of the course passed on one of the last [`RECENT_DAYS`] local
/// days, `today` included.
pub fn recent(content: &ContentStore, passes: &HashMap<String, LessonPass>, tz: i64, today: i64) -> i64 {
    content
        .lessons()
        .iter()
        .filter_map(|l| passes.get(&l.id)?.passed_at)
        .filter(|&t| crate::earnings::day_of(tz, t) > today - RECENT_DAYS)
        .count() as i64
}

/// Forget the date: every lesson is passed (the home page sees it).
pub fn clear(conn: &Connection, user_id: i64, course_id: &str) -> rusqlite::Result<()> {
    conn.execute("UPDATE projects SET goal_date = '' WHERE user_id = ?1 AND course_id = ?2", params![user_id, course_id])?;
    Ok(())
}

/// The chip on the course home and the two lines it opens.
#[derive(Debug, Clone)]
pub struct Chip {
    /// The date, short ("12 December"); empty once it has passed.
    pub date: String,
    /// "on pace", "about N lessons a week" or "Date passed".
    pub label: String,
    /// The date is behind today: the chip is grey.
    pub passed: bool,
    /// The opened line: lessons left, the pace that gets there, the last two weeks.
    pub line: String,
    /// Where **Change the date** goes, and where **Remove it** posts.
    pub change_url: String,
    pub remove_action: String,
}

/// The chip for a stored `goal_date` in course `course_id`; None without a
/// date or with nothing left to pass.
pub fn chip(course_id: &str, goal_date: &str, left: i64, today: i64, recent: i64) -> Option<Chip> {
    let goal = parse(goal_date)?;
    let (_, m, d) = crate::profile::civil(goal * 86_400);
    let date = crate::i18n::fill(&crate::t("goal_date.date"), &[("day", &d), ("month", &crate::profile::month_name(m))]);
    let to_pass = crate::i18n::fill(&crate::tn("goal_date.left", left), &[("date", &date)]);
    let say = |n: i64, label: String, later: bool| {
        let mut line = format!("{to_pass} {}", crate::t1("goal_date.need", "n", n));
        // Nothing passed lately: nothing said about it.
        if recent > 0 {
            line = format!("{line} {}", crate::t1("goal_date.lately", "n", recent));
        }
        if later {
            line = format!("{line} {}", crate::t("goal_date.later"));
        }
        (label, line, false)
    };
    let (label, line, passed) = match pace(left, today, goal, recent) {
        Pace::None => return None,
        Pace::OnPace(n) => say(n, crate::t("goal_date.on_pace"), false),
        Pace::PerWeek(n) => say(n, crate::tn("goal_date.per_week", n), false),
        Pace::OutOfReach(n) => say(n, crate::tn("goal_date.per_week", n), true),
        Pace::Passed => (
            crate::t("goal_date.passed"),
            format!("{} {}", crate::t1("goal_date.passed_line", "date", &date), crate::tn("goal_date.still", left)),
            true,
        ),
    };
    Some(Chip {
        date: if passed { String::new() } else { date },
        label,
        passed,
        line,
        change_url: crate::url(&format!("/courses#course-settings-{course_id}")),
        remove_action: crate::url(&format!("/courses/{course_id}/settings")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TODAY: i64 = 20_000;

    #[test]
    fn weeks_are_rounded_up() {
        // 14 lessons over 3 weeks: 4.67 a week, asked as 5.
        assert_eq!(pace(14, TODAY, TODAY + 21, 0), Pace::PerWeek(5));
        // 12 over 4 weeks: exactly 3.
        assert_eq!(pace(12, TODAY, TODAY + 28, 0), Pace::PerWeek(3));
        // 10 over 10 days: 7 a week.
        assert_eq!(pace(10, TODAY, TODAY + 10, 0), Pace::PerWeek(7));
    }

    #[test]
    fn under_a_week_counts_as_one_week() {
        // Tomorrow, or today itself (the last day): never more than is left.
        assert_eq!(pace(4, TODAY, TODAY + 1, 0), Pace::PerWeek(4));
        assert_eq!(pace(4, TODAY, TODAY, 0), Pace::PerWeek(4));
        assert_eq!(pace(30, TODAY, TODAY + 6, 0), Pace::PerWeek(30));
    }

    #[test]
    fn on_pace_compares_with_the_last_two_weeks() {
        // 5 a week needed; 10 passed in 14 days is 5 a week.
        assert_eq!(pace(15, TODAY, TODAY + 21, 10), Pace::OnPace(5));
        assert_eq!(pace(15, TODAY, TODAY + 21, 9), Pace::PerWeek(5));
    }

    #[test]
    fn far_above_lately_is_out_of_reach() {
        // 2 passed in 14 days is 1 a week: 2 is within 2.5 times, 3 is not.
        assert_eq!(pace(6, TODAY, TODAY + 21, 2), Pace::PerWeek(2));
        assert_eq!(pace(9, TODAY, TODAY + 21, 2), Pace::OutOfReach(3));
        // Nothing passed lately: no later date is suggested.
        assert_eq!(pace(28, TODAY, TODAY + 7, 0), Pace::PerWeek(28));
    }

    #[test]
    fn a_passed_date_and_a_finished_course() {
        assert_eq!(pace(3, TODAY, TODAY - 1, 5), Pace::Passed);
        assert_eq!(pace(0, TODAY, TODAY - 1, 5), Pace::None);
        assert_eq!(pace(0, TODAY, TODAY + 30, 5), Pace::None);
    }

    #[test]
    fn dates_parse_as_input_type_date_sends_them() {
        assert_eq!(parse("1970-01-01"), None); // before 2000
        assert_eq!(parse("2000-01-01"), Some(10_957));
        assert_eq!(parse("2026-11-19"), Some(20_776));
        assert_eq!(parse("2028-02-29"), Some(parse("2028-02-28").unwrap() + 1));
        for bad in ["", "2026-2-1", "2026-02-30", "2026-13-01", "2026-00-10", "20261119xx", "+026-11-19", "2026/11/19"] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn the_chip_reads_as_the_mockup() {
        let today = parse("2026-11-19").unwrap();
        let c = chip("es", "2026-12-10", 14, today, 10).unwrap();
        assert_eq!((c.date.as_str(), c.label.as_str(), c.passed), ("10 December", "on pace", false));
        assert_eq!(c.line, "14 lessons to pass by 10 December. About 5 a week gets you there. In the last two weeks you passed 10.");
        let c = chip("es", "2026-11-20", 14, today, 4).unwrap();
        assert_eq!(c.label, "about 14 lessons a week");
        assert!(c.line.ends_with("A later date would fit how you have been learning."), "{}", c.line);
        let c = chip("es", "2026-12-10", 14, today, 0).unwrap();
        assert_eq!(c.line, "14 lessons to pass by 10 December. About 5 a week gets you there.");
        let c = chip("es", "2026-11-16", 1, today, 4).unwrap();
        assert_eq!((c.date.as_str(), c.label.as_str(), c.passed), ("", "Date passed", true));
        assert_eq!(c.line, "You were aiming for 16 November. Pick a new date, or carry on without one. 1 lesson is still to pass.");
        assert!(chip("es", "2026-12-10", 0, today, 4).is_none());
        assert!(chip("es", "", 5, today, 4).is_none());
    }
}
