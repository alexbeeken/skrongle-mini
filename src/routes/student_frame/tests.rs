//! The student frame's rules (#219): every page in `FRAME_PAGES` is one
//! document and one stylesheet, with no script, font or picture;
//! `static/student.css` uses no CSS variable and stays small; a student
//! changes its own password with form posts only.

use super::FRAME_PAGES;

/// The stylesheet's size limit, in bytes. It is sent to every student on
/// every computer in the room; raise it only for a reason.
const STUDENT_CSS_MAX: usize = 8 * 1024;

const PASSWORD: &str = "right-password";

/// Fails unless `html` (the page at `path`) is light: no `<script`, no
/// `<style` or `style=`, no picture, frame or media element, no font, and
/// exactly one stylesheet, `student.css`. The only other `<link>` is the
/// empty icon that keeps the browser from asking for `/favicon.ico`. A test
/// of a page in the frame calls this on what it fetched.
pub(crate) fn assert_light(path: &str, html: &str) {
    let low = html.to_ascii_lowercase();
    for bad in ["<script", "<style", " style=", "<img", "<svg", "<iframe", "<object", "<embed", "<video", "<audio", "@font-face", ".woff", ".ttf", ".otf", "manifest"] {
        assert!(!low.contains(bad), "{path} has `{bad}`:\n{html}");
    }
    let links: Vec<&str> = low
        .match_indices("<link")
        .map(|(i, _)| &low[i..low[i..].find('>').map_or(low.len(), |e| i + e + 1)])
        .collect();
    let sheets: Vec<&&str> = links.iter().filter(|l| l.contains("stylesheet")).collect();
    assert_eq!(sheets.len(), 1, "{path}: one stylesheet, not {links:?}");
    assert!(sheets[0].contains("/static/student.css"), "{path}: the stylesheet is student.css, not {}", sheets[0]);
    for l in &links {
        assert!(l.contains("stylesheet") || l.contains(r#"rel="icon" href="data:,""#), "{path}: nothing else is loaded: {l}");
    }
    assert!(html.contains(r#"data-testid="student-bar""#), "{path}: the frame's top bar");
}

/// A small server on the real router: (status, Location, the first
/// Set-Cookie's `name=value`, body). Redirects are not followed.
struct Server {
    rt: tokio::runtime::Runtime,
    base: String,
    agent: ureq::Agent,
    state: std::sync::Arc<crate::AppState>,
}

type Reply = (u16, String, String, String);

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
        Server { rt, base, agent, state }
    }

    fn send(r: Result<ureq::http::Response<ureq::Body>, ureq::Error>) -> Reply {
        let mut r = r.unwrap();
        let header = |k: &str| r.headers().get(k).map(|v| v.to_str().unwrap().to_string()).unwrap_or_default();
        let (location, cookie) = (header("location"), header("set-cookie").split(';').next().unwrap_or_default().to_string());
        (r.status().as_u16(), location, cookie, r.body_mut().read_to_string().unwrap_or_default())
    }

    fn get(&self, path: &str, cookie: &str) -> Reply {
        Self::send(self.agent.get(&format!("{}{path}", self.base)).header("cookie", cookie).call())
    }

    fn post(&self, path: &str, cookie: &str, form: &[(&str, &str)]) -> Reply {
        Self::send(self.agent.post(&format!("{}{path}", self.base)).header("cookie", cookie).send_form(form.iter().copied()))
    }

    /// Signs in and returns the session cookie ('' when refused).
    fn login(&self, name: &str, password: &str) -> String {
        self.post("/login", "", &[("email", name), ("password", password)]).2
    }

    /// A teacher (`t@example.org`) and one student, `kid1`, both with `PASSWORD`.
    fn with_student(&self) {
        let hash = crate::auth::hash_password(PASSWORD).unwrap();
        self.state.db.with(|c| crate::students::tests::teacher_with_students(c, "t@example.org", "kid", 1, &hash)).unwrap();
        let _ = &self.rt;
    }
}

/// The test from the issue: every route in the frame, opened as a student
/// (the sign-in page signed out), is one document and one stylesheet with
/// no script and no font.
#[test]
fn every_page_in_the_frame_is_light() {
    let s = Server::start();
    s.with_student();
    let kid = s.login("kid1", PASSWORD);
    assert!(kid.starts_with("sid="), "the student signs in");
    for path in FRAME_PAGES {
        let cookie = if *path == "/login" { "" } else { kid.as_str() };
        let (status, _, _, page) = s.get(path, cookie);
        assert_eq!(status, 200, "{path}");
        assert_light(path, &page);
    }
    // The sign-in page's refusals are in the frame too.
    let (status, _, _, page) = s.post("/login", "", &[("email", "kid1"), ("password", "wrong-password")]);
    assert_eq!(status, 200);
    assert!(page.contains(r#"data-testid="login-error""#));
    assert_light("/login (refused)", &page);
}

/// The top bar: the student's name (its username while it has none) and
/// the three plain links; Sign out is a form post.
#[test]
fn the_top_bar_names_the_student_and_has_three_links() {
    let s = Server::start();
    s.with_student();
    let kid = s.login("kid1", PASSWORD);
    let (_, _, _, page) = s.get("/password", &kid);
    assert!(page.contains(r#"<span data-testid="student-bar-name">kid1</span>"#), "{page}");
    for (testid, text) in [("student-my-subjects", "My subjects"), ("student-change-password", "Change my password"), ("student-sign-out", "Sign out")] {
        assert!(page.contains(&format!(r#"data-testid="{testid}">{text}<"#)), "{testid}: {page}");
    }
    assert!(page.contains(r#"<form method="post" action="/logout""#));
    // Signed out, the sign-in page's bar has no links.
    let (_, _, _, login) = s.get("/login", "");
    assert!(!login.contains("student-sign-out") && !login.contains("student-bar-name"));
}

/// The stylesheet's own rules: no CSS variable, no font or picture to
/// fetch, no layout that needs flexbox or grid, and under its size.
#[test]
fn student_css_stays_plain_and_small() {
    let file = include_str!("../../../static/student.css");
    assert!(file.len() < STUDENT_CSS_MAX, "static/student.css is {} bytes, over {STUDENT_CSS_MAX}", file.len());
    // The rules without the comments, which may name what they leave out.
    let css: String = file.split("/*").enumerate().map(|(i, part)| if i == 0 { part } else { part.split_once("*/").map_or("", |(_, rest)| rest) }).collect();
    for bad in ["var(", "--", "url(", "@import", "@font-face", "flex", "grid", "calc("] {
        assert!(!css.contains(bad), "static/student.css has `{bad}`");
    }
    // Body text 16px or more: no smaller size anywhere.
    for size in css.split("font-size:").skip(1) {
        let size = size.trim_start();
        if let Some(px) = size.split("px").next().and_then(|n| n.trim().parse::<u32>().ok()) {
            assert!(px >= 16, "a font size under 16px: {size:.20}");
        } else {
            assert!(size.starts_with("inherit"), "font sizes are in px or inherited: {size:.20}");
        }
    }
}

/// Change my password: the current one is checked, the new one twice; done,
/// the page says so, the old password stops working and the student's
/// other session ends. A normal account goes to Profile settings.
#[test]
fn a_student_changes_its_password() {
    let s = Server::start();
    s.with_student();
    let kid = s.login("kid1", PASSWORD);
    let other = s.login("kid1", PASSWORD);
    let (status, _, _, page) = s.get("/password", &kid);
    assert_eq!(status, 200);
    assert!(page.contains(r#"data-testid="student-password-form""#) && !page.contains("student-password-saved"));

    let change = |current: &str, new: &str, confirm: &str| s.post("/password", &kid, &[("current", current), ("new", new), ("confirm", confirm)]);
    let (status, _, _, page) = change(PASSWORD, "new-password-1", "new-password-2");
    assert_eq!(status, 200);
    assert!(page.contains("The new passwords do not match."), "{page}");
    assert_light("/password (mismatch)", &page);
    let (status, _, _, page) = change("wrong-password", "new-password-1", "new-password-1");
    assert_eq!(status, 200);
    assert!(page.contains("The current password is not right."), "{page}");
    let (status, _, _, page) = change(PASSWORD, "short", "short");
    assert_eq!(status, 200);
    assert!(page.contains(r#"data-testid="student-password-error""#), "{page}");

    let (status, location, _, _) = change(PASSWORD, "new-password-1", "new-password-1");
    assert_eq!((status, location.as_str()), (303, "/password?saved=1"));
    let (status, _, _, page) = s.get("/password?saved=1", &kid);
    assert_eq!(status, 200);
    assert!(page.contains(r#"data-testid="student-password-saved""#));
    assert_eq!(s.get("/password", &other).0, 303, "the other session ended");
    assert_eq!(s.login("kid1", PASSWORD), "", "the old password no longer signs in");
    assert!(s.login("kid1", "new-password-1").starts_with("sid="));

    // A normal account changes its password on Profile settings.
    let teacher = s.login("t@example.org", PASSWORD);
    let (status, location, _, _) = s.get("/password", &teacher);
    assert_eq!((status, location.as_str()), (303, "/profile#account"));
    let (status, location, _, _) = s.post("/password", &teacher, &[("current", PASSWORD), ("new", "new-password-1"), ("confirm", "new-password-1")]);
    assert_eq!((status, location.as_str()), (303, "/profile#account"));
    assert!(s.login("t@example.org", PASSWORD).starts_with("sid="), "nothing changed");
    // Signed out: the sign-in page.
    assert_eq!(s.get("/password", "").0, 303);
}
