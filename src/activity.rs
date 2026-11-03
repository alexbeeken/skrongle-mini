//! Learning activity shared by every mode (#558): the daily goal's effort
//! points and the lesson every mode resumes at. Each mode reports what the
//! learner did with [`record`] (and [`touch`] for a lesson merely opened);
//! the home page, Sprint mode, Study mode and the Guide read the same
//! numbers back. The rule is in `docs/progress.md` ("Daily goal" and "Where
//! you left off").
//!
//! The daily goal counts *effort points*, not answers, so a learner who
//! reads, plays a game or reviews flashcards moves it as much as one who
//! answers quizzes. A kind's weight is roughly the time one of it takes,
//! with a typed quiz answer (about 15 seconds) as one point;
//! `scripts/effort-calibrate.sh` measures the medians from a database. The
//! weights are read when the points are added up, so changing one here
//! changes every day already logged too.

use std::collections::HashMap;

use rusqlite::{params, Connection};

use crate::content::{ContentStore, Lesson};
use crate::db::now;
use crate::progress::LessonPass;

/// Effort points a day's goal asks for: about five minutes of practice.
pub const DAILY_GOAL: i64 = 20;

/// Something a learner did that counts towards the daily goal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A course quiz question, answered in any mode (Study practice or a
    /// reading check, a Guide quick check, Sprint mode, an agent).
    Answer,
    /// A question in a conversation with the AI tutor.
    TutorAnswer,
    /// A Sprint mode exercise that is not a quiz question (build, sort,
    /// ladder, pairs, the listening ones).
    Exercise,
    /// One item of a game round: a pair, a number, a date, a picture, a
    /// form in the paradigm drill.
    GameItem,
    /// One sentence of a listening round.
    Listening,
    /// One flashcard reviewed.
    Flashcard,
    /// One of a Guide stage's concept cards.
    Card,
    /// A lesson read in Study mode (its reading check passed, or "I've read
    /// it" on a lesson without questions).
    Reading,
}

impl Kind {
    pub const ALL: [Kind; 8] =
        [Kind::Answer, Kind::TutorAnswer, Kind::Exercise, Kind::GameItem, Kind::Listening, Kind::Flashcard, Kind::Card, Kind::Reading];

    /// Effort points for one of these.
    pub const fn points(self) -> f64 {
        match self {
            Kind::Answer => 1.0,
            Kind::TutorAnswer => 1.5,
            Kind::Exercise => 0.6,
            Kind::GameItem => 0.4,
            Kind::Listening => 1.0,
            Kind::Flashcard => 0.3,
            Kind::Card => 0.3,
            Kind::Reading => 4.0,
        }
    }

    /// The name stored in `activity_log.kind`.
    pub const fn key(self) -> &'static str {
        match self {
            Kind::Answer => "answer",
            Kind::TutorAnswer => "tutor",
            Kind::Exercise => "exercise",
            Kind::GameItem => "game",
            Kind::Listening => "listening",
            Kind::Flashcard => "flashcard",
            Kind::Card => "card",
            Kind::Reading => "reading",
        }
    }

    fn of_key(key: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.key() == key)
    }
}

/// Where a lesson was last worked on, so "Continue" can open it there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Sprint,
    Study,
    Guide,
    Play,
    Tutor,
    Agent,
}

impl Mode {
    pub const fn key(self) -> &'static str {
        match self {
            Mode::Sprint => "sprint",
            Mode::Study => "study",
            Mode::Guide => "guide",
            Mode::Play => "play",
            Mode::Tutor => "tutor",
            Mode::Agent => "agent",
        }
    }

    /// The mode a stored name stands for. Rows from before #558 (seen in
    /// `lesson_passes`, answered in `user_progress`) name their mode the
    /// way `progress.rs` does; anything unknown is Study mode.
    pub fn of_key(key: &str) -> Mode {
        match key {
            "sprint" | crate::progress::VIA_SPRINT => Mode::Sprint,
            "guide" | crate::progress::VIA_PLAY => Mode::Guide,
            "play" => Mode::Play,
            "tutor" => Mode::Tutor,
            "agent" => Mode::Agent,
            _ => Mode::Study,
        }
    }

    /// The page that picks a lesson up again in this mode. Play mode's
    /// games, the tutor and agents go to the lesson page: it has every way
    /// on from there.
    pub fn continue_url(self, slug: &str) -> String {
        match self {
            Mode::Sprint => crate::url(&format!("/sprint/play?lesson={slug}")),
            Mode::Guide => crate::url(&format!("/guide/{slug}")),
            Mode::Study | Mode::Play | Mode::Tutor | Mode::Agent => crate::url(&format!("/lessons/{slug}")),
        }
    }
}

