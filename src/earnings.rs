//! What a learner has earned in a course, for the motivators that count in
//! units of their own (XP, #561), the day streak and the
//! reward line at the end of a run (#568).
//!
//! Earnings are worked out from the shared tables, never priced in advance:
//! effort points from `activity_log` (#558), lesson passes from
//! `lesson_passes` (passes from before #558 count too; effort before it was
//! never logged). A motivator therefore keeps counting while another is
//! worn, and switching to it shows everything already earned.
//!
//! [`between`] counts what was earned in a window of time as a [`Tally`]:
//! effort points, lessons passed, days whose daily goal was reached and
//! units finished in it. It reads the log for that window only, so a run's
//! tally ([`crate::motivators::RunOutcome`]) and [`today`] cost a day of
//! rows at most. The course total ([`total`]) is not added up from the log:
//! what grows without bound (the points ever logged, the goals ever met,
//! the streak) lives in one `earnings` row per learner and course, the
//! [`Ledger`], which [`log_effort`] moves with every activity recorded
//! (#589). Passes and units finished come from `lesson_passes`, one row per
//! lesson of the course at most. [`Rates`] turns a tally into a motivator's
//! own units.
//!
//! Rest days (#237) are the weekdays a learner does not study ([`Rest`],
//! `users.rest_days`, every course alike): a rest day without activity
//! keeps the streak going and is not counted in it ([`only_rest_between`]).

use std::collections::HashMap;

use rusqlite::{params, Connection, OptionalExtension};

use crate::activity::{Kind, DAILY_GOAL};
use crate::content::ContentStore;

/// What was earned in a window of time.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Tally {
    /// Effort points logged (`activity.rs` weights; not rounded).
    pub points: f64,
    /// Lessons that became passed.
    pub passes: i64,
    /// Days whose daily goal was reached in the window (the moment the day's
    /// points reached [`DAILY_GOAL`] fell in it).
    pub goals: i64,
    /// Units whose last lesson was passed in the window (every lesson of the
    /// unit passed).
    pub units: i64,
}

#[cfg(test)]
impl Tally {
    /// Nothing at all was earned.
    pub fn is_empty(&self) -> bool {
        self.points <= 0.0 && self.passes == 0 && self.goals == 0 && self.units == 0
    }
}

/// How many of a motivator's units each thing earns. Tunable: changing a
/// rate re-prices everything already earned too, since only what was done
/// is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rates {
    pub per_point: i64,
    pub per_pass: i64,
    pub per_goal: i64,
    pub per_unit: i64,
}

/// XP (#561): 10 per effort point, +50 per lesson passed, +100 per daily
/// goal met, +250 per unit finished.
pub const XP_RATES: Rates = Rates { per_point: 10, per_pass: 50, per_goal: 100, per_unit: 250 };

impl Rates {
    /// The tally in these units (effort rounded down after the rate).
    pub fn earned(&self, t: &Tally) -> i64 {
        (t.points * self.per_point as f64).floor() as i64
            + t.passes * self.per_pass
            + t.goals * self.per_goal
            + t.units * self.per_unit
    }
}

/// One local day's effort: the points logged that day before the window
/// opened, and those logged in it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DayEffort {
    pub before: f64,
    pub during: f64,
}

/// Everything [`tally`] needs, read by [`load`].
#[derive(Debug, Clone, Default)]
pub struct Facts {
    /// Effort per local day (day number = (at + tz) / 86400) touching the
    /// window.
    pub days: HashMap<i64, DayEffort>,
    /// When each passed lesson was passed.
    pub passed_at: HashMap<String, i64>,
    /// The course's units, each as its lessons' ids.
    pub units: Vec<Vec<String>>,
}

/// What was earned in `[from, to)`. Pure: everything comes in `facts`.
pub fn tally(facts: &Facts, from: i64, to: i64, goal: i64) -> Tally {
    let in_window = |at: i64| at >= from && at < to;
    let points = facts.days.values().map(|d| d.during).sum();
    let goals = facts
        .days
        .values()
        .filter(|d| d.during > 0.0 && (d.before.floor() as i64) < goal && ((d.before + d.during).floor() as i64) >= goal)
        .count() as i64;
    let passes = facts.passed_at.values().filter(|&&at| in_window(at)).count() as i64;
    let units = units_finished(&facts.units, &facts.passed_at).filter(|&last| in_window(last)).count() as i64;
    Tally { points, passes, goals, units }
}

/// When each unit of `units` was finished (its last lesson passed), of
/// those whose every lesson is in `passed_at`.
fn units_finished<'a>(units: &'a [Vec<String>], passed_at: &'a HashMap<String, i64>) -> impl Iterator<Item = i64> + 'a {
    units
        .iter()
        .filter(|lessons| !lessons.is_empty())
        .filter_map(move |lessons| lessons.iter().map(|id| passed_at.get(id).copied()).collect::<Option<Vec<i64>>>())
        .filter_map(|ats| ats.into_iter().max())
}

