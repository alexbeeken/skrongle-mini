//! Daily work (mini #221, docs/features.md "Daily work"): a teacher's plan of
//! activities for one course on one day, and each student's progress
//! through it. Data and rules only; the pages are later issues (#222 the
//! Plan tab, #223 and #224 the student's day, #225 the progress table).
//!
//! A plan (`daily_plans`) belongs to one course and one day: a whole number
//! of days since 1 January 1970 on the class clock ([`today`], the box's one
//! time zone, `settings::class_tz`). It holds up to [`MAX_ACTIVITIES`]
//! ordered activities (`daily_activities`): study, review or play over
//! lessons of the course. Students (the ones the course is given to,
//! [`students_of`]) see a plan once it is assigned and do its activities in
//! order; their progress is one `daily_done` row per activity, keyed by the
//! activity's id, so moving an activity keeps it and removing one removes it.
//!
//! Every function takes a `&Connection`: call it inside one `Db::call`. One
//! that needs "today" takes the current time (`now`, seconds) so tests can
//! fix it. A rule broken is `Ok(false)` (or nothing) with nothing changed.

// The pages that call these come later (#222 to #225); until then only the
// tests and the housekeeping do.
#![cfg_attr(not(test), allow(dead_code))]

use std::collections::HashMap;

use rusqlite::{params, Connection, OptionalExtension};

use crate::content::ContentStore;

/// Activities one plan may hold.
pub const MAX_ACTIVITIES: usize = 20;
/// The most times through a `play` may ask for (0 = no limit).
pub const MAX_ROUNDS: i64 = 5;
/// The games a `play` may offer, in the order they are stored: Word Match
/// and Flashcards.
pub const GAMES: [&str; 2] = ["matching", "flashcards"];
/// A plan may be made or changed from today up to this many days after it.
pub const PLAN_AHEAD_DAYS: i64 = 28;
/// Days "Plan my week" looks at, today first.
pub const AUTO_DAYS: i64 = 7;
/// `daily_done.mark` of a started `study` whose lesson the student read
/// through the stop: only then may it be finished ([`mark_studied`]).
pub const STUDIED: i64 = -1;
/// `daily_done.mark` of a started `review` whose Review round the student
/// finished: only then may it be finished ([`mark_reviewed`]).
pub const REVIEWED: i64 = -2;

/// What an activity asks a student to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Study,
    Review,
    Play,
}

impl Kind {
    /// The name stored in `daily_activities.kind`.
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Study => "study",
            Kind::Review => "review",
            Kind::Play => "play",
        }
    }

    /// The kind a stored or posted name stands for.
    pub fn parse(s: &str) -> Option<Kind> {
        match s {
            "study" => Some(Kind::Study),
            "review" => Some(Kind::Review),
            "play" => Some(Kind::Play),
            _ => None,
        }
    }
}

/// One step of a plan.
#[derive(Debug, Clone, PartialEq)]
pub struct Activity {
    /// The stop's id; never reused.
    pub id: i64,
    /// 0, 1, 2 … in the plan, no gaps.
    pub position: i64,
    pub kind: Kind,
    /// Lesson ids, in the course's order.
    pub lessons: Vec<String>,
    /// For a `play`: the games offered, from [`GAMES`].
    pub games: Vec<String>,
    /// For a `play`: times through; 0 = no limit.
    pub rounds: i64,
}

/// One course's plan for one day.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub id: i64,
    pub course_id: String,
    pub day: i64,
    /// Students see it.
    pub assigned: bool,
    /// By position.
    pub activities: Vec<Activity>,
}

/// Where a student stands on one activity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopState {
    Done,
    /// The first activity not done: the one to do now.
    Next,
    /// After the next one.
    Locked,
}

/// One activity as a student sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct Stop {
    pub activity: Activity,
    pub state: StopState,
    /// The student started it (a `daily_done` row).
    pub started: bool,
    /// The work its kind asks for is recorded: a study's lesson read
    /// ([`STUDIED`]), a review's round finished ([`REVIEWED`]). Always false
    /// for a play, which counts [`Stop::plays`].
    pub marked: bool,
    /// Times through a `play`.
    pub plays: i64,
}

/// One student's state on one activity, as the teacher sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cell {
    Done,
    /// Started, not finished.
    Doing,
    NotStarted,
}

/// One student's row of the teacher's progress table.
#[derive(Debug, Clone, PartialEq)]
pub struct StudentRow {
    pub student_id: i64,
    /// First and last name, else the username.
    pub name: String,
    /// One per activity, in order.
    pub cells: Vec<Cell>,
}

/// What one day of the teacher's week row says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DayState {
    /// No plan.
    Nothing,
    /// A plan not assigned yet, with this many activities.
    Draft(usize),
    /// An assigned plan of today or later, with this many activities.
    Assigned(usize),
    /// An assigned plan of a day gone by: students who finished every
    /// activity, of the students given the course.
    Past { finished: usize, students: usize },
}

/// One day of the teacher's week row ([`week`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeekDay {
    pub day: i64,
    pub state: DayState,
    /// Today up to [`PLAN_AHEAD_DAYS`] after: its plan may be changed.
    pub plannable: bool,
}

/// One day, today or later, whose plan uses a lesson ([`lesson_days`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LessonDay {
    pub day: i64,
    /// Students see the plan.
    pub assigned: bool,
    /// The plan's activities that use the lesson, by position: the kind and
    /// how many lessons the activity has.
    pub activities: Vec<(Kind, usize)>,
}

/// An earlier assigned day as a student sees it ([`earlier_days`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EarlierDay {
    pub day: i64,
    /// Activities the plan has.
    pub activities: i64,
    /// Of those, the ones the student has not finished.
    pub left: i64,
}

