use super::*;
use crate::course_flags::table::{self, Show};
use std::cell::RefCell;
use std::io::{Read, Write};

/// A Jev that scores by words in the text and counts its calls.
struct Fake {
    calls: RefCell<Vec<String>>,
    fail: bool,
}

impl Fake {
    fn new() -> Fake {
        Fake { calls: RefCell::new(Vec::new()), fail: false }
    }

    fn calls(&self) -> usize {
        self.calls.borrow().len()
    }
}

impl Judge for Fake {
    fn model(&self) -> &str {
        "fake-jev"
    }

    fn judge(&self, state: &str) -> Result<jev::Answers, String> {
        if self.fail {
            return Err("Jev: request failed: connection refused".into());
        }
        self.calls.borrow_mut().push(state.to_string());
        Ok(answers(state, 1000))
    }
}

/// Every category low, except "hate" for "vermin" and "self_harm" for
/// "hurt yourself" (the other tests' fakes use this too).
pub(crate) fn answers(state: &str, input_tokens: i64) -> jev::Answers {
    let probabilities = CATEGORIES
        .iter()
        .map(|c| {
            let p = match c.name {
                "hate" if state.contains("vermin") => 0.9,
                "self_harm" if state.contains("hurt yourself") => 0.95,
                "terms" => 0.05,
                _ => 0.01,
            };
            (c.name.to_string(), p)
        })
        .collect();
    jev::Answers { probabilities, input_tokens }
}

fn lesson(path: &str, title: &str, body: &str) -> CourseFile {
    CourseFile { path: path.into(), content: serde_json::json!({"id": "x-id", "title": title, "content": body, "quiz": [{"id": "q1", "type": "true-false", "question": "Right?"}]}).to_string() }
}