/// The course's units, each as its lessons' ids.
fn course_units(course: &ContentStore) -> Vec<Vec<String>> {
    let mut units: Vec<Vec<String>> = Vec::new();
    for l in course.lessons() {
        if units.len() <= l.unit_index {
            units.resize(l.unit_index + 1, Vec::new());
        }
        if let Some(u) = units.get_mut(l.unit_index) {
            u.push(l.id.clone());
        }
    }
    units
}

/// When each passed lesson of the course was passed (inside `Db::call`):
/// one row per lesson of the course at most.
fn passed_at(conn: &Connection, user_id: i64, course_id: &str) -> rusqlite::Result<HashMap<String, i64>> {
    let mut stmt = conn.prepare_cached(
        "SELECT lesson_id, passed_at FROM lesson_passes WHERE user_id = ?1 AND course_id = ?2 AND passed_at IS NOT NULL",
    )?;
    let rows = stmt.query_map(params![user_id, course_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
    rows.collect()
}

/// Read the facts for `[from, to)` (inside `Db::call`). `tz` is the
/// learner's offset from UTC in seconds, which decides where a day starts.
/// The log is read from the start of `from`'s day to `to` only.
pub fn load(conn: &Connection, user_id: i64, course: &ContentStore, tz: i64, from: i64, to: i64) -> rusqlite::Result<Facts> {
    let course_id = course.id();
    let day_start = crate::sprint::local_day_start(tz, from);
    let mut stmt = conn.prepare_cached(
        "SELECT (at + ?3) / 86400, at < ?5, kind, SUM(n) FROM activity_log
         WHERE user_id = ?1 AND course_id = ?2 AND at >= ?4 AND at < ?6
         GROUP BY 1, 2, 3",
    )?;
    let rows = stmt.query_map(params![user_id, course_id, tz, day_start, from, to], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, bool>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?))
    })?;
    let mut days: HashMap<i64, DayEffort> = HashMap::new();
    for row in rows {
        let (day, before, kind, n) = row?;
        let pts = Kind::ALL.into_iter().find(|k| k.key() == kind).map_or(0.0, Kind::points) * n as f64;
        let d = days.entry(day).or_default();
        if before {
            d.before += pts;
        } else {
            d.during += pts;
        }
    }
    Ok(Facts { days, passed_at: passed_at(conn, user_id, course_id)?, units: course_units(course) })
}

/// What was earned in `[from, to)` (inside `Db::call`).
pub fn between(conn: &Connection, user_id: i64, course: &ContentStore, tz: i64, from: i64, to: i64) -> rusqlite::Result<Tally> {
    Ok(tally(&load(conn, user_id, course, tz, from, to)?, from, to, DAILY_GOAL))
}

/// Everything earned in the course so far: the [`Ledger`]'s points and
/// goals, and the passes and units finished from `lesson_passes`.
pub fn total(conn: &Connection, user_id: i64, course: &ContentStore, tz: i64) -> rusqlite::Result<Tally> {
    let ledger = Ledger::load(conn, user_id, course.id(), tz)?;
    let passed = passed_at(conn, user_id, course.id())?;
    let units = units_finished(&course_units(course), &passed).count() as i64;
    Ok(Tally { points: ledger.points, passes: passed.len() as i64, goals: ledger.goals, units })
}

/// The local day number `at` falls on for an offset of `tz` seconds.
pub fn day_of(tz: i64, at: i64) -> i64 {
    (at + tz).div_euclid(86_400)
}

/// The weekday of local day number `day`, 0 for Sunday: day 0
/// (1970-01-01) was a Thursday.
pub fn weekday(day: i64) -> u8 {
    (day + 4).rem_euclid(7) as u8
}

/// The most rest days a week a learner may pick.
pub const MAX_REST_DAYS: u32 = 2;

/// Every day strictly between `from` and `to` is one of the weekdays in
/// `rest` (bit [`weekday`] set); true when there is none. Stops at the
/// first day that is not, so a long gap costs a few steps at most.
pub fn only_rest_between(from: i64, to: i64, rest: u8) -> bool {
    (from + 1..to).all(|d| rest & (1 << weekday(d)) != 0)
}

/// A learner's rest days (#237): `users.rest_days` and `rest_days_since`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rest {
    /// One bit a weekday ([`weekday`]), at most [`MAX_REST_DAYS`] set.
    pub mask: u8,
    /// The local day the mask was last changed on. No day before it ever
    /// counts as a rest day: a change takes effect from that day on, and a
    /// streak that had already ended stays ended ([`set_rest_days`]).
    pub since: i64,
}

