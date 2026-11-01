use super::*;

pub const MANIFEST: &str = r#"{
        "id": "t", "title": "Test", "terms": {"label": "Español", "lang": "es"}, "meanings": {"label": "English", "lang": "en"},
        "units": [{"id": "A1", "title": "A1", "description": "Beginner"}, {"id": "A2", "title": "A2"}],
        "groups": [{"id": "Grammar", "title": "Grammar"}],
        "paradigm": {"slots": [{"key": "yo", "label": "yo"}, {"key": "tu", "label": "tú"}], "sets": [{"key": "present", "label": "Present"}]}
    }"#;

/// A course that is not about a language (terms and meanings both English), with a Rustwake block:
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
        "sprint": {"spots": [{"sentence": "A bowline tightens under load.", "wrong": "tightens", "fix": "holds", "why": "It is a fixed loop."}]},
        "themes": {"rustwake": {
            "vocabulary": [{"term": "bight", "example": "Tie a bight around the scrap pile."},
                           {"term": "dome rope", "meaning": "the rope every dome keeps by the door"}],
            "quiz": [{"question_id": "k-bowline-rustwake-01", "prompt": "Which knot hauls a scavenger out of the crater?", "type": "multiple-choice",
                      "options": ["a bowline", "a slip knot"], "correct_answer": "a bowline"}],
            "spots": [{"sentence": "A bowline slips when the scavenger pulls.", "wrong": "slips", "fix": "holds", "why": "It never slips."}]
        }}
    }"###;

