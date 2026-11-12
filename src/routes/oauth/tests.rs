//! The OAuth endpoints over HTTP (#169): the real router on a free port with
//! real rate limits. `Site::start` starts it (with or without a public
//! address, with or without a signing-in proxy); `Site::flow` registers a
//! client and builds a request; `Site::consent` shows the consent page;
//! `Site::allow` answers it; `Site::connect` does all three and returns
//! the code; `Site::trade` posts it to the token endpoint.

use serde_json::Value;
use std::sync::Arc;

use crate::auth::api::Scope;
use crate::oauth_server::s256;
use crate::AppState;

const PASSWORD: &str = "right-password";
const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
const CB: &str = "https://app.example/cb";

struct Site {
    _rt: tokio::runtime::Runtime,
    state: Arc<AppState>,
    /// `http://127.0.0.1:<port>`: also the public address when one is set.
    base: String,
    agent: ureq::Agent,
}

struct Answer {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

impl Answer {
    fn header(&self, name: &str) -> &str {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map_or("", |(_, v)| v.as_str())
    }

    fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap_or(Value::Null)
    }

    /// The query value `name` of the `Location` header.
    fn location_param(&self, name: &str) -> Option<String> {
        let loc = self.header("location");
        let q = loc.split_once('?')?.1;
        let uri: axum::http::Uri = format!("/x?{q}").parse().ok()?;
        let axum::extract::Query(pairs) = axum::extract::Query::<Vec<(String, String)>>::try_from_uri(&uri).ok()?;
        pairs.into_iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }
}

/// One authorization request, its client registered.
struct Flow {
    client_id: String,
    pairs: Vec<(String, String)>,
}

impl Flow {
    fn query(&self) -> String {
        self.pairs.iter().map(|(k, v)| format!("{k}={}", crate::urlenc::urlencode(v))).collect::<Vec<_>>().join("&")
    }

    fn without(mut self, name: &str) -> Flow {
        self.pairs.retain(|(k, _)| k != name);
        self
    }

    fn with(mut self, name: &str, value: &str) -> Flow {
        self.pairs.retain(|(k, _)| k != name);
        self.pairs.push((name.into(), value.into()));
        self
    }
}

impl Site {
    fn start(public: bool, proxy_from: Option<&str>) -> Site {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let listener = rt.block_on(tokio::net::TcpListener::bind("127.0.0.1:0")).unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let mut s = Arc::into_inner(crate::routes::test_state(false)).unwrap();
        s.public_url = public.then(|| base.clone());
        s.proxy_auth = proxy_from.map(|f| crate::proxy_auth::Config::parse(Some("Remote-Email"), Some(f)).unwrap().unwrap());
        let state = Arc::new(s);
        state.db.with(|c| crate::settings::set_apps_on(c, true)).unwrap();
        let app = crate::routes::router(state.clone());
        rt.spawn(async move { axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap() });
        let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
        Site { _rt: rt, state, base, agent }
    }

    /// A Teacher account and a session cookie for it.
    fn account(&self, email: &str) -> (i64, String) {
        let hash = crate::auth::hash_password(PASSWORD).unwrap();
        let id = self.state.db.with(|c| crate::perms::create_teacher(c, email, &hash)).unwrap();
        let sid = self.state.db.with(|c| crate::auth::create_session(c, id)).unwrap();
        (id, format!("sid={sid}"))
    }

    fn apps_on(&self, on: bool) {
        self.state.db.with(|c| crate::settings::set_apps_on(c, on)).unwrap();
    }

    fn answer(mut r: ureq::http::Response<ureq::Body>) -> Answer {
        let headers = r.headers().iter().map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string())).collect();
        Answer { status: r.status().as_u16(), headers, body: r.body_mut().read_to_string().unwrap_or_default() }
    }

    fn get(&self, path: &str, headers: &[(&str, &str)]) -> Answer {
        let mut req = self.agent.get(&format!("{}{path}", self.base));
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        Self::answer(req.call().unwrap())
    }

    fn post_form(&self, path: &str, headers: &[(&str, &str)], form: &[(&str, &str)]) -> Answer {
        let mut req = self.agent.post(&format!("{}{path}", self.base));
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        Self::answer(req.send_form(form.iter().copied()).unwrap())
    }

    fn post_raw(&self, path: &str, content_type: &str, body: &str) -> Answer {
        Self::answer(self.agent.post(&format!("{}{path}", self.base)).header("content-type", content_type).send(body).unwrap())
    }

    /// Register a client with these redirect addresses; its id.
    fn register(&self, redirects: &[&str]) -> String {
        let body = serde_json::json!({ "client_name": "Test app", "redirect_uris": redirects }).to_string();
        let a = self.post_raw("/oauth/register", "application/json", &body);
        assert_eq!(a.status, 201, "{}", a.body);
        a.json()["client_id"].as_str().unwrap().to_string()
    }

    /// A registered client and a request that passes every check.
    fn flow(&self) -> Flow {
        let client_id = self.register(&[CB]);
        let pairs = [
            ("response_type", "code".to_string()),
            ("client_id", client_id.clone()),
            ("redirect_uri", CB.to_string()),
            ("code_challenge", s256(VERIFIER)),
            ("code_challenge_method", "S256".to_string()),
            ("state", "st-123".to_string()),
            ("resource", format!("{}/mcp", self.base)),
        ];
        Flow { client_id, pairs: pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect() }
    }

    /// GET /oauth/authorize for `flow` with this cookie (if any).
    fn consent(&self, cookie: Option<&str>, flow: &Flow) -> Answer {
        let path = format!("/oauth/authorize?{}", flow.query());
        match cookie {
            Some(c) => self.get(&path, &[("cookie", c)]),
            None => self.get(&path, &[]),
        }
    }

    /// POST the consent form for `flow`: `decision` and extra fields, from
    /// the page itself (`Sec-Fetch-Site: same-origin`).
    fn allow(&self, cookie: &str, flow: &Flow, decision: &str, extra: &[(&str, &str)]) -> Answer {
        let mut form: Vec<(&str, &str)> = flow.pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        form.push(("decision", decision));
        form.extend_from_slice(extra);
        self.post_form("/oauth/authorize", &[("cookie", cookie), ("sec-fetch-site", "same-origin")], &form)
    }

    /// The whole consent for a new client: the flow and its code.
    fn connect(&self, cookie: &str) -> (Flow, String) {
        let flow = self.flow();
        assert_eq!(self.consent(Some(cookie), &flow).status, 200);
        let a = self.allow(cookie, &flow, "allow", &[]);
        assert_eq!(a.status, 303, "{}", a.body);
        let code = a.location_param("code").expect("a code");
        (flow, code)
    }

    /// Trade a code at the token endpoint, the request's fields repeated.
    fn trade(&self, flow: &Flow, code: &str) -> Answer {
        let resource = format!("{}/mcp", self.base);
        self.post_form(
            "/oauth/token",
            &[],
            &[("grant_type", "authorization_code"), ("code", code), ("redirect_uri", CB), ("code_verifier", VERIFIER), ("client_id", &flow.client_id), ("resource", &resource)],
        )
    }

    fn refresh(&self, client_id: &str, token: &str) -> Answer {
        self.post_form("/oauth/token", &[], &[("grant_type", "refresh_token"), ("refresh_token", token), ("client_id", client_id)])
    }

    fn whoami(&self, token: &str) -> u16 {
        self.get("/api/v1/whoami", &[("authorization", &format!("Bearer {token}"))]).status
    }

    fn grants(&self) -> i64 {
        self.state.db.with(|c| c.query_row("SELECT COUNT(*) FROM api_grants", [], |r| r.get(0))).unwrap()
    }
}

const ALL_ROUTES: [(&str, &str); 8] = [
    ("GET", "/.well-known/oauth-authorization-server"),
    ("GET", "/.well-known/openid-configuration"),
    ("GET", "/.well-known/oauth-protected-resource"),
    ("GET", "/.well-known/oauth-protected-resource/mcp"),
    ("GET", "/oauth/authorize"),
    ("POST", "/oauth/token"),
    ("POST", "/oauth/register"),
    ("POST", "/oauth/revoke"),
];

