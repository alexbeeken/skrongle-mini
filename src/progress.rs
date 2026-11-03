//! One idea of "done" for every mode: a lesson is **passed** once it has been
//! *seen* (its teaching material was put in front of the learner) and one
//! *run* of its exercises scored at least `PASS_PCT` over at least
//! `required_answers(lesson)` answers. Order does not matter: a good run
//! before the reading counts the moment the lesson is seen.
//!
//! Every mode reports here: Study mode (the lesson page's "I've read this"
//! button, practice rounds, listening rounds), Sprint mode (lesson sessions),
//! Play mode (Guide stages, Word Match on a lesson's board), AI tutoring and
//! outside AI agents (`/api/progress/*`). Passing unlocks the next stop of
//! the Sprint mode path and the Guide, and an item of the paradigm drill.
//!
//! Mastery (`mastery.rs`) is separate and unchanged: it says how *fresh* a
//! lesson is and decays; a pass is a milestone and stays.

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::HashMap;

use crate::content::Lesson;
use crate::db::{now, CachedSql};

/// A run must score at least this percentage to pass the lesson.
pub const PASS_PCT: i64 = 70;
/// A run needs this many answers, or every quiz question of a lesson that
/// has fewer.
pub const MIN_ANSWERS: i64 = 3;
/// Agents' evidence notes are kept up to this many characters.
pub const MAX_EVIDENCE_CHARS: usize = 500;

/// The rules, in the words given to the built-in tutor and to outside
/// agents (`GET /api/progress`). Keep in step with `docs/progress.md`.
pub const RULES: &str = "A lesson counts as learned (\"passed\") when both of these are true, in any order, in any mode:\n\
1. Seen: the learner has been taught the lesson's material: read the lesson page and passed its short reading check, played a Sprint mode lesson session or a Guide stage (both open with the lesson's teaching cards), or been taught it one-to-one by a tutor.\n\
2. Shown: in one sitting, the learner answered at least 3 of the lesson's exercises (or all of them, if the lesson has fewer than 3 quiz questions) and got at least 70% right. Hints are fine; answers given away or skipped count as wrong. Exercises must test the lesson's own focus, not other lessons.\n\
Passing is a milestone and stays. Mastery is separate: a 0-100 score per lesson that rises with each right answer, falls with wrong ones and decays with time. Report every graded answer so mastery stays honest, and declare a pass only when both conditions hold.";

/// The `via` stored for a seen or a run in each mode. They are the modes'
/// old one-letter names, kept so stored progress needs no migration; show
/// them through `via_label` and `via_id`.
pub const VIA_SPRINT: &str = "d";
pub const VIA_STUDY: &str = "k";
pub const VIA_PLAY: &str = "g";

/// Which mode a seen or a run came from, in the learner's language.
pub fn via_label(via: &str) -> String {
    crate::t(match via {
        VIA_STUDY => "study.via.study",
        VIA_SPRINT => "study.via.sprint",
        VIA_PLAY => "study.via.play",
        "tutor" => "study.via.tutor",
        "agent" => "study.via.agent",
        "legacy" => "study.via.legacy",
        _ => "study.via.other",
    })
}

/// A stored `via` as the progress API gives it: the mode's name (`sprint`,
/// `study`, `play`), or the value itself (`tutor`, `agent`, `legacy`).
pub fn via_id(via: &str) -> &str {
    match via {
        VIA_SPRINT => "sprint",
        VIA_STUDY => "study",
        VIA_PLAY => "play",
        other => other,
    }
}

/// Is `lesson` *skipped*? A learner who starts a course with `levels` at a
/// later level (`Project::start`, a unit index) skips the lessons of the
/// units below it: they stay open but are never recommended, practised,
/// reviewed or counted as passed. Once the learner studies one anyway
/// (`touched`: seen, read or answered) it is theirs like any other.
pub fn is_skipped(lesson: &Lesson, start: usize, touched: bool) -> bool {
    lesson.unit_index < start && !touched
}

/// Answers a run of this lesson needs to count.
pub fn required_answers(lesson: &Lesson) -> i64 {
    MIN_ANSWERS.min(lesson.quiz.len() as i64)
}

