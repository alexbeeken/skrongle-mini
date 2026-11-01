use super::*;
use crate::ai::{self, ChatMessage};
use crate::content::CourseFile;
use crate::content_check::jev;
use std::cell::{Cell, RefCell};
use std::sync::Arc;

const H: i64 = 3600;

/// Seconds from 1970 of a UTC date and hour.
fn utc(y: i64, m: i64, d: i64, h: i64) -> i64 {
    zone::days_from_civil(y, m, d) * 86_400 + h * H
}

#[test]
fn a_rule_with_summer_time_changes_on_its_days() {
    let pacific = Zone::parse("PST8PDT,M3.2.0,M11.1.0").unwrap();
    // 2026: summer time from Sunday 8 March, 2:00 PST, to Sunday 1 November, 2:00 PDT.
    assert_eq!(pacific.offset(utc(2026, 1, 15, 12)), -8 * H);
    assert_eq!(pacific.offset(utc(2026, 3, 8, 10) - 1), -8 * H);
    assert_eq!(pacific.offset(utc(2026, 3, 8, 10)), -7 * H);
    assert_eq!(pacific.offset(utc(2026, 11, 1, 9) - 1), -7 * H);
    assert_eq!(pacific.offset(utc(2026, 11, 1, 9)), -8 * H);
    // Midnight is 8:00 UTC in winter, 7:00 in summer, and right on the days it changes.
    assert_eq!(pacific.next_midnight(utc(2026, 1, 15, 12)), utc(2026, 1, 16, 8));
    assert_eq!(pacific.next_midnight(utc(2026, 7, 15, 12)), utc(2026, 7, 16, 7));
    assert_eq!(pacific.next_midnight(utc(2026, 3, 7, 20)), utc(2026, 3, 8, 8));
    assert_eq!(pacific.next_midnight(utc(2026, 3, 8, 12)), utc(2026, 3, 9, 7));
    assert_eq!(pacific.next_midnight(utc(2026, 10, 31, 12)), utc(2026, 11, 1, 7));
    assert_eq!(pacific.next_midnight(utc(2026, 11, 1, 12)), utc(2026, 11, 2, 8));
    // At midnight exactly, the next one is a day later.
    assert_eq!(pacific.next_midnight(utc(2026, 1, 16, 8)), utc(2026, 1, 17, 8));
}

#[test]
fn other_rules_read() {
    assert_eq!(Zone::parse("UTC0").unwrap().next_midnight(utc(2026, 5, 1, 13)), utc(2026, 5, 2, 0));
    assert_eq!(Zone::parse("MST7").unwrap().next_midnight(utc(2026, 12, 1, 13)), utc(2026, 12, 2, 7));
    // The south: summer time across the new year, a change at 3:00.
    let sydney = Zone::parse("AEST-10AEDT,M10.1.0,M4.1.0/3").unwrap();
    assert_eq!(sydney.offset(utc(2026, 1, 15, 0)), 11 * H);
    assert_eq!(sydney.offset(utc(2026, 6, 15, 0)), 10 * H);
    assert_eq!(sydney.next_midnight(utc(2026, 6, 15, 0)), utc(2026, 6, 15, 14));
    // Week 5 is the last one: the last Sunday of October 2026 is the 25th.
    let europe = Zone::parse("CET-1CEST,M3.5.0,M10.5.0/3").unwrap();
    assert_eq!(europe.offset(utc(2026, 10, 25, 1) - 1), 2 * H);
    assert_eq!(europe.offset(utc(2026, 10, 25, 1)), H);
    assert_eq!(Zone::parse("<+0530>-5:30").unwrap().offset(0), 5 * H + 30 * 60);
    for bad in ["", "P8", "PST", "PST8PDT", "PST8PDT,J60,J300", "PST8PDT,M13.1.0,M11.1.0"] {
        assert!(Zone::parse(bad).is_err(), "{bad:?}");
    }
    assert!(Zone::named("../../etc/passwd").is_err());
    assert!(Zone::named("Nowhere/Atlantis").is_err());
}

#[test]
fn a_zoneinfo_file_gives_its_last_line() {
    let mut file = b"TZif2\0\0\0 binary data here".to_vec();
    file.extend_from_slice(b"\nPST8PDT,M3.2.0,M11.1.0\n");
    assert_eq!(zone::footer(&file).as_deref(), Some("PST8PDT,M3.2.0,M11.1.0"));
    assert_eq!(zone::footer(b"TZif\0 version one has no rule\n"), None);
    assert_eq!(zone::footer(b"not a zone file\nMST7\n"), None);
}