fn course(db: &Db, id: &str) {
    db.with(|c| {
        let uid = crate::auth::create_user(c, &format!("{id}@example.org"), "x")?;
        let files = vec![
            CourseFile { path: "course.json".into(), content: r#"{"id": "c", "title": "My course"}"#.into() },
            lesson("lessons/a.json", "Colours", "El gato es negro."),
            lesson("lessons/b.json", "Numbers", "uno, dos, tres"),
        ];
        crate::courses::create_course(c, uid, id, "My course", &files)
    })
    .unwrap();
}

#[test]
fn a_course_is_split_into_parts_of_text() {
    let files = vec![
        CourseFile { path: "course.json".into(), content: r#"{"id": "c", "title": "My course", "description": "Learn"}"#.into() },
        lesson("lessons/a.json", "Colours", "El gato es negro."),
        CourseFile { path: "pictures/cat.svg".into(), content: "<svg/>".into() },
        CourseFile { path: "lessons/b.json".into(), content: "not json, read as it is".into() },
    ];
    let parts = chunks("Spanish", &files);
    assert_eq!(parts.iter().map(|p| p.key.as_str()).collect::<Vec<_>>(), ["course.json", "lessons/a.json", "lessons/b.json"], "pictures are left out");
    let a = &parts[1];
    assert_eq!(a.lesson, "Colours");
    assert!(a.state.starts_with("Course: Spanish\nLesson: Colours\n\n"), "{}", a.state);
    assert!(a.state.contains("El gato es negro.") && a.state.contains("Right?"));
    assert!(!a.state.contains("x-id") && !a.state.contains("true-false") && !a.state.contains("q1"), "ids and types are not text: {}", a.state);
    assert_eq!(parts[2].lesson, "lessons/b.json");
    assert_eq!(a.hash.len(), 64);
    assert_eq!(chunks("Spanish", &files)[1].hash, a.hash, "the same text, the same hash");
}

#[test]
fn a_long_lesson_is_split_at_line_ends() {
    let line = "una frase corta y normal. ".repeat(20);
    let body = vec![line.clone(); PART_CHARS * 2 / line.len() + 3].join("\n");
    let parts = chunks("T", &[lesson("lessons/long.json", "Long", &body)]);
    assert!(parts.len() >= 2, "{}", parts.len());
    assert_eq!(parts[0].key, "lessons/long.json");
    assert_eq!(parts[1].key, "lessons/long.json#2");
    assert!(parts.iter().all(|p| p.lesson == "Long"));
    let one_line = "x".repeat(PART_CHARS + 10);
    assert_eq!(split(&one_line, PART_CHARS).len(), 2, "a line longer than a part is cut");
    assert!(split(&one_line, PART_CHARS).iter().all(|p| p.chars().count() <= PART_CHARS));
}

#[test]
fn a_scan_stores_scores_and_sends_only_what_changed() {
    let db = Db::open_in_memory().unwrap();
    course(&db, "c1");
    let jev = Fake::new();

    let r = scan(&db, &jev, "c1").unwrap();
    assert_eq!((r.parts, r.scanned, r.needs_review, r.skipped), (3, 3, false, false));
    assert!((r.cost_usd - 3.0 * 1000.0 * jev::PRICE_PER_TOKEN).abs() < 1e-12);
    let s = db.with(|c| summary(c, "c1")).unwrap().unwrap();
    assert_eq!((s.parts, s.needs_review, s.top()), (3, false, 0.05));
    // One score per part and category (#1046), in the category order.
    assert_eq!(s.scores.iter().map(|(c, _)| c.as_str()).collect::<Vec<_>>(), CATEGORIES.iter().map(|c| c.name).collect::<Vec<_>>());
    let rows: i64 = db.with(|c| c.query_row("SELECT COUNT(*) FROM content_category_scores WHERE course_id = 'c1'", [], |r| r.get(0))).unwrap();
    assert_eq!(rows, 3 * CATEGORIES.len() as i64);
    assert!(s.high.is_empty());

    // Nothing changed: nothing is sent.
    assert_eq!(scan(&db, &jev, "c1").unwrap().scanned, 0);
    assert_eq!(jev.calls(), 3);

    // One lesson edited: only it is sent again.
    db.with(|c| crate::courses::write_file(c, "c1", "lessons/b.json", &lesson("lessons/b.json", "Numbers", "If you fail, hurt yourself.").content)).unwrap();
    let r = scan(&db, &jev, "c1").unwrap();
    assert_eq!((r.scanned, r.needs_review), (1, true));
    assert!(jev.calls.borrow().last().unwrap().contains("Lesson: Numbers"));
    let s = db.with(|c| summary(c, "c1")).unwrap().unwrap();
    assert!(s.needs_review);
    assert_eq!(s.top(), 0.95);
    assert_eq!(s.high.len(), 1);
    let h = &s.high[0];
    assert_eq!((h.chunk.as_str(), h.lesson.as_str(), h.categories.clone(), h.score("self_harm"), h.review.as_str()), ("lessons/b.json", "Numbers", vec!["self_harm".to_string()], 0.95, "queued"));
    assert_eq!(s.high_categories(), ["self_harm"]);
    let queued: String = db.with(|c| c.query_row("SELECT review FROM content_scores WHERE course_id = 'c1' AND chunk = 'lessons/b.json'", [], |r| r.get(0))).unwrap();
    assert_eq!(queued, "queued", "stage 2 (#1037) reads this");

    // The lesson is deleted: its score goes, and the course passes again.
    db.with(|c| crate::courses::delete_file(c, "c1", "lessons/b.json")).unwrap();
    let r = scan(&db, &jev, "c1").unwrap();
    assert_eq!((r.parts, r.scanned, r.needs_review), (2, 0, false));
    assert_eq!(db.with(|c| summary(c, "c1")).unwrap().unwrap().parts, 2);
    let rows: i64 = db.with(|c| c.query_row("SELECT COUNT(*) FROM content_category_scores WHERE course_id = 'c1' AND chunk = 'lessons/b.json'", [], |r| r.get(0))).unwrap();
    assert_eq!(rows, 0, "its category scores go with it");
}

#[test]
fn a_failed_call_keeps_what_was_scored_and_the_rest_waits() {
    let db = Db::open_in_memory().unwrap();
    course(&db, "c2");
    let down = Fake { fail: true, ..Fake::new() };
    assert!(scan(&db, &down, "c2").unwrap_err().contains("connection refused"));
    assert_eq!(db.with(|c| summary(c, "c2")).unwrap(), None);
    let jev = Fake::new();
    assert_eq!(scan(&db, &jev, "c2").unwrap().scanned, 3);
}

#[test]
fn a_course_being_written_or_gone_is_not_scanned() {
    let db = Db::open_in_memory().unwrap();
    course(&db, "c3");
    db.with(|c| c.execute("UPDATE courses SET state = 'generating' WHERE id = 'c3'", [])).unwrap();
    let jev = Fake::new();
    assert!(scan(&db, &jev, "c3").unwrap().skipped);
    assert!(scan(&db, &jev, "nope").unwrap().skipped);
    assert_eq!(jev.calls(), 0);
}

#[test]
fn a_high_score_shows_as_needs_review_in_the_course_table_and_blocks_nothing() {
    let db = Db::open_in_memory().unwrap();
    course(&db, "ok");
    course(&db, "bad");
    db.with(|c| crate::courses::write_file(c, "bad", "lessons/a.json", &lesson("lessons/a.json", "Them", "people like that are vermin").content)).unwrap();
    let jev = Fake::new();
    scan(&db, &jev, "ok").unwrap();
    scan(&db, &jev, "bad").unwrap();
    db.with(|c| {
        let all = table::courses(c, "", Show::All, 1)?;
        assert_eq!(all.list.iter().map(|r| (r.id.as_str(), r.state())).collect::<Vec<_>>(), [("bad", "review"), ("ok", "clear")], "needs review comes first");
        let review = table::courses(c, "", Show::NeedsReview, 1)?;
        assert_eq!(review.list.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["bad"]);
        assert_eq!(review.list[0].scan.as_ref().map(|s| s.high_categories()), Some(vec!["hate".to_string()]));
        assert!(!crate::course_flags::blocks(c, "bad")?, "stage 1 doesn't block");
        assert!(!crate::courses::load_ready_files(c, "bad")?.is_empty(), "it still opens");
        Ok(())
    })
    .unwrap();
    assert_eq!(Show::parse("review"), Show::NeedsReview);
}

#[test]
fn the_job_kind_is_known_and_nothing_is_queued_when_the_server_does_not_scan() {
    assert!(jobs::kind(SCAN.name).is_some());
    let db = Db::open_in_memory().unwrap();
    course(&db, "c4");
    // No Jev in tests (init is never called): finishing a generation queues nothing.
    db.with(|c| crate::courses::finish_generation(c, "c4", None)).unwrap();
    let n: i64 = db.with(|c| c.query_row("SELECT COUNT(*) FROM jobs WHERE kind = ?1", [SCAN.name], |r| r.get(0))).unwrap();
    assert_eq!(n, 0);
}

#[test]
fn jev_replies_are_read_and_a_missing_answer_is_an_error() {
    let q = [("terms", "a"), ("self_harm", "b")];
    let a = jev::parse(r#"{"answers":{"terms":{"type":"noul","noul":0.1},"self_harm":{"type":"noul","noul":0.0}},"usage":{"input_tokens":301}}"#, &q).unwrap();
    assert_eq!((a.get("terms"), a.get("self_harm"), a.input_tokens), (0.1, 0.0, 301));
    assert!((a.cost_usd() - 301.0 * 0.042 / 1e6).abs() < 1e-15);
    assert!(jev::parse(r#"{"answers":{"terms":{"noul":0.1}}}"#, &q).unwrap_err().contains("self_harm"));
    assert!(jev::parse("oops", &q).is_err());
}

/// The client over HTTP, against a one-shot server that checks the request.
#[test]
fn the_jev_client_speaks_the_systemone_api() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let n = s.read(&mut chunk).unwrap();
            buf.extend_from_slice(&chunk[..n]);
            let text = String::from_utf8_lossy(&buf);
            if let Some(end) = text.find("\r\n\r\n") {
                let len = text.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap())).unwrap_or(0);
                if buf.len() >= end + 4 + len {
                    break;
                }
            }
        }
        let answers: serde_json::Map<String, serde_json::Value> =
            CATEGORIES.iter().map(|c| (c.name.to_string(), serde_json::json!({"type": "noul", "noul": if c.name == "self_harm" { 0.7 } else { 0.2 }}))).collect();
        let body = serde_json::json!({"model": "jev-1.13.0", "answers": answers, "usage": {"input_tokens": 50}}).to_string();
        write!(s, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        String::from_utf8(buf).unwrap()
    });
    let j = Jev::new(&format!("http://127.0.0.1:{port}/"), "k-123");
    let a = j.judge("Course: T\n\nhello").unwrap();
    assert_eq!((a.get("terms"), a.get("self_harm"), a.input_tokens), (0.2, 0.7, 50));
    let req = server.join().unwrap();
    assert!(req.starts_with("POST /v1/systemone "), "{req}");
    assert!(req.contains("Bearer k-123"), "{req}");
    let body: serde_json::Value = serde_json::from_str(req.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(body["model"], jev::MODEL);
    assert_eq!(body["state"], "Course: T\n\nhello");
    // Every category in the one call (#1046).
    assert_eq!(body["questions"].as_object().unwrap().len(), CATEGORIES.len());
    assert_eq!(body["questions"]["self_harm"]["type"], "noul");
    assert_eq!(body["questions"]["firearms"]["instructions"], Category::named("firearms").unwrap().question);
}

/// The dev server's stand-in scores from its fixture (data/test-fixtures/jev).
#[test]
fn the_dev_stand_in_answers_from_its_fixture() {
    let fixture: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(jev::STAND_IN_FIXTURE).unwrap()).unwrap();
    let ask = |state: &str| {
        let qs = Category::questions();
        let questions: serde_json::Map<String, serde_json::Value> = qs.iter().map(|(n, _)| (n.to_string(), serde_json::json!({"type": "noul"}))).collect();
        let req = serde_json::json!({"model": "m", "state": state, "questions": questions});
        let a = jev::stand_in(&fixture, &req);
        jev::parse(&a.to_string(), &qs).unwrap()
    };
    let calm = ask("El gato es negro.");
    assert!(high(&calm).is_empty());
    let bad = ask("When it hurts, HURTING YOURSELF IS THE ANSWER.");
    assert_eq!(high(&bad), ["self_harm"]);
    assert!(bad.get("self_harm") > 0.9);
}

