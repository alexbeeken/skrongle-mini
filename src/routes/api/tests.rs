//! Connected apps over HTTP (#167): `/api/v1/whoami` and its bearer-only
//! extractor, the profile's Make a token and Revoke, the owner's switch and
//! list, and the events that revoke every grant of an account.

use std::sync::Arc;

use crate::auth::api::{self, TOKEN_PREFIX};
use crate::AppState;

const PASSWORD: &str = "right-password";

/// A server on a free port, with real rate limits.
struct Server {
    _rt: tokio::runtime::Runtime,
    state: Arc<AppState>,
    base: String,
    agent: ureq::Agent,
}

/// What came back: status, headers we look at, body.
struct Answer {
    status: u16,
    location: String,
    cache: String,
    set_cookie: String,
    www_auth: String,
    body: String,
}

impl Server {
    fn start(proxy_from: Option<&str>) -> Server {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let listener = rt.block_on(tokio::net::TcpListener::bind("127.0.0.1:0")).unwrap();
        let addr = listener.local_addr().unwrap();
        let mut s = Arc::into_inner(crate::routes::test_state(false)).unwrap();
        s.proxy_auth = proxy_from.map(|f| crate::proxy_auth::Config::parse(Some("Remote-Email"), Some(f)).unwrap().unwrap());
        let state = Arc::new(s);
        let app = crate::routes::router(state.clone());
        rt.spawn(async move { axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap() });
        let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
        Server { _rt: rt, state, base: format!("http://{addr}"), agent }
    }

    /// An account with the password `PASSWORD` (Teacher set) and a session cookie for it.
    fn account(&self, email: &str) -> (i64, String) {
        let hash = crate::auth::hash_password(PASSWORD).unwrap();
        let id = self.state.db.with(|c| crate::perms::create_teacher(c, email, &hash)).unwrap();
        (id, self.cookie(id))
    }

    fn cookie(&self, id: i64) -> String {
        format!("sid={}", self.state.db.with(|c| crate::auth::create_session(c, id)).unwrap())
    }

    fn apps_on(&self, on: bool) {
        self.state.db.with(|c| crate::settings::set_apps_on(c, on)).unwrap();
    }

    fn answer(mut r: ureq::http::Response<ureq::Body>) -> Answer {
        let h = |name: &str| r.headers().get(name).map(|v| v.to_str().unwrap().to_string()).unwrap_or_default();
        let (status, location, cache, set_cookie, www_auth) = (r.status().as_u16(), h("location"), h("cache-control"), h("set-cookie"), h("www-authenticate"));
        Answer { status, location, cache, set_cookie, www_auth, body: r.body_mut().read_to_string().unwrap_or_default() }
    }

    fn get(&self, path: &str, headers: &[(&str, &str)]) -> Answer {
        let mut req = self.agent.get(&format!("{}{path}", self.base));
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        Self::answer(req.call().unwrap())
    }

    fn post(&self, path: &str, cookie: &str, form: &[(&str, &str)]) -> Answer {
        Self::answer(self.agent.post(&format!("{}{path}", self.base)).header("cookie", cookie).send_form(form.iter().copied()).unwrap())
    }

    /// GET /api/v1/whoami with this token.
    fn whoami(&self, token: &str) -> Answer {
        self.get("/api/v1/whoami", &[("authorization", &format!("Bearer {token}"))])
    }

    /// Make a token on the profile: the answer, and the token it shows.
    fn make_token(&self, cookie: &str, name: &str, password: &str) -> (Answer, Option<String>) {
        let a = self.post(
            "/profile/apps/token",
            cookie,
            &[("name", name), ("scope", "courses:read"), ("scope", "courses:write"), ("lifetime", "90"), ("password", password)],
        );
        let token = a.body.find(TOKEN_PREFIX).map(|i| a.body[i..i + TOKEN_PREFIX.len() + 64].to_string());
        (a, token)
    }
}

fn stored_hashes(state: &AppState) -> Vec<String> {
    state.db.with(|c| c.prepare("SELECT token_hash FROM api_tokens")?.query_map([], |r| r.get(0))?.collect()).unwrap()
}