impl Rest {
    /// The account's rest days (inside `Db::call`); none for an unknown one.
    pub fn load(conn: &Connection, user_id: i64) -> rusqlite::Result<Rest> {
        Ok(conn
            .prepare_cached("SELECT rest_days, rest_days_since FROM users WHERE id = ?1")?
            .query_row([user_id], |r| Ok(Rest { mask: r.get::<_, i64>(0)? as u8, since: r.get(1)? }))
            .optional()?
            .unwrap_or_default())
    }

    /// A streak active on day `from` carries on to day `to`: nothing lies
    /// between, or only rest days on or after `since`.
    pub fn bridges(&self, from: i64, to: i64) -> bool {
        from + 1 >= to || (from + 1 >= self.since && only_rest_between(from, to, self.mask))
    }

    /// The weekdays set, Monday first, as numbers ([`weekday`]).
    pub fn weekdays(&self) -> Vec<u8> {
        [1, 2, 3, 4, 5, 6, 0].into_iter().filter(|w| self.mask & (1 << w) != 0).collect()
    }

    /// The weekdays set by name, Monday first, in the learner's language.
    pub fn names(&self) -> Vec<String> {
        self.weekdays().into_iter().map(day_name).collect()
    }
}

/// The name of weekday `w` ([`weekday`]) in the learner's language.
pub fn day_name(w: u8) -> String {
    crate::t(match w {
        0 => "rest.day.sun",
        1 => "rest.day.mon",
        2 => "rest.day.tue",
        3 => "rest.day.wed",
        4 => "rest.day.thu",
        5 => "rest.day.fri",
        _ => "rest.day.sat",
    })
}

/// A learner's running totals in one course: the one `earnings` row (#589).
/// Everything that would otherwise need the whole log is here.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Ledger {
    /// Effort points ever logged.
    pub points: f64,
    /// Days whose daily goal was reached.
    pub goals: i64,
    /// The local day last active ([`day_of`]); None before any activity.
    pub day: Option<i64>,
    /// Effort points logged on `day`.
    pub day_points: f64,
    /// Days active in a row, ending on `day`; rest days between them
    /// without activity are skipped, not counted.
    pub streak: i64,
}

impl Ledger {
    /// The learner's row in the course (inside `Db::call`). A learner with
    /// activity from before the ledger existed (#589) gets it added up from
    /// their history once, here, and kept from then on; `tz` is their
    /// offset from UTC in seconds, which decides where those days start.
    pub fn load(conn: &Connection, user_id: i64, course_id: &str, tz: i64) -> rusqlite::Result<Ledger> {
        let row = conn
            .prepare_cached("SELECT points, goals, day, day_points, streak FROM earnings WHERE user_id = ?1 AND course_id = ?2")?
            .query_row(params![user_id, course_id], |r| {
                Ok(Ledger { points: r.get(0)?, goals: r.get(1)?, day: r.get(2)?, day_points: r.get(3)?, streak: r.get(4)? })
            })
            .optional()?;
        match row {
            Some(l) => Ok(l),
            None => {
                let l = backfill(conn, user_id, course_id, tz)?;
                l.save(conn, user_id, course_id)?;
                Ok(l)
            }
        }
    }

    fn save(&self, conn: &Connection, user_id: i64, course_id: &str) -> rusqlite::Result<()> {
        conn.prepare_cached(
            "INSERT INTO earnings (user_id, course_id, points, goals, day, day_points, streak) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT (user_id, course_id) DO UPDATE SET points = excluded.points, goals = excluded.goals,
                day = excluded.day, day_points = excluded.day_points, streak = excluded.streak",
        )?
        .execute(params![user_id, course_id, self.points, self.goals, self.day, self.day_points, self.streak])?;
        Ok(())
    }

    /// Log `pts` effort points on `day`: the totals, the day's points (and
    /// the goal if they reach it) and the streak move. The streak goes on
    /// when only `rest` days lie between. Pure, for the tests.
    pub fn add(&mut self, pts: f64, day: i64, rest: &Rest) {
        self.points += pts;
        match self.day {
            Some(d) if d == day => {}
            // A day from the future (the clock or the time zone moved back):
            // the points count, the day stays.
            Some(d) if d > day => return,
            Some(d) if rest.bridges(d, day) => {
                self.streak += 1;
                self.day = Some(day);
                self.day_points = 0.0;
            }
            _ => {
                self.streak = 1;
                self.day = Some(day);
                self.day_points = 0.0;
            }
        }
        let before = self.day_points;
        self.day_points += pts;
        if (before.floor() as i64) < DAILY_GOAL && (self.day_points.floor() as i64) >= DAILY_GOAL {
            self.goals += 1;
        }
    }

    /// The streak as of `today` (a day number) and whether today is in it:
    /// the streak ends on `day`, so it is current while that is today, or
    /// an earlier day with only `rest` days since, and 0 otherwise.
    pub fn streak_at(&self, today: i64, rest: &Rest) -> (i64, bool) {
        match self.day {
            Some(d) if d == today => (self.streak, true),
            Some(d) if d < today && rest.bridges(d, today) => (self.streak, false),
            _ => (0, false),
        }
    }

