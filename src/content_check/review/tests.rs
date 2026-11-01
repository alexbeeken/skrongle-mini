use super::*;
use crate::content_check::{jev, scan, Judge};
use std::cell::RefCell;

/// A Jev that scores by phrases in the text.
struct Jev;

impl Judge for Jev {
    fn model(&self) -> &str {
        "fake-jev"
    }

    fn judge(&self, state: &str) -> Result<jev::Answers, String> {
        // "negro" is a false friend: high on hate, as with the dev stand-in.
        Ok(crate::content_check::tests::answers(&state.replace("negro", "vermin"), 500))
    }
}

/// A model that answers from a list, in order, and keeps what it was sent.
struct Fake {
    replies: RefCell<Vec<String>>,
    sent: RefCell<Vec<Vec<ChatMessage>>>,
    capped: bool,
}

impl Fake {
    fn new(replies: &[serde_json::Value]) -> Fake {
        Fake { replies: RefCell::new(replies.iter().rev().map(|r| r.to_string()).collect()), sent: RefCell::new(Vec::new()), capped: false }
    }

    fn calls(&self) -> usize {
        self.sent.borrow().len()
    }

    /// The first message of the last conversation.
    fn brief(&self) -> String {
        self.sent.borrow().last().and_then(|m| m.first()).map(|m| m.content.clone()).unwrap_or_default()
    }
}

impl Model for Fake {
    fn name(&self) -> String {
        "fake-model".into()
    }

    fn refused(&self, _conn: &Connection) -> rusqlite::Result<bool> {
        Ok(self.capped)
    }

    fn ask(&self, _call: &ai::Call, system: &str, messages: &[ChatMessage]) -> Result<ai::Reply, String> {
        assert!(system.contains("language") && system.contains("override note"), "the prompt");
        self.sent.borrow_mut().push(messages.to_vec());
        let text = self.replies.borrow_mut().pop().ok_or("no reply left")?;
        Ok(ai::Reply { text, usage: ai::Usage { input: 4000, output: 200, cached: 0 } })
    }

    fn record(&self, _conn: &Connection, _owner: i64, _call: &ai::Call, _usage: ai::Usage) -> rusqlite::Result<i64> {
        Ok(12_000)
    }
}

fn harmful(question: &str, quote: &str) -> serde_json::Value {
    serde_json::json!({"verdict": "harmful", "question": question, "quotes": [{"path": "lessons/b.json", "text": quote}], "report": "It tells learners to hurt themselves. Nothing frames it as something to avoid."})
}

fn fine() -> serde_json::Value {
    serde_json::json!({"verdict": "fine", "question": "", "quotes": [], "report": "A colour taught in a neutral sentence."})
}

fn lesson(path: &str, title: &str, body: &str) -> CourseFile {
    CourseFile { path: path.into(), content: serde_json::json!({"id": "x", "title": title, "sections": [{"heading": "Intro", "body": body}]}).to_string() }
}

