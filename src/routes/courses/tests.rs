use super::*;
use serde_json::json;

/// The autosave builds the course after it has let go of the database: while
/// the build is held, an unrelated database call still returns, and the
/// result is recorded once the build ends (#84).
#[test]
fn the_autosave_builds_off_the_database_lock() {
    use std::sync::{mpsc, Arc, Mutex};
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let state = crate::routes::test_state(false);
    let ann = state.db.with(|c| crate::auth::create_user(c, "ann@example.org", "x")).unwrap();
    let cookie = format!("sid={}", state.db.with(|c| crate::auth::create_session(c, ann)).unwrap());
    let id = "held-abc123";
    state.db.with(|c| crate::courses::create_course(c, ann, id, "Held", &crate::courses::starter_files(id, "Held"))).unwrap();
    let app = crate::routes::router(state.clone());
    let base = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    });
    // Hold the build of this course until released.
    let (entered_tx, entered_rx) = mpsc::channel::<()>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let (entered_tx, release_rx) = (Mutex::new(entered_tx), Mutex::new(release_rx));
    let hook: Arc<dyn Fn(&str) + Send + Sync> = Arc::new(move |_| {
        let _ = entered_tx.lock().unwrap().send(());
        let _ = release_rx.lock().unwrap().recv();
    });
    crate::courses::CHECK_HOOK.lock().unwrap().push((id.to_string(), hook));
    let (url, cookie_header) = (format!("{base}/api/courses/{id}/lessons"), cookie);
    let saving = std::thread::spawn(move || {
        let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
        let body = json!({"lessons": [{"key": "u1-first-lesson", "unit": "1", "title": "Renamed while held"}], "session": "tab-a"});
        let mut r = agent.post(&url).header("cookie", &cookie_header).send_json(body).unwrap();
        (r.status().as_u16(), serde_json::from_str::<serde_json::Value>(&r.body_mut().read_to_string().unwrap()).unwrap_or_default())
    });
    entered_rx.recv_timeout(std::time::Duration::from_secs(20)).expect("the save reached its build");
    // The save is committed and the build is held: the database is free.
    let (done_tx, done_rx) = mpsc::channel();
    let db = state.db.clone();
    std::thread::spawn(move || {
        let _ = done_tx.send(db.with(|c| c.query_row("SELECT COUNT(*) FROM users", [], |r| r.get::<_, i64>(0))));
    });
    let n = done_rx.recv_timeout(std::time::Duration::from_secs(5)).expect("an unrelated database call returns while the build is held");
    assert_eq!(n.unwrap(), 1);
    release_tx.send(()).unwrap();
    let (status, body) = saving.join().unwrap();
    crate::courses::CHECK_HOOK.lock().unwrap().retain(|(i, _)| i != id);
    assert_eq!((status, body["saved"].as_bool(), body["loads"].as_bool()), (200, Some(true), Some(true)), "{body}");
    assert!(state.db.with(|c| crate::courses::course(c, id)).unwrap().unwrap().error.is_none());
}

/// A test server with one starter course `id` of a new account whose
/// language is `lang` (the owner, so it may do everything): the state, the server's address and the account's cookie.
fn course_site(rt: &tokio::runtime::Runtime, id: &str, lang: &str) -> (Arc<AppState>, String, String) {
    let state = crate::routes::test_state(false);
    let ann = state.db.with(|c| crate::auth::create_user(c, "ann@example.org", "x")).unwrap();
    state.db.with(|c| c.execute("UPDATE users SET lang = ?2, owner = 1 WHERE id = ?1", rusqlite::params![ann, lang])).unwrap();
    let cookie = format!("sid={}", state.db.with(|c| crate::auth::create_session(c, ann)).unwrap());
    state.db.with(|c| courses::create_course(c, ann, id, "Held", &courses::starter_files(id, "Held"))).unwrap();
    let app = crate::routes::router(state.clone());
    let base = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    });
    (state, base, cookie)
}

