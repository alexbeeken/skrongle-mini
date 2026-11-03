//! Set the clock (#1340): a `clock` question's time is `H:MM`, an hour of
//! the 12-hour clock and two digits of minutes. The widget in the page
//! (`static/clock.js`) posts the time it shows in that form, and it is
//! graded here as the same time on the clock face (`12:05` = `0:05`).

use crate::content::{Lesson, QuizItem, QuizType};

impl Lesson {
    /// Does the lesson ask a clock question (load static/clock.js)?
    pub fn has_clock(&self) -> bool {
        self.quiz.iter().any(|q| q.kind == QuizType::Clock)
    }
}

/// The minute hand's step when the question names none.
pub const DEFAULT_STEP: u32 = 5;

/// The minute hand's step of a clock question.
pub fn step(item: &QuizItem) -> u32 {
    item.step_minutes.unwrap_or(DEFAULT_STEP)
}

/// `H:MM` as (hour on the face 0–11, minutes). Hours up to 23 are read on
/// the 12-hour face, so `15:45` is `3:45`.
pub fn parse(s: &str) -> Option<(u32, u32)> {
    let (h, m) = s.trim().split_once(':')?;
    if h.is_empty() || h.len() > 2 || m.len() != 2 || !h.bytes().chain(m.bytes()).all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some((h % 12, m))
}

/// Do two times show the same on the clock face?
pub fn same(a: &str, b: &str) -> bool {
    matches!((parse(a), parse(b)), (Some(x), Some(y)) if x == y)
}

/// The angles of the hands for a time, in degrees clockwise from 12:
/// (hour hand, minute hand). The hour hand moves on as the minutes pass.
pub fn hands(time: &str) -> Option<(f64, f64)> {
    let (h, m) = parse(time)?;
    Some((f64::from(h) * 30.0 + f64::from(m) * 0.5, f64::from(m) * 6.0))
}

/// Check a clock question as the loader does: the step divides an hour,
/// every time is `H:MM` with an hour from 1 to 12, a time to set falls on a
/// step, and a clock turned around (with `options`) has its time among them.
pub fn validate(q: &QuizItem) -> Result<(), String> {
    let step = step(q);
    if step == 0 || 60 % step != 0 {
        return Err(format!("step_minutes must divide 60 (5, 15, 30…), not {step}"));
    }
    let time = |s: &str| -> Result<u32, String> {
        let h: Option<u32> = s.trim().split_once(':').and_then(|(h, _)| h.parse().ok());
        match parse(s) {
            Some((_, m)) if h.is_some_and(|h| (1..=12).contains(&h)) => Ok(m),
            _ => Err(format!("'{s}' is not a time on the clock: write H:MM with an hour from 1 to 12, as 3:45")),
        }
    };
    if q.options.is_empty() {
        for a in std::iter::once(&q.correct_answer).chain(&q.accepted_answers) {
            if time(a)? % step != 0 {
                return Err(format!("'{a}' is not on a {step}-minute step of the minute hand"));
            }
        }
        return Ok(());
    }
    for o in &q.options {
        time(o)?;
    }
    if q.options.len() < 2 || !q.options.contains(&q.correct_answer) {
        return Err("a clock with options needs at least 2 of them, correct_answer among them".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_on_the_face() {
        assert_eq!(parse("3:45"), Some((3, 45)));
        assert_eq!(parse(" 12:00 "), Some((0, 0)));
        assert_eq!(parse("15:45"), Some((3, 45)));
        assert_eq!(parse("3:5"), None);
        assert_eq!(parse("3.45"), None);
        assert_eq!(parse("3:60"), None);
        assert!(same("12:05", "0:05"));
        assert!(same("3:45", "15:45"));
        assert!(!same("3:45", "4:45"));
        assert!(!same("3:45", "quarter to four"));
    }

    #[test]
    fn the_hour_hand_follows_the_minutes() {
        assert_eq!(hands("3:45"), Some((112.5, 270.0)));
        assert_eq!(hands("12:00"), Some((0.0, 0.0)));
        assert_eq!(hands("6:30"), Some((195.0, 180.0)));
    }

    fn q(json: &str) -> QuizItem {
        serde_json::from_str(&format!(r#"{{ "question_id": "c", "prompt": "p", "type": "clock", {json} }}"#)).unwrap()
    }

    #[test]
    fn what_the_loader_accepts() {
        assert!(validate(&q(r#""correct_answer": "3:45""#)).is_ok());
        assert!(validate(&q(r#""correct_answer": "3:47""#)).is_err(), "off the 5-minute step");
        assert!(validate(&q(r#""correct_answer": "3:47", "step_minutes": 1"#)).is_ok());
        assert!(validate(&q(r#""correct_answer": "3:45", "step_minutes": 7"#)).is_err());
        assert!(validate(&q(r#""correct_answer": "15:45""#)).is_err(), "hours run 1 to 12");
        assert!(validate(&q(r#""correct_answer": "0:45""#)).is_err());
        assert!(validate(&q(r#""correct_answer": "3:45", "options": ["3:45", "9:15"]"#)).is_ok());
        assert!(validate(&q(r#""correct_answer": "3:45", "options": ["4:45", "9:15"]"#)).is_err());
        assert!(validate(&q(r#""correct_answer": "3:45", "options": ["3:45", "late"]"#)).is_err());
    }
}