fn statuses(s: &Site) -> Vec<u16> {
    ALL_ROUTES
        .iter()
        .map(|(m, p)| if *m == "GET" { s.get(p, &[]).status } else { s.post_raw(p, "application/x-www-form-urlencoded", "").status })
        .collect()
}

/// Every route answers 404 with the owner's switch off, and with no public
/// address set (an issuer must be a fixed address).
#[test]
fn every_route_is_404_off_or_without_a_public_address() {
    let s = Site::start(true, None);
    assert!(statuses(&s).iter().all(|&st| st != 404), "{:?}", statuses(&s));
    s.apps_on(false);
    assert_eq!(statuses(&s), vec![404; ALL_ROUTES.len()], "switch off");
    let s = Site::start(false, None);
    assert_eq!(statuses(&s), vec![404; ALL_ROUTES.len()], "no public address");
}

/// The discovery documents name endpoints built from the public address,
/// whatever `Host` and `X-Forwarded-*` say.
#[test]
fn discovery_addresses_come_from_the_setting_only() {
    let s = Site::start(true, None);
    let forged = [("host", "evil.example"), ("x-forwarded-host", "evil.example"), ("x-forwarded-proto", "https")];
    let meta = s.get("/.well-known/oauth-authorization-server", &forged);
    assert_eq!(meta.status, 200);
    assert_eq!(meta.header("cache-control"), "no-store");
    let m = meta.json();
    assert_eq!(m["issuer"], s.base.as_str());
    for (k, path) in [("authorization_endpoint", "/oauth/authorize"), ("token_endpoint", "/oauth/token"), ("registration_endpoint", "/oauth/register"), ("revocation_endpoint", "/oauth/revoke")] {
        assert_eq!(m[k], format!("{}{path}", s.base), "{k}");
    }
    assert_eq!(m["code_challenge_methods_supported"], serde_json::json!(["S256"]));
    assert_eq!(m["token_endpoint_auth_methods_supported"], serde_json::json!(["none"]));
    assert_eq!(m["scopes_supported"], serde_json::json!(["courses:read", "courses:write"]));
    assert_eq!(m["authorization_response_iss_parameter_supported"], true);
    assert!(!meta.body.contains("evil.example"));
    assert_eq!(s.get("/.well-known/openid-configuration", &forged).json(), m, "the same document");
    let prm = s.get("/.well-known/oauth-protected-resource/mcp", &forged).json();
    assert_eq!(prm["resource"], format!("{}/mcp", s.base));
    assert_eq!(prm["authorization_servers"], serde_json::json!([s.base]));
    assert_eq!(s.get("/.well-known/oauth-protected-resource", &forged).json()["resource"], s.base.as_str());
}

/// `/mcp`'s 401 names the protected-resource document, from the setting.
#[test]
fn mcp_names_the_resource_metadata() {
    let s = Site::start(true, None);
    let a = Site::answer(s.agent.post(&format!("{}/mcp", s.base)).header("content-type", "application/json").header("host", "evil.example").send("{}").unwrap());
    assert_eq!(a.status, 401);
    assert_eq!(a.header("www-authenticate"), format!("Bearer resource_metadata=\"{}/.well-known/oauth-protected-resource/mcp\"", s.base));
}

/// A request that fails a check gets an error page here, never a redirect
/// to the app, even when the redirect address itself is fine.
#[test]
fn a_failed_check_never_redirects() {
    let s = Site::start(true, None);
    let (_, me) = s.account("ann@example.org");
    let cases: Vec<(&str, Flow)> = vec![
        ("code_challenge", s.flow().without("code_challenge")),
        ("code_challenge_method", s.flow().with("code_challenge_method", "plain")),
        ("state", s.flow().without("state")),
        ("resource", s.flow().without("resource")),
        ("resource", s.flow().with("resource", "https://other.example/mcp")),
        ("redirect_uri", s.flow().with("redirect_uri", "https://evil.example/cb")),
        ("client", s.flow().with("client_id", "unknown")),
        ("response_type", s.flow().with("response_type", "token")),
        ("scope", s.flow().with("scope", "courses:delete")),
    ];
    for (what, flow) in cases {
        for cookie in [Some(me.as_str()), None] {
            let a = s.consent(cookie, &flow);
            assert_eq!(a.status, 400, "{what}");
            assert_eq!(a.header("location"), "", "{what}: no redirect");
            assert!(a.body.contains("data-testid=\"oauth-error\""), "{what}");
            let said = crate::t(&format!("oauth.error.{what}"));
            assert!(["&#39;", "&#x27;"].iter().any(|q| a.body.contains(&said.replace('\'', q))), "{what}: the page names the problem");
        }
        // The consent POST checks the request again, with the same result.
        let a = s.allow(&me, &flow, "allow", &[]);
        assert_eq!((a.status, a.header("location")), (400, ""), "{what} on the POST");
    }
    let mut dup = s.flow();
    dup.pairs.push(("state".into(), "again".into()));
    assert_eq!(s.consent(Some(&me), &dup).status, 400, "a field twice");
    assert_eq!(s.grants(), 0);
}

/// Signed in: the consent page names the app (escaped), the host the answer
/// goes to from the registered address, the scopes, the course picker and
/// its one line, the expiry, Allow and Deny; it may not be framed and is
/// not cached; GET grants nothing.
#[test]
fn the_consent_page_says_who_asks_and_grants_nothing_on_get() {
    let s = Site::start(true, None);
    let (ann, me) = s.account("ann@example.org");
    s.state.db.with(|c| crate::courses::create_course(c, ann, "ann-course-abc123", "Ann's course", &crate::courses::starter_files("ann-course-abc123", "Ann's course"))).unwrap();
    let flow = s.flow();
    let a = s.consent(Some(&me), &flow);
    assert_eq!(a.status, 200);
    for id in ["oauth-consent-app", "oauth-consent-host", "oauth-consent-scope", "oauth-consent-course", "oauth-consent-courses-note", "oauth-consent-lifetime", "oauth-consent-allow", "oauth-consent-deny"] {
        assert!(a.body.contains(&format!("data-testid=\"{id}\"")), "{id}");
    }
    assert!(a.body.contains(">app.example</strong>"), "the host from the registered address");
    assert!(a.body.contains(&Scope::Read.label()) && a.body.contains(&Scope::Write.label()));
    assert!(a.header("content-security-policy").contains("frame-ancestors 'none'"));
    assert_eq!(a.header("x-frame-options"), "DENY");
    assert_eq!(a.header("cache-control"), "no-store");
    assert_eq!(s.grants(), 0, "nothing on GET");
    let codes: i64 = s.state.db.with(|c| c.query_row("SELECT COUNT(*) FROM oauth_codes", [], |r| r.get(0))).unwrap();
    assert_eq!(codes, 0);
}

/// The consent POST is a real form from this site's page with the session
/// cookie: no cookie, a cross-site page, or no browser headers at all is
/// refused, and nothing is granted.
#[test]
fn the_consent_post_needs_the_session_and_this_sites_page() {
    let s = Site::start(true, None);
    let (_, me) = s.account("ann@example.org");
    let flow = s.flow();
    let mut form: Vec<(&str, &str)> = flow.pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    form.push(("decision", "allow"));
    let origin = s.base.clone();
    let no_cookie = s.post_form("/oauth/authorize", &[("sec-fetch-site", "same-origin")], &form);
    assert_eq!(no_cookie.status, 303);
    assert!(no_cookie.header("location").contains("/login"), "signed out: the sign-in page, not the app");
    assert_eq!(s.post_form("/oauth/authorize", &[("cookie", &me), ("sec-fetch-site", "cross-site")], &form).status, 403);
    assert_eq!(s.post_form("/oauth/authorize", &[("cookie", &me), ("sec-fetch-site", "same-site")], &form).status, 403);
    assert_eq!(s.post_form("/oauth/authorize", &[("cookie", &me), ("origin", "https://evil.example")], &form).status, 403);
    assert_eq!(s.post_form("/oauth/authorize", &[("cookie", &me)], &form).status, 403, "no browser headers: not the page's form");
    assert_eq!(s.grants(), 0);
    let ok = s.post_form("/oauth/authorize", &[("cookie", &me), ("origin", &origin)], &form);
    assert_eq!(ok.status, 303, "a browser without Sec-Fetch-Site, from this site: {}", ok.body);
    assert_eq!(s.grants(), 1);
}