/// One request a test sends: a form or JSON, to a path under the server.
enum Req {
    Form(String, Vec<(&'static str, String)>),
    Json(String, serde_json::Value),
}

/// Send it with `cookie`: the status, and the redirect's target or else the body.
fn send(base: &str, cookie: &str, req: Req) -> (u16, String) {
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    let mut r = match req {
        Req::Form(path, form) => agent.post(&format!("{base}{path}")).header("cookie", cookie).send_form(form.iter().map(|(k, v)| (*k, v.as_str()))),
        Req::Json(path, body) => agent.post(&format!("{base}{path}")).header("cookie", cookie).send_json(body),
    }
    .unwrap();
    let to = r.headers().get("location").map(|v| v.to_str().unwrap().to_string());
    (r.status().as_u16(), to.unwrap_or_else(|| r.body_mut().read_to_string().unwrap()))
}

/// Holds every build of course `id` (`courses::check`) until let go: each
/// build sends the language it runs in first. Dropped, it lets go for good.
struct Held {
    id: String,
    entered: std::sync::mpsc::Receiver<String>,
    release: std::sync::mpsc::Sender<()>,
}

impl Held {
    fn new(id: &str) -> Held {
        use std::sync::{mpsc, Mutex};
        let (entered_tx, entered) = mpsc::channel::<String>();
        let (release, release_rx) = mpsc::channel::<()>();
        let (entered_tx, release_rx) = (Mutex::new(entered_tx), Mutex::new(release_rx));
        let hook: Arc<dyn Fn(&str) + Send + Sync> = Arc::new(move |_| {
            let _ = entered_tx.lock().unwrap().send(crate::i18n::lang().to_string());
            let _ = release_rx.lock().unwrap().recv();
        });
        courses::CHECK_HOOK.lock().unwrap().push((id.to_string(), hook));
        Held { id: id.to_string(), entered, release }
    }

    /// Send `req` and, while its build is held, check that an unrelated
    /// database call returns; then let the build go. The language the build
    /// ran in, and what the request answered.
    fn send(&self, state: &AppState, base: &str, cookie: &str, req: Req) -> (String, (u16, String)) {
        let (base, cookie) = (base.to_string(), cookie.to_string());
        let sending = std::thread::spawn(move || send(&base, &cookie, req));
        let Ok(lang) = self.entered.recv_timeout(std::time::Duration::from_secs(20)) else {
            panic!("the request did not reach its build: {:?}", sending.join());
        };
        assert!(db_is_free(state), "an unrelated database call returns while the build is held");
        self.release.send(()).unwrap();
        (lang, sending.join().unwrap())
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        courses::CHECK_HOOK.lock().unwrap().retain(|(i, _)| i != &self.id);
    }
}

/// Whether an unrelated database call returns within 5 s.
fn db_is_free(state: &AppState) -> bool {
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let db = state.db.clone();
    std::thread::spawn(move || {
        let _ = done_tx.send(db.with(|c| c.query_row("SELECT COUNT(*) FROM users", [], |r| r.get::<_, i64>(0))));
    });
    done_rx.recv_timeout(std::time::Duration::from_secs(5)).is_ok()
}

/// A second lesson for the starter course, `title` its name.
fn second_lesson(title: &str) -> String {
    json!({ "id": "u1-second", "slug": "second", "title": title, "unit": "1", "group": "basics", "order": 2, "content": "## One\n\nText." }).to_string()
}

/// #84 step 2: the editor's other writes (Upload, the file page's Save and
/// Delete, Check the course, Undo, Redo, the review's Accept) build the
/// course after they have let go of the database, in the request's
/// language, and answer as they did when they built under it.
#[test]
fn the_editors_writes_build_off_the_database_lock() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let id = "writes-abc123";
    let (state, base, cookie) = course_site(&rt, id, "es");
    // An Undo step to undo and redo.
    let (status, _) = send(&base, &cookie, Req::Json(format!("/api/courses/{id}/lessons"), json!({"lessons": [{"key": "u1-first-lesson", "unit": "1", "title": "Renamed"}], "session": "tab-a"})));
    assert_eq!(status, 200);
    let held = Held::new(id);
    let path = "lessons/second.json".to_string();
    let course = |p: &str| format!("/courses/{id}{p}");
    let writes = [
        ("Upload", Req::Json(format!("/api/courses/{id}/files"), json!({"files": [{"path": path, "content": second_lesson("Second")}]})), "\"error\":null"),
        ("Save", Req::Form(course("/file"), vec![("path", path.clone()), ("content", second_lesson("Second, saved"))]), "notice=saved"),
        ("Delete", Req::Form(course("/file/delete"), vec![("path", path.clone())]), "notice=file-deleted"),
        ("Check the course", Req::Form(course("/publish"), vec![]), "notice=loads"),
        ("Undo", Req::Form(course("/undo"), vec![]), "notice=undone"),
        ("Redo", Req::Form(course("/redo"), vec![]), "notice=redone"),
    ];
    for (what, req, answer) in writes {
        let (lang, (status, to)) = held.send(&state, &base, &cookie, req);
        assert_eq!(lang, "es", "{what}: built in the request's language");
        assert!(matches!(status, 200 | 303) && to.contains(answer), "{what}: {status} {to}");
        assert!(state.db.with(|c| courses::course(c, id)).unwrap().unwrap().error.is_none(), "{what}: recorded as loading");
    }
    // The review of an AI run that added a lesson: Accept with it left out.
    state
        .db
        .with(|c| {
            let before = courses::load_files(c, id)?;
            courses::write_file(c, id, &path, &second_lesson("Second, by AI"))?;
            courses::record_ai_run(c, id, &before, "Added a lesson", "")
        })
        .unwrap();
    let (lang, (status, to)) = held.send(&state, &base, &cookie, Req::Form(course("/review"), vec![("file", path.clone())]));
    assert_eq!((lang.as_str(), status), ("es", 303), "{to}");
    assert!(to.contains("notice=accepted-left-undo"), "{to}");
    assert!(state.db.with(|c| courses::read_file(c, id, &path)).unwrap().is_none(), "the lesson left out is gone");
}

/// #84 step 2: a save that lands while a write's build runs wins: the
/// build's result is not recorded over it, and the review, which builds
/// before it writes, writes nothing and says the course changed.
#[test]
fn a_save_while_a_course_is_built_wins() {
    use std::sync::{Arc, Mutex};
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let id = "wins-abc123";
    let (state, base, cookie) = course_site(&rt, id, "en");
    // The hook makes the save, as another tab would: on its own thread, so
    // it waits for the database if the build holds it.
    let saves: Arc<Mutex<Vec<bool>>> = Arc::new(Mutex::new(Vec::new()));
    let (log, db) = (Arc::clone(&saves), state.db.clone());
    let hook: Arc<dyn Fn(&str) + Send + Sync> = Arc::new(move |id| {
        let (db, id) = (db.clone(), id.to_string());
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(db.with(|c| {
                courses::write_file(c, &id, "lessons/first-lesson.json", "{ not json")?;
                courses::set_status(c, &id, None, Some("the newer save's error"))
            }));
        });
        log.lock().unwrap().push(rx.recv_timeout(std::time::Duration::from_secs(5)).is_ok());
    });
    courses::CHECK_HOOK.lock().unwrap().push((id.to_string(), hook));
    let (status, to) = send(&base, &cookie, Req::Form(format!("/courses/{id}/publish"), vec![]));
    let row = state.db.with(|c| courses::course(c, id)).unwrap().unwrap();
    // The review: an AI run that added a lesson, accepted with it left out.
    let path = "lessons/second.json";
    state
        .db
        .with(|c| {
            courses::write_file(c, id, "lessons/first-lesson.json", &courses::starter_files(id, "Held")[1].content)?;
            let before = courses::load_files(c, id)?;
            courses::write_file(c, id, path, &second_lesson("Second, by AI"))?;
            courses::record_ai_run(c, id, &before, "Added a lesson", "")
        })
        .unwrap();
    let (review_status, page) = send(&base, &cookie, Req::Form(format!("/courses/{id}/review"), vec![("file", path.to_string())]));
    courses::CHECK_HOOK.lock().unwrap().retain(|(i, _)| i != id);
    assert_eq!(*saves.lock().unwrap(), vec![true, true], "each save got the database while the build ran");
    // Check the course reports the files it built, and records nothing over the newer save.
    assert_eq!(status, 303);
    assert!(to.contains("notice=loads"), "{to}");
    assert_eq!(row.error.as_deref(), Some("the newer save's error"));
    // The review wrote nothing and says why.
    assert_eq!(review_status, 200);
    assert!(page.contains("changed somewhere else"), "{}", &page[page.find("error").unwrap_or(0)..][..400.min(page.len())]);
    assert!(state.db.with(|c| courses::read_file(c, id, path)).unwrap().is_some(), "the lesson left out is still there");
}

