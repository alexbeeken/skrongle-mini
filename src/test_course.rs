//! Keeps the test course complete. The test course
//! (`data/test-courses/test-course/`, offered only with `TEST_COURSE=1`) uses
//! every feature a course file can switch on, one lesson per feature, so the
//! browser tests can reach each feature without playing the Spanish course.
//!
//! These tests fail when something a course can use is missing from it: a
//! field of `docs/course.schema.json` or one of its enum values, a legacy
//! alias of `src/content.rs`, a quiz type, a Sprint mode exercise kind, a Guide
//! step, an installed theme. Adding a feature means adding it to the test
//! course (and to the browser tests that use it); its AUTHORING.md says how.

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::content::{ContentStore, GuideStep, QuizType, MANIFEST_FILE};
use crate::courses::{Courses, TEST_COURSES_DIR};
use crate::sprint::{self, Exercise};
use crate::quiz::Rng;

const ID: &str = "test-course";

fn dir() -> PathBuf {
    Path::new("data").join(TEST_COURSES_DIR).join(ID)
}

fn store() -> ContentStore {
    ContentStore::load_dir(&dir()).expect("the test course loads")
}

/// The manifest and every lesson file, as raw JSON (aliases and all).
fn raw() -> (Value, Vec<(String, Value)>) {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "json") {
                out.push(p);
            }
        }
    }
    let read = |p: &Path| -> Value { serde_json::from_str(&fs::read_to_string(p).unwrap()).unwrap() };
    let mut paths = Vec::new();
    walk(&dir().join("lessons"), &mut paths);
    paths.sort();
    let lessons = paths.iter().map(|p| (p.display().to_string(), read(p))).collect();
    (read(&dir().join(MANIFEST_FILE)), lessons)
}

fn raw_lesson(id: &str) -> Value {
    raw().1.into_iter().map(|(_, l)| l).find(|l| l["id"] == id).unwrap_or_else(|| panic!("no lesson '{id}' in the test course"))
}

// ---------- the schema ----------

fn schema() -> Value {
    serde_json::from_str(include_str!("../docs/course.schema.json")).expect("course.schema.json parses")
}

fn deref<'a>(root: &'a Value, r: &str) -> &'a Value {
    r.trim_start_matches("#/").split('/').fold(root, |n, part| &n[part])
}

/// Walk `schema` over `values`, gathering the values that reach each `$ref`
/// (a quiz item is checked once, whether it sits in a lesson or a theme block).
fn gather<'a>(root: &'a Value, schema: &'a Value, values: Vec<&'a Value>, seen: &mut BTreeMap<String, Vec<&'a Value>>) {
    if let Some(r) = schema.get("$ref").and_then(Value::as_str) {
        seen.entry(r.to_string()).or_default().extend(values.iter().copied());
        return gather(root, deref(root, r), values, seen);
    }
    for alt in schema.get("oneOf").and_then(Value::as_array).into_iter().flatten() {
        gather(root, alt, values.clone(), seen);
    }
    for (k, sub) in schema.get("properties").and_then(Value::as_object).into_iter().flatten() {
        gather(root, sub, values.iter().filter_map(|v| v.get(k)).collect(), seen);
    }
    if let Some(items) = schema.get("items") {
        gather(root, items, values.iter().filter_map(|v| v.as_array()).flatten().collect(), seen);
    }
    if let Some(extra) = schema.get("additionalProperties").filter(|a| a.is_object()) {
        gather(root, extra, values.iter().filter_map(|v| v.as_object()).flat_map(|o| o.values()).collect(), seen);
    }
}

