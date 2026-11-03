//! XP (#561): everything done in a course earns XP, and XP raises the
//! learner's character level in that course. Nothing is priced in advance:
//! XP is `earnings::XP_RATES` applied to `earnings::total` (the running
//! `earnings` row plus the lesson passes, never the whole log; #589), so it
//! keeps building while another motivator is worn. (Not to be confused with
//! a course's CEFR starting levels.)

use askama::Template;
use rusqlite::Connection;

use super::{Hero, Motivator, Progress, Reward, RunOutcome};
use crate::activity::{Kind, DAILY_GOAL};
use crate::content::ContentStore;
use crate::earnings::{self, Tally, XP_RATES};
use crate::routes::CourseView;

/// Going from level n to n + 1 costs `LEVEL_STEP * n` XP. Tunable:
/// changing it re-levels everyone, since only XP is ever worked out.
pub const LEVEL_STEP: i64 = 100;

/// A character level and how far into it the learner is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Level {
    /// 1 at 0 XP.
    pub level: i64,
    /// XP earned since reaching `level`.
    pub into: i64,
    /// XP the step from `level` to the next costs.
    pub need: i64,
}

impl Level {
    /// How full the bar to the next level is, 0–100.
    pub fn pct(&self) -> i64 {
        if self.need <= 0 {
            100
        } else {
            (self.into * 100 / self.need).clamp(0, 100)
        }
    }
}

/// Total XP it takes to reach `level` from nothing.
pub fn xp_to_reach(level: i64) -> i64 {
    let n = level.max(1);
    LEVEL_STEP * n * (n - 1) / 2
}

/// The level `xp` total XP makes.
pub fn level_of(xp: i64) -> Level {
    let xp = xp.max(0);
    let mut level = 1;
    while xp_to_reach(level + 1) <= xp {
        level += 1;
    }
    Level { level, into: xp - xp_to_reach(level), need: LEVEL_STEP * level }
}

/// The tally less a part of it (a run's tally taken off the total).
fn less(total: &Tally, part: &Tally) -> Tally {
    Tally {
        // Round away float dust so flooring in `Rates::earned` stays exact.
        points: ((total.points - part.points) * 1e6).round() / 1e6,
        passes: total.passes - part.passes,
        goals: total.goals - part.goals,
        units: total.units - part.units,
    }
}

pub struct Xp;

impl Motivator for Xp {
    fn id(&self) -> &'static str {
        "xp"
    }
    fn icon(&self) -> &'static str {
        "⭐"
    }
    fn load(&self, conn: &Connection, user_id: i64, course: &ContentStore, tz: i64) -> rusqlite::Result<Box<dyn Hero>> {
        let xp = XP_RATES.earned(&earnings::total(conn, user_id, course, tz)?);
        Ok(Box::new(XpHero { xp }))
    }
    fn reward(&self, conn: &Connection, user_id: i64, course: &ContentStore, run: &RunOutcome) -> rusqlite::Result<Option<Box<dyn Reward>>> {
        let gained = XP_RATES.earned(&run.earned);
        if gained <= 0 {
            return Ok(None);
        }
        let total = earnings::total(conn, user_id, course, run.tz)?;
        let after = level_of(XP_RATES.earned(&total)).level;
        let before = level_of(XP_RATES.earned(&less(&total, &run.earned))).level;
        Ok(Some(Box::new(XpReward { gained, level_up: (after > before).then_some(after) })))
    }
}

/// "+45 XP", and "Level up! Lv 8" when the run crossed one or more levels.
struct XpReward {
    gained: i64,
    level_up: Option<i64>,
}

impl Reward for XpReward {
    fn render(&self) -> askama::Result<String> {
        let mut out = format!(
            r#"<span data-testid="xp-reward-gained" data-xp="{n}">{}</span>"#,
            crate::error::html_escape(&crate::t1("motivator.xp.reward.earned", "n", self.gained)),
            n = self.gained
        );
        if let Some(level) = self.level_up {
            out.push_str(&format!(
                r#" · <span data-testid="xp-reward-level-up" data-level="{level}">{}</span>"#,
                crate::error::html_escape(&crate::t1("motivator.xp.reward.level_up", "n", level))
            ));
        }
        Ok(out)
    }
}

/// The learner's total XP in the course; the rest comes from [`Progress`].
struct XpHero {
    xp: i64,
}

