//! Mastery model.
//!
//! Each (user, course, lesson) triple has a stored `mastery_score` in 0..=100.
//! Correct answers move the score towards 100 with diminishing returns, a
//! streak bonus rewards consistency, and mistakes knock a proportion off. The
//! *effective* score shown to the user additionally decays with time since the
//! lesson was last practised, so neglected topics drift back into "needs
//! practice".

use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::sync::Arc;

use crate::db::{now, CachedSql};

pub mod cache;

/// Mastery at or above this is considered "mastered".
pub const MASTERED_AT: f64 = 80.0;
/// Below this (once tested) the topic "needs practice", if it has earned it:
/// some answer was wrong, or the score has faded since (see `Status::of`).
pub const WEAK_BELOW: f64 = 50.0;

const SECS_PER_DAY: f64 = 86_400.0;
/// No decay for the first 3 days after practising.
const DECAY_GRACE_DAYS: f64 = 3.0;
/// After the grace period, lose this fraction of the score per day...
const DECAY_PER_DAY: f64 = 0.015;
/// ...but never below this fraction of the stored score.
const DECAY_FLOOR: f64 = 0.4;

#[derive(Debug, Clone, Default)]
pub struct Progress {
    pub lesson_id: String,
    pub mastery_score: f64,
    pub times_tested: i64,
    pub times_correct: i64,
    pub streak: i64,
    pub last_practiced_at: Option<i64>,
    /// When the learner passed the lesson's reading check (see `mark_read`).
    /// Only read lessons are drawn into recommended practice.
    pub read_at: Option<i64>,
}

impl Progress {
    pub fn new(lesson_id: &str) -> Self {
        Progress { lesson_id: lesson_id.to_string(), ..Default::default() }
    }

    /// Score after time decay, as shown on the dashboard and used for quiz selection.
    pub fn effective(&self, at: i64) -> f64 {
        effective_mastery(self.mastery_score, self.last_practiced_at, at)
    }

    /// Whether the lesson has been read: its two-question check passed, or it
    /// was practised before the check existed.
    pub fn is_read(&self) -> bool {
        self.read_at.is_some()
    }

    pub fn accuracy_pct(&self) -> u32 {
        if self.times_tested == 0 {
            0
        } else {
            ((self.times_correct as f64 / self.times_tested as f64) * 100.0).round() as u32
        }
    }

    /// Apply the outcome of one answered question.
    pub fn record_answer(&mut self, correct: bool, at: i64) {
        // Start from the decayed value so that a long break costs something real.
        let current = self.effective(at);
        self.mastery_score = if correct {
            // Gain a fraction of the remaining distance to 100. The fraction grows
            // with the streak (capped) so 5-6 consecutive right answers can take a
            // fresh topic to ~80%, a quick "confidence" build-up.
            let gain = 0.20 + 0.04 * (self.streak.min(5) as f64);
            current + (100.0 - current) * gain
        } else {
            // Lose 35% of the current score; being wrong on a strong topic hurts
            // proportionally more than on a weak one.
            current * 0.65
        };
        self.mastery_score = self.mastery_score.clamp(0.0, 100.0);
        self.times_tested += 1;
        if correct {
            self.times_correct += 1;
            self.streak += 1;
        } else {
            self.streak = 0;
        }
        self.last_practiced_at = Some(at);
    }
}

pub fn effective_mastery(score: f64, last_practiced_at: Option<i64>, at: i64) -> f64 {
    let Some(last) = last_practiced_at else { return score };
    let days = (at - last).max(0) as f64 / SECS_PER_DAY;
    if days <= DECAY_GRACE_DAYS {
        return score;
    }
    let factor = (1.0 - DECAY_PER_DAY * (days - DECAY_GRACE_DAYS)).max(DECAY_FLOOR);
    score * factor
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Untested,
    Mastered,
    InProgress,
    NeedsPractice,
    /// Below the level the learner started the course at and never studied
    /// (`crate::progress::is_skipped`): open, but not pending.
    Skipped,
}

impl Status {
    /// `of`, for a lesson of a course started at unit `start`.
    pub fn of_lesson(p: Option<&Progress>, at: i64, lesson: &crate::content::Lesson, start: usize) -> Status {
        if crate::quiz::is_skipped(p, lesson, start) {
            return Status::Skipped;
        }
        Status::of(p, at)
    }

