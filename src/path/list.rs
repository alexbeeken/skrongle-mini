//! A learner's list in one course (`docs/features-learner-path.md`, "The
//! list"): what is next, what was done today and what is hidden; and hiding
//! one activity from it or bringing it back (`path_hidden`).

use std::collections::{HashMap, HashSet};

use rusqlite::{params, Connection};

use super::{score, sort, Kind, Scored, Tried, SETTINGS};
use crate::content::{ContentStore, Lesson};
use crate::earnings::day_of;

/// One activity on a learner's list.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub lesson_id: String,
    pub title: String,
    pub kind: Kind,
    /// Days until it is due (`path::score`); `None` once a Study or a Play is tried.
    pub score: Option<f64>,
    pub streak: u32,
    pub tried: bool,
    /// Days between its last attempt and the next time it is due (`path::gap`).
    pub gap: f64,
}

/// A learner's three lists.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Path {
    /// Due now, in the order to do them (`path::sort`). A Play only once the day's goal is met.
    pub next: Vec<Item>,
    /// Last tried today in the learner's time zone, oldest first.
    pub done_today: Vec<Item>,
    /// Hidden by the learner, or in a lesson the starting level skipped, in course order.
    pub hidden: Vec<Item>,
}

/// The kinds of activity `lesson` has, in order: a Study when it has Study
/// steps, a Review when Review can deal it something to answer, a Play when
/// Word Match can deal a board from its terms.
pub fn kinds_of(course: &ContentStore, lesson: &Lesson) -> Vec<Kind> {
    Kind::ALL
        .into_iter()
        .filter(|k| match k {
            Kind::Study => !lesson.guide_steps.is_empty(),
            Kind::Review => {
                let session = crate::sprint::lesson_session(course, lesson, &HashSet::new(), &mut crate::quiz::Rng::seeded(1));
                session.exercises.iter().any(|e| !matches!(e, crate::sprint::Exercise::Bite { .. }))
            }
            Kind::Play => lesson.vocabulary.len() >= crate::games::MIN_WORDS,
        })
        .collect()
}

type Key = (String, Kind);

/// Each tried activity: (last_at, streak).
fn attempts(conn: &Connection, user_id: i64, course_id: &str) -> rusqlite::Result<HashMap<Key, (i64, u32)>> {
    let mut stmt =
        conn.prepare_cached("SELECT lesson_id, kind, last_at, streak FROM path_activity WHERE user_id = ?1 AND course_id = ?2")?;
    let rows =
        stmt.query_map(params![user_id, course_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get(2)?, r.get(3)?)))?;
    let mut out = HashMap::new();
    for row in rows {
        let (lesson, kind, at, streak) = row?;
        if let Some(kind) = Kind::parse(&kind) {
            out.insert((lesson, kind), (at, streak));
        }
    }
    Ok(out)
}