/// The same, for a run in `via`. Sprint mode can't draw the questions that
/// need a page (`QuizType::needs_widget`), so they don't count there.
fn required_answers_via(lesson: &Lesson, via: &str) -> i64 {
    match via {
        VIA_SPRINT => MIN_ANSWERS.min(lesson.quiz.iter().filter(|q| !q.kind.needs_widget()).count() as i64),
        _ => required_answers(lesson),
    }
}

/// Percentage right, 100 for a run with nothing to answer.
pub fn pct(correct: i64, answered: i64) -> i64 {
    if answered <= 0 {
        100
    } else {
        (correct.clamp(0, answered) * 100) / answered
    }
}

/// Does a run of `answered` with `correct` right pass `lesson`?
pub fn run_passes(lesson: &Lesson, correct: i64, answered: i64) -> bool {
    answered >= required_answers(lesson) && pct(correct, answered) >= PASS_PCT
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct LessonPass {
    pub lesson_id: String,
    pub seen_at: Option<i64>,
    pub seen_via: Option<String>,
    /// Best qualifying run (enough answers), or -1 for none yet.
    pub best_pct: i64,
    pub best_via: Option<String>,
    pub passed_at: Option<i64>,
    pub passed_via: Option<String>,
    /// Qualifying runs reported.
    pub runs: i64,
    /// The last agent's note on how the learner showed it.
    pub evidence: String,
}

impl LessonPass {
    fn new(lesson_id: &str) -> Self {
        LessonPass { lesson_id: lesson_id.to_string(), best_pct: -1, ..Default::default() }
    }

    pub fn seen(&self) -> bool {
        self.seen_at.is_some()
    }

    pub fn passed(&self) -> bool {
        self.passed_at.is_some()
    }

    /// One short line for a page: "Passed in Sprint mode", "Read · best run 60%"…
    pub fn summary(&self) -> String {
        if let Some(v) = &self.passed_via {
            let key = if v == "agent" { "study.pass.passed_with" } else { "study.pass.passed_in" };
            return crate::t1(key, "via", via_label(v));
        }
        match (self.seen(), self.best_pct) {
            (true, b) if b >= 0 => crate::t2("study.pass.seen_best", "best", b, "pass", PASS_PCT),
            (true, _) => crate::t1("study.pass.seen_now", "pass", PASS_PCT),
            (false, b) if b >= PASS_PCT => crate::t("study.pass.done_read"),
            (false, b) if b >= 0 => crate::t1("study.pass.best_run", "n", b),
            _ => crate::t("study.pass.not_started"),
        }
    }

    /// Pass now if both conditions hold; true when this call passed it.
    fn settle(&mut self, via: &str, at: i64) -> bool {
        if self.passed() || !self.seen() || self.best_pct < PASS_PCT {
            return false;
        }
        self.passed_at = Some(at);
        self.passed_via = Some(via.to_string());
        true
    }
}

/// What a report did.
#[derive(Debug, Clone, Serialize)]
pub struct Outcome {
    pub lesson_id: String,
    pub seen: bool,
    pub passed: bool,
    /// This report is what passed it.
    pub passed_now: bool,
    pub best_pct: i64,
    /// The run reported, as a percentage (None for a seen report).
    pub run_pct: Option<i64>,
    /// The run had enough answers to count.
    pub run_counted: bool,
    pub required_answers: i64,
    pub summary: String,
}

const COLS: &str = "lesson_id, seen_at, seen_via, best_pct, best_via, passed_at, passed_via, runs, evidence";

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<LessonPass> {
    Ok(LessonPass {
        lesson_id: r.get(0)?,
        seen_at: r.get(1)?,
        seen_via: r.get(2)?,
        best_pct: r.get(3)?,
        best_via: r.get(4)?,
        passed_at: r.get(5)?,
        passed_via: r.get(6)?,
        runs: r.get(7)?,
        evidence: r.get(8)?,
    })
}

pub fn load_all(conn: &Connection, user_id: i64, course_id: &str) -> rusqlite::Result<HashMap<String, LessonPass>> {
    let mut stmt =
        conn.prepare_cached(&format!("SELECT {COLS} FROM lesson_passes WHERE user_id = ?1 AND course_id = ?2"))?;
    let rows = stmt.query_map(params![user_id, course_id], row)?;
    let mut map = HashMap::new();
    for r in rows {
        let p = r?;
        map.insert(p.lesson_id.clone(), p);
    }
    Ok(map)
}

/// Lessons passed per course the user has passed any in: course_id -> count.
pub fn passed_by_course(conn: &Connection, user_id: i64) -> rusqlite::Result<HashMap<String, usize>> {
    let mut stmt = conn.prepare_cached(
        "SELECT course_id, COUNT(*) FROM lesson_passes WHERE user_id = ?1 AND passed_at IS NOT NULL GROUP BY course_id",
    )?;
    let rows = stmt.query_map([user_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize)))?;
    rows.collect()
}