/// The editor's autosave contract (#151, #190): saves of one page share
/// an Undo step, a page that is behind or a busy course gets 409, a table
/// the rules refuse gets 422 and stores nothing, no change is not an error.
#[test]
fn the_editor_saves_as_it_is_edited() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let state = crate::routes::test_state(false);
    let ann = state.db.with(|c| crate::auth::create_user(c, "ann@example.org", "x")).unwrap();
    let cookie = format!("sid={}", state.db.with(|c| crate::auth::create_session(c, ann)).unwrap());
    let id = "auto-abc123";
    state.db.with(|c| crate::courses::create_course(c, ann, id, "Auto", &crate::courses::starter_files(id, "Auto"))).unwrap();
    let app = crate::routes::router(state.clone());
    let base = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    });
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    let save = |body: serde_json::Value| {
        let mut r = agent.post(&format!("{base}/api/courses/{id}/lessons")).header("cookie", &cookie).send_json(body).unwrap();
        let status = r.status().as_u16();
        (status, serde_json::from_str::<serde_json::Value>(&r.body_mut().read_to_string().unwrap()).unwrap_or_default())
    };
    let version = || state.db.with(|c| Ok(crate::courses::version_of(&crate::courses::load_files(c, id)?))).unwrap();
    let steps = || state.db.with(|c| crate::courses::undo_step(c, id)).unwrap().map_or(0, |s| s.steps);
    let first = json!({"key": "u1-first-lesson", "unit": "1", "title": "Your first lesson"});

    // Nothing changed: fine, nothing stored.
    let (status, body) = save(json!({"lessons": [first], "session": "tab-a", "base": version()}));
    assert_eq!((status, body["saved"].as_bool()), (200, Some(false)), "{body}");
    assert_eq!(steps(), 0);

    // A new lesson: its row learns its id and file, the files it changed come back.
    let v0 = version();
    let (status, body) = save(json!({"lessons": [first, {"key": null, "unit": "1", "title": "Second one"}], "session": "tab-a", "base": v0}));
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["rows"][1]["id"], "second-one");
    let path = body["rows"][1]["path"].as_str().unwrap().to_string();
    assert!(body["changed"][&path].as_str().unwrap().contains("\"group\""), "{body}");
    assert_eq!(body["version"].as_str().unwrap(), version());
    assert!(body["undo_html"].as_str().unwrap().contains("course-undo-form"));
    assert!(!body["undo_html"].as_str().unwrap().contains("course-redo-form"), "nothing to redo yet");
    assert_eq!(body["loads"], true);
    assert_eq!(steps(), 1);

    // The same page again: the same Undo step, named for both saves.
    let second = json!({"key": "second-one", "unit": "1", "title": "Second lesson"});
    let (status, body) = save(json!({"lessons": [first, second], "session": "tab-a", "base": version()}));
    assert_eq!(status, 200, "{body}");
    assert_eq!(steps(), 1);
    assert_eq!(body["label"], "Saved the lessons (1 renamed)");
    assert!(state.db.with(|c| crate::courses::undo_step(c, id)).unwrap().unwrap().label.contains("1 added"));

    // A page that is behind (it never saw these saves) is refused, and nothing changes.
    let before = version();
    let (status, body) = save(json!({"lessons": [first], "session": "tab-b", "base": v0}));
    assert_eq!(status, 409);
    assert!(body["error"].as_str().unwrap().contains("Reload the page"), "{body}");
    assert_eq!(version(), before);

    // The rules refuse it: 422 for the table, nothing stored.
    let (status, body) = save(json!({"lessons": [first, {"key": "second-one", "unit": "9", "title": "Second lesson"}], "session": "tab-a", "base": version()}));
    assert_eq!(status, 422);
    assert!(body["errors"]["table"].as_str().unwrap().contains("no unit '9'"), "{body}");
    let p = &body["problems"][0];
    assert_eq!((p["kind"].as_str(), p["field"].as_str(), p["row"].as_u64()), (Some("rules"), Some("unit"), Some(1)), "{body}");
    assert_eq!(p["text"], body["errors"]["table"]);
    assert_eq!(version(), before);

    // Another page: a step of its own.
    let (status, _) = save(json!({"lessons": [second, first], "session": "tab-b", "base": version()}));
    assert_eq!(status, 200);
    assert_eq!(steps(), 2);

    // While an AI job holds the course: 409 for a page, 400 as before without a session.
    state.db.with(|c| crate::courses::set_revising(c, id, "job")).unwrap();
    let (status, body) = save(json!({"lessons": [first], "session": "tab-a", "base": version()}));
    assert_eq!(status, 409);
    assert!(body["error"].as_str().unwrap().contains("being revised"), "{body}");
    assert_eq!(save(json!({"lessons": [first]})).0, 400);
}

