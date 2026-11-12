//! Sharing a course (#451), the cases past the happy path (#513) that only
//! the server sees: the share form's limits, and a link opened while the
//! course is being written or revised. The happy path is
//! `a_shared_course_is_on_the_recipients_list_read_only` in
//! `routes/tests.rs`; the browser cases are `e2e/specs/share-edges.spec.ts`.

use std::sync::Arc;

use crate::AppState;

/// A served app and its helpers: GET (status, location, body) and POST
/// (status, session cookie, location), no redirects followed.
struct Server {
    _rt: tokio::runtime::Runtime,
    base: String,
    agent: ureq::Agent,
    state: Arc<AppState>,
}

impl Server {
    fn start() -> Server {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let state = crate::routes::test_state(false);
        let app = crate::routes::router(state.clone());
        let base = rt.block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move { axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap() });
            format!("http://{addr}")
        });
        let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
        Server { _rt: rt, base, agent, state }
    }

    fn get(&self, path: &str, cookie: &str) -> (u16, String, String) {
        let mut r = self.agent.get(&format!("{}{path}", self.base)).header("cookie", cookie).call().unwrap();
        let loc = r.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
        (r.status().as_u16(), loc, r.body_mut().read_to_string().unwrap())
    }

    fn post(&self, path: &str, cookie: &str, form: &[(&str, &str)]) -> (u16, String, String) {
        let r = self.agent.post(&format!("{}{path}", self.base)).header("cookie", cookie).send_form(form.iter().copied()).unwrap();
        let set = r.headers().get("set-cookie").map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string()).unwrap_or_default();
        let loc = r.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
        (r.status().as_u16(), set, loc)
    }

    /// Register (which signs in); the session cookie.
    fn account(&self, email: &str) -> String {
        let (status, cookie, _) = self.post("/register", "", &[("email", email), ("password", "long-enough-pw"), ("age", "on")]);
        assert_eq!(status, 303, "registered and signed in");
        cookie
    }

    /// A new course of the signed-in owner; its id.
    fn course(&self, owner: &str) -> String {
        let (status, _, loc) = self.post("/courses/new", owner, &[("title", "Fractions")]);
        assert_eq!(status, 303);
        loc.strip_prefix("/courses/").and_then(|l| l.strip_suffix("?generate=1")).expect("the editor").to_string()
    }

    /// Post the Share form: (status, location, page). Made, the page is
    /// the dialog with the link just made.
    fn share(&self, id: &str, cookie: &str, label: &str) -> (u16, String, String) {
        let mut r = self.agent.post(&format!("{}/courses/{id}/share", self.base)).header("cookie", cookie).send_form([("label", label)]).unwrap();
        let loc = r.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
        (r.status().as_u16(), loc, r.body_mut().read_to_string().unwrap())
    }

    /// Make an invitation link named `label` for course `id` and return its token from the page.
    fn token(&self, id: &str, owner: &str, label: &str) -> String {
        let (status, _, page) = self.share(id, owner, label);
        assert_eq!(status, 200, "{page}");
        let marker = "/courses/shared/";
        let at = page.find(marker).expect("the link") + marker.len();
        page[at..at + 64].to_string()
    }

    /// Put the course in `state` (generating, revising, ready), as a job
    /// would, and drop the loaded copy so the next request reads it again.
    fn set_state(&self, id: &str, state: &str) {
        let (cid, s) = (id.to_string(), state.to_string());
        self.state.db.with(move |c| c.execute("UPDATE courses SET state = ?2 WHERE id = ?1", [cid, s])).unwrap();
        self.state.courses.invalidate(id);
    }
}

