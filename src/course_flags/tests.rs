use super::*;
use crate::content::CourseFile;
use crate::db::Db;
use std::net::SocketAddr;
use std::path::Path;

fn lesson(body: &str) -> Vec<CourseFile> {
    vec![
        CourseFile { path: "course.json".into(), content: r#"{"id": "c", "title": "My course"}"#.into() },
        CourseFile { path: "lessons/colours.json".into(), content: serde_json::json!({"title": "Colours", "sections": [{"body": body}]}).to_string() },
    ]
}

fn found(body: &str) -> Vec<String> {
    terms::scan(&lesson(body)).into_iter().map(|m| m.term).collect()
}

#[test]
fn terms_match_whole_words_in_any_case_and_without_accents() {
    assert_eq!(found("You SPIC."), vec!["spic"]);
    assert_eq!(found("los sudácas"), vec!["sudacas"], "accents are folded");
    assert_eq!(found("He said: sieg   Heil!"), vec!["sieg heil"], "a term of two words, any spacing");
    assert!(found("spicy food, a spick-and-span room").is_empty(), "part of a word does not count");
    assert!(found("sieg alone, heil alone").is_empty());
    // Ordinary words of the languages taught are not on the list.
    assert!(found("El gato es negro. Voy a coger el autobús. Je suis en retard.").is_empty());
}

#[test]
fn a_match_names_the_file_the_field_and_the_lesson() {
    let m = terms::scan(&lesson("one two three four five six seven eight gook nine ten eleven twelve thirteen fourteen fifteen"));
    assert_eq!(m.len(), 1);
    assert_eq!((m[0].path.as_str(), m[0].field.as_str(), m[0].lesson.as_str()), ("lessons/colours.json", "/sections/0/body", "Colours"));
    assert_eq!(m[0].text, "...three four five six seven eight gook nine ten eleven twelve thirteen fourteen...");
}

/// The list flags nothing in the courses that come with the app.
#[test]
fn the_built_in_courses_have_no_listed_term() {
    for root in ["data/courses", "data/test-courses"] {
        for dir in std::fs::read_dir(root).unwrap() {
            let dir = dir.unwrap().path();
            let files = crate::content::read_course_dir(Path::new(&dir)).unwrap();
            let hits = terms::scan(&files);
            assert!(hits.is_empty(), "{}: {hits:?}", dir.display());
        }
    }
}

fn course(c: &mut Connection, id: &str, files: &[CourseFile]) -> i64 {
    let uid = crate::auth::create_user(c, &format!("{id}@example.org"), "x").unwrap();
    crate::courses::create_course(c, uid, id, "T", files).unwrap();
    uid
}

#[test]
fn the_check_flags_clears_and_keeps_an_override() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let bad = lesson("hola gook");
        course(c, "c", &bad);
        let f = check(c, "c", &bad)?.unwrap();
        assert!(f.blocks());
        assert_eq!((f.source, f.terms()), (Source::Terms, vec!["gook"]));
        assert!(crate::courses::load_ready_files(c, "c")?.is_empty(), "a flagged course opens for nobody");

        // The same files are not checked again.
        c.execute("UPDATE course_flags SET checked_at = 1", [])?;
        assert_eq!(check(c, "c", &bad)?.unwrap().checked_at, 1);

        // Fixed: the flag clears and the course opens.
        let good = lesson("hola amigo");
        let f = check(c, "c", &good)?.unwrap();
        assert_eq!(f.state, State::Clear);
        assert!(f.matches.is_empty());

        // A superuser let "gook" through: the same term stays let through,
        // a new one flags it again, and the override's note is kept.
        check(c, "c", &bad)?;
        c.execute("UPDATE course_flags SET state = 'overridden', override_note = 'a quote', overridden_at = 5", [])?;
        let f = check(c, "c", &lesson("gook again"))?.unwrap();
        assert_eq!(f.state, State::Overridden);
        let f = check(c, "c", &lesson("gook and kike"))?.unwrap();
        assert!(f.blocks());
        assert_eq!((f.override_note.as_deref(), f.overridden_at), (Some("a quote"), Some(5)));

        // The AI scan's flag stays when no term matches: the AI looks again (#1036).
        record(c, "c", State::Flagged, Source::Ai, &[], "the AI's report", "old", 1)?;
        let f = check(c, "c", &good)?.unwrap();
        assert_eq!((f.state, f.source, f.reason.as_str()), (State::Flagged, Source::Ai, "the AI's report"));
        Ok(())
    })
    .unwrap();
}