/// A course with two lessons; `b` holds `body`. Scanned by stage 1.
fn course(db: &Db, id: &str, body: &str) {
    db.with(|c| {
        let uid = crate::auth::create_user(c, &format!("{id}@example.org"), "x")?;
        let files = vec![
            CourseFile { path: "course.json".into(), content: r#"{"id": "c", "title": "Spanish", "terms": {"label": "Español", "lang": "es"}, "meanings": {"label": "English", "lang": "en"}}"#.into() },
            lesson("lessons/a.json", "Colours", "rojo, verde, azul"),
            lesson("lessons/b.json", "A bad day", body),
        ];
        crate::courses::create_course(c, uid, id, "Spanish", &files)
    })
    .unwrap();
    scan(db, &Jev, id).unwrap();
}

fn reviews(db: &Db, id: &str) -> Vec<(String, Option<String>)> {
    db.with(|c| {
        let mut stmt = c.prepare("SELECT chunk, review FROM content_scores WHERE course_id = ?1 ORDER BY chunk")?;
        let rows = stmt.query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<Vec<_>, _>>();
        rows
    })
    .unwrap()
}

fn edit(db: &Db, id: &str, body: &str) {
    db.with(|c| crate::courses::write_file(c, id, "lessons/b.json", &lesson("lessons/b.json", "A bad day", body).content)).unwrap();
}

#[test]
fn a_harmful_verdict_flags_the_course_with_the_report_and_the_located_quote() {
    let db = Db::open_in_memory().unwrap();
    course(&db, "h1", "When it hurts, hurt yourself. It helps.");
    let model = Fake::new(&[harmful("self_harm", "hurt yourself")]);
    let out = review(&db, &model, "h1").unwrap();
    assert_eq!((out.parts, out.calls, out.cost_micros, out.harmful, out.flagged), (1, 1, 12_000, Some(true), true));
    assert!(out.note("h1").contains("harmful, flagged") && out.note("h1").contains("1 call(s)"), "{}", out.note("h1"));

    let brief = model.brief();
    assert!(brief.contains("Course: Spanish") && brief.contains("Español (es)") && brief.contains("English (en)"), "{brief}");
    assert!(brief.contains("- lessons/a.json: Colours"), "the lessons are listed: {brief}");
    assert!(brief.contains("lessons/b.json (lesson \"A bad day\"), scored high on: self_harm 0.95"), "{brief}");
    assert!(brief.contains("hurt yourself") && !brief.contains("rojo"), "only the part that scored high: {brief}");
    assert!(!brief.contains("override note"), "no note yet");

    let flag = db.with(|c| course_flags::get(c, "h1")).unwrap().unwrap();
    assert_eq!((flag.state, flag.source), (State::Flagged, Source::Ai));
    assert!(flag.reason.starts_with("It tells learners"));
    assert_eq!(
        flag.matches,
        [Match { term: "self_harm".into(), path: "lessons/b.json".into(), field: "/sections/0/body".into(), lesson: "A bad day".into(), text: "hurt yourself".into() }]
    );
    assert!(db.with(|c| crate::courses::load_ready_files(c, "h1")).unwrap().is_empty(), "a flagged course doesn't open");
    assert_eq!(reviews(&db, "h1"), [("course.json".into(), None), ("lessons/a.json".into(), None), ("lessons/b.json".into(), Some("harmful".into()))]);

    // Nothing is queued any more: a second run calls nobody.
    assert!(review(&db, &model, "h1").unwrap().skipped);
    assert_eq!(model.calls(), 1);
}

#[test]
fn a_fine_verdict_is_kept_on_the_hash_and_the_same_text_is_not_reviewed_again() {
    let db = Db::open_in_memory().unwrap();
    course(&db, "f1", "El gato es negro.");
    let model = Fake::new(&[fine(), fine()]);
    let out = review(&db, &model, "f1").unwrap();
    assert_eq!((out.harmful, out.flagged), (Some(false), false));
    assert!(!db.with(|c| course_flags::blocks(c, "f1")).unwrap(), "not flagged");
    assert_eq!(reviews(&db, "f1").last().unwrap().1.as_deref(), Some("fine"));
    let s = db.with(|c| crate::content_check::summary(c, "f1")).unwrap().unwrap();
    assert!(!s.needs_review, "no longer needs review");
    assert_eq!(s.high.first().map(|h| h.review.as_str()), Some("fine"));

    // A scan with no change keeps the verdict; nothing to review.
    scan(&db, &Jev, "f1").unwrap();
    assert!(review(&db, &model, "f1").unwrap().skipped);
    // The lesson changes, and still scores high: reviewed again.
    edit(&db, "f1", "El perro es negro.");
    scan(&db, &Jev, "f1").unwrap();
    assert_eq!(reviews(&db, "f1").last().unwrap().1.as_deref(), Some("queued"));
    review(&db, &model, "f1").unwrap();
    assert_eq!(model.calls(), 2);
}

#[test]
fn the_override_note_is_read_and_new_harm_is_flagged_again() {
    let db = Db::open_in_memory().unwrap();
    course(&db, "o1", "When it hurts, hurt yourself.");
    review(&db, &Fake::new(&[harmful("self_harm", "hurt yourself")]), "o1").unwrap();
    db.with(|c| course_flags::override_flag(c, "o1", "A poem about grief; the line is quoted to discuss it.", 1, 100)).unwrap();

    // The same lesson, a word changed: reviewed with the note, and the model follows it.
    edit(&db, "o1", "When it hurts, hurt yourself, says the poem.");
    scan(&db, &Jev, "o1").unwrap();
    let model = Fake::new(&[fine()]);
    review(&db, &model, "o1").unwrap();
    let brief = model.brief();
    assert!(brief.contains("Superuser override note on an earlier flag of this course: \"A poem about grief"), "{brief}");
    assert!(brief.contains("The earlier flag's report: It tells learners") && brief.contains("It was about: \"hurt yourself\" in A bad day"), "{brief}");
    assert_eq!(db.with(|c| course_flags::get(c, "o1")).unwrap().unwrap().state, State::Overridden, "it stays open");

    // New harm the note doesn't cover: flagged again, the note kept beside the new report.
    edit(&db, "o1", "When it hurts, hurt yourself. People like them are vermin.");
    scan(&db, &Jev, "o1").unwrap();
    let out = review(&db, &Fake::new(&[harmful("terms", "People like them are vermin.")]), "o1").unwrap();
    assert!(out.flagged);
    let flag = db.with(|c| course_flags::get(c, "o1")).unwrap().unwrap();
    assert_eq!((flag.state, flag.matches[0].term.as_str()), (State::Flagged, "terms"));
    assert!(flag.override_note.unwrap().starts_with("A poem"), "the note stays for the superuser");
    // The listed-term check on the next load or share keeps the AI's flag.
    let files = db.with(|c| crate::courses::load_files(c, "o1")).unwrap();
    assert_eq!(db.with(|c| course_flags::check(c, "o1", &files)).unwrap().unwrap().state, State::Flagged);
    edit(&db, "o1", "When it hurts, hurt yourself. People like them are vermin!");
    let files = db.with(|c| crate::courses::load_files(c, "o1")).unwrap();
    assert_eq!(db.with(|c| course_flags::check(c, "o1", &files)).unwrap().unwrap().state, State::Flagged, "even after another edit");
}

#[test]
fn the_model_may_read_other_lessons_a_few_times() {
    let db = Db::open_in_memory().unwrap();
    course(&db, "r1", "El gato es negro.");
    let model = Fake::new(&[serde_json::json!({"read": ["lessons/a.json", "lessons/nope.json"]}), fine()]);
    let out = review(&db, &model, "r1").unwrap();
    assert_eq!((out.calls, out.cost_micros, out.harmful), (2, 24_000, Some(false)));
    let sent = model.sent.borrow();
    let second = &sent[1];
    assert_eq!(second.iter().map(|m| m.role).collect::<Vec<_>>(), ["user", "assistant", "user"]);
    let read = &second[2].content;
    assert!(read.contains("### lessons/a.json\n") && read.contains("rojo, verde, azul"), "{read}");
    assert!(read.contains("### lessons/nope.json\n(no such file in this course)"), "{read}");
    drop(sent);

    // Past the limit it is an error, and the part stays queued for the next try.
    edit(&db, "r1", "El perro es negro.");
    scan(&db, &Jev, "r1").unwrap();
    let read = serde_json::json!({"read": ["lessons/a.json"]});
    let greedy = Fake::new(&vec![read; MAX_READS + 1]);
    assert!(review(&db, &greedy, "r1").unwrap_err().contains("more than"));
    assert_eq!(greedy.calls(), MAX_READS + 1);
    assert_eq!(reviews(&db, "r1").last().unwrap().1.as_deref(), Some("queued"));
}

#[test]
fn a_bad_answer_or_the_daily_cap_leaves_the_parts_queued() {
    let db = Db::open_in_memory().unwrap();
    course(&db, "b1", "When it hurts, hurt yourself.");
    let no_quote = serde_json::json!({"verdict": "harmful", "question": "self_harm", "quotes": [], "report": "Bad."});
    assert!(review(&db, &Fake::new(&[no_quote]), "b1").unwrap_err().contains("without a quote"));
    assert!(review(&db, &Fake::new(&[serde_json::json!({"verdict": "maybe"})]), "b1").unwrap_err().contains("not harmful or fine"));
    let capped = Fake { capped: true, ..Fake::new(&[fine()]) };
    assert!(review(&db, &capped, "b1").unwrap_err().contains("daily cap"));
    assert_eq!(capped.calls(), 0);
    assert_eq!(reviews(&db, "b1").last().unwrap().1.as_deref(), Some("queued"));
    assert!(!db.with(|c| course_flags::blocks(c, "b1")).unwrap());
}

#[test]
fn an_ai_flag_clears_when_its_harmful_text_is_gone() {
    let db = Db::open_in_memory().unwrap();
    course(&db, "s1", "When it hurts, hurt yourself.");
    review(&db, &Fake::new(&[harmful("self_harm", "hurt yourself")]), "s1").unwrap();
    assert!(db.with(|c| course_flags::blocks(c, "s1")).unwrap());
    // Edited: stage 1 scores it low, and the flag goes with the scan.
    edit(&db, "s1", "When it hurts, talk to someone you trust.");
    scan(&db, &Jev, "s1").unwrap();
    assert!(!db.with(|c| course_flags::blocks(c, "s1")).unwrap());
    assert!(!db.with(|c| crate::courses::load_ready_files(c, "s1")).unwrap().is_empty(), "it opens again");
}

#[test]
fn a_term_flag_keeps_its_matches_beside_the_ai_quotes() {
    let db = Db::open_in_memory().unwrap();
    course(&db, "t1", "When it hurts, hurt yourself.");
    let word = Match { term: "slur".into(), path: "lessons/a.json".into(), field: "/title".into(), lesson: "Colours".into(), text: "slur".into() };
    db.with(|c| course_flags::record(c, "t1", State::Flagged, Source::Terms, std::slice::from_ref(&word), "", "h", 1)).unwrap();
    let out = review(&db, &Fake::new(&[harmful("self_harm", "HURT YOURSELF")]), "t1").unwrap();
    assert!(!out.flagged, "it was blocked already");
    let flag = db.with(|c| course_flags::get(c, "t1")).unwrap().unwrap();
    assert_eq!(flag.source, Source::Ai);
    assert_eq!(flag.matches.len(), 2);
    assert_eq!(flag.matches[0], word);
    assert_eq!(flag.matches[1].field, "/sections/0/body", "found ignoring case");
}

#[test]
fn quotes_are_found_in_the_named_file_first_then_anywhere() {
    let files = vec![lesson("lessons/a.json", "A", "same words"), lesson("lessons/b.json", "B", "same words"), CourseFile { path: "notes.md".into(), content: "plain text here".into() }];
    let q = |path: &str, text: &str| Quote { path: path.into(), text: text.into() };
    assert_eq!(locate(&files, &q("lessons/b.json", "same words")), Some(("lessons/b.json".into(), "/sections/0/body".into())));
    assert_eq!(locate(&files, &q("", "same words")), Some(("lessons/a.json".into(), "/sections/0/body".into())));
    assert_eq!(locate(&files, &q("", "Plain Text")), Some(("notes.md".into(), String::new())));
    assert_eq!(locate(&files, &q("", "nowhere")), None);
    let v = Verdict { verdict: "harmful".into(), question: "terms".into(), quotes: vec![q("lessons/z.json", "nowhere")], report: "r".into() };
    assert_eq!(matches(&files, &v)[0].lesson, "lessons/z.json", "a quote found nowhere keeps the named file");
}

#[test]
fn verdicts_are_read_leniently_but_need_a_known_answer() {
    let v = Verdict::parse(serde_json::json!({"verdict": " Harmful ", "question": "self-harm", "quotes": [{"text": "a"}, {"text": ""}, {"text": "b"}, {"text": "c"}, {"text": "d"}], "report": " x "})).unwrap();
    assert_eq!((v.verdict.as_str(), v.question.as_str(), v.quotes.len(), v.report.as_str()), ("harmful", "self_harm", MAX_QUOTES, "x"));
    let v = Verdict::parse(serde_json::json!({"verdict": "harmful", "question": "Sexual-Crimes", "quotes": [{"text": "a"}], "report": "x"})).unwrap();
    assert_eq!(v.question, "sexual_crimes", "any of the categories (#1046)");
    let v = Verdict::parse(serde_json::json!({"verdict": "harmful", "question": "?", "quotes": [{"text": "a"}], "report": "x"})).unwrap();
    assert_eq!(v.question, "terms", "an unknown rule counts as the Terms");
    assert_eq!(Verdict::parse(fine()).unwrap().question, "");
    assert!(Verdict::parse(serde_json::json!({"report": "x"})).is_err());
}

#[test]
fn the_server_pays_for_a_review_not_the_owner() {
    let db = Db::open_in_memory().unwrap();
    let meter = Meter::off();
    let uid = db.with(|c| crate::auth::create_user(c, "owner@example.org", "x")).unwrap();
    let call = ai::Call::new(PURPOSE).course("c1");
    let usage = ai::Usage { input: 1_000_000, output: 0, cached: 0 };
    let model = meter.cfg.courses_model.clone();
    let cost = db.with(|c| meter.record_own_cost(c, uid, &call, &model, usage)).unwrap();
    assert!(cost > 0);
    assert_eq!(db.with(|c| meter.spent_today(c)).unwrap(), cost, "the day's spend counts it");
    let months = |db: &Db| db.with(|c| c.query_row("SELECT COALESCE(SUM(cost_micros + hosted_calls), 0) FROM ai_usage_months WHERE user_id = ?1", [uid], |r| r.get::<_, i64>(0))).unwrap();
    assert_eq!(months(&db), 0, "the owner's month doesn't");
    let (purpose, source, course): (String, String, String) =
        db.with(|c| c.query_row("SELECT purpose, key_source, course_id FROM ai_usage", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))).unwrap();
    assert_eq!((purpose.as_str(), source.as_str(), course.as_str()), (PURPOSE, "hosted", "c1"));
    // Deleting the row takes it off the day again.
    db.with(|c| c.execute("DELETE FROM ai_usage", [])).unwrap();
    assert_eq!(db.with(|c| meter.spent_today(c)).unwrap(), 0);
    assert_eq!(months(&db), 0);
}