/// Deny sends `access_denied` back with `state` and `iss`; Allow sends a
/// code with them, and makes one grant limited to the picked own course.
#[test]
fn deny_and_allow_answer_the_app() {
    let s = Site::start(true, None);
    let (ann, me) = s.account("ann@example.org");
    let (bob, _) = s.account("bob@example.org");
    s.state
        .db
        .with(|c| {
            crate::courses::create_course(c, ann, "ann-course-abc123", "Ann's", &crate::courses::starter_files("ann-course-abc123", "Ann's"))?;
            crate::courses::create_course(c, bob, "bob-course-abc123", "Bob's", &crate::courses::starter_files("bob-course-abc123", "Bob's"))
        })
        .unwrap();
    let flow = s.flow();
    let denied = s.allow(&me, &flow, "deny", &[]);
    assert_eq!(denied.status, 303);
    assert!(denied.header("location").starts_with(CB));
    assert_eq!(denied.location_param("error").as_deref(), Some("access_denied"));
    assert_eq!(denied.location_param("state").as_deref(), Some("st-123"));
    assert_eq!(denied.location_param("iss").as_deref(), Some(s.base.as_str()));
    assert_eq!(s.grants(), 0);

    let allowed = s.allow(&me, &flow, "allow", &[("course", "ann-course-abc123"), ("course", "bob-course-abc123"), ("lifetime", "30")]);
    assert_eq!(allowed.status, 303);
    assert_eq!(allowed.location_param("state").as_deref(), Some("st-123"), "state unchanged");
    assert_eq!(allowed.location_param("iss").as_deref(), Some(s.base.as_str()));
    assert_eq!(allowed.header("cache-control"), "no-store");
    let (courses, life): (String, i64) = s.state.db.with(|c| c.query_row("SELECT courses, expires_at - created_at FROM api_grants", [], |r| Ok((r.get(0)?, r.get(1)?)))).unwrap();
    assert_eq!(courses, "ann-course-abc123", "own courses only");
    assert_eq!(life, 30 * 86_400);
}

/// The whole flow: code to tokens (no-store), the token works, a refresh
/// rotates (another client can neither use nor replay it), the code
/// replayed revokes the grant and every token under it.
#[test]
fn code_refresh_and_replay() {
    let s = Site::start(true, None);
    let (_, me) = s.account("ann@example.org");
    let (flow, code) = s.connect(&me);
    let t = s.trade(&flow, &code);
    assert_eq!(t.status, 200, "{}", t.body);
    assert_eq!(t.header("cache-control"), "no-store");
    let j = t.json();
    assert_eq!(j["token_type"], "Bearer");
    assert_eq!(j["expires_in"], 3600);
    assert_eq!(j["scope"], "courses:read courses:write");
    let (access, refresh) = (j["access_token"].as_str().unwrap().to_string(), j["refresh_token"].as_str().unwrap().to_string());
    assert_eq!(s.whoami(&access), 200);
    assert_eq!(s.whoami(&refresh), 401, "a refresh token opens nothing");

    let r = s.refresh(&flow.client_id, &refresh);
    assert_eq!(r.status, 200);
    let new_refresh = r.json()["refresh_token"].as_str().unwrap().to_string();
    let other = s.flow();
    assert_eq!(s.refresh(&other.client_id, &new_refresh).json()["error"], "invalid_grant", "another client may not");
    assert_eq!(s.refresh(&other.client_id, &refresh).json()["error"], "invalid_grant", "nor replay a used one");
    assert_eq!(s.grants(), 1, "another client's replay revokes nothing");
    assert_eq!(s.refresh(&flow.client_id, &new_refresh).status, 200, "and did not use it up");

    let replay = s.trade(&flow, &code);
    assert_eq!(replay.json()["error"], "invalid_grant");
    assert_eq!(s.grants(), 0, "the replay revoked the grant");
    assert_eq!(s.whoami(&access), 401);
}

/// The old refresh token presented again (#184) is `invalid_grant` and
/// revokes the connection: the newest access and refresh tokens stop
/// working and the profile's Connected apps no longer lists the app.
#[test]
fn a_reused_refresh_token_disconnects_the_app() {
    let s = Site::start(true, None);
    let (_, me) = s.account("ann@example.org");
    let (flow, code) = s.connect(&me);
    let t = s.trade(&flow, &code).json();
    let old = t["refresh_token"].as_str().unwrap().to_string();
    let r = s.refresh(&flow.client_id, &old).json();
    let (access, newest) = (r["access_token"].as_str().unwrap().to_string(), r["refresh_token"].as_str().unwrap().to_string());
    assert_eq!(s.whoami(&access), 200);
    let profile = s.get("/profile", &[("cookie", &me)]);
    assert!(profile.body.contains("data-testid=\"profile-app\""), "listed before");

    let again = s.refresh(&flow.client_id, &old);
    assert_eq!((again.status, again.json()["error"].as_str()), (400, Some("invalid_grant")));
    assert_eq!(again.json().as_object().map(|o| o.len()), Some(1), "the error code only");
    assert_eq!(s.grants(), 0, "the reuse revoked the grant");
    assert_eq!(s.whoami(&access), 401, "the newest access token stops working");
    assert_eq!(s.refresh(&flow.client_id, &newest).json()["error"], "invalid_grant", "and the newest refresh token");
    let profile = s.get("/profile", &[("cookie", &me)]);
    assert_eq!(profile.status, 200);
    assert!(!profile.body.contains("data-testid=\"profile-app\""), "Connected apps no longer lists it");
    assert!(profile.body.contains("data-testid=\"profile-apps-none\""));
}

/// The token request repeats the redirect address and the resource, with
/// the right verifier; a wrong one revokes nothing.
#[test]
fn the_token_request_repeats_the_binding() {
    let s = Site::start(true, None);
    let (_, me) = s.account("ann@example.org");
    let (flow, code) = s.connect(&me);
    let resource = format!("{}/mcp", s.base);
    let send = |fields: &[(&str, &str)]| s.post_form("/oauth/token", &[], fields).json()["error"].as_str().unwrap_or("").to_string();
    let base = |skip: &str, over: (&str, &str)| -> Vec<(String, String)> {
        let all = [("grant_type", "authorization_code"), ("code", code.as_str()), ("redirect_uri", CB), ("code_verifier", VERIFIER), ("client_id", flow.client_id.as_str()), ("resource", resource.as_str())];
        all.iter().filter(|(k, _)| *k != skip).map(|(k, v)| (k.to_string(), if *k == over.0 { over.1.to_string() } else { v.to_string() })).collect()
    };
    let try_it = |skip: &str, over: (&str, &str)| {
        let v = base(skip, over);
        let refs: Vec<(&str, &str)> = v.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        send(&refs)
    };
    assert_eq!(try_it("resource", ("", "")), "invalid_request", "resource is required");
    assert_eq!(try_it("redirect_uri", ("", "")), "invalid_request");
    assert_eq!(try_it("code_verifier", ("", "")), "invalid_request", "no verifier, no token");
    assert_eq!(try_it("", ("resource", "https://other.example/mcp")), "invalid_target");
    assert_eq!(try_it("", ("resource", &s.base)), "invalid_target", "not the resource the code was for");
    assert_eq!(try_it("", ("redirect_uri", "https://app.example/other")), "invalid_grant");
    assert_eq!(try_it("", ("code_verifier", "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXX")), "invalid_grant");
    assert_eq!(try_it("", ("client_id", "unknown")), "invalid_client");
    assert_eq!(try_it("", ("grant_type", "password")), "unsupported_grant_type");
    assert_eq!(s.grants(), 1);
    assert_eq!(s.trade(&flow, &code).status, 200, "the right request still works");
    let dup = s.post_raw("/oauth/token", "application/x-www-form-urlencoded", "grant_type=refresh_token&grant_type=refresh_token&client_id=x&refresh_token=y");
    assert_eq!(dup.json()["error"], "invalid_request", "a field twice");
    let json = s.post_raw("/oauth/token", "application/json", "{}");
    assert_eq!(json.json()["error"], "invalid_request", "a form only");
}