    pub fn of(p: Option<&Progress>, at: i64) -> Status {
        match p {
            None => Status::Untested,
            Some(p) if p.times_tested == 0 => Status::Untested,
            Some(p) => {
                let e = p.effective(at);
                if e >= MASTERED_AT {
                    Status::Mastered
                } else if e < WEAK_BELOW && (p.times_correct < p.times_tested || e < p.mastery_score) {
                    // A low score from a few answers, all right, is a start,
                    // not a weakness (#681): only a miss or fading makes it weak.
                    Status::NeedsPractice
                } else {
                    Status::InProgress
                }
            }
        }
    }

    /// The status in words, in the learner's language.
    pub fn label(self) -> String {
        crate::t(match self {
            Status::Untested => "study.status.not_started",
            Status::Mastered => "study.status.mastered",
            Status::InProgress => "study.status.in_progress",
            Status::NeedsPractice => "study.status.needs_practice",
            Status::Skipped => "study.status.skipped",
        })
    }

    /// CSS modifier class.
    pub fn css(self) -> &'static str {
        match self {
            Status::Untested => "untested",
            Status::Mastered => "mastered",
            Status::InProgress => "progress",
            Status::NeedsPractice => "weak",
            Status::Skipped => "skipped",
        }
    }
}

// ---------- persistence ----------

const COLS: &str = "lesson_id, mastery_score, times_tested, times_correct, streak, last_practiced_at, read_at";

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Progress> {
    Ok(Progress {
        lesson_id: r.get(0)?,
        mastery_score: r.get(1)?,
        times_tested: r.get(2)?,
        times_correct: r.get(3)?,
        streak: r.get(4)?,
        last_practiced_at: r.get(5)?,
        read_at: r.get(6)?,
    })
}

/// The learner's progress rows for a course, by lesson id: from the
/// [`cache`] when it holds them, else read and held. Shared, not copied.
pub fn load_progress(conn: &Connection, user_id: i64, course_id: &str) -> rusqlite::Result<Arc<HashMap<String, Progress>>> {
    let cache = cache::current();
    if let Some(map) = cache.as_ref().and_then(|c| c.get(user_id, course_id)) {
        return Ok(map);
    }
    let map = Arc::new(read_progress(conn, user_id, course_id)?);
    if let Some(c) = cache {
        #[cfg(test)]
        c.reads.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // Inside a transaction the rows may include writes not yet kept.
        if conn.is_autocommit() {
            c.fill(user_id, course_id, Arc::clone(&map));
        }
    }
    Ok(map)
}

fn read_progress(conn: &Connection, user_id: i64, course_id: &str) -> rusqlite::Result<HashMap<String, Progress>> {
    let mut stmt =
        conn.prepare_cached(&format!("SELECT {COLS} FROM user_progress WHERE user_id = ?1 AND course_id = ?2"))?;
    let rows = stmt.query_map(params![user_id, course_id], row)?;
    let mut map = HashMap::new();
    for r in rows {
        let p = r?;
        map.insert(p.lesson_id.clone(), p);
    }
    Ok(map)
}

pub fn load_one(conn: &Connection, user_id: i64, course_id: &str, lesson_id: &str) -> rusqlite::Result<Option<Progress>> {
    use rusqlite::OptionalExtension;
    if let Some(held) = cache::current().and_then(|c| c.get_one(user_id, course_id, lesson_id)) {
        return Ok(held);
    }
    conn.query_row_cached(
        &format!("SELECT {COLS} FROM user_progress WHERE user_id = ?1 AND course_id = ?2 AND lesson_id = ?3"),
        params![user_id, course_id, lesson_id],
        row,
    )
    .optional()
}

