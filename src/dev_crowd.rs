//! A crowd of made-up learners with weeks of history (#583), for testing
//! what depends on many people's behaviour over time: the Social
//! motivator's comparisons, streaks, the daily goal, mastery decay, the
//! History page, the retention jobs, and how the queries behind them hold
//! up with a thousand accounts in the tables.
//!
//! `SKRONGLE_DEV_SEED_USERS=1000` (`./.dev-start --seed-users 1000`) makes
//! that many learners at boot, each with up to `SKRONGLE_DEV_SEED_DAYS` (90)
//! days of activity ending now. Dev mode only.
//!
//! Every learner is a small model: when they joined, how keen they are
//! (the chance of studying on a given day), how good (the chance of a right
//! answer), which mode they prefer, their time zone and usual hour, and
//! whether they drifted away. A day they study is a session: open the
//! lesson they are on, read or play it, answer its questions, now and then
//! a game round or a review of an earlier lesson. Every write goes through
//! the same functions the modes call (`progress`, `mastery`, `activity`,
//! `games`), only at a time in the past, so the rows obey every rule the
//! real ones do and keep doing so when a rule changes.
//!
//! The crowd is deterministic: learner `i` always gets the same life, so a
//! second boot adds nothing (it only tops the crowd up to the number asked
//! for), and two developers see the same numbers. Their addresses end in
//! [`DOMAIN`]; the password is the dev user's, so any of them can sign in.

use std::sync::Arc;

use rusqlite::{params, Connection};

use crate::activity::{self, Kind, Mode};
use crate::content::{ContentStore, Lesson};
use crate::db::{now, Db};
use crate::progress::{self, VIA_PLAY, VIA_SPRINT, VIA_STUDY};
use crate::quiz::Rng;

/// The crowd's e-mail domain (`learner-<n>@seed.example`). `.example` is
/// reserved and never resolves, so nobody real is ever written to.
pub const DOMAIN: &str = "seed.example";

/// Learners made per transaction. Smaller batches keep the memory of one
/// transaction low on a small box; larger ones are faster.
const BATCH: usize = 50;

/// The longest gap between two `dev crowd: n/N learners` lines. `.dev-start`
/// waits 20 s for the server and starts over on each new line, and one
/// batch can take longer than that on a busy machine with `SKRONGLE_SQL_TRACE`
/// on (#936), so a line also goes out in the middle of a batch.
const PROGRESS_EVERY: std::time::Duration = std::time::Duration::from_secs(5);

/// Common offsets from UTC, in hours, so the crowd's days start at
/// different times (the Social motivator buckets effort by quarter-hour).
const ZONES: [f64; 13] = [-8.0, -7.0, -6.0, -5.0, -3.0, 0.0, 1.0, 2.0, 3.0, 5.5, 8.0, 9.0, 10.0];

/// First names for the profiles, so pages that show one look lived in.
const NAMES: [&str; 24] = [
    "Ana", "Ben", "Chloe", "Dev", "Elena", "Farid", "Grace", "Hugo", "Ines", "Jonas", "Kai", "Lena", "Mateo", "Nia", "Omar", "Priya", "Quinn",
    "Rosa", "Sam", "Tomas", "Uma", "Vera", "Wen", "Yuki",
];

/// How a learner studies: the mode most of their sessions use.
#[derive(Clone, Copy)]
enum Prefers {
    Sprint,
    Study,
    Guide,
}

/// One learner's habits, drawn once from their index.
struct Learner {
    index: usize,
    email: String,
    name: &'static str,
    /// Offset from UTC in seconds.
    tz: i64,
    /// The day they joined, as days before today (0 = today).
    joined_days_ago: i64,
    /// Days before today they stopped, if they drifted away.
    left_days_ago: Option<i64>,
    /// Chance of studying on a given day.
    keen: f64,
    /// Chance of a right answer.
    accuracy: f64,
    /// Usual local hour of the day, 0-23.
    hour: i64,
    prefers: Prefers,
    /// No write later than this: history ends now.
    deadline: i64,
}

