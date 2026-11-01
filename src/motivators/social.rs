//! Social (#563): anonymous comparisons with the course's other learners.
//! "You put in more effort than 70 % of learners in this course this week",
//! "23 people are studying this course today", "6 learners worked on Greetings
//! in the last hour" (#501), "You're one of 41 who passed this lesson this week",
//! "14 learners kept their streak going today" (#505). Nobody is named or shown,
//! the learner included, and everyone's activity counts whatever motivator they wear.
//!
//! Everything comes from the shared tables (`activity_log` for effort,
//! `lesson_passes` for passes, `lesson_touches` for who is on a lesson, the
//! `earnings` ledger for streaks); Social stores nothing. A comparison is only
//! made when at least [`MIN_CROWD`] learners (the learner among them) were
//! active in its window, so a small server never says "more than 100 %" of
//! one other person; below that the hero falls back to Continue and the
//! daily goal, and the reward line says nothing.
//!
//! Cost on a Pi: the others' numbers are cross-user aggregates, read as
//! index ranges over the one course and its last eight days
//! (`idx_activity_log_course_at`, `idx_lesson_passes_course_passed`, #602,
//! #603; `idx_lesson_touches_course_at` over the last hour or so, #501;
//! `idx_earnings_course_day` for the streaks, #505), so other courses and
//! older history are never read. They are kept
//! in a per-course [`Crowd`] in memory for [`cache_secs`] (five minutes);
//! the learner's own numbers are always read fresh through the per-user
//! index and put in place of their cached ones.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};

use askama::Template;
use rusqlite::{params, Connection, OptionalExtension};

use super::{Hero, Motivator, Progress, Reward, RunOutcome};
use crate::activity::Kind;
use crate::content::ContentStore;
use crate::routes::CourseView;

/// A comparison is hidden when fewer learners than this (the learner
/// included) were active in its window.
pub const MIN_CROWD: usize = 5;

/// "This week": the last seven days.
pub const WEEK_SECS: i64 = 7 * 86_400;

/// "In the last hour": who worked on a lesson lately (#501).
pub const HOUR_SECS: i64 = 3_600;

/// Effort is kept per quarter of an hour, so any learner's local midnight
/// (every time zone is a whole number of quarters from UTC) starts a bucket.
const BUCKET_SECS: i64 = 900;

/// How long a course's [`Crowd`] is reused: `SKRONGLE_SOCIAL_CACHE_SECS`, five
/// minutes by default; 0 reads it afresh every time (the e2e server does).
pub fn cache_secs() -> i64 {
    if cfg!(test) {
        return 0;
    }
    crate::env("SOCIAL_CACHE_SECS").and_then(|v| v.parse().ok()).unwrap_or(300)
}

/// The crowd's effort: ?1 since, ?2 course, ?3 bucket length.
const EFFORT_SQL: &str = "SELECT user_id, kind, at / ?3 * ?3, SUM(n) FROM activity_log
     WHERE course_id = ?2 AND at >= ?1 GROUP BY user_id, kind, at / ?3";

/// The crowd's passes: ?1 course, ?2 since.
const PASSES_SQL: &str = "SELECT lesson_id, user_id, passed_at FROM lesson_passes WHERE course_id = ?1 AND passed_at >= ?2";

/// The crowd's streaks of two days or more (#505): ?1 course, ?2 the first
/// local day read. `earnings.day` is each learner's own local day.
const STREAKS_SQL: &str = "SELECT user_id, day FROM earnings WHERE course_id = ?1 AND day >= ?2 AND streak >= 2";

/// A streak counts as kept today once it runs two days or more and today
/// is one of them: a first day starts a streak, it doesn't keep one.
pub const MIN_STREAK: i64 = 2;

/// Who opened or worked on which lesson lately: ?1 course, ?2 since.
const TOUCHES_SQL: &str = "SELECT lesson_id, user_id, at FROM lesson_touches WHERE course_id = ?1 AND at >= ?2";