fn to_json(list: &[String]) -> String {
    serde_json::to_string(list).unwrap_or_else(|_| "[]".to_string())
}

fn from_json(text: &str) -> Vec<String> {
    serde_json::from_str(text).unwrap_or_default()
}

/// Runs `f` in a transaction, or inside the caller's when there is one.
fn in_tx<T>(conn: &Connection, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> rusqlite::Result<T> {
    if !conn.is_autocommit() {
        return f(conn);
    }
    let tx = conn.unchecked_transaction()?;
    let out = f(&tx)?;
    tx.commit()?;
    Ok(out)
}

// --------------------------------------------------------------------
// The class clock
// --------------------------------------------------------------------

/// Today on the class clock at `now`: days since 1 January 1970 in the box's
/// one time zone (`settings::class_tz`, UTC until set).
pub fn today(conn: &Connection, now: i64) -> rusqlite::Result<i64> {
    Ok(crate::earnings::day_of(crate::settings::class_tz_secs(conn)?, now))
}

/// A browser's offset from UTC in seconds (as `CurrentUser::tz`, from the
/// `tz` cookie) as the class clock stores it: half hours, the nearest.
/// `None` outside `settings::CLASS_TZ_RANGE`.
pub fn half_hours_of(offset_secs: i64) -> Option<i64> {
    Some((offset_secs + 900).div_euclid(1800)).filter(|h| crate::settings::CLASS_TZ_RANGE.contains(h))
}

/// A plan of `day` may be made or changed: today up to [`PLAN_AHEAD_DAYS`] after.
pub fn plannable(day: i64, today: i64) -> bool {
    (today..=today + PLAN_AHEAD_DAYS).contains(&day)
}

fn plannable_at(conn: &Connection, day: i64, now: i64) -> rusqlite::Result<bool> {
    Ok(plannable(day, today(conn, now)?))
}

/// The date of a day number: `(year, month 1–12, day of month, weekday 0 = Sunday)`.
pub fn civil(day: i64) -> (i64, i64, i64, i64) {
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = day + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    let weekday = (day + 4).rem_euclid(7);
    (y, m, d, weekday)
}

/// Monday to Friday (day 0, 1 January 1970, was a Thursday).
pub fn school_day(day: i64) -> bool {
    (1..=5).contains(&(day + 4).rem_euclid(7))
}

/// The Monday of the day's week (Monday to Sunday).
pub fn week_start(day: i64) -> i64 {
    day - (day + 3).rem_euclid(7)
}

// --------------------------------------------------------------------
// Reading plans
// --------------------------------------------------------------------

fn activities_of(conn: &Connection, plan_id: i64) -> rusqlite::Result<Vec<Activity>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, position, kind, lessons, games, rounds FROM daily_activities WHERE plan_id = ?1 ORDER BY position, id",
    )?;
    let rows = stmt.query_map([plan_id], |r| {
        Ok(Activity {
            id: r.get(0)?,
            position: r.get(1)?,
            kind: Kind::parse(&r.get::<_, String>(2)?).unwrap_or(Kind::Study),
            lessons: from_json(&r.get::<_, String>(3)?),
            games: from_json(&r.get::<_, String>(4)?),
            rounds: r.get(5)?,
        })
    })?;
    rows.collect()
}

fn plan_row(conn: &Connection, course_id: &str, day: i64) -> rusqlite::Result<Option<(i64, bool)>> {
    conn.query_row(
        "SELECT id, assigned_at IS NOT NULL FROM daily_plans WHERE course_id = ?1 AND day = ?2",
        params![course_id, day],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
}

/// The course's plan for the day, assigned or not, activities by position.
pub fn plan(conn: &Connection, course_id: &str, day: i64) -> rusqlite::Result<Option<Plan>> {
    let Some((id, assigned)) = plan_row(conn, course_id, day)? else { return Ok(None) };
    Ok(Some(Plan { id, course_id: course_id.to_string(), day, assigned, activities: activities_of(conn, id)? }))
}

/// The plan an activity belongs to, assigned or not (a stop's page finds
/// its course and day from it).
pub fn plan_of_activity(conn: &Connection, activity_id: i64) -> rusqlite::Result<Option<Plan>> {
    let row: Option<(i64, String, i64, bool)> = conn
        .query_row(
            "SELECT p.id, p.course_id, p.day, p.assigned_at IS NOT NULL FROM daily_activities a JOIN daily_plans p ON p.id = a.plan_id
             WHERE a.id = ?1",
            [activity_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    let Some((id, course_id, day, assigned)) = row else { return Ok(None) };
    Ok(Some(Plan { id, course_id, day, assigned, activities: activities_of(conn, id)? }))
}

/// The earliest day the course has a plan for, assigned or not (the week
/// row's ‹ link).
pub fn first_plan_day(conn: &Connection, course_id: &str) -> rusqlite::Result<Option<i64>> {
    conn.query_row("SELECT MIN(day) FROM daily_plans WHERE course_id = ?1", [course_id], |r| r.get(0))
}

/// The plan and the activity, when the activity is in that course's plan for that day.
fn plan_with(conn: &Connection, course_id: &str, day: i64, activity_id: i64) -> rusqlite::Result<Option<(Plan, Activity)>> {
    let Some(p) = plan(conn, course_id, day)? else { return Ok(None) };
    let Some(a) = p.activities.iter().find(|a| a.id == activity_id).cloned() else { return Ok(None) };
    Ok(Some((p, a)))
}

/// The students of the class: the student accounts the course's owner gave
/// it to (`students::give`), with a display name (first and last name, else
/// the username), by name.
pub fn students_of(conn: &Connection, course_id: &str) -> rusqlite::Result<Vec<(i64, String)>> {
    let mut stmt = conn.prepare_cached(
        "SELECT u.id, COALESCE(u.username, ''), u.first_name, u.last_name FROM course_shares s
           JOIN courses c ON c.id = s.course_id
           JOIN users u ON u.id = s.user_id AND u.teacher_id = c.owner_id
         WHERE s.course_id = ?1 AND s.given = 1",
    )?;
    let rows = stmt.query_map([course_id], |r| {
        let (id, username, first, last): (i64, String, String, String) = (r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?);
        let name = format!("{first} {last}").trim().to_string();
        Ok((id, if name.is_empty() { username } else { name }))
    })?;
    let mut out = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    out.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()).then(a.0.cmp(&b.0)));
    Ok(out)
}

fn has_course(conn: &Connection, user_id: i64, course_id: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM course_shares WHERE course_id = ?1 AND user_id = ?2 AND given = 1)",
        params![course_id, user_id],
        |r| r.get(0),
    )
}