impl Hero for XpHero {
    fn render(&self, course: &CourseView, p: &Progress) -> askama::Result<String> {
        let rate = |pts: f64| ((pts * XP_RATES.per_point as f64) + 1e-9).floor() as i64;
        let earning = Kind::ALL.into_iter().map(|k| (crate::t(&format!("goal.kind.{}", k.key())), rate(k.points()))).collect();
        let bonuses = vec![
            (crate::t("motivator.xp.bonus.pass"), XP_RATES.per_pass),
            (crate::t("motivator.xp.bonus.goal"), XP_RATES.per_goal),
            (crate::t("motivator.xp.bonus.unit"), XP_RATES.per_unit),
        ];
        XpTemplate {
            course,
            p,
            xp: self.xp,
            level: level_of(self.xp),
            today_xp: p.today.points * XP_RATES.per_point,
            goal_xp: DAILY_GOAL * XP_RATES.per_point,
            earning,
            bonuses,
            step: LEVEL_STEP,
        }
        .render()
    }
}

#[derive(Template)]
#[template(path = "motivators/xp.html")]
struct XpTemplate<'a> {
    course: &'a CourseView,
    p: &'a Progress,
    xp: i64,
    level: Level,
    /// Today's effort and the daily goal, counted in XP (effort points at
    /// the XP rate; bonuses go to the level, not the goal).
    today_xp: i64,
    goal_xp: i64,
    /// The (i): XP per kind of effort, and the bonuses.
    earning: Vec<(String, i64)>,
    bonuses: Vec<(String, i64)>,
    step: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    #[test]
    fn levels_follow_the_curve() {
        assert_eq!(level_of(0), Level { level: 1, into: 0, need: 100 });
        assert_eq!(level_of(99), Level { level: 1, into: 99, need: 100 });
        assert_eq!(level_of(100), Level { level: 2, into: 0, need: 200 });
        assert_eq!(level_of(299), Level { level: 2, into: 199, need: 200 });
        assert_eq!(level_of(300), Level { level: 3, into: 0, need: 300 });
        // Level 7 starts at 100·(1+…+6) = 2100 XP; level 8 at 2800.
        assert_eq!(level_of(2440), Level { level: 7, into: 340, need: 700 });
        assert_eq!(level_of(2800).level, 8);
        assert_eq!(level_of(-5).level, 1);
        assert_eq!(xp_to_reach(1), 0);
        assert_eq!(level_of(2440).pct(), 48);
    }

    #[test]
    fn xp_and_level_come_from_the_shared_tables() {
        // Units A1 (l0, l1) and A2 (l2).
        let lessons = [
            r#"{"id":"l0","slug":"l0","title":"L0","unit":"A1","content":"x"}"#,
            r#"{"id":"l1","slug":"l1","title":"L1","unit":"A1","content":"x"}"#,
            r#"{"id":"l2","slug":"l2","title":"L2","unit":"A2","content":"x"}"#,
        ];
        let course = crate::content::tests::store(&lessons).unwrap();
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let uid = crate::auth::create_user(c, "ann@example.org", "h").unwrap();
            let day = 10 * 86_400;
            c.execute(
                "INSERT INTO activity_log (user_id, course_id, kind, n, at) VALUES (?1, ?2, 'answer', 15, ?3), (?1, ?2, 'reading', 5, ?4)",
                params![uid, course.id(), day + 100, day + 5000],
            )?;
            for (id, at) in [("l2", 5), ("l0", day + 200), ("l1", day + 5000)] {
                c.execute(
                    "INSERT INTO lesson_passes (user_id, course_id, lesson_id, seen_at, passed_at, best_pct, updated_at) VALUES (?1, ?2, ?3, ?4, ?4, 100, ?4)",
                    params![uid, course.id(), id, at],
                )?;
            }
            // 35 points (350) + 3 passes (150) + 1 goal (100) + 2 units (500) = 1100 XP: level 5 (1000) + 100.
            let xp = XP_RATES.earned(&earnings::total(c, uid, &course, 0)?);
            assert_eq!(xp, 1100);
            assert_eq!(level_of(xp), Level { level: 5, into: 100, need: 500 });

            // A run worth 600 of it (the afternoon) crossed from level 3 to 5.
            let run = RunOutcome {
                points: 20,
                passed: true,
                goal_met: true,
                unit_finished: true,
                earned: earnings::between(c, uid, &course, 0, day + 4000, day + 6000)?,
                today: crate::activity::Today { points: 35, goal: 20 },
                streak: 1,
                tz: 0,
            };
            let line = Xp.reward(c, uid, &course, &run)?.unwrap().render().unwrap();
            assert!(line.contains(r#"data-xp="600">+600 XP<"#), "{line}");
            assert!(line.contains(r#"data-level="5">Level up! Lv 5<"#), "{line}");

            // A run that stays inside a level just says what it earned.
            let small = RunOutcome { earned: Tally { points: 2.0, ..Tally::default() }, ..run };
            let line = Xp.reward(c, uid, &course, &small)?.unwrap().render().unwrap();
            assert!(line.contains("+20 XP") && !line.contains("Level up"), "{line}");
            Ok(())
        })
        .unwrap();
    }
}
