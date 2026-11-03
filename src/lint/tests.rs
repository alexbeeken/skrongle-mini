use super::*;
use crate::content::CourseFile;
use serde_json::{json, Value};

/// A one-unit course of these lessons (each gets id, slug, unit, group and order from its place).
fn course(lessons: Vec<Value>) -> ContentStore {
    course_in("", "", lessons)
}

/// The same, with the terms and the meanings each in a language.
fn course_in(terms: &str, meanings: &str, lessons: Vec<Value>) -> ContentStore {
    let mut files = vec![CourseFile {
        path: "course.json".into(),
        content: json!({
            "id": "lint-test", "title": "Lint test", "subject": "Lint test", "description": "", "icon": "📚",
            "terms": { "label": "Term", "lang": terms }, "meanings": { "label": "Meaning", "lang": meanings },
            "units": [{ "id": "1", "title": "Unit 1", "description": "" }],
            "groups": [{ "id": "basics", "title": "Basics" }], "chars": []
        })
        .to_string(),
    }];
    for (i, mut l) in lessons.into_iter().enumerate() {
        let id = format!("l{}", i + 1);
        let o = l.as_object_mut().unwrap();
        o.entry("id").or_insert(json!(id));
        o.insert("slug".into(), json!(id));
        o.entry("title").or_insert(json!(format!("Lesson {}", i + 1)));
        o.insert("unit".into(), json!("1"));
        o.insert("group".into(), json!("basics"));
        o.insert("order".into(), json!(i + 1));
        o.entry("content").or_insert(json!("## Idea\n\nText."));
        if let Some(Value::Array(quiz)) = o.get_mut("quiz") {
            for (k, q) in quiz.iter_mut().enumerate() {
                q["question_id"] = json!(format!("{id}-q{k}"));
            }
        }
        files.push(CourseFile { path: format!("lessons/{id}.json"), content: l.to_string() });
    }
    ContentStore::from_files(&files).expect("the course loads")
}

fn mc(prompt: &str, options: &[&str], answer: &str) -> Value {
    json!({ "question_id": format!("q-{prompt}"), "prompt": prompt, "type": "multiple-choice", "options": options, "correct_answer": answer })
}

/// A blank with a second accepted answer, so it is not a thing to check.
fn fill(n: usize, answer: &str) -> Value {
    let mut q = blank(n, answer);
    q["accepted_answers"] = json!([format!("{answer} too")]);
    q
}

/// A blank that accepts its one answer only.
fn blank(n: usize, answer: &str) -> Value {
    json!({ "question_id": format!("f{n}-{answer}"), "prompt": format!("Blank {n}: ___"), "type": "fill-in-blank", "correct_answer": answer })
}

fn four_questions() -> Vec<Value> {
    (1..=4).map(|n| fill(n, "word")).collect()
}

fn kinds(store: &ContentStore) -> Vec<(String, &'static str)> {
    lint(store).into_iter().map(|f| (f.lesson_id, f.kind)).collect()
}

fn paths(store: &ContentStore) -> Vec<(&'static str, String)> {
    lint(store).into_iter().map(|f| (f.kind, f.path)).collect()
}

#[test]
fn a_clean_course_has_no_findings() {
    let store = course(vec![json!({ "quiz": four_questions() }), json!({ "quiz": four_questions(), "prerequisites": ["l1"] })]);
    assert!(kinds(&store).is_empty(), "{:?}", kinds(&store));
    assert_eq!(log(&store), 0);
}

#[test]
fn a_prerequisite_after_its_lesson_is_found() {
    let store = course(vec![json!({ "quiz": four_questions(), "prerequisites": ["l2"] }), json!({ "quiz": four_questions(), "prerequisites": ["l2"] })]);
    let found = lint(&store);
    assert_eq!(kinds(&store), vec![("l1".into(), "prerequisite-later"), ("l2".into(), "prerequisite-self")]);
    assert!(found[0].message.contains("“Lesson 2”"), "{}", found[0].message);
    assert_eq!(found[0].path, "prerequisites");
    assert_eq!(log(&store), 2);
}

#[test]
fn up_to_three_questions_leave_no_room_for_a_mistake() {
    for n in 1..=5 {
        let store = course(vec![json!({ "quiz": (1..=n).map(|k| fill(k, "word")).collect::<Vec<_>>() })]);
        let few = kinds(&store).iter().any(|(_, k)| *k == "few-questions");
        assert_eq!(few, n <= 3, "{n} questions");
    }
    // No questions: the lesson passes on seen, by design.
    let store = course(vec![json!({ "vocabulary": [{ "term": "a", "meaning": "b" }] })]);
    assert!(kinds(&store).is_empty());
}

#[test]
fn an_answer_in_its_own_prompt_is_found_unless_the_prompt_lists_the_choices() {
    let mut quiz = four_questions();
    quiz.push(mc("What does 'gato' (cat) mean?", &["cat", "dog"], "cat"));
    quiz.push(mc("Which fits: ser or estar?", &["ser", "estar"], "ser"));
    quiz.push(mc("Pick the concatenated word", &["cat", "dog"], "cat"));
    let store = course(vec![json!({ "quiz": quiz })]);
    let found: Vec<_> = lint(&store).into_iter().filter(|f| f.kind == "answer-in-prompt").collect();
    assert_eq!(found.len(), 1);
    assert!(found[0].message.contains("gato"), "{}", found[0].message);
    assert_eq!(found[0].path, "quiz.4.prompt");
}