/// What every learner of a course did lately: the others' side of every
/// comparison.
#[derive(Debug, Default)]
pub struct Crowd {
    /// User -> effort points per quarter-hour bucket (its start).
    effort: HashMap<i64, Vec<(i64, f64)>>,
    /// Lesson id -> (user, passed_at) of the passes.
    passes: HashMap<String, Vec<(i64, i64)>>,
    /// (user, local day last active) of the streaks of [`MIN_STREAK`] days
    /// or more that ended in the last few days.
    streaks: Vec<(i64, i64)>,
    /// Lesson id -> (user, at) of the last hour's touches (one per learner).
    touches: HashMap<String, Vec<(i64, i64)>>,
}

impl Crowd {
    /// The last eight days of a course (a day more than a week, so a cached
    /// copy still covers the window and any learner's "today").
    pub fn load(conn: &Connection, course_id: &str, now: i64) -> rusqlite::Result<Crowd> {
        let since = now - WEEK_SECS - 86_400;
        let mut crowd = Crowd::default();
        // Range on idx_activity_log_course_at: this course, the last eight days.
        let mut stmt = conn.prepare_cached(EFFORT_SQL)?;
        let rows = stmt.query_map(params![since, course_id, BUCKET_SECS], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?)))?;
        for row in rows {
            let (user, kind, bucket, n) = row?;
            let weight = Kind::ALL.into_iter().find(|k| k.key() == kind).map_or(0.0, Kind::points);
            let buckets = crowd.effort.entry(user).or_default();
            match buckets.iter_mut().find(|(b, _)| *b == bucket) {
                Some((_, pts)) => *pts += weight * n as f64,
                None => buckets.push((bucket, weight * n as f64)),
            }
        }
        // Range on idx_lesson_passes_course_passed.
        let mut stmt = conn.prepare_cached(PASSES_SQL)?;
        let rows = stmt.query_map(params![course_id, since], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)))?;
        for row in rows {
            let (lesson, user, at) = row?;
            crowd.passes.entry(lesson).or_default().push((user, at));
        }
        // Range on idx_earnings_course_day. Local days run up to 14 hours
        // either side of UTC, and a cached copy is reused for minutes, so
        // two days back covers every learner's "today".
        let mut stmt = conn.prepare_cached(STREAKS_SQL)?;
        let rows = stmt.query_map(params![course_id, crate::earnings::day_of(0, now) - 2], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
        crowd.streaks = rows.collect::<rusqlite::Result<_>>()?;
        // Range on idx_lesson_touches_course_at: the last hour, and as much
        // again as a cached copy lives, so it still covers its last hour.
        let mut stmt = conn.prepare_cached(TOUCHES_SQL)?;
        let rows = stmt.query_map(params![course_id, now - HOUR_SECS - cache_secs().max(0)], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)))?;
        for row in rows {
            let (lesson, user, at) = row?;
            crowd.touches.entry(lesson).or_default().push((user, at));
        }
        Ok(crowd)
    }

    /// The other learners' effort points since `since`, rounded down, of
    /// those who did anything.
    pub fn others_effort(&self, me: i64, since: i64) -> Vec<i64> {
        self.effort
            .iter()
            .filter(|(u, _)| **u != me)
            .map(|(_, b)| b.iter().filter(|(at, _)| *at >= since).map(|(_, p)| p).sum::<f64>())
            .filter(|p| *p > 0.0)
            .map(|p| p.floor() as i64)
            .collect()
    }

    /// How many other learners did something (effort or a pass) since `since`.
    pub fn others_active(&self, me: i64, since: i64) -> usize {
        let mut users: HashSet<i64> = HashSet::new();
        for (u, b) in &self.effort {
            if *u != me && b.iter().any(|(at, p)| *at >= since && *p > 0.0) {
                users.insert(*u);
            }
        }
        for passes in self.passes.values() {
            users.extend(passes.iter().filter(|(u, at)| *u != me && *at >= since).map(|(u, _)| *u));
        }
        users.len()
    }

    /// How many other learners kept a streak going on local day `today`
    /// (#505): a streak of [`MIN_STREAK`] days or more whose last day is
    /// `today`. Each row's day is its learner's own; the viewer's day
    /// number stands in for theirs.
    pub fn others_kept_streak(&self, me: i64, today: i64) -> usize {
        self.streaks.iter().filter(|(u, d)| *u != me && *d == today).count()
    }

    /// How many other learners passed `lesson` since `since`.
    pub fn others_passed(&self, lesson: &str, me: i64, since: i64) -> usize {
        self.passes.get(lesson).map_or(0, |p| p.iter().filter(|(u, at)| *u != me && *at >= since).count())
    }
}