/// Off by default: no Connected apps section, and making a token, revoking
/// and the API all answer 404. On, the section shows; a wrong password makes
/// no token; the right one shows it once, `no-store`, and only its hash is
/// stored; whoami answers with the account, the name and both scopes, and
/// the profile then lists the grant as used. Off again, the token answers
/// 404; on again it works.
#[test]
fn make_a_token_and_use_it() {
    let s = Server::start(None);
    let (ann, me) = s.account("ann@example.org");

    let profile = s.get("/profile", &[("cookie", &me)]);
    assert_eq!(profile.status, 200);
    assert!(!profile.body.contains("data-testid=\"profile-apps\""), "no section while the switch is off");
    assert_eq!(s.make_token(&me, "Test", PASSWORD).0.status, 404);
    assert_eq!(s.post("/profile/apps/revoke", &me, &[("id", "1")]).status, 404);
    assert_eq!(s.get("/api/v1/whoami", &[]).status, 404);

    s.apps_on(true);
    let profile = s.get("/profile", &[("cookie", &me)]);
    assert!(profile.body.contains("data-testid=\"profile-apps-none\""), "the section, empty");
    assert!(profile.body.contains("data-testid=\"profile-apps-make-open\""));

    let (wrong, token) = s.make_token(&me, "Test", "wrong-password");
    assert_eq!(wrong.status, 403);
    assert!(token.is_none() && wrong.body.contains("profile-apps-token-error"));
    assert!(stored_hashes(&s.state).is_empty(), "a wrong password makes no token");

    let (made, token) = s.make_token(&me, "Test", PASSWORD);
    assert_eq!(made.status, 200);
    assert_eq!(made.cache, "no-store", "the page with the token is not cached");
    let token = token.expect("the token is shown");
    assert_eq!(made.body.matches(&token).count(), 1, "shown once");
    let hashes = stored_hashes(&s.state);
    assert_eq!(hashes, vec![crate::auth::hash_token(&token)]);
    assert!(!made.body.contains(&hashes[0]), "the hash is in no page");

    let me_again = s.get("/profile", &[("cookie", &me)]);
    assert!(!me_again.body.contains(&token) && !me_again.body.contains(&hashes[0]), "never shown again");
    assert!(me_again.body.contains("data-used=\"false\""));

    let who = s.whoami(&token);
    assert_eq!(who.status, 200, "{}", who.body);
    assert_eq!(who.cache, "no-store");
    let v: serde_json::Value = serde_json::from_str(&who.body).unwrap();
    assert_eq!(v["user_id"], ann);
    assert_eq!(v["client_name"], "Test");
    assert_eq!(v["client_id"], api::PERSONAL_CLIENT_ID);
    assert_eq!(v["scopes"], serde_json::json!(["courses:read", "courses:write"]));
    assert!(!who.body.contains(&token) && !who.body.contains(&hashes[0]));
    assert!(s.get("/profile", &[("cookie", &me)]).body.contains("data-used=\"true\""), "last used is shown");

    s.apps_on(false);
    let off = s.whoami(&token);
    assert_eq!((off.status, off.body.as_str()), (404, ""), "off: no API at all");
    s.apps_on(true);
    assert_eq!(s.whoami(&token).status, 200, "on again: the grant is back");

    // Revoke on the profile.
    let id = s.state.db.with(|c| api::grants(c, ann, crate::db::now())).unwrap()[0].id.to_string();
    let r = s.post("/profile/apps/revoke", &me, &[("id", &id)]);
    assert_eq!((r.status, r.location.as_str()), (303, "/profile?saved=app-revoked#apps"));
    assert_eq!(s.whoami(&token).status, 401);
}

