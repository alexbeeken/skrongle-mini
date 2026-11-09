use super::*;

pub const MANIFEST: &str = r#"{
        "id": "t", "title": "Test", "terms": {"label": "Español", "lang": "es"}, "meanings": {"label": "English", "lang": "en"},
        "units": [{"id": "A1", "title": "A1", "description": "Beginner"}, {"id": "A2", "title": "A2"}],
        "groups": [{"id": "Grammar", "title": "Grammar"}],
        "paradigm": {"slots": [{"key": "yo", "label": "yo"}, {"key": "tu", "label": "tú"}], "sets": [{"key": "present", "label": "Present"}]}
    }"#;

/// A course that is not about a language (terms and meanings both English):
/// what the built-in Loyalty Marketing course used to cover in tests.
pub const PLAIN_MANIFEST: &str = r#"{
        "id": "knots", "title": "Knots", "terms": {"label": "Term", "lang": "en"}, "meanings": {"label": "Definition", "lang": "en"},
        "units": [{"id": "1", "title": "Basics"}]
    }"#;
pub const PLAIN_LESSON: &str = r###"{
        "id": "k-bowline", "slug": "bowline", "title": "The bowline", "unit": "1",
        "content": "## What it is\n\nA fixed loop that will not slip.",
        "vocabulary": [{"term": "bight", "meaning": "a U-shaped bend in the rope"}, {"term": "standing part", "meaning": "the rope that leads away from the knot"}],
        "quiz": [{"question_id": "k-bowline-01", "prompt": "A bowline makes a loop that…", "type": "multiple-choice",
                  "options": ["does not slip", "tightens under load"], "correct_answer": "does not slip"}],
        "listening": [{"text": "Make a small loop in the standing part.", "meaning": "Make a small loop in the standing part."}],
        "sprint": {"spots": [{"sentence": "A bowline tightens under load.", "wrong": "tightens", "fix": "holds", "why": "It is a fixed loop."}]}
    }"###;

pub fn plain_store() -> ContentStore {
    let files = [
        CourseFile { path: MANIFEST_FILE.into(), content: PLAIN_MANIFEST.into() },
        CourseFile { path: "lessons/bowline.json".into(), content: PLAIN_LESSON.into() },
    ];
    ContentStore::from_files(&files).expect("plain course loads")
}

pub fn store(lessons: &[&str]) -> Result<ContentStore, ContentError> {
    let mut files = vec![CourseFile { path: MANIFEST_FILE.into(), content: MANIFEST.into() }];
    for (i, l) in lessons.iter().enumerate() {
        files.push(CourseFile { path: format!("lessons/l{i}.json"), content: (*l).to_string() });
    }
    ContentStore::from_files(&files)
}

#[test]
fn group_title_is_looked_up_and_falls_back_to_the_id() {
    let manifest = r#"{
        "id": "t", "title": "Test", "terms": {"label": "Term", "lang": "en"}, "meanings": {"label": "Meaning", "lang": "en"},
        "units": [{"id": "1", "title": "Basics"}],
        "groups": [{"id": "gr", "title": "Grammar"}, {"id": "vo", "title": "Vocabulary"}]
    }"#;
    let files = [CourseFile { path: MANIFEST_FILE.into(), content: manifest.into() }];
    let s = ContentStore::from_files(&files).unwrap();
    assert_eq!(s.group_title("gr"), "Grammar");
    assert_eq!(s.group_title("vo"), "Vocabulary");
    assert_eq!(s.group_title("nope"), "nope");
}

#[test]
fn loads_the_builtin_spanish_course() {
    let s = ContentStore::load_dir(Path::new("data/courses/spanish")).expect("spanish course loads");
    assert_eq!(s.id(), "spanish");
    assert!(s.lessons().len() >= 20);
    assert!(s.paradigm().is_some());
    assert_eq!(s.lessons()[0].unit_index, 0);
}