impl Crowd {
    /// How many other learners opened or worked on `lesson` since `since`.
    pub fn others_on_lesson(&self, lesson: &str, me: i64, since: i64) -> usize {
        self.touches.get(lesson).map_or(0, |t| t.iter().filter(|(u, at)| *u != me && *at >= since).count())
    }

    /// The lessons someone else worked on since `since`.
    fn touched_lessons(&self, since: i64) -> impl Iterator<Item = &str> {
        self.touches.iter().filter(move |(_, t)| t.iter().any(|(_, at)| *at >= since)).map(|(l, _)| l.as_str())
    }
}

/// Lesson slug -> how many learners (the learner among them) opened or
/// worked on it since `since`, for the lessons with at least [`MIN_CROWD`].
/// The learner's own touches are read fresh (primary key range).
pub fn lessons_now(conn: &Connection, crowd: &Crowd, course: &ContentStore, me: i64, since: i64) -> rusqlite::Result<HashMap<String, usize>> {
    let mut stmt = conn.prepare_cached("SELECT lesson_id FROM lesson_touches WHERE user_id = ?1 AND course_id = ?2 AND at >= ?3")?;
    let mine: HashSet<String> = stmt.query_map(params![me, course.id(), since], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    let ids: HashSet<&str> = crowd.touched_lessons(since).chain(mine.iter().map(String::as_str)).collect();
    Ok(ids
        .into_iter()
        .filter_map(|id| {
            let n = crowd_size(crowd.others_on_lesson(id, me, since), mine.contains(id))?;
            Some((course.lesson_by_id(id)?.slug.clone(), n))
        })
        .collect())
}

/// The course's [`Crowd`], from the cache while it is fresh (inside `Db::call`).
pub fn crowd(conn: &Connection, course_id: &str, now: i64) -> rusqlite::Result<Arc<Crowd>> {
    static CACHE: OnceLock<Mutex<HashMap<String, (i64, Arc<Crowd>)>>> = OnceLock::new();
    let ttl = cache_secs();
    let cache = CACHE.get_or_init(Mutex::default);
    if ttl > 0 {
        if let Some((_, c)) = cache.lock().ok().and_then(|m| m.get(course_id).filter(|(at, _)| now - at < ttl).cloned()) {
            return Ok(c);
        }
    }
    let fresh = Arc::new(Crowd::load(conn, course_id, now)?);
    if ttl > 0 {
        if let Ok(mut m) = cache.lock() {
            m.retain(|_, (at, _)| now - *at < ttl);
            m.insert(course_id.to_string(), (now, Arc::clone(&fresh)));
        }
    }
    Ok(fresh)
}

/// How many learners were active in a window: the others and the learner
/// if `me_active`. None below [`MIN_CROWD`].
pub fn crowd_size(others: usize, me_active: bool) -> Option<usize> {
    let n = others + usize::from(me_active);
    (n >= MIN_CROWD).then_some(n)
}

/// The share (0–100, rounded down) of the other active learners who did
/// less than `mine`. None when the learner did nothing or the crowd (they
/// and the learner) is below [`MIN_CROWD`].
pub fn more_than(mine: i64, others: &[i64]) -> Option<i64> {
    if mine <= 0 || others.is_empty() {
        return None;
    }
    crowd_size(others.len(), true)?;
    let below = others.iter().filter(|o| **o < mine).count();
    Some((below * 100 / others.len()) as i64)
}

/// The learner's own effort points since `since` (per-user index).
fn my_effort(conn: &Connection, user_id: i64, course_id: &str, since: i64) -> rusqlite::Result<i64> {
    crate::activity::points_since(conn, user_id, course_id, since)
}

pub struct Social;

impl Motivator for Social {
    fn id(&self) -> &'static str {
        "social"
    }
    fn icon(&self) -> &'static str {
        "👥"
    }
    fn load(&self, conn: &Connection, user_id: i64, course: &ContentStore, tz: i64) -> rusqlite::Result<Box<dyn Hero>> {
        let now = crate::db::now();
        let crowd = crowd(conn, course.id(), now)?;
        let my_week = my_effort(conn, user_id, course.id(), now - WEEK_SECS)?;
        let lessons_now = lessons_now(conn, &crowd, course, user_id, now - HOUR_SECS)?;
        Ok(Box::new(SocialHero { crowd, me: user_id, now, my_week, today: crate::earnings::day_of(tz, now), lessons_now }))
    }
    fn reward(&self, conn: &Connection, user_id: i64, course: &ContentStore, run: &RunOutcome) -> rusqlite::Result<Option<Box<dyn Reward>>> {
        let now = crate::db::now();
        let week = now - WEEK_SECS;
        let crowd = crowd(conn, course.id(), now)?;
        let my_week = my_effort(conn, user_id, course.id(), week)?;
        if let (true, Some(lesson)) = (run.passed, run.lesson_id.as_deref()) {
            let passed_at: Option<i64> = conn
                .query_row(
                    "SELECT passed_at FROM lesson_passes WHERE user_id = ?1 AND course_id = ?2 AND lesson_id = ?3",
                    params![user_id, course.id(), lesson],
                    |r| r.get(0),
                )
                .optional()?
                .flatten();
            if passed_at.is_some_and(|at| at >= week) && crowd_size(crowd.others_active(user_id, week), true).is_some() {
                let n = crowd.others_passed(lesson, user_id, week) + 1;
                return Ok(Some(Box::new(SocialReward::Passed(n))));
            }
        }
        Ok(more_than(my_week, &crowd.others_effort(user_id, week)).map(|pct| Box::new(SocialReward::Effort(pct)) as Box<dyn Reward>))
    }
}