/// Log `n` of `kind` (nothing for `n <= 0`). This is effort only: what
/// lesson to resume at comes from [`touch`], which only lesson-focused pages
/// call, so a review mix's last question does not move it. `tz` is the
/// learner's offset from UTC in seconds (`CurrentUser::tz`): it decides
/// which local day the effort lands on in their running totals
/// (`earnings::Ledger`: the day streak, the daily goal reached).
pub fn record(conn: &Connection, user_id: i64, course_id: &str, kind: Kind, n: i64, tz: i64) -> rusqlite::Result<()> {
    record_at(conn, user_id, course_id, kind, n, now(), tz)
}

/// [`record`] at a given time: the dev crowd's backdated history (`dev_crowd.rs`).
pub(crate) fn record_at(conn: &Connection, user_id: i64, course_id: &str, kind: Kind, n: i64, at: i64, tz: i64) -> rusqlite::Result<()> {
    if n > 0 {
        // The ledger first: a learner from before it gets it from the log as
        // it stands, without this row, which is then added to both.
        crate::earnings::log_effort(conn, user_id, course_id, kind.points() * n as f64, at, tz)?;
        conn.prepare_cached("INSERT INTO activity_log (user_id, course_id, kind, n, at) VALUES (?1, ?2, ?3, ?4, ?5)")?
            .execute(params![user_id, course_id, kind.key(), n, at])?;
    }
    Ok(())
}

/// The learner opened or worked on one lesson in `mode` (its page, its
/// Guide stage, a Sprint mode session or a round on it alone): it becomes the
/// one to resume at, unless it is passed.
pub fn touch(conn: &Connection, user_id: i64, course_id: &str, lesson_id: &str, mode: Mode) -> rusqlite::Result<()> {
    touch_at(conn, user_id, course_id, lesson_id, mode, now())
}

/// Make `lesson_id` the one to resume at, in the mode last worked in (a
/// level change, #333): touched after every other lesson, even one touched
/// this same second.
pub fn move_to(conn: &Connection, user_id: i64, course_id: &str, lesson_id: &str) -> rusqlite::Result<()> {
    let last = last_touched(conn, user_id, course_id)?.into_values().max_by_key(|(at, _)| *at);
    let mode = last.map_or(Mode::Study, |(_, m)| m);
    let at = last.map_or(0, |(at, _)| at + 1).max(now());
    touch_at(conn, user_id, course_id, lesson_id, mode, at)
}

/// [`touch`] at a given time (also the dev crowd's backdated history, `dev_crowd.rs`).
pub(crate) fn touch_at(conn: &Connection, user_id: i64, course_id: &str, lesson_id: &str, mode: Mode, at: i64) -> rusqlite::Result<()> {
    conn.prepare_cached(
        "INSERT INTO lesson_touches (user_id, course_id, lesson_id, mode, at) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (user_id, course_id, lesson_id) DO UPDATE SET mode = excluded.mode, at = excluded.at",
    )?
    .execute(params![user_id, course_id, lesson_id, mode.key(), at])?;
    Ok(())
}

/// What earns how many points, for the daily goal's (i): (what, points).
pub fn table() -> Vec<(String, String)> {
    Kind::ALL
        .into_iter()
        .map(|k| {
            let pts = k.points();
            let shown = if pts.fract() == 0.0 { format!("{pts:.0}") } else { format!("{pts:.1}") };
            (crate::t(&format!("goal.kind.{}", k.key())), shown)
        })
        .collect()
}

/// Effort points earned since `since`, rounded down.
pub fn points_since(conn: &Connection, user_id: i64, course_id: &str, since: i64) -> rusqlite::Result<i64> {
    let mut stmt = conn.prepare_cached(
        "SELECT kind, SUM(n) FROM activity_log WHERE user_id = ?1 AND course_id = ?2 AND at >= ?3 GROUP BY kind",
    )?;
    let rows = stmt.query_map(params![user_id, course_id, since], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
    let mut total = 0.0;
    for row in rows {
        let (key, n) = row?;
        total += Kind::of_key(&key).map_or(0.0, Kind::points) * n as f64;
    }
    Ok(total.floor() as i64)
}

/// Today's effort against the goal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Today {
    pub points: i64,
    pub goal: i64,
}

impl Today {
    pub fn load(conn: &Connection, user_id: i64, course_id: &str, day_start: i64) -> rusqlite::Result<Today> {
        Ok(Today { points: points_since(conn, user_id, course_id, day_start)?, goal: DAILY_GOAL })
    }

    /// How full the ring is, 0–100.
    pub fn pct(&self) -> i64 {
        if self.goal <= 0 {
            100
        } else {
            (self.points * 100 / self.goal).clamp(0, 100)
        }
    }

    pub fn done(&self) -> bool {
        self.points >= self.goal
    }

    /// Points still to go, 0 once the goal is met.
    pub fn left(&self) -> i64 {
        (self.goal - self.points).max(0)
    }
}