/// What `values` never use of `schema`: a property, an enum value, an array
/// left empty. `$ref`s are not followed: each is checked on its own.
fn unused(schema: &Value, values: &[&Value], path: &str, out: &mut Vec<String>) {
    if schema.get("$ref").is_some() {
        return;
    }
    for alt in schema.get("oneOf").and_then(Value::as_array).into_iter().flatten() {
        unused(alt, values, path, out);
    }
    for e in schema.get("enum").and_then(Value::as_array).into_iter().flatten() {
        if !values.contains(&e) {
            out.push(format!("{path} = {e}"));
        }
    }
    for (k, sub) in schema.get("properties").and_then(Value::as_object).into_iter().flatten() {
        let found: Vec<&Value> = values.iter().filter_map(|v| v.get(k)).collect();
        match found.is_empty() {
            true => out.push(format!("{path}.{k}")),
            false => unused(sub, &found, &format!("{path}.{k}"), out),
        }
    }
    if let Some(items) = schema.get("items") {
        let found: Vec<&Value> = values.iter().filter_map(|v| v.as_array()).flatten().collect();
        match found.is_empty() {
            true => out.push(format!("{path}[]")),
            false => unused(items, &found, &format!("{path}[]"), out),
        }
    }
    if let Some(extra) = schema.get("additionalProperties").filter(|a| a.is_object()) {
        let found: Vec<&Value> = values.iter().filter_map(|v| v.as_object()).flat_map(|o| o.values()).collect();
        match found.is_empty() {
            true => out.push(format!("{path}.*")),
            false => unused(extra, &found, &format!("{path}.*"), out),
        }
    }
}

#[test]
fn loads_only_with_test_course_on() {
    let data = Path::new("data");
    let plain = Courses::load(data, false).expect("built-in courses load");
    assert!(!plain.is_builtin(ID), "the test course must stay out unless TEST_COURSE=1");
    let with = Courses::load(data, true).expect("built-in and test courses load");
    assert!(with.is_builtin(ID));
    let ids = |c: &Courses| c.builtin().iter().map(|s| s.id().to_string()).collect::<Vec<_>>();
    assert_eq!(ids(&with)[..ids(&plain).len()], ids(&plain)[..], "real courses come first");
    let s = store();
    assert_eq!(s.id(), ID);
    assert!(s.title().contains("Test"), "the title says what it is: {}", s.title());
}

#[test]
fn uses_every_field_and_enum_value_of_the_schema() {
    let root = schema();
    let (manifest, lessons) = raw();
    let placement: Value = serde_json::from_str(&fs::read_to_string(dir().join(crate::placement::FILE)).expect("the test course has a placement test")).unwrap();
    let mut seen = BTreeMap::new();
    for (r, values) in [("#/$defs/manifest", vec![&manifest]), ("#/$defs/placement", vec![&placement]), ("#/$defs/lesson", lessons.iter().map(|(_, l)| l).collect())] {
        seen.entry(r.to_string()).or_insert_with(Vec::new).extend(values.iter().copied());
        gather(&root, deref(&root, r), values, &mut seen);
    }
    let mut missing = Vec::new();
    for (r, values) in &seen {
        unused(deref(&root, r), values, r.trim_start_matches("#/$defs/"), &mut missing);
    }
    assert!(
        missing.is_empty(),
        "the test course does not use these parts of docs/course.schema.json; add them to data/test-courses/{ID}/ \
         (and to the browser tests that use it):\n  {}",
        missing.join("\n  ")
    );
}

#[test]
fn the_everything_lesson_has_every_lesson_field() {
    let root = schema();
    let lesson = raw_lesson("tc-everything");
    let missing: Vec<&String> =
        deref(&root, "#/$defs/lesson")["properties"].as_object().unwrap().keys().filter(|k| lesson.get(k.as_str()).is_none()).collect();
    assert!(missing.is_empty(), "lessons/0-all/everything.json lacks {missing:?}");
}

#[test]
fn uses_every_legacy_alias() {
    let src = include_str!("content.rs");
    let aliases: BTreeSet<&str> = src
        .match_indices("alias = \"")
        .map(|(i, m)| {
            let rest = &src[i + m.len()..];
            &rest[..rest.find('"').unwrap()]
        })
        .collect();
    assert!(aliases.len() >= 10, "{aliases:?}");
    fn keys(v: &Value, out: &mut HashSet<String>) {
        match v {
            Value::Object(o) => o.iter().for_each(|(k, v)| {
                out.insert(k.clone());
                keys(v, out);
            }),
            Value::Array(a) => a.iter().for_each(|v| keys(v, out)),
            _ => {}
        }
    }
    let mut used = HashSet::new();
    keys(&raw_lesson("tc-legacy"), &mut used);
    let missing: Vec<&&str> = aliases.iter().filter(|a| !used.contains(**a)).collect();
    assert!(missing.is_empty(), "lessons/5-edge/legacy.json does not use the aliases {missing:?}");
}