/// Write a lesson's progress row and update the [`cache`]: the one place
/// a `user_progress` row is written (see `cache` for the others).
pub fn save(conn: &Connection, user_id: i64, course_id: &str, p: &Progress) -> rusqlite::Result<()> {
    conn.execute_cached(
        "INSERT INTO user_progress
            (user_id, course_id, lesson_id, mastery_score, times_tested, times_correct, streak, last_practiced_at, read_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(user_id, course_id, lesson_id) DO UPDATE SET
            mastery_score = excluded.mastery_score,
            times_tested = excluded.times_tested,
            times_correct = excluded.times_correct,
            streak = excluded.streak,
            last_practiced_at = excluded.last_practiced_at,
            read_at = excluded.read_at",
        params![
            user_id,
            course_id,
            p.lesson_id,
            p.mastery_score,
            p.times_tested,
            p.times_correct,
            p.streak,
            p.last_practiced_at,
            p.read_at
        ],
    )?;
    if let Some(c) = cache::current() {
        // Inside a transaction the write may yet be rolled back: forget the
        // learner's map rather than hold a row the table may not keep.
        // `record_answer` puts it back after its commit.
        if conn.is_autocommit() {
            c.put(user_id, course_id, p);
        } else {
            c.forget_user(user_id);
        }
    }
    Ok(())
}

/// Mark a lesson as read: the learner passed its two-question check. The
/// first pass sets `read_at`; later passes leave it alone. Returns the
/// progress row, which keeps whatever mastery the check's answers produced.
pub fn mark_read(conn: &Connection, user_id: i64, course_id: &str, lesson_id: &str, at: i64) -> rusqlite::Result<Progress> {
    let mut progress = load_one(conn, user_id, course_id, lesson_id)?.unwrap_or_else(|| Progress::new(lesson_id));
    if progress.read_at.is_none() {
        progress.read_at = Some(at);
        save(conn, user_id, course_id, &progress)?;
    }
    Ok(progress)
}

/// Record an answer in `quiz_history` and update the lesson's progress row, in
/// one transaction. Returns the updated progress.
pub fn record_answer(
    conn: &mut Connection,
    user_id: i64,
    course_id: &str,
    lesson_id: &str,
    question_id: &str,
    selected_answer: &str,
    correct: bool,
) -> rusqlite::Result<Progress> {
    record_answer_with(conn, user_id, course_id, lesson_id, question_id, selected_answer, correct, |_| Ok(())).map(|(p, ())| p)
}

/// [`record_answer`] with more writes in the same transaction: `more` runs
/// on the transaction after the answer is written, and an error from it
/// rolls the answer back too.
#[allow(clippy::too_many_arguments)]
pub fn record_answer_with<T>(
    conn: &mut Connection,
    user_id: i64,
    course_id: &str,
    lesson_id: &str,
    question_id: &str,
    selected_answer: &str,
    correct: bool,
    more: impl FnOnce(&Connection) -> rusqlite::Result<T>,
) -> rusqlite::Result<(Progress, T)> {
    // The learner's held map, taken out while the transaction runs (its
    // `save` would forget it) and put back, with the new row, once the
    // answer is committed. An error leaves it out: the next load reads.
    let cache = cache::current();
    let held = cache.as_ref().and_then(|c| c.take(user_id, course_id));
    let tx = conn.transaction()?;
    let progress = record_answer_at(&tx, user_id, course_id, lesson_id, question_id, selected_answer, correct, now())?;
    let extra = more(&tx)?;
    tx.commit()?;
    if let (Some(c), Some(mut map)) = (cache, held) {
        Arc::make_mut(&mut map).insert(progress.lesson_id.clone(), progress.clone());
        c.fill(user_id, course_id, map);
    }
    Ok((progress, extra))
}

/// [`record_answer`]'s two writes at a given time, inside the caller's
/// transaction (the time passed in).
#[allow(clippy::too_many_arguments)]
pub(crate) fn record_answer_at(
    conn: &Connection,
    user_id: i64,
    course_id: &str,
    lesson_id: &str,
    question_id: &str,
    selected_answer: &str,
    correct: bool,
    at: i64,
) -> rusqlite::Result<Progress> {
    let mut progress = load_one(conn, user_id, course_id, lesson_id)?.unwrap_or_else(|| Progress::new(lesson_id));
    progress.record_answer(correct, at);
    save(conn, user_id, course_id, &progress)?;
    conn.prepare_cached(
        "INSERT INTO quiz_history (user_id, course_id, lesson_id, question_id, selected_answer, was_correct, answered_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?
    .execute(params![user_id, course_id, lesson_id, question_id, selected_answer, correct, at])?;
    Ok(progress)
}

/// Question ids this user answered correctly in the last `window_secs`.
/// Used to avoid re-asking what they just got right.
pub fn recently_correct(conn: &Connection, user_id: i64, course_id: &str, window_secs: i64) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare_cached(
        "SELECT DISTINCT question_id FROM quiz_history
         WHERE user_id = ?1 AND course_id = ?2 AND was_correct = 1 AND answered_at > ?3",
    )?;
    let rows = stmt.query_map(params![user_id, course_id, now() - window_secs], |r| r.get::<_, String>(0))?;
    rows.collect()
}

#[derive(Debug, Clone)]
pub struct HistoryRow {
    pub lesson_id: String,
    pub question_id: String,
    pub selected_answer: String,
    pub was_correct: bool,
    pub answered_at: i64,
}

pub fn recent_history(conn: &Connection, user_id: i64, course_id: &str, limit: i64) -> rusqlite::Result<Vec<HistoryRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT lesson_id, question_id, selected_answer, was_correct, answered_at
         FROM quiz_history WHERE user_id = ?1 AND course_id = ?2 ORDER BY answered_at DESC, id DESC LIMIT ?3",
    )?;
    let rows = stmt.query_map(params![user_id, course_id, limit], |r| {
        Ok(HistoryRow {
            lesson_id: r.get(0)?,
            question_id: r.get(1)?,
            selected_answer: r.get(2)?,
            was_correct: r.get(3)?,
            answered_at: r.get(4)?,
        })
    })?;
    rows.collect()
}