/// The share form makes a link per click, named as typed (trimmed, cut
/// at 60 characters) or not at all, up to 50 per course; the 51st is
/// refused with a notice and nothing made.
#[test]
fn the_share_form_names_a_link_as_typed_and_stops_at_its_limit() {
    let s = Server::start();
    let ann = s.account("ann@example.org");
    let id = s.course(&ann);
    // A refusal goes back to the dialog with a notice; a link answers with the links page.
    let share = |label: &str| {
        let (status, loc, page) = s.share(&id, &ann, label);
        if status == 200 {
            page.split(r#"data-testid="course-share-link-row" data-label=""#).nth(1).and_then(|r| r.split('"').next()).unwrap_or_default().to_string()
        } else {
            loc
        }
    };
    let made = || s.state.db.with(|c| c.query_row("SELECT COUNT(*) FROM course_shares", [], |r| r.get::<_, i64>(0))).unwrap();

    assert_eq!(share("  Bob   from\nwork "), "Bob from work");
    assert_eq!(share(""), "", "no name is fine");
    assert_eq!(share("Bob from work"), "Bob from work", "the same name twice is fine");
    assert_eq!(share(&"x".repeat(80)), "x".repeat(60), "cut at 60");
    assert_eq!(made(), 4);
    for _ in 4..50 {
        share("more");
    }
    assert_eq!(made(), 50);
    assert!(share("late").ends_with("notice=share-full"));
    assert_eq!(made(), 50, "nothing made past the limit");

    // The dialog says why in words, and lists the links with their names.
    let (status, _, page) = s.get(&format!("/courses/{id}?share=1&notice=share-full"), &ann);
    assert!(status == 200 && page.contains("A course can have up to 50 invitation links."), "{page}");
    assert!(page.contains(r#"data-label="Bob from work" data-opened="false">"#) && page.contains(r#"data-label="" data-opened="false">"#), "{page}");
    assert!(page.contains(r#"<td>Invitation link</td>"#), "an unnamed link is called that\n{page}");
}

/// A link opened while the course is being written (#144) is refused with
/// "not ready", but it counts as opened: the course joins the learner's
/// list once it is ready, without the link again. While the owner revises
/// it with the AI, the recipient keeps learning from it.
#[test]
fn a_link_opened_while_the_course_is_written_counts_once_it_is_ready() {
    let s = Server::start();
    let ann = s.account("ann@example.org");
    let id = s.course(&ann);
    let token = s.token(&id, &ann, "Bob");
    let bob = s.account("bob@example.org");
    let on_list = || s.get("/courses", &bob).2.contains(&format!(r#"data-testid="courses-card" data-course-id="{id}""#));

    s.set_state(&id, "generating");
    let (status, loc, _) = s.get(&format!("/courses/shared/{token}"), &bob);
    assert_eq!((status, loc.as_str()), (303, "/courses?notice=share-not-ready"));
    let (_, _, page) = s.get(&loc, &bob);
    assert!(page.contains(r#"data-testid="courses-notice""#) && page.contains("That course is not ready to open yet."), "{page}");
    assert!(!on_list(), "not while it is being written");

    s.set_state(&id, "ready");
    assert!(s.get(&format!("/courses/{id}"), &ann).2.contains(r#"data-label="Bob" data-opened="true""#), "the owner sees it opened");
    assert!(on_list(), "ready: on the list, no second click on the link");
    assert_eq!(s.post(&format!("/courses/open/{id}"), &bob, &[]).0, 303);
    assert_eq!(s.get("/lessons", &bob).0, 200);

    // Revising does not take it away from the recipient.
    s.set_state(&id, "revising");
    assert_eq!(s.get("/lessons", &bob).0, 200);
    assert!(on_list());
    let (status, loc, _) = s.get(&format!("/courses/shared/{token}"), &bob);
    assert_eq!((status, loc.as_str()), (303, "/"), "the link still opens it");
}

/// Share courses (#144, `Perm::ShareCourses`): without it no link is made,
/// by request or by button; links made before keep working, their owner
/// still removes them, and anyone may open one. An owner of the server
/// shares whatever is stored.
#[test]
fn making_a_link_needs_the_share_courses_permission() {
    let s = Server::start();
    // The first account is the server's owner: Ann is the second.
    s.account("root@example.org");
    let ann = s.account("ann@example.org");
    let id = s.course(&ann);
    let empty = s.course(&ann);
    let token = s.token(&id, &ann, "Bob");
    let set = |email: &str, sql: &'static str, n: i64| {
        let email = email.to_string();
        s.state.db.with(move |c| c.execute(sql, rusqlite::params![n, email])).unwrap();
    };
    let perms = |email: &str, bits: i64| set(email, "UPDATE users SET perms = ?1, preset_id = NULL WHERE email = ?2", bits);
    let made = || s.state.db.with(|c| c.query_row("SELECT COUNT(*) FROM course_shares", [], |r| r.get::<_, i64>(0))).unwrap();
    assert_eq!(made(), 1);

    // Teacher minus Share courses (63 - 8): refused, nothing made.
    perms("ann@example.org", 55);
    let (status, _, page) = s.share(&id, &ann, "Cat");
    assert!(status == 403 && page.contains(&crate::t("error.no_permission")), "{status}\n{page}");
    assert_eq!(made(), 1, "no link made");

    // The course with a link: Share is there, the dialog lists the link
    // with Remove and says why there is no form.
    let (status, _, page) = s.get(&format!("/courses/{id}?share=1"), &ann);
    assert_eq!(status, 200);
    assert!(page.contains(r#"data-testid="course-edit-share""#) && page.contains(r#"data-label="Bob" data-opened="false">"#) && page.contains(r#"data-testid="course-share-remove""#), "{page}");
    assert!(page.contains(r#"data-testid="course-share-not-allowed""#) && page.contains("Your account cannot make new invitation links."), "{page}");
    assert!(!page.contains("course-share-send") && !page.contains("course-share-form") && !page.contains("course-share-label"), "no form\n{page}");
    // The course without one: no button, no dialog.
    let (status, _, page) = s.get(&format!("/courses/{empty}?share=1"), &ann);
    assert_eq!(status, 200);
    assert!(!page.contains("course-edit-share") && !page.contains(r#"id="course-share""#), "{page}");

    // A Student (no permission at all) opens the link.
    let bob = s.account("bob@example.org");
    perms("bob@example.org", 0);
    let (status, loc, _) = s.get(&format!("/courses/shared/{token}"), &bob);
    assert_eq!((status, loc.as_str()), (303, "/"), "opening a link is never gated");
    assert!(s.get("/courses", &bob).2.contains(&format!(r#"data-testid="courses-card" data-course-id="{id}""#)));

    // Share without Create courses (8) is stored as nothing: refused.
    perms("ann@example.org", 8);
    assert_eq!(s.share(&id, &ann, "Cat").0, 403);
    assert_eq!(made(), 1);

    // Removing a link made before still works.
    let row = s.state.db.with(|c| c.query_row("SELECT id FROM course_shares", [], |r| r.get::<_, i64>(0))).unwrap();
    let (status, _, loc) = s.post(&format!("/courses/{id}/share/remove"), &ann, &[("id", &row.to_string())]);
    assert_eq!(status, 303);
    assert!(loc.ends_with("notice=share-removed"), "{loc}");
    assert_eq!(made(), 0);
    // With the last link gone, so is the button.
    assert!(!s.get(&format!("/courses/{id}"), &ann).2.contains("course-edit-share"));

    // An owner of the server has every permission, whatever is stored.
    perms("ann@example.org", 0);
    set("ann@example.org", "UPDATE users SET owner = ?1 WHERE email = ?2", 1);
    let (status, _, page) = s.share(&id, &ann, "Dan");
    assert!(status == 200 && page.contains(r#"data-testid="course-share-link""#), "{status}\n{page}");
    assert_eq!(made(), 1);
    assert!(s.get(&format!("/courses/{empty}"), &ann).2.contains(r#"data-testid="course-share-send""#), "the form, for an owner");
}