#[test]
fn the_default_is_pacific_time_with_or_without_zoneinfo() {
    // Late October 2026: UTC-7 under either tzdata (summer time, or BC's all-year UTC-7).
    let z = Zone::named(zone::DEFAULT).unwrap();
    assert_eq!(z.next_midnight(utc(2026, 10, 27, 19)), utc(2026, 10, 28, 7));
    assert_eq!(Zone::parse(zone::DEFAULT_RULE).unwrap().next_midnight(utc(2026, 10, 27, 19)), utc(2026, 10, 28, 7));
    assert_eq!((settings().per_night, settings().recheck_behind), (PER_NIGHT, RECHECK_BEHIND));
}

// ---------- the job ----------

/// A local midnight in the setting's zone, after `now` and within a day.
fn is_next_midnight(t: i64, now: i64) {
    let z = &settings().zone;
    assert_eq!((t + z.offset(t)).rem_euclid(86_400), 0, "{t} is local midnight");
    assert!(t > now && t - now <= 86_400 + H, "{t} is the next one after {now}");
}

#[test]
fn the_job_waits_for_midnight_runs_then_queues_the_next_one() {
    const T: i64 = 1_800_000_000; // a fake clock, in 2027
    let db = Db::open_in_memory().unwrap();
    let runs = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = Arc::clone(&runs);
    let runner = jobs::Runner::new().handle(NIGHTLY, move |_, _| {
        counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok("ran".into())
    });
    // A boot queues it for the next midnight, not now.
    assert_eq!(runner.seed(&db, T).unwrap(), 1);
    let first = db.with(|c| jobs::recent(c, 5)).unwrap()[0].run_at;
    is_next_midnight(first, T);
    assert_eq!(runner.run_one(&db, T).unwrap(), None);
    assert_eq!(runner.run_one(&db, first - 1).unwrap(), None, "not before midnight");
    let (_, outcome) = runner.run_one(&db, first).unwrap().unwrap();
    assert_eq!(outcome, jobs::Outcome::Done);
    assert_eq!(runs.load(std::sync::atomic::Ordering::SeqCst), 1);
    // The next run waits for the next midnight.
    let rows = db.with(|c| jobs::recent(c, 5)).unwrap();
    assert_eq!((rows[0].kind.as_str(), rows[0].state.as_str()), (NIGHTLY.name, "queued"));
    is_next_midnight(rows[0].run_at, first);
    assert!(rows[0].run_at >= first + 23 * H);
    assert_eq!(runner.seed(&db, first + 60).unwrap(), 0, "a boot adds no second one");
}

// ---------- the run ----------

/// A Jev that scores "hurt yourself" high, counts its calls, or fails.
struct Jev {
    calls: Cell<usize>,
    fail: bool,
}

impl Judge for Jev {
    fn model(&self) -> &str {
        "fake-jev"
    }

    fn judge(&self, state: &str) -> Result<jev::Answers, String> {
        if self.fail {
            return Err("Jev: connection refused".into());
        }
        self.calls.set(self.calls.get() + 1);
        Ok(crate::content_check::tests::answers(state, 1000))
    }
}

fn fake() -> Jev {
    Jev { calls: Cell::new(0), fail: false }
}

/// A review model that finds the text harmful.
struct Harsh {
    calls: RefCell<usize>,
}

impl Model for Harsh {
    fn name(&self) -> String {
        "fake-model".into()
    }

    fn ask(&self, _call: &ai::Call, _system: &str, _messages: &[ChatMessage]) -> Result<ai::Reply, String> {
        *self.calls.borrow_mut() += 1;
        let text = serde_json::json!({"verdict": "harmful", "question": "self_harm", "quotes": [{"path": "lessons/a.json", "text": "hurt yourself"}], "report": "It tells learners to hurt themselves. Nothing frames it as something to avoid."});
        Ok(ai::Reply { text: text.to_string(), usage: ai::Usage { input: 4000, output: 200, cached: 0 } })
    }

    fn record(&self, _conn: &Connection, _owner: i64, _call: &ai::Call, _usage: ai::Usage) -> rusqlite::Result<i64> {
        Ok(20_000)
    }
}

fn lesson(body: &str) -> CourseFile {
    CourseFile { path: "lessons/a.json".into(), content: serde_json::json!({"id": "a", "title": "Colours", "content": body}).to_string() }
}