/// The activity's course is given to the user ([`has_course`]): the check
/// every student function makes before it reads or records a stop.
fn has_activity(conn: &Connection, user_id: i64, activity_id: i64) -> rusqlite::Result<bool> {
    let course: Option<String> = conn
        .query_row(
            "SELECT p.course_id FROM daily_activities a JOIN daily_plans p ON p.id = a.plan_id WHERE a.id = ?1",
            [activity_id],
            |r| r.get(0),
        )
        .optional()?;
    match course {
        Some(course_id) => has_course(conn, user_id, &course_id),
        None => Ok(false),
    }
}

/// The games of `games` that `lessons` still allow ([`games_for`]), in
/// their stored order.
fn games_kept(course: &ContentStore, lessons: &[String], games: &[String]) -> Vec<String> {
    let allowed = games_for(course, lessons);
    games.iter().filter(|g| allowed.contains(&g.as_str())).cloned().collect()
}

// --------------------------------------------------------------------
// The teacher's plan
// --------------------------------------------------------------------

fn touch(conn: &Connection, plan_id: i64) -> rusqlite::Result<()> {
    conn.execute("UPDATE daily_plans SET updated_at = ?2 WHERE id = ?1", params![plan_id, crate::db::now()])?;
    Ok(())
}

/// Positions 0..n again, in the order given.
fn renumber(conn: &Connection, ids: &[i64]) -> rusqlite::Result<()> {
    for (pos, id) in ids.iter().enumerate() {
        conn.execute("UPDATE daily_activities SET position = ?2 WHERE id = ?1", params![id, pos as i64])?;
    }
    Ok(())
}

/// The first lesson, in the course's order, that no `study` of this course
/// on this day or an earlier one has; `None` when every one is taken.
pub fn fresh_lesson(conn: &Connection, course: &ContentStore, course_id: &str, day: i64) -> rusqlite::Result<Option<String>> {
    let mut stmt = conn.prepare_cached(
        "SELECT a.lessons FROM daily_activities a JOIN daily_plans p ON p.id = a.plan_id
         WHERE p.course_id = ?1 AND p.day <= ?2 AND a.kind = 'study'",
    )?;
    let mut taken: Vec<String> = Vec::new();
    for lessons in stmt.query_map(params![course_id, day], |r| r.get::<_, String>(0))? {
        taken.extend(from_json(&lessons?));
    }
    Ok(course.lessons().iter().find(|l| !taken.contains(&l.id)).map(|l| l.id.clone()))
}

/// The lesson a new activity takes: [`fresh_lesson`], or the first lesson
/// when every one is taken; `None` for a course with no lessons.
pub fn next_lesson(conn: &Connection, course: &ContentStore, course_id: &str, day: i64) -> rusqlite::Result<Option<String>> {
    Ok(fresh_lesson(conn, course, course_id, day)?.or_else(|| course.lessons().first().map(|l| l.id.clone())))
}

/// The games a `play` over these lessons can offer: Word Match when one of
/// them has enough words for a board (`games::MIN_WORDS`), Flashcards when
/// one has vocabulary.
pub fn games_for(course: &ContentStore, lessons: &[String]) -> Vec<&'static str> {
    let found: Vec<_> = lessons.iter().filter_map(|id| course.lesson_by_id(id)).collect();
    let mut games = Vec::new();
    if found.iter().any(|l| l.vocabulary.len() >= crate::games::MIN_WORDS) {
        games.push("matching");
    }
    if found.iter().any(|l| !l.vocabulary.is_empty()) {
        games.push("flashcards");
    }
    games
}

fn ensure_plan(conn: &Connection, course_id: &str, day: i64) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT OR IGNORE INTO daily_plans (course_id, day, updated_at) VALUES (?1, ?2, ?3)",
        params![course_id, day, crate::db::now()],
    )?;
    conn.query_row("SELECT id FROM daily_plans WHERE course_id = ?1 AND day = ?2", params![course_id, day], |r| r.get(0))
}

/// Inserts an activity at `pos` (positions at and after it move up one). A
/// `play` starts with every game its lesson allows and no rounds.
fn insert_activity(conn: &Connection, course: &ContentStore, plan_id: i64, pos: usize, kind: Kind, lesson: &str) -> rusqlite::Result<()> {
    let lessons = vec![lesson.to_string()];
    let games: Vec<String> =
        if kind == Kind::Play { games_for(course, &lessons).into_iter().map(str::to_string).collect() } else { Vec::new() };
    conn.execute(
        "UPDATE daily_activities SET position = position + 1 WHERE plan_id = ?1 AND position >= ?2",
        params![plan_id, pos as i64],
    )?;
    conn.execute(
        "INSERT INTO daily_activities (plan_id, position, kind, lessons, games, rounds) VALUES (?1, ?2, ?3, ?4, ?5, 0)",
        params![plan_id, pos as i64, kind.as_str(), to_json(&lessons), to_json(&games)],
    )?;
    Ok(())
}