#[test]
fn a_blank_whose_answer_stands_in_its_prompt_is_found_unless_the_answer_is_short() {
    let mut quiz = four_questions();
    let mut q = |n: usize, prompt: &str, answer: &str| {
        let mut b = fill(n, answer);
        b["prompt"] = json!(prompt);
        quiz.push(b);
    };
    q(5, "El gato duerme: el ___ (gato)", "gato");
    q(6, "Voy ___ Madrid, no de Madrid", "de");
    q(7, "A concatenated ___", "cat");
    q(8, "Complete with du, de la or des: Elle mange ___ soupe.", "de la");
    let store = course(vec![json!({ "quiz": quiz })]);
    let found: Vec<_> = lint(&store).into_iter().filter(|f| f.kind == "answer-in-prompt").collect();
    assert_eq!(found.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), vec!["quiz.4.prompt"]);
}

#[test]
fn a_blank_with_one_accepted_answer_is_found_with_its_field() {
    let mut quiz = four_questions();
    quiz.push(blank(5, "me llamo"));
    quiz.push(blank(6, "gato"));
    let store = course(vec![json!({ "quiz": quiz })]);
    let found: Vec<_> = lint(&store).into_iter().filter(|f| f.kind == "one-spelling").collect();
    // The one-word blank is the common case and is left alone.
    assert_eq!(found.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), vec!["quiz.4.accepted_answers"]);
    assert!(found[0].message.contains("me llamo"), "{}", found[0].message);
}

#[test]
fn a_sprint_of_bites_only_is_found() {
    let store = course(vec![json!({}), json!({ "quiz": four_questions() })]);
    assert_eq!(kinds(&store), vec![("l1".into(), "sprint-reading-only")]);
}

#[test]
fn a_term_taught_in_a_third_lesson_is_found_there() {
    let words = |t: &str| json!({ "quiz": four_questions(), "vocabulary": [{ "term": t, "meaning": "x" }] });
    let mut last = words("hola ");
    last["vocabulary"] = json!([{ "term": "other", "meaning": "y" }, { "term": "hola ", "meaning": "x" }]);
    let store = course(vec![words("hola"), words("Hola"), words("adiós"), last]);
    let found = lint(&store);
    assert_eq!(kinds(&store), vec![("l4".into(), "repeated-term")]);
    assert!(found[0].message.contains("“Lesson 1”, “Lesson 2”"), "{}", found[0].message);
    assert_eq!(paths(&store), vec![("repeated-term", "vocabulary.1.term".into())]);
}

#[test]
fn text_in_the_terms_language_of_a_course_explained_in_another_is_found() {
    let spanish = "## Saludos\n\nEn esta lección vas a aprender a saludar. La palabra se usa cuando conoces a alguien, y es lo primero que escucha de ti. Practica con un amigo o una amiga.";
    let english = "## Greetings\n\nIn this lesson you will learn how to say hello. The word is used when you meet someone, and it is the first thing they hear from you. Practise it with a friend.";
    let lesson = |content: &str| json!({ "quiz": four_questions(), "content": content });
    let store = course_in("es", "en-GB", vec![lesson(spanish), lesson(english)]);
    let found = lint(&store);
    assert_eq!(kinds(&store), vec![("l1".into(), "other-language")]);
    assert_eq!(found[0].path, "content");
    assert!(found[0].message.contains("Spanish") && found[0].message.contains("English"), "{}", found[0].message);
    // A course explained in its own language, or one that is not a language course, is left alone.
    assert!(kinds(&course_in("es", "es", vec![lesson(spanish)])).is_empty());
    assert!(kinds(&course_in("", "", vec![lesson(spanish)])).is_empty());
}

#[test]
fn contains_words_needs_word_edges() {
    assert!(contains_words("is it cat?", "cat"));
    assert!(!contains_words("concatenate", "cat"));
    assert!(contains_words("cat", "cat"));
}

/// Not a test: how often each rule fires on the courses in the repo.
/// `cargo test lint::tests::counts_on_the_built_in_courses -- --ignored --nocapture`
#[test]
#[ignore]
fn counts_on_the_built_in_courses() {
    for dir in ["data/courses/spanish", "data/courses/french", "data/test-courses/test-course"] {
        let store = ContentStore::load_dir(std::path::Path::new(dir)).expect("the course loads");
        let found = lint(&store);
        let mut by_kind: Vec<(&str, usize)> = Vec::new();
        for f in &found {
            match by_kind.iter_mut().find(|(k, _)| *k == f.kind) {
                Some((_, n)) => *n += 1,
                None => by_kind.push((f.kind, 1)),
            }
        }
        println!("{dir}: {} lessons, {} findings: {by_kind:?}", store.lessons().len(), found.len());
        for f in found.iter().filter(|f| matches!(f.kind, "answer-in-prompt" | "other-language")) {
            println!("  {} {} {}: {}", f.lesson_id, f.kind, f.path, f.message);
        }
    }
}