/// Every quiz type. A new variant does not compile here: add it to the list
/// and give the test course a lesson of it.
fn all_quiz_types() -> [QuizType; 12] {
    match QuizType::MultipleChoice {
        QuizType::MultipleChoice | QuizType::FillInBlank | QuizType::TrueFalse | QuizType::Numeric | QuizType::Expression | QuizType::FillBar | QuizType::ChordSpell | QuizType::StaffNote | QuizType::Keyboard | QuizType::AlgebraTiles | QuizType::Clock | QuizType::Measure => {}
    }
    [QuizType::MultipleChoice, QuizType::FillInBlank, QuizType::TrueFalse, QuizType::Numeric, QuizType::Expression, QuizType::FillBar, QuizType::ChordSpell, QuizType::StaffNote, QuizType::Keyboard, QuizType::AlgebraTiles, QuizType::Clock, QuizType::Measure]
}

#[test]
fn every_quiz_type_has_its_own_lesson() {
    let lessons = raw().1;
    for t in all_quiz_types() {
        let slug = t.as_str();
        let l = lessons.iter().map(|(_, l)| l).find(|l| l["slug"] == slug).unwrap_or_else(|| panic!("no lesson with slug '{slug}'"));
        let kinds: Vec<&Value> = l["quiz"].as_array().unwrap().iter().map(|q| &q["type"]).collect();
        assert!(!kinds.is_empty() && kinds.iter().all(|k| **k == slug), "lesson '{slug}' should only ask {slug} questions: {kinds:?}");
    }
}

/// The name of an exercise, telling apart the variants a browser test cares
/// about. A new exercise kind does not compile here: name it and make sure a
/// lesson of the test course produces it.
fn kind(e: &Exercise) -> &'static str {
    match e {
        Exercise::Bite { .. } => "bite",
        Exercise::Choice { .. } => "choice",
        Exercise::Clock { options, .. } if options.is_empty() => "set-clock",
        Exercise::Clock { .. } => "read-clock",
        Exercise::Tiles { math: false, .. } => "word-tiles",
        Exercise::Tiles { tiles, .. } if tiles.is_empty() => "typed-math",
        Exercise::Tiles { .. } => "number-tiles",
        Exercise::FillBar { .. } => "fill-bar",
        Exercise::ChordSpell { .. } => "chord-spell",
        Exercise::StaffNote { .. } => "staff-note",
        Exercise::Keyboard { .. } => "keyboard",
        Exercise::Sort { .. } => "sort",
        Exercise::Spot { .. } => "spot",
        Exercise::Build { id, .. } if id.starts_with("cbuild:") => "conversation-build",
        Exercise::Build { .. } => "build",
        Exercise::Ladder { .. } => "ladder",
        Exercise::Scale { .. } => "scale",
        Exercise::Pick { to_term: true, .. } => "pick-term",
        Exercise::Pick { .. } => "pick-meaning",
        Exercise::Match { .. } => "match",
        Exercise::HearBuild { .. } => "hear-build",
        Exercise::HearGap { .. } => "hear-gap",
        Exercise::HearPick { to_meaning: true, .. } => "hear-meaning",
        Exercise::HearPick { .. } => "hear-pick",
        Exercise::HearWhich { .. } => "hear-which",
        Exercise::HearMatch { .. } => "hear-match",
    }
}

const ALL_KINDS: [&str; 26] = [
    "bite", "choice", "set-clock", "read-clock", "word-tiles", "typed-math", "number-tiles", "fill-bar", "chord-spell", "staff-note", "keyboard", "sort", "spot", "conversation-build", "build", "ladder", "scale",
    "pick-term", "pick-meaning", "match", "hear-build", "hear-gap", "hear-meaning", "hear-pick", "hear-which", "hear-match",
];