/// Adds an activity over [`next_lesson`] at position `at` (the end when
/// `None` or past it), making the plan (a draft) when there is none. False
/// when the day is not [`plannable`], the plan is full or the course has no
/// lessons.
pub fn add(conn: &Connection, course: &ContentStore, course_id: &str, day: i64, kind: Kind, at: Option<usize>, now: i64) -> rusqlite::Result<bool> {
    if !plannable_at(conn, day, now)? {
        return Ok(false);
    }
    in_tx(conn, |conn| {
        let existing = plan(conn, course_id, day)?;
        let n = existing.as_ref().map_or(0, |p| p.activities.len());
        if n >= MAX_ACTIVITIES {
            return Ok(false);
        }
        let Some(lesson) = next_lesson(conn, course, course_id, day)? else { return Ok(false) };
        let plan_id = match existing {
            Some(p) => p.id,
            None => ensure_plan(conn, course_id, day)?,
        };
        insert_activity(conn, course, plan_id, at.filter(|&a| a <= n).unwrap_or(n), kind, &lesson)?;
        touch(conn, plan_id)?;
        Ok(true)
    })
}

/// Moves an activity to position `to` (the end when past it). Every
/// student's progress on it moves with it.
pub fn move_to(conn: &Connection, course_id: &str, day: i64, activity_id: i64, to: usize, now: i64) -> rusqlite::Result<bool> {
    if !plannable_at(conn, day, now)? {
        return Ok(false);
    }
    in_tx(conn, |conn| {
        let Some((p, _)) = plan_with(conn, course_id, day, activity_id)? else { return Ok(false) };
        let mut ids: Vec<i64> = p.activities.iter().map(|a| a.id).filter(|&id| id != activity_id).collect();
        ids.insert(to.min(ids.len()), activity_id);
        renumber(conn, &ids)?;
        touch(conn, p.id)?;
        Ok(true)
    })
}

/// Removes an activity, and every student's progress on it.
pub fn remove(conn: &Connection, course_id: &str, day: i64, activity_id: i64, now: i64) -> rusqlite::Result<bool> {
    if !plannable_at(conn, day, now)? {
        return Ok(false);
    }
    in_tx(conn, |conn| {
        let Some((p, _)) = plan_with(conn, course_id, day, activity_id)? else { return Ok(false) };
        conn.execute("DELETE FROM daily_activities WHERE id = ?1", [activity_id])?;
        let ids: Vec<i64> = p.activities.iter().map(|a| a.id).filter(|&id| id != activity_id).collect();
        renumber(conn, &ids)?;
        touch(conn, p.id)?;
        Ok(true)
    })
}

/// Sets an activity's lessons: lessons of the course only, exactly one for a
/// `study`, one or more for a `review` or `play`. Duplicates are dropped and
/// the order is the course's. A `play` loses games the lessons no longer
/// allow. Progress on it is kept.
pub fn set_lessons(
    conn: &Connection,
    course: &ContentStore,
    course_id: &str,
    day: i64,
    activity_id: i64,
    lessons: &[String],
    now: i64,
) -> rusqlite::Result<bool> {
    if !plannable_at(conn, day, now)? {
        return Ok(false);
    }
    let Some((p, a)) = plan_with(conn, course_id, day, activity_id)? else { return Ok(false) };
    if lessons.iter().any(|id| course.lesson_by_id(id).is_none()) {
        return Ok(false);
    }
    let ordered: Vec<String> = course.lessons().iter().filter(|l| lessons.contains(&l.id)).map(|l| l.id.clone()).collect();
    let fits = match a.kind {
        Kind::Study => ordered.len() == 1,
        Kind::Review | Kind::Play => !ordered.is_empty(),
    };
    if !fits {
        return Ok(false);
    }
    let games = games_kept(course, &ordered, &a.games);
    conn.execute("UPDATE daily_activities SET lessons = ?2, games = ?3 WHERE id = ?1", params![activity_id, to_json(&ordered), to_json(&games)])?;
    touch(conn, p.id)?;
    Ok(true)
}

/// Turns one game of a `play` on or off; only games its lessons allow.
#[allow(clippy::too_many_arguments)]
pub fn set_game(
    conn: &Connection,
    course: &ContentStore,
    course_id: &str,
    day: i64,
    activity_id: i64,
    game: &str,
    on: bool,
    now: i64,
) -> rusqlite::Result<bool> {
    if !plannable_at(conn, day, now)? {
        return Ok(false);
    }
    let Some((p, a)) = plan_with(conn, course_id, day, activity_id)? else { return Ok(false) };
    if a.kind != Kind::Play || !games_for(course, &a.lessons).contains(&game) {
        return Ok(false);
    }
    let games: Vec<String> =
        GAMES.iter().filter(|g| if **g == game { on } else { a.games.iter().any(|x| x == *g) }).map(|g| g.to_string()).collect();
    conn.execute("UPDATE daily_activities SET games = ?2 WHERE id = ?1", params![activity_id, to_json(&games)])?;
    touch(conn, p.id)?;
    Ok(true)
}