/// When and in which mode each lesson was last worked on: lesson id ->
/// (time, mode). Before #558 nothing logged touches, so the time a lesson
/// was first seen (`lesson_passes`) stands in for one.
pub fn last_touched(conn: &Connection, user_id: i64, course_id: &str) -> rusqlite::Result<HashMap<String, (i64, Mode)>> {
    let mut stmt = conn.prepare_cached(
        "SELECT lesson_id, at, mode FROM lesson_touches WHERE user_id = ?1 AND course_id = ?2
         UNION ALL
         SELECT lesson_id, seen_at, COALESCE(seen_via, '') FROM lesson_passes WHERE user_id = ?1 AND course_id = ?2 AND seen_at IS NOT NULL",
    )?;
    let rows = stmt.query_map(params![user_id, course_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?)))?;
    let mut out: HashMap<String, (i64, Mode)> = HashMap::new();
    for row in rows {
        let (lesson, at, mode) = row?;
        let mode = Mode::of_key(&mode);
        // Ties go to the first row: lesson_touches knows the mode best.
        match out.get(&lesson) {
            Some((t, _)) if *t >= at => {}
            _ => {
                out.insert(lesson, (at, mode));
            }
        }
    }
    Ok(out)
}

/// Where the learner left off.
#[derive(Debug, Clone)]
pub struct Resume<'a> {
    pub lesson: &'a Lesson,
    /// The mode the learner last worked in (Study mode before anything):
    /// "Continue" opens the lesson there.
    pub mode: Mode,
}

