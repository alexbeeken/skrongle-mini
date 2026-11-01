use super::*;

/// The session of the dev user every `state` has (a developer; the dev
/// mailbox itself needs no sign-in).
const DEV_TOKEN: &str = "d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0";

fn state(dev: bool) -> Arc<AppState> {
    let s = crate::routes::test_state(dev, None);
    s.db.with(|c| {
        let id = crate::auth::create_user(c, dev::DEV_EMAIL, "x")?;
        crate::terms::accept(c, id, crate::terms::version())?;
        dev::set_developer(c, id)?;
        c.execute(
            "INSERT INTO sessions (user_id, session_token, created_at, expires_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![id, crate::auth::hash_token(DEV_TOKEN), crate::db::now(), crate::db::now() + 3600],
        )?;
        Ok(())
    })
    .unwrap();
    s
}

/// An HTTP client signed in as that developer.
fn dev_agent(redirects: bool) -> ureq::Agent {
    let cookie = format!("{}={DEV_TOKEN}", crate::auth::SESSION_COOKIE);
    let mut b = ureq::Agent::config_builder().http_status_as_error(false);
    if !redirects {
        b = b.max_redirects(0);
    }
    b.middleware(move |mut req: axum::http::Request<ureq::SendBody>, next: ureq::middleware::MiddlewareNext| {
        // A request that sends cookies of its own (a sign-in flow) keeps them alone.
        if !req.headers().contains_key("cookie") {
            req.headers_mut().insert("cookie", cookie.parse().unwrap());
        }
        next.handle(req)
    })
    .build()
    .into()
}