/// Sets a `play`'s times through, `0..=MAX_ROUNDS` (0 = no limit).
pub fn set_rounds(conn: &Connection, course_id: &str, day: i64, activity_id: i64, rounds: i64, now: i64) -> rusqlite::Result<bool> {
    if !plannable_at(conn, day, now)? {
        return Ok(false);
    }
    let Some((p, a)) = plan_with(conn, course_id, day, activity_id)? else { return Ok(false) };
    if a.kind != Kind::Play || !(0..=MAX_ROUNDS).contains(&rounds) {
        return Ok(false);
    }
    conn.execute("UPDATE daily_activities SET rounds = ?2 WHERE id = ?1", params![activity_id, rounds])?;
    touch(conn, p.id)?;
    Ok(true)
}

/// The recommended day: fills a day with no activities with a study, a
/// review and a play, all over [`next_lesson`]. A draft until assigned.
pub fn recommended(conn: &Connection, course: &ContentStore, course_id: &str, day: i64, now: i64) -> rusqlite::Result<bool> {
    if !plannable_at(conn, day, now)? {
        return Ok(false);
    }
    in_tx(conn, |conn| {
        if plan(conn, course_id, day)?.is_some_and(|p| !p.activities.is_empty()) {
            return Ok(false);
        }
        let Some(lesson) = next_lesson(conn, course, course_id, day)? else { return Ok(false) };
        let plan_id = ensure_plan(conn, course_id, day)?;
        for (pos, kind) in [Kind::Study, Kind::Review, Kind::Play].into_iter().enumerate() {
            insert_activity(conn, course, plan_id, pos, kind, &lesson)?;
        }
        touch(conn, plan_id)?;
        Ok(true)
    })
}

/// Shows the plan to the students. False for a plan with no activities or
/// a day not [`plannable`]. Assigning again changes nothing.
pub fn assign(conn: &Connection, course_id: &str, day: i64, now: i64) -> rusqlite::Result<bool> {
    if !plannable_at(conn, day, now)? {
        return Ok(false);
    }
    let Some(p) = plan(conn, course_id, day)? else { return Ok(false) };
    if p.activities.is_empty() {
        return Ok(false);
    }
    conn.execute("UPDATE daily_plans SET assigned_at = COALESCE(assigned_at, ?2), updated_at = ?2 WHERE id = ?1", params![p.id, now])?;
    Ok(true)
}

/// Clears the day: deletes its plan, its activities and everyone's progress
/// on them. False when there was none or the day is not [`plannable`].
pub fn clear(conn: &Connection, course_id: &str, day: i64, now: i64) -> rusqlite::Result<bool> {
    if !plannable_at(conn, day, now)? {
        return Ok(false);
    }
    Ok(conn.execute("DELETE FROM daily_plans WHERE course_id = ?1 AND day = ?2", params![course_id, day])? > 0)
}

/// "Plan my week", in one transaction: each of the [`AUTO_DAYS`] days from
/// today that is a [`school_day`] and has no plan row at all (a draft, even
/// an empty one, is a plan; a cleared day has none) gets the
/// [`recommended`] day, assigned at `now`. Days go in date order, so each
/// takes the lesson after the one before. It stops at the first such day
/// with no [`fresh_lesson`] left: no lesson is planned a second time.
/// Returns the days filled, in order, and whether it stopped for want of a
/// lesson.
pub fn auto_week(conn: &Connection, course: &ContentStore, course_id: &str, now: i64) -> rusqlite::Result<(Vec<i64>, bool)> {
    let from = today(conn, now)?;
    in_tx(conn, |conn| {
        let mut filled = Vec::new();
        for day in from..from + AUTO_DAYS {
            if !school_day(day) || plan_row(conn, course_id, day)?.is_some() {
                continue;
            }
            if fresh_lesson(conn, course, course_id, day)?.is_none() {
                return Ok((filled, true));
            }
            if recommended(conn, course, course_id, day, now)? && assign(conn, course_id, day, now)? {
                filled.push(day);
            }
        }
        Ok((filled, false))
    })
}