#[test]
fn levels_a_course_can_be_started_at() {
    // Spanish: units are levels; only those with lessons are offered.
    let s = ContentStore::load_dir(Path::new("data/courses/spanish")).expect("spanish course loads");
    assert!(s.manifest().levels);
    let with_lessons: Vec<usize> = (0..s.units().len()).filter(|&u| s.lessons_in(u).next().is_some()).collect();
    assert!(with_lessons.starts_with(&[0, 1]), "{with_lessons:?}");
    assert_eq!(s.start_units(), with_lessons);
    assert_eq!((s.start_index(""), s.start_index("A1"), s.start_index("A2")), (0, 0, 1));
    assert_eq!(s.start_index("Z9"), 0, "an id the course no longer has");
    // A level without lessons is not a start.
    let levels = MANIFEST.replace(r#""units": ["#, r#""levels": true, "units": [{"id": "A0", "title": "A0"}, "#);
    let files = [
        CourseFile { path: MANIFEST_FILE.into(), content: levels },
        CourseFile { path: "lessons/a.json".into(), content: r#"{"id":"a","slug":"a","title":"A","unit":"A1","content":"x"}"#.into() },
        CourseFile { path: "lessons/b.json".into(), content: r#"{"id":"b","slug":"b","title":"B","unit":"A2","content":"x"}"#.into() },
    ];
    let empty = ContentStore::from_files(&files).unwrap();
    assert_eq!(empty.start_units(), vec![1, 2]);
    assert_eq!(empty.start_index("A0"), 0, "a level without lessons is not a start");
    assert_eq!(empty.start_index("A2"), 2);
    // Without `levels`, no picker and always the beginning.
    let plain = store(&[
        r#"{"id":"a","slug":"a","title":"A","unit":"A1","content":"x"}"#,
        r#"{"id":"b","slug":"b","title":"B","unit":"A2","content":"x"}"#,
    ])
    .unwrap();
    assert!(!plain.manifest().levels);
    assert!(plain.start_units().is_empty());
    assert_eq!(plain.start_index("A2"), 0);
}

#[test]
fn loads_every_builtin_course() {
    let courses = crate::courses::Courses::load_builtin(Path::new("data")).expect("built-in courses load");
    let ids: Vec<&str> = courses.builtin().iter().map(|c| c.id()).collect();
    assert!(ids.contains(&"spanish"), "{ids:?}");
    for c in courses.builtin().iter().map(|c| courses.builtin_store(c.id()).expect("builds")) {
        assert!(!c.lessons().is_empty(), "{} has no lessons", c.id());
        for l in c.lessons() {
            assert!(!l.quiz.is_empty(), "{}/{} has no quiz", c.id(), l.slug);
            assert!(l.vocabulary.len() >= 2, "{}/{} has too little vocabulary", c.id(), l.slug);
        }
    }
}

#[test]
fn language_courses_are_detected() {
    let s = store(&[]).unwrap();
    assert!(s.manifest().is_language());
    let mut m = s.manifest().clone();
    m.meanings.lang = "ES".into();
    assert!(!m.is_language(), "same language on both sides is not a language course");
    m.meanings.lang = String::new();
    assert!(m.is_language());
    m.terms.lang = String::new();
    assert!(!m.is_language(), "terms without a language are terminology, not a language");
}

#[test]
fn language_features_override_the_language_codes() {
    let s = store(&[]).unwrap();
    let mut m = s.manifest().clone();
    m.language_features = Some(false);
    assert!(!m.is_language(), "a Spanish course with the switch off keeps its lang codes but loses the language features");
    m.terms.lang = String::new();
    m.language_features = Some(true);
    assert!(m.is_language(), "a course in the learner's own language can switch them on");
    m.language_features = None;
    assert!(!m.is_language());
    // The field is written only when set, so courses that never touched it stay as they were.
    let json = serde_json::to_string(&m).unwrap();
    assert!(!json.contains("language_features"), "{json}");
    m.language_features = Some(false);
    assert!(serde_json::to_string(&m).unwrap().contains("\"language_features\":false"));
}

#[test]
fn calendar_is_validated() {
    let s = ContentStore::load_dir(Path::new("data/courses/spanish")).unwrap();
    let cal = s.manifest().calendar.as_ref().expect("spanish course has a calendar");
    assert_eq!(cal.days[0], "lunes");
    assert_eq!(cal.months[9], "octubre");
    assert_eq!(cal.write(3, 9, 9, 2026), "jueves, 9 de octubre de 2026");

    let with = |cal: &str| {
        let m = MANIFEST.trim_end().trim_end_matches('}').to_string() + ", \"calendar\": " + cal + "}";
        ContentStore::from_files(&[CourseFile { path: MANIFEST_FILE.into(), content: m }])
    };
    let days = r#"["l","m","x","j","v","s","d"]"#;
    let months = r#"["1","2","3","4","5","6","7","8","9","10","11","12"]"#;
    let ok = with(&format!(r#"{{"days": {days}, "months": {months}}}"#)).unwrap();
    assert_eq!(ok.manifest().calendar.as_ref().unwrap().format, "{day}, {d} {month} {year}");
    let e = with(&format!(r#"{{"days": ["l","m"], "months": {months}}}"#)).unwrap_err().to_string();
    assert!(e.contains("calendar.days") && e.contains(MANIFEST_FILE), "{e}");
    let e = with(&format!(r#"{{"days": {days}, "months": ["1"]}}"#)).unwrap_err().to_string();
    assert!(e.contains("calendar.months"), "{e}");
    let e = with(&format!(r#"{{"days": {days}, "months": {months}, "format": "{{d}}"}}"#)).unwrap_err().to_string();
    assert!(e.contains("calendar.format"), "{e}");
    let e = with(&format!(r#"{{"days": ["l","l","x","j","v","s","d"], "months": {months}}}"#)).unwrap_err().to_string();
    assert!(e.contains("distinct"), "{e}");
}

#[test]
fn aliases_keep_old_files_loading() {
    let s = store(&[r###"{"id":"l","slug":"l","title":"L","level":"A1","category":"Grammar",
            "markdown_content":"## One\n\ntext","target_grammar_point":"x",
            "vocabulary":[{"es":"hola","en":"hello"}],
            "conjugations":[{"verb":"hablar","meaning":"to speak","tense":"present","forms":{"yo":"hablo","tu":"hablas"}}],
            "journey":[{"section":"one"},{"vocab":1}]}"###])
    .unwrap();
    let l = &s.lessons()[0];
    assert_eq!(l.unit, "A1");
    assert_eq!(l.group, "Grammar");
    assert_eq!(l.focus, "x");
    assert_eq!(l.vocabulary[0].term, "hola");
    assert_eq!(l.conjugations[0].item, "hablar");
    assert_eq!(l.conjugations[0].form("tu"), "hablas");
    assert_eq!(l.guide_steps.len(), 2);
}

#[test]
fn rejects_bad_content() {
    let bad_unit = r#"{"id":"l","slug":"l","title":"L","unit":"Z9","content":"x"}"#;
    assert!(store(&[bad_unit]).unwrap_err().to_string().contains("unit 'Z9'"));
    let bad_form = r#"{"id":"l","slug":"l","title":"L","unit":"A1","content":"x",
            "conjugations":[{"item":"ir","meaning":"go","set":"present","forms":{"yo":"voy"}}]}"#;
    assert!(store(&[bad_form]).unwrap_err().to_string().contains("missing the 'tu' form"));
    let dup = r#"{"id":"l","slug":"l","title":"L","unit":"A1","content":"x"}"#;
    assert!(matches!(store(&[dup, dup]), Err(ContentError::Clash { .. })));
    let empty = r#"{"id":"l","slug":"l","title":"L","unit":"A1","content":""}"#;
    assert!(store(&[empty]).unwrap_err().to_string().contains("guide"));
    assert!(ContentStore::from_files(&[]).unwrap_err().to_string().contains("course.json"));
}

#[test]
fn maths_and_true_false_questions() {
    let l = r#"{"id":"l","slug":"l","title":"L","unit":"A1","content":"x","quiz":[
            {"question_id":"n1","prompt":"3/4 as a decimal?","type":"numeric","correct_answer":"0.75"},
            {"question_id":"n2","prompt":"Roots of x^2+x-6","type":"numeric","correct_answer":"2; -3"},
            {"question_id":"n3","prompt":"Length?","type":"numeric","correct_answer":"12.5","unit":"cm","tolerance":0.1},
            {"question_id":"e1","prompt":"Expand 2(x+1)","type":"expression","correct_answer":"2x + 2"},
            {"question_id":"t1","prompt":"0 is even.","type":"true-false","correct_answer":"true"}]}"#;
    let s = store(&[l]).unwrap();
    let quiz = &s.lessons()[0].quiz;
    assert_eq!(quiz[2].unit, "cm");
    assert_eq!(quiz[3].kind, QuizType::Expression);
    // true-false is served as a two-option choice.
    assert_eq!(quiz[4].kind, QuizType::MultipleChoice);
    assert_eq!(quiz[4].options, vec!["True", "False"]);
    assert_eq!(quiz[4].correct_answer, "True");
    let bad = |q: &str| {
        let l = format!(r#"{{"id":"l","slug":"l","title":"L","unit":"A1","content":"x","quiz":[{q}]}}"#);
        store(&[l.as_str()]).unwrap_err().to_string()
    };
    assert!(bad(r#"{"question_id":"n","prompt":"p","type":"numeric","correct_answer":"twelve"}"#).contains("not a number"));
    assert!(bad(r#"{"question_id":"e","prompt":"p","type":"expression","correct_answer":"2x +"}"#).contains("not an expression"));
    assert!(bad(r#"{"question_id":"t","prompt":"p","type":"true-false","correct_answer":"maybe"}"#).contains("true-false"));
    assert!(bad(r#"{"question_id":"f","prompt":"p","type":"fill-in-blank","correct_answer":"a","unit":"cm"}"#).contains("numeric"));
    assert!(bad(r#"{"question_id":"n","prompt":"p","type":"numeric","correct_answer":"1","tolerance":-1}"#).contains("tolerance"));
}

#[test]
fn parses_enum_names() {
    let t: QuizType = serde_json::from_str("\"fill-in-blank\"").unwrap();
    assert_eq!(t, QuizType::FillInBlank);
}

#[test]
fn splits_sections_and_builds_guide() {
    let md = "Intro text.\n\n## Use *ser*\n\nser body\n\n## Use estar\n\nestar body\n\n## Third\n\nmore";
    let secs = split_sections(md);
    assert_eq!(secs.len(), 4);
    assert_eq!(secs[0].heading, "Introduction");
    assert_eq!(secs[1].heading, "Use ser");
    assert!(secs[2].html.contains("<p>estar body</p>"));

    let q = |id: &str| QuizItem {
        question_id: id.into(),
        prompt: "p".into(),
        kind: QuizType::FillInBlank,
        options: vec![],
        correct_answer: "x".into(),
        accepted_answers: vec![],
        explanation: String::new(),
        hints: vec![],
        teaches: vec![],
        tolerance: None,
        unit: String::new(),
        abc: String::new(), play: String::new(), option_kind: String::new(),
        tiles: vec![],
        exact: false,
        clef: String::new(),
        any_octave: false, octave: false, octaves: None, keys: vec![],
        form: String::new(),
        step_minutes: None,
        measure: Default::default(),
    };
    let mut lesson: Lesson = serde_json::from_str(
        r#"{"id":"l","slug":"l","title":"L","unit":"A1","content":"","vocabulary":[{"term":"hola","meaning":"hello"}]}"#,
    )
    .unwrap();
    lesson.sections = secs;
    lesson.quiz = vec![q("q1"), q("q2"), q("q3")];
    let steps = build_guide(&lesson).unwrap();
    let kinds: Vec<&str> = steps
        .iter()
        .map(|s| match s {
            GuideStep::Concept { .. } => "c",
            GuideStep::Question { .. } => "q",
            GuideStep::Vocab { .. } => "v",
            GuideStep::Tip { .. } => "t",
        })
        .collect();
    assert_eq!(kinds, vec!["c", "v", "c", "c", "q", "c", "q", "q"]);

    lesson.guide = vec![
        GuideSpec::Section { section: "use ser".into() },
        GuideSpec::Question { question: "q2".into() },
        GuideSpec::Tip { tip: "hi".into() },
        GuideSpec::Vocab { vocab: 5 },
    ];
    let steps = build_guide(&lesson).unwrap();
    assert_eq!(steps.len(), 4);
    assert!(matches!(&steps[1], GuideStep::Question { index: 1 }));
    lesson.guide = vec![GuideSpec::Question { question: "nope".into() }];
    assert!(build_guide(&lesson).is_err());
}

#[test]
fn words_taught_explicit_and_automatic() {
    let s = store(&[r#"{"id":"l","slug":"l","title":"L","unit":"A1","content":"x",
            "vocabulary":[{"term":"hablar","meaning":"to speak"},{"term":"hablo","meaning":"I speak"},{"term":"el banco","meaning":"bank"}],
            "quiz":[{"question_id":"q1","prompt":"p","type":"fill-in-blank","correct_answer":"Hablo","teaches":["hablar"]}]}"#])
    .unwrap();
    let l = &s.lessons()[0];
    let words: Vec<&str> = s.words_taught(l, &l.quiz[0]).iter().map(|w| w.term.as_str()).collect();
    assert_eq!(words, vec!["hablar", "hablo"]);
}

#[test]
fn markdown_renders_tables() {
    let html = render_markdown("| a | b |\n|---|---|\n| 1 | 2 |");
    assert!(html.contains("<table>"));
    assert!(html.starts_with(r#"<div class="table-scroll" data-testid="table-scroll"><table>"#), "{html}");
    assert!(html.trim_end().ends_with("</table></div>") || html.trim_end().ends_with("</table>\n</div>"), "{html}");
}

/// Course text can come from anyone (an upload, an AI fed hostile
/// material): no HTML or script survives rendering.
#[test]
fn markdown_never_renders_raw_html_or_script_links() {
    let html = render_markdown("Hi <script>alert(1)</script>\n\n<img src=x onerror=alert(1)>\n\n<b onclick=x>bold</b>");
    assert!(!html.contains("<script"), "{html}");
    assert!(!html.contains("<img"), "{html}");
    assert!(!html.contains("<b "), "{html}");
    assert!(html.contains("&lt;script&gt;"), "{html}");
    for bad in ["javascript:alert(1)", "JaVaScRiPt:alert(1)", "java\tscript:alert(1)", "data:text/html;base64,PHNjcmlwdD4=", "vbscript:x"] {
        let html = render_markdown(&format!("[x](<{bad}>) ![y](<{bad}>)"));
        assert!(!html.to_ascii_lowercase().contains("script:") && !html.contains("data:"), "{bad}: {html}");
        assert!(html.contains("href=\"#\"") && html.contains("src=\"#\""), "{bad}: {html}");
    }
    // Images load only from this site or the lesson itself (#386); one
    // from another host is a plain link, with its alt text or its URL.
    let html = render_markdown(r"![pixel](https://tracker.example/p.gif) ![](//tracker.example/q.gif) ![x](\\\\tracker.example/r.gif) ![map](pictures/map.svg) ![dot](data:image/png;base64,iVBOR)");
    assert_eq!(html.matches("<img").count(), 2, "{html}");
    assert!(!html.contains("src=\"https:") && !html.contains("src=\"//") && !html.contains("src=\"\\"), "{html}");
    assert!(html.contains(r#"<a href="https://tracker.example/p.gif">pixel</a>"#), "{html}");
    assert!(html.contains(r#"<a href="//tracker.example/q.gif">//tracker.example/q.gif</a>"#), "{html}");
    assert!(html.contains(r#"<img src="pictures/map.svg" alt="map" />"#), "{html}");
    assert!(html.contains(r#"<img src="data:image/png;base64,iVBOR" alt="dot" />"#), "{html}");
    let html = render_markdown("[a](https://example.org/x) [b](/lessons/x) [c](mailto:a@b.c) [d](notes/a:b) [e](#top)");
    for ok in ["https://example.org/x", "/lessons/x", "mailto:a@b.c", "notes/a:b", "#top"] {
        assert!(html.contains(&format!("href=\"{ok}\"")), "{ok}: {html}");
    }
}

const SVG: &str = "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 10 10'><rect width='10' height='10'/></svg>";

#[test]
fn pictures_are_validated_and_served() {
    // An SVG under pictures/ is kept as an asset of the course; nothing else is.
    let files = vec![
        CourseFile { path: MANIFEST_FILE.into(), content: MANIFEST.into() },
        CourseFile {
            path: "lessons/l.json".into(),
            content: r#"{"id": "l", "slug": "l", "title": "L", "unit": "A1", "content": "![room](pictures/room.svg)"}"#.into(),
        },
        CourseFile { path: "pictures/room.svg".into(), content: SVG.into() },
    ];
    let s = ContentStore::from_files(&files).unwrap();
    assert_eq!(s.asset("pictures/room.svg"), Some(SVG));
    assert_eq!(s.asset("pictures/other.svg"), None);
    assert_eq!(s.asset("lessons/l.json"), None);
    assert!(is_picture_path("pictures/a/b.svg"));
    assert!(!is_picture_path("pictures/.svg"));
    assert!(!is_picture_path("pictures/b.json"));
    // The same characters as a course file path (#387).
    for bad in ["pictures/a'b.svg", "pictures/a b.svg", "pictures/a\"b.svg", "pictures/../x.svg", "pictures//x.svg", "pictures/<x>.svg", "pictures/é.svg"] {
        assert!(!is_picture_path(bad), "{bad}");
        assert!(!crate::courses::valid_path(bad), "{bad}");
    }
    assert!(is_picture_path("pictures/scene-1_a.v2.svg") && crate::courses::valid_path("pictures/scene-1_a.v2.svg"));
}

/// Every stop on the Sprint path has its own icon, in every built-in course
/// and the test course, and none looks like a locked stop (the `locked`
/// icon). Lessons without an icon get one of `DEFAULT_LESSON_ICONS` in turn
/// and are not checked.
#[test]
fn stop_icons_are_unique_on_the_path() {
    let mut dirs: Vec<std::path::PathBuf> = std::fs::read_dir("data/courses").unwrap().map(|e| e.unwrap().path()).filter(|p| p.is_dir()).collect();
    dirs.push("data/test-courses/test-course".into());
    for dir in &dirs {
        let store = ContentStore::load_dir(dir).unwrap();
        // A locked stop shows the locked icon: no lesson may look locked.
        let locked = Icons::default().locked;
        let mut seen: HashMap<&str, &str> = HashMap::from([(locked.as_str(), "the locked icon")]);
        for l in store.lessons().iter().filter(|l| !l.sprint.icon.is_empty()) {
            if let Some(other) = seen.insert(&l.sprint.icon, &l.slug) {
                panic!("{}: {} and {} both show {} on the path", dir.display(), other, l.slug, l.sprint.icon);
            }
        }
    }
    assert!(dirs.len() >= 3, "{dirs:?}");
}

#[test]
fn quiz_prompts_are_unique_in_every_builtin_course() {
    let norm = |s: &str| s.to_lowercase().split(|c: char| !c.is_alphanumeric() && c != '_').filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ");
    for dir in std::fs::read_dir("data/courses").unwrap().map(|e| e.unwrap().path()).filter(|p| p.is_dir()) {
        let store = ContentStore::load_dir(&dir).unwrap();
        let mut seen: HashMap<String, &str> = HashMap::new();
        for l in store.lessons() {
            for q in &l.quiz {
                if let Some(other) = seen.insert(norm(&q.prompt), &q.question_id) {
                    panic!("{}: {} and {} have the same prompt: {:?}", dir.display(), other, q.question_id, q.prompt);
                }
            }
        }
    }
}

/// "Listen and type" tests the ear, not memory: no listening sentence in
/// a built-in course is word for word a build, a vocabulary example or a
/// quiz prompt of the same lesson (AUTHORING.md, listening).
#[test]
fn listening_does_not_copy_builds_or_examples() {
    for dir in std::fs::read_dir("data/courses").unwrap().map(|e| e.unwrap().path()).filter(|p| p.is_dir()) {
        let store = ContentStore::load_dir(&dir).unwrap();
        for l in store.lessons() {
            let used: Vec<&str> = l.sprint.builds.iter().map(|b| b.text.as_str())
                .chain(l.vocabulary.iter().filter_map(|v| v.example.as_deref()))
                .chain(l.quiz.iter().map(|q| q.prompt.as_str()))
                .collect();
            for item in &l.listening {
                let text = item.text.trim();
                assert!(!used.iter().any(|u| u.trim() == text),
                    "{}: {}: listening {:?} copies a build, vocabulary example or quiz prompt", dir.display(), l.id, text);
            }
        }
    }
}

/// Prerequisites in every built-in course and the test course name lessons
/// that exist, at the lesson's level or below (never a later unit), and
/// that come earlier on the path, so the graph has no cycle and no lesson
/// waits on one the learner only reaches after it. The one exception is the
/// test course's first lesson, `tc-everything`: it carries every lesson
/// field, `prerequisites` included, and nothing comes before it.
#[test]
fn prerequisites_point_back_down_the_path() {
    let mut dirs: Vec<std::path::PathBuf> = std::fs::read_dir("data/courses").unwrap().map(|e| e.unwrap().path()).filter(|p| p.is_dir()).collect();
    dirs.push("data/test-courses/test-course".into());
    for dir in &dirs {
        let store = ContentStore::load_dir(dir).unwrap();
        let lessons = store.lessons();
        let at: HashMap<&str, usize> = lessons.iter().enumerate().map(|(i, l)| (l.id.as_str(), i)).collect();
        for (i, l) in lessons.iter().enumerate() {
            for p in &l.prerequisites {
                if l.id == "tc-everything" {
                    assert!(at.contains_key(p.as_str()), "{}: {} names unknown prerequisite {p}", dir.display(), l.id);
                    continue;
                }
                let Some(&j) = at.get(p.as_str()) else { panic!("{}: {} names unknown prerequisite {p}", dir.display(), l.id) };
                let pre = &lessons[j];
                assert!(pre.unit_index <= l.unit_index, "{}: {} (unit {}) needs {p} from a higher unit ({})", dir.display(), l.id, l.unit, pre.unit);
                assert!(j < i, "{}: {} needs {p}, which comes later on the path", dir.display(), l.id);
            }
        }
        // No cycle, checked on its own so the message says so even if the
        // course order rule above is ever relaxed.
        fn visit<'a>(id: &'a str, store: &'a ContentStore, state: &mut HashMap<&'a str, bool>, path: &mut Vec<&'a str>) {
            match state.get(id) {
                Some(true) => return,
                Some(false) => panic!("prerequisite cycle: {} -> {id}", path.join(" -> ")),
                None => {}
            }
            state.insert(id, false);
            path.push(id);
            for p in &store.lesson_by_id(id).unwrap().prerequisites {
                visit(p, store, state, path);
            }
            path.pop();
            state.insert(id, true);
        }
        let mut state = HashMap::new();
        for l in lessons {
            visit(&l.id, &store, &mut state, &mut Vec::new());
        }
    }
}

#[test]
fn derived_guide_step_shares_its_sections_html() {
    let mut lesson: Lesson = serde_json::from_str(
        r#"{"id":"l","slug":"l","title":"L","unit":"A1","content":"","vocabulary":[]}"#,
    )
    .unwrap();
    lesson.sections = split_sections("Intro.\n\n## One\n\nbody one\n\n## Two\n\nbody two");
    let steps = build_guide(&lesson).unwrap();
    let mut n = 0;
    for (step, sec) in steps.iter().zip(&lesson.sections) {
        let GuideStep::Concept { html, .. } = step else { panic!("expected a concept step") };
        assert!(std::sync::Arc::ptr_eq(html, &sec.html), "guide step copies the section HTML");
        n += 1;
    }
    assert_eq!(n, 3);
}
