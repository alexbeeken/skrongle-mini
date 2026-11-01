//! Skrongle Cash (#562): play money, like tickets at an arcade. Each course
//! has its own balance, priced from what was earned there
//! (`earnings::SKC_RATES`: 1 per effort point, +5 per lesson passed, +10
//! per daily goal met, +25 per unit finished). Nothing is priced in
//! advance: the balance is `earnings::total` (the running `earnings` row
//! plus the lesson passes, never the whole log; #589), so it keeps counting
//! while another motivator is worn. What the shop sold (#569,
//! [`super::shop`]) comes off it: the [`balance`] is earned less spent.
//!
//! [`symbol`] and [`amount`] draw SKC's own glyph; use them wherever an SKC
//! amount shows.

use askama::Template;
use rusqlite::Connection;

use super::{Hero, Motivator, Progress, Reward, RunOutcome};
use crate::content::ContentStore;
use crate::earnings::{Rates, Tally, SKC_RATES};
use crate::routes::CourseView;

/// The rates Skrongle Cash is priced at.
pub const RATES: Rates = SKC_RATES;

/// SKC's glyph: an arcade ticket (notched at both ends) with a spark on it.
/// An original shape, not a currency sign; drawn in `currentColor`, 1em
/// high, so it follows the text's colour in every theme and colour scheme.
pub fn symbol() -> String {
    format!(
        r#"<svg class="skc-symbol" data-testid="skc-symbol" viewBox="0 0 24 24" width="1em" height="1em" role="img" aria-label="{}" focusable="false"><path d="M4 6h16a1 1 0 0 1 1 1v2.5a2.5 2.5 0 0 0 0 5V17a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1v-2.5a2.5 2.5 0 0 0 0-5V7a1 1 0 0 1 1-1z" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"/><path d="M12 8.6l1.05 2.35 2.35 1.05-2.35 1.05L12 15.4l-1.05-2.35L8.6 12l2.35-1.05z" fill="currentColor"/></svg>"#,
        crate::error::html_escape(&crate::t("motivator.skc.short"))
    )
}

/// An SKC amount with the glyph: `<span class="skc-amount">◇ 120</span>`.
pub fn amount(n: i64) -> String {
    format!(r#"<span class="skc-amount" data-skc="{n}">{} {n}</span>"#, symbol())
}

/// The course's SKC balance (inside `Db::call`): everything earned there
/// at [`RATES`], less what the shop sold. Use it wherever a balance shows.
pub fn balance(conn: &Connection, user_id: i64, course: &ContentStore, tz: i64) -> rusqlite::Result<i64> {
    Ok(RATES.earned(&crate::earnings::total(conn, user_id, course, tz)?) - super::shop::spent(conn, user_id, course.id())?)
}

/// The daily goal in SKC: the effort-point goal at [`RATES`].
pub fn goal(points_goal: i64) -> i64 {
    RATES.earned(&Tally { points: points_goal as f64, ..Tally::default() })
}

pub struct Skc;

impl Motivator for Skc {
    fn id(&self) -> &'static str {
        "skc"
    }
    fn icon(&self) -> &'static str {
        "🎟️"
    }
    fn load(&self, conn: &Connection, user_id: i64, course: &ContentStore, tz: i64) -> rusqlite::Result<Box<dyn Hero>> {
        Ok(Box::new(SkcHero::load(conn, user_id, course, tz)?))
    }
    fn reward(&self, _: &Connection, _: i64, _: &ContentStore, run: &RunOutcome) -> rusqlite::Result<Option<Box<dyn Reward>>> {
        Ok(Some(Box::new(SkcReward(run.earned))))
    }
}

/// "+27 SKC · +10 SKC daily goal · +5 SKC lesson passed": the run's total,
/// then each bonus that landed.
struct SkcReward(Tally);

impl SkcReward {
    fn bonuses(&self) -> Vec<(&'static str, i64)> {
        let t = &self.0;
        [("motivator.skc.reward.unit", t.units * RATES.per_unit), ("motivator.skc.reward.goal", t.goals * RATES.per_goal), ("motivator.skc.reward.pass", t.passes * RATES.per_pass)]
            .into_iter()
            .filter(|&(_, n)| n > 0)
            .collect()
    }
}