/// The course's plans from today on (class clock at `now`), by day.
fn plans_from_today(conn: &Connection, course_id: &str, now: i64) -> rusqlite::Result<Vec<Plan>> {
    let from = today(conn, now)?;
    let mut stmt = conn.prepare_cached("SELECT day FROM daily_plans WHERE course_id = ?1 AND day >= ?2 ORDER BY day")?;
    let days = stmt.query_map(params![course_id, from], |r| r.get::<_, i64>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut out = Vec::new();
    for day in days {
        out.extend(plan(conn, course_id, day)?);
    }
    Ok(out)
}

/// A lesson's plans: every day from today on, in order, whose plan,
/// assigned or not, has an activity over `lesson_id`. Days before today are
/// never listed.
pub fn lesson_days(conn: &Connection, course_id: &str, lesson_id: &str, now: i64) -> rusqlite::Result<Vec<LessonDay>> {
    Ok(plans_from_today(conn, course_id, now)?
        .into_iter()
        .filter_map(|p| {
            let activities: Vec<(Kind, usize)> =
                p.activities.iter().filter(|a| a.lessons.iter().any(|l| l == lesson_id)).map(|a| (a.kind, a.lessons.len())).collect();
            (!activities.is_empty()).then_some(LessonDay { day: p.day, assigned: p.assigned, activities })
        })
        .collect())
}

/// Takes `lesson_id` out of every plan [`lesson_days`] lists, in one
/// transaction. An activity whose only lesson it is goes, with everyone's
/// progress on it (as [`remove`]); one with other lessons loses just this
/// lesson and keeps its progress; a `play` also loses the games its other
/// lessons no longer allow, as with [`set_lessons`] (the course is read from
/// its files for that; when they do not load, the games stay). Days before
/// today are never touched. Returns the number of plans changed.
pub fn remove_lesson(conn: &Connection, course_id: &str, lesson_id: &str, now: i64) -> rusqlite::Result<usize> {
    in_tx(conn, |conn| {
        let mut changed = 0;
        let mut course: Option<Option<ContentStore>> = None;
        for p in plans_from_today(conn, course_id, now)? {
            if !p.activities.iter().any(|a| a.lessons.iter().any(|l| l == lesson_id)) {
                continue;
            }
            let mut keep = Vec::new();
            for a in &p.activities {
                if !a.lessons.iter().any(|l| l == lesson_id) {
                    keep.push(a.id);
                    continue;
                }
                let rest: Vec<String> = a.lessons.iter().filter(|l| *l != lesson_id).cloned().collect();
                if rest.is_empty() {
                    conn.execute("DELETE FROM daily_activities WHERE id = ?1", [a.id])?;
                } else {
                    let store = match &mut course {
                        Some(store) => store,
                        None => course.insert(ContentStore::from_files(&crate::courses::load_files(conn, course_id)?).ok()),
                    };
                    let games = match store {
                        Some(store) => games_kept(store, &rest, &a.games),
                        None => a.games.clone(),
                    };
                    conn.execute(
                        "UPDATE daily_activities SET lessons = ?2, games = ?3 WHERE id = ?1",
                        params![a.id, to_json(&rest), to_json(&games)],
                    )?;
                    keep.push(a.id);
                }
            }
            renumber(conn, &keep)?;
            touch(conn, p.id)?;
            changed += 1;
        }
        Ok(changed)
    })
}

/// Deletes every plan more than `caps::DAILY_KEEP_DAYS` days before today,
/// with its activities and everyone's progress on them (the hourly
/// housekeeping). Returns the number of plans deleted.
pub fn prune(conn: &Connection, now: i64) -> rusqlite::Result<usize> {
    let oldest = today(conn, now)? - crate::caps::DAILY_KEEP_DAYS;
    conn.execute("DELETE FROM daily_plans WHERE day < ?1", [oldest])
}

// --------------------------------------------------------------------
// The student's day
// --------------------------------------------------------------------

/// `activity_id -> (done, mark, plays)` for the user's progress in one plan.
fn done_rows(conn: &Connection, user_id: i64, plan_id: i64) -> rusqlite::Result<HashMap<i64, (bool, i64, i64)>> {
    let mut stmt = conn.prepare_cached(
        "SELECT d.activity_id, d.done_at IS NOT NULL, d.mark, d.plays FROM daily_done d JOIN daily_activities a ON a.id = d.activity_id
         WHERE a.plan_id = ?1 AND d.user_id = ?2",
    )?;
    let rows = stmt.query_map(params![plan_id, user_id], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?, r.get(3)?))))?;
    rows.collect()
}

fn stops_of(conn: &Connection, user_id: i64, p: &Plan) -> rusqlite::Result<Vec<Stop>> {
    let done = done_rows(conn, user_id, p.id)?;
    let mut next_given = false;
    Ok(p.activities
        .iter()
        .map(|a| {
            let row = done.get(&a.id).copied();
            let state = match row {
                Some((true, _, _)) => StopState::Done,
                _ if !next_given => {
                    next_given = true;
                    StopState::Next
                }
                _ => StopState::Locked,
            };
            let mark = row.map_or(0, |r| r.1);
            let marked = match a.kind {
                Kind::Study => mark == STUDIED,
                Kind::Review => mark == REVIEWED,
                Kind::Play => false,
            };
            Stop { activity: a.clone(), state, started: row.is_some(), marked, plays: row.map_or(0, |r| r.2) }
        })
        .collect())
}

/// The assigned plan of the day and the user's stops on it, when the course
/// is given to the user.
fn assigned_stops(conn: &Connection, user_id: i64, course_id: &str, day: i64) -> rusqlite::Result<Option<(Plan, Vec<Stop>)>> {
    if !has_course(conn, user_id, course_id)? {
        return Ok(None);
    }
    let Some(p) = plan(conn, course_id, day)?.filter(|p| p.assigned) else { return Ok(None) };
    let stops = stops_of(conn, user_id, &p)?;
    Ok(Some((p, stops)))
}

/// The student's day: one stop per activity of the day's assigned plan, in
/// order, with its state. A finished one is done wherever it stands; the
/// first one not finished is next, and every one after it that is not
/// finished is locked. Empty when the day has no assigned plan or the
/// course is not given to the student.
pub fn stops(conn: &Connection, user_id: i64, course_id: &str, day: i64) -> rusqlite::Result<Vec<Stop>> {
    Ok(assigned_stops(conn, user_id, course_id, day)?.map(|(_, s)| s).unwrap_or_default())
}

/// The student's next activity of the day: the first one not finished.
/// `None` when every one is done or there is nothing assigned.
pub fn next_stop(conn: &Connection, user_id: i64, course_id: &str, day: i64) -> rusqlite::Result<Option<Stop>> {
    Ok(stops(conn, user_id, course_id, day)?.into_iter().find(|s| s.state == StopState::Next))
}

/// The student starts their next activity of an assigned plan of today or
/// an earlier day. Starting it again keeps the first row and what it holds.
pub fn start(conn: &Connection, user_id: i64, course_id: &str, day: i64, activity_id: i64, now: i64) -> rusqlite::Result<bool> {
    if day > today(conn, now)? {
        return Ok(false);
    }
    let Some((_, stops)) = assigned_stops(conn, user_id, course_id, day)? else { return Ok(false) };
    if !stops.iter().any(|s| s.activity.id == activity_id && s.state == StopState::Next) {
        return Ok(false);
    }
    conn.execute(
        "INSERT OR IGNORE INTO daily_done (activity_id, user_id, started_at) VALUES (?1, ?2, ?3)",
        params![activity_id, user_id, now],
    )?;
    Ok(true)
}