fn hidden_rows(conn: &Connection, user_id: i64, course_id: &str) -> rusqlite::Result<HashSet<Key>> {
    let mut stmt = conn.prepare_cached("SELECT lesson_id, kind FROM path_hidden WHERE user_id = ?1 AND course_id = ?2")?;
    let rows = stmt.query_map(params![user_id, course_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut out = HashSet::new();
    for row in rows {
        let (lesson, kind) = row?;
        out.extend(Kind::parse(&kind).map(|k| (lesson, k)));
    }
    Ok(out)
}

/// Lessons the learner opened or worked on (`lesson_touches`) or tried an activity of.
fn touched(conn: &Connection, user_id: i64, course_id: &str) -> rusqlite::Result<HashSet<String>> {
    let mut stmt = conn.prepare_cached(
        "SELECT lesson_id FROM lesson_touches WHERE user_id = ?1 AND course_id = ?2
         UNION SELECT lesson_id FROM path_activity WHERE user_id = ?1 AND course_id = ?2",
    )?;
    let rows = stmt.query_map(params![user_id, course_id], |r| r.get::<_, String>(0))?;
    rows.collect()
}

/// `user_id`'s list in `course` at `now` (Unix seconds), with days in the
/// time zone `tz` (`CurrentUser::tz`) and lessons of the units below
/// `start` (`Project::start`) skipped until touched. A Play is a reward:
/// it is due only once the day's goal (`activity::Today`) is met.
pub fn list(conn: &Connection, user_id: i64, course: &ContentStore, start: usize, now: i64, tz: i64) -> rusqlite::Result<Path> {
    let course_id = course.id();
    let tried = attempts(conn, user_id, course_id)?;
    let hidden = hidden_rows(conn, user_id, course_id)?;
    let touched = touched(conn, user_id, course_id)?;
    let today = day_of(tz, now);
    let goal_met = crate::activity::Today::load(conn, user_id, course_id, crate::sprint::local_day_start(tz, now))?.done();
    let mut path = Path::default();
    let mut due = Vec::new();
    let mut done = Vec::new();
    let mut order = 0;
    for lesson in course.lessons() {
        let skipped = crate::progress::is_skipped(lesson, start, touched.contains(&lesson.id));
        for kind in kinds_of(course, lesson) {
            order += 1;
            let key = (lesson.id.clone(), kind);
            let (last, item) = scored(lesson, kind, tried.get(&key).copied(), today, tz);
            let score = item.score;
            if skipped || hidden.contains(&key) {
                path.hidden.push(item);
            } else if let Some((at, _)) = last.filter(|(_, t)| t.day == today) {
                done.push((at, order, item));
            } else if kind != Kind::Play || goal_met {
                due.push(Scored { score, order, item });
            }
        }
    }
    done.sort_by_key(|&(at, order, _)| (at, order));
    path.done_today = done.into_iter().map(|(_, _, item)| item).collect();
    path.next = sort(due).into_iter().map(|s| s.item).collect();
    Ok(path)
}

/// One activity of `lesson` scored at local day `today`, from its last
/// attempt `(last_at, streak)`: the attempt's time and day, and the item.
fn scored(lesson: &Lesson, kind: Kind, attempt: Option<(i64, u32)>, today: i64, tz: i64) -> (Option<(i64, Tried)>, Item) {
    let last = attempt.map(|(at, streak)| (at, Tried { day: day_of(tz, at), streak }));
    let streak = last.map_or(0, |(_, t)| t.streak);
    let item = Item {
        lesson_id: lesson.id.clone(),
        title: lesson.title.clone(),
        kind,
        score: score(last.map(|(_, t)| t), kind.repeats(), today, &SETTINGS),
        streak,
        tried: last.is_some(),
        gap: super::gap(streak, &SETTINGS),
    };
    (last, item)
}

/// Hide one activity from `user_id`'s list. Its attempts stay as they are.
pub fn hide(conn: &Connection, user_id: i64, course_id: &str, lesson_id: &str, kind: Kind, now: i64) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO path_hidden (user_id, course_id, lesson_id, kind, hidden_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![user_id, course_id, lesson_id, kind.as_str(), now],
    )?;
    Ok(())
}

/// Bring one activity back onto `user_id`'s list, scored by its attempts as
/// before. In a lesson the starting level skipped, the lesson becomes the
/// learner's (`activity::touch`) and its other activities stay hidden.
pub fn unhide(
    conn: &Connection,
    user_id: i64,
    course: &ContentStore,
    start: usize,
    lesson_id: &str,
    kind: Kind,
    now: i64,
) -> rusqlite::Result<()> {
    let course_id = course.id();
    conn.execute(
        "DELETE FROM path_hidden WHERE user_id = ?1 AND course_id = ?2 AND lesson_id = ?3 AND kind = ?4",
        params![user_id, course_id, lesson_id, kind.as_str()],
    )?;
    let Some(lesson) = course.lesson_by_id(lesson_id) else { return Ok(()) };
    if !crate::progress::is_skipped(lesson, start, touched(conn, user_id, course_id)?.contains(lesson_id)) {
        return Ok(());
    }
    for other in kinds_of(course, lesson).into_iter().filter(|&k| k != kind) {
        hide(conn, user_id, course_id, lesson_id, other, now)?;
    }
    let mode = match kind {
        Kind::Study => crate::activity::Mode::Guide,
        Kind::Review => crate::activity::Mode::Sprint,
        Kind::Play => crate::activity::Mode::Play,
    };
    crate::activity::touch(conn, user_id, course_id, lesson_id, mode)
}

#[cfg(test)]
mod tests;
