use super::*;
use crate::content::{CourseFile, MANIFEST_FILE};

const MANIFEST: &str = r#"{
    "id": "p", "title": "Placed", "levels": true,
    "units": [{"id": "A1", "title": "A1"}, {"id": "A2", "title": "A2"}, {"id": "B1", "title": "B1"}]
}"#;

/// A lesson in `unit` with `n` multiple-choice questions (the answer is "yes") and one fill-in.
fn lesson(unit: &str, n: usize) -> CourseFile {
    let quiz: Vec<String> = (1..=n)
        .map(|i| format!(r#"{{"question_id": "{unit}-{i}", "prompt": "{unit} q{i}", "type": "multiple-choice", "options": ["yes", "no"], "correct_answer": "yes"}}"#))
        .chain([format!(r#"{{"question_id": "{unit}-typed", "prompt": "type", "type": "fill-in-blank", "correct_answer": "x"}}"#)])
        .collect();
    let id = unit.to_lowercase();
    CourseFile {
        path: format!("lessons/{id}.json"),
        content: format!(r###"{{"id": "{id}", "slug": "{id}", "title": "{unit}", "unit": "{unit}", "content": "## {unit}\n\nText.", "quiz": [{}]}}"###, quiz.join(",")),
    }
}

fn course(manifest: &str, placement: Option<&str>) -> Result<ContentStore, ContentError> {
    let mut files = vec![CourseFile { path: MANIFEST_FILE.into(), content: manifest.into() }, lesson("A1", 3), lesson("A2", 3), lesson("B1", 1)];
    if let Some(p) = placement {
        files.push(CourseFile { path: FILE.into(), content: p.into() });
    }
    ContentStore::from_files(&files)
}

const WRITTEN: &str = r#"{"questions": [
    {"unit": "A1", "prompt": "One", "options": ["a", "b"], "answer": "a"},
    {"unit": "A1", "prompt": "Two", "options": ["a", "b"], "answer": "b"},
    {"unit": "A2", "prompt": "Three", "options": ["a", "b", "c"], "answer": "c"}
]}"#;

#[test]
fn placement_json_is_not_a_lesson_and_is_checked() {
    let store = course(MANIFEST, Some(WRITTEN)).unwrap();
    assert_eq!(store.lessons().len(), 3);
    assert_eq!(store.placement().unwrap().questions.len(), 3);
    for (bad, says) in [
        (r#"{"questions": [{"unit": "C9", "prompt": "x", "options": ["a", "b"], "answer": "a"}]}"#, "no unit 'C9'"),
        (r#"{"questions": [{"unit": "A1", "prompt": "x", "options": ["a"], "answer": "a"}]}"#, "at least 2 options"),
        (r#"{"questions": [{"unit": "A1", "prompt": "x", "options": ["a", "a"], "answer": "a"}]}"#, "option twice"),
        (r#"{"questions": [{"unit": "A1", "prompt": "x", "options": ["a", "b"], "answer": "c"}]}"#, "one of the options"),
        (r#"{"questions": [{"unit": "A1", "prompt": " ", "options": ["a", "b"], "answer": "a"}]}"#, "no prompt"),
        (r#"{}"#, "no questions"),
        (r#"{"random_per_unit": 11}"#, "at most 10"),
    ] {
        let e = course(MANIFEST, Some(bad)).unwrap_err().to_string();
        assert!(e.starts_with("placement.json") && e.contains(says), "{bad}: {e}");
    }
}

#[test]
fn a_course_without_levels_offers_no_test() {
    let plain = MANIFEST.replace(r#""levels": true,"#, "");
    let store = course(&plain, Some(WRITTEN)).unwrap();
    assert!(store.placement().is_none());
    assert!(draw(&store, &mut Rng::seeded(1)).is_empty());
    assert!(mark(&store, &[]).is_none());
}

#[test]
fn written_questions_win_and_other_units_draw_multiple_choice() {
    let t = r#"{"random_per_unit": 2, "questions": [{"unit": "A2", "prompt": "Three", "options": ["a", "b", "c"], "answer": "c"}]}"#;
    let store = course(MANIFEST, Some(t)).unwrap();
    let shown = draw(&store, &mut Rng::seeded(7));
    let units: Vec<&str> = shown.iter().map(|q| q.unit.as_str()).collect();
    // A1 draws two of three, A2 has its written one, B1 has only one to draw.
    assert_eq!(units, ["A1", "A1", "A2", "B1"]);
    assert!(shown.iter().filter(|q| q.unit != "A2").all(|q| q.id.starts_with("q:") && !q.id.contains("typed")));
    assert_eq!(shown[2].id, "w1");
    let mut opts = shown[2].options.clone();
    opts.sort();
    assert_eq!(opts, ["a", "b", "c"]);
}

#[test]
fn the_first_unit_with_a_miss_is_where_to_start() {
    let store = course(MANIFEST, Some(WRITTEN)).unwrap();
    let a = |pairs: &[(&str, &str)]| pairs.iter().map(|(i, a)| (i.to_string(), a.to_string())).collect::<Vec<_>>();
    // A1 half right, A2 right: start at A1.
    let r = mark(&store, &a(&[("w1", "a"), ("w2", "a"), ("w3", "c")])).unwrap();
    assert_eq!(r.start_unit, "A1");
    assert_eq!(r.units.iter().map(|u| (u.unit.as_str(), u.correct, u.total, u.status)).collect::<Vec<_>>(), [("A1", 1, 2, "some"), ("A2", 1, 1, "all")]);
    // A1 right, A2 wrong: start at A2, A1 can be skipped.
    let r = mark(&store, &a(&[("w1", "a"), ("w2", "b"), ("w3", "a")])).unwrap();
    assert_eq!((r.start_unit.as_str(), r.units[1].status), ("A2", "none"));
    // All right: the unit after the last one tested.
    let r = mark(&store, &a(&[("w1", "a"), ("w2", "b"), ("w3", "c")])).unwrap();
    assert_eq!(r.start_unit, "B1");
    // Answers twice, unknown ids and typed questions are left out.
    let r = mark(&store, &a(&[("w1", "b"), ("w1", "a"), ("w9", "a"), ("q:A1-typed", "x"), ("nope", "a")])).unwrap();
    assert_eq!((r.units.len(), r.units[0].correct, r.units[0].total), (1, 0, 1));
}

#[test]
fn drawn_questions_are_marked_by_the_lesson_answer() {
    let store = course(MANIFEST, Some(r#"{"random_per_unit": 1}"#)).unwrap();
    let a = |pairs: &[(&str, &str)]| pairs.iter().map(|(i, a)| (i.to_string(), a.to_string())).collect::<Vec<_>>();
    let r = mark(&store, &a(&[("q:A1-1", "yes"), ("q:A2-2", "no"), ("q:B1-1", "yes")])).unwrap();
    assert_eq!(r.start_unit, "A2");
    // Everything right through the last unit: stay at the last one.
    let r = mark(&store, &a(&[("q:A1-1", "yes"), ("q:A2-2", "yes"), ("q:B1-1", "yes")])).unwrap();
    assert_eq!(r.start_unit, "B1");
}