/// Signed out, the request goes through the normal sign-in and back to
/// the authorize address (a local path) after the password. A cookie made
/// up to send the sign-in elsewhere still lands on the local path.
#[test]
fn sign_in_comes_back_to_the_request_and_nowhere_else() {
    let s = Site::start(true, None);
    s.account("ann@example.org");
    let flow = s.flow();
    let out = s.consent(None, &flow);
    assert_eq!(out.status, 303);
    assert_eq!(out.header("location"), "/login");
    let set = out.header("set-cookie");
    assert!(set.starts_with("oauth_next=") && set.contains("HttpOnly") && set.contains("SameSite=Lax"), "{set}");
    let cookie = set.split(';').next().unwrap().to_string();
    let signed = s.post_form("/login", &[("cookie", &cookie)], &[("email", "ann@example.org"), ("password", PASSWORD)]);
    assert_eq!(signed.status, 303);
    assert_eq!(signed.header("location"), format!("/oauth/authorize?{}", flow.query()));
    let sid = signed.headers.iter().filter(|(k, _)| k == "set-cookie").find_map(|(_, v)| v.strip_prefix("sid=").map(|r| format!("sid={}", r.split(';').next().unwrap()))).unwrap();
    assert_eq!(s.consent(Some(&sid), &flow).status, 200, "the consent page");

    // A forged cookie: //evil.example, a full address, a header break.
    for evil in ["//evil.example/x", "https://evil.example/", "a\r\nLocation: https://evil.example"] {
        let hex: String = evil.bytes().map(|b| format!("{b:02x}")).collect();
        let a = s.post_form("/login", &[("cookie", &format!("oauth_next={hex}"))], &[("email", "ann@example.org"), ("password", PASSWORD)]);
        let to = a.header("location");
        assert!(to == "/" || to.starts_with("/oauth/authorize?"), "{evil:?} led to {to}");
    }
}

/// An account without Create courses gets a page saying to ask the owner,
/// and its Allow grants nothing.
#[test]
fn an_account_without_create_courses_cannot_connect() {
    let s = Site::start(true, None);
    let (kid, me) = s.account("kid@example.org");
    s.state.db.with(|c| crate::perms::set_custom(c, kid, crate::perms::Perms::STUDENT)).unwrap();
    let flow = s.flow();
    let a = s.consent(Some(&me), &flow);
    assert_eq!(a.status, 403);
    assert!(a.body.contains("data-testid=\"oauth-no-permission\""));
    assert!(!a.body.contains("oauth-consent-allow"));
    assert_eq!(s.allow(&me, &flow, "allow", &[]).status, 403);
    assert_eq!(s.grants(), 0);
}

/// Behind a signing-in proxy the consent page shows for the header's
/// account, but the POST needs the session cookie: the header alone signs
/// nobody in there. The program endpoints never sign in by the header.
#[test]
fn the_proxy_header_alone_grants_nothing() {
    let s = Site::start(true, Some("127.0.0.1"));
    s.account("ann@example.org");
    let flow = s.flow();
    let page = s.consent_with(&flow, &[("remote-email", "ann@example.org")]);
    assert_eq!(page.status, 200, "the proxy vouched for the account");
    let sid = page.headers.iter().find_map(|(k, v)| (k == "set-cookie").then(|| v.split(';').next().unwrap().to_string())).expect("a session");
    let mut form: Vec<(&str, &str)> = flow.pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    form.push(("decision", "allow"));
    let header_only = s.post_form("/oauth/authorize", &[("remote-email", "ann@example.org"), ("sec-fetch-site", "same-origin")], &form);
    assert!(!header_only.header("location").starts_with(CB));
    assert_eq!(s.grants(), 0);
    let with_session = s.post_form("/oauth/authorize", &[("cookie", &sid), ("remote-email", "ann@example.org"), ("sec-fetch-site", "same-origin")], &form);
    assert!(with_session.header("location").starts_with(CB), "{}", with_session.body);
    let sessions = |s: &Site| s.state.db.with(|c| c.query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get::<_, i64>(0))).unwrap();
    let before = sessions(&s);
    for path in ["/oauth/token", "/oauth/register", "/oauth/revoke"] {
        let a = s.post_form(path, &[("remote-email", "ann@example.org")], &[]);
        assert_eq!(a.header("set-cookie"), "", "{path}");
    }
    assert_eq!(s.get("/.well-known/oauth-authorization-server", &[("remote-email", "ann@example.org")]).header("set-cookie"), "");
    assert_eq!(sessions(&s), before, "no session made by the program endpoints");
}

impl Site {
    fn consent_with(&self, flow: &Flow, headers: &[(&str, &str)]) -> Answer {
        self.get(&format!("/oauth/authorize?{}", flow.query()), headers)
    }
}

/// The program endpoints read no cookie: a token request with a signed-in
/// session and nothing else is `invalid_request`, and a cross-site POST is
/// not refused by the page's CSRF check (it carries no sign-in to abuse).
#[test]
fn the_program_endpoints_read_no_cookie() {
    let s = Site::start(true, None);
    let (_, me) = s.account("ann@example.org");
    let a = s.post_form("/oauth/token", &[("cookie", &me)], &[("grant_type", "refresh_token")]);
    assert_eq!(a.json()["error"], "invalid_request");
    assert_eq!(a.header("set-cookie"), "");
    let cross = s.post_form("/oauth/token", &[("sec-fetch-site", "cross-site"), ("cookie", &me)], &[("grant_type", "refresh_token")]);
    assert_eq!(cross.json()["error"], "invalid_request", "an OAuth answer, not the CSRF refusal");
}

/// Registration: https or loopback only, five addresses of 200 characters,
/// a name of 60, a body of 4 KB, 20 an hour from one address.
#[test]
fn registration_cannot_fill_the_box() {
    let s = Site::start(true, None);
    let reg = |body: Value| s.post_raw("/oauth/register", "application/json", &body.to_string());
    let evil = reg(serde_json::json!({ "client_name": "x", "redirect_uris": ["http://evil.example/cb"] }));
    assert_eq!((evil.status, evil.json()["error"].as_str()), (400, Some("invalid_redirect_uri")));
    assert_eq!(reg(serde_json::json!({ "client_name": "<script>", "redirect_uris": [CB] })).status, 400, "no markup");
    assert_eq!(reg(serde_json::json!({ "client_name": "x", "redirect_uris": [CB], "token_endpoint_auth_method": "client_secret_post" })).status, 400);
    let big = format!("{{\"client_name\":\"x\",\"redirect_uris\":[\"{CB}\"],\"pad\":\"{}\"}}", "a".repeat(5000));
    assert_eq!(s.post_raw("/oauth/register", "application/json", &big).status, 413);
    let ok = reg(serde_json::json!({ "client_name": "App", "redirect_uris": [CB], "logo_uri": "https://evil.example/logo.png" }));
    assert_eq!(ok.status, 201);
    assert_eq!(ok.json()["token_endpoint_auth_method"], "none");
    assert!(ok.json().get("logo_uri").is_none(), "nothing kept or fetched for it");
    // Five tried so far; 20 an hour.
    let mut last = 0;
    for _ in 0..15 {
        last = reg(serde_json::json!({ "client_name": "App", "redirect_uris": [CB] })).status;
    }
    assert_eq!(last, 201);
    assert_eq!(reg(serde_json::json!({ "client_name": "App", "redirect_uris": [CB] })).status, 429);
}