/// Social's line: how many passed the lesson this week, else how the
/// week's effort compares.
#[derive(Debug, PartialEq, Eq)]
enum SocialReward {
    Passed(usize),
    Effort(i64),
}

impl Reward for SocialReward {
    fn render(&self) -> askama::Result<String> {
        let text = match self {
            SocialReward::Passed(1) => crate::t("motivator.social.reward.first"),
            SocialReward::Passed(n) => crate::t1("motivator.social.reward.passed", "n", n),
            SocialReward::Effort(pct) => crate::t1("motivator.social.reward.effort", "pct", pct),
        };
        Ok(crate::error::html_escape(&text))
    }
}

/// One comparison on the hero: what it says (`kind`, for tests) and its text.
pub struct Line {
    pub kind: &'static str,
    pub text: String,
}

struct SocialHero {
    crowd: Arc<Crowd>,
    me: i64,
    now: i64,
    my_week: i64,
    /// The learner's local day number ([`crate::earnings::day_of`]).
    today: i64,
    /// Lesson slug -> learners on it in the last hour, at least [`MIN_CROWD`].
    lessons_now: HashMap<String, usize>,
}

impl SocialHero {
    /// At most four lines: the week (the learner's effort against the
    /// others', or how many studied), how many are studying today, how many
    /// kept their streak going today (#505) and how many worked on the lesson
    /// to continue at in the last hour (#501).
    fn lines(&self, p: &Progress) -> Vec<Line> {
        let week = self.now - WEEK_SECS;
        let mut lines = Vec::new();
        if let Some(pct) = more_than(self.my_week, &self.crowd.others_effort(self.me, week)) {
            lines.push(Line { kind: "week-effort", text: crate::t1("motivator.social.week_effort", "pct", pct) });
        } else if let Some(n) = crowd_size(self.crowd.others_active(self.me, week), self.my_week > 0) {
            lines.push(Line { kind: "week-count", text: crate::t1("motivator.social.week_count", "n", n) });
        }
        if let Some(n) = crowd_size(self.crowd.others_active(self.me, p.day_start), p.today.points > 0) {
            lines.push(Line { kind: "today-count", text: crate::t1("motivator.social.today_count", "n", n) });
        }
        let mine = p.practised_today && p.streak >= MIN_STREAK;
        if let Some(n) = crowd_size(self.crowd.others_kept_streak(self.me, self.today), mine) {
            let key = if mine { "motivator.social.streak_count_you" } else { "motivator.social.streak_count" };
            lines.push(Line { kind: "streak-count", text: crate::t1(key, "n", n) });
        }
        if let Some((slug, title, _)) = &p.resume {
            if let Some(n) = self.lessons_now.get(slug) {
                lines.push(Line { kind: "lesson-now", text: crate::t2("motivator.social.lesson_now", "n", n, "title", title) });
            }
        }
        lines
    }
}

