//! Motivators (#559): how a learner's progress is dressed up. A theme skins
//! the app; a motivator decides what keeps the learner coming back: plain
//! statistics, XP and levels, a currency to spend, a leaderboard. The
//! learner picks one per course in its settings (`projects.motivator`,
//! '' for [`DEFAULT`]; #574). It owns two places: the top of the course home page (its
//! [`Hero`]) and one short line at the end of a lesson or session in every
//! mode (its [`Reward`], from what the run earned: a [`RunOutcome`]).
//!
//! What a motivator decorates never changes: lesson passes
//! (`progress.rs`), the daily goal's effort points and the resume point
//! (`activity.rs`), mastery, vocabulary. Every mode keeps reporting there
//! whichever motivator is worn, so a motivator that counts something of its
//! own (XP, coins) works it out from those shared tables (the running
//! `earnings` row and the lesson passes, never a learner's whole history;
//! #589) and keeps counting while another is worn; anything it stores
//! itself (coins spent, items bought) lives in its own tables and is never
//! cleared by a switch.
//! Switching motivators therefore loses nothing, and switching back finds
//! everything earned in the meantime.
//!
//! Adding one: a type that implements [`Motivator`] (and [`Hero`] and
//! [`Reward`] for what it shows), an entry in [`ALL`], a `motivator.<id>.*` title and blurb in
//! every `i18n/<code>.yaml`, and its hero template under
//! `templates/motivators/`. `docs/features.md` ("Motivators") has the rule.

use askama::Template;
use rusqlite::{params, Connection, OptionalExtension};

use crate::activity::Today;
use crate::content::ContentStore;
use crate::earnings::Tally;
use crate::routes::CourseView;

pub mod shop;
pub mod skc;
mod xp;

/// The motivator worn by anyone who has not picked one ([`Stats`]).
pub const DEFAULT: &str = "stats";

/// Every motivator the app has, in the order a course's settings offer them.
pub static ALL: &[&dyn Motivator] = &[&Stats, &xp::Xp, &skc::Skc];

/// A way of presenting progress. It reads what it keeps of its own in
/// [`load`](Motivator::load); the shared measures come in [`Progress`].
pub trait Motivator: Sync {
    /// Stored in `projects.motivator`; never renamed once shipped.
    fn id(&self) -> &'static str;
    /// The emoji on its chip in a course's settings.
    fn icon(&self) -> &'static str;
    /// Its name, in the app's language.
    fn title(&self) -> String {
        crate::t(&format!("motivator.{}.title", self.id()))
    }
    /// One sentence on what it does, for the course settings' (i).
    fn about(&self) -> String {
        crate::t(&format!("motivator.{}.about", self.id()))
    }
    /// Reads whatever this motivator keeps for the learner in the course,
    /// or works out from the shared tables (inside `Db::call`; `tz` is the
    /// learner's offset from UTC in seconds, for `earnings`). The result
    /// draws the home page's hero on the request's own task, where the
    /// learner's language is set.
    fn load(&self, conn: &Connection, user_id: i64, course: &ContentStore, tz: i64) -> rusqlite::Result<Box<dyn Hero>>;
    /// What it says about a run that earned something (inside `Db::call`;
    /// [`reward`] never asks about a run that earned nothing). The line is
    /// drawn on the request's own task, like the hero. `None` says nothing.
    fn reward(&self, conn: &Connection, user_id: i64, course: &ContentStore, run: &RunOutcome) -> rusqlite::Result<Option<Box<dyn Reward>>>;
}

/// The motivator's line at the end of a lesson or session.
pub trait Reward: Send {
    /// One short line of HTML (escape anything not your own markup), in the
    /// app's language. [`RewardLine::html`] wraps it in the shared element.
    fn render(&self) -> askama::Result<String>;
}