/// 30 refused token requests from one address in ten minutes, then 429.
#[test]
fn guessing_codes_is_rate_limited() {
    let s = Site::start(true, None);
    let flow = s.flow();
    let guess = || s.post_form("/oauth/token", &[], &[("grant_type", "refresh_token"), ("refresh_token", "skr_guess"), ("client_id", &flow.client_id)]).status;
    for _ in 0..30 {
        assert_eq!(guess(), 400);
    }
    assert_eq!(guess(), 429);
}

/// Revoke: the app disconnects itself; an unknown token is still 200.
#[test]
fn an_app_can_disconnect_itself() {
    let s = Site::start(true, None);
    let (_, me) = s.account("ann@example.org");
    let (flow, code) = s.connect(&me);
    let t = s.trade(&flow, &code).json();
    let access = t["access_token"].as_str().unwrap();
    assert_eq!(s.post_form("/oauth/revoke", &[], &[("token", "skr_unknown"), ("client_id", &flow.client_id)]).status, 200);
    assert_eq!(s.post_form("/oauth/revoke", &[], &[("token", access), ("client_id", "unknown")]).status, 401);
    assert_eq!(s.whoami(access), 200);
    let a = s.post_form("/oauth/revoke", &[], &[("token", t["refresh_token"].as_str().unwrap()), ("client_id", &flow.client_id)]);
    assert_eq!((a.status, a.header("cache-control")), (200, "no-store"));
    assert_eq!(s.whoami(access), 401);
    assert_eq!(s.grants(), 0);
}

// ---------- #170: the security review ----------

/// Every route issues #167 to #169 added, each method it answers, as one
/// list: with the switch off every one answers 404 (a signed-in session and
/// a good token too); with it on, none sends an `Access-Control-*` header,
/// a preflight from another site included.
const NEW_ROUTES: [(&str, &str); 14] = [
    ("GET", "/.well-known/oauth-authorization-server"),
    ("GET", "/.well-known/openid-configuration"),
    ("GET", "/.well-known/oauth-protected-resource"),
    ("GET", "/.well-known/oauth-protected-resource/mcp"),
    ("GET", "/oauth/authorize"),
    ("POST", "/oauth/authorize"),
    ("POST", "/oauth/token"),
    ("POST", "/oauth/register"),
    ("POST", "/oauth/revoke"),
    ("GET", "/api/v1/whoami"),
    ("POST", "/mcp"),
    ("GET", "/mcp"),
    ("POST", "/profile/apps/token"),
    ("POST", "/profile/apps/revoke"),
];

impl Site {
    /// `method path` with the session cookie, a good bearer token, this
    /// site's `Sec-Fetch-Site` and extra headers.
    fn any(&self, method: &str, path: &str, cookie: &str, token: &str, extra: &[(&str, &str)]) -> Answer {
        let url = format!("{}{path}", self.base);
        let auth = format!("Bearer {token}");
        let mut headers = vec![("cookie", cookie), ("authorization", auth.as_str()), ("sec-fetch-site", "same-origin")];
        headers.extend_from_slice(extra);
        let r = match method {
            "GET" => headers.iter().fold(self.agent.get(&url), |r, (k, v)| r.header(*k, *v)).call(),
            "OPTIONS" => headers.iter().fold(self.agent.options(&url), |r, (k, v)| r.header(*k, *v)).call(),
            _ => headers.iter().fold(self.agent.post(&url), |r, (k, v)| r.header(*k, *v)).header("content-type", "application/json").send("{}"),
        };
        Self::answer(r.unwrap())
    }

    /// A personal token for `uid`, both scopes.
    fn personal(&self, uid: i64) -> String {
        self.state
            .db
            .with(|c| {
                let g = crate::auth::api::NewGrant { client_id: "personal", client_name: "Mine", scopes: &Scope::ALL, courses: &[], lifetime: crate::auth::api::Lifetime::Days90 };
                Ok(crate::auth::api::make_personal(c, uid, &g, crate::db::now())?.unwrap())
            })
            .unwrap()
    }
}

#[test]
fn every_new_route_is_404_off_and_none_sends_cors() {
    let s = Site::start(true, None);
    let (ann, me) = s.account("ann@example.org");
    let token = s.personal(ann);
    let cors = |a: &Answer| a.headers.iter().any(|(k, _)| k.to_ascii_lowercase().starts_with("access-control-"));
    s.apps_on(false);
    for (m, p) in NEW_ROUTES {
        let a = s.any(m, p, &me, &token, &[]);
        assert_eq!(a.status, 404, "{m} {p} with the switch off: {}", a.body);
        assert!(!cors(&a), "{m} {p}");
    }
    s.apps_on(true);
    let flow = s.flow();
    for (m, p) in NEW_ROUTES {
        let p = if p == "/oauth/authorize" { format!("{p}?{}", flow.query()) } else { p.to_string() };
        for extra in [&[][..], &[("origin", "https://evil.example")][..]] {
            let a = s.any(m, &p, &me, &token, extra);
            assert!(!cors(&a), "{m} {p} {extra:?}: {:?}", a.headers);
            let pre = s.any("OPTIONS", &p, &me, &token, &[("origin", "https://evil.example"), ("access-control-request-method", "POST")]);
            assert!(!cors(&pre), "OPTIONS {p}: {:?}", pre.headers);
        }
    }
}

/// Each route that changes something takes one kind of sign-in, never both:
/// the profile's and the owner's forms and the consent POST take the
/// session with the CSRF check (a bearer token alone is the sign-in page,
/// a cross-site POST is refused); `/mcp` takes a bearer token only (a
/// cookie alone is 401); the program endpoints take neither (a session
/// cookie or a bearer token signs nothing in there).
#[test]
fn changing_routes_take_one_kind_of_sign_in() {
    let s = Site::start(true, None);
    let (ann, me) = s.account("ann@example.org");
    s.state.db.with(|c| c.execute("UPDATE users SET owner = 1 WHERE id = ?1", [ann])).unwrap();
    let token = s.personal(ann);
    let auth = format!("Bearer {token}");
    let flow = s.flow();
    let mut consent: Vec<(&str, &str)> = flow.pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    consent.push(("decision", "allow"));
    let session_routes: [(&str, Vec<(&str, &str)>); 5] = [
        ("/profile/apps/token", vec![("name", "X"), ("scope", "courses:read"), ("lifetime", "90"), ("password", PASSWORD)]),
        ("/profile/apps/revoke", vec![("id", "1")]),
        ("/manage/apps", vec![("on", "1")]),
        ("/manage/apps/revoke", vec![("id", "1")]),
        ("/oauth/authorize", consent),
    ];
    let before = (s.grants(), stored_tokens(&s));
    for (path, form) in &session_routes {
        let bearer_only = s.post_form(path, &[("authorization", &auth)], form);
        assert!(bearer_only.status == 303 && bearer_only.header("location").contains("/login"), "{path} with a bearer token alone: {} {}", bearer_only.status, bearer_only.header("location"));
        let cross = s.post_form(path, &[("cookie", &me), ("sec-fetch-site", "cross-site"), ("authorization", &auth)], form);
        assert_eq!(cross.status, 403, "{path} from another site");
    }
    assert_eq!((s.grants(), stored_tokens(&s)), before, "nothing was made or revoked");

    let mcp = Site::answer(s.agent.post(&format!("{}/mcp", s.base)).header("cookie", &me).header("content-type", "application/json").send("{}").unwrap());
    assert_eq!(mcp.status, 401, "a cookie alone on /mcp");

    for path in PROGRAM_PATHS_FOR_TESTS {
        for headers in [vec![("cookie", me.as_str())], vec![("authorization", auth.as_str())]] {
            let a = s.post_form(path, &headers, &[("grant_type", "refresh_token"), ("client_id", &flow.client_id), ("token", "x")]);
            assert_ne!(a.status, 200, "{path} {headers:?}: {}", a.body);
            assert_eq!(a.header("set-cookie"), "", "{path}");
        }
    }
    assert_eq!(s.whoami(&token), 200, "the bearer token was not revoked through a program endpoint");
}

const PROGRAM_PATHS_FOR_TESTS: [&str; 2] = ["/oauth/token", "/oauth/register"];

