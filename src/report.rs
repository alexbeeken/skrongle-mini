//! The course editor's feature usage report (#328): for each lesson, which
//! question types its quiz asks and which lesson features its file switches
//! on, so an author sees at a glance how much of the course format the AI (or
//! they) used. Computed from the loaded `ContentStore`; drawn by
//! `templates/course_report.html` in the editor's report modal.

use crate::content::{ContentStore, Lesson, QuizItem, QuizType};

/// One entry of a lesson's lists: a question type or a lesson feature.
pub struct Mark {
    pub key: &'static str,
    pub label: String,
    pub used: bool,
}

pub struct LessonReport {
    pub id: String,
    pub title: String,
    pub questions: Vec<Mark>,
    pub features: Vec<Mark>,
}

impl LessonReport {
    pub fn used(&self) -> usize {
        self.questions.iter().chain(&self.features).filter(|m| m.used).count()
    }

    pub fn total(&self) -> usize {
        self.questions.len() + self.features.len()
    }
}

pub struct UnitReport {
    pub id: String,
    pub title: String,
    pub lessons: Vec<LessonReport>,
    /// Features at least one of its lessons uses.
    pub features_used: usize,
    /// Question types at least one of its lessons asks.
    pub questions_used: usize,
}

pub struct CourseReport {
    pub units: Vec<UnitReport>,
    /// Features the report lists.
    features: usize,
    /// Question types the report lists: music types only in a music course.
    questions: usize,
}

impl CourseReport {
    pub fn feature_count(&self) -> usize {
        self.features
    }

    pub fn question_count(&self) -> usize {
        self.questions
    }
}

/// Every quiz type, as the author writes it, with the key of its label
/// (`i18n`). The loader turns `true-false` into a two-option multiple
/// choice; `kind_of` tells them apart again.
pub const QUESTION_TYPES: [(QuizType, &str); 12] = [
    (QuizType::MultipleChoice, "courses.report.question.multiple_choice"),
    (QuizType::FillInBlank, "courses.report.question.fill_in_blank"),
    (QuizType::TrueFalse, "courses.report.question.true_false"),
    (QuizType::Numeric, "courses.report.question.numeric"),
    (QuizType::Expression, "courses.report.question.expression"),
    (QuizType::FillBar, "courses.report.question.fill_bar"),
    (QuizType::ChordSpell, "courses.report.question.chord_spell"),
    (QuizType::StaffNote, "courses.report.question.staff_note"),
    (QuizType::Keyboard, "courses.report.question.keyboard"),
    (QuizType::AlgebraTiles, "courses.report.question.algebra_tiles"),
    (QuizType::Clock, "courses.report.question.clock"),
    (QuizType::Measure, "courses.report.question.measure"),
];

/// Quiz types about music: the report lists them in a course with a `music` block only.
pub const MUSIC_TYPES: [QuizType; 4] = [QuizType::FillBar, QuizType::ChordSpell, QuizType::StaffNote, QuizType::Keyboard];

type Detect = fn(&Lesson) -> bool;

/// The lesson features of the report: (key, the key of its label in
/// `i18n`, does this lesson use it).
pub const FEATURES: [(&str, &str, Detect); 20] = [
    ("text", "courses.report.feature.text", |l| !l.content.trim().is_empty()),
    ("cards", "courses.report.feature.cards", |l| l.sections.len() > 1),
    ("tables", "courses.report.feature.tables", |l| l.sections.iter().any(|s| s.html.contains("<table"))),
    ("summary", "courses.report.feature.summary", |l| !l.summary.trim().is_empty()),
    ("focus", "courses.report.feature.focus", |l| !l.focus.trim().is_empty()),
    ("prerequisites", "courses.report.feature.prerequisites", |l| !l.prerequisites.is_empty()),
    ("vocabulary", "courses.report.feature.vocabulary", |l| !l.vocabulary.is_empty()),
    ("examples", "courses.report.feature.examples", |l| l.vocabulary.iter().any(|v| v.example.as_deref().is_some_and(|e| !e.trim().is_empty()))),
    ("hints", "courses.report.feature.hints", |l| l.quiz.iter().any(|q| !q.hints.is_empty())),
    ("explanations", "courses.report.feature.explanations", |l| l.quiz.iter().any(|q| !q.explanation.trim().is_empty())),
    ("teaches", "courses.report.feature.teaches", |l| l.quiz.iter().any(|q| !q.teaches.is_empty())),
    ("conjugations", "courses.report.feature.conjugations", |l| !l.conjugations.is_empty()),
    ("conversations", "courses.report.feature.conversations", |l| !l.conversations.is_empty()),
    ("listening", "courses.report.feature.listening", |l| !l.listening.is_empty()),
    ("guide", "courses.report.feature.guide", |l| !l.guide.is_empty()),
    ("sprint-icon", "courses.report.feature.sprint_icon", |l| !l.sprint.icon.trim().is_empty()),
    ("bites", "courses.report.feature.bites", |l| !l.sprint.bites.is_empty()),
    ("sorts", "courses.report.feature.sorts", |l| !l.sprint.sorts.is_empty()),
    ("spots", "courses.report.feature.spots", |l| !l.sprint.spots.is_empty()),
    ("builds", "courses.report.feature.builds", |l| !l.sprint.builds.is_empty()),
];