impl Learner {
    fn draw(index: usize, days: i64, today: i64, rng: &mut Rng) -> Learner {
        // Joining dates spread over the window, with a few from its first day
        // (the long-standing learners every window needs).
        let joined_days_ago = if rng.next_f64() < 0.15 { days } else { (rng.next_f64() * days as f64) as i64 };
        // A third drift away somewhere along the way.
        let left_days_ago = (rng.next_f64() < 0.33).then(|| (rng.next_f64() * joined_days_ago as f64) as i64);
        let keen = match rng.below(10) {
            0..=2 => 0.15 + rng.next_f64() * 0.15, // casual: once or twice a week
            3..=6 => 0.35 + rng.next_f64() * 0.3,  // regular
            _ => 0.75 + rng.next_f64() * 0.22,     // daily
        };
        let prefers = match rng.below(10) {
            0..=4 => Prefers::Sprint,
            5..=7 => Prefers::Study,
            _ => Prefers::Guide,
        };
        Learner {
            index,
            email: format!("learner-{index}@{DOMAIN}"),
            name: NAMES.get(rng.below(NAMES.len())).copied().unwrap_or("Ana"),
            tz: (ZONES.get(rng.below(ZONES.len())).copied().unwrap_or(0.0) * 3600.0) as i64,
            joined_days_ago,
            left_days_ago,
            keen,
            accuracy: 0.55 + rng.next_f64() * 0.42,
            hour: 7 + rng.below(16) as i64,
            prefers,
            deadline: today - 1,
        }
    }
}

/// How many learners the crowd has.
pub fn count(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM users WHERE email LIKE ?1", [format!("%@{DOMAIN}")], |r| r.get(0))
}

/// Make the crowd `users` strong, with up to `days` days of history each,
/// across the given courses (most in the first). Learners already there
/// stay as they are; only the missing ones are made. Returns how many were
/// added.
pub fn seed(db: &Db, courses: &[Arc<ContentStore>], users: usize, days: i64) -> Result<usize, String> {
    if courses.is_empty() {
        return Err("no course to seed a crowd into".into());
    }
    let days = days.max(1);
    let existing = db.with(|c| count(c)).map_err(|e| e.to_string())? as usize;
    if existing >= users {
        return Ok(0);
    }
    let hash = crate::auth::hash_password(crate::dev::DEV_PASSWORD).map_err(|e| format!("{e:?}"))?;
    let started = std::time::Instant::now();
    let today = now();
    let mut added = 0;
    tracing::info!("dev crowd: {existing}/{users} learners");
    let mut said = std::time::Instant::now();
    for from in (existing..users).step_by(BATCH) {
        let to = (from + BATCH).min(users);
        db.with(|c| {
            let tx = c.transaction()?;
            for index in from..to {
                let mut rng = Rng::seeded(0x5EED_C0DE ^ (index as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15));
                let learner = Learner::draw(index, days, today, &mut rng);
                // Most of the crowd learns the first course; the rest spread over the others.
                let pick = if courses.len() == 1 || rng.next_f64() < 0.8 { 0 } else { 1 + rng.below(courses.len() - 1) };
                let Some(course) = courses.get(pick).or(courses.first()) else { continue };
                live(&tx, &learner, course, &hash, today, &mut rng)?;
                if index + 1 < to && said.elapsed() >= PROGRESS_EVERY {
                    tracing::info!("dev crowd: {}/{} learners", index + 1, users);
                    said = std::time::Instant::now();
                }
            }
            tx.commit()
        })
        .map_err(|e| e.to_string())?;
        added += to - from;
        tracing::info!("dev crowd: {}/{} learners", existing + added, users);
        said = std::time::Instant::now();
    }
    tracing::info!("dev crowd: {added} learner(s) with up to {days} days of history in {:.1}s", started.elapsed().as_secs_f64());
    Ok(added)
}