fn stored_tokens(s: &Site) -> i64 {
    s.state.db.with(|c| c.query_row("SELECT COUNT(*) FROM api_tokens", [], |r| r.get(0))).unwrap()
}

/// An app's name cannot hide a text-direction override or an invisible
/// character (it could before #170: `U+202E` "elgnorkS" shows as
/// "Skrongle"). A name that is plain "Skrongle" is allowed (an accepted
/// risk): the consent page still names the host the answer goes to, and
/// markup in a name never reaches the page as markup.
#[test]
fn app_names_cannot_mislead() {
    let s = Site::start(true, None);
    let (_, me) = s.account("ann@example.org");
    let reg = |name: &str| s.post_raw("/oauth/register", "application/json", &serde_json::json!({ "client_name": name, "redirect_uris": [CB] }).to_string());
    for name in ["\u{202E}elgnorkS", "Claude\u{200B}", "Claude\u{2066}x\u{2069}", "\u{FEFF}Claude", "Claude\u{E0041}"] {
        let a = reg(name);
        assert_eq!((a.status, a.json()["error"].as_str()), (400, Some("invalid_client_metadata")), "{name:?}");
    }
    let skr = reg("Skrongle");
    assert_eq!(skr.status, 201);
    let mut flow = s.flow();
    flow = flow.with("client_id", skr.json()["client_id"].as_str().unwrap());
    let page = s.consent(Some(&me), &flow);
    assert_eq!(page.status, 200);
    assert!(page.body.contains(">app.example</strong>"), "the host is named beside the name");
    let amp = reg("A & B's 'app'");
    assert_eq!(amp.status, 201);
    let page = s.consent(Some(&me), &flow.with("client_id", amp.json()["client_id"].as_str().unwrap()));
    assert!(page.body.contains(">A &#38; B&#39;s &#39;app&#39;</strong>"), "escaped as text");
}