#[test]
fn sprint_serves_every_exercise_kind() {
    let c = store();
    let mut seen = BTreeSet::new();
    let none = HashSet::new();
    let learned: HashSet<(String, String)> = c.lessons().iter().flat_map(|l| l.vocabulary.iter().map(|v| (l.id.clone(), v.term.clone()))).collect();
    for seed in 0..40 {
        let mut rng = Rng::seeded(seed);
        for l in c.lessons() {
            seen.extend(sprint::lesson_session(&c, l, &none, &mut rng).exercises.iter().map(kind));
        }
        let review = sprint::review_session(&c, &Default::default(), &none, &crate::vocab::Learned { keys: learned.clone(), due: Default::default() }, 0, 0, &mut rng);
        let lightning = sprint::lightning_session(&c, &Default::default(), &none, &learned, 0, 0, &mut rng);
        for s in [review, lightning] {
            seen.extend(s.exercises.iter().map(kind));
            serde_json::to_string(&s).unwrap();
        }
    }
    let missing: Vec<&&str> = ALL_KINDS.iter().filter(|k| !seen.contains(**k)).collect();
    assert!(missing.is_empty(), "no lesson of the test course produces {missing:?}");
}

/// What a Sprint mode session of each feature lesson always contains, whatever the
/// shuffle: what the AUTHORING.md map promises the browser tests.
#[test]
fn each_feature_lesson_always_shows_its_feature() {
    let c = store();
    let expect: &[(&str, &[&str])] = &[
        ("multiple-choice", &["choice"]),
        ("true-false", &["choice"]),
        ("fill-in-blank", &["word-tiles"]),
        ("numeric", &["number-tiles"]),
        ("expression", &["typed-math"]),
        ("fill-bar", &["fill-bar"]),
        ("chord-spell", &["chord-spell"]),
        ("staff-note", &["staff-note"]),
        ("keyboard", &["keyboard"]),
        ("clock", &["set-clock", "read-clock"]),
        ("bites", &["bite"]),
        ("sort", &["sort"]),
        ("spot", &["spot"]),
        ("build", &["build"]),
        ("conjugations", &["ladder"]),
        ("conversation", &["conversation-build"]),
        ("vocabulary", &["pick-term", "match"]),
        ("read-only", &["bite"]),
        ("notation", &["bite", "build", "choice"]),
        ("ear", &["choice", "sort", "scale"]),
    ];
    for seed in 0..40 {
        let mut rng = Rng::seeded(seed);
        for (slug, kinds) in expect {
            let l = c.lesson_by_slug(slug).unwrap_or_else(|| panic!("no lesson '{slug}'"));
            let got: BTreeSet<&str> = sprint::lesson_session(&c, l, &HashSet::new(), &mut rng).exercises.iter().map(kind).collect();
            for k in *kinds {
                assert!(got.contains(k), "seed {seed}: the '{slug}' session has no {k}: {got:?}");
            }
        }
        let l = c.lesson_by_slug("listening").unwrap();
        let got: BTreeSet<&str> = sprint::lesson_session(&c, l, &HashSet::new(), &mut rng).exercises.iter().map(kind).collect();
        assert!(got.iter().any(|k| k.starts_with("hear-")), "seed {seed}: the listening session has no listening exercise: {got:?}");
    }
}

/// Algebra tiles need a board: Sprint mode never deals them (#1355).
#[test]
fn sprint_mode_skips_algebra_tiles() {
    let c = store();
    let l = c.lesson_by_slug("algebra-tiles").expect("the algebra-tiles lesson");
    for seed in 0..10 {
        let s = sprint::lesson_session(&c, l, &HashSet::new(), &mut Rng::seeded(seed));
        let ids: Vec<String> = s.exercises.iter().map(|e| format!("{e:?}")).filter(|d| d.contains("tc-algebra-tiles-")).collect();
        assert!(ids.is_empty(), "seed {seed}: Sprint mode dealt {ids:?}");
    }
}