/// Over HTTP: a flagged course shows its owner a red ! that names the word
/// and the lesson; Start and Share are refused; it is gone from the list of
/// the account it is shared with; fixing it brings everything back.
#[test]
fn a_flagged_course_does_not_start_or_share_until_fixed() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let state = crate::routes::test_state(false, None);
    let app = crate::routes::router(state.clone());
    let base = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await.unwrap() });
        format!("http://{addr}")
    });
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    let get = |path: &str, cookie: &str| {
        let mut r = agent.get(&format!("{base}{path}")).header("cookie", cookie).call().unwrap();
        (r.status().as_u16(), r.body_mut().read_to_string().unwrap())
    };
    let post = |path: &str, cookie: &str, form: &[(&str, &str)]| {
        let r = agent.post(&format!("{base}{path}")).header("cookie", cookie).send_form(form.iter().copied()).unwrap();
        let set = r.headers().get("set-cookie").map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string()).unwrap_or_default();
        let loc = r.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
        (set, loc)
    };
    let account = |email: &str| {
        post("/register", "", &[("email", email), ("password", "long-enough-pw"), ("age", "on"), ("accept", "on")]);
        post("/login", "", &[("email", email), ("password", "long-enough-pw")]).0
    };
    let (bob, carol) = (account("bob@example.org"), account("carol@example.org"));
    state
        .db
        .with(|c| {
            c.execute("UPDATE users SET email_verified_at = 5", [])?;
            let bob: i64 = c.query_row("SELECT id FROM users WHERE email = 'bob@example.org'", [], |r| r.get(0))?;
            crate::courses::create_course(c, bob, "bobs-course", "Bob's course", &crate::courses::starter_files("bobs-course", "Bob's course"))?;
            crate::share::invite(c, "bobs-course", "carol@example.org", 1)?;
            Ok(())
        })
        .unwrap();
    let carol_sees = || get("/courses", &carol).1.contains(r#"data-course-id="bobs-course""#);
    assert!(carol_sees());

    // Bob saves a lesson with a listed term.
    let path = state.db.with(|c| crate::courses::list_files(c, "bobs-course")).unwrap().into_iter().map(|f| f.0).find(|p| p.starts_with("lessons/")).unwrap();
    let original = state.db.with(|c| crate::courses::read_file(c, "bobs-course", &path)).unwrap().unwrap();
    let mut v: serde_json::Value = serde_json::from_str(&original).unwrap();
    v["title"] = "Insults".into();
    v["summary"] = "Never say wetback.".into();
    let (_, loc) = post("/courses/bobs-course/file", &bob, &[("path", &path), ("content", &v.to_string())]);
    assert!(loc.ends_with("notice=saved"), "{loc}");

    let page = get("/courses", &bob).1;
    assert!(page.contains(r#"data-testid="courses-card-flagged""#), "the red ! is on the card");
    assert!(page.contains(r#"data-word="wetback""#) && page.contains(">w******</span> · Insults"), "its dialog has the word, masked, and the lesson");
    assert_eq!(page.matches("wetback").count(), 1, "the word is in the page once, in the masked word's data-word");
    let (_, loc) = post("/courses/open/bobs-course", &bob, &[]);
    assert!(loc.ends_with("/courses/bobs-course?notice=flagged"), "Start is refused: {loc}");
    let editor = get("/courses/bobs-course?notice=flagged", &bob).1;
    assert!(editor.contains("w****** (in Insults)") && !editor.contains("wetback"), "the editor names the word masked");
    let (_, loc) = post("/courses/bobs-course/share", &bob, &[("emails", "dan@example.org")]);
    assert!(loc.ends_with("notice=share-flagged"), "Share is refused: {loc}");
    assert!(!carol_sees(), "it stops opening for whoever it is shared with");

    // Fixed: everything comes back.
    let (_, loc) = post("/courses/bobs-course/file", &bob, &[("path", &path), ("content", &original)]);
    assert!(loc.ends_with("notice=saved"), "{loc}");
    assert!(!get("/courses", &bob).1.contains("courses-card-flagged"));
    assert!(carol_sees());
    let (_, loc) = post("/courses/bobs-course/share", &bob, &[("emails", "dan@example.org")]);
    assert!(!loc.contains("share-flagged"), "{loc}");
}

/// The superuser's course table (#1034): flagged first, search by title,
/// id or address, the filters; an override keeps the course open until it
/// is taken back, and taking it back checks the files again.
#[test]
fn the_course_table_searches_filters_and_overrides() {
    use table::{courses, Show};
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let ann = course(c, "ann-course", &lesson("hola amigo"));
        course(c, "bad-course", &lesson("hola gook"));
        course(c, "old-course", &lesson("hola"));
        c.execute("UPDATE courses SET updated_at = 1 WHERE id = 'bad-course'", [])?;
        for id in ["ann-course", "bad-course", "old-course"] {
            crate::courses::load_ready_files(c, id)?;
        }
        crate::moderation::take_down(c, "old-course", 2)?;

        let ids = |q: &str, show: Show| courses(c, q, show, 1).map(|p| p.list.into_iter().map(|r| r.id).collect::<Vec<_>>());
        assert_eq!(ids("", Show::All)?.first().map(String::as_str), Some("bad-course"), "flagged first, though edited longest ago");
        assert_eq!(ids("Ann-Course@EXAMPLE", Show::All)?, vec!["ann-course"], "by the owner's address, any case");
        assert_eq!(ids("old-course", Show::All)?, vec!["old-course"], "by id");
        assert_eq!(ids("", Show::Flagged)?, vec!["bad-course"]);
        assert_eq!(ids("", Show::TakenDown)?, vec!["old-course"]);
        let page = courses(c, "", Show::All, 9)?;
        assert_eq!((page.total, page.page, page.pages), (3, 1, 1), "a page past the end is the last");
        let bad = courses(c, "bad", Show::All, 1)?.list.remove(0);
        assert_eq!((bad.state(), bad.flag.as_ref().map(|f| f.terms())), ("flagged", Some(vec!["gook"])));

        // Override: it opens, the note and who wrote it are kept, a second override does nothing.
        assert!(override_flag(c, "bad-course", "a quote", ann, 7)?);
        assert!(!override_flag(c, "bad-course", "again", ann, 8)?);
        assert!(!crate::courses::load_ready_files(c, "bad-course")?.is_empty());
        let bad = courses(c, "", Show::Overridden, 1)?.list.remove(0);
        assert_eq!((bad.state(), bad.overridden_by_email.as_str()), ("overridden", "ann-course@example.org"));
        assert_eq!(bad.flag.unwrap().override_note.as_deref(), Some("a quote"));

        // Taken back: the word is still there, so it is flagged again and the note is gone.
        assert!(clear_override(c, "bad-course")?);
        assert!(!clear_override(c, "bad-course")?);
        assert!(crate::courses::load_ready_files(c, "bad-course")?.is_empty());
        let f = get(c, "bad-course")?.unwrap();
        assert_eq!((f.state, f.override_note), (State::Flagged, None));
        Ok(())
    })
    .unwrap();
}

/// Over HTTP: only a superuser may use the course table's actions; override
/// needs a note; delete removes the course and its shares.
#[test]
fn superusers_act_on_courses_from_the_table() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let state = crate::routes::test_state(false, None);
    let app = crate::routes::router(state.clone());
    let base = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await.unwrap() });
        format!("http://{addr}")
    });
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    let get = |path: &str, cookie: &str| {
        let mut r = agent.get(&format!("{base}{path}")).header("cookie", cookie).call().unwrap();
        (r.status().as_u16(), r.body_mut().read_to_string().unwrap())
    };
    let post = |path: &str, cookie: &str, form: &[(&str, &str)]| {
        let r = agent.post(&format!("{base}{path}")).header("cookie", cookie).send_form(form.iter().copied()).unwrap();
        let set = r.headers().get("set-cookie").map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string()).unwrap_or_default();
        let loc = r.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
        (r.status().as_u16(), set, loc)
    };
    let account = |email: &str| {
        post("/register", "", &[("email", email), ("password", "long-enough-pw"), ("age", "on"), ("accept", "on")]);
        post("/login", "", &[("email", email), ("password", "long-enough-pw")]).1
    };
    let (root, bob) = (account("root@example.org"), account("bob@example.org"));
    state
        .db
        .with(|c| {
            c.execute("UPDATE users SET superuser = 1 WHERE email = 'root@example.org'", [])?;
            c.execute("UPDATE users SET email_verified_at = 5", [])?;
            let bob: i64 = c.query_row("SELECT id FROM users WHERE email = 'bob@example.org'", [], |r| r.get(0))?;
            course(c, "bad-course", &lesson("hola gook"));
            c.execute("UPDATE courses SET owner_id = ?1", [bob])?;
            crate::courses::load_ready_files(c, "bad-course")?;
            crate::share::invite(c, "bad-course", "carol@example.org", 1)?;
            Ok(())
        })
        .unwrap();

    for path in ["/superuser/courses/override", "/superuser/courses/clear-override", "/superuser/courses/delete", "/superuser/courses/email-owner"] {
        assert_eq!(post(path, &bob, &[("course", "bad-course"), ("note", "fine")]).0, 404, "{path}");
    }
    assert_eq!(get("/superuser", &bob).0, 404);
    let page = get("/superuser?course_show=flagged", &root).1;
    assert!(page.contains(r#"data-testid="superuser-course-row" data-course-id="bad-course" data-state="flagged" data-source="terms""#), "{page}");
    assert!(page.contains(r#"data-testid="superuser-course-match">"#) && page.contains(r#"data-word="gook""#));

    assert!(post("/superuser/courses/override", &root, &[("course", "bad-course"), ("note", "  ")]).2.contains("did=no-note"));
    let (_, _, loc) = post("/superuser/courses/override", &root, &[("course", "bad-course"), ("note", "a quote")]);
    assert_eq!(loc, "/superuser?did=overridden&about=bad-course");
    let page = get(&loc, &root).1;
    assert!(page.contains(r#"data-course-id="bad-course" data-state="overridden""#) && page.contains("a quote"), "{page}");
    assert!(get("/courses", &bob).1.contains(r#"action="/courses/open/bad-course""#), "the owner can start it again");

    let (_, _, loc) = post("/superuser/courses/clear-override", &root, &[("course", "bad-course")]);
    assert_eq!(loc, "/superuser?did=override-cleared&about=bad-course");
    assert!(get(&loc, &root).1.contains(r#"data-course-id="bad-course" data-state="flagged""#));

    // E-mail the owner (#1035): drafted in bob's language, sent only when
    // the superuser presses Send, with their edits; the row remembers it.
    let outbox = || {
        state
            .db
            .with(|c| {
                let mut st = c.prepare("SELECT to_addr, subject, body_text FROM mail_outbox WHERE purpose = ?1")?;
                let rows = st.query_map([owner_mail::PURPOSE], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?.collect::<rusqlite::Result<Vec<_>>>();
                rows
            })
            .unwrap()
    };
    state.db.with(|c| Ok(c.execute("UPDATE users SET lang = 'es' WHERE email = 'bob@example.org'", [])?)).unwrap();
    let page = get("/superuser", &root).1;
    assert!(page.contains(r#"data-testid="superuser-course-email-owner""#) && page.contains("gook"), "{page}");
    assert!(page.contains("no se puede empezar ni compartir"), "the draft is in the owner's language: {page}");
    assert!(outbox().is_empty(), "nothing is sent on its own");
    assert!(post("/superuser/courses/email-owner", &root, &[("course", "bad-course"), ("subject", "Hi"), ("text", " ")]).2.contains("did=no-mail-text"));
    let (_, _, loc) = post("/superuser/courses/email-owner", &root, &[("course", "bad-course"), ("subject", "Tu curso\r\nT"), ("text", "Quita la palabra gook, por favor.")]);
    assert_eq!(loc, "/superuser?did=owner-emailed&about=bad-course&email=bob%40example.org");
    assert_eq!(outbox(), vec![("bob@example.org".to_string(), "Tu curso T".to_string(), "Quita la palabra gook, por favor.".to_string())]);
    let page = get(&loc, &root).1;
    assert!(page.contains(r#"data-testid="superuser-course-emailed""#) && page.contains(r#"data-testid="superuser-email-owner-before""#), "{page}");
    assert!(post("/superuser/courses/email-owner", &root, &[("course", "nope"), ("subject", "a"), ("text", "b")]).2.contains("did=no-course"));

    let (_, _, loc) = post("/superuser/courses/delete", &root, &[("course", "bad-course")]);
    assert_eq!(loc, "/superuser?did=course-deleted&about=bad-course");
    assert!(!get(&loc, &root).1.contains(r#"data-course-id="bad-course""#));
    let left: (i64, i64) = state.db.with(|c| Ok(c.query_row("SELECT (SELECT COUNT(*) FROM course_shares), (SELECT COUNT(*) FROM course_files)", [], |r| Ok((r.get(0)?, r.get(1)?)))?)).unwrap();
    assert_eq!(left, (0, 0), "its files and shares go with it");
    assert!(post("/superuser/courses/delete", &root, &[("course", "bad-course")]).2.contains("did=no-course"));
}

/// More courses than a page holds: the second page has the rest, and the
/// search applies to every page.
#[test]
fn the_course_table_pages() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let n = table::COURSES_PER_PAGE + 2;
        for i in 0..n {
            course(c, &format!("c{i}"), &lesson("hola"));
        }
        let first = table::courses(c, "", table::Show::All, 1)?;
        let second = table::courses(c, "", table::Show::All, 2)?;
        assert_eq!((first.total, first.pages, first.list.len(), second.list.len()), (n, 2, table::COURSES_PER_PAGE as usize, 2));
        assert!(second.list.iter().all(|r| !first.list.iter().any(|f| f.id == r.id)), "no course on both pages");
        assert_eq!(table::courses(c, "c1", table::Show::All, 2)?.pages, 1, "the search narrows the pages");
        Ok(())
    })
    .unwrap();
}

/// Why a course doesn't open, for the e-mail to its owner (#1035): taken
/// down wins, then a blocking flag; an open course has nothing to explain.
/// The draft names the words and how to fix it, and nobody who reported it.
#[test]
fn the_owner_mail_says_why() {
    use owner_mail::{draft, Why};
    let flag = |state: State, source: Source| Flag {
        course_id: "c".into(),
        state,
        source,
        matches: vec![Match { term: "gook".into(), path: "lessons/a.json".into(), field: "/summary".into(), lesson: "A".into(), text: "they are gooks".into() }],
        reason: "Mocks a group.".into(),
        content_hash: String::new(),
        checked_at: 1,
        override_note: None,
        overridden_by: None,
        overridden_at: None,
        terms_version: 1,
        ai_version: None,
    };
    assert_eq!(Why::of(false, None), None);
    assert_eq!(Why::of(false, Some(&flag(State::Overridden, Source::Terms))), None, "an overridden course opens");
    assert_eq!(Why::of(true, Some(&flag(State::Flagged, Source::Terms))), Some(Why::Down));
    assert_eq!(Why::of(false, Some(&flag(State::Flagged, Source::Ai))), Some(Why::Ai));
    // A flagged course's e-mail says so and links to the site; it never names the words, quotes or report.
    for why in [Why::Terms, Why::Ai] {
        let (subject, text) = draft("Insults", &why, "https://skrongle.example");
        assert!(subject.contains("\"Insults\"") && !subject.contains('\n'), "{subject}");
        assert!(text.contains("can't be started or shared") && text.contains("Our content check flagged it."), "{text}");
        assert!(text.contains("Sign in to see what to fix: https://skrongle.example/courses") && text.contains(&crate::terms::legal_email()), "{text}");
        assert!(!text.contains("gook") && !text.contains("Mocks a group"), "{text}");
    }
    assert!(draft("Insults", &Why::Down, "https://skrongle.example").1.contains("We took it down"));
    assert_eq!(owner_mail::clean_subject(" a\r\n b \n"), "a b");
}

#[test]
fn a_check_saves_its_term_list_version_and_an_older_one_checks_again() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let good = lesson("hola amigo");
        course(c, "c", &good);
        let f = check(c, "c", &good)?.unwrap();
        assert_eq!((f.terms_version, f.ai_version), (terms::TERMS_VERSION, None));

        // Same files, older list version (#1039): checked again and saved at the current one.
        c.execute("UPDATE course_flags SET terms_version = ?1, checked_at = 1", [terms::TERMS_VERSION - 1])?;
        let f = check(c, "c", &good)?.unwrap();
        assert_eq!(f.terms_version, terms::TERMS_VERSION);
        assert!(f.checked_at > 1, "it ran");

        // An AI flag the term check leaves alone still records that the term check ran.
        record(c, "c", State::Flagged, Source::Ai, &[], "the AI's report", "old", 1)?;
        stamp(c, "c", Source::Ai)?;
        c.execute("UPDATE course_flags SET terms_version = 0", [])?;
        let f = check(c, "c", &good)?.unwrap();
        assert_eq!((f.state, f.source), (State::Flagged, Source::Ai));
        assert_eq!((f.terms_version, f.ai_version), (terms::TERMS_VERSION, Some(crate::content_check::AI_CHECK_VERSION)));
        Ok(())
    })
    .unwrap();
}

#[test]
fn the_table_filters_courses_whose_ai_check_is_far_behind() {
    use crate::content_check::{nightly, AI_CHECK_VERSION};
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let n = nightly::settings().recheck_behind;
        for (id, v) in [("far", Some(AI_CHECK_VERSION - n)), ("near", Some(AI_CHECK_VERSION - n + 1)), ("never", None), ("down", Some(AI_CHECK_VERSION - n - 1))] {
            course(c, id, &lesson("hola"));
            if let Some(v) = v {
                c.execute(
                    "INSERT INTO content_scores (course_id, chunk, hash, model, scanned_at, ai_version) VALUES (?1, 'lessons/colours.json', 'h', 'jev', 1, ?2)",
                    rusqlite::params![id, v],
                )?;
            }
        }
        c.execute("UPDATE courses SET taken_down_at = 1 WHERE id = 'down'", [])?;
        let page = table::courses(c, "", table::Show::Behind, 1)?;
        assert_eq!(page.list.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), vec!["far"]);
        assert_eq!(table::behind_count(c)?, 1);
        assert_eq!(table::Show::parse("behind"), table::Show::Behind);
        Ok(())
    })
    .unwrap();
}

#[test]
fn a_masked_word_keeps_its_first_letter() {
    use super::view::mask;
    assert_eq!(mask("wetback"), "w******");
    assert_eq!(mask("ñandú"), "ñ****");
    assert_eq!(mask("two words"), "t** *****");
}