// ---------- calibration (scripts/content-check-calibrate.sh) ----------

#[derive(serde::Deserialize)]
struct Sample {
    name: String,
    expect: String,
    /// For a harmful one: the category it should score high on (#1046).
    #[serde(default)]
    category: String,
    course: String,
    lesson: String,
    text: String,
}

/// Score `parts` with Jev asking `questions`, eight calls at a time.
fn score_all(jev: &Jev, parts: &[Chunk], questions: &[(&str, &str)]) -> Vec<jev::Answers> {
    let out = std::sync::Mutex::new(vec![jev::Answers { probabilities: Vec::new(), input_tokens: 0 }; parts.len()]);
    let next = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|s| {
        for _ in 0..8 {
            s.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let Some(p) = parts.get(i) else { break };
                let a = (0..3).find_map(|_| jev.ask(&p.state, questions).ok()).unwrap_or_else(|| panic!("Jev failed three times on {}", p.key));
                out.lock().unwrap()[i] = a;
            });
        }
    });
    out.into_inner().unwrap()
}

/// The highest score per category over several answers.
fn best_of<'a>(answers: impl Iterator<Item = &'a jev::Answers>) -> Vec<f64> {
    let mut best = vec![0.0f64; CATEGORIES.len()];
    for a in answers {
        for (i, c) in CATEGORIES.iter().enumerate() {
            best[i] = best[i].max(a.get(c.name));
        }
    }
    best
}

