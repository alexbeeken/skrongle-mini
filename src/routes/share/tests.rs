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

    /// Register and sign in; the session cookie.
    fn account(&self, email: &str) -> String {
        let (status, _, _) = self.post("/register", "", &[("email", email), ("password", "long-enough-pw"), ("age", "on")]);
        assert_eq!(status, 200, "registering shows the check-your-mail page");
        let (status, cookie, _) = self.post("/login", "", &[("email", email), ("password", "long-enough-pw")]);
        assert_eq!(status, 303, "signed in");
        cookie
    }

    /// A new course of the signed-in owner; its id.
    fn course(&self, owner: &str) -> String {
        let (status, _, loc) = self.post("/courses/new", owner, &[("title", "Fractions")]);
        assert_eq!(status, 303);
        loc.strip_prefix("/courses/").and_then(|l| l.strip_suffix("?generate=1")).expect("the editor").to_string()
    }

    /// The token of the newest invitation to `to`.
    fn token(&self, to: &str) -> String {
        let to = to.to_string();
        let text: String = self
            .state
            .db
            .with(move |c| c.query_row("SELECT body_text FROM mail_outbox WHERE to_addr = ?1 AND purpose = 'course-share' ORDER BY id DESC LIMIT 1", [to], |r| r.get(0)))
            .unwrap();
        let link = text.split_whitespace().find(|w| w.contains("/courses/shared/")).expect("the link");
        link.rsplit('/').next().unwrap().to_string()
    }

    /// Put the course in `state` (generating, revising, ready), as a job
    /// would, and drop the loaded copy so the next request reads it again.
    fn set_state(&self, id: &str, state: &str) {
        let (cid, s) = (id.to_string(), state.to_string());
        self.state.db.with(move |c| c.execute("UPDATE courses SET state = ?2 WHERE id = ?1", [cid, s])).unwrap();
        self.state.courses.invalidate(id);
    }
}

/// The share form refuses, with a notice and nothing sent: the owner's own
/// address in any case, an empty list, a bad address, more than 20 at a
/// time, and the address past the 50 a course may have.
#[test]
fn the_share_form_refuses_its_own_address_in_any_case_and_past_its_limits() {
    let s = Server::start();
    let ann = s.account("ann@example.org");
    let id = s.course(&ann);
    let share = |emails: &str| s.post(&format!("/courses/{id}/share"), &ann, &[("emails", emails)]).2;
    let outbox = || s.state.db.with(|c| c.query_row("SELECT COUNT(*) FROM mail_outbox WHERE purpose = 'course-share'", [], |r| r.get::<_, i64>(0))).unwrap();

    assert!(share("bob@example.org ANN@Example.ORG").ends_with("notice=share-own"));
    assert!(share(" \n ").ends_with("notice=share-none"));
    assert!(share("bob@example.org, not-an-address").ends_with("notice=share-bad"));
    let addresses = |from: usize, n: usize| (from..from + n).map(|i| format!("u{i}@example.org")).collect::<Vec<_>>().join("\n");
    assert!(share(&addresses(0, 21)).ends_with("notice=share-many"));
    assert_eq!(outbox(), 0, "nothing sent for a refused list");

    // Up to 50: 20 + 20 + 10; the 51st is refused, and so is a list that would go past 50.
    assert!(share(&addresses(0, 20)).ends_with("notice=share-sent-20-0"));
    assert!(share(&addresses(20, 20)).ends_with("notice=share-sent-20-0"));
    assert!(share(&addresses(40, 9)).ends_with("notice=share-sent-9-0"));
    assert!(share(&addresses(49, 2)).ends_with("notice=share-full"));
    assert!(share(&addresses(49, 1)).ends_with("notice=share-sent-1-0"));
    assert!(share("late@example.org").ends_with("notice=share-full"));
    assert_eq!(outbox(), 50);

    // The dialog says why in words.
    for (key, text) in [
        ("share-own", "That is your own address: the course is yours already."),
        ("share-full", "A course can be shared with up to 50 addresses."),
        ("share-many", "Share with up to 20 addresses at a time."),
        ("share-none", "Type at least one e-mail address."),
        ("share-bad", "One of those is not an e-mail address."),
    ] {
        let (status, _, page) = s.get(&format!("/courses/{id}?share=1&notice={key}"), &ann);
        assert!(status == 200 && page.contains(text), "{key}: {page}");
    }
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
    assert!(s.post(&format!("/courses/{id}/share"), &ann, &[("emails", "bob@example.org")]).2.ends_with("notice=share-sent-1-0"));
    let token = s.token("bob@example.org");
    let bob = s.account("bob@example.org");
    let on_list = || s.get("/courses", &bob).2.contains(&format!(r#"data-testid="courses-card" data-course-id="{id}""#));

    s.set_state(&id, "generating");
    let (status, loc, _) = s.get(&format!("/courses/shared/{token}"), &bob);
    assert_eq!((status, loc.as_str()), (303, "/courses?notice=share-not-ready"));
    let (_, _, page) = s.get(&loc, &bob);
    assert!(page.contains(r#"data-testid="courses-notice""#) && page.contains("That course is not ready to open yet."), "{page}");
    assert!(!on_list(), "not while it is being written");

    s.set_state(&id, "ready");
    assert!(s.get(&format!("/courses/{id}"), &ann).2.contains(r#"data-email="bob@example.org" data-opened="true""#), "the owner sees it opened");
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
