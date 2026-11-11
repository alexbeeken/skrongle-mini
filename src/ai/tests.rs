use super::*;
use std::sync::Arc;

#[test]
fn internal_addresses_are_recognised() {
    let internal = [
        "127.0.0.1", "10.1.2.3", "172.16.0.1", "192.168.1.42", "169.254.169.254", "100.64.0.1", "0.0.0.0", "255.255.255.255",
        "224.0.0.1", "240.0.0.1", "::1", "::", "fd00::1", "fe80::1", "::ffff:127.0.0.1", "::ffff:169.254.169.254", "64:ff9b::a9fe:a9fe",
        "::127.0.0.1",
    ];
    for ip in internal {
        assert!(is_internal_ip(ip.parse().unwrap()), "{ip} should be internal");
    }
    for ip in ["8.8.8.8", "1.1.1.1", "160.79.104.10", "2606:4700::1111", "::ffff:8.8.8.8"] {
        assert!(!is_internal_ip(ip.parse().unwrap()), "{ip} should be public");
    }
}

#[test]
fn base_url_hosts_parse() {
    assert_eq!(url_host_port("https://api.example.org/v1"), Some(("api.example.org".into(), 443)));
    assert_eq!(url_host_port("http://Example.org:8080"), Some(("example.org".into(), 8080)));
    assert_eq!(url_host_port("http://[::1]:11434/v1"), Some(("::1".into(), 11434)));
    assert_eq!(url_host_port("http://user:pw@example.org/"), None);
    assert_eq!(url_host_port("ftp://example.org"), None);
    assert_eq!(url_host_port("http://"), None);
    assert_eq!(url_host_port("http://example.org:notaport"), None);
}

/// A learner's base URL cannot point the server at itself or its network
/// (SSRF), unless the operator allows it.
#[test]
fn custom_base_urls_must_be_public() {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    for url in [
        "http://127.0.0.1:8080",
        "http://localhost:11434",
        "http://api.localhost",
        "http://[::1]/v1",
        "http://169.254.169.254/latest",
        "http://10.0.0.5",
        "http://192.168.1.1:80/v1",
        "http://2130706433/",
        "http://0x7f000001/",
        "http://u@example.org",
    ] {
        assert!(rt.block_on(check_base_url(url, false)).is_err(), "{url} should be refused");
    }
    assert!(rt.block_on(check_base_url("http://127.0.0.1:11434", true)).is_ok());
    assert!(rt.block_on(check_base_url("http://localhost:11434", true)).is_ok());
    // A public IP literal needs no DNS.
    assert!(rt.block_on(check_base_url("https://8.8.8.8/v1", false)).is_ok());
}

/// A custom base URL may use only the web ports (#373), unless private
/// URLs are allowed (dev mode, a self-hosted Ollama on 11434).
#[test]
fn custom_base_urls_use_only_web_ports() {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    for url in ["https://8.8.8.8:8443/v1", "http://8.8.8.8:22", "http://8.8.8.8:11434/v1", "https://[2606:4700::1111]:25"] {
        let err = rt.block_on(check_base_url(url, false)).unwrap_err();
        assert!(err.contains("port 443 (https) or 80 (http)"), "{url}: {err}");
    }
    for url in ["https://8.8.8.8:443/v1", "http://8.8.8.8:80/v1", "http://8.8.8.8/v1"] {
        assert!(rt.block_on(check_base_url(url, false)).is_ok(), "{url}");
    }
    assert!(rt.block_on(check_base_url("http://127.0.0.1:11434/v1", true)).is_ok());
    assert!(rt.block_on(check_base_url("https://8.8.8.8:8443/v1", true)).is_ok());
}

/// An OpenAI-compatible server that answers every request with
/// `reply(prompt)` as the message content, or holds every request open
/// while `hold` is set. Returns its base URL.
fn scripted_llm(reply: impl Fn(&str) -> String + Send + 'static, hold: Arc<AtomicBool>) -> String {
    use std::io::{BufRead, BufReader, Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut len = 0usize;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0u8; len];
            let _ = reader.read_exact(&mut body);
            while hold.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(10));
            }
            let req: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
            let prompt = req["messages"].as_array().and_then(|m| m.last()).and_then(|m| m["content"].as_str()).unwrap_or_default().to_string();
            let out = serde_json::json!({ "choices": [ { "message": { "role": "assistant", "content": reply(&prompt) } } ] }).to_string();
            let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}", out.len());
        }
    });
    url
}

/// A course row to generate into, and a request for it.
fn generating(db: &Db, user: &str, id: &str, base_url: String) -> Request {
    let uid = db
        .with(|c| {
            let uid = crate::auth::create_user(c, &format!("{user}@example.org"), "h")?;
            crate::courses::create_generating(c, uid, id, "New", "anything", "")?;
            crate::courses::set_generating(c, id, "j")?;
            Ok(uid)
        })
        .unwrap();
    Request {
        user_id: uid,
        provider: Provider { kind: ProviderKind::OpenAi, base_url, api_key: "k".into(), model: "fake".into(), effort: None },
        course_id: id.into(),
        wish: "anything".into(),
        lesson_count: 3,
        depth: Depth::Full,
        notes: String::new(),
        kind: SubjectKind::Auto,
        instructions: String::new(),
        resume_outline: None,
        revise: None,
        add_lesson: None,
        edit: None,
        translate: None,
        i18n: 0,
        named: Default::default(),
    }
}

fn outline(lessons: usize) -> String {
    let entries: Vec<serde_json::Value> = (1..=lessons)
        .map(|i| serde_json::json!({ "id": format!("l{i}"), "slug": format!("l{i}"), "title": format!("Lesson {i}"), "unit": "1", "group": "core", "prerequisites": [] }))
        .collect();
    serde_json::json!({
        "course": { "id": "placeholder", "title": "Big", "subject": "S", "description": "d", "icon": "📐",
            "terms": { "label": "Term", "lang": "" }, "meanings": { "label": "Meaning", "lang": "" },
            "units": [ { "id": "1", "title": "One" } ], "groups": [ { "id": "core", "title": "Core" } ] },
        "lessons": entries,
    })
    .to_string()
}