/// The lesson every mode resumes at: the one last worked on, in any mode,
/// unless it is passed; then the next lesson after it that is neither
/// passed nor skipped (from the start of the course if none is left after
/// it). With nothing worked on yet, the first lesson that is neither.
/// `None` once every lesson is passed or skipped.
pub fn resume<'a>(
    content: &'a ContentStore,
    passes: &HashMap<String, LessonPass>,
    touched: &HashMap<String, (i64, Mode)>,
    start: usize,
) -> Option<Resume<'a>> {
    let lessons = content.lessons();
    let open = |l: &Lesson| {
        let passed = passes.get(&l.id).is_some_and(LessonPass::passed);
        !passed && !crate::progress::is_skipped(l, start, touched.contains_key(&l.id) || passes.get(&l.id).is_some_and(LessonPass::seen))
    };
    let last = touched
        .iter()
        .filter_map(|(id, (at, mode))| Some((lessons.iter().position(|l| &l.id == id)?, *at, *mode)))
        .max_by_key(|(pos, at, _)| (*at, *pos));
    let mode = last.map_or(Mode::Study, |(_, _, m)| m);
    let from = match last {
        Some((pos, _, _)) => {
            let lesson = lessons.get(pos)?;
            if open(lesson) {
                return Some(Resume { lesson, mode });
            }
            pos + 1
        }
        None => 0,
    };
    let after = lessons.get(from..).unwrap_or_default().iter();
    let before = lessons.get(..from).unwrap_or_default().iter();
    after.chain(before).find(|l| open(l)).map(|lesson| Resume { lesson, mode })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    /// Lessons l0..l7: four in unit A1, four in A2.
    fn course() -> ContentStore {
        let lessons: Vec<String> = (0..8)
            .map(|i| {
                let unit = if i < 4 { "A1" } else { "A2" };
                format!(r#"{{"id":"l{i}","slug":"l{i}","title":"L{i}","unit":"{unit}","content":"x","quiz":[{{"question_id":"q{i}","prompt":"p","type":"fill-in-blank","correct_answer":"a"}}]}}"#)
            })
            .collect();
        crate::content::tests::store(&lessons.iter().map(String::as_str).collect::<Vec<_>>()).unwrap()
    }

    fn user(c: &Connection) -> i64 {
        crate::auth::create_user(c, "ann@example.org", "h").unwrap()
    }

    #[test]
    fn points_weigh_each_kind() {
        let db = Db::open_in_memory().unwrap();
        db.with(|c| {
            let uid = user(c);
            record_at(c, uid, "es", Kind::Answer, 3, 100, 0)?;
            record_at(c, uid, "es", Kind::Reading, 1, 100, 0)?;
            record_at(c, uid, "es", Kind::GameItem, 5, 100, 0)?;
            record_at(c, uid, "es", Kind::Flashcard, 0, 100, 0)?;
            record_at(c, uid, "fr", Kind::Answer, 9, 100, 0)?;
            record_at(c, uid, "es", Kind::Answer, 9, 10, 0)?;
            // 3 × 1.0 + 4.0 + 5 × 0.4; the French answers and yesterday's are not today's.
            assert_eq!(points_since(c, uid, "es", 50)?, 9);
            let today = Today { points: 9, goal: 20 };
            assert_eq!((today.pct(), today.left(), today.done()), (45, 11, false));
            assert_eq!(Today { points: 30, goal: 20 }.pct(), 100);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn every_kind_has_its_own_key_and_text() {
        let table = table();
        for (k, (what, pts)) in Kind::ALL.into_iter().zip(&table) {
            assert_eq!(Kind::of_key(k.key()), Some(k));
            assert!(k.points() > 0.0);
            assert!(!what.starts_with("goal.kind."), "no text for {}", k.key());
            assert!(pts.parse::<f64>().is_ok(), "{pts}");
        }
        assert_eq!(table.len(), Kind::ALL.len());
    }

    #[test]
    fn resume_follows_the_last_lesson_touched() {
        let content = course();
        let lessons = content.lessons();
        let (a, b, c) = (&lessons[0], &lessons[1], &lessons[5]);
        let mut passes: HashMap<String, LessonPass> = HashMap::new();
        let mut touched: HashMap<String, (i64, Mode)> = HashMap::new();

        // Nothing yet: the first lesson.
        assert_eq!(resume(&content, &passes, &touched, 0).map(|r| r.lesson.id.clone()), Some(a.id.clone()));

        // A lesson further on, worked on last in Sprint mode: there, in Sprint mode.
        touched.insert(a.id.clone(), (10, Mode::Study));
        touched.insert(c.id.clone(), (20, Mode::Sprint));
        let r = resume(&content, &passes, &touched, 0).unwrap();
        assert_eq!((r.lesson.id.as_str(), r.mode), (c.id.as_str(), Mode::Sprint));

        // Passed: the next lesson after it that is not, still in Sprint mode.
        let passed = |id: &str| LessonPass { lesson_id: id.into(), seen_at: Some(1), passed_at: Some(2), ..Default::default() };
        passes.insert(c.id.clone(), passed(&c.id));
        let r = resume(&content, &passes, &touched, 0).unwrap();
        assert_eq!((r.lesson.id.as_str(), r.mode), (lessons[6].id.as_str(), Mode::Sprint));

        // Every lesson after it passed: back to the first one that is not.
        for l in &lessons[5..] {
            passes.insert(l.id.clone(), passed(&l.id));
        }
        assert_eq!(resume(&content, &passes, &touched, 0).map(|r| r.lesson.id.clone()), Some(a.id.clone()));
        passes.insert(a.id.clone(), passed(&a.id));
        assert_eq!(resume(&content, &passes, &touched, 0).map(|r| r.lesson.id.clone()), Some(b.id.clone()));
        for l in lessons {
            passes.insert(l.id.clone(), passed(&l.id));
        }
        assert!(resume(&content, &passes, &touched, 0).is_none());
    }

    #[test]
    fn resume_passes_over_skipped_lessons() {
        let content = course();
        let none = HashMap::new();
        let r = resume(&content, &none, &HashMap::new(), 1).unwrap();
        assert_eq!(r.lesson.id, "l4", "the first lesson of A2");
        // A skipped lesson the learner opens anyway is theirs.
        let first = &content.lessons()[0];
        let touched = HashMap::from([(first.id.clone(), (5, Mode::Guide))]);
        let r = resume(&content, &none, &touched, 1).unwrap();
        assert_eq!((r.lesson.id.as_str(), r.mode), (first.id.as_str(), Mode::Guide));
    }

    #[test]
    fn moving_to_a_lesson_makes_it_the_one_to_resume_at() {
        let db = Db::open_in_memory().unwrap();
        let content = course();
        db.with(|c| {
            let uid = user(c);
            let far = now() + 100;
            touch_at(c, uid, "es", "l5", Mode::Guide, far)?;
            // A level change: back to l0, even though l5 was touched "later".
            move_to(c, uid, "es", "l0")?;
            let t = last_touched(c, uid, "es")?;
            assert_eq!(t.get("l0"), Some(&(far + 1, Mode::Guide)));
            let r = resume(&content, &HashMap::new(), &t, 0).unwrap();
            assert_eq!((r.lesson.id.as_str(), r.mode), ("l0", Mode::Guide));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn last_touched_reads_older_progress_too() {
        let db = Db::open_in_memory().unwrap();
        db.with(|c| {
            let uid = user(c);
            c.execute(
                "INSERT INTO lesson_passes (user_id, course_id, lesson_id, seen_at, seen_via, updated_at) VALUES (?1, 'es', 'old', 50, 'd', 50)",
                [uid],
            )?;
            touch_at(c, uid, "es", "new", Mode::Guide, 60)?;
            touch_at(c, uid, "es", "new", Mode::Study, 70)?;
            let t = last_touched(c, uid, "es")?;
            assert_eq!(t.get("old"), Some(&(50, Mode::Sprint)));
            assert_eq!(t.get("new"), Some(&(70, Mode::Study)));
            Ok(())
        })
        .unwrap();
    }
}