impl Hero for SocialHero {
    fn render(&self, course: &CourseView, p: &Progress) -> askama::Result<String> {
        let ahead_today = more_than(p.today.points, &self.crowd.others_effort(self.me, p.day_start));
        SocialTemplate { course, p, lines: self.lines(p), ahead_today }.render()
    }
}

#[derive(Template)]
#[template(path = "motivators/social.html")]
struct SocialTemplate<'a> {
    course: &'a CourseView,
    p: &'a Progress,
    lines: Vec<Line>,
    /// The share of today's other learners the learner is ahead of.
    ahead_today: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::Today;
    use crate::db::Db;

    fn course() -> ContentStore {
        let lesson = r#"{"id":"l0","slug":"l0","title":"L0","unit":"A1","content":"x","quiz":[{"question_id":"q0","prompt":"p","type":"fill-in-blank","correct_answer":"a"}]}"#;
        crate::content::tests::store(&[lesson]).unwrap()
    }

    /// `n` learners besides `me`, each with `pts` answers today, the first
    /// `passed` of them having passed l0.
    fn others(c: &Connection, course: &ContentStore, n: usize, pts: &[i64], passed: usize) -> rusqlite::Result<()> {
        for i in 0..n {
            let uid = crate::auth::create_user(c, &format!("o{i}@example.org"), "h").unwrap();
            crate::activity::record(c, uid, course.id(), Kind::Answer, pts.get(i).copied().unwrap_or(1), 0)?;
            if i < passed {
                let l = course.lesson_by_id("l0").unwrap();
                crate::progress::record_taught_run(c, uid, course.id(), l, crate::progress::VIA_SPRINT, 1, 1)?;
            }
        }
        Ok(())
    }

    fn progress(points: i64) -> Progress {
        Progress {
            today: Today { points, goal: 20 },
            practised_today: points > 0,
            streak: 1,
            resume: Some(("l0".into(), "L0".into(), "/lessons/l0".into())),
            effort_table: vec![],
            passed_count: 0,
            total_lessons: 1,
            skipped_count: 0,
            words_learned: 0,
            current_unit: "A1".into(),
            unit_number: 1,
            unit_count: 1,
            unit_passed: 0,
            unit_total: 1,
            unit_percent: 0,
            day_start: crate::db::now() - 3600,
        }
    }

    #[test]
    fn comparisons_need_a_crowd() {
        assert_eq!(crowd_size(3, true), None);
        assert_eq!(crowd_size(4, false), None);
        assert_eq!(crowd_size(4, true), Some(MIN_CROWD));
        assert_eq!(more_than(10, &[1, 2, 3]), None, "four learners are too few");
        assert_eq!(more_than(10, &[1, 2, 3, 20]), Some(75));
        assert_eq!(more_than(0, &[1, 2, 3, 4, 5]), None, "nothing done, no comparison");
        assert_eq!(more_than(3, &[1, 2, 3, 4, 5]), Some(40), "equal is not more");
        assert_eq!(more_than(100, &[1; 40]), Some(100));
    }

    #[test]
    fn few_learners_get_no_comparison() {
        let course = course();
        let db = Db::open_in_memory().unwrap();
        db.with(|c| {
            let me = crate::auth::create_user(c, "me@example.org", "h").unwrap();
            super::super::set(c, me, course.id(), "social")?;
            others(c, &course, 3, &[2, 2, 2], 3)?;
            crate::activity::record(c, me, course.id(), Kind::Answer, 9, 0)?;
            let since = crate::db::now();
            let l = course.lesson_by_id("l0").unwrap();
            crate::progress::record_taught_run(c, me, course.id(), l, crate::progress::VIA_SPRINT, 1, 1)?;
            // Four learners in all: no reward line, and the hero says nothing about others.
            assert!(super::super::reward(c, me, &course, 0, since - 10, Some("l0"))?.is_none());
            let hero = SocialHero { crowd: crowd(c, course.id(), crate::db::now())?, me, now: crate::db::now(), my_week: 9, today: crate::earnings::day_of(0, crate::db::now()), lessons_now: HashMap::new() };
            assert!(hero.lines(&progress(9)).is_empty());
            let html = hero.render(&CourseView::of(&course), &progress(9)).unwrap();
            assert!(html.contains(r#"data-motivator="social""#) && html.contains("home-continue"), "{html}");
            assert!(!html.contains(r#"data-testid="home-social-line""#) && !html.contains("home-social-ahead"), "{html}");
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn many_learners_are_compared_and_counted() {
        let course = course();
        let db = Db::open_in_memory().unwrap();
        db.with(|c| {
            let me = crate::auth::create_user(c, "me@example.org", "h").unwrap();
            super::super::set(c, me, course.id(), "social")?;
            // Nine others: 1..=9 answers; three of them passed l0.
            others(c, &course, 9, &[1, 2, 3, 4, 5, 6, 7, 8, 9], 3)?;
            crate::activity::record(c, me, course.id(), Kind::Answer, 7, 0)?;
            let since = crate::db::now();
            let hero = Social.load(c, me, &course, 0)?;
            let html = hero.render(&CourseView::of(&course), &progress(7)).unwrap();
            // Ahead of 1..=6: six of nine, 66 %; ten learners today.
            assert!(html.contains("more effort than 66 %"), "{html}");
            assert!(html.contains("10 people are studying this course today"), "{html}");
            assert!(html.contains("more than 66 % of today"), "{html}");
            // Passing l0 makes the learner one of four this week.
            let l = course.lesson_by_id("l0").unwrap();
            crate::progress::record_taught_run(c, me, course.id(), l, crate::progress::VIA_SPRINT, 1, 1)?;
            let line = super::super::reward_html(super::super::reward(c, me, &course, 0, since - 10, Some("l0"))?).unwrap();
            assert!(line.contains(r#"data-motivator="social""#) && line.contains("one of 4 who passed"), "{line}");
            // A run that passed nothing compares the week's effort.
            let line = super::super::reward_html(super::super::reward(c, me, &course, 0, crate::db::now() + 1, None)?);
            assert!(line.is_none(), "nothing earned, no line");
            let run = RunOutcome::measure(c, me, &course, 0, since - 10, None)?;
            let r = Social.reward(c, me, &course, &RunOutcome { passed: false, ..run })?.unwrap();
            assert!(r.render().unwrap().contains("More effort than 66 %"));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn the_lesson_count_needs_a_crowd_in_the_last_hour() {
        let course = course();
        let db = Db::open_in_memory().unwrap();
        db.with(|c| {
            let me = crate::auth::create_user(c, "me@example.org", "h").unwrap();
            let now = crate::db::now();
            let other = |c: &Connection, i: usize, at: i64| -> rusqlite::Result<()> {
                let uid = crate::auth::create_user(c, &format!("t{i}@example.org"), "h").unwrap();
                crate::activity::touch_at(c, uid, course.id(), "l0", crate::activity::Mode::Study, at)
            };
            // Three others in the last hour, one two hours ago, and the learner: four, hidden.
            for i in 0..3 {
                other(c, i, now - 60)?;
            }
            other(c, 3, now - 2 * HOUR_SECS)?;
            crate::activity::touch_at(c, me, course.id(), "l0", crate::activity::Mode::Study, now - 10)?;
            let counts = |c: &Connection| lessons_now(c, &Crowd::load(c, course.id(), now).unwrap(), &course, me, now - HOUR_SECS);
            assert!(counts(c)?.is_empty(), "four learners are too few");
            // A fourth other in the last hour: five, shown, the learner counted once.
            other(c, 4, now - 30)?;
            crate::activity::touch_at(c, me, course.id(), "l0", crate::activity::Mode::Sprint, now)?;
            assert_eq!(counts(c)?.get("l0"), Some(&MIN_CROWD));
            // On the hero, for the lesson to continue at.
            let hero = SocialHero { crowd: crowd(c, course.id(), now)?, me, now, my_week: 0, today: crate::earnings::day_of(0, now), lessons_now: counts(c)? };
            let lines = hero.lines(&progress(0));
            let line = lines.iter().find(|l| l.kind == "lesson-now").expect("a lesson line");
            assert!(line.text.contains("5 learners worked on L0 in the last hour"), "{}", line.text);
            // Without the learner, four: hidden.
            c.execute("DELETE FROM lesson_touches WHERE user_id = ?1", params![me])?;
            assert!(counts(c)?.is_empty());
            Ok(())
        })
        .unwrap();
    }

    /// The query plan's details for `sql` (#602, #603).
    fn plan(c: &Connection, sql: &str, args: impl rusqlite::Params) -> Vec<String> {
        let mut stmt = c.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
        stmt.query_map(args, |r| r.get::<_, String>(3)).unwrap().collect::<rusqlite::Result<_>>().unwrap()
    }

    #[test]
    fn the_crowd_reads_index_ranges() {
        let db = Db::open_in_memory().unwrap();
        db.with(|c| {
            let effort = plan(c, EFFORT_SQL, params![0, "c", BUCKET_SECS]);
            assert!(effort.iter().any(|d| d.contains("idx_activity_log_course_at")), "{effort:?}");
            assert!(!effort.iter().any(|d| d.contains("SCAN activity_log")), "{effort:?}");
            let streaks = plan(c, STREAKS_SQL, params!["c", 0]);
            assert!(streaks.iter().any(|d| d.contains("idx_earnings_course_day")), "{streaks:?}");
            assert!(!streaks.iter().any(|d| d.contains("SCAN earnings")), "{streaks:?}");
            let passes = plan(c, PASSES_SQL, params!["c", 0]);
            assert!(passes.iter().any(|d| d.contains("idx_lesson_passes_course_passed")), "{passes:?}");
            assert!(!passes.iter().any(|d| d.contains("SCAN lesson_passes")), "{passes:?}");
            let touches = plan(c, TOUCHES_SQL, params!["c", 0]);
            assert!(touches.iter().any(|d| d.contains("idx_lesson_touches_course_at")), "{touches:?}");
            assert!(!touches.iter().any(|d| d.contains("SCAN lesson_touches")), "{touches:?}");
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn the_crowd_skips_the_learner_and_old_days() {
        let course = course();
        let db = Db::open_in_memory().unwrap();
        db.with(|c| {
            let me = crate::auth::create_user(c, "me@example.org", "h").unwrap();
            others(c, &course, 2, &[3, 4], 1)?;
            crate::activity::record(c, me, course.id(), Kind::Answer, 5, 0)?;
            let old = crate::db::now() - 10 * 86_400;
            c.execute("INSERT INTO activity_log (user_id, course_id, kind, n, at) VALUES (?1, ?2, 'answer', 50, ?3)", params![me + 1, course.id(), old])?;
            let crowd = Crowd::load(c, course.id(), crate::db::now())?;
            let mut effort = crowd.others_effort(me, crate::db::now() - WEEK_SECS);
            effort.sort_unstable();
            assert_eq!(effort, vec![3, 4]);
            assert_eq!(crowd.others_active(me, 0), 2);
            assert_eq!(crowd.others_passed("l0", me, 0), 1);
            Ok(())
        })
        .unwrap();
    }

    /// Puts a learner's streak in the ledger: `streak` days ending on `day`.
    fn ledger(c: &Connection, uid: i64, course_id: &str, day: i64, streak: i64) -> rusqlite::Result<()> {
        c.execute(
            "INSERT INTO earnings (user_id, course_id, points, day, day_points, streak) VALUES (?1, ?2, 1, ?3, 1, ?4)
             ON CONFLICT (user_id, course_id) DO UPDATE SET day = excluded.day, streak = excluded.streak",
            params![uid, course_id, day, streak],
        )?;
        Ok(())
    }

    #[test]
    fn learners_who_kept_a_streak_today_are_counted_from_the_ledger() {
        let course = course();
        let db = Db::open_in_memory().unwrap();
        db.with(|c| {
            let now = crate::db::now();
            let today = crate::earnings::day_of(0, now);
            let me = crate::auth::create_user(c, "me@example.org", "h").unwrap();
            let user = |c: &Connection, i: usize| crate::auth::create_user(c, &format!("s{i}@example.org"), "h");
            // Three others kept a streak today; one is on day one, one stopped
            // yesterday, one kept it in another course: none of those count.
            for i in 0..3 {
                ledger(c, user(c, i).unwrap(), course.id(), today, 3)?;
            }
            ledger(c, user(c, 3).unwrap(), course.id(), today, 1)?;
            ledger(c, user(c, 4).unwrap(), course.id(), today - 1, 5)?;
            ledger(c, user(c, 5).unwrap(), "other", today, 4)?;
            let streak_lines = |me_streak: i64, me_today: bool| -> rusqlite::Result<Vec<String>> {
                let hero = SocialHero { crowd: crowd(c, course.id(), now)?, me, now, my_week: 0, today, lessons_now: HashMap::new() };
                let p = Progress { streak: me_streak, practised_today: me_today, ..progress(if me_today { 1 } else { 0 }) };
                Ok(hero.lines(&p).into_iter().filter(|l| l.kind == "streak-count").map(|l| l.text).collect())
            };
            assert_eq!(Crowd::load(c, course.id(), now)?.others_kept_streak(me, today), 3);
            // Four with the learner: below MIN_CROWD, so no line.
            ledger(c, me, course.id(), today, 2)?;
            assert!(streak_lines(2, true)?.is_empty(), "four learners are too few");
            // A fourth other learner: five with the learner, who is one of them.
            ledger(c, user(c, 6).unwrap(), course.id(), today, 2)?;
            assert_eq!(streak_lines(2, true)?, vec!["5 learners kept their streak going today. You're one of them.".to_string()]);
            // Without the learner the four others are too few again.
            assert!(streak_lines(1, true)?.is_empty(), "a first day doesn't keep a streak");
            assert!(streak_lines(4, false)?.is_empty(), "not practised today");
            // A fifth other: the line shows, without "one of them".
            ledger(c, user(c, 7).unwrap(), course.id(), today, 9)?;
            assert_eq!(streak_lines(0, false)?, vec!["5 learners kept their streak going today.".to_string()]);
            Ok(())
        })
        .unwrap();
    }
}