impl Reward for SkcReward {
    fn render(&self) -> askama::Result<String> {
        let total = RATES.earned(&self.0);
        let mut html = format!(
            r#"{} <span data-testid="skc-reward" data-skc="{total}">{}</span>"#,
            symbol(),
            crate::error::html_escape(&crate::t1("motivator.skc.reward.amount", "n", total))
        );
        for (key, n) in self.bonuses() {
            html.push_str(" · ");
            html.push_str(&format!(r#"<span data-testid="skc-bonus" data-skc="{n}">{}</span>"#, crate::error::html_escape(&crate::t1(key, "n", n))));
        }
        Ok(html)
    }
}

/// The balance and today's earnings, read in [`Skc::load`].
struct SkcHero {
    balance: i64,
    today: i64,
    /// The badges bought in this course, in the worn theme's look.
    shelf: Vec<super::shop::BadgeView>,
}

impl SkcHero {
    fn load(conn: &Connection, user_id: i64, course: &ContentStore, tz: i64) -> rusqlite::Result<SkcHero> {
        let balance = balance(conn, user_id, course, tz)?;
        let today = RATES.earned(&crate::earnings::today(conn, user_id, course, tz)?);
        let shelf = super::shop::owned(conn, user_id, course.id())?
            .iter()
            .filter_map(|id| super::shop::catalogue().get(id))
            .map(|b| super::shop::BadgeView::of(b, course.theme(), true))
            .collect();
        Ok(SkcHero { balance, today, shelf })
    }
}

impl Hero for SkcHero {
    fn render(&self, _: &CourseView, p: &Progress) -> askama::Result<String> {
        let goal = goal(p.today.goal);
        let pct = if goal <= 0 { 100 } else { (self.today * 100 / goal).clamp(0, 100) };
        let rates = [
            (crate::t("motivator.skc.info.per_point"), RATES.per_point),
            (crate::t("motivator.skc.info.per_pass"), RATES.per_pass),
            (crate::t("motivator.skc.info.per_goal"), RATES.per_goal),
            (crate::t("motivator.skc.info.per_unit"), RATES.per_unit),
        ];
        SkcTemplate { p, balance: self.balance, today: self.today, goal, pct, done: p.today.done(), rates, shelf: &self.shelf }.render()
    }
}

#[derive(Template)]
#[template(path = "motivators/skc.html")]
struct SkcTemplate<'a> {
    p: &'a Progress,
    balance: i64,
    today: i64,
    goal: i64,
    pct: i64,
    done: bool,
    rates: [(String, i64); 4],
    shelf: &'a [super::shop::BadgeView],
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::Kind;

    #[test]
    fn the_goal_and_the_line_are_priced_at_skc_rates() {
        assert_eq!(goal(20), 20);
        let line = SkcReward(Tally { points: 7.5, passes: 1, goals: 1, units: 0 });
        assert_eq!(line.bonuses(), vec![("motivator.skc.reward.goal", 10), ("motivator.skc.reward.pass", 5)]);
        let html = line.render().unwrap();
        assert!(html.contains(r#"data-skc="22">+22 SKC<"#), "{html}");
        assert!(html.contains("+10 SKC daily goal"), "{html}");
        assert!(SkcReward(Tally { points: 3.0, ..Tally::default() }).bonuses().is_empty());
        assert!(symbol().contains("currentColor") && !symbol().contains('$'));
    }

    #[test]
    fn the_balance_is_what_the_shared_tables_earned_in_this_course() {
        let lessons = [
            r#"{"id":"l0","slug":"l0","title":"L0","unit":"A1","content":"x","quiz":[{"question_id":"q0","prompt":"p","type":"fill-in-blank","correct_answer":"a"}]}"#,
            r#"{"id":"l1","slug":"l1","title":"L1","unit":"A2","content":"x","quiz":[{"question_id":"q1","prompt":"p","type":"fill-in-blank","correct_answer":"a"}]}"#,
        ];
        let course = crate::content::tests::store(&lessons).unwrap();
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let uid = crate::auth::create_user(c, "ann@example.org", "h").unwrap();
            // Worn by another motivator: SKC keeps counting anyway.
            crate::activity::record(c, uid, course.id(), Kind::Answer, 12, 0)?;
            crate::activity::record(c, uid, "other-course", Kind::Answer, 50, 0)?;
            let l = course.lesson_by_id("l0").unwrap();
            crate::progress::record_taught_run(c, uid, course.id(), l, crate::progress::VIA_SPRINT, 1, 1)?;
            crate::activity::record(c, uid, course.id(), Kind::Answer, 10, 0)?;
            // Effort 22 (the answers; the taught run may log more), +5 pass,
            // +25 unit A1, +10 daily goal.
            let earned = crate::earnings::total(c, uid, &course, 0)?;
            assert_eq!((earned.passes, earned.units, earned.goals), (1, 1, 1));
            let expected = earned.points.floor() as i64 + 5 + 25 + 10;
            assert!(earned.points >= 22.0);
            let hero = SkcHero::load(c, uid, &course, 0)?;
            assert_eq!(hero.balance, expected);
            assert_eq!(hero.today, expected, "all of it today");
            Ok(())
        })
        .unwrap();
    }
}
