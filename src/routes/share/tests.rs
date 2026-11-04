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