/// #366: creating, uploading, saving a file and saving the lesson table
/// stop at the course, account and count limits and at the write rate
/// limit, store nothing then, and say which limit it was. #383: a
/// learning page opens a user-made active course for its owner only.
#[test]
fn course_writes_stop_at_the_limits() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let mut state = crate::routes::test_state(false);
    let limits = courses::Limits { max_courses: 2, course_bytes: 20_000, account_bytes: 30_000 };
    Arc::get_mut(&mut state).unwrap().courses = Arc::new(courses::Courses::empty().with_limits(limits));
    let user = |email: &str| {
        let uid = state.db.with(|c| crate::auth::create_user(c, email, "x")).unwrap();
        state.db.with(|c| crate::perms::apply_preset(c, uid, crate::perms::TEACHER_PRESET)).unwrap();
        (uid, format!("sid={}", state.db.with(|c| crate::auth::create_session(c, uid)).unwrap()))
    };
    let (ann, cookie) = user("ann@example.org");
    let app = crate::routes::router(state.clone());
    let base = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    });
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    let location = |r: &ureq::http::Response<ureq::Body>| r.headers().get("location").map(|v| v.to_str().unwrap().to_string()).unwrap_or_default();

    // Two courses, then the count limit.
    let create = || agent.post(&format!("{base}/courses/new")).header("cookie", &cookie).send_form([("title", "")]).unwrap();
    assert!(location(&create()).contains("generate=1"));
    assert!(location(&create()).contains("generate=1"));
    let r = create();
    assert!(location(&r).ends_with("/courses?notice=course-count-2"), "{:?}", location(&r));
    assert_eq!(picker_notice("course-count-2").as_deref(), Some("An account may have at most 2 courses. Delete a course you no longer need to make another."));
    let ids: Vec<String> = state.db.with(|c| courses::courses_owned_by(c, ann)).unwrap().into_iter().map(|r| r.id).collect();
    assert_eq!(ids.len(), 2);
    let (a, b) = (&ids[0], &ids[1]);
    let lesson = |n: usize| json!({ "id": "big", "title": "Big", "x": "y".repeat(n) }).to_string();
    let upload = |id: &str, n: usize| {
        let mut r = agent
            .post(&format!("{base}/api/courses/{id}/files"))
            .header("cookie", &cookie)
            .send_json(json!({ "files": [{ "path": "lessons/big.json", "content": lesson(n) }] }))
            .unwrap();
        serde_json::from_str::<serde_json::Value>(&r.body_mut().read_to_string().unwrap()).unwrap()
    };
    let stored = |id: &str| state.db.with(|c| courses::read_file(c, id, "lessons/big.json")).unwrap();

    // Over the course's limit: refused, nothing stored.
    let body = upload(a, 25_000);
    assert_eq!(body["saved"], json!([]), "{body}");
    assert!(body["rejected"][0].as_str().unwrap().contains("a course may take at most"), "{body}");
    assert!(stored(a).is_none());
    // Within it: stored.
    let body = upload(a, 15_000);
    assert_eq!(body["saved"], json!(["lessons/big.json"]), "{body}");
    // The file page: within B's limit, but A and B together are over the account's.
    let mut r = agent
        .post(&format!("{base}/courses/{b}/file"))
        .header("cookie", &cookie)
        .send_form([("path", "lessons/big.json"), ("content", lesson(15_000).as_str())])
        .unwrap();
    assert_eq!(r.status().as_u16(), 200);
    let page = r.body_mut().read_to_string().unwrap();
    assert!(page.contains("courses together may take at most"), "{}", &page[page.find("error").unwrap_or(0)..][..400.min(page.len())]);
    assert!(stored(b).is_none());
    // The lesson table: a lesson's JSON that would take B over its limit.
    let table = |content: String| {
        let mut r = agent
            .post(&format!("{base}/api/courses/{b}/lessons"))
            .header("cookie", &cookie)
            .send_json(json!({ "lessons": [{"key": "u1-first-lesson", "unit": "1", "title": "Your first lesson", "content": content}], "session": "tab" }))
            .unwrap();
        (r.status().as_u16(), serde_json::from_str::<serde_json::Value>(&r.body_mut().read_to_string().unwrap()).unwrap_or_default())
    };
    let first = state.db.with(|c| courses::read_file(c, b, "lessons/first-lesson.json")).unwrap().expect("the starter lesson");
    let mut v: serde_json::Value = serde_json::from_str(&first).unwrap();
    v["notes"] = json!("z".repeat(25_000));
    let (status, body) = table(serde_json::to_string(&v).unwrap());
    assert_eq!(status, 422, "{body}");
    assert!(body["errors"]["table"].as_str().unwrap().contains("a course may take at most"), "{body}");
    assert_eq!(state.db.with(|c| courses::read_file(c, b, "lessons/first-lesson.json")).unwrap().unwrap(), first);

    // The write rate limit: once it is used up, saves are refused, naming it.
    let limit = crate::security::COURSE_WRITES_PER_USER;
    while state.limits.hit(limit, &ann.to_string(), crate::db::now()) {}
    let (status, body) = table(first.clone());
    assert_eq!(status, 422, "{body}");
    assert!(body["errors"]["table"].as_str().unwrap().contains(&format!("{} times in {} minutes", limit.max, limit.window / 60)), "{body}");

    // #383: Bob's active course set to Ann's (by a writer that skipped the
    // owner check): his learning pages do not open it, and it is dropped.
    let (bob, bob_cookie) = user("bob@example.org");
    state.db.with(|c| c.execute("UPDATE users SET active_course = ?2 WHERE id = ?1", rusqlite::params![bob, b])).unwrap();
    let r = agent.get(&format!("{base}/")).header("cookie", &bob_cookie).call().unwrap();
    assert!(location(&r).ends_with("/courses"), "{:?} {}", location(&r), r.status());
    assert_eq!(state.db.with(|c| c.query_row("SELECT active_course FROM users WHERE id = ?1", [bob], |r| r.get::<_, Option<String>>(0))).unwrap(), None);
    // Ann, its owner, works in it.
    state.db.with(|c| courses::open_project(c, ann, b)).unwrap();
    let r = agent.get(&format!("{base}/")).header("cookie", &cookie).call().unwrap();
    assert_eq!(r.status().as_u16(), 200, "{:?}", location(&r));
}