/// Bearer only: a session cookie on the API is a 401 with an empty body that
/// names nobody; a bearer token on a page is the sign-in redirect; the proxy
/// sign-in header from a trusted address gives no API access and starts no
/// session there.
#[test]
fn bearer_only_never_the_cookie_or_the_proxy_header() {
    let s = Server::start(Some("127.0.0.1"));
    let (_, me) = s.account("ann@example.org");
    s.apps_on(true);
    let token = s.make_token(&me, "Test", PASSWORD).1.unwrap();

    let cookie = s.get("/api/v1/whoami", &[("cookie", &me)]);
    assert_eq!(cookie.status, 401);
    assert_eq!(cookie.body, "", "an empty body");
    assert_eq!(cookie.www_auth, "Bearer");

    let page = s.get("/profile", &[("authorization", &format!("Bearer {token}"))]);
    assert_eq!((page.status, page.location.as_str()), (303, "/login"), "a token opens no page");

    let proxied = s.get("/api/v1/whoami", &[("remote-email", "ann@example.org")]);
    assert_eq!(proxied.status, 401, "the proxy header is no token");
    assert_eq!(proxied.set_cookie, "", "and starts no session on the API");
    // The header still signs in on a page, so the test's proxy is trusted.
    assert_eq!(s.get("/profile", &[("remote-email", "ann@example.org")]).status, 200);

    // A wrong token, an expired-looking one, a hash: the same empty 401.
    let mut near = token.clone();
    near.pop();
    near.push(if token.ends_with('0') { '1' } else { '0' });
    for bad in [near.as_str(), &crate::auth::hash_token(&token), "nonsense"] {
        let a = s.whoami(bad);
        assert_eq!((a.status, a.body.as_str()), (401, ""), "{bad}");
    }
}

/// Never in the address: `?access_token=` or `?token=` is a 400, even
/// beside a good header.
#[test]
fn a_token_in_the_address_is_refused() {
    let s = Server::start(None);
    let (_, me) = s.account("ann@example.org");
    s.apps_on(true);
    let token = s.make_token(&me, "Test", PASSWORD).1.unwrap();
    let auth = format!("Bearer {token}");
    for q in [format!("access_token={token}"), format!("token={token}"), "Access_Token=x".to_string()] {
        let a = s.get(&format!("/api/v1/whoami?{q}"), &[("authorization", &auth)]);
        assert_eq!(a.status, 400, "{q}");
        assert!(!a.body.contains(&token));
    }
    assert_eq!(s.get("/api/v1/whoami", &[("authorization", &auth)]).status, 200);
}

/// Ten bad tokens from one address in ten minutes, then a bad token is 429
/// (#185). A request with no `Authorization` header is a 401 and is not
/// counted (it is how an MCP client finds `resource_metadata`); a good token
/// is served even when its address is over the limit. An expired token is
/// refused like an unknown one.
#[test]
fn bad_tokens_are_limited_per_address() {
    let s = Server::start(None);
    let (_, me) = s.account("ann@example.org");
    s.apps_on(true);
    let token = s.make_token(&me, "Test", PASSWORD).1.unwrap();
    let expired = s.make_token(&me, "Old", PASSWORD).1.unwrap();
    s.state.db.with(|c| c.execute("UPDATE api_tokens SET expires_at = 0 WHERE token_hash = ?1", [crate::auth::hash_token(&expired)])).unwrap();
    let max = crate::security::API_BAD_TOKENS_PER_IP.max;
    let no_header = || s.get("/api/v1/whoami", &[]).status;
    for i in 0..max {
        assert_eq!(no_header(), 401, "no header {i}");
    }
    let bad = format!("{TOKEN_PREFIX}{}", "0".repeat(64));
    for i in 0..max {
        assert_eq!(s.whoami(&bad).status, 401, "bad token {i}: requests with no header were not counted");
    }
    assert_eq!(s.whoami(&token).status, 200, "a good token is served over the limit");
    assert_eq!(s.whoami(&bad).status, 429, "an eleventh bad token");
    assert_eq!(s.whoami(&expired).status, 429, "an expired token is refused like an unknown one");
    assert_eq!(s.get("/api/v1/whoami", &[("authorization", "Basic abc")]).status, 429, "a header that holds no token of ours counts");
    assert_eq!(no_header(), 401, "no header is still not a guess");
    assert_eq!(s.whoami(&token).status, 200);
}