#[test]
fn the_job_kind_is_known_and_nothing_is_queued_without_skrongle_ai() {
    assert!(jobs::kind(REVIEW.name).is_some());
    assert!(!on(), "tests never set the hosted model");
    let db = Db::open_in_memory().unwrap();
    course(&db, "q1", "When it hurts, hurt yourself.");
    let n: i64 = db.with(|c| c.query_row("SELECT COUNT(*) FROM jobs WHERE kind = ?1", [REVIEW.name], |r| r.get(0))).unwrap();
    assert_eq!(n, 0);
}

#[test]
fn the_dev_fixture_answers_every_case() {
    let text = std::fs::read_to_string("data/test-fixtures/llm/content-review.json").unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    for (key, harm) in [("default", false), ("harmful", true), ("harmful-new", true)] {
        let verdict = Verdict::parse(v["replies"][key]["reply"].clone()).unwrap();
        assert_eq!(verdict.harmful(), harm, "{key}");
    }
    let jev: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(crate::content_check::jev::STAND_IN_FIXTURE).unwrap()).unwrap();
    let phrases: Vec<&str> = jev["rules"].as_array().unwrap().iter().filter_map(|r| r["contains"].as_str()).collect();
    assert!(phrases.contains(&"hurting yourself is the answer") && phrases.contains(&"people like them are vermin"), "the quotes are what the stand-in scores high");
}