/// What one run (a lesson session, a practice or listening round, a game, a
/// Guide stage, a tutor's lesson) earned, from the shared tables: the input
/// to [`Motivator::reward`].
#[derive(Debug, Clone)]
pub struct RunOutcome {
    /// Effort points the run logged (`activity_log`), rounded down.
    pub points: i64,
    /// The run passed a lesson.
    pub passed: bool,
    /// The run took today's effort to the daily goal.
    pub goal_met: bool,
    /// The run passed the last lesson of a unit.
    pub unit_finished: bool,
    /// All of it as an earnings tally: price it with `earnings::Rates`.
    pub earned: Tally,
    /// Today's effort after the run, and the streak.
    pub today: Today,
    pub streak: i64,
    /// The learner's offset from UTC in seconds, for `earnings` windows.
    pub tz: i64,
}

impl RunOutcome {
    /// What was earned in `[since, now]` (inside `Db::call`). `tz` is the
    /// learner's offset from UTC in seconds (`routes::sprint::tz_offset_secs`).
    pub fn measure(conn: &Connection, user_id: i64, course: &ContentStore, tz: i64, since: i64, _lesson_id: Option<&str>) -> rusqlite::Result<RunOutcome> {
        let at = crate::db::now();
        let earned = crate::earnings::between(conn, user_id, course, tz, since, at + 1)?;
        let today = Today::load(conn, user_id, course.id(), crate::sprint::local_day_start(tz, at))?;
        let (streak, _) = crate::sprint::day_streak(conn, user_id, course.id(), tz, at)?;
        Ok(RunOutcome {
            points: earned.points.floor() as i64,
            passed: earned.passes > 0,
            goal_met: earned.goals > 0,
            unit_finished: earned.units > 0,
            earned,
            today,
            streak,
            tz,
        })
    }

    /// Nothing logged, passed or reached: no line.
    pub fn earned_nothing(&self) -> bool {
        self.points <= 0 && !self.passed && !self.goal_met && !self.unit_finished
    }
}

/// The worn motivator's line for a run, ready to draw.
pub struct RewardLine {
    motivator: &'static str,
    line: Box<dyn Reward>,
}

impl RewardLine {
    /// The line in its shared element (`data-testid="motivator-reward"`,
    /// `data-motivator`), on the request's task. None if it fails to draw.
    pub fn html(&self) -> Option<String> {
        let inner = self.line.render().ok()?;
        Some(format!(
            r#"<p class="motivator-reward" data-testid="motivator-reward" data-motivator="{}">{inner}</p>"#,
            self.motivator
        ))
    }
}

/// The line the worn motivator gives a run that began at `since` (inside
/// `Db::call`): None if the run earned nothing. Every mode's end screen
/// calls this and shows [`RewardLine::html`] (server pages) or sends it as
/// `reward` in its JSON (script end screens).
pub fn reward(conn: &Connection, user_id: i64, course: &ContentStore, tz: i64, since: i64, lesson_id: Option<&str>) -> rusqlite::Result<Option<RewardLine>> {
    let run = RunOutcome::measure(conn, user_id, course, tz, since, lesson_id)?;
    if run.earned_nothing() {
        return Ok(None);
    }
    let m = worn(conn, user_id, course.id())?;
    Ok(m.reward(conn, user_id, course, &run)?.map(|line| RewardLine { motivator: m.id(), line }))
}

/// [`RewardLine::html`] of an optional line.
pub fn reward_html(line: Option<RewardLine>) -> Option<String> {
    line.and_then(|l| l.html())
}

/// What a motivator shows at the top of the course home page.
pub trait Hero: Send {
    fn render(&self, course: &CourseView, progress: &Progress) -> askama::Result<String>;
}

/// The shared measures every motivator decorates, worked out once by the
/// home page whichever motivator is worn.
#[derive(Debug, Clone)]
pub struct Progress {
    /// Today's effort points against the daily goal (`activity.rs`).
    pub today: Today,
    pub practised_today: bool,
    pub streak: i64,
    /// The lesson every mode resumes at: (slug, title, the URL that opens it
    /// in the mode last used). None once every lesson is passed.
    pub resume: Option<(String, String, String)>,
    /// The daily goal's (i): what earns how many points.
    pub effort_table: Vec<(String, String)>,
    /// Lessons passed in any mode, and lessons of the course less the skipped ones.
    pub passed_count: usize,
    pub total_lessons: usize,
    /// Lessons below the starting level, never studied.
    pub skipped_count: usize,
    pub words_learned: i64,
    /// The unit of the resume point: its title, position (1-based) of how
    /// many, its lessons passed of those not skipped, and that as 0–100.
    pub current_unit: String,
    pub unit_number: usize,
    pub unit_count: usize,
    pub unit_passed: usize,
    pub unit_total: usize,
    pub unit_percent: usize,
}