/// The consent POST, abused: replayed after Allow it makes a second grant
/// (one per Allow, counted against the four; a never-traded one is purged
/// once its code expires); for a client deleted after the page was shown it
/// grants nothing; two Allows at once with one grant left make one grant.
#[test]
fn consent_replays_deleted_clients_and_double_clicks() {
    let s = Site::start(true, None);
    let (ann, me) = s.account("ann@example.org");
    let flow = s.flow();
    let first = s.allow(&me, &flow, "allow", &[]);
    let again = s.allow(&me, &flow, "allow", &[]);
    assert_eq!((first.status, again.status), (303, 303));
    assert_ne!(first.location_param("code"), again.location_param("code"));
    assert_eq!(s.grants(), 2, "one grant per Allow");
    assert_eq!(s.trade(&flow, &first.location_param("code").unwrap()).status, 200);
    let later = crate::db::now() + crate::oauth_server::CODE_TTL_SECS + 1;
    s.state.db.with(|c| crate::auth::api::purge_expired(c, later)).unwrap();
    assert_eq!(s.grants(), 1, "the untraded grant is purged with its code");

    let gone = s.flow();
    assert_eq!(s.consent(Some(&me), &gone).status, 200);
    s.state.db.with(|c| c.execute("DELETE FROM oauth_clients WHERE client_id = ?1", [&gone.client_id])).unwrap();
    let a = s.allow(&me, &gone, "allow", &[]);
    assert_eq!((a.status, a.header("location")), (400, ""), "a deleted client");
    assert_eq!(s.grants(), 1);

    // Two more grants: three of four. Two Allows at once: one is made.
    s.personal(ann);
    s.personal(ann);
    let race = s.flow();
    let statuses: Vec<u16> = std::thread::scope(|sc| {
        let hs: Vec<_> = (0..2).map(|_| sc.spawn(|| s.allow(&me, &race, "allow", &[]).status)).collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let mut sorted = statuses.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, vec![303, 409], "{statuses:?}");
    assert_eq!(s.grants(), 4);
}

/// A course list never turns into "all your courses": when every course
/// picked is not the account's own (deleted meanwhile, someone else's, made
/// up), Allow and Make a token refuse and make nothing. Before #170 the
/// list came out empty, and an empty list means every course plus creating
/// new ones.
#[test]
fn a_course_list_never_widens_to_all_courses() {
    let s = Site::start(true, None);
    let (ann, me) = s.account("ann@example.org");
    let (bob, _) = s.account("bob@example.org");
    s.state
        .db
        .with(|c| {
            crate::courses::create_course(c, ann, "ann-course-abc123", "Ann's", &crate::courses::starter_files("ann-course-abc123", "Ann's"))?;
            crate::courses::create_course(c, bob, "bob-course-abc123", "Bob's", &crate::courses::starter_files("bob-course-abc123", "Bob's"))
        })
        .unwrap();
    let flow = s.flow();
    for picked in [&[("course", "bob-course-abc123")][..], &[("course", "gone-course-000000")][..], &[("course", "spanish")][..]] {
        let a = s.allow(&me, &flow, "allow", picked);
        assert_eq!((a.status, a.header("location")), (409, ""), "{picked:?}");
        assert!(a.body.contains(&crate::t("apps.courses_gone").replace('\'', "&#39;")), "{picked:?}: says why");
        let mut form = vec![("name", "Limited"), ("scope", "courses:read"), ("scope", "courses:write"), ("lifetime", "90"), ("password", PASSWORD)];
        form.extend_from_slice(picked);
        let made = s.post_form("/profile/apps/token", &[("cookie", &me)], &form);
        assert!(!made.body.contains("skr_"), "{picked:?}: no token");
    }
    assert_eq!(s.grants(), 0);
    let mixed = s.allow(&me, &flow, "allow", &[("course", "ann-course-abc123"), ("course", "gone-course-000000")]);
    assert_eq!(mixed.status, 303, "one of them is hers: the list keeps that one");
    let courses: String = s.state.db.with(|c| c.query_row("SELECT courses FROM api_grants", [], |r| r.get(0))).unwrap();
    assert_eq!(courses, "ann-course-abc123");
}

/// Token kinds never stand in for each other: a personal token at
/// `/oauth/revoke` revokes nothing; a refresh token is no bearer token at
/// `/mcp`; an access token or a personal token is no refresh token; a
/// refresh token another client presents at `/oauth/revoke` stays.
#[test]
fn token_kinds_never_cross() {
    let s = Site::start(true, None);
    let (ann, me) = s.account("ann@example.org");
    let personal = s.personal(ann);
    let (flow, code) = s.connect(&me);
    let t = s.trade(&flow, &code).json();
    let (access, refresh) = (t["access_token"].as_str().unwrap().to_string(), t["refresh_token"].as_str().unwrap().to_string());

    assert_eq!(s.post_form("/oauth/revoke", &[], &[("token", &personal), ("client_id", &flow.client_id)]).status, 200);
    assert_eq!(s.whoami(&personal), 200, "a personal token is not revoked through OAuth");
    let mcp = |tok: &str| {
        Site::answer(s.agent.post(&format!("{}/mcp", s.base)).header("authorization", &format!("Bearer {tok}")).header("content-type", "application/json").send(r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#).unwrap()).status
    };
    assert_eq!(mcp(&refresh), 401, "a refresh token at /mcp");
    assert_eq!(mcp(&access), 200);
    for not_refresh in [&access, &personal] {
        assert_eq!(s.refresh(&flow.client_id, not_refresh).json()["error"], "invalid_grant");
    }
    let other = s.flow();
    assert_eq!(s.post_form("/oauth/revoke", &[], &[("token", &refresh), ("client_id", &other.client_id)]).status, 200);
    assert_eq!(s.refresh(&flow.client_id, &refresh).status, 200, "another client's revoke left it alone");
    assert_eq!(s.whoami(&personal), 200);
}

/// The account is asked again at the token request: demoted to Student
/// after Allow and before the trade, the code is `invalid_grant` (and works
/// once the permission is back, within its ten minutes); demoted later, a
/// refresh is `invalid_grant`; a temporary password between Allow and the
/// trade revokes the grant, so the code never works.
#[test]
fn an_account_changed_mid_flow_cannot_trade() {
    let s = Site::start(true, None);
    let (ann, me) = s.account("ann@example.org");
    let student = |on: bool| {
        let p = if on { crate::perms::Perms::STUDENT } else { crate::perms::Perms::TEACHER };
        s.state.db.with(|c| crate::perms::set_custom(c, ann, p)).unwrap();
    };
    let (flow, code) = s.connect(&me);
    student(true);
    assert_eq!(s.trade(&flow, &code).json()["error"], "invalid_grant");
    student(false);
    let t = s.trade(&flow, &code);
    assert_eq!(t.status, 200, "{}", t.body);
    let refresh = t.json()["refresh_token"].as_str().unwrap().to_string();
    student(true);
    assert_eq!(s.refresh(&flow.client_id, &refresh).json()["error"], "invalid_grant");
    assert_eq!(s.whoami(t.json()["access_token"].as_str().unwrap()), 401);
    student(false);

    let (flow, code) = s.connect(&me);
    s.state.db.with(|c| crate::owner::set_temporary_password(c, ann, "x", None)).unwrap();
    s.state.db.with(|c| c.execute("UPDATE users SET must_change_password = 0", [])).unwrap();
    assert_eq!(s.trade(&flow, &code).json()["error"], "invalid_grant", "the temporary password revoked the grant");
}

/// The OAuth half of #170's checklist, as one test: a reused code, a code
/// from another client, a verifier that does not match, a redirect address
/// one character off, a `resource` for another site, a forged `Host`. A
/// code another client presents is not used up by it.
#[test]
fn oauth_refuses_the_checklist() {
    let s = Site::start(true, None);
    let (_, me) = s.account("ann@example.org");
    let (flow, code) = s.connect(&me);
    let resource = format!("{}/mcp", s.base);
    let other = s.flow();
    let send = |client: &str, redirect: &str, verifier: &str, res: &str| {
        s.post_form("/oauth/token", &[], &[("grant_type", "authorization_code"), ("code", &code), ("redirect_uri", redirect), ("code_verifier", verifier), ("client_id", client), ("resource", res)])
            .json()["error"]
            .as_str()
            .unwrap_or("")
            .to_string()
    };
    assert_eq!(send(&other.client_id, CB, VERIFIER, &resource), "invalid_grant", "another client");
    assert_eq!(send(&flow.client_id, CB, &VERIFIER.replace('d', "e"), &resource), "invalid_grant", "a verifier that does not match");
    assert_eq!(send(&flow.client_id, "https://app.example/cc", VERIFIER, &resource), "invalid_grant", "one character off");
    assert_eq!(send(&flow.client_id, "https://app.example/cb/", VERIFIER, &resource), "invalid_grant", "one character more");
    assert_eq!(send(&flow.client_id, CB, VERIFIER, "https://other.example/mcp"), "invalid_target", "another site");
    assert_eq!(s.trade(&flow, &code).status, 200, "none of those used the code up");
    assert_eq!(s.trade(&flow, &code).json()["error"], "invalid_grant", "a reused code");
    assert_eq!(s.grants(), 0, "and the reuse revoked the grant");
    let meta = s.get("/.well-known/oauth-authorization-server", &[("host", "evil.example"), ("x-forwarded-host", "evil.example")]);
    assert_eq!(meta.json()["issuer"], s.base.as_str(), "a forged Host");
}

/// The owner's Connected apps card counts the apps registered and the
/// tokens handed out in the last day: a registration, a code traded and a
/// refresh each show. A Teacher's Manage Users page shows no card.
#[test]
fn the_owner_sees_registrations_and_tokens_of_the_last_day() {
    let s = Site::start(true, None);
    let (owner, me) = s.account("owner@example.org");
    s.state.db.with(|c| c.execute("UPDATE users SET owner = 1 WHERE id = ?1", [owner])).unwrap();
    let counts = |cookie: &str| {
        let page = s.get("/manage", &[("cookie", cookie)]);
        let attr = |name: &str| page.body.split(&format!("{name}=\"")).nth(1).and_then(|r| r.split('"').next()).map(str::to_string);
        (attr("data-registered"), attr("data-issued"), page.body.contains(&crate::tn("apps.manage_issued", 1)))
    };
    assert_eq!(counts(&me), (Some("0".into()), Some("0".into()), false));
    let (flow, code) = s.connect(&me);
    let t = s.trade(&flow, &code).json();
    assert_eq!(counts(&me), (Some("1".into()), Some("1".into()), true), "one app, one token");
    s.refresh(&flow.client_id, t["refresh_token"].as_str().unwrap());
    s.flow();
    assert_eq!(counts(&me).0.as_deref(), Some("2"));
    assert_eq!(counts(&me).1.as_deref(), Some("2"), "a refresh counts");
    let (_, teacher) = s.account("teacher@example.org");
    let page = s.get("/manage", &[("cookie", &teacher)]);
    assert!(!page.body.contains("data-testid=\"manage-apps-counts\""), "owners only");
}

/// No token, code, verifier or hash in any log line, at every level down
/// to TRACE, over a whole flow: registration, consent, Allow, the trade,
/// whoami, an MCP call, a refresh, a replayed code and a revocation.
/// Run in a process of its own with a global subscriber, so the server's
/// worker threads log into it too.
#[test]
fn no_secret_in_the_trace_log_of_a_full_flow() {
    const CHILD: &str = "SKRONGLE_OAUTH_LOG_CHILD";
    if std::env::var_os(CHILD).is_some() {
        return trace_log_of_a_full_flow();
    }
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["routes::oauth::tests::no_secret_in_the_trace_log_of_a_full_flow", "--exact", "--test-threads=1", "--nocapture"])
        .env(CHILD, "1")
        .output()
        .unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && text.contains("1 passed"), "{text}");
}

fn trace_log_of_a_full_flow() {
    #[derive(Clone, Default)]
    struct Buf(Arc<std::sync::Mutex<Vec<u8>>>);
    impl std::io::Write for Buf {
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
    tracing::subscriber::set_global_default(subscriber).unwrap();
    tracing::callsite::rebuild_interest_cache();

    let s = Site::start(true, None);
    let (ann, me) = s.account("ann@example.org");
    let personal = s.personal(ann);
    let (flow, code) = s.connect(&me);
    let t = s.trade(&flow, &code).json();
    let (access, refresh) = (t["access_token"].as_str().unwrap().to_string(), t["refresh_token"].as_str().unwrap().to_string());
    assert_eq!(s.whoami(&access), 200);
    let mcp = Site::answer(s.agent.post(&format!("{}/mcp", s.base)).header("authorization", &format!("Bearer {access}")).header("content-type", "application/json").send(r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list_courses"}}"#).unwrap());
    assert_eq!(mcp.status, 200);
    let r = s.refresh(&flow.client_id, &refresh).json();
    let refresh2 = r["refresh_token"].as_str().unwrap().to_string();
    s.whoami("skr_0000000000000000000000000000000000000000000000000000000000000000");
    s.trade(&flow, &code);
    s.post_form("/oauth/revoke", &[], &[("token", &refresh2), ("client_id", &flow.client_id)]);
    // A second connection, its refresh token used twice (#184).
    let (flow2, code2) = s.connect(&me);
    let refresh3 = s.trade(&flow2, &code2).json()["refresh_token"].as_str().unwrap().to_string();
    let refresh4 = s.refresh(&flow2.client_id, &refresh3).json()["refresh_token"].as_str().unwrap().to_string();
    s.refresh(&flow2.client_id, &refresh3);

    let log = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
    assert!(log.contains("connected an OAuth app") && log.contains("code was used twice"), "the log was captured: {log}");
    assert!(log.contains("refresh token was used twice") && log.contains(&format!("of account {ann}")), "the reuse names the grant and account: {log}");
    let sid = me.trim_start_matches("sid=").to_string();
    let challenge = s256(VERIFIER);
    for secret in [&personal, &access, &refresh, &refresh2, &refresh3, &refresh4, &code, &code2, &sid] {
        let bare = secret.trim_start_matches("skr_");
        assert!(!log.contains(bare), "a token or code is in the log");
        assert!(!log.contains(&crate::auth::hash_token(secret)), "a hash is in the log");
    }
    assert!(!log.contains(VERIFIER) && !log.contains(&challenge), "the PKCE verifier or challenge is in the log");
}

/// Discovery and the whole flow under `SKRONGLE_BASE_PATH=/skrongle`: every
/// document and address carries the base path, nothing answers at the
/// root, the consent page posts to the base path, the program endpoints
/// are spared the CSRF check and the proxy's sign-in there too, and the
/// token works at `/skrongle/mcp`. The base path is set once per process,
/// so this runs in a process of its own.
#[test]
fn discovery_and_the_whole_flow_under_a_base_path() {
    const CHILD: &str = "SKRONGLE_OAUTH_BASE_PATH_CHILD";
    if std::env::var_os(CHILD).is_some() {
        return whole_flow_under_a_base_path();
    }
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["routes::oauth::tests::discovery_and_the_whole_flow_under_a_base_path", "--exact", "--test-threads=1", "--nocapture"])
        .env(CHILD, "1")
        .output()
        .unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && text.contains("1 passed"), "{text}");
}

fn whole_flow_under_a_base_path() {
    const BASE: &str = "/skrongle";
    crate::BASE_PATH.set(BASE.to_string()).unwrap();
    let s = Site::start(true, None);
    let issuer = format!("{}{BASE}", s.base);
    let (_, me) = s.account("ann@example.org");

    // Discovery: under the base path only, every address from the issuer.
    for root in ["/.well-known/oauth-authorization-server", "/.well-known/oauth-protected-resource/mcp", "/oauth/authorize", "/mcp"] {
        assert_eq!(s.get(root, &[]).status, 404, "{root} at the root");
    }
    let meta = s.get(&format!("{BASE}/.well-known/oauth-authorization-server"), &[("host", "evil.example")]);
    assert_eq!(meta.status, 200);
    let m = meta.json();
    assert_eq!(m["issuer"], issuer.as_str());
    for (k, path) in [("authorization_endpoint", "/oauth/authorize"), ("token_endpoint", "/oauth/token"), ("registration_endpoint", "/oauth/register"), ("revocation_endpoint", "/oauth/revoke")] {
        assert_eq!(m[k], format!("{issuer}{path}"), "{k}");
    }
    assert_eq!(s.get(&format!("{BASE}/.well-known/openid-configuration"), &[]).json(), m);
    let prm = s.get(&format!("{BASE}/.well-known/oauth-protected-resource/mcp"), &[]).json();
    assert_eq!((prm["resource"].as_str(), prm["authorization_servers"][0].as_str()), (Some(format!("{issuer}/mcp").as_str()), Some(issuer.as_str())));
    assert_eq!(s.get(&format!("{BASE}/.well-known/oauth-protected-resource"), &[]).json()["resource"], issuer.as_str());
    let unauthorized = Site::answer(s.agent.post(&format!("{issuer}/mcp")).header("content-type", "application/json").send("{}").unwrap());
    assert_eq!(unauthorized.status, 401);
    assert_eq!(unauthorized.header("www-authenticate"), format!("Bearer resource_metadata=\"{issuer}/.well-known/oauth-protected-resource/mcp\""));

    // Registration, cross-site and with the proxy's header: an OAuth answer,
    // no session.
    let reg = s.post_raw(&format!("{BASE}/oauth/register"), "application/json", &serde_json::json!({ "client_name": "App", "redirect_uris": [CB] }).to_string());
    assert_eq!(reg.status, 201, "{}", reg.body);
    let client_id = reg.json()["client_id"].as_str().unwrap().to_string();
    // Behind a signing-in proxy (a second server, same base path).
    let proxied = Site::start(true, Some("127.0.0.1"));
    proxied.account("ann@example.org");
    for path in ["/oauth/token", "/oauth/register", "/oauth/revoke"] {
        let cross = proxied.post_form(&format!("{BASE}{path}"), &[("sec-fetch-site", "cross-site"), ("remote-email", "ann@example.org")], &[("grant_type", "refresh_token")]);
        assert!(cross.json()["error"].is_string(), "{path}: the CSRF check spares the program endpoints under the base path: {}", cross.body);
        assert_eq!(cross.header("set-cookie"), "", "{path}: and the proxy signs nobody in there");
    }
    let mcp = Site::answer(proxied.agent.post(&format!("{}{BASE}/mcp", proxied.base)).header("remote-email", "ann@example.org").header("content-type", "application/json").send("{}").unwrap());
    assert_eq!((mcp.status, mcp.header("set-cookie")), (401, ""), "the proxy's header is no token at /skrongle/mcp");

    // The flow: signed out to the sign-in page under the base path and back.
    let resource = format!("{issuer}/mcp");
    let challenge = s256(VERIFIER);
    let pairs = [
        ("response_type", "code"),
        ("client_id", client_id.as_str()),
        ("redirect_uri", CB),
        ("code_challenge", challenge.as_str()),
        ("code_challenge_method", "S256"),
        ("state", "st-1"),
        ("resource", resource.as_str()),
    ];
    let query = pairs.iter().map(|(k, v)| format!("{k}={}", crate::urlenc::urlencode(v))).collect::<Vec<_>>().join("&");
    let out = s.get(&format!("{BASE}/oauth/authorize?{query}"), &[]);
    assert_eq!((out.status, out.header("location")), (303, "/skrongle/login"));
    assert!(out.header("set-cookie").contains("Path=/skrongle"), "{}", out.header("set-cookie"));
    let cookie = out.header("set-cookie").split(';').next().unwrap().to_string();
    let signed = s.post_form(&format!("{BASE}/login"), &[("cookie", &cookie)], &[("email", "ann@example.org"), ("password", PASSWORD)]);
    assert_eq!(signed.header("location"), format!("{BASE}/oauth/authorize?{query}"), "{} {}", signed.status, &signed.body[..signed.body.len().min(400)]);
    let page = s.get(&format!("{BASE}/oauth/authorize?{query}"), &[("cookie", &me)]);
    assert_eq!(page.status, 200);
    assert!(page.body.contains(&format!("action=\"{BASE}/oauth/authorize\"")), "the form posts under the base path");
    let mut form: Vec<(&str, &str)> = pairs.to_vec();
    form.push(("decision", "allow"));
    let origin = s.base.clone();
    let allowed = s.post_form(&format!("{BASE}/oauth/authorize"), &[("cookie", &me), ("origin", &origin)], &form);
    assert_eq!(allowed.status, 303, "{}", allowed.body);
    assert_eq!(allowed.location_param("iss").as_deref(), Some(issuer.as_str()));
    let code = allowed.location_param("code").unwrap();
    let t = s.post_form(
        &format!("{BASE}/oauth/token"),
        &[],
        &[("grant_type", "authorization_code"), ("code", &code), ("redirect_uri", CB), ("code_verifier", VERIFIER), ("client_id", &client_id), ("resource", &resource)],
    );
    assert_eq!(t.status, 200, "{}", t.body);
    let access = t.json()["access_token"].as_str().unwrap().to_string();
    let list = Site::answer(
        s.agent
            .post(&format!("{issuer}/mcp"))
            .header("authorization", &format!("Bearer {access}"))
            .header("content-type", "application/json")
            .send(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#)
            .unwrap(),
    );
    assert_eq!(list.status, 200);
    assert!(list.body.contains("put_course_files"));
    let wrong_resource = s.flow_token_with_resource(&client_id, &format!("{}/mcp", s.base));
    assert_eq!(wrong_resource, "invalid_target", "the resource without the base path is another one");
}

impl Site {
    /// A token request whose `resource` is `resource`: its error code.
    fn flow_token_with_resource(&self, client_id: &str, resource: &str) -> String {
        let a = self.post_form(
            &format!("{}/oauth/token", crate::base_path()),
            &[],
            &[("grant_type", "authorization_code"), ("code", "x"), ("redirect_uri", CB), ("code_verifier", VERIFIER), ("client_id", client_id), ("resource", resource)],
        );
        a.json()["error"].as_str().unwrap_or("").to_string()
    }
}