/// Checked on every request: the account losing Create courses, or waiting
/// behind a temporary password, gets a 401 at its next request, and its
/// profile shows no section (a Student) and refuses to make a token.
#[test]
fn permission_and_temporary_password_are_checked_each_request() {
    let s = Server::start(None);
    let (ann, me) = s.account("ann@example.org");
    s.apps_on(true);
    let token = s.make_token(&me, "Test", PASSWORD).1.unwrap();
    assert_eq!(s.whoami(&token).status, 200);

    s.state.db.with(|c| crate::perms::apply_preset(c, ann, crate::perms::STUDENT_PRESET)).unwrap();
    assert_eq!(s.whoami(&token).status, 401, "no Create courses");
    assert!(!s.get("/profile", &[("cookie", &me)]).body.contains("data-testid=\"profile-apps\""), "a Student has no section");
    assert_eq!(s.make_token(&me, "Two", PASSWORD).0.status, 404);
    s.state.db.with(|c| crate::perms::apply_preset(c, ann, crate::perms::TEACHER_PRESET)).unwrap();
    assert_eq!(s.whoami(&token).status, 200, "the grant was kept");

    s.state.db.with(|c| c.execute("UPDATE users SET must_change_password = 1 WHERE id = ?1", [ann])).unwrap();
    assert_eq!(s.whoami(&token).status, 401, "waiting behind a temporary password");
}

/// Revoked by events: changing the password on the profile, replacing a
/// temporary password, the owner setting a temporary password and the owner
/// turning off two-factor each delete every grant of the account. The
/// screens say so.
#[test]
fn password_and_two_factor_events_revoke_every_grant() {
    let s = Server::start(None);
    let (root, owner) = s.account("root@example.org");
    s.state.db.with(|c| crate::owner::make_owner(c, root)).unwrap();
    let (ann, me) = s.account("ann@example.org");
    s.apps_on(true);

    // The profile's password change.
    let token = s.make_token(&me, "Test", PASSWORD).1.unwrap();
    assert!(s.get("/profile", &[("cookie", &me)]).body.contains("profile-password-apps-note"), "the dialog says so");
    let new = "a-new-password";
    let r = s.post("/profile/password", &me, &[("current", PASSWORD), ("new", new), ("confirm", new)]);
    assert_eq!((r.status, r.location.as_str()), (303, "/profile?saved=password-apps"));
    assert!(s.get("/profile?saved=password-apps", &[("cookie", &me)]).body.contains(&crate::t("apps.password_changed")));
    assert_eq!(s.whoami(&token).status, 401);
    let hash = crate::auth::hash_password(PASSWORD).unwrap();
    s.state.db.with(|c| crate::profile::set_password_hash(c, ann, &hash)).unwrap();

    // The owner sets a temporary password.
    let token = s.make_token(&me, "Test", PASSWORD).1.unwrap();
    let manage = s.get("/manage", &[("cookie", &owner)]);
    assert!(manage.body.contains(&crate::t("apps.manage_revoked_too")), "the owner's dialogs say so");
    assert_eq!(s.post("/manage/temp-password", &owner, &[("email", "ann@example.org")]).status, 200);
    assert_eq!(s.whoami(&token).status, 401);

    // Replacing the temporary password (whoever had it may have connected an app).
    s.state.db.with(|c| crate::profile::set_password_hash(c, ann, &hash)).unwrap();
    let me = s.cookie(ann);
    let token = s.make_token(&me, "Test", PASSWORD).1.unwrap();
    let temp = crate::auth::hash_password("temporary-1").unwrap();
    s.state.db.with(|c| c.execute("UPDATE users SET password_hash = ?2, must_change_password = 1 WHERE id = ?1", rusqlite::params![ann, temp])).unwrap();
    let r = s.post("/password/change", &me, &[("new", new), ("confirm", new), ("next", "/")]);
    assert_eq!(r.status, 303);
    assert_eq!(s.whoami(&token).status, 401);
    s.state.db.with(|c| crate::profile::set_password_hash(c, ann, &hash)).unwrap();

    // The owner turns off two-factor.
    let token = s.make_token(&me, "Test", PASSWORD).1.unwrap();
    assert_eq!(s.whoami(&token).status, 200);
    assert_eq!(s.post("/manage/two-factor-off", &owner, &[("email", "ann@example.org")]).status, 303);
    assert_eq!(s.whoami(&token).status, 401);
    assert!(stored_hashes(&s.state).is_empty());
}