#[test]
fn every_guide_step_kind_appears() {
    let c = store();
    let mut seen = BTreeSet::new();
    for l in c.lessons() {
        for s in &l.guide_steps {
            // A new step kind does not compile here: name it and use it in the test course.
            seen.insert(match s {
                GuideStep::Concept { .. } => "concept",
                GuideStep::Question { .. } => "question",
                GuideStep::Vocab { .. } => "vocab",
                GuideStep::Tip { .. } => "tip",
            });
        }
    }
    assert_eq!(seen.len(), 4, "{seen:?}");
    assert!(c.lessons().iter().any(|l| l.guide.is_empty() && l.guide_question_count() > 0), "a lesson with a derived Guide");
}

#[test]
fn every_course_level_feature_is_on() {
    let c = store();
    let m = c.manifest();
    assert!(m.is_language(), "a language course: Listening, Numbers by ear, tap to hear");
    assert!(m.calendar.is_some(), "Date Dash");
    assert!(c.has_conjugations(), "the paradigm drill");
    assert!(c.has_listening(), "Listening");
    assert!(c.conversations().next().is_some(), "Sprint mode conversation builds");
    assert!(c.lessons().iter().any(|l| l.vocabulary.len() >= 5), "Word Match, flashcards, match the pairs");
    assert!(crate::placement::offered(&c), "the placement test (#1106)");

    let lessons = c.lessons();
    assert!(lessons.iter().any(|l| l.quiz.is_empty() && l.vocabulary.is_empty()), "a lesson to read only (I've read it)");
    assert!(lessons.iter().any(|l| l.group == "general" && l.order == 100), "a lesson with the default group and order");
    assert!(lessons.iter().any(|l| !m.groups.iter().any(|g| g.id == l.group) && l.group != "general"), "a group the manifest does not list");
    assert!(lessons.iter().any(|l| !l.prerequisites.is_empty()), "prerequisites");
    let listening: Vec<_> = lessons.iter().flat_map(|l| &l.listening).collect();
    assert!(listening.iter().any(|i| i.note.as_deref().is_some_and(|n| n.starts_with("New word:"))), "a listening new word");
    assert!(listening.iter().any(|i| i.meaning.is_empty()), "a listening sentence without a meaning");
    assert!(listening.iter().any(|i| crate::content::words_of(&i.text).len() > 10), "a listening sentence too long for tiles");
    assert!(lessons.iter().flat_map(|l| &l.sections).any(|s| s.heading.to_lowercase().starts_with("quick check")), "a Quick check section");
}

#[test]
fn every_installed_theme_has_complete_material() {
    let themes = crate::themes::Themes::load(Path::new("data")).expect("themes load");
    assert!(!themes.is_empty());
    let root = schema();
    let fields: Vec<&String> = deref(&root, "#/$defs/lesson_theme")["properties"].as_object().unwrap().keys().collect();
    let lesson = raw_lesson("tc-themes");
    let blocks = lesson["themes"].as_object().unwrap();
    for pack in themes.all() {
        let id = &pack.theme.id;
        let block = blocks.get(id).unwrap_or_else(|| panic!("lessons/5-edge/themes.json has no block for the theme '{id}'"));
        let missing: Vec<&&String> = fields.iter().filter(|f| block.get(f.as_str()).is_none()).collect();
        assert!(missing.is_empty(), "the '{id}' block of lessons/5-edge/themes.json lacks {missing:?}");
        let s = ContentStore::load_dir_themed(&dir(), Some(&pack.theme)).unwrap_or_else(|e| panic!("with theme {id}: {e}"));
        let l = s.lesson_by_id("tc-themes").unwrap();
        assert!(l.quiz[0].themed && l.sprint.bites[0].themed, "theme {id} material comes first");
    }
    assert!(
        blocks.keys().any(|k| themes.get(k).is_none()),
        "a block for a theme that is not installed (kept and never shown)"
    );
}