/// The type the author wrote: a loaded `true-false` question is a multiple
/// choice of exactly True / False.
fn kind_of(q: &QuizItem) -> QuizType {
    if q.kind == QuizType::MultipleChoice && q.options.len() == 2 && q.options[0] == "True" && q.options[1] == "False" {
        return QuizType::TrueFalse;
    }
    q.kind
}

pub fn lesson_report(l: &Lesson) -> LessonReport {
    LessonReport {
        id: l.id.clone(),
        title: l.title.clone(),
        questions: QUESTION_TYPES
            .iter()
            .map(|(t, label)| Mark { key: t.as_str(), label: crate::t(label), used: l.quiz.iter().any(|q| kind_of(q) == *t) })
            .collect(),
        features: FEATURES.iter().map(|(key, label, f)| Mark { key, label: crate::t(label), used: f(l) }).collect(),
    }
}

/// The report of a whole course: its units in manifest order, each with its lessons in order.
pub fn feature_usage(store: &ContentStore) -> CourseReport {
    let music = store.manifest().music.is_some();
    let questions = QUESTION_TYPES.len() - if music { 0 } else { MUSIC_TYPES.len() };
    let units = store
        .units()
        .iter()
        .enumerate()
        .map(|(i, u)| {
            let lessons: Vec<LessonReport> = store.lessons().iter().filter(|l| l.unit_index == i).map(lesson_report).map(|mut r| {
                r.questions.retain(|m| music || !MUSIC_TYPES.iter().any(|t| t.as_str() == m.key));
                r
            }).collect();
            let any = |pick: fn(&LessonReport) -> &Vec<Mark>, n: usize| (0..n).filter(|&k| lessons.iter().any(|l| pick(l)[k].used)).count();
            UnitReport {
                id: u.id.clone(),
                title: u.title.clone(),
                features_used: any(|l| &l.features, FEATURES.len()),
                questions_used: any(|l| &l.questions, questions),
                lessons,
            }
        })
        .collect();
    CourseReport { units, features: FEATURES.len(), questions }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn test_course() -> ContentStore {
        ContentStore::load_dir(&Path::new("data").join(crate::courses::TEST_COURSES_DIR).join("test-course")).expect("the test course loads")
    }

    /// The test course switches on every feature (one lesson per feature), so
    /// the report must find each of them somewhere; a feature it cannot find
    /// is either missing from the test course or detected wrongly here.
    #[test]
    fn test_course_uses_every_reported_feature() {
        let report = feature_usage(&test_course());
        let lessons: Vec<&LessonReport> = report.units.iter().flat_map(|u| &u.lessons).collect();
        for (k, (key, ..)) in FEATURES.iter().enumerate() {
            assert!(lessons.iter().any(|l| l.features[k].used), "no lesson of the test course is reported to use '{key}'");
        }
        for (k, (t, _)) in QUESTION_TYPES.iter().enumerate() {
            assert!(lessons.iter().any(|l| l.questions[k].used), "no lesson of the test course is reported to ask {}", t.as_str());
        }
    }

    /// Each question-type lesson of the test course asks only its own type.
    #[test]
    fn question_type_lessons_report_only_their_type() {
        let store = test_course();
        for (t, _) in QUESTION_TYPES {
            let l = store.lessons().iter().find(|l| l.slug == t.as_str()).unwrap();
            let r = lesson_report(l);
            let asked: Vec<&str> = r.questions.iter().filter(|m| m.used).map(|m| m.key).collect();
            assert_eq!(asked, vec![t.as_str()], "lesson '{}'", l.slug);
        }
    }

    #[test]
    fn the_everything_lesson_uses_every_feature() {
        let store = test_course();
        let r = lesson_report(store.lesson_by_id("tc-everything").unwrap());
        let unused: Vec<&str> = r.features.iter().filter(|m| !m.used).map(|m| m.key).collect();
        assert!(unused.is_empty(), "tc-everything does not use {unused:?}");
    }

    #[test]
    fn unit_counts_are_features_used_by_any_lesson() {
        let report = feature_usage(&test_course());
        for u in &report.units {
            assert!(u.features_used <= FEATURES.len() && u.questions_used <= QUESTION_TYPES.len());
            let best = u.lessons.iter().map(|l| l.features.iter().filter(|m| m.used).count()).max().unwrap_or(0);
            assert!(u.features_used >= best, "{}", u.id);
        }
        assert_eq!(report.units.iter().map(|u| u.lessons.len()).sum::<usize>(), test_course().lessons().len());
    }

    /// The keyboard is a music question: a course without `music` leaves it out.
    #[test]
    fn a_course_without_music_leaves_the_keyboard_out() {
        let spanish = ContentStore::load_dir(Path::new("data/courses/spanish")).expect("Spanish loads");
        let report = feature_usage(&spanish);
        assert_eq!(report.question_count(), QUESTION_TYPES.len() - MUSIC_TYPES.len());
        assert!(report.units.iter().flat_map(|u| &u.lessons).all(|l| l.questions.iter().all(|m| m.key != "keyboard")));
        assert_eq!(feature_usage(&test_course()).question_count(), QUESTION_TYPES.len());
    }
}