/// AI generation keeps to the course caps (#367): an outline of more
/// than `MAX_LESSONS` lessons is refused before anything is saved, a
/// lesson file over `courses::MAX_FILE_BYTES` is never written, and a
/// reply over `MAX_REPLY_BYTES` is not read.
#[test]
fn ai_generation_keeps_to_the_course_caps() {
    let db = Db::open_in_memory().unwrap();
    let registry = crate::courses::Courses::empty();
    let go = Arc::new(AtomicBool::new(false));

    let url = scripted_llm(|_| outline(MAX_LESSONS + 1), Arc::clone(&go));
    let req = generating(&db, "long", "too-long", url);
    let err = run(&test_job("too-long"), &db, &registry, &req).unwrap_err();
    assert!(err.contains(&format!("at most {MAX_LESSONS}")), "{err}");
    assert_eq!(db.with(|c| crate::courses::file_count(c, "too-long")).unwrap(), 0, "nothing saved");

    // A lesson that pretty-prints to more than 1 MiB (its reply is well under).
    let url = scripted_llm(
        |prompt| {
            if prompt.contains("Design a course") {
                outline(1)
            } else {
                format!(r###"{{"slug": "l1", "content": "## A\n\nB", "pad": [{}0]}}"###, "0,".repeat(200_000))
            }
        },
        Arc::clone(&go),
    );
    let req = generating(&db, "big", "too-big", url);
    let err = run(&test_job("too-big"), &db, &registry, &req).unwrap_err();
    assert!(err.contains("lessons/l1.json is larger than 1024 KB"), "{err}");
    assert_eq!(db.with(|c| crate::courses::file_count(c, "too-big")).unwrap(), 1, "only course.json");

    // A reply body over MAX_REPLY_BYTES.
    let url = scripted_llm(|_| "x".repeat(MAX_REPLY_BYTES as usize + 10), Arc::clone(&go));
    let req = generating(&db, "huge", "huge-reply", url);
    let err = run(&test_job("huge-reply"), &db, &registry, &req).unwrap_err();
    assert!(err.contains("the reply is larger than 1024 KB"), "{err}");
    assert_eq!(plain_failure(&err), plain_failure("the reply contains no JSON object"), "reads as an unusable reply");

    // The same caps on any set of files the AI would leave.
    let file = |path: &str, len: usize| CourseFile { path: path.into(), content: "x".repeat(len) };
    assert!(check_file_caps(&[file("course.json", 10), file("lessons/a.json", courses::MAX_FILE_BYTES)]).is_ok());
    assert!(check_file_caps(&[file("lessons/a.json", courses::MAX_FILE_BYTES + 1)]).unwrap_err().contains("lessons/a.json"));
    let many: Vec<CourseFile> = (0..=courses::MAX_FILES).map(|i| file(&format!("lessons/{i}.json"), 1)).collect();
    assert!(check_file_caps(&many).unwrap_err().contains("at most"));
}

/// The generation slots (#368, #874): taken under one lock, so two requests
/// at once cannot both get one; held until the generator thread exits, even
/// after the job is dropped with its account; a second job while one runs is
/// refused, never queued (#43); and a job stops when its time budget runs out.
#[test]
fn ai_job_slots_are_taken_atomically_and_held_until_the_thread_exits() {
    let db = Db::open_in_memory().unwrap();
    let registry = Arc::new(crate::courses::Courses::empty());
    let jobs = Arc::new(Jobs::default());
    let hold = Arc::new(AtomicBool::new(true));
    let url = scripted_llm(|_| "not JSON".into(), Arc::clone(&hold));

    // One user posting Generate many times at once gets one job.
    let ann = generating(&db, "ann", "ann-course", url.clone());
    let started: Vec<bool> = (0..8)
        .map(|i| {
            let (jobs, db, registry, req) = (Arc::clone(&jobs), db.clone(), Arc::clone(&registry), ann.clone());
            std::thread::spawn(move || start(&jobs, db, registry, req, format!("ann-{i}"), 0).is_ok())
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|t| t.join().unwrap())
        .collect();
    assert_eq!(started.iter().filter(|ok| **ok).count(), 1, "{started:?}");

    // One job at a time: Bob is refused at once, with the message, and
    // nothing is left waiting.
    let bob = generating(&db, "bob", "bob-course", url.clone());
    let refused = start(&jobs, db.clone(), Arc::clone(&registry), bob.clone(), "bob".into(), 0).unwrap_err();
    assert_eq!(refused, crate::t("courses.generate.busy_other"));
    assert_eq!(check_capacity(&jobs, bob.user_id), Err(refused));
    assert!(jobs.get("bob").is_none(), "a refused job is not in the table");
    assert!(jobs.lock().waiting.is_empty());
    assert!(check_capacity(&jobs, ann.user_id).unwrap_err().contains("already have"), "one job each");
    assert!(!slot_free(&jobs));

    // Ann's account goes: her job leaves the table, but its thread is
    // still waiting on the model, so the slot stays taken.
    jobs.drop_user(ann.user_id);
    assert!((0..8).all(|i| jobs.get(&format!("ann-{i}")).is_none()));
    assert!(!slot_free(&jobs), "the slot is held until the thread exits");

    // The model answers: the threads end and free their slots.
    hold.store(false, Ordering::SeqCst);
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while !slot_free(&jobs) || jobs.get("bob").is_some_and(|j| !j.snapshot().done) {
        assert!(std::time::Instant::now() < deadline, "the slots were not freed");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(jobs.lock().live.is_empty());
    // With the slot free, Bob may start.
    assert!(check_capacity(&jobs, bob.user_id).is_ok());

    // Out of budget: no request is made, and the job says why.
    let job = test_job("c");
    let job = Job { deadline: Mutex::new(std::time::Instant::now()), ..job };
    let err = job.llm(&ann, &Call::new("generate-outline"), "s", "u").unwrap_err();
    assert!(err.contains("took longer than") && err.contains("resume"), "{err}");
}

/// After a restart (#874): a generation with the server's or a saved key
/// waits in line again and its course stays 'generating'; one with a pasted
/// key, and any revision, stops as before (Resume). The rows never hold a key.
#[test]
fn interrupted_generations_wait_again_unless_the_key_was_pasted() {
    let db = Db::open_in_memory().unwrap();
    let url = "http://127.0.0.1:9".to_string();
    let saved = generating(&db, "saved", "saved-course", url.clone());
    let pasted = generating(&db, "pasted", "pasted-course", url.clone());
    let mut revising = generating(&db, "rev", "rev-course", url);
    revising.revise = Some("shorter".into());
    let now = crate::db::now();
    let row = |req: &Request, id: &str, saved_key: bool| db.with(|c| hold(c, &payload(req, id, saved_key), now)).unwrap();
    let (a, b, c) = (row(&saved, "j", true), row(&pasted, "j", false), row(&revising, "j", true));
    let json: Vec<String> = db.with(|c| c.prepare("SELECT payload_json FROM jobs")?.query_map([], |r| r.get(0))?.collect()).unwrap();
    assert!(json.iter().all(|p| !p.contains("\"k\"")), "no key in a row: {json:?}");

    assert_eq!(db.with(|c| at_boot(c, now)).unwrap(), 1);
    let state_of = |id: i64| db.with(|c| c.query_row("SELECT state FROM jobs WHERE id = ?1", [id], |r| r.get::<_, String>(0))).unwrap();
    assert_eq!((state_of(a).as_str(), state_of(b).as_str(), state_of(c).as_str()), ("queued", "failed", "failed"));
    assert_eq!(db.with(|c| crate::courses::fail_interrupted(c)).unwrap(), 2, "the pasted-key and revision courses stop");
    let course = |id: &str| db.with(|c| crate::courses::course(c, id)).unwrap().unwrap();
    assert!(course("saved-course").is_generating());
    assert_eq!(course("pasted-course").state, crate::courses::CourseState::Failed);

    // Back in the table, waiting, so the learner's page follows it.
    let jobs = Jobs::default();
    assert_eq!(adopt(&jobs, &db).unwrap(), 1);
    assert_eq!(jobs.status(&jobs.get("j").unwrap()).phase, "Waiting", "until the runner starts it");
    assert!(check_capacity(&jobs, saved.user_id).is_err(), "it is still their job");
}

/// One AI job slot per account (#42): starting drops the account's
/// finished jobs and any expired ones; other accounts keep theirs.
#[test]
fn ai_jobs_hold_one_entry_per_account() {
    let jobs = Jobs::default();
    jobs.insert_finished_for_test("a1", 1, "c");
    jobs.lock().make_room(1);
    jobs.insert_finished_for_test("a2", 1, "c");
    assert_eq!(jobs.lock().map.len(), 1, "same user: one entry");
    jobs.lock().make_room(2);
    jobs.insert_finished_for_test("b1", 2, "c");
    assert_eq!(jobs.lock().map.len(), 2, "two users, two entries");
    // An expired job of another user is gone after the next insert.
    jobs.get("b1").unwrap().update(|s| s.finished_at = Some(crate::db::now() - JOB_TTL_SECS - 1));
    jobs.lock().make_room(1);
    assert!(jobs.get("b1").is_none() && jobs.get("a2").is_none());
}

/// A fake OpenAI-compatible server: answers the outline request with a
/// two-unit maths course and each lesson request with a lesson for the
/// slug it asks about, except the slugs in `broken` (garbage). Records
/// every request body.
fn fake_llm(broken: Vec<&'static str>) -> (String, Arc<Mutex<Vec<String>>>) {
    use std::io::{BufRead, BufReader, Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut len = 0usize;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0u8; len];
            reader.read_exact(&mut body).unwrap();
            let body = String::from_utf8(body).unwrap();
            log.lock().unwrap().push(body.clone());
            let req: serde_json::Value = serde_json::from_str(&body).unwrap();
            let prompt = req["messages"].as_array().unwrap().last().unwrap()["content"].as_str().unwrap().to_string();
            let reply = if prompt.contains("Design a course") {
                serde_json::json!({
                    "course": { "id": "placeholder", "title": "Quadratics", "subject": "Algebra", "description": "d", "icon": "📐",
                        "terms": { "label": "Term", "lang": "" }, "meanings": { "label": "Meaning", "lang": "" },
                        "units": [ { "id": "1", "title": "Factoring" }, { "id": "2", "title": "The formula" } ],
                        "groups": [ { "id": "core", "title": "Core" } ] },
                    "lessons": [
                        { "id": "u1-factor", "slug": "factor", "title": "Factoring", "unit": "1", "group": "core", "prerequisites": [] },
                        { "id": "u1-roots", "slug": "roots", "title": "Roots", "unit": "1", "group": "core", "prerequisites": ["u1-factor"] },
                        { "id": "u2-formula", "slug": "formula", "title": "The formula", "unit": "2", "group": "core", "prerequisites": [] }
                    ]
                })
                .to_string()
            } else {
                let slug = after_reference(&prompt).split("\"slug\": \"").nth(1).and_then(|r| r.split('"').next()).unwrap_or("x").to_string();
                if broken.contains(&slug.as_str()) {
                    "Sorry, I cannot do that.".to_string()
                } else {
                    serde_json::json!({
                        "id": "ignored", "slug": slug, "title": "ignored",
                        "content": "## Idea\n\nFactor x² + 5x + 6.\n\n## Check\n\nMultiply back.",
                        "vocabulary": [ { "term": "root", "meaning": "a value that makes the expression zero" } ],
                        "quiz": [
                            { "question_id": format!("{slug}-01"), "prompt": "Solve x^2 - 4 = 0", "type": "numeric", "correct_answer": "2; -2", "explanation": "x = ±2" },
                            { "question_id": format!("{slug}-02"), "prompt": "Expand (x+1)^2", "type": "expression", "correct_answer": "x^2 + 2x + 1" },
                            { "question_id": format!("{slug}-03"), "prompt": "Every quadratic has two real roots.", "type": "true-false", "correct_answer": "false" }
                        ]
                    })
                    .to_string()
                }
            };
            let out = serde_json::json!({ "choices": [ { "message": { "role": "assistant", "content": reply } } ] }).to_string();
            let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}", out.len());
        }
    });
    (url, seen)
}

fn test_job(course_id: &str) -> Job {
    Job {
        user_id: 1,
        course_id: course_id.into(),
        status: Mutex::new(Status {
            id: "j".into(),
            phase: String::new(),
            log: vec![],
            lessons_done: 0,
            lessons_total: 0,
            done: false,
            error: None,
            course_id: None,
            course_title: None,
            revision: false,
            adding: false,
            editing: false,
            lesson_title: None,
            translating: false,
            finished_at: None,
            notes: vec![],
        }),
        cancel: AtomicBool::new(false),
        deadline: Mutex::new(std::time::Instant::now() + JOB_BUDGET),
    }
}

#[test]
fn generates_a_maths_course_and_resumes_after_a_failure() {
    let db = Db::open_in_memory().unwrap();
    let registry = crate::courses::Courses::empty();
    let id = "quadratics-abc123";
    db.with(|c| {
        let uid = crate::auth::create_user(c, "ann@example.org", "h")?;
        crate::courses::create_generating(c, uid, id, "Quadratics", "factoring", "my notes: x^2+5x+6")?;
        crate::courses::set_generating(c, id, "j")
    })
    .unwrap();
    let (url, seen) = fake_llm(vec!["roots"]);
    let provider = Provider { kind: ProviderKind::OpenAi, base_url: url, api_key: "k".into(), model: "fake".into(), effort: None };
    let mut req = Request {
        user_id: 1,
        provider,
        course_id: id.into(),
        wish: "factoring for Friday".into(),
        lesson_count: 3,
        depth: Depth::Full,
        notes: "my notes: x^2+5x+6".into(),
        kind: SubjectKind::Math,
        instructions: "Custom rules.".into(),
        resume_outline: None,
        revise: None,
        add_lesson: None,
        edit: None,
        translate: None,
        i18n: 0,
        named: Default::default(),
    };
    // First run: the second lesson never comes back usable.
    let job = test_job(id);
    let err = run(&job, &db, &registry, &req).unwrap_err();
    assert!(err.contains("lesson 2"), "{err}");
    assert_eq!(job.snapshot().lessons_done, 1);
    {
        let bodies = seen.lock().unwrap();
        // outline + lesson 1 + three tries at lesson 2
        assert_eq!(bodies.len(), 5);
        let first: serde_json::Value = serde_json::from_str(&bodies[0]).unwrap();
        assert_eq!(first["messages"][0]["content"], "Custom rules.");
        assert!(bodies.iter().all(|b| b.contains("<material>") && b.contains("my notes")));
        assert!(bodies[0].contains("maths or quantitative"));
    }
    db.with(|c| crate::courses::finish_generation(c, id, Some(&err))).unwrap();
    let row = db.with(|c| crate::courses::course(c, id)).unwrap().unwrap();
    assert_eq!(row.title, "Quadratics");
    let outline = row.outline.clone().unwrap();
    assert_eq!(db.with(|c| crate::courses::file_count(c, id)).unwrap(), 2);

    // Resume with a working model: only the two missing lessons are asked for.
    let (url, seen) = fake_llm(vec![]);
    req.provider.base_url = url;
    req.resume_outline = Some(outline);
    req.instructions = String::new();
    db.with(|c| crate::courses::set_generating(c, id, "j2")).unwrap();
    let job = test_job(id);
    run(&job, &db, &registry, &req).unwrap();
    assert_eq!(seen.lock().unwrap().len(), 2);
    let first: serde_json::Value = serde_json::from_str(&seen.lock().unwrap()[0]).unwrap();
    assert_eq!(first["messages"][0]["content"], DEFAULT_INSTRUCTIONS);
    assert_eq!((job.snapshot().lessons_done, job.snapshot().lessons_total), (3, 3));
    db.with(|c| crate::courses::finish_generation(c, id, None)).unwrap();

    // The finished course loads, private and ready, with maths questions graded by value.
    let files = db.with(|c| crate::courses::load_ready_files(c, id)).unwrap();
    let store = ContentStore::from_files(&files).unwrap();
    assert_eq!(store.id(), id);
    assert_eq!(store.lessons().len(), 3);
    let q = &store.lessons()[0].quiz[0];
    assert!(crate::quiz::check_answer(q, "-2 and 2").correct);
    let row = db.with(|c| crate::courses::course(c, id)).unwrap().unwrap();
    assert_eq!(row.state, crate::courses::CourseState::Ready);
}

