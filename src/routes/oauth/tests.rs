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
        let id = self.state.db.with(|c| crate::auth::create_user(c, email, &hash)).unwrap();
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
/// rotates, the old refresh token is `invalid_grant`, the code replayed
/// revokes the grant and every token under it.
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
    let old = s.refresh(&flow.client_id, &refresh);
    assert_eq!((old.status, old.json()["error"].as_str()), (400, Some("invalid_grant")));
    assert_eq!(old.json().as_object().map(|o| o.len()), Some(1), "the error code only");
    let other = s.flow();
    assert_eq!(s.refresh(&other.client_id, &new_refresh).json()["error"], "invalid_grant", "another client may not");
    assert_eq!(s.refresh(&flow.client_id, &new_refresh).status, 200, "and did not use it up");

    let replay = s.trade(&flow, &code);
    assert_eq!(replay.json()["error"], "invalid_grant");
    assert_eq!(s.grants(), 0, "the replay revoked the grant");
    assert_eq!(s.whoami(&access), 401);
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