/// The whole "Continue with Google" flow against the stand-in, over the
/// real router: sign up (confirm step), sign in again, link to an
/// account, and each failure the stand-in can produce, shown as the
/// login page's error.
#[test]
fn mock_google_runs_the_real_flow_end_to_end() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let (addr, state) = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mut state = state(true);
        Arc::get_mut(&mut state).unwrap().google = Some(dev::mock_google(addr));
        let app = crate::routes::router(state.clone());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (addr, state)
    });
    let base = format!("http://{addr}");
    let agent = dev_agent(false);
    // (status, Location, cookies set as "name=value", body)
    let get = |path: &str, cookies: &str| {
        let mut r = agent.get(&format!("{base}{path}")).header("cookie", cookies).call().unwrap();
        let loc = r.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
        let set: Vec<String> = r
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|v| v.to_str().ok()?.split(';').next().map(str::to_string))
            .collect();
        (r.status().as_u16(), loc, set, r.body_mut().read_to_string().unwrap())
    };
    let post = |path: &str, cookies: &str, form: &[(&str, &str)]| {
        let mut r = agent.post(&format!("{base}{path}")).header("cookie", cookies).send_form(form.iter().copied()).unwrap();
        let loc = r.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
        let set: Vec<String> = r
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|v| v.to_str().ok()?.split(';').next().map(str::to_string))
            .collect();
        (r.status().as_u16(), loc, set, r.body_mut().read_to_string().unwrap())
    };
    let value = |set: &[String], name: &str| {
        set.iter().find_map(|c| c.strip_prefix(&format!("{name}=")).map(str::to_string)).unwrap_or_else(|| panic!("no {name} cookie in {set:?}"))
    };
    // /auth/google -> the stand-in -> the callback; returns the callback's answer.
    let through = |query: &str, cookies: &str| {
        let (status, to_google, set, _) = get(&format!("/auth/google{query}"), cookies);
        assert_eq!(status, 303, "start");
        assert!(to_google.starts_with("/dev/google/auth?client_id="), "{to_google}");
        let oauth = value(&set, "oauth");
        let (status, back, _, body) = get(&to_google, "");
        assert_eq!(status, 303, "stand-in: {body}");
        assert!(back.starts_with("/auth/google/callback?"), "{back}");
        let jar = format!("oauth={oauth}; {cookies}");
        get(&back, &jar)
    };

    // Sign up: the identity waits at the confirm step, then the account exists.
    let (status, loc, set, _) = through("?mock_email=Ana.Lopez@example.org", "");
    assert_eq!((status, loc.as_str()), (303, "/auth/google/confirm"));
    // Two cookies in one response, both sent (#1028): the spent state is cleared, the sign-up kept.
    assert_eq!(value(&set, "oauth"), "", "{set:?}");
    let signup = format!("signup={}", value(&set, "signup"));
    let (status, _, _, body) = get("/auth/google/confirm", &signup);
    assert_eq!(status, 200);
    assert!(body.contains("ana.lopez@example.org"), "{body}");
    // Without the Terms box the form comes back, the error inside it, the
    // age and AI boxes kept, and only the missing box marked.
    let (status, _, set, body) = post("/auth/google/confirm", &signup, &[("age", "on"), ("no_ai", "on")]);
    assert_eq!(status, 200);
    assert!(body.contains(r#"data-testid="google-confirm-error">Accept the Terms to continue."#), "{body}");
    assert!(body.contains(r#"data-testid="google-confirm-terms" required aria-invalid="true""#), "{body}");
    assert!(body.contains(r#"data-testid="google-confirm-age" required checked>"#), "{body}");
    assert!(body.contains(r#"data-testid="google-confirm-no-ai" checked"#), "{body}");
    assert!(!set.iter().any(|c| c.starts_with("sid=")), "{set:?}");
    let (status, loc, set, _) = post("/auth/google/confirm", &signup, &[("age", "on"), ("accept", "on"), ("no_ai", "on")]);
    assert_eq!((status, loc.as_str()), (303, "/"));
    for spent in ["signup", "signup_invite", "share_next"] {
        assert_eq!(value(&set, spent), "", "{spent} cleared: {set:?}");
    }
    let sid = format!("sid={}", value(&set, "sid"));
    let (ana_sub, _) = dev::mock_google_account("ana.lopez@example.org");
    let ana = state.db.with(|c| crate::auth::find_user_by_google(c, &ana_sub)).unwrap().expect("account created");
    let age_confirmed = state.db.with(|c| c.query_row("SELECT age_confirmed FROM users WHERE id = ?1", [ana], |r| r.get::<_, bool>(0))).unwrap();
    assert!(age_confirmed, "the confirm step records the age confirmation");
    assert!(get("/profile", &sid).3.contains("Google account connected"));
    // The "Hide all AI features" box counts, as on /register.
    assert!(state.db.with(|c| crate::profile::load(c, ana)).unwrap().unwrap().ai_off);

    // Sign in again: the same address is the same Google account.
    let (status, loc, set, _) = through("?mock_email=ANA.lopez@example.org", "");
    assert_eq!((status, loc.as_str()), (303, "/"));
    assert!(set.iter().any(|c| c.starts_with("sid=")), "{set:?}");

    // Link: a signed-in password account connects the default stand-in
    // account after its password (#360); a signed-in /auth/google only
    // goes to the profile.
    let hash = crate::auth::hash_password("their-password").unwrap();
    let password_account = |email: &str| {
        let id = state.db.with(|c| crate::auth::create_user(c, email, &hash)).unwrap();
        state.db.with(|c| crate::terms::accept(c, id, crate::terms::version())).unwrap();
        (id, format!("sid={}", state.db.with(|c| crate::auth::create_session(c, id)).unwrap()))
    };
    let (bob, bob_sid) = password_account("bob@example.org");
    let (status, loc, _, _) = get("/auth/google", &bob_sid);
    assert_eq!((status, loc.as_str()), (303, "/profile#account"));
    // The profile's link form -> the stand-in (as `hint`) -> the callback.
    let link = |sid: &str, hint: &str| {
        let (status, to_google, set, _) = post("/profile/google/link", sid, &[("password", "their-password")]);
        assert_eq!(status, 303, "link start");
        let oauth = value(&set, "oauth");
        let (status, back, _, body) = get(&format!("{to_google}{hint}"), "");
        assert_eq!(status, 303, "stand-in: {body}");
        get(&back, &format!("oauth={oauth}; {sid}"))
    };
    let (status, loc, _, _) = link(&bob_sid, "");
    assert_eq!((status, loc.as_str()), (303, "/profile?saved=google-linked#account"));
    assert!(state.db.with(|c| crate::auth::google_linked(c, bob)).unwrap());
    // ... and Ana's account cannot be attached to another account too.
    let (_, cy_sid) = password_account("cy@example.org");
    let (status, _, _, body) = link(&cy_sid, "&login_hint=ana.lopez%40example.org");
    assert_eq!(status, 400);
    assert!(body.contains("already attached to another user"), "{body}");
    assert_eq!(state.db.with(|c| crate::auth::find_user_by_google(c, &ana_sub)).unwrap(), Some(ana));

    // Each failure, as the login page reports it (timeout is left to the browser specs: 15 s).
    for (error, shown) in [
        ("cancel", "Google sign-in was cancelled."),
        ("invalid_grant", "Could not complete the sign-in with Google."),
        ("unavailable", "Could not complete the sign-in with Google."),
        ("unverified", "Google returned an identity this site cannot accept."),
    ] {
        let (status, _, _, body) = through(&format!("?mock_email=carl@example.org&mock_error={error}"), "");
        assert_eq!(status, 200, "{error}");
        assert!(body.contains(r#"data-testid="login-error""#) && body.contains(shown), "{error}: {body}");
    }
    assert!(state.db.with(|c| crate::auth::find_user_by_google(c, &dev::mock_google_account("carl@example.org").0)).unwrap().is_none());

    // Forgetting (the login page's dev line, no user needed): Bob keeps
    // his password and loses Google; Ana's Google-only account is deleted
    // and she can sign up again.
    let (status, loc, _, _) = post("/dev/google/forget", "", &[("email", "Bob@example.org")]);
    assert_eq!((status, loc.as_str()), (303, "/login?forgot=unlinked"));
    assert!(!state.db.with(|c| crate::auth::google_linked(c, bob)).unwrap());
    let (status, loc, _, _) = post("/dev/google/forget", "", &[("email", "nobody@example.org")]);
    assert_eq!((status, loc.as_str()), (303, "/login?forgot=none"));
    let (status, loc, _, _) = post("/dev/google/forget", "", &[("email", "ana.lopez@example.org")]);
    assert_eq!((status, loc.as_str()), (303, "/login?forgot=deleted"));
    assert_eq!(state.db.with(|c| crate::auth::find_user_by_google(c, &ana_sub)).unwrap(), None);
    let (status, loc, _, _) = through("?mock_email=ana.lopez@example.org", "");
    assert_eq!((status, loc.as_str()), (303, "/auth/google/confirm"), "Ana signs up again");
    let login = get("/login?forgot=deleted", "").3;
    assert!(login.contains(r#"data-testid="login-dev-forget-google""#) && login.contains("Google-only account is deleted"), "{login}");

    // The stand-in checks its callers like Google does.
    let (status, to_google, _, _) = get("/auth/google?mock_error=explode", "");
    assert_eq!(status, 303);
    let (status, _, _, body) = get(&to_google, "");
    assert!(status == 400 && body.contains("unknown mock_error"), "{status} {body}");
    let (status, _, _, body) = get("/dev/google/auth?client_id=other&redirect_uri=/auth/google/callback&response_type=code&state=s&code_challenge=c&code_challenge_method=S256", "");
    assert!(status == 400 && body.contains("invalid_client"), "{status} {body}");
    let mut r = agent
        .post(&format!("{base}/dev/google/token"))
        .send_form([("grant_type", "authorization_code"), ("client_id", dev::MOCK_GOOGLE_CLIENT_ID), ("client_secret", "wrong"), ("code", "4/mock.x")])
        .unwrap();
    assert_eq!(r.status().as_u16(), 401);
    assert!(r.body_mut().read_to_string().unwrap().contains("invalid_client"));

    // The login and register pages carry one button per outcome (#191).
    for page in ["login", "register"] {
        let body = get(&format!("/{page}"), "").3;
        assert!(body.contains(&format!(r#"href="/auth/google?mock_email=mock.learner%40example.com" title="/auth/google?mock_email=mock.learner%40example.com" data-testid="{page}-dev-google-ok""#)), "{body}");
        for e in dev::MockGoogleError::ALL {
            let e = e.as_str();
            assert!(body.contains(&format!(r#"href="/auth/google?mock_error={e}" title="/auth/google?mock_error={e}" data-testid="{page}-dev-google-{e}""#)), "{page} {e}: {body}");
        }
    }
}

/// Without the stand-in configured (a real client, or none), its routes are not there.
#[test]
fn mock_google_routes_need_the_stand_in() {
    let found: Vec<u16> = statuses(true, &["/dev/google/auth", "/auth/google"]).into_iter().map(|(s, _)| s).collect();
    assert_eq!(found, vec![404, 404]);
}

/// Serve the real router on a free port; GET `paths` without following redirects.
fn statuses(dev: bool, paths: &'static [&'static str]) -> Vec<(u16, String)> {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
    rt.block_on(async move {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, crate::routes::router(state(dev))).await.unwrap() });
        tokio::task::spawn_blocking(move || {
            let agent = dev_agent(false);
            paths
                .iter()
                .map(|p| {
                    let r = agent.get(&format!("http://{addr}{p}")).call().unwrap();
                    let loc = r.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
                    let cookie = r.headers().get("set-cookie").is_some();
                    (r.status().as_u16(), if cookie { format!("{loc} +cookie") } else { loc })
                })
                .collect()
        })
        .await
        .unwrap()
    })
}

/// The button on /dev/mail queues a test message; the page lists it and
/// scripts still get JSON.
#[test]
fn dev_mail_queues_a_test_message() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
    rt.block_on(async move {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, crate::routes::router(state(true))).await.unwrap() });
        tokio::task::spawn_blocking(move || {
            let agent = dev_agent(false);
            let url = |p: &str| format!("http://{addr}{p}");
            let r = agent.post(&url("/dev/mail/send")).send_form([("to", "ann@example.org")]).unwrap();
            assert_eq!(r.status().as_u16(), 303);
            assert_eq!(agent.post(&url("/dev/mail/send")).send_form([("to", "nonsense")]).unwrap().status().as_u16(), 400);
            let json: serde_json::Value = agent.get(&url("/dev/mail")).call().unwrap().body_mut().read_json().unwrap();
            let m = &json["messages"][0];
            assert_eq!((m["to"].as_str(), m["purpose"].as_str(), m["state"].as_str()), (Some("ann@example.org"), Some("dev-test"), Some("queued")));
            let html = agent.get(&url("/dev/mail")).header("accept", "text/html").call().unwrap().body_mut().read_to_string().unwrap();
            assert!(html.contains(r#"data-testid="dev-mail-send""#) && html.contains("ann@example.org"), "{html}");
            assert!(html.contains(r#"data-testid="dev-mail-message""#) && html.contains(r#"data-read="false""#), "{html}");
            // One row's page (marks it read), archive, delete for good.
            let id = m["id"].as_i64().unwrap();
            let one = agent.get(&url(&format!("/dev/mail/{id}"))).header("accept", "text/html").call().unwrap().body_mut().read_to_string().unwrap();
            assert!(one.contains(r#"data-testid="dev-mail-subject""#) && one.contains("If you can read this"), "{one}");
            let rows: serde_json::Value = agent.get(&url("/dev/inbox/rows")).call().unwrap().body_mut().read_json().unwrap();
            assert_eq!(rows["out"]["unread"].as_i64(), Some(0));
            assert!(rows["out"]["html"].as_str().unwrap().contains(r#"data-read="true""#));
            let act = |action: &str| agent.post(&url(&format!("/dev/mail/{id}"))).header("x-dev-box", "1").send_form([("do", action)]).unwrap().status().as_u16();
            assert_eq!(act("archive"), 204);
            let rows: serde_json::Value = agent.get(&url("/dev/inbox/rows")).call().unwrap().body_mut().read_json().unwrap();
            assert!(rows["out"]["html"].as_str().unwrap().contains(r#"data-archived="true""#));
            assert_eq!(act("delete"), 204);
            assert_eq!(act("delete"), 404);
            assert_eq!(agent.get(&url(&format!("/dev/mail/{id}"))).call().unwrap().status().as_u16(), 404);
            let json: serde_json::Value = agent.get(&url("/dev/mail")).call().unwrap().body_mut().read_json().unwrap();
            assert_eq!(json["messages"].as_array().map(Vec::len), Some(0));
        })
        .await
        .unwrap()
    });
}

/// /dev/jobs lists the jobs table; its buttons queue a failing job and
/// make a row due now (#872).
#[test]
fn dev_jobs_lists_and_acts_on_rows() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
    rt.block_on(async move {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, crate::routes::router(state(true))).await.unwrap() });
        tokio::task::spawn_blocking(move || {
            let agent = dev_agent(false);
            let url = |p: &str| format!("http://{addr}{p}");
            let html = agent.get(&url("/dev/jobs")).header("accept", "text/html").call().unwrap().body_mut().read_to_string().unwrap();
            assert!(html.contains(r#"data-testid="dev-jobs-empty""#), "{html}");
            assert_eq!(agent.post(&url("/dev/jobs/fail")).send_form([("x", "")]).unwrap().status().as_u16(), 303);
            let json: serde_json::Value = agent.get(&url("/dev/jobs")).call().unwrap().body_mut().read_json().unwrap();
            let job = &json["jobs"][0];
            assert_eq!((job["kind"].as_str(), job["state"].as_str()), (Some("dev.fail"), Some("queued")));
            let id = job["id"].as_i64().unwrap();
            let html = agent.get(&url("/dev/jobs")).header("accept", "text/html").call().unwrap().body_mut().read_to_string().unwrap();
            assert!(html.contains(r#"data-kind="dev.fail""#) && html.contains(r#"data-testid="dev-jobs-run-now""#), "{html}");
            let act = |id: i64, what: &str| agent.post(&url(&format!("/dev/jobs/{id}"))).send_form([("do", what)]).unwrap().status().as_u16();
            assert_eq!(act(id, "run_now"), 303);
            assert_eq!(act(id, "explode"), 400);
            assert_eq!(act(id + 99, "run_now"), 404);
        })
        .await
        .unwrap()
    });
}

/// /dev/inbox lists what the dev mailbox received and opens one message
/// with its HTML framed and its links; without the mailbox it says so.
#[test]
fn dev_inbox_shows_what_arrived() {
    let inbox = crate::mail::sink::start(0).unwrap();
    let from: lettre::message::Mailbox = "Skrongle <noreply@localhost>".parse().unwrap();
    let mail = crate::mail::Outgoing {
        id: 7,
        to: "ann+x@example.org".into(),
        subject: "Confirm".into(),
        text: "Open http://127.0.0.1:9/verify-email?t=abc".into(),
        html: Some("<p><a href=\"http://127.0.0.1:9/verify-email?t=abc\">Confirm</a></p>".into()),
        purpose: "test".into(),
        reply_to: None,
    };
    crate::mail::sink::provider(&inbox, &from).unwrap().send(&from, &mail).unwrap();
    let mut with = state(true);
    Arc::get_mut(&mut with).unwrap().dev_inbox = Some(inbox);
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
    rt.block_on(async move {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, crate::routes::router(with)).await.unwrap() });
        let off = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let off_addr = off.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(off, crate::routes::router(state(true))).await.unwrap() });
        tokio::task::spawn_blocking(move || {
            let agent = dev_agent(false);
            let url = |p: &str| format!("http://{addr}{p}");
            let page = |u: &str| agent.get(u).header("accept", "text/html").call().unwrap().body_mut().read_to_string().unwrap();
            let json: serde_json::Value = agent.get(&url("/dev/inbox?to=ANN%2Bx@example.org")).call().unwrap().body_mut().read_json().unwrap();
            let m = &json["messages"][0];
            assert_eq!((m["subject"].as_str(), m["outbox_id"].as_i64()), (Some("Confirm"), Some(7)));
            assert_eq!(m["links"][0].as_str(), Some("http://127.0.0.1:9/verify-email?t=abc"));
            let id = m["id"].as_i64().unwrap();
            let none: serde_json::Value = agent.get(&url("/dev/inbox?to=bob@example.org")).call().unwrap().body_mut().read_json().unwrap();
            assert_eq!(none["messages"].as_array().map(Vec::len), Some(0));
            let r = agent.get(&url("/dev/inbox/latest?to=ann%2Bx@example.org")).call().unwrap();
            assert_eq!(r.headers().get("location").unwrap().to_str().unwrap(), format!("/dev/inbox/{id}"));
            let r = agent.get(&url("/dev/inbox/latest?to=bob@example.org")).call().unwrap();
            assert_eq!(r.headers().get("location").unwrap().to_str().unwrap(), "/dev/inbox?to=bob@example.org");
            let list = page(&url("/dev/inbox"));
            assert!(list.contains(r#"data-testid="dev-inbox-item""#) && list.contains(r#"data-read="false""#), "{list}");
            let rows = |q: &str| -> serde_json::Value { agent.get(&url(&format!("/dev/inbox/rows{q}"))).call().unwrap().body_mut().read_json().unwrap() };
            assert_eq!(rows("")["in"]["unread"].as_i64(), Some(1));
            assert!(rows("?to=bob@example.org")["in"]["html"].as_str().unwrap().contains("No messages to bob@example.org."));
            // JSON reads leave it unread; opening it in a browser marks it read.
            agent.get(&url(&format!("/dev/inbox/{id}?format=json"))).call().unwrap();
            assert_eq!(rows("")["in"]["unread"].as_i64(), Some(1));
            let html = page(&url(&format!("/dev/inbox/{id}")));
            assert_eq!(rows("")["in"]["unread"].as_i64(), Some(0));
            assert!(rows("")["in"]["html"].as_str().unwrap().contains(r#"data-read="true" data-archived="false""#));
            assert!(html.contains(r#"data-testid="dev-inbox-html""#) && html.contains("srcdoc=\"&#60;base target=&#34;_top&#34;&#62;"), "{html}");
            assert!(html.contains(r#"data-testid="dev-inbox-link""#), "{html}");
            let raw = agent.get(&url(&format!("/dev/inbox/{id}/raw"))).call().unwrap().body_mut().read_to_string().unwrap();
            assert!(raw.contains("Subject: Confirm"), "{raw}");
            assert_eq!(agent.get(&url("/dev/inbox/999")).call().unwrap().status().as_u16(), 404);
            // Archive: struck out (marked), 204 for the page's own posts; a form post goes back.
            let act = |action: &str, live: bool| {
                let mut r = agent.post(&url(&format!("/dev/inbox/{id}")));
                if live {
                    r = r.header("x-dev-box", "1");
                }
                r.send_form([("do", action)]).unwrap().status().as_u16()
            };
            assert_eq!(act("archive", true), 204);
            assert!(rows("")["in"]["html"].as_str().unwrap().contains(r#"data-archived="true""#));
            assert_eq!(act("unarchive", false), 303);
            assert!(rows("")["in"]["html"].as_str().unwrap().contains(r#"data-archived="false""#));
            assert_eq!(act("shred", true), 400);
            assert_eq!(agent.post(&url("/dev/inbox/999")).send_form([("do", "delete")]).unwrap().status().as_u16(), 404);
            assert_eq!(agent.post(&url("/dev/inbox/clear")).send_empty().unwrap().status().as_u16(), 303);
            assert_eq!(agent.get(&url(&format!("/dev/inbox/{id}"))).call().unwrap().status().as_u16(), 404);

            let off = |p: &str| format!("http://{off_addr}{p}");
            assert!(page(&off("/dev/inbox")).contains(r#"data-testid="dev-inbox-off""#));
            assert_eq!(agent.get(&off("/dev/inbox/1")).call().unwrap().status().as_u16(), 404);
        })
        .await
        .unwrap()
    });
}

#[test]
fn dev_routes_404_unless_dev_mode_is_on() {
    let off = statuses(false, &["/dev/login", "/dev/reset", "/dev/ai", "/healthz"]);
    assert_eq!(off.iter().map(|s| s.0).collect::<Vec<_>>(), vec![404, 404, 404, 200]);
    let on = statuses(true, &["/dev/login?open=spanish&keys=fake&next=/courses/generate", "/dev/login?open=nope", "/dev/ai", "/dev/reset"]);
    assert_eq!(on[0], (303, "/courses/generate +cookie".to_string()));
    assert_eq!(on[1].0, 400);
    assert_eq!(on[2].0, 200);
    assert_eq!(on[3].0, 303);
}

/// Dev mode has no keyless stand-in: with no key saved the generate
/// form asks for one, and `?keys=fake` (like `keys=dev`) saves the
/// simulated provider's key for every provider, so the form offers each.
#[test]
fn dev_keys_unlock_the_generate_form_for_every_provider() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    rt.block_on(async move {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, crate::routes::router(state(true))).await.unwrap() });
        tokio::task::spawn_blocking(move || {
            let agent = dev_agent(false);
            let url = |p: &str| format!("http://{addr}{p}");
            let editor = |cookie: &str| {
                let made = agent.post(&url("/courses/new")).header("cookie", cookie).send_form([("title", "")]).unwrap();
                let editor = made.headers().get("location").unwrap().to_str().unwrap().to_string();
                agent.get(&url(&editor)).header("cookie", cookie).call().unwrap().body_mut().read_to_string().unwrap()
            };
            let login = |q: &str| {
                let r = agent.get(&url(&format!("/dev/login?open=spanish{q}"))).call().unwrap();
                r.headers().get("set-cookie").unwrap().to_str().unwrap().split(';').next().unwrap().to_string()
            };
            let html = editor(&login(""));
            assert!(html.contains("generate-no-key"), "no saved key: the form asks for one");
            assert!(!html.contains(r#"value="mock""#), "no mock provider");
            let html = editor(&login("&keys=fake"));
            assert!(!html.contains("generate-no-key"));
            for p in ["anthropic", "openai", "google"] {
                assert!(html.contains(&format!(r#"value="{p}""#)), "{p}: one of the dev user's keys");
            }
            assert_eq!(html.matches(r#"data-testid="generate-provider""#).count(), 3, "a choice of the three");
        })
        .await
        .unwrap()
    })
}

/// The login and register pages offer the dev login only in dev mode.
#[test]
fn auth_pages_offer_the_dev_login_in_dev_mode() {
    let pages = |dev: bool| {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        rt.block_on(async move {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move { axum::serve(listener, crate::routes::router(state(dev))).await.unwrap() });
            tokio::task::spawn_blocking(move || {
                ["/login", "/register"].map(|p| ureq::get(&format!("http://{addr}{p}")).call().unwrap().body_mut().read_to_string().unwrap())
            })
            .await
            .unwrap()
        })
    };
    let [login, register] = pages(true);
    assert!(login.contains(r#"data-testid="login-dev-login""#) && login.contains("/dev/login"));
    assert!(register.contains(r#"data-testid="register-dev-login""#));
    assert!(pages(false).iter().all(|html| !html.contains("dev-login")));
}

#[test]
fn landing_stays_inside_the_app() {
    assert_eq!(landing(Some("/courses/generate")), "/courses/generate");
    assert_eq!(landing(Some("//evil.example")), "/");
    assert_eq!(landing(Some("https://evil.example")), "/");
    assert_eq!(landing(None), "/");
}