    /// How long the streak was, once it has ended (0 while it lasts, or
    /// with none): the home page says so, and that a new one starts today.
    pub fn ended_at(&self, today: i64, rest: &Rest) -> i64 {
        match self.day {
            Some(d) if d < today && self.streak_at(today, rest).0 == 0 => self.streak,
            _ => 0,
        }
    }

    /// The rest days are about to change on `today`: a streak still going
    /// over the `old` ones moves its last day to yesterday (a rest day with
    /// no points, so `day_points` is 0), so the days it has crossed are
    /// never looked at again. One that has ended is left as it is;
    /// [`Rest::since`] keeps it ended.
    fn carry(&mut self, today: i64, old: &Rest) {
        if let Some(d) = self.day {
            if d < today - 1 && old.bridges(d, today) {
                self.day = Some(today - 1);
                self.day_points = 0.0;
            }
        }
    }
}

/// Move the learner's [`Ledger`] for `pts` effort points logged at `at`
/// (inside `Db::call`, from `activity::record`).
pub fn log_effort(conn: &Connection, user_id: i64, course_id: &str, pts: f64, at: i64, tz: i64) -> rusqlite::Result<()> {
    let mut ledger = Ledger::load(conn, user_id, course_id, tz)?;
    ledger.add(pts, day_of(tz, at), &Rest::load(conn, user_id)?);
    ledger.save(conn, user_id, course_id)
}

/// The day streak and whether the learner practised today, from the ledger.
pub fn streak(conn: &Connection, user_id: i64, course_id: &str, tz: i64, now: i64) -> rusqlite::Result<(i64, bool)> {
    Ok(Ledger::load(conn, user_id, course_id, tz)?.streak_at(day_of(tz, now), &Rest::load(conn, user_id)?))
}

/// How long the learner's last streak was, if it has ended ([`Ledger::ended_at`]).
pub fn streak_ended(conn: &Connection, user_id: i64, course_id: &str, tz: i64, now: i64) -> rusqlite::Result<i64> {
    Ok(Ledger::load(conn, user_id, course_id, tz)?.ended_at(day_of(tz, now), &Rest::load(conn, user_id)?))
}

