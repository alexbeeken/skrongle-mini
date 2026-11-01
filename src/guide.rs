//! The Guide: persistence for stage progress and stars. (The table keeps its
//! original name, `journey_progress`; the feature was renamed. Its `xp`
//! column is no longer written: XP went in #558.)

use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;

use crate::db::now;

#[derive(Debug, Clone, Default)]
pub struct StageProgress {
    pub lesson_id: String,
    pub step: i64,
    pub completed: i64,
    pub best_stars: i64,
    pub run_correct: i64,
    pub run_answered: i64,
}

impl StageProgress {
    pub fn new(lesson_id: &str) -> Self {
        StageProgress { lesson_id: lesson_id.to_string(), ..Default::default() }
    }
}

/// Stars for a finished run: 3 for >=90% right, 2 for >=70%, else 1.
pub fn stars_for(correct: i64, answered: i64) -> i64 {
    if answered == 0 {
        return 3;
    }
    let pct = correct as f64 / answered as f64;
    if pct >= 0.9 {
        3
    } else if pct >= 0.7 {
        2
    } else {
        1
    }
}

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<StageProgress> {
    Ok(StageProgress {
        lesson_id: r.get(0)?,
        step: r.get(1)?,
        completed: r.get(2)?,
        best_stars: r.get(3)?,
        run_correct: r.get(4)?,
        run_answered: r.get(5)?,
    })
}

const COLS: &str = "lesson_id, step, completed, best_stars, run_correct, run_answered";

pub fn load_all(conn: &Connection, user_id: i64, course_id: &str) -> rusqlite::Result<HashMap<String, StageProgress>> {
    let mut stmt =
        conn.prepare_cached(&format!("SELECT {COLS} FROM journey_progress WHERE user_id = ?1 AND course_id = ?2"))?;
    let rows = stmt.query_map(params![user_id, course_id], row)?;
    let mut map = HashMap::new();
    for r in rows {
        let p = r?;
        map.insert(p.lesson_id.clone(), p);
    }
    Ok(map)
}

pub fn load_one(conn: &Connection, user_id: i64, course_id: &str, lesson_id: &str) -> rusqlite::Result<Option<StageProgress>> {
    conn.query_row(
        &format!("SELECT {COLS} FROM journey_progress WHERE user_id = ?1 AND course_id = ?2 AND lesson_id = ?3"),
        params![user_id, course_id, lesson_id],
        row,
    )
    .optional()
}

pub fn save(conn: &Connection, user_id: i64, course_id: &str, p: &StageProgress) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO journey_progress
            (user_id, course_id, lesson_id, step, completed, best_stars, run_correct, run_answered, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(user_id, course_id, lesson_id) DO UPDATE SET
            step = excluded.step, completed = excluded.completed, best_stars = excluded.best_stars,
            run_correct = excluded.run_correct, run_answered = excluded.run_answered,
            updated_at = excluded.updated_at",
        params![
            user_id,
            course_id,
            p.lesson_id,
            p.step,
            p.completed,
            p.best_stars,
            p.run_correct,
            p.run_answered,
            now()
        ],
    )?;
    Ok(())
}

/// Outcome of completing one step.
pub struct StepResult {
    pub progress: StageProgress,
    pub finished: bool,
    pub stars: Option<i64>,
    /// False for a stale repeat (nothing changed).
    pub counted: bool,
}

/// Mark step `index` of a stage with `total` steps done. Idempotent for
/// stale repeats: a step that is not the next one is ignored (the stored
/// progress is returned unchanged).
#[allow(clippy::too_many_arguments)]
pub fn complete_step(
    conn: &Connection,
    user_id: i64,
    course_id: &str,
    lesson_id: &str,
    index: i64,
    total: i64,
    is_question: bool,
    correct: bool,
) -> rusqlite::Result<StepResult> {
    let mut p = load_one(conn, user_id, course_id, lesson_id)?.unwrap_or_else(|| StageProgress::new(lesson_id));
    // A finished stage reopens at its first step (routes/guide.rs): step 0 then
    // starts a new run (completed count and best stars stay).
    if index == 0 && p.step >= total {
        p.step = 0;
        p.run_correct = 0;
        p.run_answered = 0;
    }
    if index != p.step {
        return Ok(StepResult { progress: p, finished: false, stars: None, counted: false });
    }
    if is_question {
        p.run_answered += 1;
        if correct {
            p.run_correct += 1;
        }
    }
    p.step += 1;
    let mut finished = false;
    let mut stars = None;
    if p.step >= total {
        finished = true;
        let s = stars_for(p.run_correct, p.run_answered);
        p.best_stars = p.best_stars.max(s);
        p.completed += 1;
        stars = Some(s);
    }
    save(conn, user_id, course_id, &p)?;
    Ok(StepResult { progress: p, finished, stars, counted: true })
}