/// Making a token is a sensitive action: counted by `PASSWORD_PER_USER`
/// (past it even the right password is refused with a 429), and not offered
/// to an account without a password (signed in through the proxy header).
#[test]
fn making_a_token_needs_the_password_and_is_limited() {
    let s = Server::start(None);
    let (_, me) = s.account("ann@example.org");
    s.apps_on(true);
    for _ in 0..crate::security::PASSWORD_PER_USER.max {
        assert_eq!(s.make_token(&me, "Test", "wrong-password").0.status, 403);
    }
    let (over, token) = s.make_token(&me, "Test", PASSWORD);
    assert_eq!(over.status, 429);
    assert!(token.is_none());

    let none = s.state.db.with(|c| crate::perms::create_teacher(c, "proxy@example.org", "")).unwrap();
    let theirs = s.cookie(none);
    let page = s.get("/profile", &[("cookie", &theirs)]);
    assert!(page.body.contains("data-testid=\"profile-apps\"") && !page.body.contains("profile-apps-make-open"), "no Make a token");
    assert!(page.body.contains("profile-apps-no-password"));
    assert_eq!(s.make_token(&theirs, "Test", "").0.status, 404);
    assert!(stored_hashes(&s.state).is_empty());
}

/// A fifth grant is refused with a message saying to revoke one.
#[test]
fn a_fifth_token_is_refused_on_the_profile() {
    let s = Server::start(None);
    let (_, me) = s.account("ann@example.org");
    s.apps_on(true);
    for i in 0..crate::caps::MAX_GRANTS_PER_ACCOUNT {
        assert!(s.make_token(&me, &format!("T{i}"), PASSWORD).1.is_some());
    }
    let (fifth, token) = s.make_token(&me, "T5", PASSWORD);
    assert_eq!(fifth.status, 409);
    assert!(token.is_none());
    assert!(fifth.body.contains(&crate::error::html_escape(&api::Refused::TooManyGrants.message())));
}

/// The owner's switch and list on Manage Users: off by default; the list
/// names the app and the account, without the token or its hash; Revoke
/// ends it. A superuser sees neither and may not post either route.
#[test]
fn the_owner_switches_apps_on_and_revokes_any_grant() {
    let s = Server::start(None);
    let (root, owner) = s.account("root@example.org");
    s.state.db.with(|c| crate::owner::make_owner(c, root)).unwrap();
    let (sue, superuser) = s.account("sue@example.org");
    s.state.db.with(|c| crate::perms::set_custom(c, sue, crate::perms::Perms::TEACHER.with(crate::perms::Perm::Superuser, true))).unwrap();
    let (_, me) = s.account("ann@example.org");

    let page = s.get("/manage", &[("cookie", &owner)]);
    assert!(page.body.contains("data-apps-on=\"false\""), "off by default");
    assert_eq!(s.post("/manage/apps", &superuser, &[("on", "1")]).status, 404);
    let r = s.post("/manage/apps", &owner, &[("on", "1")]);
    assert_eq!((r.status, r.location.as_str()), (303, "/manage?did=apps-on#apps"));

    let token = s.make_token(&me, "Test", PASSWORD).1.unwrap();
    let page = s.get("/manage", &[("cookie", &owner)]);
    assert!(page.body.contains("data-apps-on=\"true\""));
    assert!(page.body.contains("data-testid=\"manage-app\"") && page.body.contains("ann@example.org"));
    assert!(!page.body.contains(&token) && !page.body.contains(&crate::auth::hash_token(&token)));
    let sup = s.get("/manage", &[("cookie", &superuser)]);
    assert!(!sup.body.contains("manage-apps-card") && !sup.body.contains("data-testid=\"manage-app\""), "not for a superuser");

    let id = s.state.db.with(|c| api::all_grants(c, crate::db::now())).unwrap()[0].id.to_string();
    assert_eq!(s.post("/manage/apps/revoke", &superuser, &[("id", &id)]).status, 404);
    assert_eq!(s.whoami(&token).status, 200);
    assert_eq!(s.post("/manage/apps/revoke", &owner, &[("id", &id)]).status, 303);
    assert_eq!(s.whoami(&token).status, 401);
}