/// A made course with one lesson, edited at `edited` (seconds from 1970).
fn course(db: &Db, id: &str, body: &str, edited: i64) {
    db.with(|c| {
        let uid = crate::auth::create_user(c, &format!("{id}@example.org"), "x")?;
        let files = vec![CourseFile { path: "course.json".into(), content: r#"{"id": "c", "title": "My course"}"#.into() }, lesson(body)];
        crate::courses::create_course(c, uid, id, "My course", &files)?;
        c.execute("UPDATE courses SET updated_at = ?2 WHERE id = ?1", params![id, edited])
    })
    .unwrap();
}

/// When the content check last read the course's files, `None` when never.
fn checked_at(conn: &Connection, course_id: &str) -> rusqlite::Result<Option<i64>> {
    use rusqlite::OptionalExtension;
    conn.query_row("SELECT checked_at FROM content_checks WHERE course_id = ?1", [course_id], |r| r.get(0)).optional()
}

fn checked(db: &Db, id: &str, at: i64) {
    db.with(|c| c.execute("INSERT OR REPLACE INTO content_checks (course_id, checked_at) VALUES (?1, ?2)", params![id, at])).unwrap();
}

#[test]
fn only_new_and_edited_courses_are_due_oldest_edit_first() {
    let db = Db::open_in_memory().unwrap();
    course(&db, "untouched", "rojo", 1000);
    checked(&db, "untouched", 1500);
    course(&db, "edited", "azul", 2000);
    checked(&db, "edited", 1500);
    course(&db, "new", "verde", 1800);
    course(&db, "down", "negro", 100);
    db.with(|c| c.execute("UPDATE courses SET taken_down_at = 1 WHERE id = 'down'", [])).unwrap();
    assert_eq!(db.with(|c| due(c, 10)).unwrap(), (vec!["new".to_string(), "edited".to_string()], 2));
    assert_eq!(db.with(|c| due(c, 1)).unwrap(), (vec!["new".to_string()], 2));
}

#[test]
fn the_cap_leaves_the_rest_for_the_next_night() {
    let db = Db::open_in_memory().unwrap();
    course(&db, "one", "rojo", 1000);
    course(&db, "two", "azul", 2000);
    let jev = fake();
    let night = run(&db, &jev, None, 1, RECHECK_BEHIND, &|_| {}).unwrap();
    assert_eq!((night.checked, night.passed, night.left, night.failed), (1, 1, 1, 0), "{night:?}");
    assert!(night.cost_usd > 0.0);
    assert!(db.with(|c| checked_at(c, "one")).unwrap().is_some());
    assert_eq!(db.with(|c| checked_at(c, "two")).unwrap(), None);
    assert!(night.note().contains("1 checked (1 passed, 0 flagged") && night.note().contains("1 left for the next night"), "{}", night.note());
    // The next night takes the other; then nothing is due until an edit.
    let night = run(&db, &jev, None, 1, RECHECK_BEHIND, &|_| {}).unwrap();
    assert_eq!((night.checked, night.left), (1, 0));
    assert_eq!(run(&db, &jev, None, 1, RECHECK_BEHIND, &|_| {}).unwrap(), Night::default());
    let calls = jev.calls.get();
    db.with(|c| crate::courses::write_file(c, "two", "lessons/a.json", &lesson("azul y rojo").content)).unwrap();
    let night = run(&db, &jev, None, 5, RECHECK_BEHIND, &|_| {}).unwrap();
    assert_eq!(night.checked, 1);
    assert_eq!(jev.calls.get(), calls + 1, "only the edited lesson goes to Jev");
}

#[test]
fn a_high_score_is_reviewed_in_the_same_night_and_flagged() {
    let db = Db::open_in_memory().unwrap();
    course(&db, "bad", "When it hurts, hurt yourself.", 1000);
    course(&db, "good", "rojo", 1000);
    let model = Harsh { calls: RefCell::new(0) };
    let flagged = RefCell::new(Vec::new());
    let night = run(&db, &fake(), Some(&model), 10, RECHECK_BEHIND, &|id| flagged.borrow_mut().push(id.to_string())).unwrap();
    assert_eq!((night.checked, night.flagged, night.passed, night.waiting), (2, 1, 1, 0), "{night:?}");
    assert_eq!(*model.calls.borrow(), 1);
    assert_eq!(*flagged.borrow(), vec!["bad".to_string()]);
    assert!(night.cost_usd >= 0.02, "the review's cost counts: {}", night.cost_usd);
    assert!(db.with(|c| crate::course_flags::blocks(c, "bad")).unwrap());
    // Without stage 2 the part waits for its review.
    let db = Db::open_in_memory().unwrap();
    course(&db, "bad", "When it hurts, hurt yourself.", 1000);
    let night = run(&db, &fake(), None, 10, RECHECK_BEHIND, &|_| {}).unwrap();
    assert_eq!((night.checked, night.waiting), (1, 1));
}

#[test]
fn a_failing_course_is_queued_again_and_the_night_goes_on() {
    let db = Db::open_in_memory().unwrap();
    course(&db, "one", "rojo", 1000);
    course(&db, "two", "azul", 2000);
    let night = run(&db, &Jev { calls: Cell::new(0), fail: true }, None, 10, RECHECK_BEHIND, &|_| {}).unwrap();
    assert_eq!((night.checked, night.failed), (0, 2));
    let jobs = db.with(|c| jobs::recent(c, 10)).unwrap();
    assert_eq!(jobs.iter().filter(|j| j.kind == crate::content_check::SCAN.name && j.state == "queued").count(), 2);
    assert_eq!(db.with(|c| due(c, 10)).unwrap().1, 2, "still due");
}

#[test]
fn a_new_term_list_checks_every_course_again_past_the_cap() {
    let db = Db::open_in_memory().unwrap();
    course(&db, "one", "rojo", 1000);
    course(&db, "two", "azul", 2000);
    let jev = fake();
    run(&db, &jev, None, 10, RECHECK_BEHIND, &|_| {}).unwrap();
    assert!(db.with(|c| terms_behind(c)).unwrap().is_empty());

    // Both were checked under an older list (#1039); "two" now has a listed term (written past the check).
    let old = crate::course_flags::terms::TERMS_VERSION - 1;
    db.with(|c| c.execute("UPDATE course_flags SET terms_version = ?1", [old])).unwrap();
    db.with(|c| c.execute("UPDATE course_files SET content = ?1 WHERE course_id = 'two' AND path = 'lessons/a.json'", [lesson("azul gook").content])).unwrap();
    assert_eq!(db.with(|c| terms_behind(c)).unwrap(), vec!["one".to_string(), "two".to_string()]);
    let flagged = RefCell::new(Vec::new());
    let night = run(&db, &jev, None, 1, RECHECK_BEHIND, &|id| flagged.borrow_mut().push(id.to_string())).unwrap();
    assert_eq!((night.terms_rechecked, night.checked), (2, 0), "the term recheck has no cap: {night:?}");
    assert!(night.note().contains("2 checked against a new term list"), "{}", night.note());
    assert_eq!(*flagged.borrow(), vec!["two".to_string()]);
    assert!(db.with(|c| terms_behind(c)).unwrap().is_empty());
    let f = db.with(|c| crate::course_flags::get(c, "two")).unwrap().unwrap();
    assert_eq!((f.blocks(), f.terms_version), (true, crate::course_flags::terms::TERMS_VERSION));
}

// ---------- AI rechecks (#1040) ----------

/// Mark every stored Jev score of a course as made under AI check version `v`.
fn scored_under(db: &Db, id: &str, v: i64) {
    db.with(|c| c.execute("UPDATE content_scores SET ai_version = ?2 WHERE course_id = ?1", params![id, v])).unwrap();
}

fn ai_version(db: &Db, id: &str) -> i64 {
    db.with(|c| crate::content_check::summary(c, id)).unwrap().unwrap().ai_version
}

#[test]
fn only_a_course_far_enough_behind_is_rechecked() {
    let db = Db::open_in_memory().unwrap();
    course(&db, "far", "rojo", 1000);
    course(&db, "near", "azul", 1000);
    course(&db, "current", "verde", 1000);
    let jev = fake();
    run(&db, &jev, None, 10, 3, &|_| {}).unwrap();
    scored_under(&db, "far", AI_CHECK_VERSION - 3);
    scored_under(&db, "near", AI_CHECK_VERSION - 1);
    assert_eq!(db.with(|c| ai_behind(c, 3, 10)).unwrap(), (vec![("far".to_string(), AI_CHECK_VERSION - 3)], 1));

    let calls = jev.calls.get();
    let night = run(&db, &jev, None, 10, 3, &|_| {}).unwrap();
    assert_eq!((night.checked, night.recheck.ran, night.recheck.left), (0, 1, 0), "{night:?}");
    assert!(jev.calls.get() > calls, "the old scores don't count: its parts go to Jev again");
    assert!(night.recheck.cost_usd > 0.0 && night.cost_usd >= night.recheck.cost_usd);
    assert!(night.note().contains("1 rechecked (0 flagged"), "{}", night.note());
    assert_eq!((ai_version(&db, "far"), ai_version(&db, "near")), (AI_CHECK_VERSION, AI_CHECK_VERSION - 1));
    // It passed: it drops out of the list.
    assert_eq!(db.with(|c| ai_behind(c, 3, 10)).unwrap(), (vec![], 0));
}

#[test]
fn rechecks_share_the_cap_oldest_check_first() {
    let db = Db::open_in_memory().unwrap();
    course(&db, "newer", "rojo", 1000);
    course(&db, "older", "azul", 1000);
    let jev = fake();
    run(&db, &jev, None, 10, 1, &|_| {}).unwrap();
    for id in ["newer", "older"] {
        scored_under(&db, id, AI_CHECK_VERSION - 1);
    }
    checked(&db, "older", 2000);
    checked(&db, "newer", 3000);
    // An edited course comes first and takes the whole cap.
    course(&db, "edited", "verde", 5000);
    let night = run(&db, &jev, None, 1, 1, &|_| {}).unwrap();
    assert_eq!((night.checked, night.recheck.ran, night.recheck.left), (1, 0, 2), "{night:?}");
    // One a night, the oldest check first.
    let night = run(&db, &jev, None, 1, 1, &|_| {}).unwrap();
    assert_eq!((night.checked, night.recheck.ran, night.recheck.left), (0, 1, 1), "{night:?}");
    assert_eq!((ai_version(&db, "older"), ai_version(&db, "newer")), (AI_CHECK_VERSION, AI_CHECK_VERSION - 1));
    let night = run(&db, &jev, None, 1, 1, &|_| {}).unwrap();
    assert_eq!((night.recheck.ran, night.recheck.left), (1, 0));
    assert_eq!(ai_version(&db, "newer"), AI_CHECK_VERSION);
}

/// A review model with a fixed verdict that keeps what it was sent.
struct Reviewer {
    harmful: bool,
    seen: RefCell<String>,
}

impl Model for Reviewer {
    fn name(&self) -> String {
        "fake-model".into()
    }

    fn ask(&self, _call: &ai::Call, _system: &str, messages: &[ChatMessage]) -> Result<ai::Reply, String> {
        *self.seen.borrow_mut() = messages.iter().map(|m| m.content.clone()).collect::<Vec<_>>().join("\n");
        let text = if self.harmful {
            serde_json::json!({"verdict": "harmful", "question": "self_harm", "quotes": [{"path": "lessons/a.json", "text": "hurt yourself"}], "report": "It tells learners to hurt themselves."})
        } else {
            serde_json::json!({"verdict": "fine", "question": "", "quotes": [], "report": "The note covers it."})
        };
        Ok(ai::Reply { text: text.to_string(), usage: ai::Usage { input: 4000, output: 200, cached: 0 } })
    }
}

#[test]
fn a_recheck_reads_the_override_note_and_a_new_flag_says_it_was_a_recheck() {
    use crate::course_flags::State;
    let db = Db::open_in_memory().unwrap();
    course(&db, "bad", "When it hurts, hurt yourself.", 1000);
    let harsh = Reviewer { harmful: true, seen: RefCell::new(String::new()) };
    run(&db, &fake(), Some(&harsh), 10, 3, &|_| {}).unwrap();
    let note = "A lesson about what not to say; fine.";
    db.with(|c| crate::course_flags::override_flag(c, "bad", note, 1, crate::db::now())).unwrap();

    // The note covers it: the override stays.
    scored_under(&db, "bad", AI_CHECK_VERSION - 3);
    let kind = Reviewer { harmful: false, seen: RefCell::new(String::new()) };
    let night = run(&db, &fake(), Some(&kind), 10, 3, &|_| {}).unwrap();
    assert_eq!((night.recheck.ran, night.recheck.flagged), (1, 0), "{night:?}");
    assert!(kind.seen.borrow().contains(note), "stage 2 reads the note: {}", kind.seen.borrow());
    assert_eq!(db.with(|c| crate::course_flags::get(c, "bad")).unwrap().unwrap().state, State::Overridden);
    assert_eq!(ai_version(&db, "bad"), AI_CHECK_VERSION);

    // It fails under the new rules: flagged again, and the report says why it was looked at.
    scored_under(&db, "bad", AI_CHECK_VERSION - 4);
    let flagged = RefCell::new(Vec::new());
    let night = run(&db, &fake(), Some(&harsh), 10, 3, &|id| flagged.borrow_mut().push(id.to_string())).unwrap();
    assert_eq!((night.recheck.ran, night.recheck.flagged), (1, 1), "{night:?}");
    assert_eq!(*flagged.borrow(), vec!["bad".to_string()]);
    let f = db.with(|c| crate::course_flags::get(c, "bad")).unwrap().unwrap();
    assert_eq!(f.state, State::Flagged);
    let says = format!("Rechecked under AI check version {AI_CHECK_VERSION} (it was last checked under version {}). It tells", AI_CHECK_VERSION - 4);
    assert!(f.reason.starts_with(&says), "{}", f.reason);
}