/// The motivator with this id; an unknown or empty id is the default one.
pub fn get(id: &str) -> &'static dyn Motivator {
    find(id).unwrap_or(&Stats)
}

/// The motivator with this id, if the app has it.
pub fn find(id: &str) -> Option<&'static dyn Motivator> {
    ALL.iter().copied().find(|m| m.id() == id)
}

/// The motivator the user wears in a course (`projects.motivator`, #574);
/// the account's for a course they never opened.
pub fn worn(conn: &Connection, user_id: i64, course_id: &str) -> rusqlite::Result<&'static dyn Motivator> {
    let id: Option<String> = conn
        .query_row(
            "SELECT COALESCE((SELECT motivator FROM projects WHERE user_id = ?1 AND course_id = ?2), motivator) FROM users WHERE id = ?1",
            params![user_id, course_id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(get(id.as_deref().unwrap_or("")))
}

/// Open the course for the user and wear a motivator in it (tests; the app
/// saves it with the course's other settings, `courses::set_project_settings`).
#[cfg(test)]
pub fn set(conn: &Connection, user_id: i64, course_id: &str, id: &str) -> rusqlite::Result<()> {
    crate::courses::open_project(conn, user_id, course_id)?;
    conn.execute("UPDATE projects SET motivator = ?3 WHERE user_id = ?1 AND course_id = ?2", params![user_id, course_id, id])?;
    Ok(())
}

/// Statistics and metrics: the daily goal's ring, the streak, lessons
/// passed, terms collected, where to continue and the current unit's bar.
/// It keeps nothing of its own.
pub struct Stats;

impl Motivator for Stats {
    fn id(&self) -> &'static str {
        DEFAULT
    }
    fn icon(&self) -> &'static str {
        "📊"
    }
    fn load(&self, _: &Connection, _: i64, _: &ContentStore, _: i64) -> rusqlite::Result<Box<dyn Hero>> {
        Ok(Box::new(Stats))
    }
    fn reward(&self, _: &Connection, _: i64, course: &ContentStore, run: &RunOutcome) -> rusqlite::Result<Option<Box<dyn Reward>>> {
        let icons = crate::content::Icons::of(course.theme());
        Ok(Some(Box::new(StatsReward { run: run.clone(), streak_icon: icons.streak, goal_icon: icons.goal })))
    }
}

/// Stats' line: "✓ Passed · 14 / 20 today · 🔥 6".
struct StatsReward {
    run: RunOutcome,
    streak_icon: String,
    goal_icon: String,
}

impl Reward for StatsReward {
    fn render(&self) -> askama::Result<String> {
        let r = &self.run;
        let mut parts = Vec::new();
        if r.unit_finished {
            parts.push(crate::t("motivator.stats.reward.unit"));
        } else if r.passed {
            parts.push(crate::t("motivator.stats.reward.passed"));
        }
        if r.goal_met {
            parts.push(format!("{} {}", self.goal_icon, crate::t("motivator.stats.reward.goal_met")));
        } else {
            parts.push(crate::t2("motivator.stats.reward.today", "n", r.today.points, "goal", r.today.goal));
        }
        if r.streak > 0 {
            parts.push(format!("{} {}", self.streak_icon, r.streak));
        }
        Ok(crate::error::html_escape(&parts.join(" · ")))
    }
}

impl Hero for Stats {
    fn render(&self, course: &CourseView, p: &Progress) -> askama::Result<String> {
        StatsTemplate { course, p, today: p.today, practised_today: p.practised_today, streak: p.streak }.render()
    }
}