pub fn load_one(conn: &Connection, user_id: i64, course_id: &str, lesson_id: &str) -> rusqlite::Result<Option<LessonPass>> {
    conn.query_row_cached(
        &format!("SELECT {COLS} FROM lesson_passes WHERE user_id = ?1 AND course_id = ?2 AND lesson_id = ?3"),
        params![user_id, course_id, lesson_id],
        row,
    )
    .optional()
}

/// Lesson ids passed in this course.
pub fn passed_ids(conn: &Connection, user_id: i64, course_id: &str) -> rusqlite::Result<std::collections::HashSet<String>> {
    let mut stmt = conn.prepare_cached(
        "SELECT lesson_id FROM lesson_passes WHERE user_id = ?1 AND course_id = ?2 AND passed_at IS NOT NULL",
    )?;
    let rows = stmt.query_map(params![user_id, course_id], |r| r.get::<_, String>(0))?;
    rows.collect()
}

fn save(conn: &Connection, user_id: i64, course_id: &str, p: &LessonPass, at: i64) -> rusqlite::Result<()> {
    conn.execute_cached(
        "INSERT INTO lesson_passes
            (user_id, course_id, lesson_id, seen_at, seen_via, best_pct, best_via, passed_at, passed_via, runs, evidence, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
         ON CONFLICT(user_id, course_id, lesson_id) DO UPDATE SET
            seen_at = excluded.seen_at, seen_via = excluded.seen_via, best_pct = excluded.best_pct,
            best_via = excluded.best_via, passed_at = excluded.passed_at, passed_via = excluded.passed_via,
            runs = excluded.runs, evidence = excluded.evidence, updated_at = excluded.updated_at",
        params![
            user_id,
            course_id,
            p.lesson_id,
            p.seen_at,
            p.seen_via,
            p.best_pct,
            p.best_via,
            p.passed_at,
            p.passed_via,
            p.runs,
            p.evidence,
            at
        ],
    )?;
    Ok(())
}

fn outcome(lesson: &Lesson, p: &LessonPass, passed_now: bool, run: Option<(i64, bool)>) -> Outcome {
    Outcome {
        lesson_id: p.lesson_id.clone(),
        seen: p.seen(),
        passed: p.passed(),
        passed_now,
        best_pct: p.best_pct,
        run_pct: run.map(|r| r.0),
        run_counted: run.map(|r| r.1).unwrap_or(false),
        required_answers: required_answers(lesson),
        summary: p.summary(),
    }
}

/// The learner was taught the lesson (via `via`). The first sighting is kept.
pub fn mark_seen(conn: &Connection, user_id: i64, course_id: &str, lesson: &Lesson, via: &str) -> rusqlite::Result<Outcome> {
    mark_seen_at(conn, user_id, course_id, lesson, via, now())
}

/// [`mark_seen`] at a given time (the time passed in).
pub(crate) fn mark_seen_at(conn: &Connection, user_id: i64, course_id: &str, lesson: &Lesson, via: &str, at: i64) -> rusqlite::Result<Outcome> {
    let mut p = load_one(conn, user_id, course_id, &lesson.id)?.unwrap_or_else(|| LessonPass::new(&lesson.id));
    if p.seen_at.is_none() {
        p.seen_at = Some(at);
        p.seen_via = Some(via.to_string());
    }
    // A lesson with no questions needs nothing more than being seen: seeing
    // it is a run with nothing to answer. Without this, Study mode (where *I've
    // read it* is all there is to do) could never pass it.
    if required_answers_via(lesson, via) == 0 && p.best_pct < PASS_PCT {
        p.best_pct = pct(0, 0);
        p.best_via = Some(via.to_string());
    }
    // Seen anywhere is read for Study mode too: the lesson joins recommended practice.
    crate::mastery::mark_read(conn, user_id, course_id, &lesson.id, at)?;
    let passed_now = p.settle(via, at);
    save(conn, user_id, course_id, &p, at)?;
    Ok(outcome(lesson, &p, passed_now, None))
}

/// One run of the lesson's exercises. A run with too few answers is
/// reported back but changes nothing.
pub fn record_run(
    conn: &Connection,
    user_id: i64,
    course_id: &str,
    lesson: &Lesson,
    via: &str,
    correct: i64,
    answered: i64,
) -> rusqlite::Result<Outcome> {
    record_run_at(conn, user_id, course_id, lesson, via, correct, answered, now())
}

/// [`record_run`] at a given time (the time passed in).
#[allow(clippy::too_many_arguments)]
pub(crate) fn record_run_at(
    conn: &Connection,
    user_id: i64,
    course_id: &str,
    lesson: &Lesson,
    via: &str,
    correct: i64,
    answered: i64,
    at: i64,
) -> rusqlite::Result<Outcome> {
    let mut p = load_one(conn, user_id, course_id, &lesson.id)?.unwrap_or_else(|| LessonPass::new(&lesson.id));
    let run_pct = pct(correct, answered);
    let counted = answered >= required_answers_via(lesson, via);
    let mut passed_now = false;
    if counted {
        p.runs += 1;
        if run_pct > p.best_pct {
            p.best_pct = run_pct;
            p.best_via = Some(via.to_string());
        }
        passed_now = p.settle(via, at);
        save(conn, user_id, course_id, &p, at)?;
    }
    Ok(outcome(lesson, &p, passed_now, Some((run_pct, counted))))
}

/// A run that also taught the lesson (a Sprint mode session, a Guide stage, a
/// tutor or agent who taught it first).
pub fn record_taught_run(
    conn: &Connection,
    user_id: i64,
    course_id: &str,
    lesson: &Lesson,
    via: &str,
    correct: i64,
    answered: i64,
) -> rusqlite::Result<Outcome> {
    mark_seen(conn, user_id, course_id, lesson, via)?;
    record_run(conn, user_id, course_id, lesson, via, correct, answered)
}

/// Keep an agent's note on how the learner showed it.
pub fn set_evidence(conn: &Connection, user_id: i64, course_id: &str, lesson_id: &str, evidence: &str) -> rusqlite::Result<()> {
    let note: String = evidence.trim().chars().take(MAX_EVIDENCE_CHARS).collect();
    conn.execute(
        "UPDATE lesson_passes SET evidence = ?4 WHERE user_id = ?1 AND course_id = ?2 AND lesson_id = ?3",
        params![user_id, course_id, lesson_id, note],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lesson(questions: usize) -> Lesson {
        let quiz: Vec<String> = (0..questions)
            .map(|i| format!(r#"{{"question_id":"q{i}","prompt":"p","type":"multiple-choice","options":["a","b"],"correct_answer":"a"}}"#))
            .collect();
        let json = format!(r#"{{"id":"l1","slug":"l1","title":"L","unit":"1","content":"x","quiz":[{}]}}"#, quiz.join(","));
        serde_json::from_str(&json).unwrap()
    }

    #[test]
    fn the_rule() {
        let l = lesson(10);
        assert_eq!(required_answers(&l), 3);
        assert!(run_passes(&l, 7, 10));
        assert!(!run_passes(&l, 6, 10));
        assert!(!run_passes(&l, 2, 2), "too few answers");
        let short = lesson(2);
        assert_eq!(required_answers(&short), 2);
        assert!(run_passes(&short, 2, 2));
        assert!(run_passes(&lesson(0), 0, 0), "nothing to answer");
        assert_eq!(pct(0, 0), 100);
    }

    #[test]
    fn seen_and_shown_in_any_order_and_mode() {
        let db = crate::db::Db::open_in_memory().unwrap();
        let l = lesson(10);
        db.with(|c| {
            let uid = crate::auth::create_user(c, "ann@example.org", "h")?;
            // A good run first (Study mode quiz): not yet passed.
            let o = record_run(c, uid, "es", &l, VIA_STUDY, 4, 5)?;
            assert!(o.run_counted && !o.passed && !o.seen);
            // Too short a run changes nothing.
            let o = record_run(c, uid, "es", &l, VIA_SPRINT, 1, 1)?;
            assert!(!o.run_counted);
            assert_eq!(load_one(c, uid, "es", "l1")?.unwrap().runs, 1);
            // Reading it in Sprint mode passes it, credited to the mode that completed it.
            let o = mark_seen(c, uid, "es", &l, VIA_SPRINT)?;
            assert!(o.passed && o.passed_now);
            // Seen in Sprint mode is read for Study mode's recommended practice too.
            assert!(crate::mastery::load_one(c, uid, "es", "l1")?.unwrap().is_read());
            let p = load_one(c, uid, "es", "l1")?.unwrap();
            assert_eq!(p.passed_via.as_deref(), Some(VIA_SPRINT));
            assert_eq!((via_id(VIA_SPRINT), via_id(VIA_STUDY), via_id("agent")), ("sprint", "study", "agent"));
            assert_eq!(p.best_via.as_deref(), Some(VIA_STUDY));
            assert_eq!(p.summary(), "Passed in Sprint mode");
            // A bad run later never takes the pass away.
            let o = record_run(c, uid, "es", &l, VIA_PLAY, 0, 5)?;
            assert!(o.passed && !o.passed_now);
            assert_eq!(passed_ids(c, uid, "es")?.len(), 1);
            assert!(passed_ids(c, uid, "fr")?.is_empty());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn a_lesson_without_questions_passes_when_seen() {
        let db = crate::db::Db::open_in_memory().unwrap();
        let l = lesson(0);
        db.with(|c| {
            let uid = crate::auth::create_user(c, "ann@example.org", "h")?;
            // Study mode's "I've read it" is the only report such a lesson gets there.
            let o = mark_seen(c, uid, "es", &l, VIA_STUDY)?;
            assert!(o.passed && o.passed_now);
            assert_eq!(load_one(c, uid, "es", "l1")?.unwrap().summary(), "Passed in Study mode");
            // A lesson with questions still needs its run.
            let bob = crate::auth::create_user(c, "bob@example.org", "h")?;
            let o = mark_seen(c, bob, "es", &lesson(3), VIA_STUDY)?;
            assert!(o.seen && !o.passed);
            // The courses page counts passes, not lessons merely seen.
            assert_eq!(passed_by_course(c, uid)?.get("es"), Some(&1));
            assert_eq!(passed_by_course(c, bob)?.get("es"), None);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn a_weak_taught_run_is_seen_but_not_passed() {
        let db = crate::db::Db::open_in_memory().unwrap();
        let l = lesson(10);
        db.with(|c| {
            let uid = crate::auth::create_user(c, "ann@example.org", "h")?;
            let o = record_taught_run(c, uid, "es", &l, "tutor", 3, 5)?;
            assert!(o.seen && !o.passed);
            assert_eq!(o.summary, "Seen · best run 60%, 70% passes");
            let o = record_taught_run(c, uid, "es", &l, "agent", 5, 5)?;
            assert!(o.passed_now);
            set_evidence(c, uid, "es", "l1", "Explained it back and wrote three sentences.")?;
            assert!(load_one(c, uid, "es", "l1")?.unwrap().evidence.starts_with("Explained"));
            Ok(())
        })
        .unwrap();
    }
}