#[test]
fn prompt_boxes_take_twenty_characters_up_to_their_maximum() {
    assert_eq!((ai::grouped(20), ai::grouped(1000), ai::grouped(60_000), ai::grouped(1_234_567)), ("20".into(), "1,000".into(), "60,000".into(), "1,234,567".into()));
    let at = |n: usize| "x".repeat(n);
    assert_eq!(ai_text_problem(&at(19), AiMode::Add).as_deref(), Some("Say what the new lesson should teach, in at least 20 characters."));
    assert_eq!(ai_text_problem(&at(19), AiMode::Change).as_deref(), Some("Say what should change, in at least 20 characters."));
    assert_eq!(ai_text_problem(&at(20), AiMode::Add), None);
    assert_eq!(ai_text_problem(&at(ai::MAX_BRIEF), AiMode::Add), None);
    assert_eq!(ai_text_problem(&at(ai::MAX_BRIEF + 1), AiMode::Add).as_deref(), Some("Keep it to 1,000 characters at most."));
    assert_eq!(ai_text_problem(&at(ai::MAX_BRIEF + 1), AiMode::Change), None, "a revision takes more");
    assert_eq!(ai_text_problem(&at(ai::MAX_FEEDBACK + 1), AiMode::Change).as_deref(), Some("Keep it to 4,000 characters at most."));
    // Edit lessons takes a change's limits.
    assert_eq!(ai_text_problem(&at(19), AiMode::Edit).as_deref(), Some("Say what should change, in at least 20 characters."));
    assert_eq!(ai_text_problem(&at(ai::MAX_FEEDBACK), AiMode::Edit), None);
    assert!(ai_text_problem(&at(ai::MAX_FEEDBACK + 1), AiMode::Edit).is_some());
    assert_eq!((AiMode::parse("edit"), AiMode::parse("change"), AiMode::parse("add"), AiMode::parse("")), (AiMode::Edit, AiMode::Change, AiMode::Add, AiMode::Add));
    // Characters, not bytes.
    assert_eq!(ai_text_problem(&"ñ".repeat(20), AiMode::Add), None);
    assert!(wish_too_long().contains("2,000") && notes_too_long().contains("60,000"));
}

/// `?notice=` names a message the server holds; text of a link's own
/// never reaches the status alert (#385). The editor's delete forms ask
/// through `data-confirm`, not JavaScript in an inline handler (#387).
/// The lesson editor's "Try it" (#841): a draft question is graded with
/// the real checker, for the owner only, and nothing is recorded.
#[test]
fn preview_answer_grades_a_draft_question() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let state = crate::routes::test_state(false);
    let ann = state.db.with(|c| crate::auth::create_user(c, "ann@example.org", "x")).unwrap();
    let bob = state.db.with(|c| crate::auth::create_user(c, "bob@example.org", "x")).unwrap();
    let cookie = format!("sid={}", state.db.with(|c| crate::auth::create_session(c, ann)).unwrap());
    let bobs = format!("sid={}", state.db.with(|c| crate::auth::create_session(c, bob)).unwrap());
    let id = "preview-abc123";
    state.db.with(|c| crate::courses::create_course(c, ann, id, "Preview", &crate::courses::starter_files(id, "Preview"))).unwrap();
    let app = crate::routes::router(state.clone());
    let base = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    });
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    let ask = |cookie: &str, body: serde_json::Value| {
        let mut r = agent.post(&format!("{base}/api/courses/{id}/preview-answer")).header("cookie", cookie).send_json(body).unwrap();
        let status = r.status().as_u16();
        (status, serde_json::from_str::<serde_json::Value>(&r.body_mut().read_to_string().unwrap()).unwrap_or_default())
    };
    let numeric = json!({"question_id": "draft-1", "prompt": "Area?", "type": "numeric", "correct_answer": "12.57", "tolerance": 0.01, "unit": "cm²", "explanation": "πr²"});
    let (status, body) = ask(&cookie, json!({"question": numeric, "answer": "4π"}));
    assert_eq!(status, 200, "{body}");
    assert_eq!((body["correct"].as_bool(), body["explanation"].as_str()), (Some(true), Some("πr²")), "{body}");
    let (_, body) = ask(&cookie, json!({"question": numeric, "answer": "12.6"}));
    assert_eq!(body["correct"].as_bool(), Some(false), "{body}");
    assert_eq!(body["correct_answer"].as_str(), Some("12.57"));
    // A fill-in answer that only misses an accent gets the note the quiz gives.
    let fill = json!({"question_id": "draft-2", "prompt": "___", "type": "fill-in-blank", "correct_answer": "está"});
    let (_, body) = ask(&cookie, json!({"question": fill, "answer": "esta"}));
    assert_eq!(body["correct"].as_bool(), Some(false));
    assert!(body["note"].as_str().is_some_and(|n| n.contains("está")), "{body}");
    // Nothing recorded: no quiz history for the course.
    let n: i64 = state.db.with(|c| c.query_row("SELECT COUNT(*) FROM quiz_history", [], |r| r.get(0))).unwrap();
    assert_eq!(n, 0);
    // Not the owner: refused.
    let (status, _) = ask(&bobs, json!({"question": fill, "answer": "está"}));
    assert_eq!(status, 400);
    // A question that is not one: a plain bad request, not a crash.
    let (status, _) = ask(&cookie, json!({"question": {"prompt": "no type"}, "answer": "x"}));
    assert!((400..500).contains(&status), "{status}");

    // The editor's check of a math question's answers (#1200): the first
    // one the checker cannot read, the rule a save would refuse.
    let key = |cookie: &str, question: serde_json::Value| {
        let mut r = agent.post(&format!("{base}/api/courses/{id}/answer-key")).header("cookie", cookie).send_json(json!({ "question": question })).unwrap();
        (r.status().as_u16(), serde_json::from_str::<serde_json::Value>(&r.body_mut().read_to_string().unwrap()).unwrap_or_default())
    };
    assert_eq!(key(&cookie, numeric.clone()), (200, json!({ "answer": null })));
    let words = json!({"question_id": "d", "prompt": "?", "type": "numeric", "correct_answer": "12", "accepted_answers": ["twelve"]});
    assert_eq!(key(&cookie, words).1, json!({ "answer": "twelve" }));
    let expr = json!({"question_id": "d", "prompt": "?", "type": "expression", "correct_answer": "2x +"});
    assert_eq!(key(&cookie, expr).1, json!({ "answer": "2x +" }));
    assert_eq!(key(&cookie, fill.clone()).1, json!({ "answer": null }));
    assert_eq!(key(&bobs, fill).0, 400);
}