fn set_mark(conn: &Connection, user_id: i64, activity_id: i64, kind: Kind, mark: i64) -> rusqlite::Result<bool> {
    if !has_activity(conn, user_id, activity_id)? {
        return Ok(false);
    }
    Ok(conn.execute(
        "UPDATE daily_done SET mark = ?3 WHERE activity_id = ?1 AND user_id = ?2 AND done_at IS NULL
           AND EXISTS (SELECT 1 FROM daily_activities WHERE id = ?1 AND kind = ?4)",
        params![activity_id, user_id, mark, kind.as_str()],
    )? > 0)
}

/// Records that the student read a started, unfinished `study`'s lesson
/// through its stop ([`STUDIED`]); then it may be finished. False when there
/// is no such row or the course is no longer given to them.
pub fn mark_studied(conn: &Connection, user_id: i64, activity_id: i64) -> rusqlite::Result<bool> {
    set_mark(conn, user_id, activity_id, Kind::Study, STUDIED)
}

/// Records that the student finished a started, unfinished `review`'s round
/// ([`REVIEWED`]); then it may be finished. A review with nothing to answer
/// is marked by its page at once. False when there is no such row or the
/// course is no longer given to them.
pub fn mark_reviewed(conn: &Connection, user_id: i64, activity_id: i64) -> rusqlite::Result<bool> {
    set_mark(conn, user_id, activity_id, Kind::Review, REVIEWED)
}