/// `today`, `practised_today` and `streak` are what `goal_message.html` reads.
#[derive(Template)]
#[template(path = "motivators/stats.html")]
struct StatsTemplate<'a> {
    course: &'a CourseView,
    p: &'a Progress,
    today: Today,
    practised_today: bool,
    streak: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_the_default_exists() {
        let mut ids: Vec<_> = ALL.iter().map(|m| m.id()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), ALL.len());
        assert_eq!(get("").id(), DEFAULT);
        assert_eq!(get("no-such-motivator").id(), DEFAULT);
        assert_eq!(get("social").id(), DEFAULT, "the removed Social motivator is the default");
        assert!(find("social").is_none());
    }

    #[test]
    fn every_motivator_is_named_in_every_language() {
        for m in ALL {
            assert_ne!(m.title(), format!("motivator.{}.title", m.id()));
            assert_ne!(m.about(), format!("motivator.{}.about", m.id()));
        }
    }

    #[test]
    fn a_run_gets_the_worn_motivators_line_only_when_it_earned_something() {
        let lesson = r#"{"id":"l0","slug":"l0","title":"L0","unit":"A1","content":"x","quiz":[{"question_id":"q0","prompt":"p","type":"fill-in-blank","correct_answer":"a"}]}"#;
        let course = crate::content::tests::store(&[lesson]).unwrap();
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let uid = crate::auth::create_user(c, "ann@example.org", "h").unwrap();
            let since = crate::db::now();
            assert!(reward(c, uid, &course, 0, since, Some("l0"))?.is_none(), "nothing logged, nothing passed");
            crate::activity::record(c, uid, course.id(), crate::activity::Kind::Answer, 5, 0)?;
            let run = RunOutcome::measure(c, uid, &course, 0, since, Some("l0"))?;
            assert_eq!((run.points, run.passed, run.goal_met, run.unit_finished), (5, false, false, false));
            assert_eq!((run.today.points, run.streak), (5, 1));
            let html = reward_html(reward(c, uid, &course, 0, since, Some("l0"))?).unwrap();
            assert!(html.starts_with(r#"<p class="motivator-reward" data-testid="motivator-reward" data-motivator="stats">"#), "{html}");
            assert!(html.contains("5 / 20 today"), "{html}");
            // Passing the lesson (and with it its unit) shows on the line.
            let l = course.lesson_by_id("l0").unwrap();
            crate::progress::record_taught_run(c, uid, course.id(), l, crate::progress::VIA_SPRINT, 1, 1)?;
            let run = RunOutcome::measure(c, uid, &course, 0, since, Some("l0"))?;
            assert!(run.passed && run.unit_finished && !run.goal_met);
            let html = reward_html(reward(c, uid, &course, 0, since, Some("l0"))?).unwrap();
            assert!(html.contains("Unit finished"), "{html}");
            // Reaching the daily goal.
            crate::activity::record(c, uid, course.id(), crate::activity::Kind::Answer, 15, 0)?;
            let html = reward_html(reward(c, uid, &course, 0, since, None)?).unwrap();
            assert!(html.contains("Daily goal met"), "{html}");
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn switching_keeps_the_learners_progress() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let uid = crate::auth::create_user(c, "ann@example.org", "h").unwrap();
            assert_eq!(worn(c, uid, "es")?.id(), DEFAULT);
            crate::activity::record(c, uid, "es", crate::activity::Kind::Answer, 3, 0)?;
            set(c, uid, "es", "no-longer-shipped")?;
            assert_eq!(worn(c, uid, "es")?.id(), DEFAULT, "an id the app no longer has is the default");
            set(c, uid, "es", DEFAULT)?;
            assert_eq!(Today::load(c, uid, "es", 0)?.points, 3);
            // Each course wears its own (#574).
            set(c, uid, "fr", "skc")?;
            assert_eq!((worn(c, uid, "es")?.id(), worn(c, uid, "fr")?.id()), (DEFAULT, "skc"));
            Ok(())
        })
        .unwrap();
    }
}