#[test]
fn a_renamed_theme_id_still_reads_as_the_new_one() {
    let old = PLAIN_LESSON.replace(r#""themes": {"rustwake""#, r#""themes": {"fallout""#);
    assert_ne!(old, PLAIN_LESSON);
    let files = [
        CourseFile { path: MANIFEST_FILE.into(), content: PLAIN_MANIFEST.into() },
        CourseFile { path: "lessons/bowline.json".into(), content: old },
    ];
    let store = ContentStore::from_files(&files).unwrap();
    assert!(store.lessons()[0].has_theme("rustwake") && !store.lessons()[0].has_theme("fallout"));
}

pub fn plain_store(theme: Option<&Theme>) -> ContentStore {
    let files = [
        CourseFile { path: MANIFEST_FILE.into(), content: PLAIN_MANIFEST.into() },
        CourseFile { path: "lessons/bowline.json".into(), content: PLAIN_LESSON.into() },
    ];
    ContentStore::from_files_themed(&files, theme).expect("plain course loads")
}

pub fn store(lessons: &[&str]) -> Result<ContentStore, ContentError> {
    let mut files = vec![CourseFile { path: MANIFEST_FILE.into(), content: MANIFEST.into() }];
    for (i, l) in lessons.iter().enumerate() {
        files.push(CourseFile { path: format!("lessons/l{i}.json"), content: (*l).to_string() });
    }
    ContentStore::from_files(&files)
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
    for c in courses.builtin() {
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
fn a_renamed_theme_id_still_loads_old_blocks() {
    let s = store(&[r###"{"id":"l","slug":"l","title":"L","level":"A1","category":"Grammar",
            "markdown_content":"## One\n\ntext","target_grammar_point":"x",
            "themes":{"stardew":{"icon":"🌻"}}}"###]).unwrap();
    let l = &s.lessons()[0];
    assert!(l.has_theme("meadowbrook") && !l.themes.contains_key("stardew"));
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
        themed: false,
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

/// A hand-ordered guide cannot name a theme's questions (they exist only
/// while the theme is worn), so the themed questions in front of the
/// merged quiz are slotted in after the first card, in order.
#[test]
fn hand_ordered_guide_gets_the_themed_questions_first() {
    let s = store(&[r###"{"id":"l","slug":"l","title":"L","unit":"A1","content":"## One\n\nx\n\n## Two\n\ny",
            "vocabulary":[{"term":"a","meaning":"a"},{"term":"b","meaning":"b"}],
            "quiz":[{"question_id":"q1","prompt":"p","type":"true-false","correct_answer":"true"}],
            "guide":[{"section":"One"},{"vocab":2},{"question":"q1"},{"section":"Two"}],
            "themes":{"realm":{"quiz":[{"question_id":"q-realm-1","prompt":"p","type":"true-false","correct_answer":"true"},
                                      {"question_id":"q-realm-2","prompt":"p","type":"true-false","correct_answer":"false"}]}}}"###])
    .unwrap();
    let plain = s.lesson_by_slug("l").unwrap();
    let qs = |l: &Lesson| build_guide(l).unwrap().into_iter().filter_map(|st| match st { GuideStep::Question { index } => Some(l.quiz[index].question_id.clone()), _ => None }).collect::<Vec<_>>();
    assert_eq!(qs(plain), vec!["q1"]);
    let worn = apply_theme(plain, "realm").unwrap().expect("the lesson has a realm block");
    assert_eq!(qs(&worn), vec!["q-realm-1", "q-realm-2", "q1"]);
    let steps = build_guide(&worn).unwrap();
    assert!(matches!(steps[0], GuideStep::Concept { .. }), "the first card stays first");
    assert!(matches!(steps[1], GuideStep::Question { .. }));
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

const THEME: &str = r####"{"id": "realm", "title": "Realm", "icon": "⚔️", "colors": {"accent": "#b07d1f", "bg": "#0f0c16"},
        "dark": {"surface": "#1b1626"}, "font": "Georgia, serif", "strings": {"praise": ["Victory!"]},
        "avatars": [{"id": "noor", "label": "Wood elf", "image": "https://x.test/wood-elf.svg"}],
        "icons": {"streak": "⚔️", "mode_sprint": "🗡️"}, "lesson_icons": ["🛡️", "🏹"]}"####;

const THEMED_LESSON: &str = r####"{"id":"l","slug":"l","title":"L","unit":"A1","content":"## One\n\ntext",
        "vocabulary":[{"term":"ser","meaning":"to be","example":"Soy de Madrid.","example_meaning":"I am from Madrid."},{"term":"el parque","meaning":"park"}],
        "quiz":[{"question_id":"l-01","prompt":"p","type":"fill-in-blank","correct_answer":"soy"}],
        "listening":[{"text":"Soy de Madrid.","meaning":"I am from Madrid."}],
        "sprint":{"bites":[{"title":"Plain","rule":"r"}],"builds":[{"text":"Soy de Madrid.","meaning":"I am from Madrid."}]},
        "themes":{"realm":{
            "icon":"🧤",
            "content":"## En Eldmark\n\nmore",
            "vocabulary":[{"term":"ser","example":"Soy un orco.","example_meaning":"I am an orc."},{"term":"el orco","meaning":"orc","example":"El orco es alto."}],
            "quiz":[{"question_id":"l-realm-01","prompt":"Soy ___ orco.","type":"multiple-choice","options":["un","una"],"correct_answer":"un","teaches":["el orco"]}],
            "listening":[{"text":"Soy un orco.","meaning":"I am an orc."}],
            "bites":[{"title":"Themed","rule":"r"}],
            "builds":[{"text":"Soy un orco.","meaning":"I am an orc."}]
        }}}"####;

pub fn theme(json: &str) -> Theme {
    let t: Theme = serde_json::from_str(json).unwrap();
    validate_theme(&t).unwrap();
    t
}

fn themed_store(lesson: &str, theme: Option<&Theme>) -> Result<ContentStore, ContentError> {
    ContentStore::from_files_themed(
        &[
            CourseFile { path: MANIFEST_FILE.into(), content: MANIFEST.into() },
            CourseFile { path: "lessons/l.json".into(), content: lesson.into() },
        ],
        theme,
    )
}

#[test]
fn themes_merge_in_front_of_the_lesson() {
    // Plain: nothing themed, the lesson is exactly what the file says.
    let plain = themed_store(THEMED_LESSON, None).unwrap();
    assert!(plain.theme().is_none());
    let l = &plain.lessons()[0];
    assert_eq!(l.vocabulary.len(), 2);
    assert_eq!(l.vocabulary[0].example.as_deref(), Some("Soy de Madrid."));
    assert_eq!(l.vocabulary[0].example_meaning.as_deref(), Some("I am from Madrid."));
    assert_eq!(l.quiz.len(), 1);
    assert_eq!(l.listening.len(), 1);
    assert_eq!(l.sections.len(), 1);
    assert!(l.has_theme("realm") && !l.has_theme("other"));
    assert!(plain.question("l-realm-01").is_none());

    // Themed: the theme's material first, the lesson's own kept, strings swapped.
    let t = theme(THEME);
    let realm = themed_store(THEMED_LESSON, Some(&t)).unwrap();
    assert_eq!(realm.theme().map(|t| t.id.as_str()), Some("realm"));
    let v = &realm.lessons()[0].vocabulary[0];
    assert_eq!(v.example.as_deref(), Some("Soy un orco."));
    assert_eq!(v.example_meaning.as_deref(), Some("I am an orc."));
    let l = &realm.lessons()[0];
    assert_eq!(l.vocabulary.len(), 3);
    assert_eq!(l.vocabulary[0].example.as_deref(), Some("Soy un orco."));
    assert!(l.vocabulary[0].themed && !l.vocabulary[1].themed && l.vocabulary[2].themed);
    assert_eq!(l.vocabulary[2].term, "el orco");
    assert_eq!(l.quiz.len(), 2);
    assert!(l.quiz[0].themed && l.quiz[0].question_id == "l-realm-01" && !l.quiz[1].themed);
    assert_eq!(l.listening[0].text, "Soy un orco.");
    assert!(l.listening[0].themed && !l.listening[1].themed);
    assert_eq!(l.sprint.bites[0].title, "Themed");
    assert_eq!(l.sprint.builds.len(), 2);
    assert_eq!(l.sections.len(), 2);
    assert_eq!(l.sections[1].heading, "En Eldmark");
    assert!(realm.question("l-realm-01").is_some() && realm.question("l-01").is_some());
    assert_eq!(l.sprint.icon, "🧤");
    assert!(plain.lessons()[0].sprint.icon.is_empty());
    let icons = realm.theme().unwrap().icons();
    assert_eq!((icons.streak.as_str(), icons.mode_sprint.as_str(), icons.xp.as_str()), ("⚔️", "🗡️", "⚡"));
    assert_eq!(Icons::default().streak, "🔥");
    assert_eq!(Icons::lesson(realm.theme(), 3), "🏹");
    assert_eq!(Icons::lesson(None, 0), "📘");
    assert_eq!(realm.manifest().strings.praise, vec!["Victory!"]);
    assert_eq!(realm.manifest().strings.almost, default_almost(), "a string the theme leaves out keeps the course's");
    let css = realm.theme().unwrap().css();
    assert!(css.contains("--accent:#b07d1f;") && css.contains("--font:Georgia, serif;"), "{css}");
    assert!(css.contains("@media (prefers-color-scheme: dark){:root{--surface:#1b1626;}"), "{css}");
    assert!(format!("{realm:?}").contains("theme realm"));

    // A theme no lesson has material for still skins the course.
    let other = theme(&THEME.replace("\"realm\"", "\"other\""));
    let o = themed_store(THEMED_LESSON, Some(&other)).unwrap();
    assert_eq!(o.lessons()[0].quiz.len(), 1);
    assert_eq!(o.manifest().strings.praise, vec!["Victory!"]);
}

#[test]
fn course_strings_for_a_theme_come_first() {
    // The theme's lines are English; the course's own lines for that
    // theme (in its language) replace them while it is worn.
    let manifest = MANIFEST.replacen(
        "\"units\"",
        r#""strings": {"praise": ["¡Bien!"]}, "themes": {"realm": {"strings": {"almost": ["Casi, aventurero"]}}, "gone": {"strings": {"praise": ["x"]}}}, "units""#,
        1,
    );
    let load = |t: Option<&Theme>| {
        let files = [
            CourseFile { path: MANIFEST_FILE.into(), content: manifest.clone() },
            CourseFile { path: "lessons/l.json".into(), content: THEMED_LESSON.into() },
        ];
        ContentStore::from_files_themed(&files, t)
    };
    let realm = theme(THEME);
    let worn = load(Some(&realm)).unwrap();
    assert_eq!(worn.manifest().strings.praise, vec!["Victory!"], "no course line for it: the theme's");
    assert_eq!(worn.manifest().strings.almost, vec!["Casi, aventurero"], "the course's line for the theme wins");
    let other = load(Some(&theme(&THEME.replace("\"realm\"", "\"other\"")))).unwrap();
    assert_eq!(other.manifest().strings.almost, default_almost());
    let plain = load(None).unwrap();
    assert_eq!((plain.manifest().strings.praise.clone(), plain.manifest().strings.almost.clone()), (vec!["¡Bien!".to_string()], default_almost()));

    let empty = manifest.replace(r#"["Casi, aventurero"]"#, r#"[" "]"#);
    let files = [CourseFile { path: MANIFEST_FILE.into(), content: empty }];
    let err = ContentStore::from_files(&files).unwrap_err().to_string();
    assert!(err.contains("course.json") && err.contains("themes.realm.strings must not be empty"), "{err}");
}

#[test]
fn themed_material_is_validated() {
    let err = |l: &str| themed_store(l, None).unwrap_err().to_string();
    // themed material is validated even when no theme is on, and the error names the theme
    let bad = THEMED_LESSON.replace(r#""text":"Soy un orco.","meaning":"I am an orc."}],"#, r#""text":"Soy un orco.","meaning":"I am an orc."},{"text":"Soy un orco.","meaning":"dup"}],"#);
    let e = err(&bad);
    assert!(e.contains("(theme 'realm')") && e.contains("appears twice"), "{e}");
    // a re-examples entry must not change the meaning; a new term needs one
    let bad = THEMED_LESSON.replace(r#"{"term":"ser","example":"Soy un orco.","#, r#"{"term":"ser","meaning":"to exist","example":"Soy un orco.","#);
    assert!(err(&bad).contains("already taught by the lesson"));
    let bad = THEMED_LESSON.replace(r#"{"term":"el orco","meaning":"orc","example":"El orco es alto."}"#, r#"{"term":"el orco"}"#);
    assert!(err(&bad).contains("needs a meaning"));
    // a themed question id may not reuse a lesson's
    let bad = THEMED_LESSON.replace(r#""question_id":"l-realm-01""#, r#""question_id":"l-01""#);
    let e = err(&bad);
    assert!(e.contains("l-01") && e.contains("theme 'realm'"), "{e}");
    // the key must look like a theme id
    let e = err(&THEMED_LESSON.replace(r#""themes":{"realm""#, r#""themes":{"Bad Id""#));
    assert!(e.contains("not a theme id"), "{e}");
    // a themed lesson icon is a glyph
    let e = err(&THEMED_LESSON.replace(r#""icon":"🧤""#, r#""icon":"<script>""#));
    assert!(e.contains("themes.realm: icon"), "{e}");
}

#[test]
fn old_icon_slots_still_load() {
    let t: Theme = serde_json::from_str(r##"{"id": "a", "title": "x", "icons": {"mode_d": "🗡️", "mode_k": "📜", "mode_play": "🎯", "mode_g": "🎲"}}"##).unwrap();
    validate_theme(&t).unwrap();
    let icons = Icons::of(Some(&t));
    assert_eq!((icons.mode_sprint.as_str(), icons.mode_study.as_str(), icons.mode_play.as_str()), ("🗡️", "📜", "🎯"));
}

#[test]
fn theme_files_are_validated() {
    let with = |json: &str| validate_theme(&serde_json::from_str(json).unwrap()).unwrap_err();
    assert!(with(r##"{"id": "Bad Id", "title": "x"}"##).contains("theme id"));
    assert!(with(r##"{"id": "a", "title": ""}"##).contains("needs a title"));
    assert!(with(r##"{"id": "a", "title": "x", "colors": {"accent": "red"}}"##).contains("hex colour"));
    assert!(with(r##"{"id": "a", "title": "x", "colors": {"accent": "#fff; background: url(x)"}}"##).contains("hex colour"));
    assert!(with(r##"{"id": "a", "title": "x", "dark": {"shadow": "#000"}}"##).contains("unknown dark token 'shadow'"));
    assert!(with(r##"{"id": "a", "title": "x", "font": "x; color: red"}"##).contains("font must be"));
    assert!(with(r##"{"id": "a", "title": "x", "art": {"banner": "lessons/x.svg"}}"##).contains("must be a file of the theme"));
    assert!(with(r##"{"id": "a", "title": "x", "strings": {"praise": [""]}}"##).contains("strings must not be empty"));
    assert!(with(r##"{"id": "a", "title": "x", "icons": {"hand": "🧤"}}"##).contains("unknown icon slot 'hand'"));
    assert!(with(r##"{"id": "a", "title": "x", "icons": {"streak": "<b>"}}"##).contains("icons.streak must be"));
    assert!(with(r##"{"id": "a", "title": "x", "lesson_icons": ["🛡️", "a long one"]}"##).contains("lesson_icons must be"));
    assert!(is_icon("🧤") && is_icon("★") && !is_icon("") && !is_icon("🧤 🧤"));
    assert!(with(r##"{"id": "a", "title": "x", "avatars": [{"id": "orc", "label": "Orc", "image": "pictures/o.svg"}]}"##).contains("avatar 'orc' is not one of the profile pictures (noor, amani"));
    assert!(with(r##"{"id": "a", "title": "x", "avatars": [{"id": "noor", "label": "Orc", "image": "o.svg"}]}"##).contains("must be a file of the theme"));
    assert!(with(r##"{"id": "a", "title": "x", "avatars": [{"id": "noor", "image": "https://x.test/o.svg"}, {"id": "owl", "image": "https://x.test/o.svg"}]}"##).contains("duplicate avatar 'owl'"));
    assert!(with(r##"{"id": "a", "title": "x", "avatars": [{"id": "noor", "image": "https://x.test/o.svg", "color": "blue"}]}"##).contains("avatar 'noor': color must be a hex colour"));
    assert!(with(r##"{"id": "a", "title": "x", "avatar_colors": [{"slot": 9, "label": "Ash", "color": "#333333"}]}"##).contains("slot 9 must be 1 to 8"));
    assert!(with(r##"{"id": "a", "title": "x", "avatar_colors": [{"slot": 2, "label": "Ash", "color": "#333"}, {"slot": 2, "label": "Ash", "color": "#444"}]}"##).contains("duplicate avatar_colors slot 2"));
    assert!(with(r##"{"id": "a", "title": "x", "avatar_colors": [{"slot": 2, "label": " ", "color": "#333"}]}"##).contains("needs a label"));
    assert!(with(r##"{"id": "a", "title": "x", "avatar_colors": [{"slot": 2, "label": "Ash", "color": "grey"}]}"##).contains("slot 2: color must be a hex colour"));
    let bg = |slot: u8, label: &str, image: &str, color: &str| format!(r##"{{"slot": {slot}, "label": "{label}", "image": "{image}", "color": "{color}"}}"##);
    let bgs = |list: &[String]| with(&format!(r##"{{"id": "a", "title": "x", "avatar_backgrounds": [{}]}}"##, list.join(", ")));
    assert!(bgs(&[bg(8, "Hills", "pictures/h.svg", "#334455")]).contains("avatar_backgrounds slot 8 must be 9 to 16"));
    assert!(bgs(&[bg(17, "Hills", "pictures/h.svg", "#334455")]).contains("slot 17 must be 9 to 16"));
    assert!(bgs(&[bg(9, "Hills", "pictures/h.svg", "#334455"), bg(9, "Dunes", "pictures/d.svg", "#334455")]).contains("duplicate avatar_backgrounds slot 9"));
    assert!(bgs(&[bg(9, " ", "pictures/h.svg", "#334455")]).contains("needs a label"));
    assert!(bgs(&[bg(9, "Hills", "h.png", "#334455")]).contains("slot 9: image 'h.png' must be a file of the theme"));
    assert!(bgs(&[bg(9, "Hills", "pictures/h.svg", "green")]).contains("slot 9: color must be a hex colour"));
    assert!(is_hex_color("#abc") && is_hex_color("#AABBCCDD") && !is_hex_color("#abcde") && !is_hex_color("abc"));
    assert!(is_font_stack("'Palatino Linotype', Georgia, serif") && !is_font_stack("url(x)"));
    let t = theme(THEME);
    // By the new id, or by the old id of the same slot; never another slot.
    assert!(t.avatar("noor").is_some() && t.avatar("owl").is_some() && t.avatar("kai").is_none() && t.avatar("fox").is_none());
    let old = theme(&THEME.replace(r#""id": "noor""#, r#""id": "owl""#));
    assert_eq!(old.avatar("noor").map(|a| a.label.as_str()), Some("Wood elf"));
    assert_eq!(t.icon(), "⚔️");
}

#[test]
fn old_theme_id_is_read_as_the_new_one() {
    let old = THEMED_LESSON.replace(r#""themes":{"realm""#, r#""themes":{"wow""#);
    let t = theme(THEME);
    let store = themed_store(&old, Some(&t)).unwrap();
    assert!(store.lessons()[0].has_theme("realm") && !store.lessons()[0].has_theme("wow"));
}

/// Every stop on the Sprint path has its own icon, plain and with each
/// theme worn (a themed `icon` replaces the stop's `sprint.icon`), in every
/// built-in course and the test course, and none looks like a locked stop
/// (the theme's `locked` icon). Lessons without an icon get one from the
/// theme's `lesson_icons` in turn and are not checked.
#[test]
fn stop_icons_are_unique_on_the_path_in_every_theme() {
    let themes = crate::themes::Themes::load(Path::new("data")).unwrap();
    let mut dirs: Vec<std::path::PathBuf> = std::fs::read_dir("data/courses").unwrap().map(|e| e.unwrap().path()).filter(|p| p.is_dir()).collect();
    dirs.push("data/test-courses/test-course".into());
    let mut checked = 0;
    for dir in &dirs {
        let packs: Vec<Option<&Theme>> = std::iter::once(None).chain(themes.all().iter().map(|p| Some(&p.theme))).collect();
        for theme in packs {
            let store = ContentStore::load_dir_themed(dir, theme).unwrap();
            // A locked stop shows the theme's locked icon: no lesson may look locked.
            let locked = Icons::of(theme).locked;
            let mut seen: HashMap<&str, &str> = HashMap::from([(locked.as_str(), "the locked icon")]);
            for l in store.lessons().iter().filter(|l| !l.sprint.icon.is_empty()) {
                if let Some(other) = seen.insert(&l.sprint.icon, &l.slug) {
                    panic!("{}, theme {}: {} and {} both show {} on the path", dir.display(), theme.map_or("none", |t| t.id.as_str()), other, l.slug, l.sprint.icon);
                }
            }
            checked += 1;
        }
    }
    assert!(checked >= 2 * 4, "{checked}");
}

/// No two quiz questions in a built-in course share a prompt (compared
/// lower-cased, punctuation aside), themed questions included: the Review
/// mix, Practice and the Guide would ask the same thing twice, and a
/// generic "Which sentence is correct?" says nothing in the History table.
#[test]
fn quiz_prompts_are_unique_in_every_builtin_course() {
    let norm = |s: &str| s.to_lowercase().split(|c: char| !c.is_alphanumeric() && c != '_').filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ");
    for dir in std::fs::read_dir("data/courses").unwrap().map(|e| e.unwrap().path()).filter(|p| p.is_dir()) {
        let store = ContentStore::load_dir(&dir).unwrap();
        let mut seen: HashMap<String, &str> = HashMap::new();
        for l in store.lessons() {
            for q in l.quiz.iter().chain(l.themes.values().flat_map(|t| t.quiz.iter())) {
                if let Some(other) = seen.insert(norm(&q.prompt), &q.question_id) {
                    panic!("{}: {} and {} have the same prompt: {:?}", dir.display(), other, q.question_id, q.prompt);
                }
            }
        }
    }
}

/// "Listen and type" tests the ear, not memory: no listening sentence in
/// a built-in course is word for word a build, a vocabulary example or a
/// quiz prompt of the same lesson or theme block (AUTHORING.md, listening).
#[test]
fn listening_does_not_copy_builds_or_examples() {
    for dir in std::fs::read_dir("data/courses").unwrap().map(|e| e.unwrap().path()).filter(|p| p.is_dir()) {
        let store = ContentStore::load_dir(&dir).unwrap();
        for l in store.lessons() {
            let lesson_used: Vec<&str> = l.sprint.builds.iter().map(|b| b.text.as_str())
                .chain(l.vocabulary.iter().filter_map(|v| v.example.as_deref()))
                .chain(l.quiz.iter().map(|q| q.prompt.as_str()))
                .collect();
            let mut blocks: Vec<(String, &[ListeningItem], Vec<&str>)> = vec![("lesson".into(), &l.listening[..], lesson_used)];
            for (id, t) in &l.themes {
                let used = t.builds.iter().map(|b| b.text.as_str())
                    .chain(t.vocabulary.iter().filter_map(|v| v.example.as_deref()))
                    .chain(t.quiz.iter().map(|q| q.prompt.as_str()))
                    .collect();
                blocks.push((format!("themes.{id}"), &t.listening[..], used));
            }
            for (where_, listening, used) in blocks {
                for item in listening {
                    let text = item.text.trim();
                    assert!(!used.iter().any(|u| u.trim() == text),
                        "{}: {} {}: listening {:?} copies a build, vocabulary example or quiz prompt", dir.display(), l.id, where_, text);
                }
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
fn builtin_spanish_has_realm_material() {
    let themes = crate::themes::Themes::load(Path::new("data")).unwrap();
    let realm_theme = themes.get("realm").expect("the realm theme ships with the app");
    let plain = ContentStore::load_dir(Path::new("data/courses/spanish")).unwrap();
    assert!(plain.lessons().iter().all(|l| !l.listening.iter().any(|i| i.themed)));
    let themed_lessons = plain.lessons().iter().filter(|l| l.has_theme("realm")).count();
    assert!(themed_lessons >= 8, "{themed_lessons}");

    let realm = ContentStore::load_dir_themed(Path::new("data/courses/spanish"), Some(&realm_theme.theme)).unwrap();
    assert_eq!(realm.lessons().len(), plain.lessons().len());
    for (p, w) in plain.lessons().iter().zip(realm.lessons()) {
        assert_eq!(p.id, w.id);
        assert!(w.listening.len() >= p.listening.len() && w.quiz.len() >= p.quiz.len());
        if w.has_theme("realm") {
            assert!(w.listening[0].themed && w.sprint.bites[0].themed && w.quiz[0].themed, "{}", w.slug);
            assert!(w.vocabulary.iter().any(|v| v.themed), "{}", w.slug);
        } else {
            assert_eq!(w.listening.len(), p.listening.len());
        }
    }
    let ser = realm.lesson_by_slug("ser-vs-estar").unwrap();
    assert_eq!(ser.vocabulary[0].example.as_deref(), Some("Soy un guerrero de la Hueste de Ceniza."));
    assert!(realm.question("a1-ser-vs-estar-realm-01").is_some());
    assert_eq!(realm.manifest().strings.praise[0], "¡Victoria!");

    // Meadowbrook: the same lessons, every kind of material, including conversations.
    let sd_theme = themes.get("meadowbrook").unwrap();
    let sd = ContentStore::load_dir_themed(Path::new("data/courses/spanish"), Some(&sd_theme.theme)).unwrap();
    let themed: Vec<&Lesson> = sd.lessons().iter().filter(|l| l.has_theme("meadowbrook")).collect();
    assert!(themed.len() >= 8, "{}", themed.len());
    for l in &themed {
        let b = &l.themes["meadowbrook"];
        assert!(!b.icon.is_empty() && !b.content.is_empty() && b.vocabulary.len() >= 8 && b.quiz.len() == 2, "{}", l.slug);
        assert!(b.listening.len() == 5 && b.builds.len() == 2 && b.spots.len() == 2 && b.sorts.len() == 1 && b.bites.len() == 1, "{}", l.slug);
        assert!(l.sprint.icon == b.icon && l.quiz[0].themed && l.listening[0].themed && l.sprint.sorts[0].themed, "{}", l.slug);
    }
    assert!(themed.iter().filter(|l| !l.themes["meadowbrook"].conversations.is_empty()).count() >= 2);
    let (lesson, c) = sd.conversation("a1-gus-conv-meadowbrook-gift").unwrap();
    assert!(c.themed && lesson.slug == "gustar-saying-what-you-like" && lesson.conversations[0].id == c.id);
    assert!(plain.conversation("a1-gus-conv-meadowbrook-gift").is_none());
    assert_eq!(sd.manifest().strings.praise[0], "¡Buena cosecha!");
    // Rustwake: the same lessons again, every kind of material, with conversations in three.
    let fo_theme = themes.get("rustwake").unwrap();
    let fo = ContentStore::load_dir_themed(Path::new("data/courses/spanish"), Some(&fo_theme.theme)).unwrap();
    let themed: Vec<&Lesson> = fo.lessons().iter().filter(|l| l.has_theme("rustwake")).collect();
    assert!(themed.len() >= 8, "{}", themed.len());
    for l in &themed {
        let b = &l.themes["rustwake"];
        assert!(!b.icon.is_empty() && !b.content.is_empty() && b.vocabulary.len() >= 8 && b.quiz.len() == 2, "{}", l.slug);
        assert!(b.listening.len() == 5 && b.builds.len() == 2 && b.spots.len() == 2 && b.sorts.len() == 1 && b.bites.len() == 1, "{}", l.slug);
        assert!(l.sprint.icon == b.icon && l.quiz[0].themed && l.listening[0].themed && l.sprint.sorts[0].themed, "{}", l.slug);
        assert!(b.vocabulary.iter().any(|v| !v.meaning.is_empty()), "{}: no new wasteland term", l.slug);
    }
    assert!(themed.iter().filter(|l| !l.themes["rustwake"].conversations.is_empty()).count() >= 3);
    let (lesson, c) = fo.conversation("a1-greetings-conv-rustwake-market").unwrap();
    assert!(c.themed && lesson.slug == "greetings-and-introductions" && lesson.conversations[0].id == c.id);
    assert!(plain.conversation("a1-greetings-conv-rustwake-market").is_none() && sd.conversation("a1-greetings-conv-rustwake-market").is_none());
    assert_eq!(fo.manifest().strings.praise[0], "¡Tuercas para ti!");
    // A non-language course can carry themed material too: no listening, the rest merged the same way.
    let lm = plain_store(Some(&fo_theme.theme));
    let l = &lm.lessons()[0];
    assert!(l.has_theme("rustwake") && l.themes["rustwake"].listening.is_empty());
    assert!(l.quiz[0].themed && l.sprint.spots[0].themed && l.vocabulary.iter().any(|v| v.themed) && l.vocabulary.len() == 3);
    assert!(lm.question("k-bowline-rustwake-01").is_some());
    assert!(plain_store(None).question("k-bowline-rustwake-01").is_none());
    // and the themes never leak into each other
    assert!(sd.question("a1-ser-vs-estar-realm-01").is_none() && realm.question("a1-ser-vs-estar-meadowbrook-01").is_none());
    assert!(realm.question("a1-ser-vs-estar-rustwake-01").is_none() && fo.question("a1-ser-vs-estar-meadowbrook-01").is_none());
}