/// Record a whole run played elsewhere (a Sprint mode lesson session): the stage
/// counts as completed once more, stars are banked, and any run in progress
/// on the Guide page is reset.
pub fn record_run(
    conn: &Connection,
    user_id: i64,
    course_id: &str,
    lesson_id: &str,
    correct: i64,
    answered: i64,
) -> rusqlite::Result<StageProgress> {
    let mut p = load_one(conn, user_id, course_id, lesson_id)?.unwrap_or_else(|| StageProgress::new(lesson_id));
    p.completed += 1;
    p.best_stars = p.best_stars.max(stars_for(correct, answered));
    p.step = 0;
    p.run_correct = 0;
    p.run_answered = 0;
    save(conn, user_id, course_id, &p)?;
    Ok(p)
}

/// Start the stage over (keeps completed count, best stars and XP).
pub fn restart(conn: &Connection, user_id: i64, course_id: &str, lesson_id: &str) -> rusqlite::Result<StageProgress> {
    let mut p = load_one(conn, user_id, course_id, lesson_id)?.unwrap_or_else(|| StageProgress::new(lesson_id));
    p.step = 0;
    p.run_correct = 0;
    p.run_answered = 0;
    save(conn, user_id, course_id, &p)?;
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stars_thresholds() {
        assert_eq!(stars_for(9, 10), 3);
        assert_eq!(stars_for(7, 10), 2);
        assert_eq!(stars_for(5, 10), 1);
        assert_eq!(stars_for(0, 0), 3);
    }

    #[test]
    fn steps_persist_and_finish() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let uid = crate::auth::create_user(c, "ann@example.org", "h")?;
            let r = complete_step(c, uid, "es", "l1", 0, 3, false, false)?;
            assert_eq!(r.progress.step, 1);
            assert!(!r.finished && r.counted);
            // Stale repeat of step 0 is ignored.
            let r = complete_step(c, uid, "es", "l1", 0, 3, false, false)?;
            assert_eq!(r.progress.step, 1);
            assert!(!r.counted);
            let r = complete_step(c, uid, "es", "l1", 1, 3, true, true)?;
            assert_eq!(r.progress.run_correct, 1);
            let r = complete_step(c, uid, "es", "l1", 2, 3, true, false)?;
            assert!(r.finished);
            assert_eq!(r.stars, Some(1));
            assert_eq!(r.progress.completed, 1);
            // Reopened after the finish, without "Play again": step 0 starts a new run.
            let r = complete_step(c, uid, "es", "l1", 0, 3, false, false)?;
            assert_eq!((r.progress.step, r.progress.run_correct, r.progress.run_answered), (1, 0, 0));
            assert!(!r.finished);
            let r = complete_step(c, uid, "es", "l1", 1, 3, true, true)?;
            assert_eq!(r.progress.step, 2);
            let r = complete_step(c, uid, "es", "l1", 2, 3, true, true)?;
            assert!(r.finished);
            assert_eq!((r.stars, r.progress.completed, r.progress.best_stars), (Some(3), 2, 3));
            let p = restart(c, uid, "es", "l1")?;
            assert_eq!(p.step, 0);
            assert_eq!(p.best_stars, 3);
            let p = record_run(c, uid, "es", "l1", 9, 10)?;
            assert_eq!((p.completed, p.best_stars, p.step), (3, 3, 0));
            assert_eq!(load_all(c, uid, "es")?.len(), 1);
            assert!(load_all(c, uid, "fr")?.is_empty());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn a_replay_left_midway_resumes_where_it_was() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let uid = crate::auth::create_user(c, "bo@example.org", "h")?;
            for i in 0..3 {
                complete_step(c, uid, "es", "l1", i, 3, true, true)?;
            }
            // A replay (#490) gets one step in, then the page is reopened.
            complete_step(c, uid, "es", "l1", 0, 3, true, false)?;
            // Only a finished stage restarts on step 0: mid-replay it is a stale repeat.
            let r = complete_step(c, uid, "es", "l1", 0, 3, true, true)?;
            assert!(!r.counted);
            assert_eq!((r.progress.step, r.progress.run_correct, r.progress.run_answered), (1, 0, 1));
            let r = complete_step(c, uid, "es", "l1", 1, 3, true, true)?;
            assert_eq!((r.progress.step, r.progress.completed, r.progress.best_stars), (2, 1, 3));
            Ok(())
        })
        .unwrap();
    }
}