/// Store new rest days for the account (inside `Db::call`; `mask` already
/// checked). They take effect from today, the learner's day at `now`:
/// first every course's streak still going over the old rest days is
/// carried to yesterday ([`Ledger::carry`]), then `rest_days_since`
/// becomes today, so no day before it ever counts as a rest day. A streak
/// that had already ended can therefore never come back.
pub fn set_rest_days(conn: &Connection, user_id: i64, mask: u8, tz: i64, now: i64) -> rusqlite::Result<()> {
    let old = Rest::load(conn, user_id)?;
    if old.mask == mask {
        return Ok(());
    }
    let today = day_of(tz, now);
    let courses: Vec<String> = conn
        .prepare_cached("SELECT course_id FROM earnings WHERE user_id = ?1")?
        .query_map([user_id], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    for course_id in courses {
        let mut ledger = Ledger::load(conn, user_id, &course_id, tz)?;
        ledger.carry(today, &old);
        ledger.save(conn, user_id, &course_id)?;
    }
    conn.prepare_cached("UPDATE users SET rest_days = ?2, rest_days_since = ?3 WHERE id = ?1")?.execute(params![user_id, mask, today])?;
    Ok(())
}

/// The day streak as the ledger has it on record, for someone else looking
/// (the progress page on Manage Users, #147): nothing is added up from the
/// history and nothing is written, so a learner without a row has none.
/// `tz` is the viewer's offset, which decides what "today" is. The ledger's
/// days are the learner's, whose offset may be ahead of the viewer's: a
/// streak that ends on the viewer's tomorrow is current too. The learner's
/// rest days bridge a gap as on their own page.
pub fn streak_on_record(conn: &Connection, user_id: i64, course_id: &str, tz: i64, now: i64) -> rusqlite::Result<i64> {
    let row: Option<(Option<i64>, i64)> = conn
        .prepare_cached("SELECT day, streak FROM earnings WHERE user_id = ?1 AND course_id = ?2")?
        .query_row(params![user_id, course_id], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    let today = day_of(tz, now);
    let rest = Rest::load(conn, user_id)?;
    Ok(match row {
        Some((Some(day), streak)) if day <= today + 1 && (day >= today - 1 || rest.bridges(day, today)) => streak,
        _ => 0,
    })
}

/// A learner's ledger from their history: the one time the whole log is
/// read, for a learner from before #589 (nothing, for a new one). Points
/// and goals come from `activity_log`; the days active, and so the streak,
/// also from `quiz_history` and `game_activity`, which were all there was
/// before #558. The learner's rest days bridge gaps as in [`Ledger::add`].
fn backfill(conn: &Connection, user_id: i64, course_id: &str, tz: i64) -> rusqlite::Result<Ledger> {
    let mut stmt = conn.prepare_cached(
        "SELECT (at + ?3) / 86400, kind, SUM(n) FROM activity_log WHERE user_id = ?1 AND course_id = ?2 GROUP BY 1, 2",
    )?;
    let rows = stmt.query_map(params![user_id, course_id, tz], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))?;
    let mut days: HashMap<i64, f64> = HashMap::new();
    for row in rows {
        let (day, kind, n) = row?;
        *days.entry(day).or_default() += Kind::ALL.into_iter().find(|k| k.key() == kind).map_or(0.0, Kind::points) * n as f64;
    }
    let mut stmt = conn.prepare_cached(
        "SELECT DISTINCT d FROM (
            SELECT (answered_at + ?3) / 86400 AS d FROM quiz_history WHERE user_id = ?1 AND course_id = ?2
            UNION
            SELECT (played_at + ?3) / 86400 AS d FROM game_activity WHERE user_id = ?1 AND course_id = ?2
            UNION
            SELECT (at + ?3) / 86400 AS d FROM activity_log WHERE user_id = ?1 AND course_id = ?2
         ) ORDER BY d DESC LIMIT 400",
    )?;
    let active: Vec<i64> = stmt.query_map(params![user_id, course_id, tz], |r| r.get(0))?.collect::<Result<_, _>>()?;
    let day = active.first().copied();
    let rest = Rest::load(conn, user_id)?;
    let streak = if day.is_some() { 1 + active.windows(2).take_while(|w| matches!(w, [newer, older] if rest.bridges(*older, *newer))).count() as i64 } else { 0 };
    Ok(Ledger {
        points: days.values().sum(),
        goals: days.values().filter(|p| (p.floor() as i64) >= DAILY_GOAL).count() as i64,
        day,
        day_points: day.and_then(|d| days.get(&d).copied()).unwrap_or(0.0),
        streak,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    const DAY: i64 = 86_400;

    fn facts() -> Facts {
        Facts {
            days: HashMap::from([
                // Goal reached inside the window.
                (1, DayEffort { before: 12.0, during: 9.0 }),
                // Already reached before the window opened: not again.
                (2, DayEffort { before: 25.0, during: 3.0 }),
                // Not reached.
                (3, DayEffort { before: 0.0, during: 19.6 }),
            ]),
            passed_at: HashMap::from([("a".to_string(), 50), ("b".to_string(), 150), ("c".to_string(), 160), ("d".to_string(), 10)]),
            units: vec![vec!["a".into(), "b".into()], vec!["c".into(), "x".into()], vec!["d".into()], vec![]],
        }
    }

    #[test]
    fn a_tally_counts_points_passes_goals_and_units() {
        let t = tally(&facts(), 100, 200, 20);
        assert!((t.points - 31.6).abs() < 1e-9);
        assert_eq!((t.passes, t.goals, t.units), (2, 1, 1), "b and c passed; day 1's goal; unit a+b finished at 150");
        // Unit d finished at 10, before the window; c's unit still lacks x.
        let all = tally(&facts(), 0, 200, 20);
        assert_eq!((all.passes, all.units), (4, 2));
        assert!(tally(&Facts::default(), 0, 10, 20).is_empty());
    }

    #[test]
    fn rates_price_a_tally() {
        let t = Tally { points: 31.6, passes: 2, goals: 1, units: 1 };
        assert_eq!(XP_RATES.earned(&t), 316 + 100 + 100 + 250);
        assert_eq!(XP_RATES.earned(&Tally::default()), 0);
    }

    #[test]
    fn a_ledger_moves_with_each_days_effort() {
        let none = Rest::default();
        let mut l = Ledger::default();
        assert_eq!(l.streak_at(10, &none), (0, false));
        l.add(19.5, 10, &none);
        assert_eq!((l.points, l.goals, l.day, l.day_points, l.streak), (19.5, 0, Some(10), 19.5, 1));
        assert_eq!((l.streak_at(10, &none), l.streak_at(11, &none), l.streak_at(12, &none)), ((1, true), (1, false), (0, false)));
        // The goal is reached the moment the day's points (rounded down) get there, once.
        l.add(0.5, 10, &none);
        assert_eq!((l.goals, l.day_points), (1, 20.0));
        l.add(30.0, 10, &none);
        assert_eq!(l.goals, 1);
        // The next day continues the streak; a day missed starts over.
        l.add(1.0, 11, &none);
        assert_eq!((l.streak, l.day, l.day_points), (2, Some(11), 1.0));
        l.add(25.0, 13, &none);
        assert_eq!((l.streak, l.day, l.goals), (1, Some(13), 2));
        // A day before the last one (the clock moved back): points only.
        l.add(2.0, 12, &none);
        assert_eq!((l.streak, l.day, l.day_points, l.goals), (1, Some(13), 25.0, 2));
        assert!((l.points - 78.0).abs() < 1e-9);
    }

    // The days around 2026-11-19, a Thursday.
    const THU: i64 = 20_776;
    const WED: i64 = THU - 1;
    const FRI: i64 = THU + 1;
    const SAT: i64 = THU + 2;
    const SUN: i64 = THU + 3;
    const MON: i64 = THU + 4;

    /// Rest on the weekdays of these days, from always.
    fn rest(days: &[i64]) -> Rest {
        Rest { mask: days.iter().fold(0, |m, &d| m | 1 << weekday(d)), since: 0 }
    }

    #[test]
    fn the_weekday_of_a_known_date() {
        // 2026-11-19 00:00 UTC is day 20776, a Thursday; 2000-01-01 (day
        // 10957) was a Saturday; 1969-12-28 (day -4) a Sunday.
        assert_eq!(day_of(0, 1_795_046_400), THU);
        assert_eq!((weekday(THU), weekday(10_957), weekday(0), weekday(-4)), (4, 6, 4, 0));
        assert_eq!(rest(&[SUN, MON]).weekdays(), vec![1, 0], "Monday first");
    }

    #[test]
    fn only_rest_days_between_bridge_a_gap() {
        let fri_sat = rest(&[FRI, SAT]).mask;
        assert!(only_rest_between(THU, FRI, 0), "nothing lies between");
        assert!(only_rest_between(THU, SUN, fri_sat));
        assert!(!only_rest_between(THU, MON, fri_sat), "Sunday is not one");
        assert!(!only_rest_between(THU, SAT, 0));
        assert!(!only_rest_between(0, THU, fri_sat), "a long gap stops at its first study day");
    }

    #[test]
    fn a_streak_crosses_one_rest_day() {
        let r = rest(&[FRI]);
        let mut l = Ledger::default();
        l.add(5.0, THU, &r);
        // Friday off: the streak holds over it and is not counted up.
        assert_eq!((l.streak_at(FRI, &r), l.streak_at(SAT, &r)), ((1, false), (1, false)));
        l.add(5.0, SAT, &r);
        assert_eq!((l.streak, l.streak_at(SAT, &r)), (2, (2, true)));
        // Without the rest day the same days start over.
        let mut plain = Ledger::default();
        plain.add(5.0, THU, &Rest::default());
        plain.add(5.0, SAT, &Rest::default());
        assert_eq!(plain.streak, 1);
    }

    #[test]
    fn a_streak_crosses_two_rest_days_in_a_row() {
        let r = rest(&[FRI, SAT]);
        let mut l = Ledger::default();
        l.add(5.0, THU, &r);
        assert_eq!(l.streak_at(SUN, &r), (1, false));
        assert_eq!(l.streak_at(MON, &r), (0, false), "Sunday was missed");
        l.add(5.0, SUN, &r);
        assert_eq!((l.streak, l.ended_at(SUN, &r)), (2, 0));
    }

    #[test]
    fn a_rest_day_and_a_missed_day_end_the_streak() {
        let r = rest(&[FRI]);
        let mut l = Ledger::default();
        l.add(5.0, WED, &r);
        l.add(5.0, THU, &r);
        // Friday is a rest day, Saturday was missed: it has ended, two days long.
        assert_eq!((l.streak_at(SUN, &r), l.ended_at(SUN, &r)), ((0, false), 2));
        l.add(5.0, SUN, &r);
        assert_eq!((l.streak, l.ended_at(SUN, &r)), (1, 0), "a new one starts today");
    }

    #[test]
    fn points_on_a_rest_day_count() {
        let r = rest(&[FRI]);
        let mut l = Ledger::default();
        l.add(5.0, THU, &r);
        l.add(20.0, FRI, &r);
        assert_eq!((l.streak, l.goals, l.day, l.day_points), (2, 1, Some(FRI), 20.0), "an active rest day is a day of the streak");
        l.add(1.0, SAT, &r);
        assert_eq!(l.streak, 3);
        assert!((l.points - 26.0).abs() < 1e-9);
    }

    /// A new account, with no rest days.
    fn learner(c: &Connection) -> i64 {
        crate::auth::create_user(c, "ann@example.org", "h").unwrap()
    }

    #[test]
    fn changing_rest_days_never_brings_back_a_streak_that_ended() {
        let db = Db::open_in_memory().unwrap();
        db.with(|c| {
            let uid = learner(c);
            // Wednesday and Thursday, then Friday missed with no rest days.
            log_effort(c, uid, "es", 5.0, WED * DAY + 100, 0)?;
            log_effort(c, uid, "es", 5.0, THU * DAY + 100, 0)?;
            let sat = SAT * DAY + 100;
            assert_eq!((streak(c, uid, "es", 0, sat)?, streak_ended(c, uid, "es", 0, sat)?), ((0, false), 2));
            // On Saturday Friday becomes a rest day: the streak stays ended,
            // for the learner and for anyone looking at it.
            set_rest_days(c, uid, rest(&[FRI]).mask, 0, sat)?;
            assert_eq!(Rest::load(c, uid)?, Rest { mask: rest(&[FRI]).mask, since: SAT });
            assert_eq!((streak(c, uid, "es", 0, sat)?, streak_ended(c, uid, "es", 0, sat)?), ((0, false), 2));
            assert_eq!(streak_on_record(c, uid, "es", 0, sat)?, 0);
            log_effort(c, uid, "es", 5.0, sat, 0)?;
            assert_eq!(streak(c, uid, "es", 0, sat)?, (1, true), "a new one, not 3");
            // From then on Friday is a rest day like any other.
            log_effort(c, uid, "es", 5.0, (THU + 7) * DAY + 100, 0)?;
            log_effort(c, uid, "es", 5.0, (SAT + 7) * DAY + 100, 0)?;
            assert_eq!(streak(c, uid, "es", 0, (SAT + 7) * DAY + 200)?, (2, true));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn changing_rest_days_mid_streak_takes_effect_from_today() {
        let db = Db::open_in_memory().unwrap();
        db.with(|c| {
            let uid = learner(c);
            set_rest_days(c, uid, rest(&[FRI]).mask, 0, (THU - 7) * DAY)?;
            log_effort(c, uid, "es", 5.0, THU * DAY + 100, 0)?;
            log_effort(c, uid, "fr", 5.0, WED * DAY + 100, 0)?;
            // Saturday, Friday rested: the streak is going. Friday stops
            // being a rest day; the Friday already behind it stays one, in
            // every course.
            let sat = SAT * DAY + 100;
            assert_eq!(streak(c, uid, "es", 0, sat)?, (1, false));
            set_rest_days(c, uid, rest(&[SUN]).mask, 0, sat)?;
            assert_eq!(streak(c, uid, "es", 0, sat)?, (1, false));
            assert_eq!(streak_on_record(c, uid, "es", 0, sat)?, 1, "others see the same");
            assert_eq!(Ledger::load(c, uid, "es", 0)?.day, Some(FRI), "carried over the rest day it crossed");
            assert_eq!(streak(c, uid, "fr", 0, sat)?, (0, false), "fr, last active Wednesday, had ended and stays ended");
            // The new rest day, Sunday, counts from today on.
            log_effort(c, uid, "es", 5.0, sat, 0)?;
            log_effort(c, uid, "es", 5.0, MON * DAY + 100, 0)?;
            assert_eq!(streak(c, uid, "es", 0, MON * DAY + 200)?, (3, true));
            // Saving the same rest days again changes nothing.
            set_rest_days(c, uid, rest(&[SUN]).mask, 0, MON * DAY + 300)?;
            assert_eq!(Rest::load(c, uid)?.since, SAT);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn the_weekday_is_the_learners_own_at_midnight() {
        let db = Db::open_in_memory().unwrap();
        db.with(|c| {
            let uid = learner(c);
            let tz = -5 * 3600;
            set_rest_days(c, uid, rest(&[THU]).mask, tz, (THU - 7) * DAY)?;
            // Wednesday 20:00 and Friday 21:00 where the learner is: Thursday
            // and Saturday in UTC, with Friday (not a rest day) between.
            let wed = WED * DAY + 20 * 3600 - tz;
            let fri = FRI * DAY + 21 * 3600 - tz;
            assert_eq!((day_of(tz, wed), day_of(0, wed), day_of(tz, fri), day_of(0, fri)), (WED, THU, FRI, SAT));
            log_effort(c, uid, "es", 5.0, wed, tz)?;
            // Friday 20:00 local, before studying: Thursday rested, still going.
            assert_eq!(streak(c, uid, "es", tz, fri - 3600)?, (1, false));
            log_effort(c, uid, "es", 5.0, fri, tz)?;
            assert_eq!(streak(c, uid, "es", tz, fri)?, (2, true), "Thursday, the learner's rest day, bridged");
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn earnings_come_from_the_shared_tables() {
        // Units A1 (l0, l1) and A2 (l2).
        let lessons = [
            r#"{"id":"l0","slug":"l0","title":"L0","unit":"A1","content":"x"}"#,
            r#"{"id":"l1","slug":"l1","title":"L1","unit":"A1","content":"x"}"#,
            r#"{"id":"l2","slug":"l2","title":"L2","unit":"A2","content":"x"}"#,
        ];
        let course = crate::content::tests::store(&lessons).unwrap();
        let db = Db::open_in_memory().unwrap();
        db.with(|c| {
            let uid = crate::auth::create_user(c, "ann@example.org", "h").unwrap();
            let log = |kind: Kind, n: i64, at: i64| crate::activity::record_at(c, uid, course.id(), kind, n, at, 0);
            let pass = |id: &str, at: i64| {
                c.execute(
                    "INSERT INTO lesson_passes (user_id, course_id, lesson_id, seen_at, passed_at, best_pct, updated_at) VALUES (?1, ?2, ?3, ?4, ?4, 100, ?4)",
                    params![uid, course.id(), id, at],
                )
            };
            // Day 10: 15 answers in the morning, 5 readings (20 points) in the afternoon.
            log(Kind::Answer, 15, 10 * DAY + 100)?;
            log(Kind::Reading, 5, 10 * DAY + 5000)?;
            // Day 11: 2 points; another course's activity never counts.
            log(Kind::Answer, 2, 11 * DAY + 100)?;
            crate::activity::record_at(c, uid, "other", Kind::Answer, 99, 11 * DAY, 0)?;
            // A pass from before the log existed still counts.
            pass("l2", 5)?;
            pass("l0", 10 * DAY + 200)?;
            pass("l1", 10 * DAY + 5000)?;

            let all = between(c, uid, &course, 0, 0, 12 * DAY)?;
            assert_eq!(all, Tally { points: 37.0, passes: 3, goals: 1, units: 2 });
            // The afternoon's run: it reached the goal and finished unit A.
            let run = between(c, uid, &course, 0, 10 * DAY + 4000, 10 * DAY + 6000)?;
            assert_eq!(run, Tally { points: 20.0, passes: 1, goals: 1, units: 1 });
            assert_eq!(XP_RATES.earned(&run), 200 + 50 + 100 + 250);
            // Day 11 alone, in a time zone an hour ahead.
            let day = between(c, uid, &course, 3600, 11 * DAY - 3600, 12 * DAY - 3600)?;
            assert_eq!(day, Tally { points: 2.0, ..Tally::default() });
            // The total is the ledger's points and goals plus the passes: the
            // same as adding the log up.
            assert_eq!(total(c, uid, &course, 0)?, all);
            assert_eq!(Ledger::load(c, uid, course.id(), 0)?, Ledger { points: 37.0, goals: 1, day: Some(11), day_points: 2.0, streak: 2 });
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn a_learner_from_before_the_ledger_gets_it_from_their_history_once() {
        let db = Db::open_in_memory().unwrap();
        db.with(|c| {
            let uid = crate::auth::create_user(c, "ann@example.org", "h").unwrap();
            // Rows written before the ledger existed: the log, and answers
            // and rounds from before the log (#558).
            c.execute(
                "INSERT INTO activity_log (user_id, course_id, kind, n, at) VALUES (?1, 'es', 'answer', 25, ?2), (?1, 'es', 'reading', 1, ?3), (?1, 'es', 'answer', 3, ?4)",
                params![uid, 10 * DAY + 100, 12 * DAY + 100, 13 * DAY + 100],
            )?;
            c.execute(
                "INSERT INTO quiz_history (user_id, course_id, lesson_id, question_id, selected_answer, was_correct, answered_at) VALUES (?1, 'es', 'l', 'q', 'a', 1, ?2)",
                params![uid, 11 * DAY + 100],
            )?;
            c.execute(
                "INSERT INTO game_activity (user_id, course_id, game, board, label, duration_ms, total, correct, played_at) VALUES (?1, 'es', 'g', 'b', 'l', 1, 1, 1, ?2)",
                params![uid, 8 * DAY + 100],
            )?;
            assert_eq!(c.query_row("SELECT COUNT(*) FROM earnings", [], |r| r.get::<_, i64>(0))?, 0);
            // Days 8, 10, 11, 12, 13 active: a streak of 4 ending on day 13;
            // 32 points, day 10 reached the goal.
            let l = Ledger::load(c, uid, "es", 0)?;
            assert_eq!(l, Ledger { points: 32.0, goals: 1, day: Some(13), day_points: 3.0, streak: 4 });
            assert_eq!(streak(c, uid, "es", 0, 13 * DAY + 500)?, (4, true));
            assert_eq!(streak(c, uid, "es", 0, 14 * DAY + 500)?, (4, false));
            assert_eq!(streak(c, uid, "es", 0, 15 * DAY + 500)?, (0, false));
            // From now on the row is what counts: history added behind its
            // back is not read again.
            c.execute("INSERT INTO activity_log (user_id, course_id, kind, n, at) VALUES (?1, 'es', 'answer', 50, ?2)", params![uid, 14 * DAY])?;
            assert_eq!(Ledger::load(c, uid, "es", 0)?, l);
            // Another course, and a new learner, start from nothing.
            assert_eq!(Ledger::load(c, uid, "fr", 0)?, Ledger::default());
            assert_eq!(c.query_row("SELECT COUNT(*) FROM earnings", [], |r| r.get::<_, i64>(0))?, 2);
            Ok(())
        })
        .unwrap();
    }
}