/// Create the learner's account and play out their days.
fn live(conn: &Connection, l: &Learner, course: &ContentStore, hash: &str, today: i64, rng: &mut Rng) -> rusqlite::Result<()> {
    let cid = course.id();
    let joined = crate::sprint::local_day_start(l.tz, today - l.joined_days_ago * 86_400) + l.hour * 3600 + rng.below(3600) as i64;
    let joined = joined.min(l.deadline - 60);
    let uid = crate::auth::create_user(conn, &l.email, hash)?;
    conn.execute(
        "UPDATE users SET created_at = ?2, email_verified_at = ?2, age_confirmed = 1, first_name = ?3, avatar_color = ?4 WHERE id = ?1",
        params![uid, joined, l.name, (l.index % 8) as i64],
    )?;
    // The terms, accepted when they signed up, so their pages open without the prompt.
    crate::terms::accept(conn, uid, crate::terms::version())?;
    conn.execute("UPDATE terms_acceptances SET accepted_at = ?2 WHERE user_id = ?1", params![uid, joined])?;
    crate::courses::open_project(conn, uid, cid)?;
    let mut last_opened = joined;

    let mut next = 0usize; // the lesson they are on: the first not passed
    let last_day = l.left_days_ago.unwrap_or(0);
    for days_ago in (last_day..=l.joined_days_ago).rev() {
        if days_ago != l.joined_days_ago && rng.next_f64() >= l.keen {
            continue;
        }
        let day = crate::sprint::local_day_start(l.tz, today - days_ago * 86_400);
        let at = (day + l.hour * 3600 + rng.below(5400) as i64 - 1800).max(joined);
        // Keen learners now and then do two sessions a day.
        let sessions = if rng.next_f64() < l.keen * 0.3 { 2 } else { 1 };
        for s in 0..sessions {
            let t = at + s * 3 * 3600;
            if t >= l.deadline {
                break;
            }
            next = session(conn, uid, cid, l, course, next, t, rng)?;
            last_opened = t;
        }
    }
    conn.execute("UPDATE projects SET created_at = ?3, last_opened_at = ?4, set_up = 1 WHERE user_id = ?1 AND course_id = ?2", params![uid, cid, joined, last_opened])?;
    Ok(())
}

/// One sitting, starting at `at`: the lesson they are on, in their mode,
/// then maybe a game or a review. Returns the lesson they are on afterwards.
/// Whatever would fall after the learner's deadline (now) is left out.
#[allow(clippy::too_many_arguments)]
fn session(conn: &Connection, uid: i64, cid: &str, l: &Learner, course: &ContentStore, mut next: usize, at: i64, rng: &mut Rng) -> rusqlite::Result<usize> {
    let lessons = course.lessons();
    let mut t = at;
    if t >= l.deadline {
        return Ok(next);
    }
    // A course they finished: they review and play.
    let Some(lesson) = lessons.get(next) else {
        if let Some(any) = lessons.get(rng.below(lessons.len())) {
            review(conn, uid, cid, l, course, any, &mut t, rng)?;
        }
        return Ok(next);
    };
    let mode = match (l.prefers, rng.below(10)) {
        (Prefers::Sprint, 0..=7) | (Prefers::Study, 8) | (Prefers::Guide, 8) => Prefers::Sprint,
        (Prefers::Study, 0..=7) | (Prefers::Sprint, 8) | (Prefers::Guide, 9) => Prefers::Study,
        _ => Prefers::Guide,
    };
    let (via, touched) = match mode {
        Prefers::Sprint => (VIA_SPRINT, Mode::Sprint),
        Prefers::Study => (VIA_STUDY, Mode::Study),
        Prefers::Guide => (VIA_PLAY, Mode::Guide),
    };
    activity::touch_at(conn, uid, cid, &lesson.id, touched, t)?;
    match mode {
        Prefers::Sprint => {
            // Bites and exercises before the questions.
            let n = 4 + rng.below(5) as i64;
            activity::record_at(conn, uid, cid, Kind::Exercise, n, t, l.tz)?;
            t = (t + n * 10).min(l.deadline);
            progress::mark_seen_at(conn, uid, cid, lesson, via, t)?;
        }
        Prefers::Study => {
            // Reading the page takes a few minutes; the check at its end makes it seen.
            t = (t + 120 + rng.below(240) as i64).min(l.deadline);
            activity::record_at(conn, uid, cid, Kind::Reading, 1, t, l.tz)?;
            progress::mark_seen_at(conn, uid, cid, lesson, via, t)?;
        }
        Prefers::Guide => {
            let n = 3 + rng.below(4) as i64;
            activity::record_at(conn, uid, cid, Kind::Card, n, t, l.tz)?;
            t = (t + n * 6).min(l.deadline);
            progress::mark_seen_at(conn, uid, cid, lesson, via, t)?;
        }
    }
    if t >= l.deadline {
        return Ok(next);
    }
    let (correct, answered) = answer(conn, uid, cid, l, course, lesson, lesson.quiz.len(), &mut t, rng)?;
    let outcome = progress::record_run_at(conn, uid, cid, lesson, via, correct, answered, t)?;
    if outcome.passed {
        next += 1;
        // On a roll: keen learners often go straight on to the next lesson.
        if next < lessons.len() && rng.next_f64() < l.keen * 0.5 {
            t += 30;
            return session(conn, uid, cid, l, course, next, t, rng);
        }
    }
    match rng.below(10) {
        0..=1 => {
            // A round of Word Match on this lesson's terms.
            let pairs = lesson.vocabulary.len().clamp(4, 12) as i64;
            let wrong = (0..pairs).filter(|_| rng.next_f64() > l.accuracy).count() as i64;
            let ms = pairs * (2500 + rng.below(2500) as i64);
            t += ms / 1000;
            if t >= l.deadline {
                return Ok(next);
            }
            crate::games::record_activity_at(conn, uid, cid, "matching", &lesson.id, &lesson.title, ms, pairs, pairs, Some(ms + wrong * 3000), t)?;
            activity::record_at(conn, uid, cid, Kind::GameItem, pairs, t, l.tz)?;
        }
        2..=3 if next > 0 => {
            // A look back at an earlier lesson keeps its mastery up.
            if let Some(earlier) = lessons.get(rng.below(next.min(lessons.len()))) {
                review(conn, uid, cid, l, course, earlier, &mut t, rng)?;
            }
        }
        _ => {}
    }
    Ok(next)
}