#[test]
fn notices_are_keys_not_text() {
    assert_eq!(picker_notice("course-deleted").as_deref(), Some("Course deleted."));
    assert_eq!(picker_notice("Your account is locked, call 555"), None);
    assert_eq!(picker_notice("copy-no-room-33554432").as_deref(), Some("There is no room for a copy: all of an account's courses together may take at most 32 MB. Delete a course you no longer need."));
    assert_eq!(picker_notice("copy-too-big-8388608").as_deref(), Some("This course is too big to copy: a course may take at most 8 MB."));
    assert_eq!(picker_notice("copy-no-room-lots"), None);
    assert_eq!(editor_notice("Visit evil.example", None, None), None);
    assert_eq!(editor_notice("accepted-left-undo", None, None).as_deref(), Some("The AI's changes are in, without the parts you left out. Undo puts it back."));
    assert_eq!(editor_notice("accepted-left-nope", None, None), None);
    let u = UndoView { course_id: "c".into(), undo: Some("Added “Two”".into()), earlier: 0, redo: Some("Removed “One”".into()) };
    assert_eq!(editor_notice("undone", Some(&u), None).as_deref(), Some("Undone (Removed “One”): the course is back to how it was before."));
    assert_eq!(editor_notice("redone", Some(&u), None).as_deref(), Some("Redone (Added “Two”)."));

    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let state = crate::routes::test_state(false);
    let ann = state.db.with(|c| crate::auth::create_user(c, "ann@example.org", "x")).unwrap();
    let cookie = format!("sid={}", state.db.with(|c| crate::auth::create_session(c, ann)).unwrap());
    let id = "notice-abc123";
    state.db.with(|c| crate::courses::create_course(c, ann, id, "Notice", &crate::courses::starter_files(id, "Notice"))).unwrap();
    let app = crate::routes::router(state.clone());
    let base = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    });
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    let get = |path: &str| agent.get(&format!("{base}{path}")).header("cookie", &cookie).call().unwrap().body_mut().read_to_string().unwrap();

    let page = get("/courses?notice=Your%20account%20is%20locked");
    assert!(!page.contains("account is locked") && !page.contains("courses-notice"), "{page}");
    assert!(get("/courses?notice=course-deleted").contains(r#"data-testid="courses-notice">Course deleted.</p>"#));
    let page = get(&format!("/courses/{id}?notice=Visit%20evil.example"));
    assert!(!page.contains("evil.example") && !page.contains("course-edit-notice"), "{page}");

    // Publish redirects with a key, which the editor turns into its text.
    let mut r = agent.post(&format!("{base}/courses/{id}/publish")).header("cookie", &cookie).send_empty().unwrap();
    let _ = r.body_mut().read_to_string();
    let loc = r.headers().get("location").unwrap().to_str().unwrap().to_string();
    assert!(loc.ends_with(&format!("/courses/{id}?notice=loads")) || loc.ends_with(&format!("/courses/{id}?notice=broken")), "{loc}");
    let page = get(&loc);
    assert!(page.contains(r#"data-testid="course-edit-notice">The course"#), "{page}");

    assert!(!page.contains("onsubmit"), "no inline submit handler: {page}");
    assert!(page.contains(r#"data-testid="course-edit-delete-form" data-confirm="Delete this course?"#), "{page}");
}

/// Skip levels (#146): without the permission no level and no placement
/// test is offered, a level change and the test are a 403, and the motivator
/// still saves; with it everything works; a level chosen while it was held
/// stays when it is taken away, and cannot be moved.
#[test]
fn choosing_a_level_needs_the_skip_levels_permission() {
    use crate::perms::{Perm, Perms};
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    // The test course has levels and a placement test.
    let with_test_course = Arc::new(crate::courses::Courses::load(std::path::Path::new("data"), true).unwrap());
    let state = Arc::new(AppState { courses: with_test_course, ..Arc::into_inner(crate::routes::test_state(false)).unwrap() });
    let ann = state.db.with(|c| crate::auth::create_user(c, "ann@example.org", "x")).unwrap();
    let cookie = format!("sid={}", state.db.with(|c| crate::auth::create_session(c, ann)).unwrap());
    let id = "test-course";
    let content = rt.block_on(state.courses.get(&state.db, id)).unwrap().unwrap();
    let later = pages::level_options(&content, 0)[1].id.clone();
    assert!(!later.is_empty() && crate::placement::offered(&content));
    let app = crate::routes::router(state.clone());
    let base = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    });
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    let get = |path: &str| {
        let mut r = agent.get(&format!("{base}{path}")).header("cookie", &cookie).call().unwrap();
        (r.status().as_u16(), r.body_mut().read_to_string().unwrap())
    };
    let post = |path: &str, form: &[(&str, &str)]| agent.post(&format!("{base}{path}")).header("cookie", &cookie).send_form(form.iter().copied()).unwrap().status().as_u16();
    let mark = || agent.post(&format!("{base}/courses/{id}/placement")).header("cookie", &cookie).send_json(json!({"answers": []})).unwrap().status().as_u16();
    let project = || state.db.with(|c| crate::courses::project(c, ann, id)).unwrap();
    let level = || project().unwrap().start_unit;
    let allow = |on: bool| state.db.with(|c| crate::perms::set_custom(c, ann, Perms::STUDENT.with(Perm::SkipLevels, on))).unwrap();
    let settings = format!("/courses/{id}/settings");
    let (motivator, other) = (crate::motivators::ALL[1].id(), crate::motivators::ALL[0].id());
    let level_card = r#"data-testid="courses-settings-level""#;

    // A Student: the Start wizard offers no level and no test; a level posted by hand opens nothing.
    allow(false);
    let (status, page) = get("/courses");
    assert_eq!(status, 200);
    assert!(page.contains(r#"data-testid="courses-settings-motivator""#) && !page.contains(level_card) && !page.contains(r#"data-testid="placement"#), "{page}");
    assert_eq!(post(&format!("/courses/open/{id}"), &[("wizard", "1"), ("start_unit", &later)]), 403);
    assert!(project().is_none());
    assert_eq!(post(&format!("/courses/open/{id}"), &[("wizard", "1"), ("motivator", motivator)]), 303);
    assert_eq!((level(), project().unwrap().motivator.as_str()), (String::new(), motivator));
    // Opened: the gear has no level row; a level change is refused, the motivator and the level they have are not.
    assert!(!get("/courses").1.contains(level_card));
    assert_eq!(post(&settings, &[("start_unit", &later)]), 403);
    assert_eq!(level(), "");
    assert_eq!(post(&settings, &[("motivator", other)]), 303);
    assert_eq!(project().unwrap().motivator, other);
    assert_eq!(post(&settings, &[("start_unit", ""), ("motivator", motivator)]), 303);
    assert_eq!(get(&format!("/courses/{id}/placement")).0, 403);
    assert_eq!(mark(), 403);
    // The wizard on the home page of a course opened another way: no level step either.
    assert_eq!(post("/courses/open/spanish", &[]), 303);
    let (status, home) = get("/");
    assert_eq!(status, 200);
    assert!(home.contains(r#"data-testid="courses-settings-dialog""#) && !home.contains(level_card), "{home}");

    // Skip levels (16): the level row, a saved level, the test.
    allow(true);
    assert_eq!(state.db.with(|c| c.query_row("SELECT perms FROM users WHERE id = ?1", [ann], |r| r.get::<_, i64>(0))).unwrap(), 16);
    assert!(get("/courses").1.contains(level_card));
    assert!(get("/").1.contains(level_card));
    assert_eq!(post(&settings, &[("start_unit", &later)]), 303);
    assert_eq!(level(), later);
    assert_eq!(get(&format!("/courses/{id}/placement")).0, 200);
    assert_eq!(mark(), 200);

    // Taken away: the level stays, saving the motivator does not reset it, and it cannot be moved.
    allow(false);
    assert_eq!(level(), later);
    assert!(!get("/courses").1.contains(level_card));
    assert_eq!(post(&settings, &[("motivator", other)]), 303);
    assert_eq!(post(&settings, &[("start_unit", &later), ("motivator", motivator)]), 303);
    assert_eq!((level(), project().unwrap().motivator.as_str()), (later.clone(), motivator));
    assert_eq!(post(&settings, &[("start_unit", "")]), 403);
    assert_eq!(level(), later);
}

/// The Generate dialog's estimate (#443): lessons and time always, dollars
/// for a priced model, and the same
/// sum `Meter::estimate` reserves.
#[test]
fn the_generate_estimate_counts_lessons_time_and_cost() {
    use generate::{estimate_line, Rate};
    assert_eq!(estimate_line(6, 0, None), "6 lessons · about 5 minutes");
    let prices = crate::prices::PriceTable::parse(crate::prices::DEFAULT_PRICES).unwrap();
    assert!(prices.listed("claude-opus-5-5") && !prices.listed("gpt-5"));
    let opus = prices.price("claude-opus-5-5");
    let own = Rate { id: "anthropic", price: Some(opus) };
    let (input, output) = crate::ai::job_tokens(6, true, 0);
    let cost = crate::prices::PriceTable::cost_at(opus, input, output);
    assert_eq!(estimate_line(6, 0, Some(&own)), format!("6 lessons · about 5 minutes · roughly CA${:.2}", generate::to_cad(cost) as f64 / 1e6));
    assert_eq!(generate::to_cad(1_000_000), 1_400_000, "shown in Canadian dollars");
    let more = |notes: usize| estimate_line(6, notes, Some(&own));
    assert_ne!(more(0), more(20_000));
}

/// Use AI's form alone (`ai_form.html`), for the mode choice (#1241).
#[derive(askama::Template)]
#[template(path = "ai_form.html")]
struct AiFormAlone {
    user: Option<crate::auth::CurrentUser>,
    af: editor::AiFormView,
}

fn ai_form_html(mode: &'static str, can_change: bool) -> String {
    use askama::Template;
    let af = editor::AiFormView {
        course_id: "c".into(),
        provider: Some("Claude"),
        named_provider: false,
        can_change,
        generate: false,
        mode,
        outline: false,
        language: true,
        text: String::new(),
        error: None,
        reviewable: None,
    };
    AiFormAlone { user: None, af }.render().unwrap()
}

/// The `<input>` of one mode radio.
fn mode_radio(html: &str, value: &str) -> String {
    let at = html.find(&format!("value=\"{value}\" data-testid=\"course-ai-mode\"")).expect(value);
    let start = html[..at].rfind("<input").unwrap();
    html[start..start + html[start..].find('>').unwrap()].to_string()
}

#[test]
fn use_ai_mode_choice_follows_the_mode_and_greys_add_and_edit_in_place() {
    // A course that cannot be revised: Add and edit is there, greyed, with the reason in the (i).
    let html = ai_form_html("add", false);
    assert!(mode_radio(&html, "change").contains(" disabled"), "{html}");
    assert!(!mode_radio(&html, "edit").contains(" disabled"));
    assert!(!mode_radio(&html, "add").contains(" disabled") && mode_radio(&html, "add").contains(" checked"));
    assert!(html.contains("data-testid=\"course-ai-mode-finished-only\""));
    assert!(html.contains("maxlength=\"1000\"") && html.contains("Up to 1,000 characters."));
    assert!(html.contains(">What should the AI add?</label>"));
    // Edit: the long limit, its own label and placeholder, Outline only off.
    let html = ai_form_html("edit", true);
    assert!(mode_radio(&html, "edit").contains(" checked") && !mode_radio(&html, "change").contains(" disabled"));
    assert!(!html.contains("course-ai-mode-finished-only"));
    assert!(html.contains("maxlength=\"4000\"") && html.contains("Up to 4,000 characters."));
    assert!(html.contains(">What should the AI change?</label>") && html.contains("fix the typo in lesson 2"));
    let outline = &html[html.find("data-testid=\"course-ai-outline\"").unwrap()..];
    assert!(outline[..outline.find('>').unwrap()].contains(" disabled"));
    // Add and edit.
    let html = ai_form_html("change", true);
    assert!(mode_radio(&html, "change").contains(" checked"));
    assert!(html.contains(">What should the AI add or change?</label>") && html.contains("maxlength=\"4000\""));
    assert!(!html[html.find("data-testid=\"course-ai-outline\"").unwrap()..].split('>').next().unwrap().contains(" disabled"));
}

/// The Create courses permission (#143): without it "Create a course", Copy
/// and Copy and translate answer 403 and add no course, and the course list
/// offers none of them; a course the account already owns still opens in
/// the editor. With the Teacher set it is all as before, and an owner
/// creates whatever its stored set says.
#[test]
fn making_a_course_needs_the_create_courses_permission() {
    use crate::perms::{self, Perms};
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let state = crate::routes::test_state(false);
    let user = |email: &str| {
        let uid = state.db.with(|c| crate::auth::create_user(c, email, "x")).unwrap();
        (uid, format!("sid={}", state.db.with(|c| crate::auth::create_session(c, uid)).unwrap()))
    };
    let (stu, stu_c) = user("stu@example.org");
    let (tea, tea_c) = user("tea@example.org");
    let (root, root_c) = user("root@example.org");
    // Stu made a course while a Teacher, then became a Student; Spanish is open.
    let old = "old-abc123";
    state
        .db
        .with(|c| {
            courses::create_course(c, stu, old, "From before", &courses::starter_files(old, "From before"))?;
            courses::open_project(c, stu, "spanish")?;
            c.execute("UPDATE users SET perms = 0", [])?;
            perms::set_custom(c, tea, Perms::TEACHER)?;
            c.execute("UPDATE users SET owner = 1 WHERE id = ?1", [root])?;
            Ok(())
        })
        .unwrap();
    let app = crate::routes::router(state.clone());
    let base = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    });
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    let location = |r: &ureq::http::Response<ureq::Body>| r.headers().get("location").map(|v| v.to_str().unwrap().to_string()).unwrap_or_default();
    let post = |path: &str, cookie: &str, form: &[(&str, &str)]| {
        let r = agent.post(&format!("{base}{path}")).header("cookie", cookie).send_form(form.iter().copied()).unwrap();
        (r.status().as_u16(), location(&r))
    };
    let get = |path: &str, cookie: &str| {
        let mut r = agent.get(&format!("{base}{path}")).header("cookie", cookie).call().unwrap();
        (r.status().as_u16(), r.body_mut().read_to_string().unwrap())
    };
    let count = || state.db.with(|c| c.query_row("SELECT COUNT(*) FROM courses", [], |r| r.get::<_, i64>(0))).unwrap();
    assert_eq!(count(), 1);

    // A Student: the three routes refuse, for a built-in course or its own, and nothing is made.
    assert_eq!(post("/courses/new", &stu_c, &[("title", "")]).0, 403);
    for id in ["spanish", old] {
        assert_eq!(post(&format!("/courses/{id}/copy"), &stu_c, &[]).0, 403, "{id}");
        assert_eq!(post(&format!("/courses/{id}/translate"), &stu_c, &[("lang", "es")]).0, 403, "{id}");
    }
    assert_eq!(count(), 1);
    // Its course list offers none of it, and no heading is left where the block was.
    let (status, page) = get("/courses", &stu_c);
    assert_eq!(status, 200);
    for part in ["courses-make", "courses-create", "courses-copy", "courses-translate"] {
        assert!(!page.contains(part), "{part} is offered to a Student");
    }
    assert!(page.contains(&crate::t("courses.picker.lead_learn_only")), "{page}");
    assert!(page.contains(r#"data-course-id="spanish""#) && page.contains(&format!(r#"data-course-id="{old}""#)));
    // The course it owns from before still opens in the editor, and still saves.
    let (status, page) = get(&format!("/courses/{old}"), &stu_c);
    assert_eq!(status, 200);
    assert!(page.contains(r#"data-testid="course-edit""#));
    let r = agent
        .post(&format!("{base}/api/courses/{old}/lessons"))
        .header("cookie", &stu_c)
        .send_json(json!({"lessons": [{"key": "u1-first-lesson", "unit": "1", "title": "Renamed by a Student"}], "session": "tab-a"}))
        .unwrap();
    assert_eq!(r.status().as_u16(), 200);

    // A Teacher: as before.
    let (status, page) = get("/courses", &tea_c);
    assert_eq!(status, 200);
    assert!(page.contains(r#"data-testid="courses-create""#) && !page.contains(&crate::t("courses.picker.lead_learn_only")), "{page}");
    let (status, loc) = post("/courses/new", &tea_c, &[("title", "")]);
    assert!(status == 303 && loc.contains("generate=1"), "{status} {loc}");
    let (status, loc) = post("/courses/spanish/copy", &tea_c, &[]);
    assert!(status == 303 && loc.starts_with("/courses/"), "{status} {loc}");
    assert_eq!(count(), 3);
    // Copy and translate gets past the permission (this server has no AI key, so it stops there).
    assert_ne!(post("/courses/spanish/translate", &tea_c, &[("lang", "es")]).0, 403);
    // Copy is behind the gear of a course it has opened.
    state.db.with(|c| courses::open_project(c, tea, "spanish")).unwrap();
    assert!(get("/courses", &tea_c).1.contains(r#"data-testid="courses-copy""#));

    // An owner whose stored set is empty still creates and copies.
    assert_eq!(state.db.with(|c| c.query_row("SELECT perms FROM users WHERE id = ?1", [root], |r| r.get::<_, i64>(0))).unwrap(), 0);
    assert!(post("/courses/new", &root_c, &[("title", "")]).1.contains("generate=1"));
    assert_eq!(post("/courses/spanish/copy", &root_c, &[]).0, 303);
    assert_eq!(count(), 5);
    assert!(get("/courses", &root_c).1.contains(r#"data-testid="courses-create""#));
}
