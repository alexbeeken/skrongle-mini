//! "Measure the angle" (#1352), the `measure` quiz type. The page draws the
//! question's `angle` and a see-through protractor (`tool`) that the learner
//! drags and turns onto it (`static/measure.js`, drawn with JSXGraph from
//! `static/vendor/`); the learner reads the angle and types the number. It is
//! graded like `numeric`, and reading the other scale of the protractor
//! (180 minus the angle) gets its own note.
//!
//! Only Study (practice and the reading check) and the Guide can draw it.
//! Sprint mode and the AI tutor skip it (`QuizType::needs_widget`).

use serde::{Deserialize, Serialize};

use crate::content::{Lesson, QuizItem, QuizType};
use crate::mathcheck::{check_numeric, validate_numeric, MathVerdict};

impl Lesson {
    /// Does the lesson ask a measure question (load static/measure.js)?
    pub fn has_measure(&self) -> bool {
        self.quiz.iter().any(|q| q.kind == QuizType::Measure)
    }
}

/// The tools a `measure` question can draw.
pub const TOOLS: [&str; 1] = ["protractor"];

/// The drawing of a `measure` question; empty on every other type.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Measure {
    /// The measuring tool the page draws: `protractor`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tool: String,
    /// The angle the page draws, in degrees (more than 0, less than 180).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<f64>,
}

impl Measure {
    fn is_empty(&self) -> bool {
        self.tool.is_empty() && self.angle.is_none()
    }
}

/// What a page needs to draw a `measure` question (`templates/measure.html`).
pub struct MeasureView {
    pub tool: String,
    pub angle: String,
}

impl QuizItem {
    pub fn measure_view(&self) -> Option<MeasureView> {
        let angle = self.measure.angle.filter(|_| self.kind == QuizType::Measure)?;
        Some(MeasureView { tool: self.measure.tool.clone(), angle: format!("{angle}") })
    }
}

impl QuizType {
    /// Shown with a drawing or board only a page can make: modes that ask in text
    /// (Sprint mode, the AI tutor) leave these questions out.
    pub fn needs_widget(self) -> bool {
        matches!(self, QuizType::Measure | QuizType::AlgebraTiles)
    }
}

/// Course file checks for `tool` and `angle`, on every question.
pub fn validate(q: &QuizItem) -> Result<(), String> {
    if q.kind != QuizType::Measure {
        return if q.measure.is_empty() { Ok(()) } else { Err("tool and angle are for measure questions".into()) };
    }
    if !TOOLS.contains(&q.measure.tool.as_str()) {
        return Err(format!("a measure question needs a tool: {}", TOOLS.join(", ")));
    }
    match q.measure.angle {
        Some(a) if a > 0.0 && a < 180.0 => {}
        _ => return Err("a measure question needs an angle in degrees, more than 0 and less than 180".into()),
    }
    for a in std::iter::once(&q.correct_answer).chain(&q.accepted_answers) {
        validate_numeric(a, &q.unit)?;
    }
    if q.tolerance.is_some_and(|t| !(t >= 0.0 && t.is_finite())) {
        return Err("tolerance must be a number >= 0".into());
    }
    Ok(())
}

/// Grade a `measure` answer like `numeric`. An answer that would be right
/// for 180 minus the angle was read off the other scale.
pub fn check(q: &QuizItem, given: &str) -> MathVerdict {
    let right = |want: &str| check_numeric(want, given, q.tolerance, &q.unit);
    let main = right(&q.correct_answer);
    if main.correct || q.accepted_answers.iter().any(|a| right(a).correct) {
        return MathVerdict { correct: true, note: None };
    }
    if let Some(a) = q.measure.angle {
        if right(&format!("{}", 180.0 - a)).correct {
            return MathVerdict { correct: false, note: Some(crate::t("quiz.check.other_scale")) };
        }
    }
    MathVerdict { correct: false, note: main.note }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn protractor(angle: f64) -> QuizItem {
        serde_json::from_value(serde_json::json!({
            "question_id": "m", "prompt": "How big is this angle?", "type": "measure", "tool": "protractor",
            "angle": angle, "correct_answer": format!("{angle}"), "tolerance": 2, "unit": "°"
        }))
        .unwrap()
    }

    #[test]
    fn graded_by_value_within_the_tolerance() {
        let q = protractor(125.0);
        assert!(check(&q, "125").correct);
        assert!(check(&q, "124").correct);
        assert!(check(&q, "127°").correct);
        assert!(!check(&q, "128").correct);
        assert_eq!(check(&q, "128").note, None);
    }

    #[test]
    fn the_other_scale_gets_its_note() {
        let q = protractor(125.0);
        let v = check(&q, "55");
        assert!(!v.correct);
        assert_eq!(v.note, Some(crate::t("quiz.check.other_scale")));
        assert!(check(&q, "56").note.is_some(), "within the tolerance of 55");
    }

    #[test]
    fn fields_are_checked() {
        assert!(validate(&protractor(125.0)).is_ok());
        let mut q = protractor(125.0);
        q.measure.tool = "ruler".into();
        assert!(validate(&q).unwrap_err().contains("tool"));
        for bad in [0.0, 180.0, 200.0] {
            assert!(validate(&protractor(bad)).unwrap_err().contains("angle"), "{bad}");
        }
        let mut q = protractor(40.0);
        q.measure.angle = None;
        assert!(validate(&q).is_err());
        let mut q = protractor(40.0);
        q.kind = QuizType::Numeric;
        assert!(validate(&q).unwrap_err().contains("measure questions"));
    }
}