/// The generation flow against the checked-in fixtures, through the dev
/// simulator on loopback: each scenario is picked by a `[fixture:NAME]` token in the wish.
#[cfg(feature = "dev")]
#[test]
fn generates_from_the_llm_fixtures() {
    let db = Db::open_in_memory().unwrap();
    let registry = crate::courses::Courses::empty();
    let uid = db.with(|c| crate::auth::create_user(c, "fx@example.org", "h")).unwrap();
    let generate_named = |id: &str, wish: &str, named: crate::courses::Draft| {
        db.with(|c| {
            crate::courses::create_generating(c, uid, id, "New", wish, "")?;
            crate::courses::set_generating(c, id, "j")
        })
        .unwrap();
        let req = Request {
            user_id: uid,
            provider: crate::routes::simulated_provider(ProviderKind::Anthropic),
            course_id: id.into(),
            wish: wish.into(),
            lesson_count: 3,
            depth: Depth::Full,
            notes: String::new(),
            kind: SubjectKind::Auto,
            instructions: String::new(),
            resume_outline: None,
            revise: None,
            add_lesson: None,
            edit: None,
            translate: None,
            i18n: 0,
            named,
        };
        let job = test_job(id);
        let r = run(&job, &db, &registry, &req);
        db.with(|c| crate::courses::finish_generation(c, id, r.as_ref().err().map(String::as_str))).unwrap();
        (r, job.snapshot())
    };
    let generate = |id: &str, wish: &str| generate_named(id, wish, Default::default());

    let (r, s) = generate("fractions-ok", "fractions");
    r.unwrap();
    assert_eq!((s.lessons_done, s.lessons_total, s.course_title.as_deref()), (3, 3, Some("Fractions")));
    let files = db.with(|c| crate::courses::load_ready_files(c, "fractions-ok")).unwrap();
    assert_eq!(ContentStore::from_files(&files).unwrap().lessons().len(), 3);

    let (r, _) = generate("fractions-junk", "fractions [fixture:invalid-json]");
    assert!(r.unwrap_err().contains("no JSON object"));
    let (r, s) = generate("fractions-bad", "fractions [fixture:validation-fails]");
    assert!(r.unwrap_err().contains("lesson 2 did not validate after 3 attempts"));
    assert_eq!(s.lessons_done, 1);
    let (r, _) = generate("fractions-401", "fractions [fixture:provider-error]");
    assert!(r.unwrap_err().contains("HTTP 401"));
    let row = db.with(|c| crate::courses::course(c, "fractions-401")).unwrap().unwrap();
    assert_eq!(row.state, crate::courses::CourseState::Failed);

    // A title the learner gave the draft stays, whatever the outline says.
    let named = crate::courses::Draft { title: Some("Fractions for Friday".into()), lessons: vec!["Halves".into()] };
    let (r, s) = generate_named("fractions-named", "fractions", named);
    r.unwrap();
    assert_eq!(s.course_title.as_deref(), Some("Fractions for Friday"));
    let files = db.with(|c| crate::courses::load_ready_files(c, "fractions-named")).unwrap();
    assert_eq!(ContentStore::from_files(&files).unwrap().title(), "Fractions for Friday");
    let row = db.with(|c| crate::courses::course(c, "fractions-named")).unwrap().unwrap();
    assert_eq!(row.title, "Fractions for Friday");
}

/// Copy and translate (#630), against the fixtures: the starter course
/// copied and translated into Spanish file by file, ids and structure
/// kept, the plan's `done` list growing; a resume skips what is done.
#[cfg(feature = "dev")]
#[test]
fn translates_a_copy_from_the_llm_fixtures() {
    let db = Db::open_in_memory().unwrap();
    let registry = crate::courses::Courses::empty();
    let uid = db.with(|c| crate::auth::create_user(c, "tr@example.org", "h")).unwrap();
    let id = "untitled-course-es-abc123";
    let title = "Untitled course (Español)";
    let files = crate::courses::copied_files(&crate::courses::starter_files("orig", &crate::courses::untitled()), id, title).unwrap();
    let plan = crate::translate::Plan { from: "orig".into(), lang: "es".into(), learner: "Meaning".into(), taught: "Term".into(), ..Default::default() };
    let mut conn_plan = plan.clone();
    db.with(|c| {
        crate::courses::create_course(c, uid, id, title, &files)?;
        crate::courses::start_translation(c, id, "j", &plan.to_json())
    })
    .unwrap();
    let translate_with = |plan: &crate::translate::Plan| {
        let req = Request {
            user_id: uid,
            provider: crate::routes::simulated_provider(ProviderKind::Anthropic),
            course_id: id.into(),
            wish: String::new(),
            lesson_count: 2,
            depth: Depth::Full,
            notes: String::new(),
            kind: SubjectKind::Auto,
            instructions: String::new(),
            resume_outline: None,
            revise: None,
            add_lesson: None,
            edit: None,
            translate: Some(Translation { plan: plan.clone() }),
            i18n: 0,
            named: Default::default(),
        };
        let job = test_job(id);
        let r = translate(&job, &db, &registry, &req, plan);
        (r, job.snapshot())
    };
    let (r, s) = translate_with(&plan);
    r.unwrap();
    assert_eq!((s.lessons_done, s.lessons_total), (2, 2));
    let files = db.with(|c| crate::courses::load_files(c, id)).unwrap();
    let store = ContentStore::from_files(&files).unwrap();
    assert_eq!((store.id(), store.title()), (id, title));
    assert_eq!(store.manifest().meanings.lang, "es");
    assert_eq!(store.manifest().units.first().map(|u| u.title.as_str()), Some("Unidad 1"), "{:?}", s.log);
    let lesson = store.lessons().first().unwrap();
    assert_eq!((lesson.id.as_str(), lesson.title.as_str()), ("u1-first-lesson", "Tu primera lección"));
    let saved = db.with(|c| crate::courses::notes_of(c, id)).unwrap();
    conn_plan.done = vec![MANIFEST_FILE.into(), "lessons/first-lesson.json".into()];
    assert_eq!(crate::translate::Plan::parse(&saved), Some(conn_plan.clone()));
    // Resumed with everything done: nothing is asked again.
    let (r, s) = translate_with(&conn_plan);
    r.unwrap();
    assert_eq!((s.lessons_done, s.lessons_total), (2, 2));
    assert!(s.log.iter().all(|l| !l.contains("texts translated")), "{:?}", s.log);
}