/// How many of a learner's latest answers [`recent_mistakes`] looks through.
/// The mistakes list reads a fixed slice of history, not all of it (#589).
pub const MISTAKES_SCAN: i64 = 500;

/// Questions whose latest answer was wrong, newest first, at most `limit`:
/// the mistakes list (#338). Only the last [`MISTAKES_SCAN`] answers are
/// read, so a question answered right since its mistake drops off, and a
/// mistake older than that window does too.
pub fn recent_mistakes(conn: &Connection, user_id: i64, course_id: &str, limit: i64) -> rusqlite::Result<Vec<HistoryRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT lesson_id, question_id, selected_answer, was_correct, answered_at FROM (
             SELECT lesson_id, question_id, selected_answer, was_correct, answered_at, id,
                    ROW_NUMBER() OVER (PARTITION BY question_id ORDER BY answered_at DESC, id DESC) AS nth
             FROM (SELECT * FROM quiz_history WHERE user_id = ?1 AND course_id = ?2
                   ORDER BY answered_at DESC, id DESC LIMIT ?3))
         WHERE nth = 1 AND was_correct = 0
         ORDER BY answered_at DESC, id DESC LIMIT ?4",
    )?;
    let rows = stmt.query_map(params![user_id, course_id, MISTAKES_SCAN, limit], |r| {
        Ok(HistoryRow {
            lesson_id: r.get(0)?,
            question_id: r.get(1)?,
            selected_answer: r.get(2)?,
            was_correct: r.get(3)?,
            answered_at: r.get(4)?,
        })
    })?;
    rows.collect()
}