/// The student has a started, unfinished `study` over `lesson_id` in an
/// assigned plan of the course, on any day, and the course is still given
/// to them (the lesson's page may let them read it).
pub fn open_study_on(conn: &Connection, user_id: i64, course_id: &str, lesson_id: &str) -> rusqlite::Result<bool> {
    if !has_course(conn, user_id, course_id)? {
        return Ok(false);
    }
    let mut stmt = conn.prepare_cached(
        "SELECT a.lessons FROM daily_done d JOIN daily_activities a ON a.id = d.activity_id JOIN daily_plans p ON p.id = a.plan_id
         WHERE d.user_id = ?1 AND d.done_at IS NULL AND a.kind = 'study' AND p.course_id = ?2 AND p.assigned_at IS NOT NULL",
    )?;
    let rows = stmt.query_map(params![user_id, course_id], |r| r.get::<_, String>(0))?;
    for lessons in rows {
        if from_json(&lessons?).iter().any(|l| l == lesson_id) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// One more time through a started, unfinished `play` (a finished game of
/// it). A limited play ([`play_limited`]) counts no further once it is at
/// its rounds; a count already above them is kept, never lowered; nothing
/// counts once the course is no longer given to the student. Returns the
/// count after it (unchanged when there was nothing to count).
pub fn add_play(conn: &Connection, user_id: i64, activity_id: i64) -> rusqlite::Result<i64> {
    let row: Option<(bool, i64)> = conn
        .query_row(
            "SELECT done_at IS NOT NULL, plays FROM daily_done WHERE activity_id = ?1 AND user_id = ?2",
            params![activity_id, user_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((done, plays)) = row else { return Ok(0) };
    if !has_activity(conn, user_id, activity_id)? {
        return Ok(plays);
    }
    let Some(p) = plan_of_activity(conn, activity_id)? else { return Ok(plays) };
    let Some(a) = p.activities.iter().find(|a| a.id == activity_id) else { return Ok(plays) };
    if done || a.kind != Kind::Play || (play_limited(&p, a) && plays >= a.rounds) {
        return Ok(plays);
    }
    conn.execute("UPDATE daily_done SET plays = plays + 1 WHERE activity_id = ?1 AND user_id = ?2", params![activity_id, user_id])?;
    Ok(plays + 1)
}

/// A `play` that must be played `rounds` times before it is finished: it
/// asks for some, offers a game to play them with, and is not the plan's
/// last activity.
pub fn play_limited(plan: &Plan, activity: &Activity) -> bool {
    activity.rounds > 0 && !activity.games.is_empty() && plan.activities.last().is_some_and(|last| last.id != activity.id)
}

/// The stop may be finished: a study once its lesson is read, a review once
/// its round is finished ([`Stop::marked`]), a play when it is not limited
/// or was played `rounds` times.
pub fn may_finish(plan: &Plan, stop: &Stop) -> bool {
    match stop.activity.kind {
        Kind::Study | Kind::Review => stop.marked,
        Kind::Play => !play_limited(plan, &stop.activity) || stop.plays >= stop.activity.rounds,
    }
}

/// The student finishes their next activity of today or an earlier day:
/// started and [`may_finish`]. A finished one stays finished.
pub fn finish(conn: &Connection, user_id: i64, course_id: &str, day: i64, activity_id: i64, now: i64) -> rusqlite::Result<bool> {
    if day > today(conn, now)? {
        return Ok(false);
    }
    let Some((p, stops)) = assigned_stops(conn, user_id, course_id, day)? else { return Ok(false) };
    let Some(stop) = stops.iter().find(|s| s.activity.id == activity_id && s.state == StopState::Next) else { return Ok(false) };
    if !stop.started || !may_finish(&p, stop) {
        return Ok(false);
    }
    conn.execute(
        "UPDATE daily_done SET done_at = ?3 WHERE activity_id = ?1 AND user_id = ?2 AND done_at IS NULL",
        params![activity_id, user_id, now],
    )?;
    Ok(true)
}

/// Activities of assigned plans on days before `day` the student has not
/// finished (the "Earlier days · 3 left" count).
pub fn unfinished_before(conn: &Connection, user_id: i64, course_id: &str, day: i64) -> rusqlite::Result<i64> {
    if !has_course(conn, user_id, course_id)? {
        return Ok(0);
    }
    conn.query_row(
        "SELECT COUNT(*) FROM daily_activities a JOIN daily_plans p ON p.id = a.plan_id
         LEFT JOIN daily_done d ON d.activity_id = a.id AND d.user_id = ?1
         WHERE p.course_id = ?2 AND p.day < ?3 AND p.assigned_at IS NOT NULL AND d.done_at IS NULL",
        params![user_id, course_id, day],
        |r| r.get(0),
    )
}

/// Every assigned plan on a day before `day`, newest first, with what the
/// student has left of it. A plan never assigned is not listed.
pub fn earlier_days(conn: &Connection, user_id: i64, course_id: &str, day: i64) -> rusqlite::Result<Vec<EarlierDay>> {
    if !has_course(conn, user_id, course_id)? {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare_cached(
        "SELECT p.day, COUNT(a.id), COALESCE(SUM(a.id IS NOT NULL AND d.done_at IS NULL), 0)
         FROM daily_plans p LEFT JOIN daily_activities a ON a.plan_id = p.id
         LEFT JOIN daily_done d ON d.activity_id = a.id AND d.user_id = ?1
         WHERE p.course_id = ?2 AND p.day < ?3 AND p.assigned_at IS NOT NULL
         GROUP BY p.id ORDER BY p.day DESC",
    )?;
    let rows = stmt.query_map(params![user_id, course_id, day], |r| Ok(EarlierDay { day: r.get(0)?, activities: r.get(1)?, left: r.get(2)? }))?;
    rows.collect()
}

/// How much of the day's assigned plan the student has left: `Some(n)`
/// activities not finished (0: done for the day), `None` when nothing is
/// assigned or the course is not given to them.
pub fn left_on(conn: &Connection, user_id: i64, course_id: &str, day: i64) -> rusqlite::Result<Option<i64>> {
    Ok(assigned_stops(conn, user_id, course_id, day)?.map(|(_, s)| s.iter().filter(|s| s.state != StopState::Done).count() as i64))
}

/// For each of the student's subjects with an assigned plan today (class
/// clock at `now`): the course id and how many activities are left (0: done
/// for today), by course id. A subject with nothing assigned is not listed.
pub fn left_today(conn: &Connection, user_id: i64, now: i64) -> rusqlite::Result<Vec<(String, i64)>> {
    let day = today(conn, now)?;
    let mut stmt = conn.prepare_cached(
        "SELECT p.course_id, COUNT(a.id) - COUNT(d.done_at) FROM course_shares s
           JOIN daily_plans p ON p.course_id = s.course_id AND p.day = ?2 AND p.assigned_at IS NOT NULL
           JOIN daily_activities a ON a.plan_id = p.id
           LEFT JOIN daily_done d ON d.activity_id = a.id AND d.user_id = ?1
         WHERE s.user_id = ?1 AND s.given = 1 GROUP BY p.course_id ORDER BY p.course_id",
    )?;
    let rows = stmt.query_map(params![user_id, day], |r| Ok((r.get(0)?, r.get(1)?)))?;
    rows.collect()
}

// --------------------------------------------------------------------
// The teacher's view of progress
// --------------------------------------------------------------------

/// The progress table of a day: one row per student of the class
/// ([`students_of`]), one cell per activity of the day's plan in order. No
/// cells when there is no plan.
pub fn progress(conn: &Connection, course_id: &str, day: i64) -> rusqlite::Result<Vec<StudentRow>> {
    let p = plan(conn, course_id, day)?;
    let mut out = Vec::new();
    for (student_id, name) in students_of(conn, course_id)? {
        let cells = match &p {
            Some(p) => {
                let done = done_rows(conn, student_id, p.id)?;
                p.activities
                    .iter()
                    .map(|a| match done.get(&a.id) {
                        Some((true, _, _)) => Cell::Done,
                        Some(_) => Cell::Doing,
                        None => Cell::NotStarted,
                    })
                    .collect()
            }
            None => Vec::new(),
        };
        out.push(StudentRow { student_id, name, cells });
    }
    Ok(out)
}

/// `(students who finished every activity, students of the class)` for the
/// day. A day with no activities has no one finished.
pub fn finished_counts(conn: &Connection, course_id: &str, day: i64) -> rusqlite::Result<(usize, usize)> {
    let rows = progress(conn, course_id, day)?;
    let finished = rows.iter().filter(|r| !r.cells.is_empty() && r.cells.iter().all(|c| *c == Cell::Done)).count();
    Ok((finished, rows.len()))
}

/// The teacher's week row: seven days from `first` (usually a
/// [`week_start`]), each with its [`DayState`] and whether it is
/// [`plannable`] on the class clock at `now`.
pub fn week(conn: &Connection, course_id: &str, first: i64, now: i64) -> rusqlite::Result<Vec<WeekDay>> {
    let today = today(conn, now)?;
    let mut out = Vec::with_capacity(7);
    for day in first..first + 7 {
        let state = match plan(conn, course_id, day)? {
            None => DayState::Nothing,
            Some(p) if !p.assigned => DayState::Draft(p.activities.len()),
            Some(p) if day >= today => DayState::Assigned(p.activities.len()),
            Some(_) => {
                let (finished, students) = finished_counts(conn, course_id, day)?;
                DayState::Past { finished, students }
            }
        };
        out.push(WeekDay { day, state, plannable: plannable(day, today) });
    }
    Ok(out)
}

#[cfg(test)]
pub(crate) mod tests;
