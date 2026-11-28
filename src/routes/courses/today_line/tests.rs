use super::*;
use crate::activity::{record_at, Kind, DAILY_GOAL};

const DAY: i64 = 86_400;
/// Noon on day 100, UTC.
const NOW: i64 = 100 * DAY + 12 * 3600;

fn with_db(f: impl FnOnce(&Connection, i64) -> rusqlite::Result<()>) {
    let db = crate::db::Db::open_in_memory().unwrap();
    db.with(|c| {
        let ann = crate::auth::create_user(c, "ann@example.org", "x")?;
        f(c, ann)
    })
    .unwrap();
}

#[test]
fn a_course_never_practised_has_no_line() {
    with_db(|c, ann| {
        assert_eq!(Days::load(c, ann, 0, NOW)?.line("es", ""), None);
        Ok(())
    });
}

#[test]
fn the_line_names_todays_points_the_streak_and_the_words_due() {
    with_db(|c, ann| {
        record_at(c, ann, "es", Kind::Answer, 4, NOW - DAY, 0)?;
        record_at(c, ann, "es", Kind::Answer, 7, NOW, 0)?;
        c.execute(
            "INSERT INTO user_vocab (user_id, course_id, lesson_id, word, learned_at, due_at) VALUES (?1, 'es', 'l1', 'hola', 1, ?2), (?1, 'es', 'l1', 'adiós', 1, ?2)",
            rusqlite::params![ann, NOW - DAY],
        )?;
        let line = Days::load(c, ann, 0, NOW)?.line("es", "").unwrap();
        assert_eq!((line.today.points, line.streak, line.due), (7, Some(2), Some(2)));
        assert_eq!(line.text, "7 of 20 today · 2-day streak · 2 words due");
        // XP's panel shows no streak, so its line names none.
        assert_eq!(Days::load(c, ann, 0, NOW)?.line("es", "xp").unwrap().streak, None);
        // Another course's numbers stay its own.
        assert_eq!(Days::load(c, ann, 0, NOW)?.line("fr", ""), None);
        Ok(())
    });
}

#[test]
fn a_met_goal_is_a_tick_and_a_quiet_month_drops_the_line() {
    with_db(|c, ann| {
        record_at(c, ann, "es", Kind::Answer, DAILY_GOAL, NOW, 0)?;
        let line = Days::load(c, ann, 0, NOW)?.line("es", "").unwrap();
        assert!(line.today.done());
        assert_eq!(line.text, "✓ today · 1-day streak");
        assert!(Days::load(c, ann, 0, NOW + (QUIET_AFTER_DAYS - 1) * DAY)?.line("es", "").is_some());
        assert_eq!(Days::load(c, ann, 0, NOW + QUIET_AFTER_DAYS * DAY)?.line("es", ""), None);
        Ok(())
    });
}