/// A few of a lesson's questions again, as a practice round: mastery and
/// history move, the pass (already earned or not) does not.
#[allow(clippy::too_many_arguments)]
fn review(conn: &Connection, uid: i64, cid: &str, l: &Learner, course: &ContentStore, lesson: &Lesson, t: &mut i64, rng: &mut Rng) -> rusqlite::Result<()> {
    let n = lesson.quiz.len().min(3 + rng.below(3));
    answer(conn, uid, cid, l, course, lesson, n, t, rng)?;
    Ok(())
}

/// Answer up to `n` of the lesson's questions, about fifteen seconds each,
/// right with the learner's accuracy (a right answer collects the words it
/// taught, as in every mode). Returns (correct, answered).
#[allow(clippy::too_many_arguments)]
fn answer(conn: &Connection, uid: i64, cid: &str, l: &Learner, course: &ContentStore, lesson: &Lesson, n: usize, t: &mut i64, rng: &mut Rng) -> rusqlite::Result<(i64, i64)> {
    let mut order: Vec<usize> = (0..lesson.quiz.len()).collect();
    rng.shuffle(&mut order);
    let mut correct = 0;
    let mut answered = 0;
    for q in order.into_iter().take(n).filter_map(|i| lesson.quiz.get(i)) {
        let right = rng.next_f64() < l.accuracy;
        if *t + 24 >= l.deadline {
            break;
        }
        *t += 8 + rng.below(16) as i64;
        let given = if right { q.correct_answer.as_str() } else { q.options.iter().find(|o| **o != q.correct_answer).map_or("?", |o| o.as_str()) };
        crate::mastery::record_answer_at(conn, uid, cid, &lesson.id, &q.question_id, given, right, *t)?;
        answered += 1;
        if right {
            correct += 1;
            let words: Vec<&str> = course.words_taught(lesson, q).into_iter().map(|w| w.term.as_str()).collect();
            crate::vocab::record_learned_at(conn, uid, cid, &lesson.id, &words, *t, l.tz)?;
        }
    }
    if answered > 0 {
        activity::record_at(conn, uid, cid, Kind::Answer, answered, *t, l.tz)?;
    }
    Ok((correct, answered))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn crowd(users: usize, days: i64) -> (Db, Vec<Arc<ContentStore>>) {
        let db = Db::open_in_memory().unwrap();
        let courses = crate::courses::Courses::load_builtin(Path::new("data")).unwrap();
        let courses: Vec<Arc<ContentStore>> = courses.builtin().iter().filter_map(|c| courses.builtin_store(c.id())).collect();
        assert_eq!(seed(&db, &courses, users, days).unwrap(), users);
        (db, courses)
    }

    #[test]
    fn the_crowd_has_history_in_the_past_through_the_real_rules() {
        let (db, courses) = crowd(40, 30);
        let cid = courses[0].id().to_string();
        db.with(|c| {
            assert_eq!(count(c)?, 40);
            let today = now();
            // Nothing in the future, nothing before the window.
            for (table, col) in [("activity_log", "at"), ("quiz_history", "answered_at"), ("lesson_touches", "at"), ("game_activity", "played_at"), ("user_vocab", "learned_at")] {
                let (n, future, old): (i64, i64, i64) = c.query_row(
                    &format!("SELECT COUNT(*), SUM({col} >= ?1), SUM({col} < ?1 - 32 * 86400) FROM {table}"),
                    [today],
                    |r| Ok((r.get(0)?, r.get::<_, Option<i64>>(1)?.unwrap_or(0), r.get::<_, Option<i64>>(2)?.unwrap_or(0))),
                )?;
                assert!(n > 0, "{table} is empty");
                assert_eq!((future, old), (0, 0), "{table}: {future} rows in the future, {old} before the window");
            }
            // Every learner opened a course and signed up when their history starts.
            let without: i64 = c.query_row("SELECT COUNT(*) FROM users u WHERE email LIKE '%@seed.example' AND NOT EXISTS (SELECT 1 FROM projects p WHERE p.user_id = u.id)", [], |r| r.get(0))?;
            assert_eq!(without, 0);
            let early: i64 = c.query_row("SELECT COUNT(*) FROM activity_log a JOIN users u ON u.id = a.user_id WHERE a.at < u.created_at", [], |r| r.get(0))?;
            assert_eq!(early, 0, "activity before the account was made");
            // Passes come from the same rule as everyone's: seen plus a counted run of 70 %.
            let bad: i64 = c.query_row("SELECT COUNT(*) FROM lesson_passes WHERE passed_at IS NOT NULL AND (seen_at IS NULL OR best_pct < 70)", [], |r| r.get(0))?;
            assert_eq!(bad, 0);
            let passes: i64 = c.query_row("SELECT COUNT(*) FROM lesson_passes WHERE passed_at IS NOT NULL", [], |r| r.get(0))?;
            assert!(passes > 40, "only {passes} passes for 40 learners");
            // Most of the crowd learns the first course.
            let first: i64 = c.query_row("SELECT COUNT(*) FROM projects WHERE course_id = ?1", [&cid], |r| r.get(0))?;
            assert!(first >= 24, "{first} of 40 in the first course");
            // The Social motivator sees a crowd this week.
            let crowd = crate::motivators::social::Crowd::load(c, &cid, today)?;
            assert!(crowd.others_active(0, today - crate::motivators::social::WEEK_SECS) >= crate::motivators::social::MIN_CROWD);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn seeding_again_only_tops_up_and_is_the_same_crowd() {
        let (db, courses) = crowd(6, 10);
        let rows = |db: &Db| db.with(|c| c.query_row("SELECT COUNT(*) FROM activity_log", [], |r| r.get::<_, i64>(0))).unwrap();
        let before = rows(&db);
        assert_eq!(seed(&db, &courses, 6, 10).unwrap(), 0);
        assert_eq!(seed(&db, &courses, 4, 10).unwrap(), 0);
        assert_eq!(rows(&db), before);
        assert_eq!(seed(&db, &courses, 8, 10).unwrap(), 2);
        assert_eq!(db.with(|c| count(c)).unwrap(), 8);
        // The same learner gets the same life in another database.
        let (other, _) = crowd(8, 10);
        let life = |db: &Db| db.with(|c| c.query_row("SELECT COUNT(*), SUM(at) FROM activity_log a JOIN users u ON u.id = a.user_id WHERE u.email = 'learner-7@seed.example'", [], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))).unwrap();
        assert_eq!(life(&db), life(&other));
    }

    #[test]
    fn the_crowd_signs_in_with_the_dev_password_and_is_not_a_deploys_accounts() {
        let (db, _) = crowd(2, 3);
        db.with(|c| {
            let (id, hash) = crate::auth::find_user_credentials(c, "learner-1@seed.example")?.expect("a crowd account");
            assert!(crate::auth::verify_password(crate::dev::DEV_PASSWORD, &hash));
            assert!(id > 0);
            assert_eq!(crate::dev::other_accounts(c)?, 0, "the crowd does not make the database look like a deploy's");
            assert!(crate::terms::latest(c, id)?.is_some(), "the terms are accepted, so no prompt on sign-in");
            Ok(())
        })
        .unwrap();
    }
}