/// No log line holds a token or its hash: the lines of making a token and
/// using it, captured. Run in a process of its own (this test binary again,
/// this one test): tests on other threads would otherwise race tracing's
/// per-callsite cache and the capture could come back empty.
#[test]
fn no_token_or_hash_in_the_log() {
    const CHILD: &str = "SKRONGLE_LOG_CAPTURE_CHILD";
    if std::env::var_os(CHILD).is_some() {
        return log_capture();
    }
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["routes::api::tests::no_token_or_hash_in_the_log", "--exact", "--test-threads=1", "--nocapture"])
        .env(CHILD, "1")
        .output()
        .unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && text.contains("1 passed"), "{text}");
}

fn log_capture() {
    use std::io::Write;
    use tower::ServiceExt;

    #[derive(Clone, Default)]
    struct Buf(Arc<std::sync::Mutex<Vec<u8>>>);
    impl Write for Buf {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Buf {
        type Writer = Buf;
        fn make_writer(&'a self) -> Buf {
            self.clone()
        }
    }

    let buf = Buf::default();
    let subscriber = tracing_subscriber::fmt().with_writer(buf.clone()).with_max_level(tracing::Level::TRACE).finish();
    let state = crate::routes::test_state(false);
    let hash = crate::auth::hash_password(PASSWORD).unwrap();
    let ann = state.db.with(|c| crate::perms::create_teacher(c, "ann@example.org", &hash)).unwrap();
    let sid = state.db.with(|c| crate::auth::create_session(c, ann)).unwrap();
    state.db.with(|c| crate::settings::set_apps_on(c, true)).unwrap();
    let app = crate::routes::router(state.clone());
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let token = tracing::subscriber::with_default(subscriber, || {
        // A log line first hit by another test's thread, with no subscriber
        // there, may have been cached as "never": ask every one again.
        tracing::callsite::rebuild_interest_cache();
        rt.block_on(async {
            let body = format!("name=Test&scope=courses%3Aread&lifetime=90&password={PASSWORD}");
            let req = axum::http::Request::post("/profile/apps/token")
                .header("cookie", format!("sid={sid}"))
                .header("content-type", "application/x-www-form-urlencoded")
                .body(axum::body::Body::from(body))
                .unwrap();
            let res = app.clone().oneshot(req).await.unwrap();
            let page = String::from_utf8(axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap().to_vec()).unwrap();
            let i = page.find(TOKEN_PREFIX).unwrap();
            let token = page[i..i + TOKEN_PREFIX.len() + 64].to_string();
            for t in [token.clone(), "skr_0000".to_string()] {
                let req = axum::http::Request::get("/api/v1/whoami").header("authorization", format!("Bearer {t}")).body(axum::body::Body::empty()).unwrap();
                app.clone().oneshot(req).await.unwrap();
            }
            token
        })
    });
    let log = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
    assert!(log.contains("made a personal API token"), "the log was captured: {log}");
    assert!(!log.contains(&token[TOKEN_PREFIX.len()..]), "the token is in the log");
    assert!(!log.contains(&crate::auth::hash_token(&token)), "the hash is in the log");
}

/// #170: no template or script holds a token. The prefix appears nowhere
/// in `templates/` or `static/`, and the one place a token is shown is the
/// profile's one-time dialog (`new_token`), which only a just-made token
/// fills.
#[test]
fn no_template_shows_a_token_but_the_one_time_dialog() {
    let mut shown_in = Vec::new();
    for dir in ["templates", "static"] {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            assert!(!text.contains(TOKEN_PREFIX), "{} names the token prefix", path.display());
            if text.contains("new_token") || text.contains("token_hash") {
                shown_in.push(path.display().to_string());
            }
        }
    }
    assert_eq!(shown_in, vec!["templates/profile.html".to_string()]);
    let profile = std::fs::read_to_string("templates/profile.html").unwrap();
    assert!(profile.contains("{% if let Some(token) = a.new_token %}"));
    assert!(!profile.contains("token_hash"));
}
