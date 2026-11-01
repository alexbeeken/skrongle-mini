//! The kinds of quiz question a lesson may ask (`quiz[].type`).

use super::QuizItem;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QuizType {
    MultipleChoice,
    FillInBlank,
    /// "true" or "false"; served as a two-option multiple choice (the
    /// loader turns it into one, so no feature needs to know about it).
    TrueFalse,
    /// A number, checked by value (`3/4` = `0.75`), within `tolerance`,
    /// optionally followed by `unit`; `;` in the answer makes it an
    /// any-order list (`2; -3`). See `src/mathcheck.rs`.
    Numeric,
    /// An algebraic expression, accepted when equivalent (`2(x+1)` for `2x+2`).
    Expression,
    /// Tap note-value `tiles` into the gap of a bar of `abc` (`notation::fill_bar`).
    FillBar,
    /// Tap note `tiles` into slots to spell a chord (`notation::chord_spell`).
    ChordSpell,
    /// Tap a staff to place a note (`notation::staff_note`).
    StaffNote,
    /// Tap a key of a piano keyboard; graded by pitch class, or the exact key with `octave` (`notation::keyboard`).
    Keyboard,
    /// Lay out `tiles` as a rectangle, type its two sides (`src/algebra_tiles.rs`).
    AlgebraTiles,
    /// Set the clock (#1340): a clock face whose minute hand is dragged to
    /// the time `H:MM`, in steps of `step_minutes`. With `options` it is
    /// turned around: the clock shows the time and the learner picks it.
    /// See `src/clock.rs`.
    Clock,
    /// A number read off a drawing with a tool (`measure`): see `src/measure.rs`.
    Measure,
}

impl QuizType {
    pub fn as_str(self) -> &'static str {
        match self {
            QuizType::MultipleChoice => "multiple-choice",
            QuizType::FillInBlank => "fill-in-blank",
            QuizType::TrueFalse => "true-false",
            QuizType::Numeric => "numeric",
            QuizType::Expression => "expression",
            QuizType::FillBar => "fill-bar",
            QuizType::ChordSpell => "chord-spell",
            QuizType::StaffNote => "staff-note",
            QuizType::Keyboard => "keyboard",
            QuizType::AlgebraTiles => "algebra-tiles",
            QuizType::Clock => "clock",
            QuizType::Measure => "measure",
        }
    }

    /// Answered by typing (everything but a choice of options, the keyboard and the clock).
    pub fn is_typed(self) -> bool {
        !matches!(self, QuizType::MultipleChoice | QuizType::TrueFalse | QuizType::Keyboard | QuizType::Clock)
    }

    /// Answered with a number or a formula: the app checks it by value,
    /// so word tiles and accent hints do not apply.
    pub fn is_math(self) -> bool {
        matches!(self, QuizType::Numeric | QuizType::Expression | QuizType::AlgebraTiles | QuizType::Measure)
    }

    /// The placeholder of the answer box.
    pub fn placeholder(self) -> String {
        crate::t(match self {
            QuizType::Numeric | QuizType::Measure => "quiz.question.placeholder_number",
            QuizType::Expression => "quiz.question.placeholder_expression",
            QuizType::AlgebraTiles => "quiz.question.placeholder_sides",
            _ => "quiz.question.placeholder_text",
        })
    }
}

impl QuizItem {
    /// Answered by picking one of `options`: a multiple choice, or a clock
    /// question turned around.
    pub fn is_choice(&self) -> bool {
        self.kind == QuizType::MultipleChoice || self.kind == QuizType::TrueFalse || (self.kind == QuizType::Clock && !self.options.is_empty())
    }

    /// A clock the learner sets: the minute hand's step, 0 for any other question.
    pub fn clock_step(&self) -> u32 {
        if self.kind == QuizType::Clock && self.options.is_empty() { crate::clock::step(self) } else { 0 }
    }

    /// A clock the learner reads: its hands' angles in degrees ("hour minute"),
    /// empty for any other question. The page gets angles, not the time.
    pub fn clock_hands(&self) -> String {
        if self.kind != QuizType::Clock || self.options.is_empty() {
            return String::new();
        }
        crate::clock::hands(&self.correct_answer).map(|(h, m)| format!("{h} {m}")).unwrap_or_default()
    }
}