/// The names a draft had go to the outline request.
#[test]
fn the_outline_is_told_the_names_of_a_draft() {
    assert_eq!(named_block(&Default::default()), "");
    let block = named_block(&crate::courses::Draft { title: Some("Rust".into()), lessons: vec!["Ownership".into(), "Borrowing".into()] });
    assert!(block.contains(r#"named the course "Rust""#), "{block}");
    assert!(block.contains(r#"in this order: "Ownership", "Borrowing""#), "{block}");
}

/// Each lesson request carries what the lessons before it teach (#446):
/// lesson 3's request names lesson 1's and 2's terms and sections, lesson 1's
/// names none, and the list stays within `arrange::DIGEST_CAP`.
#[test]
fn a_lesson_request_names_the_terms_of_the_lessons_before_it() {
    let db = Db::open_in_memory().unwrap();
    let registry = crate::courses::Courses::empty();
    let prompts = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen = Arc::clone(&prompts);
    let url = scripted_llm(
        move |prompt| {
            seen.lock().unwrap().push(prompt.to_string());
            if prompt.contains("Design a course") {
                return outline(3);
            }
            let slug = after_reference(prompt).split("\"slug\": \"").nth(1).and_then(|r| r.split('"').next()).unwrap_or("x").to_string();
            serde_json::json!({
                "id": "ignored", "slug": slug, "title": "ignored",
                "content": format!("## Idea of {slug}\n\nOne idea.\n\n## Practice {slug}\n\nTry it."),
                "vocabulary": [ { "term": format!("{slug}-alpha"), "meaning": "a" }, { "term": format!("{slug}-beta"), "meaning": "b" } ],
                "quiz": [ { "question_id": format!("{slug}-01"), "prompt": "True?", "type": "true-false", "correct_answer": "true" } ]
            })
            .to_string()
        },
        Arc::new(AtomicBool::new(false)),
    );
    let req = generating(&db, "dig", "dig", url);
    run(&test_job("dig"), &db, &registry, &req).unwrap();

    let prompts = prompts.lock().unwrap();
    assert_eq!(prompts.len(), 4, "the outline and three lessons");
    assert!(!prompts[1].contains("The lessons written so far"), "lesson 1 has nothing before it");
    let third = &prompts[3];
    for term in ["l1-alpha", "l1-beta", "l2-alpha", "l2-beta", "Idea of l1", "Practice l2"] {
        assert!(third.contains(term), "lesson 3's request lacks {term}");
    }
    assert!(!third.contains("l3-alpha"), "a lesson is not in its own digest");
    assert!(third.contains("don't teach their terms or sections again"));

    // The course map, with this lesson marked, and its neighbours in full.
    assert!(third.contains("this is lesson 3 of 3, lesson 3 of 3 in unit 1"), "{third}");
    assert!(third.contains("Unit 1 \"One\":\n  1. l1 \"Lesson 1\" (group core)"), "{third}");
    assert!(third.contains("3. l3 \"Lesson 3\" (group core)   <- this lesson"), "{third}");
    assert!(third.contains("The lesson just before this one (l2), in full:\n{"), "{third}");
    assert!(third.contains(r###""content": "## Idea of l2"###), "the whole file of lesson 2: {third}");
    assert!(!third.contains("just after"), "the last lesson has nothing after it");
    let second = &prompts[2];
    assert!(second.contains("The lesson just before this one (l1), in full:"), "{second}");
    assert!(second.contains("The lesson just after this one (l3) is not written yet. Its plan:\n{"), "{second}");
    assert!(second.contains("Pick up where the lesson before leaves off, and lead into the lesson after."));
    assert!(!prompts[1].contains("just before"), "lesson 1 has nothing before it");
    assert!(prompts[1].contains("Lead into the lesson after."));

    // On Resume a later lesson may be written already: it comes in full.
    let entries: Vec<serde_json::Value> = serde_json::from_str::<serde_json::Value>(&outline(3)).unwrap()["lessons"].as_array().unwrap().clone();
    let l3 = CourseFile { path: "lessons/l3.json".into(), content: r###"{ "id": "l3", "content": "## Third" }"###.into() };
    let context = crate::arrange::lesson_context(r#"{ "units": [] }"#, &entries, 1, &[l3]);
    assert!(context.contains("The lesson just after this one (l3), in full:\n{ \"id\": \"l3\""), "{context}");
    assert!(context.contains("The lesson just before this one (l1) is not written yet."), "{context}");
    assert!(context.contains("Unit 1 \"\":"), "a unit the manifest lacks still shows: {context}");

    // A long course: the list is capped, the newest lesson stays whole.
    let files: Vec<CourseFile> = (1..=40)
        .map(|i| {
            let vocabulary: Vec<serde_json::Value> = (0..14).map(|t| serde_json::json!({ "term": format!("term-{i}-{t}-ñandú") })).collect();
            let content = format!("## First heading of lesson {i}\n\nx\n\n## Second heading of lesson {i}\n\ny");
            let v = serde_json::json!({ "title": format!("Lesson {i}"), "vocabulary": vocabulary, "content": content });
            CourseFile { path: format!("lessons/l{i}.json"), content: v.to_string() }
        })
        .collect();
    let digest = crate::arrange::written_digest(&files);
    let list = digest.split_once(":\n").map(|(_, rest)| rest).unwrap_or_default().split("\n\n").next().unwrap_or_default();
    assert!(list.len() <= crate::arrange::DIGEST_CAP, "{} bytes", list.len());
    assert!(list.contains("earlier lessons are left out"), "{list}");
    assert!(list.contains("term-40-13-ñandú; sections First heading of lesson 40"), "{list}");
    assert!(!list.contains("Lesson 1:"), "the oldest lesson goes first");
    assert_eq!(crate::arrange::written_digest(&[]), "");
}

/// Revising a finished course from feedback, against the fixtures: only
/// the planned lessons are written, the rest stay byte for byte, nothing
/// is saved when the run fails, and Undo puts the old files back.
#[cfg(feature = "dev")]
#[test]
fn revises_a_course_from_feedback_and_undoes_it() {
    let db = Db::open_in_memory().unwrap();
    let registry = crate::courses::Courses::empty();
    let id = "fractions-rev";
    let uid = db.with(|c| crate::auth::create_user(c, "rv@example.org", "h")).unwrap();
    db.with(|c| {
        crate::courses::create_generating(c, uid, id, "New", "fractions", "")?;
        crate::courses::set_generating(c, id, "j")
    })
    .unwrap();
    let provider = crate::routes::simulated_provider(ProviderKind::Anthropic);
    let mut req = Request {
        user_id: uid,
        provider,
        course_id: id.into(),
        wish: "fractions".into(),
        lesson_count: 3,
        depth: Depth::Full,
        notes: String::new(),
        kind: SubjectKind::Auto,
        instructions: String::new(),
        resume_outline: None,
        revise: None,
        add_lesson: None,
        edit: None,
        translate: None,
        i18n: 0,
        named: Default::default(),
    };
    run(&test_job(id), &db, &registry, &req).unwrap();
    db.with(|c| crate::courses::finish_generation(c, id, None)).unwrap();
    let files = |db: &Db| {
        let mut f = db.with(|c| crate::courses::load_files(c, id)).unwrap();
        f.sort_by(|a, b| a.path.cmp(&b.path));
        f.into_iter().map(|f| (f.path, f.content)).collect::<Vec<_>>()
    };
    let original = files(&db);
    let revise_with = |feedback: &str, req: &mut Request| {
        req.revise = Some(feedback.into());
        assert!(db.with(|c| crate::courses::set_revising(c, id, "r")).unwrap());
        let job = test_job(id);
        job.update(|s| s.revision = true);
        let r = revise(&job, &db, &registry, req, feedback);
        db.with(|c| crate::courses::finish_revision(c, id)).unwrap();
        (r, job.snapshot())
    };

    // A plan that can't be read, or changes nothing, leaves the course as it was.
    let (r, _) = revise_with("whatever [fixture:invalid-json]", &mut req);
    assert!(r.unwrap_err().contains("The course is unchanged"));
    let (r, _) = revise_with("looks fine [fixture:nothing]", &mut req);
    assert!(r.unwrap_err().starts_with("Nothing changed"));
    assert_eq!(files(&db), original);
    assert!(db.with(|c| crate::courses::undo_step(c, id)).unwrap().is_none());

    // The default plan rewrites one lesson and adds one; the others stay byte for byte.
    let (r, s) = revise_with("more worked examples, and word problems", &mut req);
    r.unwrap();
    assert_eq!((s.lessons_done, s.lessons_total), (2, 2));
    let revised = files(&db);
    let get = |set: &[(String, String)], path: &str| set.iter().find(|(p, _)| p == path).map(|(_, c)| c.clone());
    for kept in ["course.json", "lessons/what-is-a-fraction.json", "lessons/adding.json"] {
        assert_eq!(get(&revised, kept), get(&original, kept), "{kept}");
    }
    assert!(get(&revised, "lessons/simplifying.json").unwrap().contains("rewritten from your feedback"));
    assert!(get(&revised, "lessons/word-problems.json").unwrap().contains("\"u2-add\""));
    let store = ContentStore::from_files(&db.with(|c| crate::courses::load_files(c, id)).unwrap()).unwrap();
    let order: Vec<&str> = store.lessons().iter().map(|l| l.id.as_str()).collect();
    assert_eq!(order, ["u1-simplify", "u1-what", "u2-add", "u2-word"]);
    let row = db.with(|c| crate::courses::course(c, id)).unwrap().unwrap();
    assert_eq!((row.state, row.error.as_deref()), (crate::courses::CourseState::Ready, None));
    assert!(row.outline.unwrap().contains("u2-word"));
    let step = db.with(|c| crate::courses::undo_step(c, id)).unwrap().unwrap();
    assert_eq!((step.label.as_str(), step.steps), ("Changed with feedback: \u{201c}more worked examples, and word problems\u{201d}", 1));

    // Undo puts back exactly the files from before, and only once.
    assert!(db.with(|c| crate::courses::undo_revision(c, id)).unwrap().is_some());
    assert_eq!(files(&db), original);
    assert!(db.with(|c| crate::courses::undo_revision(c, id)).unwrap().is_none());

    // A lesson left out of the plan is removed, with no lesson requests at all.
    let (r, s) = revise_with("drop adding [fixture:remove]", &mut req);
    r.unwrap();
    assert_eq!(s.lessons_total, 0);
    let after = files(&db);
    assert!(get(&after, "lessons/adding.json").is_none());
    assert_eq!(get(&after, "lessons/simplifying.json"), get(&original, "lessons/simplifying.json"));

    // A hand-made course (no brief, no outline) can be changed overall too.
    let hand = "fractions-hand";
    let copy: Vec<CourseFile> = original.iter().map(|(path, content)| CourseFile { path: path.clone(), content: content.clone() }).collect();
    db.with(|c| crate::courses::create_course(c, uid, hand, "By hand", &copy)).unwrap();
    let row = db.with(|c| crate::courses::course(c, hand)).unwrap().unwrap();
    assert_eq!((row.source.as_str(), row.brief.as_str()), ("hand", ""));
    assert!(row.can_revise());
    req.course_id = hand.into();
    assert!(db.with(|c| crate::courses::set_revising(c, hand, "h")).unwrap());
    let job = test_job(hand);
    job.update(|s| s.revision = true);
    revise(&job, &db, &registry, &req, "more worked examples").unwrap();
    db.with(|c| crate::courses::finish_revision(c, hand)).unwrap();
    let revised = db.with(|c| crate::courses::read_file(c, hand, "lessons/simplifying.json")).unwrap().unwrap();
    assert!(revised.contains("rewritten from your feedback"));
    assert!(db.with(|c| crate::courses::undo_step(c, hand)).unwrap().is_some());
}

/// Edit lessons (#1240), against the fixtures: the planned lessons are
/// rewritten (and may be renamed), the rest stay byte for byte, a plan that
/// adds, drops or moves a lesson is refused, and Undo puts the old files back.
#[cfg(feature = "dev")]
#[test]
fn edits_lessons_from_feedback_and_undoes_it() {
    let db = Db::open_in_memory().unwrap();
    let registry = crate::courses::Courses::empty();
    let id = "fractions-edit";
    let uid = db.with(|c| crate::auth::create_user(c, "ed@example.org", "h")).unwrap();
    db.with(|c| {
        crate::courses::create_generating(c, uid, id, "New", "fractions", "")?;
        crate::courses::set_generating(c, id, "j")
    })
    .unwrap();
    let mut req = Request {
        user_id: uid,
        provider: crate::routes::simulated_provider(ProviderKind::Anthropic),
        course_id: id.into(),
        wish: "fractions".into(),
        lesson_count: 3,
        depth: Depth::Full,
        notes: String::new(),
        kind: SubjectKind::Auto,
        instructions: String::new(),
        resume_outline: None,
        revise: None,
        add_lesson: None,
        edit: None,
        translate: None,
        i18n: 0,
        named: Default::default(),
    };
    run(&test_job(id), &db, &registry, &req).unwrap();
    db.with(|c| crate::courses::finish_generation(c, id, None)).unwrap();
    let files = |db: &Db, id: &str| {
        let mut f = db.with(|c| crate::courses::load_files(c, id)).unwrap();
        f.sort_by(|a, b| a.path.cmp(&b.path));
        f.into_iter().map(|f| (f.path, f.content)).collect::<Vec<_>>()
    };
    let original = files(&db, id);
    // An outline asked for does not apply: an edit writes full lessons.
    req.depth = Depth::Outline;
    let edit_with = |feedback: &str, req: &mut Request| {
        req.edit = Some(EditRun { feedback: feedback.into(), lesson: None });
        assert!(db.with(|c| crate::courses::set_revising(c, &req.course_id, "e")).unwrap());
        let job = test_job(&req.course_id);
        job.update(|s| (s.revision, s.editing, s.course_title) = (true, true, Some("Fractions".into())));
        assert_eq!(job.snapshot().heading(), "Editing \u{201c}Fractions\u{201d}\u{2026}");
        let r = edit::edit(&job, &db, &registry, req, feedback);
        db.with(|c| crate::courses::finish_revision(c, &req.course_id)).unwrap();
        job.update(|s| (s.done, s.error) = (true, r.as_ref().err().cloned()));
        (r, job.snapshot())
    };

    // Plans that break the rules, can't be read or change nothing leave the course as it was.
    for (feedback, says) in [
        ("[fixture:adds] and word problems", "an edit adds no lesson"),
        ("[fixture:drops] adding", "an edit removes no lesson"),
        ("[fixture:reorders] please", "an edit moves no lesson"),
        ("[fixture:invalid-json] whatever", "The course is unchanged"),
        ("[fixture:nothing] looks fine", "Nothing changed"),
    ] {
        let (r, s) = edit_with(feedback, &mut req);
        let e = r.unwrap_err();
        assert!(e.contains(says) && e.contains("The course is unchanged"), "{e}");
        assert_eq!(s.heading(), "Editing stopped");
    }
    assert_eq!(files(&db, id), original);
    assert!(db.with(|c| crate::courses::undo_step(c, id)).unwrap().is_none());

    // The default plan rewrites and renames one lesson; the others and course.json stay byte for byte.
    let (r, s) = edit_with("more worked examples, simpler words", &mut req);
    r.unwrap();
    assert_eq!((s.lessons_done, s.lessons_total, s.heading().as_str()), (1, 1, "Your changes are in"));
    let edited = files(&db, id);
    let get = |set: &[(String, String)], path: &str| set.iter().find(|(p, _)| p == path).map(|(_, c)| c.clone());
    assert_eq!(edited.iter().map(|(p, _)| p).collect::<Vec<_>>(), original.iter().map(|(p, _)| p).collect::<Vec<_>>());
    for kept in ["course.json", "lessons/what-is-a-fraction.json", "lessons/adding.json"] {
        assert_eq!(get(&edited, kept), get(&original, kept), "{kept}");
    }
    let simplify = get(&edited, "lessons/simplifying.json").unwrap();
    assert!(simplify.contains("rewritten from your feedback") && simplify.contains("\"title\": \"Simplifying fractions\""), "{simplify}");
    assert!(!simplify.contains("OUTLINE"), "{simplify}");
    let store = ContentStore::from_files(&db.with(|c| crate::courses::load_files(c, id)).unwrap()).unwrap();
    assert_eq!(store.lessons().iter().map(|l| l.id.as_str()).collect::<Vec<_>>(), ["u1-simplify", "u1-what", "u2-add"]);
    let row = db.with(|c| crate::courses::course(c, id)).unwrap().unwrap();
    assert_eq!((row.state, row.error.as_deref()), (crate::courses::CourseState::Ready, None));
    let step = db.with(|c| crate::courses::undo_step(c, id)).unwrap().unwrap();
    assert_eq!((step.label.as_str(), step.steps), ("Edited with feedback: \u{201c}more worked examples, simpler words\u{201d}", 1));
    assert!(db.with(|c| crate::courses::undo_revision(c, id)).unwrap().is_some());
    assert_eq!(files(&db, id), original);

    // Any course that loads: a hand-made one too.
    let hand = "fractions-hand-edit";
    let copy: Vec<CourseFile> = original.iter().map(|(path, content)| CourseFile { path: path.clone(), content: content.replace(&format!("\"{id}\""), &format!("\"{hand}\"")) }).collect();
    db.with(|c| crate::courses::create_course(c, uid, hand, "By hand", &copy)).unwrap();
    assert_eq!(db.with(|c| crate::courses::course(c, hand)).unwrap().unwrap().source, "hand");
    req.course_id = hand.into();
    edit_with("more worked examples", &mut req).0.unwrap();
    assert!(get(&files(&db, hand), "lessons/simplifying.json").unwrap().contains("rewritten from your feedback"));
}

/// Adding one lesson with AI, against the fixtures: it goes where the
/// reply says, the other lessons keep their content, a reply that cannot
/// be placed changes nothing, and each addition is its own Undo step.
#[cfg(feature = "dev")]
#[test]
fn adds_a_lesson_where_the_ai_puts_it_and_undoes_it() {
    let db = Db::open_in_memory().unwrap();
    let registry = crate::courses::Courses::empty();
    let id = "fractions-add";
    let uid = db.with(|c| crate::auth::create_user(c, "ad@example.org", "h")).unwrap();
    db.with(|c| {
        crate::courses::create_generating(c, uid, id, "New", "fractions", "")?;
        crate::courses::set_generating(c, id, "j")
    })
    .unwrap();
    let mut req = Request {
        user_id: uid,
        provider: crate::routes::simulated_provider(ProviderKind::Anthropic),
        course_id: id.into(),
        wish: "fractions".into(),
        lesson_count: 3,
        depth: Depth::Full,
        notes: String::new(),
        kind: SubjectKind::Auto,
        instructions: String::new(),
        resume_outline: None,
        revise: None,
        add_lesson: None,
        edit: None,
        translate: None,
        i18n: 0,
        named: Default::default(),
    };
    run(&test_job(id), &db, &registry, &req).unwrap();
    db.with(|c| crate::courses::finish_generation(c, id, None)).unwrap();
    let order = |db: &Db| {
        let files = db.with(|c| crate::courses::load_files(c, id)).unwrap();
        ContentStore::from_files(&files).unwrap().lessons().iter().map(|l| l.id.clone()).collect::<Vec<_>>()
    };
    let content = |db: &Db, path: &str| -> serde_json::Value {
        let mut v: serde_json::Value = serde_json::from_str(&db.with(|c| crate::courses::read_file(c, id, path)).unwrap().unwrap()).unwrap();
        v.as_object_mut().unwrap().remove("order");
        v
    };
    let before = order(&db);
    let original_simplify = content(&db, "lessons/simplifying.json");
    let add = |brief: &str, req: &mut Request| {
        req.add_lesson = Some(brief.into());
        db.with(|c| crate::courses::set_revising(c, id, "a")).unwrap();
        let job = test_job(id);
        job.update(|s| (s.revision, s.adding) = (true, true));
        let r = add_lesson(&job, &db, &registry, req, brief);
        db.with(|c| crate::courses::finish_revision(c, id)).unwrap();
        job.update(|s| (s.done, s.error) = (true, r.as_ref().err().cloned()));
        (r, job.snapshot())
    };

    // Replies that cannot be used change nothing.
    for (brief, says) in [("[fixture:invalid-json]", "no JSON object"), ("[fixture:bad-after]", "not a lesson of the course"), ("[fixture:taken]", "taken")] {
        let (r, s) = add(brief, &mut req);
        let e = r.unwrap_err();
        assert!(e.contains(says) && e.contains("The course is unchanged"), "{e}");
        assert_eq!(s.heading(), "Adding stopped");
    }
    assert_eq!(order(&db), before);
    assert!(db.with(|c| crate::courses::undo_step(c, id)).unwrap().is_none());

    // The default reply fits any course: it goes after the last lesson.
    let (r, s) = add("comparing two fractions", &mut req);
    r.unwrap();
    assert_eq!((s.lessons_done, s.lessons_total, s.heading().as_str()), (1, 1, "The new lesson is in"));
    let after = order(&db);
    assert_eq!(after.len(), 4);
    assert_eq!(&after[..3], &before[..]);
    assert!(after[3].starts_with("compare-"), "{after:?}");
    db.with(|c| crate::courses::undo_revision(c, id)).unwrap().unwrap();
    // `middle` goes right after "What a fraction is".
    add("comparing two fractions [fixture:middle]", &mut req).0.unwrap();
    let after = order(&db);
    let at = after.iter().position(|l| l == "u1-what").unwrap();
    assert_eq!(after[at + 1], "u1-compare");
    assert_eq!(after.len(), 4);
    // The others keep their content (only their place number is written down).
    assert_eq!(content(&db, "lessons/simplifying.json"), original_simplify);
    let row = db.with(|c| crate::courses::course(c, id)).unwrap().unwrap();
    assert!(row.outline.unwrap().contains("u1-compare"));
    assert_eq!(row.state, crate::courses::CourseState::Ready);
    // A second one goes first; both are Undo steps.
    add("why fractions [fixture:first]", &mut req).0.unwrap();
    assert_eq!(order(&db)[0], "u1-intro");
    let step = db.with(|c| crate::courses::undo_step(c, id)).unwrap().unwrap();
    assert_eq!((step.label.as_str(), step.steps), ("Added with AI: \u{201c}Why fractions\u{201d}", 2));
    db.with(|c| crate::courses::undo_revision(c, id)).unwrap().unwrap();
    db.with(|c| crate::courses::undo_revision(c, id)).unwrap().unwrap();
    assert_eq!(order(&db), before);
}

/// Every job run through `start` keeps what it changed for "Review AI
/// changes": the generation (every file new), then an added lesson (one
/// new file, the others as they were), replacing the run before.
#[cfg(feature = "dev")]
#[test]
fn each_ai_run_keeps_its_changes_for_review() {
    let db = Db::open_in_memory().unwrap();
    let registry = Arc::new(crate::courses::Courses::empty());
    let jobs = Jobs::default();
    let id = "review-runs";
    let uid = db.with(|c| crate::auth::create_user(c, "rv@example.org", "h")).unwrap();
    db.with(|c| {
        crate::courses::create_generating(c, uid, id, "New", "volcanoes", "")?;
        crate::courses::set_generating(c, id, "g")
    })
    .unwrap();
    let mut req = Request {
        user_id: uid,
        provider: crate::routes::simulated_provider(ProviderKind::OpenAi),
        course_id: id.into(),
        wish: "volcanoes".into(),
        lesson_count: 3,
        depth: Depth::Full,
        notes: String::new(),
        kind: SubjectKind::Auto,
        instructions: String::new(),
        resume_outline: None,
        revise: None,
        add_lesson: None,
        edit: None,
        named: Default::default(),
        translate: None,
        i18n: 0,
    };
    let finish = |jid: &str| {
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while !jobs.get(jid).unwrap().snapshot().done {
            assert!(std::time::Instant::now() < deadline, "job {jid} did not finish");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(jobs.get(jid).unwrap().snapshot().error, None);
        db.with(|c| crate::courses::ai_run(c, id)).unwrap().unwrap()
    };
    start(&jobs, db.clone(), Arc::clone(&registry), req.clone(), "g".into(), 0).unwrap();
    let run = finish("g");
    let files = db.with(|c| crate::courses::load_files(c, id)).unwrap();
    assert_eq!(run.label, "Generated with AI: \u{201c}volcanoes\u{201d}");
    assert_eq!(run.files.len(), files.len());
    assert!(run.files.iter().all(|f| f.before.is_none() && f.after.is_some()));

    req.add_lesson = Some("anything at all".into());
    assert!(db.with(|c| crate::courses::set_revising(c, id, "a")).unwrap());
    start(&jobs, db.clone(), Arc::clone(&registry), req.clone(), "a".into(), 0).unwrap();
    let run = finish("a");
    assert!(run.label.starts_with("Added with AI:"), "{}", run.label);
    let added: Vec<_> = run.files.iter().filter(|f| f.before.is_none()).collect();
    assert_eq!(added.len(), 1, "{:?}", run.files.iter().map(|f| &f.path).collect::<Vec<_>>());
    // Anything else it touched was there before, and is still there.
    assert!(run.files.iter().all(|f| f.after.is_some()));
    let review = crate::review::files(&run, &Default::default(), &Default::default(), &Default::default());
    assert!(review.iter().any(|f| f.kind == "added" && f.title == "Comparing fractions"), "add-lesson.json's default reply");
    // A hand edit afterwards does not change what the review shows.
    db.with(|c| crate::courses::write_file(c, id, &added[0].path, "{}")).unwrap();
    let again = db.with(|c| crate::courses::ai_run(c, id)).unwrap().unwrap();
    assert_eq!(again.files.iter().find(|f| f.path == added[0].path).unwrap().after, added[0].after);
}

#[test]
fn revision_plans_are_read_leniently_but_keep_ids() {
    let manifest = serde_json::json!({ "id": "c", "title": "T" });
    let mut lessons = HashMap::new();
    lessons.insert("a".to_string(), ("lessons/a.json".to_string(), serde_json::json!({ "id": "a", "slug": "a", "title": "A", "prerequisites": [] })));
    lessons.insert("b".to_string(), ("lessons/b.json".to_string(), serde_json::json!({ "id": "b", "slug": "b", "title": "B", "prerequisites": ["a"] })));
    let plan = serde_json::json!({ "course": { "id": "other", "title": "T2" }, "lessons": [
        { "id": "b", "slug": "renamed", "title": "B again", "change": "new", "prerequisites": ["a", "gone"] },
        { "id": "z", "slug": "z", "title": "Z", "change": "keep" },
    ] });
    let (m, slots) = read_plan(&plan, &lessons, "c", &manifest).unwrap();
    assert_eq!(m["id"].as_str(), Some("c"));
    // "new" for a lesson that exists is a rewrite, and it keeps its slug.
    assert_eq!((slots[0].change, slots[0].entry["slug"].as_str()), (Change::Rewrite, Some("b")));
    // Prerequisites that leave the course are dropped.
    assert_eq!(slots[0].entry["prerequisites"], serde_json::json!([]));
    // "keep" for an unknown id is a new lesson.
    assert_eq!(slots[1].change, Change::New);
    let twice = serde_json::json!({ "lessons": [ { "id": "a" }, { "id": "a" } ] });
    assert!(read_plan(&twice, &lessons, "c", &manifest).unwrap_err().contains("twice"));
    let clash = serde_json::json!({ "lessons": [ { "id": "n", "slug": "a", "change": "new" } ] });
    assert!(read_plan(&clash, &lessons, "c", &manifest).unwrap_err().contains("slug 'a'"));
    assert!(read_plan(&serde_json::json!({ "lessons": [] }), &lessons, "c", &manifest).is_err());
    assert_eq!(without_prerequisites(r#"{"prerequisites": ["a", "b"]}"#, |p| p == "b").unwrap().unwrap(), "{\n  \"prerequisites\": [\n    \"a\"\n  ]\n}\n");
    assert!(without_prerequisites(r#"{"prerequisites": ["a"]}"#, |p| p == "b").unwrap().is_none());
}

#[test]
fn failures_read_in_plain_words() {
    let key = plain_failure("the API answered HTTP 401. invalid x-api-key").unwrap();
    assert!(key.contains("API key") && key.contains("Manage Users"));
    assert!(plain_failure("lesson 2: the API answered HTTP 529. Overloaded").unwrap().contains("busy"));
    assert!(plain_failure("request failed: timed out after 900 s").unwrap().contains("in time"));
    assert!(plain_failure("the reply contains no JSON object").unwrap().contains("couldn't be used"));
    assert!(plain_failure("HTTP 200: reply is not JSON: <html>").unwrap().contains("couldn't be used"));
    assert!(plain_failure("lesson 2 did not validate after 3 attempts: quiz[0]").unwrap().contains("couldn't be used"));
    assert!(plain_failure("the API answered HTTP 404 (check the key, model name and base URL)").unwrap().contains("turned the request down"));
    // A used-up account reads as that, not as "busy" or "turned down" (#297).
    for raw in [
        "the API answered HTTP 400. You have reached your specified API usage limits. You will regain access on 2026-11-01 at 00:00 UTC.",
        "the API answered HTTP 400. Your credit balance is too low to access the Anthropic API. Please go to Plans & Billing to upgrade or purchase credits.",
        "lesson 3: the API answered HTTP 429. You exceeded your current quota, please check your plan and billing details.",
    ] {
        assert!(plain_failure(raw).unwrap().contains("run out of credit"), "{raw}");
        assert!(plain_revision_failure(raw).unwrap().contains("run out of credit"), "{raw}");
    }
    assert!(plain_failure("the API answered HTTP 429. rate limited").unwrap().contains("busy"));
    // Already written for a learner: shown as it is.
    assert_eq!(plain_failure("Cancelled. The lessons written so far are kept; resume to write the rest."), None);
    assert_eq!(plain_failure("Generation was interrupted when the server restarted. Resume it to write the remaining lessons."), None);
    // Adding or revising has no Resume: the course is unchanged.
    for raw in ["the API answered HTTP 401. bad key", "the reply contains no JSON object", "request failed: timed out after 900 s"] {
        let plain = plain_revision_failure(raw).unwrap();
        assert!(plain.ends_with("The course is unchanged.") && !plain.contains("Resume"), "{plain}");
    }
    assert_eq!(plain_revision_failure("Cancelled. The course is unchanged."), None);
}

#[test]
fn a_cancelled_job_stops_before_its_next_request() {
    let job = test_job("c");
    assert!(job.check_cancelled().is_ok());
    job.cancel();
    assert!(job.check_cancelled().unwrap_err().contains("Cancelled"));
    assert_eq!(job.snapshot().phase, "Cancelling…");
}

#[test]
fn extracts_json_from_chatty_replies() {
    assert_eq!(parse_json(r#"{"a":1}"#).unwrap()["a"], 1);
    assert_eq!(parse_json("Here you go:\n```json\n{\"a\": 2}\n```\nEnjoy!").unwrap()["a"], 2);
    assert!(parse_json("no json here").is_err());
    assert!(parse_json("{ broken").is_err());
}

#[test]
fn lesson_files_take_the_outline_identity() {
    let entry = serde_json::json!({ "id": "u1-a", "slug": "a-lesson", "title": "A", "unit": "1", "group": "g" });
    let v = serde_json::json!({ "id": "x", "slug": "Bad Slug", "title": "B", "content": "c" });
    let f = lesson_file(&v, &entry).unwrap();
    assert_eq!(f.path, "lessons/a-lesson.json");
    let back: serde_json::Value = serde_json::from_str(&f.content).unwrap();
    assert_eq!(back["id"], "u1-a");
    assert_eq!(back["unit"], "1");
    let v = serde_json::json!({ "slug": "Bad Slug" });
    assert!(lesson_file(&v, &serde_json::json!({})).is_err());
    assert!(supports_fallbacks("claude-opus-5-5") && !supports_fallbacks("claude-haiku-5-5"));
}

#[test]
fn chat_bodies_and_urls() {
    let msgs = vec![
        ChatMessage { role: "user", content: "hi".into() },
        ChatMessage { role: "user", content: "[note]".into() },
        ChatMessage { role: "assistant", content: "hello".into() },
        ChatMessage { role: "user", content: "next".into() },
    ];
    let merged = merge_roles(&msgs);
    assert_eq!(merged.len(), 3);
    assert_eq!(merged[0].content, "hi\n\n[note]");
    // A transcript that starts with the tutor gets a user opener.
    let m = merge_roles(&[ChatMessage { role: "assistant", content: "hello".into() }]);
    assert_eq!((m[0].role, m[1].role), ("user", "assistant"));

    let a = ProviderKind::Anthropic.provider("k".into());
    let body = anthropic_body(&a, "sys", &merged, 500);
    assert_eq!(body["system"], "sys");
    assert_eq!(body["max_tokens"], 500);
    assert_eq!(body["messages"].as_array().unwrap().len(), 3);
    assert_eq!(body["fallbacks"], "default");

    // The public base URLs (a test may have pointed the defaults at the dev simulator).
    let o = Provider { base_url: "https://api.openai.com".into(), ..ProviderKind::OpenAi.provider("k".into()) };
    let body = openai_body(&o, "sys", &merged, 500);
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["messages"].as_array().unwrap().len(), 4);
    assert_eq!(body["max_completion_tokens"], 500);
    assert_eq!(openai_url(&o), "https://api.openai.com/v1/chat/completions");
    let g = Provider { base_url: "https://generativelanguage.googleapis.com/v1beta/openai".into(), ..ProviderKind::Google.provider("k".into()) };
    assert_eq!(openai_url(&g), "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions");
    let local = Provider { base_url: "http://localhost:11434/v1/".into(), ..o.clone() };
    assert_eq!(openai_url(&local), "http://localhost:11434/v1/chat/completions");
    assert_eq!(ProviderKind::from_secret(crate::secrets::Provider::Google), ProviderKind::Google);
}

/// #918: the effort goes out as each provider names it, only when a job has
/// one, and Thorough gets room to think and reserves more.
#[test]
fn effort_goes_out_per_provider() {
    let msgs = [ChatMessage { role: "user", content: "hi".into() }];
    let a = ProviderKind::Anthropic.provider("k".into());
    assert!(anthropic_body(&a, "sys", &msgs, 500).get("output_config").is_none());
    assert!(openai_body(&ProviderKind::OpenAi.provider("k".into()), "sys", &msgs, 500).get("reasoning_effort").is_none());
    for (e, wire) in [(Effort::Fast, "low"), (Effort::Balanced, "medium"), (Effort::Thorough, "high")] {
        let a = Provider { effort: Some(e), ..ProviderKind::Anthropic.provider("k".into()) };
        assert_eq!(anthropic_body(&a, "sys", &msgs, 500)["output_config"]["effort"], wire);
        assert_eq!(a.model, ProviderKind::Anthropic.default_model(), "the same model at every level");
        for kind in [ProviderKind::OpenAi, ProviderKind::Google] {
            let o = Provider { effort: Some(e), ..kind.provider("k".into()) };
            assert_eq!(openai_body(&o, "sys", &msgs, 500)["reasoning_effort"], wire);
        }
        assert_eq!(Effort::parse(wire), Some(e));
    }
    assert_eq!(Effort::parse("max"), None);
    assert_eq!(Effort::default(), Effort::Balanced);
    let thorough = Provider { effort: Some(Effort::Thorough), ..a.clone() };
    assert_eq!(budget::max_tokens(&thorough), 2 * budget::max_tokens(&a));
    assert_eq!(Effort::Thorough.output_tokens(6_000), 12_000);
    assert_eq!(Effort::Fast.output_tokens(6_000), 6_000);
}

/// A server that echoes the key back (in a reply that is not JSON, or in
/// a JSON error), and one that is not there: the error the generator and
/// the tutor log never contains the key.
#[test]
fn model_errors_never_carry_the_key() {
    use std::io::{Read, Write};
    let key = "sk-ant-api03-SECRETSECRETSECRET-abcd";
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for (i, stream) in listener.incoming().enumerate() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = vec![0u8; 65536];
            let n = stream.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let echoed = req.lines().find_map(|l| l.split_once(": ").filter(|(k, _)| k.eq_ignore_ascii_case("x-api-key") || k.eq_ignore_ascii_case("authorization")).map(|(_, v)| v.to_string())).unwrap_or_default();
            let body = if i % 2 == 0 { format!("<html>bad gateway for {echoed}</html>") } else { format!("{{\"error\": {{\"message\": \"key {echoed} is wrong\"}}}}") };
            let status = if i % 2 == 0 { "200 OK" } else { "302 Found" };
            let _ = write!(stream, "HTTP/1.1 {status}\r\ncontent-type: text/plain\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
        }
    });
    let msgs = [ChatMessage { role: "user", content: "hola".into() }];
    let mut errors = Vec::new();
    for kind in [ProviderKind::Anthropic, ProviderKind::OpenAi] {
        let p = Provider { base_url: format!("http://{addr}"), ..kind.provider(key.to_string()) };
        for _ in 0..2 {
            errors.push(chat(&p, &Call::new("tutor-turn"), "sys", &msgs, 10).unwrap_err().to_string());
        }
        let gone = Provider { base_url: "http://127.0.0.1:9".into(), ..kind.provider(key.to_string()) };
        errors.push(chat(&gone, &Call::new("generate-lesson"), "sys", &msgs, 10).unwrap_err().to_string());
    }
    assert!(errors.iter().any(|e| e.contains("[your key]")), "the server did echo the key: {errors:?}");
    for e in &errors {
        assert!(!e.contains("SECRETSECRET"), "key material in an error: {e}");
    }
    assert_eq!(redact_key("short", "abc"), "short");
}

/// A lesson request is told what kind of course it writes and carries only
/// that kind's rules (#953): a general course the model chose as such gets
/// no language-only rules (spots in every sentence, builds, listening), and
/// the kind is saved with the outline for Resume, Revise and Only add.
#[test]
fn a_non_language_request_carries_no_language_rules() {
    let db = Db::open_in_memory().unwrap();
    let registry = crate::courses::Courses::empty();
    let prompts = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen = Arc::clone(&prompts);
    let url = scripted_llm(
        move |prompt| {
            seen.lock().unwrap().push(prompt.to_string());
            if prompt.contains("Design a course") {
                let mut o: serde_json::Value = serde_json::from_str(&outline(2)).unwrap();
                o["kind"] = "general".into();
                return o.to_string();
            }
            let slug = after_reference(prompt).split("\"slug\": \"").nth(1).and_then(|r| r.split('"').next()).unwrap_or("x").to_string();
            serde_json::json!({
                "id": "ignored", "slug": slug, "title": "ignored",
                "content": "## Parts\n\nManagers, firefighters and exiles.",
                "vocabulary": [ { "term": "exile", "meaning": "a part that carries pain" } ],
                "quiz": [ { "question_id": format!("{slug}-01"), "prompt": "Exiles carry pain.", "type": "true-false", "correct_answer": "true" } ]
            })
            .to_string()
        },
        Arc::new(AtomicBool::new(false)),
    );
    let req = generating(&db, "ifs", "ifs", url);
    assert_eq!(req.kind, SubjectKind::Auto);
    run(&test_job("ifs"), &db, &registry, &req).unwrap();

    let prompts = prompts.lock().unwrap();
    assert_eq!(prompts.len(), 3, "the outline and two lessons");
    // The outline still decides, so it sees every kind's rules.
    for heading in ["Rules for a language course", "Rules for a maths course", "Rules for a general course"] {
        assert!(prompts[0].contains(heading), "outline lacks {heading}");
    }
    for lesson in &prompts[1..] {
        assert!(lesson.contains("Rules for a general course"));
        assert!(lesson.contains("This is a general course: follow the general-course rules only."));
        assert!(!lesson.contains("Rules for a language course"), "a general lesson got the language rules");
        assert!(!lesson.contains("Rules for a maths course"), "a general lesson got the maths rules");
        assert!(!lesson.contains("2 to 5 `spots`"), "a general lesson was asked for language spots");
    }
    // The system prompt holds no kind's rules either.
    assert!(!DEFAULT_INSTRUCTIONS.contains("listening") && !DEFAULT_INSTRUCTIONS.contains("`builds`"));
    let row = db.with(|c| crate::courses::course(c, "ifs")).unwrap().unwrap();
    assert_eq!(SubjectKind::of_course(row.outline.as_deref(), "{}"), SubjectKind::General);
}

/// A course with no saved kind (from before #953) is a language course when
/// its manifest says so, and stays open otherwise.
#[test]
fn a_course_without_a_saved_kind_reads_it_from_the_manifest() {
    let language = r#"{"id": "c", "title": "Spanish", "terms": {"label": "Spanish", "lang": "es"}, "meanings": {"label": "English", "lang": "en"}, "units": [], "groups": []}"#;
    let general = r#"{"id": "c", "title": "IFS", "terms": {"label": "Term", "lang": ""}, "meanings": {"label": "Meaning", "lang": ""}, "units": [], "groups": []}"#;
    assert_eq!(SubjectKind::of_course(None, language), SubjectKind::Language);
    assert_eq!(SubjectKind::of_course(None, general), SubjectKind::Auto);
    assert_eq!(SubjectKind::of_course(Some(r#"{"kind": "math"}"#), language), SubjectKind::Math);
    assert!(!SubjectKind::Math.rules().contains("Rules for a language course"));
    assert!(!SubjectKind::Language.rules().contains("Rules for a general course"));
}

/// Preview (#1119): the course is one unit with one full lesson, whatever
/// the model planned; Outline: every lesson is asked for as an outline, and
/// the outline request lets the model pick the count. Both are saved with
/// the outline, so a resume writes the same kind of lessons.
#[test]
fn preview_and_outline_shape_the_course() {
    let db = Db::open_in_memory().unwrap();
    let registry = crate::courses::Courses::empty();
    let go = Arc::new(AtomicBool::new(false));
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let log = Arc::clone(&seen);
    let url = scripted_llm(
        move |prompt| {
            log.lock().unwrap().push(prompt.to_string());
            if prompt.contains("Design a course") {
                let mut o: serde_json::Value = serde_json::from_str(&outline(3)).unwrap();
                if !prompt.contains("Plan exactly 1 lesson") {
                    return o.to_string();
                }
                o["course"]["units"] = serde_json::json!([{ "id": "1", "title": "One" }, { "id": "2", "title": "Two" }]);
                o["lessons"][0]["unit"] = "2".into();
                o["lessons"][0]["prerequisites"] = serde_json::json!(["l3"]);
                return o.to_string();
            }
            // The lesson being written is the first one in the request; the
            // neighbours (#446) follow it in full, with their own slugs.
            let own = prompt.find("Write the complete lesson JSON file").map_or(prompt, |at| &prompt[at..]);
            let slug = own.split_once("\"slug\": \"").and_then(|(_, rest)| rest.split('"').next()).unwrap_or("l1");
            serde_json::json!({ "content": "## Ideas\n\n- one\n- two", "quiz": [ { "question_id": format!("{slug}-q1"), "type": "true-false", "prompt": "Yes?", "correct_answer": "true", "explanation": "e", "hint": "h" } ] }).to_string()
        },
        go,
    );

    let mut req = generating(&db, "pre", "preview-course", url.clone());
    req.depth = Depth::Preview;
    run(&test_job("preview-course"), &db, &registry, &req).unwrap();
    let files = db.with(|c| crate::courses::load_files(c, "preview-course")).unwrap();
    let store = ContentStore::from_files(&files).unwrap();
    assert_eq!(store.lessons().len(), 1);
    assert_eq!(store.manifest().units.iter().map(|u| u.id.as_str()).collect::<Vec<_>>(), ["2"]);
    let row = db.with(|c| crate::courses::course(c, "preview-course")).unwrap().unwrap();
    assert!(row.outline.unwrap().contains(r#""depth":"preview""#));
    {
        let prompts = seen.lock().unwrap();
        assert!(prompts[0].contains("Plan exactly 1 lesson, in exactly 1 unit"), "{}", prompts[0]);
        assert!(prompts[1..].iter().all(|p| !p.contains("OUTLINE lesson")));
    }

    seen.lock().unwrap().clear();
    let mut req = generating(&db, "out", "outline-course", url);
    req.depth = Depth::Outline;
    run(&test_job("outline-course"), &db, &registry, &req).unwrap();
    let files = db.with(|c| crate::courses::load_files(c, "outline-course")).unwrap();
    assert_eq!(ContentStore::from_files(&files).unwrap().lessons().len(), 3);
    let row = db.with(|c| crate::courses::course(c, "outline-course")).unwrap().unwrap();
    assert!(row.outline.unwrap().contains(r#""depth":"outline""#));
    let prompts = seen.lock().unwrap();
    assert!(prompts[0].contains(&format!("at most {MAX_LESSONS}")), "{}", prompts[0]);
    assert_eq!(prompts.len(), 4);
    assert!(prompts[1..].iter().all(|p| p.contains("OUTLINE lesson")));
}

/// A fact check (#1238): `check` in a plan parses (a kept lesson's entry,
/// whatever the plan wrote), a lesson reply may wrap the lesson with why
/// and check lines, and a check that finds nothing wrong keeps the file.
#[test]
fn check_plans_and_replies_are_read() {
    let manifest = serde_json::json!({ "id": "c", "title": "T" });
    let mut lessons = HashMap::new();
    lessons.insert("a".to_string(), ("lessons/a.json".to_string(), serde_json::json!({ "id": "a", "slug": "a", "title": "A", "prerequisites": [] })));
    let plan = serde_json::json!({ "lessons": [ { "id": "a", "slug": "a", "title": "Renamed", "change": "check" } ] });
    let (_, slots) = read_plan(&plan, &lessons, "c", &manifest).unwrap();
    assert_eq!((slots[0].change, slots[0].entry["title"].as_str()), (Change::Check, Some("A")));
    let files = [crate::arrange::LessonFile { path: "lessons/a.json".into(), id: "a".into(), value: lessons["a"].1.clone() }];
    let slots = edit::read_edit_plan(&plan, &files, &HashMap::new()).unwrap();
    assert_eq!((slots[0].change, slots[0].entry["title"].as_str()), (Change::Check, Some("A")));

    let wrapped = serde_json::json!({ "lesson": { "id": "a", "content": "x" }, "why": ["Fixed x", "  ", 3], "check": ["Check: maybe y"] });
    let (lesson, why, unsure) = check::read_reply(&wrapped);
    assert_eq!((lesson.unwrap()["content"].as_str(), why, unsure), (Some("x"), vec!["Fixed x".to_string()], vec!["maybe y".to_string()]));
    let bare = serde_json::json!({ "id": "a", "content": "x" });
    assert_eq!(check::read_reply(&bare), (Some(bare.clone()), vec![], vec![]));
    let (lesson, _, _) = check::read_reply(&serde_json::json!({ "lesson": null, "check": [] }));
    assert!(lesson.is_none());

    let old = CourseFile { path: "lessons/a.json".into(), content: "{\"id\": \"a\",  \"content\": \"x\"}\n".into() };
    let content = |f: Option<CourseFile>| f.map(|f| f.content);
    assert_eq!(content(check::unchanged_file(Some(&old), None)), Some(old.content.clone()), "nothing wrong: the file byte for byte");
    assert_eq!(content(check::unchanged_file(Some(&old), Some(&bare))), Some(old.content.clone()), "the same lesson back: byte for byte too");
    assert!(check::unchanged_file(Some(&old), Some(&serde_json::json!({ "id": "a", "content": "y" }))).is_none());
    let notes = vec![LessonNote { path: "p".into(), title: "T".into(), why: vec!["w".into()], check: vec![], changed: true }];
    assert_eq!(notes_from_json(&notes_to_json(&notes)), notes);
    assert_eq!((notes_to_json(&[]), notes_from_json("")), (String::new(), vec![]));
}

/// A fact check through Use AI (#1238), against the fixtures, run as a job
/// so the run is kept for review: the lesson with a wrong fact is fixed and
/// carries its why line, the unchanged ones are not written (and are not in
/// the run), and the one with a Check line is listed as looked at and left
/// alone. A check that finds nothing wrong changes nothing and says so.
#[cfg(feature = "dev")]
#[test]
fn a_fact_check_fixes_and_says_why() {
    let db = Db::open_in_memory().unwrap();
    let registry = Arc::new(crate::courses::Courses::empty());
    let jobs = Jobs::default();
    let id = "fractions-check";
    let uid = db.with(|c| crate::auth::create_user(c, "fc@example.org", "h")).unwrap();
    db.with(|c| {
        crate::courses::create_generating(c, uid, id, "New", "fractions", "")?;
        crate::courses::set_generating(c, id, "g")
    })
    .unwrap();
    let mut req = Request {
        user_id: uid,
        provider: crate::routes::simulated_provider(ProviderKind::Anthropic),
        course_id: id.into(),
        wish: "fractions".into(),
        lesson_count: 3,
        depth: Depth::Full,
        notes: String::new(),
        kind: SubjectKind::Auto,
        instructions: String::new(),
        resume_outline: None,
        revise: None,
        add_lesson: None,
        edit: None,
        named: Default::default(),
        translate: None,
        i18n: 0,
    };
    let finish = |jid: &str| {
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while !jobs.get(jid).unwrap().snapshot().done {
            assert!(std::time::Instant::now() < deadline, "job {jid} did not finish");
            std::thread::sleep(Duration::from_millis(20));
        }
        jobs.get(jid).unwrap().snapshot()
    };
    start(&jobs, db.clone(), Arc::clone(&registry), req.clone(), "g".into(), 0).unwrap();
    assert_eq!(finish("g").error, None);
    let before = db.with(|c| crate::courses::load_files(c, id)).unwrap();
    let read = |path: &str, set: &[CourseFile]| set.iter().find(|f| f.path == path).map(|f| f.content.clone());

    req.revise = Some("Check the facts in every lesson and fix what is wrong [fixture:check]".into());
    assert!(db.with(|c| crate::courses::set_revising(c, id, "c")).unwrap());
    start(&jobs, db.clone(), Arc::clone(&registry), req.clone(), "c".into(), 0).unwrap();
    let s = finish("c");
    assert_eq!((s.error, s.lessons_total), (None, 3));
    let after = db.with(|c| crate::courses::load_files(c, id)).unwrap();
    assert!(read("lessons/what-is-a-fraction.json", &after).unwrap().contains("3 of the 4 parts are taken"));
    for same in ["lessons/simplifying.json", "lessons/adding.json", "course.json"] {
        assert_eq!(read(same, &after), read(same, &before), "{same} is not written");
    }
    let run = db.with(|c| crate::courses::ai_run(c, id)).unwrap().unwrap();
    assert_eq!(run.files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), ["lessons/what-is-a-fraction.json"]);
    let mut files = crate::review::files(&run, &Default::default(), &Default::default(), &Default::default());
    let left_alone = crate::review::attach_notes(&mut files, &run.why);
    assert_eq!(files[0].why.len(), 1);
    assert!(files[0].why[0].starts_with("Changed \"3 parts are taken\""), "{:?}", files[0].why);
    assert_eq!(left_alone.len(), 1);
    assert_eq!((left_alone[0].title.as_str(), left_alone[0].changed), ("Simplifying", false));
    assert!(left_alone[0].check[0].starts_with("\"Multiply back"), "the \"Check:\" the model wrote is the label's: {:?}", left_alone[0].check);

    req.revise = Some("Check the facts [fixture:check-clean]".into());
    assert!(db.with(|c| crate::courses::set_revising(c, id, "n")).unwrap());
    start(&jobs, db.clone(), Arc::clone(&registry), req.clone(), "n".into(), 0).unwrap();
    assert_eq!(finish("n").error.as_deref(), Some(check::NOTHING_WRONG));
    assert_eq!(db.with(|c| crate::courses::ai_run(c, id)).unwrap().unwrap().why, run.why, "the run before stays for review");
}

/// The revise plan numbers the lessons as the lesson table does (1-based,
/// course order) and says what each one has, zeros included (#1237).
#[test]
fn the_revise_plan_numbers_the_lessons_and_counts_what_each_has() {
    let store = ContentStore::load_dir(std::path::Path::new("data/courses/spanish")).unwrap();
    let entries: Vec<serde_json::Value> = (store.lessons().iter().enumerate())
        .map(|(i, l)| prompt::plan_entry(i + 1, l, serde_json::json!({"id": l.id})))
        .collect();
    for (i, (e, l)) in entries.iter().zip(store.lessons()).enumerate() {
        assert_eq!(e["number"], i + 1);
        assert_eq!(e["id"], l.id.as_str());
        let has = e["has"].as_str().unwrap();
        assert!(has.starts_with(&format!("{} quiz questions, {} vocabulary items, ", l.quiz.len(), l.vocabulary.len())), "{has}");
    }
    assert_eq!(entries[0]["number"], 1);
    // A lesson with no sort says so, rather than leaving sorts out.
    let sorted = store.lessons().iter().find(|l| !l.sprint.sorts.is_empty()).expect("a lesson with a sort");
    assert!(!prompt::plan_entry(1, sorted, serde_json::json!({}))["has"].as_str().unwrap().contains(" 0 Sprint sorts"));
    let mut unsorted = sorted.clone();
    unsorted.sprint.sorts.clear();
    let has = prompt::plan_entry(1, &unsorted, serde_json::json!({}))["has"].as_str().unwrap().to_string();
    assert!(has.contains(", 0 Sprint sorts, "), "{has}");
}

/// Both plan prompts that change existing lessons, revise and edit, number the
/// lessons and say what each has, and say what "lesson N" means (#1237).
#[test]
fn the_revise_and_edit_plan_prompts_number_the_lessons() {
    let db = Db::open_in_memory().unwrap();
    let registry = crate::courses::Courses::empty();
    let prompts = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen = Arc::clone(&prompts);
    let url = scripted_llm(
        move |prompt| {
            seen.lock().unwrap().push(prompt.to_string());
            if prompt.contains("Design a course") {
                return outline(3);
            }
            if prompt.contains("Now they want") {
                return "no plan".into();
            }
            let slug = after_reference(prompt).split("\"slug\": \"").nth(1).and_then(|r| r.split('"').next()).unwrap_or("x").to_string();
            serde_json::json!({
                "id": "ignored", "slug": slug, "title": "ignored", "content": format!("## Idea of {slug}\n\nOne idea."),
                "vocabulary": [ { "term": format!("{slug}-alpha"), "meaning": "a" } ],
                "quiz": [ { "question_id": format!("{slug}-01"), "prompt": "True?", "type": "true-false", "correct_answer": "true" } ]
            })
            .to_string()
        },
        Arc::new(AtomicBool::new(false)),
    );
    let req = generating(&db, "num", "num", url);
    run(&test_job("num"), &db, &registry, &req).unwrap();
    db.with(|c| crate::courses::finish_generation(c, "num", None)).unwrap();
    assert!(revise(&test_job("num"), &db, &registry, &req, "add a sort to lesson 2").is_err());
    assert!(edit::edit(&test_job("num"), &db, &registry, &req, "add a sort to lesson 2").is_err());

    let prompts = prompts.lock().unwrap();
    let plans: Vec<&String> = prompts.iter().filter(|p| p.contains("Now they want")).collect();
    assert!(plans.iter().any(|p| p.contains("Now they want it changed")), "a revise plan was asked");
    assert!(plans.iter().any(|p| p.contains("Now they want its lessons edited")), "an edit plan was asked");
    for plan in plans {
        assert!(plan.contains(prompt::NUMBER_LINE), "{plan}");
        let list = plan.split("Its lessons, in order:\n").nth(1).and_then(|r| r.split("\n\n").next()).unwrap();
        let entries: Vec<serde_json::Value> = serde_json::from_str(list).unwrap();
        let numbers: Vec<u64> = entries.iter().map(|e| e["number"].as_u64().unwrap()).collect();
        assert_eq!(numbers, [1, 2, 3]);
        assert_eq!(entries[1]["id"], "l2");
        assert_eq!(entries[1]["has"], "1 quiz questions, 1 vocabulary items, 0 listening sentences, 0 conversations, 0 Sprint sorts, 0 spots, 0 builds");
    }
}

/// A generation prompt without the format reference that now opens it (the
/// reference holds example slugs a canned reply must not pick up).
fn after_reference(prompt: &str) -> &str {
    prompt.strip_prefix(prompt::reference().as_str()).unwrap_or(prompt)
}

/// Part A of #87: the format reference opens a generation request and is the
/// cacheable block on Anthropic; the varying text follows it.
#[test]
fn reference_is_the_first_cacheable_block() {
    let a = ProviderKind::Anthropic.provider("k".into());
    let rules = SubjectKind::General.rules();
    let first = [ChatMessage { role: "user", content: prompt::with_reference(&rules, "Write lesson one.") }];
    let second = [ChatMessage { role: "user", content: prompt::with_reference(&rules, "Write lesson two, which is different.") }];
    let (b1, b2) = (anthropic_body(&a, "sys", &first, 500), anthropic_body(&a, "sys", &second, 500));
    let blocks = b1["messages"][0]["content"].as_array().unwrap();
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0]["text"], prompt::reference());
    assert!(blocks[0]["text"].as_str().unwrap().contains(FORMAT_DOC));
    assert_eq!(blocks[0]["cache_control"], serde_json::json!({ "type": "ephemeral" }));
    assert!(blocks[1].get("cache_control").is_none());
    assert_eq!(blocks[1]["text"], format!("{rules}\n\nWrite lesson one."));
    // Two lessons of one job share every byte up to the end of the cached block.
    let (s1, s2) = (serde_json::to_string(&b1).unwrap(), serde_json::to_string(&b2).unwrap());
    let end = s1.find("\"cache_control\"").unwrap() + "\"cache_control\":{\"type\":\"ephemeral\"}".len();
    assert_eq!(s1[..end], s2[..end]);
    assert_ne!(s1, s2);
    // A plain message stays a string.
    let plain = [ChatMessage { role: "user", content: "hi".into() }];
    assert_eq!(anthropic_body(&a, "sys", &plain, 500)["messages"][0]["content"], "hi");
    // OpenAI-compatible providers get the same words as one string, reference first.
    let o = openai_body(&ProviderKind::OpenAi.provider("k".into()), "sys", &first, 500);
    assert_eq!(o["messages"][1]["content"], format!("{}{rules}\n\nWrite lesson one.", prompt::reference()));
}

// ---------- #85: the windows that can leave a course stuck ----------

/// The three records of one job, read the way the pages read them:
/// (course state, course job id, jobs row state, job in memory is done).
fn records(db: &Db, jobs: &Jobs, course: &str, row: i64, job: &str) -> (String, Option<String>, String, Option<bool>) {
    let (state, job_id) = db
        .with(|c| crate::courses::course(c, course).map(|r| r.map(|r| (r.state.as_str().to_string(), r.job_id)).unwrap()))
        .unwrap();
    let row_state = db.with(|c| c.query_row("SELECT state FROM jobs WHERE id = ?1", [row], |r| r.get::<_, String>(0))).unwrap();
    (state, job_id, row_state, jobs.get(job).map(|j| j.snapshot().done))
}

/// Wait until every generator thread of `jobs` has exited (the loop only
/// polls the table the threads leave; no outcome depends on how long it takes).
fn wait_for_slots(jobs: &Jobs) {
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while !slot_free(jobs) || !jobs.lock().live.is_empty() {
        assert!(std::time::Instant::now() < deadline, "the generator threads did not exit");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// `hold` for a job id, as `submit` does.
fn hold_row(c: &rusqlite::Connection, req: &Request, id: &str) -> rusqlite::Result<i64> {
    hold(c, &payload(req, id, false), crate::db::now())
}

/// Window 3 of the audit in `docs/architecture.md` (#85): two Generate posts
/// at once on one course. Both read a course that was not busy, so both call
/// `set_generating` before either job starts; one `start` wins, the other is
/// refused and the route then fails the course. The functions are driven in
/// that order, with no timing. The course must stay the winner's.
#[test]
fn a_second_generate_post_does_not_take_the_course_from_the_first() {
    let db = Db::open_in_memory().unwrap();
    let registry = Arc::new(crate::courses::Courses::empty());
    let jobs = Arc::new(Jobs::default());
    let hold_llm = Arc::new(AtomicBool::new(true));
    let url = scripted_llm(|_| "not JSON".into(), Arc::clone(&hold_llm));
    let req = generating(&db, "win3", "win3-course", url);
    // The learner's course before they press Generate: ready, not busy.
    db.with(|c| crate::courses::finish_generation(c, "win3-course", None)).unwrap();

    // Both requests passed the "not busy" check in `generate_in`; both mark
    // the course. The second is told the course is taken, and its route stops
    // there (before `restart_generation` and `ai::submit`).
    assert!(db.with(|c| crate::courses::set_generating(c, "win3-course", "first")).unwrap());
    assert!(!db.with(|c| crate::courses::set_generating(c, "win3-course", "second")).unwrap());
    // The first request's job is taken and starts.
    let row = db.with(|c| hold_row(c, &req, "first")).unwrap();
    start(&jobs, db.clone(), Arc::clone(&registry), req.clone(), "first".into(), row).unwrap();

    // Without the guard (the second route went on) `start` refused it, `submit`
    // settled its row and the route failed the course: the first job's course
    // came out `failed` with the second's job id.
    // A course a job is revising is just as taken.
    db.with(|c| c.execute("UPDATE courses SET state = 'revising' WHERE id = 'win3-course'", [])).unwrap();
    assert!(!db.with(|c| crate::courses::set_generating(c, "win3-course", "third")).unwrap());
    db.with(|c| c.execute("UPDATE courses SET state = 'generating' WHERE id = 'win3-course'", [])).unwrap();

    // The first job is still running; its course must say so, with its id.
    let (state, job_id, row_state, done) = records(&db, &jobs, "win3-course", row, "first");
    assert_eq!((row_state.as_str(), done), ("running", Some(false)));
    assert_eq!((state.as_str(), job_id.as_deref()), ("generating", Some("first")));

    hold_llm.store(false, Ordering::SeqCst);
    wait_for_slots(&jobs);
}

/// Window 7 (#85): the write that ends a job fails (a full disk, a locked
/// file). A trigger makes the course update fail, then a real generator
/// thread runs a job that fails at once. Records today's behaviour: the
/// thread logs the error and goes on, so the course stays busy, the jobs row
/// stays `running`, and only memory says the job is done. Nothing in the
/// running server moves the course on from there; the way out is the Cancel
/// button (`generate_cancel`, a generation only) or a restart. Fixing it
/// needs the one end-of-job transaction of #85 part 2, so this test passes
/// by describing the stuck state.
#[test]
fn window_7_a_failed_end_of_job_write_leaves_the_course_busy() {
    let db = Db::open_in_memory().unwrap();
    let registry = Arc::new(crate::courses::Courses::empty());
    let jobs = Arc::new(Jobs::default());
    let url = scripted_llm(|_| "not JSON".into(), Arc::new(AtomicBool::new(false)));
    let req = generating(&db, "win7", "win7-course", url);
    db.with(|c| {
        c.execute_batch(
            "CREATE TRIGGER fail_the_end BEFORE UPDATE OF state ON courses WHEN NEW.state IN ('ready', 'failed')
             BEGIN SELECT RAISE(ABORT, 'disk full'); END;",
        )
        .map(drop)
    })
    .unwrap();
    let row = db.with(|c| hold_row(c, &req, "j7")).unwrap();
    start(&jobs, db.clone(), registry, req, "j7".into(), row).unwrap();
    wait_for_slots(&jobs);

    // Window 7: busy course, running row, finished job in memory.
    let (state, job_id, row_state, done) = records(&db, &jobs, "win7-course", row, "j7");
    assert_eq!((state.as_str(), job_id.as_deref()), ("generating", Some("j")));
    assert_eq!((row_state.as_str(), done), ("running", Some(true)));
    // The slot is free, so a second job is not blocked by this one.
    assert!(slot_free(&jobs));
    // The way out for a generation: with the job done in memory, Cancel
    // finds no live job and fails the course (`generate_cancel`).
    db.with(|c| c.execute_batch("DROP TRIGGER fail_the_end").map(drop)).unwrap();
    db.with(|c| crate::courses::finish_generation(c, "win7-course", Some("cancelled"))).unwrap();
    assert_eq!(records(&db, &jobs, "win7-course", row, "j7").0, "failed");
}

/// Window 9 (#85): a generator thread dies without its end sequence. In the
/// release build a panic aborts the whole process (`panic = "abort"` in
/// `Cargo.toml`, held by `scripts/panic-check.sh`), so a thread cannot die
/// while the server lives; what is left is a server that stopped mid-job. And
/// the panic cannot be reproduced in a test build either: an SQL function
/// that panics is caught by rusqlite and becomes an ordinary job failure, and
/// no other code in the job path panics. So this test builds the state a dead
/// thread leaves (course busy, row running, nothing in memory) and shows that
/// the next boot ends it (`at_boot`, then `fail_interrupted`, as `main` does).
#[test]
fn window_9_what_a_job_that_died_without_its_end_sequence_leaves_is_ended_at_boot() {
    let db = Db::open_in_memory().unwrap();
    let jobs = Jobs::default();
    let req = generating(&db, "win9", "win9-course", "http://127.0.0.1:1/v1".into());
    let row = db.with(|c| hold_row(c, &req, "j9")).unwrap();
    // As the pages see it before the boot: busy course, running row, no job in memory.
    assert_eq!(records(&db, &jobs, "win9-course", row, "j9"), ("generating".into(), Some("j".into()), "running".into(), None));

    // A pasted key (`hold_row` passes `saved_key = false`): not resumable, so it stops.
    db.with(|c| at_boot(c, crate::db::now())).unwrap();
    db.with(|c| crate::courses::fail_interrupted(c)).unwrap();
    let (state, job_id, row_state, _) = records(&db, &jobs, "win9-course", row, "j9");
    assert_eq!((state.as_str(), job_id.as_deref(), row_state.as_str()), ("failed", Some("j"), "failed"));
    let error = db.with(|c| crate::courses::course(c, "win9-course")).unwrap().unwrap().error;
    assert!(error.is_some_and(|e| e.contains("interrupted")));
}