/// Run by scripts/content-check-calibrate.sh with the real key: the planted
/// and ordinary lessons of data/test-fixtures/content-check/calibration.json,
/// then every built-in course as if it were made (false flags on real
/// content, and the cost per course). Prints each lesson's scores, then per
/// category and threshold how many planted lessons of that category it
/// catches and how many ordinary lessons and built-in parts it flags, then
/// the totals at the current thresholds. `CALIBRATE_SINGLE=1` also asks
/// each category of the set in a call of its own and prints how far the
/// one-call scores are from those (#1046).
#[test]
#[ignore = "calls the real Jev: scripts/content-check-calibrate.sh"]
fn calibrate() {
    let key = std::env::var(KEY_VAR).expect("TYPESAFE_API_KEY");
    let jev = Jev::new(jev::DEFAULT_BASE_URL, &key);
    let qs = Category::questions();
    let set: serde_json::Value = serde_json::from_str(&std::fs::read_to_string("data/test-fixtures/content-check/calibration.json").unwrap()).unwrap();
    let samples: Vec<Sample> = serde_json::from_value(set["lessons"].clone()).unwrap();
    for s in samples.iter().filter(|s| s.expect == "harmful") {
        assert!(Category::named(&s.category).is_some(), "{}: no known category {:?}", s.name, s.category);
    }
    let mut parts = Vec::new();
    let mut owner = Vec::new();
    for (i, s) in samples.iter().enumerate() {
        let file = CourseFile { path: format!("lessons/{}.json", s.name), content: serde_json::json!({"title": s.lesson, "content": s.text}).to_string() };
        for p in chunks(&s.course, &[file]) {
            parts.push(p);
            owner.push(i);
        }
    }
    let scores = score_all(&jev, &parts, &qs);
    let best: Vec<Vec<f64>> = (0..samples.len()).map(|i| best_of(scores.iter().zip(&owner).filter(|(_, o)| **o == i).map(|(a, _)| a))).collect();
    let tokens: i64 = scores.iter().map(|a| a.input_tokens).sum();
    let name = |i: usize| CATEGORIES[i].name;
    println!("\n{:<32} {:>8} {:<15} {:>6}   highest", "lesson", "expect", "category", "score");
    for (s, b) in samples.iter().zip(&best) {
        let own = CATEGORIES.iter().position(|c| c.name == s.category).map_or(String::new(), |i| format!("{:.2}", b[i]));
        let mut top: Vec<(usize, f64)> = b.iter().copied().enumerate().collect();
        top.sort_by(|a, b| b.1.total_cmp(&a.1));
        let top: Vec<String> = top.iter().take(2).filter(|(_, v)| *v >= 0.1).map(|(i, v)| format!("{} {v:.2}", name(*i))).collect();
        println!("{:<32} {:>8} {:<15} {:>6}   {}", s.name, s.expect, s.category, own, top.join(", "));
    }
    println!("set: {} lessons, {} calls, {} input tokens, ${:.5}", samples.len(), parts.len(), tokens, tokens as f64 * jev::PRICE_PER_TOKEN);

    if std::env::var("CALIBRATE_SINGLE").is_ok_and(|v| v == "1") {
        let (mut worst, mut sum, mut n, mut single_tokens) = (0.0f64, 0.0, 0, 0i64);
        for (i, q) in qs.iter().enumerate() {
            let one = score_all(&jev, &parts, &[*q]);
            single_tokens += one.iter().map(|a| a.input_tokens).sum::<i64>();
            for (k, a) in one.iter().enumerate() {
                let d = (a.get(q.0) - scores[k].get(q.0)).abs();
                worst = worst.max(d);
                sum += d;
                n += 1;
                if d >= 0.1 {
                    println!("  {} {}: one call {:.2}, alone {:.2}", parts[k].key, name(i), scores[k].get(q.0), a.get(q.0));
                }
            }
        }
        println!(
            "one call vs a call per category: mean difference {:.3}, largest {worst:.2}; tokens {tokens} vs {single_tokens} ({:.1}x)",
            sum / n.max(1) as f64,
            single_tokens as f64 / tokens.max(1) as f64
        );
    }

    // The built-in courses, as made courses: every part.
    let mut courses: Vec<(&str, Vec<Vec<f64>>)> = Vec::new();
    for dir in ["data/courses/spanish", "data/courses/french"] {
        let files = crate::content::read_course_dir(std::path::Path::new(dir)).unwrap();
        let title = files.iter().find(|f| f.path == "course.json").and_then(|f| serde_json::from_str::<serde_json::Value>(&f.content).ok()).and_then(|v| v["title"].as_str().map(String::from)).unwrap_or_default();
        let lessons = files.iter().filter(|f| f.path.starts_with("lessons/")).count();
        let parts = chunks(&title, &files);
        let scores = score_all(&jev, &parts, &qs);
        let tokens: i64 = scores.iter().map(|s| s.input_tokens).sum();
        let per_part: Vec<Vec<f64>> = scores.iter().map(|a| best_of(std::iter::once(a))).collect();
        let mut top: Vec<(f64, String)> = per_part
            .iter()
            .zip(&parts)
            .flat_map(|(b, p)| b.iter().enumerate().map(move |(i, v)| (*v, format!("{} {}", name(i), p.key))))
            .collect();
        top.sort_by(|a, b| b.0.total_cmp(&a.0));
        println!(
            "{dir}: {lessons} lessons, {} parts, {tokens} tokens, ${:.5} (${:.6} a lesson); highest: {}",
            parts.len(),
            tokens as f64 * jev::PRICE_PER_TOKEN,
            tokens as f64 * jev::PRICE_PER_TOKEN / lessons.max(1) as f64,
            top.iter().take(5).map(|(s, k)| format!("{s:.2} {k}")).collect::<Vec<_>>().join(", ")
        );
        courses.push((dir, per_part));
    }

    // Per category: caught / planted of that category, and false flags (ordinary lessons + built-in parts).
    let thresholds = [0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9];
    println!("\nper category: caught/planted, then false flags (ordinary lessons + built-in parts); * = its threshold");
    print!("{:<15}", "category");
    for t in thresholds {
        print!(" {t:>11.1}");
    }
    println!();
    for (i, c) in CATEGORIES.iter().enumerate() {
        print!("{:<15}", c.name);
        for t in thresholds {
            let planted: Vec<usize> = samples.iter().enumerate().filter(|(_, s)| s.expect == "harmful" && s.category == c.name).map(|(k, _)| k).collect();
            let caught = planted.iter().filter(|k| best[**k][i] >= t).count();
            let fine = samples.iter().zip(&best).filter(|(s, b)| s.expect == "fine" && b[i] >= t).count();
            let builtin: usize = courses.iter().map(|(_, ps)| ps.iter().filter(|p| p[i] >= t).count()).sum();
            let mark = if (t - c.threshold).abs() < 1e-9 { "*" } else { " " };
            print!(" {:>4} {:>5}{mark}", format!("{caught}/{}", planted.len()), format!("{fine}+{builtin}"));
        }
        println!();
    }

    // At the current thresholds, any category.
    let flagged = |b: &Vec<f64>| CATEGORIES.iter().enumerate().any(|(i, c)| b[i] >= c.threshold);
    let harmful: Vec<&Vec<f64>> = samples.iter().zip(&best).filter(|(s, _)| s.expect == "harmful").map(|(_, b)| b).collect();
    let caught = harmful.iter().filter(|b| flagged(b)).count();
    let false_flags: Vec<&str> = samples.iter().zip(&best).filter(|(s, b)| s.expect == "fine" && flagged(b)).map(|(s, _)| s.name.as_str()).collect();
    let builtin: Vec<String> = courses.iter().map(|(_, ps)| format!("{}/{}", ps.iter().filter(|p| flagged(p)).count(), ps.len())).collect();
    let ordinary = samples.iter().filter(|s| s.expect == "fine").count() + courses.iter().map(|(_, ps)| ps.len()).sum::<usize>();
    let reviewed = false_flags.len() + courses.iter().map(|(_, ps)| ps.iter().filter(|p| flagged(p)).count()).sum::<usize>();
    println!(
        "\nat the current thresholds: caught {caught}/{}, missed {}, false flags {} {:?}, built-in parts flagged {}, stage 2 rate {:.1}%",
        harmful.len(),
        harmful.len() - caught,
        false_flags.len(),
        false_flags,
        builtin.join(" "),
        100.0 * reviewed as f64 / ordinary.max(1) as f64
    );
}