/// (answered, correct) counts since `since`.
pub fn stats_since(conn: &Connection, user_id: i64, course_id: &str, since: i64) -> rusqlite::Result<(i64, i64)> {
    conn.query_row_cached(
        "SELECT COUNT(*), COALESCE(SUM(was_correct), 0) FROM quiz_history
         WHERE user_id = ?1 AND course_id = ?2 AND answered_at >= ?3",
        params![user_id, course_id, since],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
}

/// Lessons with any progress and how many are mastered, per course the user
/// has touched: course_id -> (lessons tested, lessons mastered).
pub fn summary_by_course(conn: &Connection, user_id: i64, at: i64) -> rusqlite::Result<HashMap<String, (usize, usize)>> {
    let mut stmt = conn.prepare_cached(
        "SELECT course_id, mastery_score, times_tested, last_practiced_at FROM user_progress WHERE user_id = ?1",
    )?;
    let rows = stmt.query_map([user_id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?, r.get::<_, i64>(2)?, r.get::<_, Option<i64>>(3)?))
    })?;
    let mut out: HashMap<String, (usize, usize)> = HashMap::new();
    for r in rows {
        let (course, score, tested, last) = r?;
        if tested == 0 {
            continue;
        }
        let e = out.entry(course).or_default();
        e.0 += 1;
        if effective_mastery(score, last, at) >= MASTERED_AT {
            e.1 += 1;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: i64 = 1_700_000_000;

    #[test]
    fn six_correct_in_a_row_masters_a_topic() {
        let mut p = Progress::new("x");
        for i in 0..6 {
            p.record_answer(true, T0 + i);
        }
        assert!(p.mastery_score >= MASTERED_AT, "score was {}", p.mastery_score);
        assert_eq!(p.streak, 6);
        assert_eq!(p.accuracy_pct(), 100);
    }

    #[test]
    fn mistakes_cost_proportionally() {
        let mut p = Progress::new("x");
        for i in 0..6 {
            p.record_answer(true, T0 + i);
        }
        let before = p.mastery_score;
        p.record_answer(false, T0 + 10);
        assert!((p.mastery_score - before * 0.65).abs() < 1e-9);
        assert_eq!(p.streak, 0);
        assert_eq!(Status::of(Some(&p), T0 + 10), Status::InProgress);
    }

    #[test]
    fn decay_kicks_in_after_grace_period() {
        let p = Progress { mastery_score: 90.0, last_practiced_at: Some(T0), times_tested: 5, ..Progress::new("x") };
        assert_eq!(p.effective(T0 + 2 * 86_400), 90.0);
        let after_30_days = p.effective(T0 + 30 * 86_400);
        assert!(after_30_days < 90.0 && after_30_days > 36.0);
        // Floor at 40% of the stored score.
        assert!((p.effective(T0 + 365 * 86_400) - 36.0).abs() < 1e-9);
        assert_eq!(Status::of(Some(&p), T0 + 365 * 86_400), Status::NeedsPractice);
    }

    #[test]
    fn a_few_right_answers_are_in_progress_not_weak() {
        let mut p = Progress::new("x");
        p.record_answer(true, T0);
        p.record_answer(true, T0 + 1);
        assert!(p.mastery_score < WEAK_BELOW, "score was {}", p.mastery_score);
        assert_eq!(Status::of(Some(&p), T0 + 2), Status::InProgress);
        // A miss makes it weak...
        let mut missed = p.clone();
        missed.record_answer(false, T0 + 3);
        assert_eq!(Status::of(Some(&missed), T0 + 3), Status::NeedsPractice);
        // ...and so does fading after the grace period.
        assert_eq!(Status::of(Some(&p), T0 + 30 * 86_400), Status::NeedsPractice);
    }

    #[test]
    fn untested_status() {
        assert_eq!(Status::of(None, T0), Status::Untested);
        assert_eq!(Status::of(Some(&Progress::new("x")), T0), Status::Untested);
    }

    #[test]
    fn record_answer_persists_per_course() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let uid = crate::auth::create_user(c, "ann@example.org", "h")?;
            let p = record_answer(c, uid, "spanish", "l1", "q1", "a", true)?;
            assert_eq!(p.times_tested, 1);
            let p = record_answer(c, uid, "spanish", "l1", "q2", "b", false)?;
            assert_eq!(p.times_tested, 2);
            assert_eq!(p.times_correct, 1);
            // The same lesson id in another course is a separate record.
            record_answer(c, uid, "other", "l1", "q1", "a", true)?;
            let all = load_progress(c, uid, "spanish")?;
            assert_eq!(all.len(), 1);
            assert_eq!(all["l1"].times_tested, 2);
            assert_eq!(load_progress(c, uid, "other")?["l1"].times_tested, 1);
            let hist = recent_history(c, uid, "spanish", 10)?;
            assert_eq!(hist.len(), 2);
            assert_eq!(hist[0].question_id, "q2");
            assert_eq!(recently_correct(c, uid, "spanish", 3600)?, vec!["q1".to_string()]);
            assert_eq!(stats_since(c, uid, "spanish", 0)?, (2, 1));
            let summary = summary_by_course(c, uid, now())?;
            assert_eq!(summary["spanish"], (1, 0));
            assert_eq!(summary["other"], (1, 0));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn mistakes_are_questions_whose_latest_answer_was_wrong() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let uid = crate::auth::create_user(c, "mia@example.org", "h")?;
            record_answer_at(c, uid, "spanish", "l1", "q1", "x", false, T0)?;
            record_answer_at(c, uid, "spanish", "l1", "q2", "y", false, T0 + 1)?;
            record_answer_at(c, uid, "spanish", "l1", "q2", "z", false, T0 + 2)?;
            record_answer_at(c, uid, "spanish", "l1", "q3", "w", false, T0 + 3)?;
            // Right since: off the list.
            record_answer_at(c, uid, "spanish", "l1", "q3", "ok", true, T0 + 4)?;
            record_answer_at(c, uid, "other", "l1", "q9", "x", false, T0 + 5)?;
            let m = recent_mistakes(c, uid, "spanish", 10)?;
            let got: Vec<(&str, &str)> = m.iter().map(|h| (h.question_id.as_str(), h.selected_answer.as_str())).collect();
            assert_eq!(got, vec![("q2", "z"), ("q1", "x")], "newest first, last wrong answer, one row per question");
            assert_eq!(recent_mistakes(c, uid, "spanish", 1)?.len(), 1);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn mark_read_is_sticky_and_survives_answers() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let uid = crate::auth::create_user(c, "bob@example.org", "h")?;
            // Answering alone does not read a lesson (the check does).
            let p = record_answer(c, uid, "spanish", "l1", "q1", "a", true)?;
            assert!(!p.is_read());
            let p = mark_read(c, uid, "spanish", "l1", T0)?;
            assert_eq!(p.read_at, Some(T0));
            assert_eq!(p.times_tested, 1, "mastery from the check's answers is kept");
            // Later answers keep the read mark; a second pass does not move it.
            let p = record_answer(c, uid, "spanish", "l1", "q2", "b", false)?;
            assert_eq!(p.read_at, Some(T0));
            let p = mark_read(c, uid, "spanish", "l1", T0 + 99)?;
            assert_eq!(p.read_at, Some(T0));
            // A never-practised lesson can be read too.
            let p = mark_read(c, uid, "spanish", "l2", T0)?;
            assert!(p.is_read() && p.times_tested == 0);
            assert_eq!(load_progress(c, uid, "spanish")?.len(), 2);
            Ok(())
        })
        .unwrap();
    }

    fn counts(c: &Connection) -> Vec<i64> {
        ["quiz_history", "activity_log", "user_vocab", "user_progress"]
            .iter()
            .map(|t| c.query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get(0)).unwrap())
            .collect()
    }

    #[test]
    fn one_transaction_answer_leaves_the_same_rows_as_separate_writes() {
        use crate::activity::{self, Kind};
        let db_a = crate::db::Db::open_in_memory().unwrap();
        let db_b = crate::db::Db::open_in_memory().unwrap();
        let run_a = db_a
            .with(|c| {
                let uid = crate::auth::create_user(c, "a@example.org", "h")?;
                for (q, ok) in [("q1", true), ("q2", false), ("q3", true)] {
                    record_answer(c, uid, "spanish", "l1", q, "x", ok)?;
                    activity::record(c, uid, "spanish", Kind::Answer, 1, 0)?;
                    crate::vocab::record_learned(c, uid, "spanish", "l1", &["hola", "adios"], 0)?;
                }
                Ok((counts(c), load_one(c, uid, "spanish", "l1")?.unwrap()))
            })
            .unwrap();
        let run_b = db_b
            .with(|c| {
                let uid = crate::auth::create_user(c, "a@example.org", "h")?;
                for (q, ok) in [("q1", true), ("q2", false), ("q3", true)] {
                    let (_, new) = record_answer_with(c, uid, "spanish", "l1", q, "x", ok, |tx| {
                        activity::record(tx, uid, "spanish", Kind::Answer, 1, 0)?;
                        crate::vocab::record_learned(tx, uid, "spanish", "l1", &["hola", "adios"], 0)
                    })?;
                    assert_eq!(new.len(), if q == "q1" { 2 } else { 0 });
                }
                Ok((counts(c), load_one(c, uid, "spanish", "l1")?.unwrap()))
            })
            .unwrap();
        assert_eq!(run_a.0, vec![3, 3, 2, 1]);
        assert_eq!(run_a.0, run_b.0);
        let (a, b) = (&run_a.1, &run_b.1);
        assert_eq!((a.times_tested, a.times_correct, a.streak), (b.times_tested, b.times_correct, b.streak));
        assert_eq!(a.mastery_score, b.mastery_score);
    }

    #[test]
    fn a_failure_in_the_second_write_rolls_back_the_first() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let uid = crate::auth::create_user(c, "r@example.org", "h")?;
            let before = counts(c);
            let r = record_answer_with(c, uid, "spanish", "l1", "q1", "x", true, |tx| {
                crate::activity::record(tx, uid, "spanish", crate::activity::Kind::Answer, 1, 0)?;
                tx.execute("INSERT INTO no_such_table VALUES (1)", [])?;
                Ok(())
            });
            assert!(r.is_err());
            assert_eq!(counts(c), before, "answer, history and activity all rolled back");
            assert!(load_one(c, uid, "spanish", "l1")?.is_none());
            Ok(())
        })
        .unwrap();
    }
}