#[test]
fn a_score_counts_only_at_the_current_ai_check_version() {
    let db = Db::open_in_memory().unwrap();
    course(&db, "c1");
    let jev = Fake::new();
    scan(&db, &jev, "c1").unwrap();
    let versions: Vec<i64> = db.with(|c| c.prepare("SELECT ai_version FROM content_scores")?.query_map([], |r| r.get(0))?.collect()).unwrap();
    assert_eq!(versions, vec![AI_CHECK_VERSION; 3]);
    assert_eq!(db.with(|c| summary(c, "c1")).unwrap().unwrap().ai_version, AI_CHECK_VERSION);

    // A part scored under older rules (#1039) is sent again, and the course shows its oldest version until then.
    db.with(|c| c.execute("UPDATE content_scores SET ai_version = ?1 WHERE chunk = 'lessons/a.json'", [AI_CHECK_VERSION - 1])).unwrap();
    assert_eq!(db.with(|c| summary(c, "c1")).unwrap().unwrap().ai_version, AI_CHECK_VERSION - 1);
    assert_eq!(scan(&db, &jev, "c1").unwrap().scanned, 1);
    assert_eq!(db.with(|c| summary(c, "c1")).unwrap().unwrap().ai_version, AI_CHECK_VERSION);
}

/// docs/content-check-versions.md has a line for each current version.
#[test]
fn the_changelog_names_the_current_versions() {
    let doc = std::fs::read_to_string("docs/content-check-versions.md").unwrap();
    for (check, version) in [("Terms", crate::course_flags::terms::TERMS_VERSION), ("AI", AI_CHECK_VERSION)] {
        let line = format!("| {check} | {version} |");
        assert!(doc.lines().any(|l| l.starts_with(&line)), "docs/content-check-versions.md has no line starting {line:?}: add one for the new version");
    }
}
