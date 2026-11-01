/// The startup line and /healthz say which edition runs (#641), and
/// /healthz still starts with "ok" for the health checks.
#[test]
fn healthz_names_the_edition() {
    let version = crate::VERSION;
    assert_eq!(crate::edition(true), format!("skrongle mini {version}"));
    assert_eq!(crate::edition(false), format!("skrongle {version}"));
    let body = tokio::runtime::Builder::new_current_thread().build().map(|rt| rt.block_on(super::healthz()));
    assert!(body.is_ok_and(|b| b.starts_with("ok skrongle ")));
}

#[test]
fn asset_urls_are_versioned() {
    let v = super::asset_version();
    assert_eq!(v.len(), 10);
    assert!(v.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(super::asset_url("style.css"), format!("/static/style.css?v={v}"));
    assert!(super::Assets::get("style.css").is_some());
}

/// A comment opener plus a script tag would put the HTML tokenizer in
/// the double-escaped state and swallow the real `</script>`; every
/// markup character leaves the JSON escaped, and it still parses back.
#[test]
fn script_json_escapes_markup_characters() {
    let text = "<!--<script>x</script> & \u{2028}\u{2029} >";
    let out = super::script_json(&serde_json::json!({ "t": text })).unwrap();
    for bad in ['<', '>', '&', '\u{2028}', '\u{2029}'] {
        assert!(!out.contains(bad), "{bad:?} left in {out}");
    }
    assert!(out.contains("\\u003c!--\\u003cscript\\u003e"));
    let back: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(back["t"], text);
}

#[test]
fn time_ago_formats() {
    assert_eq!(super::time_ago(100, 130), "just now");
    assert_eq!(super::time_ago(0, 60), "1 minute ago");
    assert_eq!(super::time_ago(0, 7200), "2 hours ago");
    assert_eq!(super::time_ago(0, 3 * 86_400), "3 days ago");
}

/// Every response carries the security headers; a POST from another site
/// is refused before its handler runs, except Stripe's webhook.
#[test]
fn security_headers_and_cross_site_posts() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let app = super::router(super::test_state(false, None));
    let base = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    });
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    for path in ["/login", "/no-such-page", "/static/style.css"] {
        let r = agent.get(&format!("{base}{path}")).call().unwrap();
        let h = r.headers();
        assert!(h.get("content-security-policy").unwrap().to_str().unwrap().contains("frame-ancestors 'none'"), "{path}");
        assert_eq!(h.get("x-frame-options").unwrap(), "DENY", "{path}");
        assert_eq!(h.get("x-content-type-options").unwrap(), "nosniff", "{path}");
        assert_eq!(h.get("referrer-policy").unwrap(), "same-origin", "{path}");
        assert!(h.get("strict-transport-security").is_none(), "no HSTS over plain HTTP");
    }
    let login = |headers: &[(&str, &str)]| {
        let mut req = agent.post(&format!("{base}/login"));
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        req.send_form([("email", "nobody@example.org"), ("password", "wrong-password")]).unwrap().status().as_u16()
    };
    let host = base.trim_start_matches("http://");
    assert_eq!(login(&[]), 200, "no browser headers: not a cross-site form");
    assert_eq!(login(&[("sec-fetch-site", "same-origin")]), 200);
    assert_eq!(login(&[("origin", &base)]), 200);
    assert_eq!(login(&[("sec-fetch-site", "cross-site")]), 403);
    assert_eq!(login(&[("origin", "https://evil.example"), ("host", host)]), 403);
    let webhook = agent.post(&format!("{base}/billing/webhook")).header("sec-fetch-site", "cross-site").send("{}").unwrap();
    assert_ne!(webhook.status().as_u16(), 403, "the webhook is exempt");
}

/// #370: bodies are capped at 64 KiB, except on the session-gated editor
/// and AI routes; an address over the sign-in limit is refused before
/// its body is read.
#[test]
fn request_bodies_are_capped() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let state = super::test_state(false, None);
    let hash = crate::auth::hash_password("right-password").unwrap();
    let ann = state.db.with(|c| crate::auth::create_user(c, "ann@example.org", &hash)).unwrap();
    state.db.with(|c| crate::terms::accept(c, ann, crate::terms::version())).unwrap();
    let sid = state.db.with(|c| crate::auth::create_session(c, ann)).unwrap();
    let app = super::router(state.clone());
    let base = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap() });
        format!("http://{addr}")
    });
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    let post = |path: &str, cookie: &str, kind: &str, body: &str| {
        agent.post(&format!("{base}{path}")).header("cookie", cookie).header("content-type", kind).send(body).unwrap().status().as_u16()
    };
    let form = "application/x-www-form-urlencoded";
    let padded = |n: usize| format!("email=nobody%40example.org&password=wrong&pad={}", "x".repeat(n));
    let me = format!("sid={sid}");

    assert_eq!(post("/login", "", form, &padded(10)), 200);
    assert_eq!(post("/login", "", form, &padded(super::BODY_LIMIT)), 413, "an account form over 64 KiB");
    assert_eq!(post("/register", "", form, &padded(super::BODY_LIMIT)), 413);
    let json = "application/json";
    let big = format!("{{\"pad\": \"{}\"}}", "x".repeat(200 * 1024));
    assert_eq!(post("/api/profile", &me, json, &big), 413, "a signed-in API over 64 KiB");
    // The editor takes more, signed in (any answer but 413: the course does not exist).
    let lessons = "/api/courses/no-such-course/lessons";
    assert_ne!(post(lessons, &me, json, &big), 413);
    assert_ne!(post("/courses/no-such-course/ai", &me, form, &format!("text={}", "x".repeat(200 * 1024))), 413);
    let huge = format!("{{\"pad\": \"{}\"}}", "x".repeat(super::BIG_BODY_LIMIT));
    assert_eq!(post(lessons, &me, json, &huge), 413, "but not without end");

    // Once an address is over the sign-in limit, its bodies are not even read.
    for _ in 1..crate::security::LOGIN_PER_IP.max {
        post("/login", "", form, &padded(10));
    }
    assert_eq!(post("/login", "", form, &padded(10)), 429);
    assert_eq!(post("/login", "", form, &padded(super::BODY_LIMIT)), 429, "refused before the body");
}

/// #381: the learning write endpoints take a burst per account, then
/// answer 429 with a JSON error (other accounts unaffected); an account
/// records at most `AGENT_ANSWERS_PER_DAY` exercises an agent graded,
/// while course questions still count.
#[test]
fn learning_writes_are_rate_limited() {
    use crate::security::LEARNING_WRITES_PER_USER;
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let state = super::test_state(false, None);
    let learner = |email: &str| {
        state
            .db
            .with(|c| {
                let u = crate::auth::create_user(c, email, "x")?;
                crate::terms::accept(c, u, crate::terms::version())?;
                c.execute("UPDATE users SET active_course = 'spanish' WHERE id = ?1", [u])?;
                Ok((u, crate::auth::create_session(c, u)?))
            })
            .unwrap()
    };
    let ((_, ann), (bob_id, bob)) = (learner("ann@example.org"), learner("bob@example.org"));
    let course = crate::content::ContentStore::load_dir(std::path::Path::new("data/courses/spanish")).unwrap();
    let lesson = course.lessons().iter().find(|l| !l.quiz.is_empty()).unwrap();
    let app = super::router(state.clone());
    let base = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    });
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    let post = |path: &str, sid: &str, body: serde_json::Value| {
        let mut r = agent.post(&format!("{base}{path}")).header("cookie", &format!("sid={sid}")).send_json(body).unwrap();
        (r.status().as_u16(), r.body_mut().read_to_string().unwrap())
    };
    let seen = serde_json::json!({ "lesson": lesson.id });
    for _ in 0..LEARNING_WRITES_PER_USER.max {
        assert_eq!(post("/api/progress/seen", &ann, seen.clone()).0, 200);
    }
    let (status, body) = post("/api/progress/seen", &ann, seen.clone());
    assert_eq!(status, 429);
    assert!(serde_json::from_str::<serde_json::Value>(&body).unwrap()["error"].is_string(), "{body}");
    assert_eq!(post("/api/answer", &ann, serde_json::json!({ "question_id": lesson.quiz[0].question_id, "answer": "x" })).0, 429, "every write endpoint counts");
    assert_eq!(post("/api/progress/seen", &bob, seen).0, 200, "another account is not held up");

    // Bob's agent has graded its daily allowance already.
    let day_ago = crate::db::now() - 86_000;
    state
        .db
        .with(|c| {
            let mut stmt = c.prepare(
                "INSERT INTO quiz_history (user_id, course_id, lesson_id, question_id, selected_answer, was_correct, answered_at) VALUES (?1, 'spanish', ?2, 'agent:say hi', 'hola', 1, ?3)",
            )?;
            for _ in 0..super::progress::AGENT_ANSWERS_PER_DAY {
                stmt.execute(rusqlite::params![bob_id, lesson.id, day_ago])?;
            }
            Ok(())
        })
        .unwrap();
    let own = serde_json::json!({ "lesson": lesson.id, "prompt": "Say hello", "answer": "hola", "correct": true });
    let (status, body) = post("/api/progress/answers", &bob, own);
    assert_eq!(status, 429, "{body}");
    let course_question = serde_json::json!({ "question_id": lesson.quiz[0].question_id, "answer": "x" });
    assert_eq!(post("/api/progress/answers", &bob, course_question).0, 200);
}

/// Password guessing is throttled per account and registrations per
/// address; a password change signs out the account's other sessions.
#[test]
fn account_forms_are_rate_limited() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let state = super::test_state(false, None);
    let hash = crate::auth::hash_password("right-password").unwrap();
    let ann = state.db.with(|c| crate::auth::create_user(c, "ann@example.org", &hash)).unwrap();
    state.db.with(|c| crate::terms::accept(c, ann, crate::terms::version())).unwrap();
    let session = |u| state.db.with(|c| crate::auth::create_session(c, u)).unwrap();
    let (here, elsewhere) = (session(ann), session(ann));
    let app = super::router(state.clone());
    let base = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap() });
        format!("http://{addr}")
    });
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    let post = |path: &str, cookie: &str, form: &[(&str, &str)]| {
        agent.post(&format!("{base}{path}")).header("cookie", cookie).send_form(form.iter().copied()).unwrap().status().as_u16()
    };

    // Password change: works, ends the other session, keeps this one.
    let change = |current: &str| post("/profile/password", &format!("sid={here}"), &[("current", current), ("new", "new-password-1"), ("confirm", "new-password-1")]);
    assert_eq!(change("right-password"), 303);
    let alive = |t: &str| state.db.with(|c| Ok(crate::auth::user_for_session(c, t)?.is_some())).unwrap();
    assert!(alive(&here) && !alive(&elsewhere));
    for _ in 1..crate::security::PASSWORD_PER_USER.max {
        assert_eq!(change("wrong"), 200);
    }
    assert_eq!(change("new-password-1"), 429);
    // Deleting the account checks the password on the same allowance.
    assert_eq!(post("/profile/delete", &format!("sid={here}"), &[("password", "new-password-1")]), 429);
    assert!(alive(&here), "the account is still there");

    // Signing in takes the e-mail address, in any case.
    let login = |email: &str, pw: &str| post("/login", "", &[("email", email), ("password", pw)]);
    assert_eq!(login(" ANN@example.org ", "new-password-1"), 303);

    // The profile's wrong passwords counted against signing in (#378):
    // with them, ten wrong passwords for ann, from any address, and
    // even the right one waits.
    let profile_guesses = crate::security::PASSWORD_PER_USER.max - 1;
    for _ in profile_guesses..crate::security::LOGIN_FAILURES_PER_USER.max {
        assert_eq!(login("ann@example.org", "guess"), 200);
    }
    assert_eq!(login("ann@example.org", "new-password-1"), 429);
    assert_eq!(login("ANN@example.org", "new-password-1"), 429, "addresses are case-insensitive");

    // And the other way: while sign-in is blocked, the profile's password forms wait too (#378).
    let bea = state.db.with(|c| crate::auth::create_user(c, "bea@example.org", &hash)).unwrap();
    state.db.with(|c| crate::terms::accept(c, bea, crate::terms::version())).unwrap();
    let bea_sid = format!("sid={}", session(bea));
    for _ in 0..crate::security::LOGIN_FAILURES_PER_USER.max {
        assert_eq!(login("bea@example.org", "guess"), 200);
    }
    let bea_change = post("/profile/password", &bea_sid, &[("current", "right-password"), ("new", "new-password-2"), ("confirm", "new-password-2")]);
    assert_eq!(bea_change, 429);
    assert_eq!(post("/profile/delete", &bea_sid, &[("password", "right-password")]), 429);

    // Registration needs the age and Terms boxes, and records both.
    let new_user = |email: &str| state.db.with(|c| crate::auth::find_user_credentials(c, email)).unwrap().map(|(id, _)| id);
    assert_eq!(post("/register", "", &[("email", "cy@example.org"), ("password", "long-enough-pw")]), 200);
    assert_eq!(new_user("cy@example.org"), None, "no account without the box");
    assert_eq!(post("/register", "", &[("email", "cy@example.org"), ("password", "long-enough-pw"), ("age", "on"), ("accept", "on")]), 200, "the check-your-mail page");
    let cy = new_user("cy@example.org").expect("registered");
    let age_confirmed = state.db.with(|c| c.query_row("SELECT age_confirmed FROM users WHERE id = ?1", [cy], |r| r.get::<_, bool>(0))).unwrap();
    assert!(age_confirmed, "registration records the age confirmation");

    // Registrations from one address.
    for _ in 2..crate::security::REGISTER_PER_IP.max {
        assert_eq!(post("/register", "", &[("email", ""), ("password", "p")]), 200);
    }
    assert_eq!(post("/register", "", &[("email", "n@example.org"), ("password", "long-enough-pw")]), 429);
}

/// The second sign-in step counts every wrong code (the last one too)
/// against the account, and refuses codes and resends while the
/// account is blocked (#359).
#[test]
fn sign_in_codes_count_against_the_account_limit() {
    use crate::two_factor;
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let state = super::test_state(false, None);
    let ann = state.db.with(|c| crate::auth::create_user(c, "ann@example.org", "h")).unwrap();
    state.db.with(|c| crate::verify::mark_verified(c, ann, 1)).unwrap();
    state.db.with(|c| two_factor::set_method(c, ann, Some(two_factor::Method::Email)).map(|r| r.unwrap())).unwrap();
    let app = super::router(state.clone());
    let base = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap() });
        format!("http://{addr}")
    });
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    let post = |path: &str, token: &str, form: &[(&str, &str)]| {
        agent.post(&format!("{base}{path}")).header("cookie", &format!("{}={token}", two_factor::COOKIE)).send_form(form.iter().copied()).unwrap().status().as_u16()
    };
    let challenge = || state.db.with(|c| two_factor::start_login(c, ann, crate::db::now())).unwrap().unwrap();
    let blocked = || state.limits.blocked(crate::security::LOGIN_FAILURES_PER_USER, "ann@example.org", crate::db::now());

    // Two sign-ins' worth of wrong codes is the account's whole allowance
    // (2 x 5 = 10), so the last wrong code must count too.
    assert_eq!(2 * two_factor::MAX_ATTEMPTS, crate::security::LOGIN_FAILURES_PER_USER.max as i64);
    for _ in 0..2 {
        let ch = challenge();
        let code = ch.send.clone().unwrap().1;
        let wrong = if code == "999999" { "999998" } else { "999999" };
        assert!(!blocked());
        for _ in 1..two_factor::MAX_ATTEMPTS {
            assert_eq!(post("/login/code", &ch.token, &[("code", wrong)]), 422);
        }
        assert_eq!(post("/login/code", &ch.token, &[("code", wrong)]), 200, "the last wrong code ends the sign-in");
    }
    assert!(blocked(), "ten wrong codes block the account");

    // A blocked account: even the right code and a resend are refused.
    let ch = challenge();
    let code = ch.send.clone().unwrap().1;
    assert_eq!(post("/login/code/resend", &ch.token, &[]), 429);
    assert_eq!(post("/login/code", &ch.token, &[("code", &code)]), 429);
    assert!(state.db.with(|c| two_factor::pending(c, &ch.token, crate::db::now())).unwrap().is_some(), "no try was used");
}

/// Registering queues a confirmation link; opening it (signed in or not)
/// confirms the address once; the dashboard and profile say where things
/// stand; "Send it again" is limited per account.
/// Content reports and copyright notices (#968): a learner reports a
/// course shared with them (not their own, not a built-in one); anyone
/// sends a copyright notice, five a minute; both reach the superuser by
/// mail and on the Superuser page, where Forward to owner mails the
/// course's owner and Resolve takes the report off the list. Without
/// mail (Skrongle mini) the form is the contact address and posts 404.
#[test]
fn content_reports_reach_the_operator_and_the_owner() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let serve = |state: std::sync::Arc<crate::AppState>| {
        let app = super::router(state);
        rt.block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move { axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap() });
            format!("http://{addr}")
        })
    };
    let state = super::test_state(false, None);
    let base = serve(state.clone());
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    let get = |base: &str, path: &str, cookie: &str| agent.get(&format!("{base}{path}")).header("cookie", cookie).call().unwrap().body_mut().read_to_string().unwrap();
    let post = |base: &str, path: &str, cookie: &str, form: &[(&str, &str)]| {
        let r = agent.post(&format!("{base}{path}")).header("cookie", cookie).send_form(form.iter().copied()).unwrap();
        let set = r.headers().get("set-cookie").map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string()).unwrap_or_default();
        let loc = r.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
        (r.status().as_u16(), set, loc)
    };
    let mut sids = Vec::new();
    for who in ["root", "ann", "bob"] {
        let email = format!("{who}@example.org");
        post(&base, "/register", "", &[("email", &email), ("password", "long-enough-pw"), ("age", "on"), ("accept", "on")]);
        sids.push(post(&base, "/login", "", &[("email", &email), ("password", "long-enough-pw")]).1);
    }
    let (root, ann, bob) = (&sids[0], &sids[1], &sids[2]);
    state
        .db
        .with(|c| {
            c.execute_batch(
                "UPDATE users SET superuser = 1 WHERE email = 'root@example.org';
                 UPDATE users SET email_verified_at = 5;
                 INSERT INTO courses (id, owner_id, title, created_at, updated_at) SELECT 'verbs-abc123', id, 'Verbs', 1, 1 FROM users WHERE email = 'ann@example.org';
                 INSERT INTO course_shares (course_id, email, user_id, token_hash, created_at) SELECT 'verbs-abc123', email, id, 'h', 1 FROM users WHERE email = 'bob@example.org';",
            )
        })
        .unwrap();
    let mails = |purpose: &'static str| {
        state
            .db
            .with(move |c| {
                let mut st = c.prepare("SELECT to_addr, body_text FROM mail_outbox WHERE purpose = ?1 ORDER BY id")?;
                let rows = st.query_map([purpose], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>();
                rows
            })
            .unwrap()
    };
    let report = [("reason", "harmful"), ("email", "bob@example.org"), ("details", "Lesson 3 is cruel.")];

    // Bob reports the course shared with him; its owner and a built-in course cannot be reported.
    assert_eq!(post(&base, "/courses/verbs-abc123/report", ann, &report).0, 404, "the owner's own course");
    assert_eq!(post(&base, "/courses/spanish/report", bob, &report).0, 404, "a built-in course");
    assert_eq!(post(&base, "/courses/verbs-abc123/report", bob, &[("reason", "rude"), ("email", "bob@example.org"), ("details", "x")]).0, 400);
    let (status, _, loc) = post(&base, "/courses/verbs-abc123/report", bob, &report);
    assert_eq!((status, loc.as_str()), (303, "/courses?notice=reported"));
    let to_root = mails("content-report");
    assert_eq!(to_root.len(), 1);
    assert_eq!(to_root[0].0, "root@example.org");
    assert!(to_root[0].1.contains("Lesson 3 is cruel.") && to_root[0].1.contains("/superuser#reports"), "{}", to_root[0].1);
    assert!(get(&base, "/courses?notice=reported", bob).contains("Your report reached us"));

    // Signed out: the copyright form, linked from the Terms; the notice names the course by its link.
    assert!(get(&base, "/terms", "").contains(r#"data-testid="terms-copyright-link""#));
    assert!(get(&base, "/legal/copyright", "").contains(r#"data-testid="copyright-form""#));
    let notice = [
        ("name", "Cleo Claimant"),
        ("email", "cleo@example.org"),
        ("work", "My verb tables"),
        ("location", "https://example.org/courses/verbs-abc123"),
        ("details", "Lesson 2 copies my tables."),
        ("good_faith", "on"),
    ];
    let (status, _, _) = post(&base, "/legal/copyright", "", &notice[..5]);
    assert_eq!(status, 200, "without the statement the form comes back");
    let (status, _, loc) = post(&base, "/legal/copyright", "", &notice);
    assert_eq!((status, loc.as_str()), (303, "/legal/copyright?sent=1"));
    assert!(get(&base, "/legal/copyright?sent=1", "").contains(r#"data-testid="copyright-sent""#));
    assert_eq!(mails("content-report").len(), 2);

    // The Superuser page lists both; forwarding mails ann a copy, the claimant's address only on the notice.
    let page = get(&base, "/superuser", root);
    assert_eq!(page.matches(r#"data-testid="superuser-report""#).count(), 2, "{page}");
    let ids: Vec<i64> = state.db.with(|c| c.prepare("SELECT id FROM content_reports ORDER BY id")?.query_map([], |r| r.get(0))?.collect()).unwrap();
    assert_eq!(post(&base, "/superuser/reports/forward", bob, &[("id", &ids[0].to_string())]).0, 404, "superusers only");
    for id in &ids {
        let (_, _, loc) = post(&base, "/superuser/reports/forward", root, &[("id", &id.to_string())]);
        assert_eq!(loc, "/superuser?did=report-forwarded#reports");
    }
    let fwd = mails("content-report-forward");
    assert_eq!(fwd.len(), 2);
    assert!(fwd.iter().all(|(to, _)| to == "ann@example.org"));
    assert!(!fwd[0].1.contains("bob@example.org") && fwd[0].1.contains("Lesson 3 is cruel."), "{}", fwd[0].1);
    assert!(fwd[1].1.contains("cleo@example.org") && fwd[1].1.contains("Cleo Claimant"), "{}", fwd[1].1);
    let (_, _, loc) = post(&base, "/superuser/reports/resolve", root, &[("id", &ids[0].to_string())]);
    assert_eq!(loc, "/superuser?did=report-resolved#reports");
    assert_eq!(get(&base, "/superuser", root).matches(r#"data-testid="superuser-report""#).count(), 1);

    // Five tries a minute from one address (the one without the statement counted): the sixth is refused.
    for _ in 0..3 {
        assert_eq!(post(&base, "/legal/copyright", "", &notice).0, 303);
    }
    assert_eq!(post(&base, "/legal/copyright", "", &notice).0, 429);

    // Skrongle mini: no mail, so the contact address only, and nothing to post to.
    let mini = serve(super::test_state_without(false));
    let page = get(&mini, "/legal/copyright", "");
    assert!(page.contains(r#"data-testid="copyright-contact""#) && !page.contains(r#"data-testid="copyright-form""#), "{page}");
    assert_eq!(post(&mini, "/legal/copyright", "", &notice).0, 404);
}

/// The Superuser page (#220): only superusers see it (or its link in
/// the account menu) or may post to it; "Reset email verification"
/// unconfirms any account and sends it a new link.
#[test]
fn superusers_reset_email_verification() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let state = super::test_state(false, None);
    let app = super::router(state.clone());
    let base = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap() });
        format!("http://{addr}")
    });
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    let get = |path: &str, cookie: &str| agent.get(&format!("{base}{path}")).header("cookie", cookie).call().unwrap().body_mut().read_to_string().unwrap();
    let status = |path: &str, cookie: &str| agent.get(&format!("{base}{path}")).header("cookie", cookie).call().unwrap().status().as_u16();
    let post = |path: &str, cookie: &str, form: &[(&str, &str)]| {
        let r = agent.post(&format!("{base}{path}")).header("cookie", cookie).send_form(form.iter().copied()).unwrap();
        let set = r.headers().get("set-cookie").map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string()).unwrap_or_default();
        let loc = r.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
        (r.status().as_u16(), set, loc)
    };
    let sql = |q: &'static str, email: &'static str| state.db.with(move |c| c.query_row(q, [email], |r| r.get::<_, i64>(0))).unwrap();
    post("/register", "", &[("email", "root@example.org"), ("password", "long-enough-pw"), ("age", "on"), ("accept", "on")]);
    let (_, root, _) = post("/login", "", &[("email", "root@example.org"), ("password", "long-enough-pw")]);
    post("/register", "", &[("email", "bob@example.org"), ("password", "long-enough-pw"), ("age", "on"), ("accept", "on")]);
    let (_, bob, _) = post("/login", "", &[("email", "bob@example.org"), ("password", "long-enough-pw")]);
    state.db.with(|c| c.execute("UPDATE users SET superuser = 1 WHERE email = 'root@example.org'", [])).unwrap();
    state.db.with(|c| c.execute("UPDATE users SET email_verified_at = 5 WHERE email = 'bob@example.org'", [])).unwrap();
    let bob_links = || sql("SELECT COUNT(*) FROM mail_outbox o JOIN users u ON u.id = o.user_id WHERE u.email = ?1 AND o.purpose = 'email-verify'", "bob@example.org");

    // Not a superuser: no link in the menu, the page is a 404, and posting by hand is a 404.
    assert!(!get("/profile", &bob).contains("nav-superuser"));
    assert_eq!(status("/superuser", &bob), 404);
    assert_eq!(post("/superuser/reset-email-verification", &bob, &[("email", "bob@example.org")]).0, 404);
    assert_eq!(post("/superuser/terms-announce", &bob, &[]).0, 404);
    assert_eq!(sql("SELECT email_verified_at FROM users WHERE email = ?1", "bob@example.org"), 5);

    // A superuser: the link, and the page with every account and its actions.
    assert!(get("/profile", &root).contains(r#"href="/superuser" data-testid="nav-superuser""#));
    let page = get("/superuser", &root);
    assert!(page.contains(r#"data-testid="superuser-accounts""#) && page.contains(r#"data-account-email="bob@example.org""#) && page.contains("superuser-verify-dialog"), "{page}");
    assert!(!page.contains("superuser-pager"), "two accounts: one page");
    // No coming change of the terms in the changelog: no notice, no Send.
    assert!(!page.contains("superuser-terms-changed") && !page.contains("superuser-terms-send"), "{page}");

    // Someone else's address, in any case: unconfirmed again, with a new link.
    let (status, _, loc) = post("/superuser/reset-email-verification", &root, &[("email", " BOB@example.org ")]);
    assert_eq!((status, loc.as_str()), (303, "/superuser?did=reset&email=bob%40example.org"));
    assert_eq!(sql("SELECT COUNT(*) FROM users WHERE email = ?1 AND email_verified_at IS NULL", "bob@example.org"), 1);
    assert_eq!(bob_links(), 2);
    assert!(get(&loc, &root).contains("bob@example.org is unconfirmed again"));
    assert!(!get(&loc, &bob).contains("unconfirmed again"), "only superusers get the notice");

    // An address nobody has.
    let (_, _, loc) = post("/superuser/reset-email-verification", &root, &[("email", "nobody@example.org")]);
    assert!(get(&loc, &root).contains(r#"data-testid="superuser-error""#));
    assert_eq!(bob_links(), 2);

    // Pages of 25, newest first; a page past the end is the last one.
    state
        .db
        .with(|c| {
            for n in 0..30 {
                crate::auth::create_user(c, &format!("u{n:02}@example.org"), "h")?;
            }
            Ok(())
        })
        .unwrap();
    let page = get("/superuser", &root);
    assert_eq!(page.matches("superuser-account-row").count(), 25, "{page}");
    assert!(page.contains("<td>u29@example.org</td>") && !page.contains("<td>root@example.org</td>"), "{page}");
    assert!(page.contains("Page 1 of 2") && page.contains(r#"href="/superuser?page=2" data-testid="superuser-next""#) && !page.contains("superuser-prev"), "{page}");
    let page = get("/superuser?page=2", &root);
    assert_eq!(page.matches("superuser-account-row").count(), 7, "{page}");
    assert!(page.contains("<td>root@example.org</td>") && page.contains(r#"href="/superuser?page=1" data-testid="superuser-prev""#) && !page.contains("superuser-next"), "{page}");
    assert!(get("/superuser?page=9", &root).contains("Page 2 of 2"));
    assert!(get("/superuser?page=0", &root).contains("Page 1 of 2"));
}

/// The Superuser page's account actions (#635): only superusers may
/// post them. A temporary password is shown once and signs the account
/// out; signing in with it leads to `/password/change` (API: 403) until
/// a new one is chosen. "Turn off two-factor" clears the method and key.
#[test]
fn superusers_reset_passwords_and_two_factor() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let state = super::test_state(false, None);
    let app = super::router(state.clone());
    let base = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap() });
        format!("http://{addr}")
    });
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    let get = |path: &str, cookie: &str| {
        let mut r = agent.get(&format!("{base}{path}")).header("cookie", cookie).call().unwrap();
        let loc = r.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
        (r.status().as_u16(), loc, r.body_mut().read_to_string().unwrap())
    };
    // (status, session cookie set, location, body, cache-control)
    let post = |path: &str, cookie: &str, form: &[(&str, &str)]| {
        let mut r = agent.post(&format!("{base}{path}")).header("cookie", cookie).send_form(form.iter().copied()).unwrap();
        let header = |h: &str| r.headers().get(h).and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
        let set = header("set-cookie").split(';').next().unwrap_or_default().to_string();
        let (loc, cache) = (header("location"), header("cache-control"));
        (r.status().as_u16(), set, loc, r.body_mut().read_to_string().unwrap(), cache)
    };
    // Registering signs nobody in (#379): the session comes from /login.
    let register = |email: &str| {
        post("/register", "", &[("email", email), ("password", "long-enough-pw"), ("age", "on"), ("accept", "on")]);
        post("/login", "", &[("email", email), ("password", "long-enough-pw")]).1
    };
    let (root, bob) = (register("root@example.org"), register("bob@example.org"));
    state.db.with(|c| c.execute("UPDATE users SET superuser = 1 WHERE email = 'root@example.org'", [])).unwrap();
    let bob_id: i64 = state.db.with(|c| c.query_row("SELECT id FROM users WHERE email = 'bob@example.org'", [], |r| r.get(0))).unwrap();
    let hash = || state.db.with(|c| crate::profile::password_hash(c, bob_id)).unwrap().unwrap();
    let flagged = || state.db.with(|c| c.query_row("SELECT must_change_password FROM users WHERE id = ?1", [bob_id], |r| r.get::<_, bool>(0))).unwrap();
    let old_hash = hash();

    // Not a superuser: 404 on both, and nothing changes.
    assert_eq!(post("/superuser/temp-password", &bob, &[("email", "root@example.org")]).0, 404);
    assert_eq!(post("/superuser/two-factor-off", &bob, &[("email", "root@example.org")]).0, 404);
    assert_eq!(get("/superuser", &bob).0, 404);

    // A superuser: each row offers both actions.
    let (_, _, page) = get("/superuser", &root);
    assert!(page.contains(r#"data-account-email="bob@example.org""#) && page.contains("superuser-two-factor-dialog"), "{page}");

    // A temporary password, shown once, uncached; bob is signed out.
    let (status, _, _, page, cache) = post("/superuser/temp-password", &root, &[("email", " Bob@example.org ")]);
    assert_eq!((status, cache.as_str()), (200, "no-store"));
    let at = page.find(r#"data-testid="superuser-temp-password""#).expect("the password is on the page");
    let value = page[..at].rsplit("value=\"").next().unwrap();
    let temp = value.split('"').next().unwrap().to_string();
    assert!(temp.len() >= 12, "{temp}");
    assert_ne!(hash(), old_hash);
    assert!(crate::auth::verify_password(&temp, &hash()));
    assert!(flagged());
    assert_eq!(get("/profile", &bob).0, 303, "bob's old session ended");
    assert!(!get("/superuser", &root).2.contains(&temp), "shown only once");
    assert_eq!(get("/profile", &root).0, 200, "root keeps going");

    // Bob signs in with it: the gate, before anything else.
    let (status, bob, _, _, _) = post("/login", "", &[("email", "bob@example.org"), ("password", &temp)]);
    assert_eq!(status, 303);
    let (status, loc, _) = get("/profile", &bob);
    assert_eq!((status, loc.as_str()), (303, "/password/change?next=%2Fprofile"));
    assert_eq!(get("/api/progress", &bob).0, 403);
    let (status, _, page) = get(&loc, &bob);
    assert_eq!(status, 200);
    assert!(page.contains(r#"data-testid="password-change-form""#) && !page.contains(r#"name="current""#), "{page}");
    let change = |new: &str, confirm: &str| post("/password/change", &bob, &[("new", new), ("confirm", confirm), ("next", "/profile")]);
    let (status, _, _, page, _) = change("brand-new-pw", "brand-new-pX");
    assert!(status == 200 && page.contains("password-change-error"), "{page}");
    assert_eq!(change("short", "short").0, 200);
    assert!(flagged());
    let (status, _, loc, _, _) = change("brand-new-pw", "brand-new-pw");
    assert_eq!((status, loc.as_str()), (303, "/profile"));
    assert!(!flagged());
    assert!(crate::auth::verify_password("brand-new-pw", &hash()));
    assert_eq!(get("/profile", &bob).0, 200);
    assert_eq!(get("/password/change", &bob).1, "/", "nothing to change any more");

    // Two-factor off: the method, the app's key and a waiting sign-in go.
    state
        .db
        .with(|c| {
            c.execute("UPDATE users SET two_factor = 'totp' WHERE id = ?1", [bob_id])?;
            c.execute("INSERT INTO totp_keys (user_id, secret, created_at, confirmed_at) VALUES (?1, x'00', 1, 1)", [bob_id])?;
            crate::two_factor::start_login(c, bob_id, crate::db::now()).map(|_| ())
        })
        .unwrap();
    let (status, _, loc, _, _) = post("/superuser/two-factor-off", &root, &[("email", "bob@example.org")]);
    assert_eq!((status, loc.as_str()), (303, "/superuser?did=two-factor-off&email=bob%40example.org"));
    let left: (String, i64, i64) = state
        .db
        .with(|c| {
            c.query_row(
                "SELECT two_factor, (SELECT COUNT(*) FROM totp_keys WHERE user_id = ?1), (SELECT COUNT(*) FROM two_factor_challenges WHERE user_id = ?1) FROM users WHERE id = ?1",
                [bob_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
        })
        .unwrap();
    assert_eq!(left, (String::new(), 0, 0));
    assert!(get(&loc, &root).2.contains("Two-factor sign-in is off for bob@example.org"));

    // An address nobody has.
    let (_, _, loc, _, _) = post("/superuser/temp-password", &root, &[("email", "nobody@example.org")]);
    assert!(get(&loc, &root).2.contains(r#"data-testid="superuser-error""#), "{loc}");
}

/// A server on `SKRONGLE_SIGNUP` (#634), its router served on a free
/// port, with "Continue with Google" pointed at the stand-in.
fn signup_server(mode: crate::signup::Mode) -> (tokio::runtime::Runtime, std::sync::Arc<crate::AppState>, String) {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let listener = rt.block_on(tokio::net::TcpListener::bind("127.0.0.1:0")).unwrap();
    let addr = listener.local_addr().unwrap();
    let mut s = std::sync::Arc::into_inner(super::test_state(false, None)).unwrap();
    s.signup = mode;
    s.google = Some(crate::dev::mock_google(addr));
    let state = std::sync::Arc::new(s);
    let app = super::router(state.clone());
    rt.spawn(async move { axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap() });
    (rt, state, format!("http://{addr}"))
}

/// `SKRONGLE_SIGNUP=invite`: the first account is the owner's, made
/// only through the owner link from the log; after it `/register` is
/// closed except through a link the owner makes on the Superuser page, which
/// works once, for 7 days, until revoked.
#[test]
fn signup_by_invitation() {
    let (_rt, state, base) = signup_server(crate::signup::Mode::Invite);
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    let get = |path: &str, cookie: &str| {
        let mut r = agent.get(&format!("{base}{path}")).header("cookie", cookie).call().unwrap();
        (r.status().as_u16(), r.body_mut().read_to_string().unwrap())
    };
    let post = |path: &str, cookie: &str, form: &[(&str, &str)]| {
        let mut r = agent.post(&format!("{base}{path}")).header("cookie", cookie).send_form(form.iter().copied()).unwrap();
        let set = r.headers().get("set-cookie").map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string()).unwrap_or_default();
        (r.status().as_u16(), set, r.body_mut().read_to_string().unwrap())
    };
    let register = |email: &str, invite: &str| post("/register", "", &[("email", email), ("password", "long-enough-pw"), ("age", "on"), ("accept", "on"), ("invite", invite)]);
    let users = || state.db.with(|c| c.query_row("SELECT COUNT(*) FROM users", [], |r| r.get::<_, i64>(0))).unwrap();
    let superuser = |email: &'static str| state.db.with(|c| c.query_row("SELECT superuser FROM users WHERE email = ?1", [email], |r| r.get::<_, bool>(0))).unwrap();

    // An empty server: without the owner link, nobody gets in first.
    let (status, page) = get("/register", "");
    assert_eq!(status, 200);
    assert!(page.contains("register-closed") && page.contains("no owner yet") && !page.contains("register-form"), "{page}");
    assert_eq!(register("eve@example.org", "").0, 403);
    let owner_link = state.db.with(|c| crate::signup::owner_link(c, crate::db::now())).unwrap().unwrap();
    assert!(get("/register?invite=0123abcd", "").1.contains("no owner yet"));
    assert_eq!(users(), 0);
    // With it: the form, saying the account is the owner's.
    let (status, page) = get(&format!("/register?invite={owner_link}"), "");
    assert_eq!(status, 200);
    assert!(page.contains("register-owner-note") && page.contains(&format!(r#"name="invite" value="{owner_link}""#)), "{page}");
    // A form error keeps the line (#665).
    let (status, _, page) = post("/register", "", &[("email", "owner@example.org"), ("password", "short"), ("age", "on"), ("accept", "on"), ("invite", &owner_link)]);
    assert_eq!(status, 200);
    assert!(page.contains("register-error") && page.contains("register-owner-note"), "{page}");
    // Registering signs nobody in (#379): "check your mail", then /login.
    assert_eq!(register("owner@example.org", &owner_link).0, 200);
    let (status, owner, _) = post("/login", "", &[("email", "owner@example.org"), ("password", "long-enough-pw")]);
    assert_eq!(status, 303);
    assert!(superuser("owner@example.org"), "the first account is a superuser");
    assert_eq!(register("eve@example.org", &owner_link).0, 403, "the owner link works once");

    // Now closed without a link: a short page, and posting makes nothing.
    let (status, page) = get("/register", "");
    assert_eq!(status, 200);
    assert!(page.contains("register-closed") && !page.contains("register-form"), "{page}");
    assert!(page.contains("This server is not taking new accounts"), "{page}");
    assert!(!page.contains("owner@example.org"), "no address on the closed page");
    assert_eq!(register("eve@example.org", "").0, 403);
    assert_eq!(register("eve@example.org", "not-a-token").0, 403);
    assert_eq!(users(), 1);

    // The owner's Superuser page: the list and the button.
    let (_, page) = get("/superuser", &owner);
    assert!(page.contains("superuser-accounts") && page.contains("superuser-invite-form") && page.contains(r#"data-mode="invite""#), "{page}");
    assert!(page.contains("<td>owner@example.org</td>"));

    // A link, shown once, used once.
    let make = || {
        let (status, _, page) = post("/superuser/invites", &owner, &[]);
        assert_eq!(status, 200);
        // superuser.js sets the address bar back to this, so a reload does not post again (#665).
        assert!(page.contains(r#"data-page-url="/superuser""#), "{page}");
        let at = page.find("/register?invite=").expect("the new link is on the page") + "/register?invite=".len();
        page[at..at + 64].to_string()
    };
    let token = make();
    assert!(token.bytes().all(|b| b.is_ascii_hexdigit()));
    assert!(!get("/superuser", &owner).1.contains(&token), "the link is shown only when made");
    let (_, page) = get(&format!("/register?invite={token}"), "");
    assert!(page.contains(&format!(r#"name="invite" value="{token}""#)) && !page.contains("register-owner-note"), "{page}");
    assert!(page.contains(&format!("/auth/google?invite={token}")), "Google sign-up carries the link");
    assert_eq!(register("ann@example.org", &token).0, 200);
    let (status, ann, _) = post("/login", "", &[("email", "ann@example.org"), ("password", "long-enough-pw")]);
    assert_eq!(status, 303);
    assert!(!superuser("ann@example.org"));
    let (_, page) = get(&format!("/register?invite={token}"), "");
    assert!(page.contains("already been used"), "{page}");
    assert_eq!(register("eve@example.org", &token).0, 403);
    assert_eq!(users(), 2);

    // Expired.
    let late = make();
    state.db.with(|c| c.execute("UPDATE signup_invites SET expires_at = 1 WHERE used_at IS NULL", [])).unwrap();
    assert!(get(&format!("/register?invite={late}"), "").1.contains("has expired"));
    assert_eq!(register("eve@example.org", &late).0, 403);

    // Revoked: listed, then gone.
    let gone = make();
    let (_, page) = get("/superuser", &owner);
    assert_eq!(page.matches("superuser-invite-row").count(), 1, "only the open link is listed");
    let id: i64 = state.db.with(|c| c.query_row("SELECT id FROM signup_invites WHERE used_at IS NULL AND expires_at > 1", [], |r| r.get(0))).unwrap();
    // Not a superuser: 404 on both forms and on the page.
    assert_eq!(post("/superuser/invites", &ann, &[]).0, 404);
    assert_eq!(post("/superuser/invites/revoke", &ann, &[("id", &id.to_string())]).0, 404);
    assert_eq!(get("/superuser", &ann).0, 404);
    let (status, _, _) = post("/superuser/invites/revoke", &owner, &[("id", &id.to_string())]);
    assert_eq!(status, 303);
    assert!(get(&format!("/register?invite={gone}"), "").1.contains("is not valid"));
    assert_eq!(users(), 2);
}

/// `SKRONGLE_SIGNUP=closed`: the first account is still made, through
/// the owner link, and is the owner's; then nobody registers, not with
/// a link either, nor with Google, while existing accounts still sign
/// in with Google.
#[test]
fn closed_signup_lets_only_the_owner_in() {
    let (_rt, state, base) = signup_server(crate::signup::Mode::Closed);
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    let get = |path: &str| agent.get(&format!("{base}{path}")).call().unwrap().body_mut().read_to_string().unwrap();
    let register = |email: &str, invite: &str| {
        let form = [("email", email), ("password", "long-enough-pw"), ("age", "on"), ("accept", "on"), ("invite", invite)];
        agent.post(&format!("{base}/register")).send_form(form).unwrap().status().as_u16()
    };
    let confirm_with = |token: &str, invite: &str| {
        let cookie = format!("signup={token}; signup_invite={invite}");
        let mut r = agent.post(&format!("{base}/auth/google/confirm")).header("cookie", &cookie).send_form([("age", "on"), ("accept", "on")]).unwrap();
        (r.status().as_u16(), r.body_mut().read_to_string().unwrap())
    };
    let confirm = |token: &str| confirm_with(token, "");
    let users = || state.db.with(|c| c.query_row("SELECT COUNT(*) FROM users", [], |r| r.get::<_, i64>(0))).unwrap();

    // The first account comes in with Google here, through the owner
    // link (carried by the Google button), and is the owner's.
    let owner_link = state.db.with(|c| crate::signup::owner_link(c, crate::db::now())).unwrap().unwrap();
    assert!(get("/register").contains("no owner yet"));
    let page = get(&format!("/register?invite={owner_link}"));
    assert!(page.contains("register-owner-note") && page.contains(&format!("/auth/google?invite={owner_link}")), "{page}");
    let (sub, _) = crate::dev::mock_google_account("owner@example.org");
    let identity = |sub: &str| crate::oauth::Identity { sub: sub.into(), email: "owner@example.org".into(), name: None };
    let t = state.signups.insert(identity(&sub), crate::db::now());
    assert_eq!(confirm(&t).0, 403, "not without the owner link");
    assert_eq!(users(), 0);
    let t = state.signups.insert(identity(&sub), crate::db::now());
    assert_eq!(confirm_with(&t, &owner_link).0, 303);
    let su: bool = state.db.with(|c| c.query_row("SELECT superuser FROM users WHERE google_sub = ?1", [&sub], |r| r.get(0))).unwrap();
    assert!(su, "the first account, made with Google, is the owner");

    // Then: closed, with or without a link, by password or by Google.
    assert!(get("/register").contains("register-closed"));
    assert!(get("/register?invite=abc123").contains("This server is not taking new accounts"));
    assert_eq!(register("eve@example.org", ""), 403);
    let t = state.signups.insert(crate::oauth::Identity { sub: "g-eve".into(), email: "eve@example.org".into(), name: None }, crate::db::now());
    let (status, page) = confirm(&t);
    assert_eq!(status, 403);
    assert!(page.contains("register-closed"), "{page}");
    assert_eq!(users(), 1);

    // The owner's Google account still signs in.
    let t = state.signups.insert(crate::oauth::Identity { sub, email: "owner@example.org".into(), name: None }, crate::db::now());
    assert_eq!(confirm(&t).0, 303);
    assert_eq!(users(), 1);
    // No invitations in closed mode.
    let owner_cookie = state.db.with(|c| {
        let id: i64 = c.query_row("SELECT id FROM users", [], |r| r.get(0))?;
        crate::auth::create_session(c, id)
    });
    let owner_cookie = format!("sid={}", owner_cookie.unwrap());
    let r = agent.post(&format!("{base}/superuser/invites")).header("cookie", &owner_cookie).send_empty().unwrap();
    assert_eq!(r.status().as_u16(), 404);
}

/// With the setting unset (`open`), nothing changes: anyone registers,
/// and the first account is an ordinary one.
#[test]
fn open_signup_is_unchanged() {
    let (_rt, state, base) = signup_server(crate::signup::Mode::Open);
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    let page = agent.get(&format!("{base}/register")).call().unwrap().body_mut().read_to_string().unwrap();
    assert!(page.contains("register-form") && !page.contains("register-owner-note"));
    for email in ["a@example.org", "b@example.org"] {
        let form = [("email", email), ("password", "long-enough-pw"), ("age", "on"), ("accept", "on")];
        // 200: "check your mail" (#379; this server has mail).
        assert_eq!(agent.post(&format!("{base}/register")).send_form(form).unwrap().status().as_u16(), 200);
    }
    let su: i64 = state.db.with(|c| c.query_row("SELECT COUNT(*) FROM users WHERE superuser = 1", [], |r| r.get(0))).unwrap();
    assert_eq!(su, 0);
}

/// A server whose proxy sign-in (#642) trusts `from` (`None`: off),
/// on `SKRONGLE_SIGNUP=mode`, served on a free loopback port.
fn proxy_server(mode: crate::signup::Mode, from: Option<&str>) -> (tokio::runtime::Runtime, std::sync::Arc<crate::AppState>, String) {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let listener = rt.block_on(tokio::net::TcpListener::bind("127.0.0.1:0")).unwrap();
    let addr = listener.local_addr().unwrap();
    let mut s = std::sync::Arc::into_inner(super::test_state(false, None)).unwrap();
    s.signup = mode;
    s.proxy_auth = from.map(|f| crate::proxy_auth::Config::parse(Some("Remote-Email"), Some(f)).unwrap().unwrap());
    let state = std::sync::Arc::new(s);
    let app = super::router(state.clone());
    rt.spawn(async move { axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap() });
    (rt, state, format!("http://{addr}"))
}

/// A request through the proxy: (status, Location, the `sid` it sets, body).
fn proxied(base: &str, method: &str, path: &str, cookie: &str, user: Option<&str>) -> (u16, String, String, String) {
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    let url = format!("{base}{path}");
    let mut r = if method == "POST" {
        let mut req = agent.post(&url).header("cookie", cookie);
        if let Some(u) = user {
            req = req.header("remote-email", u);
        }
        req.send_empty().unwrap()
    } else {
        let mut req = agent.get(&url).header("cookie", cookie);
        if let Some(u) = user {
            req = req.header("remote-email", u);
        }
        req.call().unwrap()
    };
    let header = |name: &str| r.headers().get(name).map(|v| v.to_str().unwrap().to_string()).unwrap_or_default();
    let location = header("location");
    let sid = r
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .filter_map(|v| v.split(';').next())
        .find(|c| c.starts_with("sid=") && c.len() > 4)
        .unwrap_or_default()
        .to_string();
    (r.status().as_u16(), location, sid, r.body_mut().read_to_string().unwrap())
}

fn sessions_of(state: &crate::AppState, id: i64) -> i64 {
    state.db.with(|c| c.query_row("SELECT COUNT(*) FROM sessions WHERE user_id = ?1", [id], |r| r.get(0))).unwrap()
}

fn member(state: &crate::AppState, email: &str) -> i64 {
    state
        .db
        .with(|c| {
            let id = crate::auth::create_user(c, email, "h")?;
            crate::terms::accept(c, id, crate::terms::version())?;
            Ok(id)
        })
        .unwrap()
}

/// #642: from a trusted address the header signs a known account in
/// (any case), once; a session of another account is replaced; the
/// sign-in pages only say the proxy does it; logout says it signed out.
#[test]
fn proxy_header_signs_in_a_known_account() {
    let (_rt, state, base) = proxy_server(crate::signup::Mode::Invite, Some("10.0.0.0/8, 127.0.0.1"));
    let ann = member(&state, "ann@example.org");
    let bob = member(&state, "bob@example.org");

    let (status, _, sid, page) = proxied(&base, "GET", "/profile", "", Some("ANN@Example.org"));
    assert_eq!(status, 200, "{page}");
    assert!(page.contains("ann@example.org"), "the profile is Ann's");
    assert!(!sid.is_empty(), "a session cookie is set");
    assert_eq!(sessions_of(&state, ann), 1);
    let ann_sid = sid;

    // With its session, nothing new is made.
    let (status, _, sid, _) = proxied(&base, "GET", "/profile", &ann_sid, Some("ann@example.org"));
    assert_eq!((status, sid.as_str()), (200, ""));
    assert_eq!(sessions_of(&state, ann), 1);

    // Bob's session, but the proxy says Ann: Bob's session ends, Ann's begins.
    let bob_sid = format!("sid={}", state.db.with(|c| crate::auth::create_session(c, bob)).unwrap());
    let (status, _, sid, page) = proxied(&base, "GET", "/profile", &format!("lang=en; {bob_sid}"), Some("ann@example.org"));
    assert_eq!(status, 200);
    assert!(page.contains("ann@example.org") && !page.contains("bob@example.org"), "{page}");
    assert!(!sid.is_empty());
    assert_eq!(sessions_of(&state, bob), 0, "Bob's session is ended");
    assert_eq!(sessions_of(&state, ann), 2);

    // No two-factor step, no rate limit: many sign-ins in a row all work.
    for _ in 0..40 {
        assert_eq!(proxied(&base, "GET", "/profile", "", Some("bob@example.org")).0, 200);
    }

    // /login and /register say the proxy signs in; posting them does nothing.
    for path in ["/login", "/register"] {
        let (status, _, _, page) = proxied(&base, "GET", path, "", None);
        assert_eq!(status, 200);
        assert!(page.contains("proxy-signin-note") && !page.contains("login-form") && !page.contains("register-form"), "{page}");
        assert_eq!(proxied(&base, "POST", path, "", None).0, 403);
    }
    // Logout ends the session and says so; it does not sign back in.
    let (status, _, sid, page) = proxied(&base, "POST", "/logout", &ann_sid, Some("ann@example.org"));
    assert_eq!(status, 200);
    assert!(page.contains("proxy-signin-signed-out"), "{page}");
    assert!(sid.is_empty(), "no new session on logout");
    assert_eq!(sessions_of(&state, ann), 1);
}

/// #642: the header from an address not in the list is ignored, and with
/// the settings unset it means nothing at all.
#[test]
fn proxy_header_from_elsewhere_is_ignored() {
    for from in [Some("10.1.2.3"), None] {
        let (_rt, state, base) = proxy_server(crate::signup::Mode::Open, from);
        let ann = member(&state, "ann@example.org");
        let (status, location, sid, _) = proxied(&base, "GET", "/profile", "", Some("ann@example.org"));
        assert_eq!(status, 303, "{from:?}");
        assert!(location.ends_with("/login"), "{location}");
        assert!(sid.is_empty());
        assert_eq!(sessions_of(&state, ann), 0);
        // An unknown address makes no account either, even in open mode.
        proxied(&base, "GET", "/", "", Some("new@example.org"));
        let users: i64 = state.db.with(|c| c.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))).unwrap();
        assert_eq!(users, 1);
        // On, /login only says the proxy signs in; off, it is the usual form.
        let want = if from.is_some() { "proxy-signin-note" } else { "login-form" };
        assert!(proxied(&base, "GET", "/login", "", None).3.contains(want));
    }
}

/// #642: an address with no account follows `SKRONGLE_SIGNUP`: refused
/// by invitation, made (no password, address confirmed, Terms still to
/// accept) when open.
#[test]
fn proxy_header_for_an_unknown_address_follows_the_signup_rule() {
    let (_rt, state, base) = proxy_server(crate::signup::Mode::Invite, Some("127.0.0.1"));
    member(&state, "owner@example.org");
    let (status, _, sid, page) = proxied(&base, "GET", "/", "", Some("new@example.org"));
    assert_eq!(status, 403);
    assert!(page.contains("register-closed"), "{page}");
    assert!(sid.is_empty());
    let users: i64 = state.db.with(|c| c.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))).unwrap();
    assert_eq!(users, 1);

    let (_rt, state, base) = proxy_server(crate::signup::Mode::Open, Some("127.0.0.1"));
    let (status, location, sid, _) = proxied(&base, "GET", "/profile", "", Some("New@Example.org"));
    assert_eq!(status, 303);
    assert!(location.contains("/terms/accept"), "the Terms gate first: {location}");
    assert!(!sid.is_empty());
    let (hash, verified): (String, bool) = state
        .db
        .with(|c| c.query_row("SELECT password_hash, email_verified_at IS NOT NULL FROM users WHERE email = 'new@example.org'", [], |r| Ok((r.get(0)?, r.get(1)?))))
        .unwrap();
    assert_eq!(hash, "", "no password, like a Google-only account");
    assert!(verified, "the proxy vouched for the address");
    // Not an address: nobody is made.
    proxied(&base, "GET", "/", "", Some("just-a-name"));
    let users: i64 = state.db.with(|c| c.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))).unwrap();
    assert_eq!(users, 1);
}

/// #623: everyone's Skrongle AI spend today shows on `/superuser` only; a
/// learner gets a 404 there and never sees it on the profile (their own
/// month is a bar without dollars); without Skrongle AI there is no card.
#[test]
fn only_superusers_see_the_days_ai_spend() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    for hosted in [true, false] {
        let state = super::test_state_with(false, None, hosted.then(|| ("sk-ant-server".to_string(), crate::metering::MeterConfig::default())));
        let app = super::router(state.clone());
        let base = rt.block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move { axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap() });
            format!("http://{addr}")
        });
        let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
        let get = |path: &str, cookie: &str| {
            let mut r = agent.get(&format!("{base}{path}")).header("cookie", cookie).call().unwrap();
            (r.status().as_u16(), r.body_mut().read_to_string().unwrap())
        };
        // Registering signs nobody in (#379): the session comes from /login.
        let register = |email: &str| {
            agent.post(&format!("{base}/register")).send_form([("email", email), ("password", "long-enough-pw"), ("age", "on"), ("accept", "on")]).unwrap();
            let r = agent.post(&format!("{base}/login")).send_form([("email", email), ("password", "long-enough-pw")]).unwrap();
            r.headers().get("set-cookie").unwrap().to_str().unwrap().split(';').next().unwrap().to_string()
        };
        let (root, bob) = (register("root@example.org"), register("bob@example.org"));
        state
            .db
            .with(|c| {
                c.execute("UPDATE users SET superuser = 1 WHERE email = 'root@example.org'", [])?;
                // Bob's hosted call today and yesterday's, which does not count.
                for at in [crate::db::now(), crate::metering::day_start(crate::db::now()) - 1] {
                    c.execute(
                        "INSERT INTO ai_usage (user_id, purpose, provider, model, cost_micros, key_source, created_at)
                             SELECT id, 'tutor-turn', 'anthropic', 'm', 3200000, 'hosted', ?1 FROM users WHERE email = 'bob@example.org'",
                        [at],
                    )?;
                }
                Ok(())
            })
            .unwrap();
        let (status, page) = get("/superuser", &root);
        assert_eq!(status, 200);
        if hosted {
            assert!(page.contains(r#"data-testid="superuser-ai-today" data-level="ok""#) && page.contains(r#"data-testid="superuser-ai-today-bar" data-used="32""#), "{page}");
            assert!(page.contains("$3.20 of $10.00 · resets at midnight UTC"), "{page}");
        } else {
            assert!(page.contains("superuser-accounts") && !page.contains("superuser-ai-today"), "no Skrongle AI, no card: {page}");
        }
        assert_eq!(get("/superuser", &bob).0, 404, "a learner has no such page");
        let (_, page) = get("/profile", &bob);
        assert!(!page.contains("ai-today") && !page.contains("$3.20") && !page.contains("$10.00"), "a learner never sees it: {page}");
    }
}

#[test]
fn email_verification_over_http() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let state = super::test_state(false, None);
    let app = super::router(state.clone());
    let base = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap() });
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
    let mails = |email: &str| -> Vec<String> {
        let e = email.to_string();
        state
            .db
            .with(move |c| {
                let mut stmt = c.prepare(
                    "SELECT o.body_text FROM mail_outbox o JOIN users u ON u.id = o.user_id WHERE u.email = ?1 AND o.purpose = 'email-verify' ORDER BY o.id",
                )?;
                let v = stmt.query_map([e], |r| r.get(0))?.collect::<Result<Vec<String>, _>>();
                v
            })
            .unwrap()
    };
    let token_in = |body: &str| body.split(&format!("{base}/verify-email?token=")).nth(1).unwrap().split_whitespace().next().unwrap().to_string();
    let verified = |email: &str| {
        let e = email.to_string();
        state.db.with(move |c| c.query_row("SELECT email_verified_at IS NOT NULL FROM users WHERE email = ?1", [e], |r| r.get::<_, bool>(0))).unwrap()
    };

    // Registering: one message with a link to this server.
    assert_eq!(post("/register", "", &[("email", "ann@example.org"), ("password", "long-enough-pw"), ("age", "on"), ("accept", "on")]).0, 200);
    let (status, cookie, _) = post("/login", "", &[("email", "ann@example.org"), ("password", "long-enough-pw")]);
    assert_eq!(status, 303);
    let sent = mails("ann@example.org");
    assert_eq!(sent.len(), 1);
    let token = token_in(&sent[0]);
    assert_eq!(token.len(), 64);
    assert!(!verified("ann@example.org"));
    assert_eq!(post("/courses/open/spanish", &cookie, &[]).0, 303);
    let profile = get("/profile", &cookie).1;
    assert!(profile.contains(r#"data-testid="email-verify-resend""#) && !profile.contains(r#"data-testid="email-verified""#));

    // A wrong token changes nothing; the right one works once, signed out too.
    let (status, html) = get("/verify-email?token=0123", "");
    assert_eq!(status, 200);
    assert!(html.contains("verify-email-invalid"));
    assert!(!verified("ann@example.org"));
    let (status, html) = get(&format!("/verify-email?token={token}"), "");
    assert_eq!(status, 200);
    assert!(html.contains("verify-email-confirmed"), "{html}");
    assert!(verified("ann@example.org"));
    assert!(get(&format!("/verify-email?token={token}"), "").1.contains("verify-email-invalid"), "single use");
    assert!(get("/profile", &cookie).1.contains(r#"data-testid="email-verified""#));
    // Confirmed: "send again" sends nothing.
    assert_eq!(post("/profile/verify-email/resend", &cookie, &[]).2, "/profile?saved=verify-sent");
    assert_eq!(mails("ann@example.org").len(), 1);

    // Send again: a new link each time, the older one void, then silence.
    post("/register", "", &[("email", "bob@example.org"), ("password", "long-enough-pw"), ("age", "on"), ("accept", "on")]);
    let (_, bob, _) = post("/login", "", &[("email", "bob@example.org"), ("password", "long-enough-pw")]);
    let first = token_in(&mails("bob@example.org")[0]);
    assert_eq!(post("/profile/verify-email/resend", &bob, &[]).2, "/profile?saved=verify-sent");
    assert!(get("/profile?saved=verify-sent", &bob).1.contains("Sent to bob@example.org."));
    for _ in 1..crate::security::VERIFY_RESEND_PER_USER.max {
        assert_eq!(post("/profile/verify-email/resend", &bob, &[]).0, 303);
    }
    let sent = mails("bob@example.org");
    assert_eq!(sent.len(), 1 + crate::security::VERIFY_RESEND_PER_USER.max as usize);
    assert_eq!(post("/profile/verify-email/resend", &bob, &[]).2, "/profile?saved=verify-sent", "throttled silently");
    assert_eq!(mails("bob@example.org").len(), sent.len());
    assert!(get(&format!("/verify-email?token={first}"), "").1.contains("verify-email-invalid"), "superseded");
    let html = get(&format!("/verify-email?token={first}"), &bob).1;
    assert!(html.contains(r#"data-testid="verify-email-resend""#), "signed in: a new link is offered");
    assert!(get(&format!("/verify-email?token={}", token_in(sent.last().unwrap())), &bob).1.contains("verify-email-confirmed"));
    assert!(verified("bob@example.org"));

}

/// A theme a learner made over HTTP: only they can wear it, see its
/// pictures and delete it.
#[test]
fn made_themes_are_private_over_http() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let state = super::test_state(false, None);
    let (ann, bob) = state
        .db
        .with(|c| Ok((crate::auth::create_user(c, "ann@example.org", "x")?, crate::auth::create_user(c, "bob@example.org", "x")?)))
        .unwrap();
    let files = vec![
        ("theme.json".to_string(), r##"{"id": "x", "title": "Night", "colors": {"accent": "#334455"}, "art": {"mark": "pictures/m.svg"}}"##.to_string()),
        ("pictures/m.svg".to_string(), "<svg xmlns='http://www.w3.org/2000/svg'/>".to_string()),
    ];
    let id = rt.block_on(state.themes.create(&state.db, ann, files)).unwrap();
    for u in [ann, bob] {
        state.db.with(|c| crate::courses::open_project(c, u, "spanish")).unwrap();
    }
    let cookie = |u| {
        state.db.with(|c| crate::terms::accept(c, u, crate::terms::version())).unwrap();
        format!("sid={}", state.db.with(|c| crate::auth::create_session(c, u)).unwrap())
    };
    let (ann_c, bob_c) = (cookie(ann), cookie(bob));
    let app = super::router(state.clone());
    let base = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    });
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    let get = |path: &str, c: &str| {
        let mut r = agent.get(&format!("{base}{path}")).header("cookie", c).call().unwrap();
        (r.status().as_u16(), r.body_mut().read_to_string().unwrap())
    };
    let post = |path: &str, c: &str, theme: &str| {
        agent.post(&format!("{base}{path}")).header("cookie", c).send_form([("theme", theme), ("next", "profile")]).unwrap().status().as_u16()
    };
    let wear = |c: &str, theme: &str| {
        agent.post(&format!("{base}/courses/spanish/settings")).header("cookie", c).send_form([("theme", theme)]).unwrap().status().as_u16()
    };
    let worn = |u: i64| state.db.with(|c| c.query_row("SELECT COALESCE(theme, '') FROM projects WHERE user_id = ?1", [u], |r| r.get::<_, String>(0))).unwrap();
    let picture = format!("/themes/{id}/pictures/m.svg");

    assert_eq!(get(&picture, &ann_c).0, 200);
    assert_eq!(get(&picture, &bob_c).0, 404);
    assert_eq!(get(&picture, "").0, 404);
    // Bob cannot wear it in a course; Ann can, and gets its colours.
    assert_eq!(wear(&bob_c, &id), 400);
    assert_eq!(worn(bob), "");
    assert_eq!(wear(&ann_c, &id), 303);
    assert_eq!(worn(ann), id);
    assert!(get("/theme.css", &ann_c).1.contains("#334455"));
    assert!(!get("/theme.css", &bob_c).1.contains("#334455"));
    // Offered in Ann's course settings only.
    assert!(get("/courses", &ann_c).1.contains(&format!(r#"data-theme-id="{id}""#)));
    assert!(!get("/courses", &bob_c).1.contains(&id));
    // Only Ann can delete it; then her course is back to Default and the pictures are gone.
    assert_eq!(post("/theme/delete", &bob_c, &id), 404);
    assert_eq!(post("/theme/delete", &ann_c, &id), 303);
    assert_eq!(worn(ann), "");
    assert_eq!(get(&picture, &ann_c).0, 404);
}

/// The profile picture is a course's setting (#565): each course shows
/// its own in the top-right corner, starting with the account's; the
/// course list (outside any course) shows a gear.
#[test]
fn each_course_has_its_own_picture() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let state = super::test_state(false, None);
    let ann = state.db.with(|c| crate::auth::create_user(c, "ann@example.org", "x")).unwrap();
    state.db.with(|c| crate::terms::accept(c, ann, crate::terms::version())).unwrap();
    let cookie = format!("sid={}", state.db.with(|c| crate::auth::create_session(c, ann)).unwrap());
    state.db.with(|c| c.execute("UPDATE users SET avatar = 'mochi', avatar_color = 3 WHERE id = ?1", [ann])).unwrap();
    let app = super::router(state.clone());
    let base = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    });
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    let get = |path: &str| agent.get(&format!("{base}{path}")).header("cookie", &cookie).call().unwrap().body_mut().read_to_string().unwrap();
    let set = |course: &str, avatar: &str, color: &str| {
        let form = [("theme", ""), ("avatar", avatar), ("avatar_color", color)];
        agent.post(&format!("{base}/courses/{course}/settings")).header("cookie", &cookie).send_form(form).unwrap().status().as_u16()
    };
    let open = |course: &str| agent.post(&format!("{base}/courses/open/{course}")).header("cookie", &cookie).send_empty().unwrap().status().as_u16();
    // The picture in the top-right corner of a page: the symbol id, or "blank".
    let badge = |path: &str| {
        let html = get(path);
        let head = html.split("data-testid=\"nav-account-toggle\"").nth(1).unwrap_or_default().split("</summary>").next().unwrap_or_default().to_string();
        if head.contains("avatar-blank") { "blank".to_string() } else { head.split("#av-").nth(1).unwrap_or_default().split('"').next().unwrap_or_default().to_string() }
    };

    // No course yet: a gear in the circle everywhere (#577).
    assert_eq!((badge("/profile"), badge("/courses")), ("blank".to_string(), "blank".to_string()));
    assert!(get("/courses").split("</summary>").next().unwrap_or_default().contains(r#"<circle cx="12" cy="12" r="3"/>"#), "the gear");
    // A course opened starts with the account's picture; the course list stays plain.
    assert_eq!(open("spanish"), 303);
    assert_eq!((badge("/"), badge("/profile"), badge("/courses")), ("mochi".to_string(), "mochi".to_string(), "blank".to_string()));
    assert!(!get("/profile").contains("profile-avatar-edit"), "the profile does not edit the picture");
    // Changed in the course's settings: that course only.
    assert_eq!(set("spanish", "kai", "4"), 303);
    assert_eq!(badge("/"), "kai");
    assert!(get("/").contains("--av:#3b8ea5"), "its background too");
    assert_eq!(open("french"), 303);
    assert_eq!(badge("/"), "mochi");
    assert_eq!(open("spanish"), 303);
    assert_eq!(badge("/"), "kai");
    // The settings dialog has it checked.
    assert!(get("/courses").contains(r#"data-testid="courses-settings-avatar" value="kai" checked"#));
    // A picture or background that is not one: 400, nothing changes.
    assert_eq!(set("spanish", "dragon", "4"), 400);
    assert_eq!(set("spanish", "kai", "0"), 400);
    assert_eq!(set("spanish", "kai", "17"), 400);
    assert_eq!(badge("/"), "kai");
}

/// A course wears a theme of its own (#565): every page while it is the
/// active course, except the course picker, which wears the profile's.
#[test]
fn a_course_wears_its_own_theme() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let state = super::test_state(false, None);
    let (ann, bob) = state
        .db
        .with(|c| Ok((crate::auth::create_user(c, "ann@example.org", "x")?, crate::auth::create_user(c, "bob@example.org", "x")?)))
        .unwrap();
    let theme = |title: &str, accent: &str| {
        let json = format!(r##"{{"id": "x", "title": "{title}", "colors": {{"accent": "{accent}"}}}}"##);
        rt.block_on(state.themes.create(&state.db, ann, vec![("theme.json".to_string(), json)])).unwrap()
    };
    let (night, day) = (theme("Night", "#334455"), theme("Day", "#ddeeff"));
    let cookie = |u| {
        state.db.with(|c| crate::terms::accept(c, u, crate::terms::version())).unwrap();
        format!("sid={}", state.db.with(|c| crate::auth::create_session(c, u)).unwrap())
    };
    let (ann_c, bob_c) = (cookie(ann), cookie(bob));
    state.db.with(|c| crate::courses::open_project(c, ann, "spanish")).unwrap();
    let app = super::router(state.clone());
    let base = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    });
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    let get = |path: &str, c: &str| {
        let mut r = agent.get(&format!("{base}{path}")).header("cookie", c).call().unwrap();
        (r.status().as_u16(), r.body_mut().read_to_string().unwrap())
    };
    let set = |c: &str, course: &str, theme: &str| {
        let form = [("theme", theme), ("avatar", "noor"), ("avatar_color", "1")];
        agent.post(&format!("{base}/courses/{course}/settings")).header("cookie", c).send_form(form).unwrap().status().as_u16()
    };
    // The theme a page wears: its <html data-theme>, '' for Default.
    let worn = |path: &str| {
        let html = get(path, &ann_c).1;
        html.split_once("data-theme=\"").filter(|(head, _)| !head.contains("<body")).map(|(_, t)| t.split('"').next().unwrap_or_default().to_string()).unwrap_or_default()
    };

    // Until the course has a theme, every page is Default; the profile has no theme to pick (#577).
    assert_eq!((worn("/"), worn("/courses")), (String::new(), String::new()));
    assert!(get("/courses", &ann_c).1.contains(r#"data-testid="courses-settings""#), "the gear on the opened course's card");
    assert!(!get("/profile", &ann_c).1.contains(r#"data-testid="profile-theme""#));
    // The course's own: worn in it; not on the course list or the profile (#951).
    assert_eq!(set(&ann_c, "spanish", &day), 303);
    assert_eq!((worn("/"), worn("/lessons"), worn("/profile"), worn("/courses")), (day.clone(), day.clone(), String::new(), String::new()));
    assert!(get(&format!("/theme.css?t={night}"), &ann_c).1.contains("#334455"));
    assert!(get(&format!("/theme.css?t={day}"), &ann_c).1.contains("#ddeeff"));
    assert!(!get(&format!("/theme.css?t={day}"), &bob_c).1.contains("#ddeeff"), "only its owner gets a made theme's colours");
    // On the course list the card wears it, from the cards' stylesheet (its owner's themes only).
    assert!(get("/courses", &ann_c).1.contains(&format!(r#"data-course-id="spanish" data-course-status="ready" data-active="true" data-card-theme="{day}""#)));
    let cards = get("/courses/themes.css", &ann_c).1;
    assert!(cards.contains(&format!(r#":is(.course-card, dialog.course-settings)[data-card-theme="{day}"]{{--accent:#ddeeff;}}"#)), "{cards}");
    assert!(!get("/courses/themes.css", &bob_c).1.contains(&day));
    // Default in the course: the card wears what the list wears.
    assert_eq!(set(&ann_c, "spanish", ""), 303);
    assert_eq!((worn("/"), worn("/courses")), (String::new(), String::new()));
    assert!(!get("/courses", &ann_c).1.contains("data-card-theme="), "Default: the card wears no theme of its own");
    // "Same as profile" is gone; someone else's theme or one that is not one: 400; a course never opened: 404.
    assert_eq!(set(&ann_c, "spanish", "profile"), 400);
    state.db.with(|c| crate::courses::open_project(c, bob, "spanish")).unwrap();
    assert_eq!(set(&bob_c, "spanish", &day), 400);
    assert_eq!(set(&ann_c, "french", &day), 404);
    assert_eq!(set(&ann_c, "spanish", "nope"), 400);
    // Deleting the theme a course wears puts Default back on it.
    assert_eq!(set(&ann_c, "spanish", &night), 303);
    assert_eq!(worn("/"), night);
    let del = agent.post(&format!("{base}/theme/delete")).header("cookie", &ann_c).send_form([("theme", night.as_str())]).unwrap();
    assert_eq!(del.status().as_u16(), 303);
    assert_eq!(worn("/"), "");
}

/// A course's first visit (#574): its home shows the settings as a
/// wizard until the learner saves them, even saving nothing (the wizard
/// closed); the motivator chosen there is worn in that course only, and
/// the profile no longer offers one.
#[test]
fn a_new_course_asks_for_its_settings_once() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let state = super::test_state(false, None);
    let ann = state.db.with(|c| crate::auth::create_user(c, "ann@example.org", "x")).unwrap();
    state.db.with(|c| crate::terms::accept(c, ann, crate::terms::version())).unwrap();
    let cookie = format!("sid={}", state.db.with(|c| crate::auth::create_session(c, ann)).unwrap());
    let app = super::router(state.clone());
    let base = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    });
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    let home = || agent.get(&format!("{base}/")).header("cookie", &cookie).call().unwrap().body_mut().read_to_string().unwrap();
    let post = |path: &str, form: &[(&str, &str)]| {
        let r = agent.post(&format!("{base}{path}")).header("cookie", &cookie).send_form(form.iter().copied()).unwrap();
        (r.status().as_u16(), r.headers().get("location").map(|v| v.to_str().unwrap().to_string()).unwrap_or_default())
    };

    // Opening a course for the first time: the home page holds the wizard.
    assert_eq!(post("/courses/open/spanish", &[]).0, 303);
    let page = home();
    assert!(page.contains("data-wizard data-open-on-load") && page.contains("course-settings.js"), "the wizard on the first visit");
    assert!(page.contains(r#"data-testid="courses-settings-motivator" data-motivator-id="stats""#));
    // Get Started!: the settings are saved and the home page comes back without it.
    let started = [("motivator", "skc"), ("theme", ""), ("avatar", "noor"), ("avatar_color", "2"), ("next", "home")];
    assert_eq!(post("/courses/spanish/settings", &started), (303, "/".to_string()));
    let page = home();
    assert!(!page.contains("data-wizard") && page.contains(r#"data-motivator="skc""#), "the course wears SKC, the wizard is gone");
    // The gear on the course list is the plain form with the choice made.
    let list = agent.get(&format!("{base}/courses")).header("cookie", &cookie).call().unwrap().body_mut().read_to_string().unwrap();
    assert!(list.contains(r#"data-testid="courses-settings-about""#) && !list.contains(r#"data-course-id="spanish" data-wizard"#));
    assert!(list.contains(r#"<input type="radio" name="motivator" value="skc" class="sr-only" checked>"#));
    // Another course: its own wizard, closed without a choice; the default motivator.
    assert_eq!(post("/courses/open/french", &[]).0, 303);
    assert!(home().contains("data-wizard"));
    assert_eq!(post("/courses/french/settings", &[("next", "home")]), (303, "/".to_string()));
    let page = home();
    assert!(!page.contains("data-wizard") && !page.contains(r#"data-motivator="skc""#), "French keeps its own motivator");
    // A motivator that is not one: 400.
    assert_eq!(post("/courses/french/settings", &[("motivator", "nope")]).0, 400);
}

/// The wizard behind Start (#587): a course not opened yet carries its
/// settings wizard on the course list, the level first for a course
/// with levels; "Get Started!" opens the course with them, set up. The
/// level then changes in the gear's form only; there is no `/start`.
#[test]
fn start_opens_the_course_with_its_wizard_settings() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let state = super::test_state(false, None);
    let ann = state.db.with(|c| crate::auth::create_user(c, "ann@example.org", "x")).unwrap();
    state.db.with(|c| crate::terms::accept(c, ann, crate::terms::version())).unwrap();
    let cookie = format!("sid={}", state.db.with(|c| crate::auth::create_session(c, ann)).unwrap());
    let app = super::router(state.clone());
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
    let project = || state.db.with(|c| crate::courses::project(c, ann, "spanish")).unwrap();

    // On the course list, Spanish (not opened) has its wizard, levels first.
    let list = get("/courses").1;
    assert!(list.contains("data-wizard data-start") && !list.contains("data-open-on-load"));
    assert!(list.contains(r#"action="/courses/open/spanish""#) && list.contains(r#"name="start_unit" value="A2""#));
    // A level that is not one: 400, and nothing is opened.
    assert_eq!(post("/courses/open/spanish", &[("wizard", "1"), ("start_unit", "Z9")]), 400);
    assert!(project().is_none());
    // Get Started! at A2: open, set up, at A2, wearing SKC.
    let started = [("wizard", "1"), ("start_unit", "A2"), ("motivator", "skc"), ("theme", ""), ("avatar", "noor"), ("avatar_color", "2")];
    assert_eq!(post("/courses/open/spanish", &started), 303);
    let pr = project().unwrap();
    assert!(pr.set_up && pr.start_unit == "A2" && pr.motivator == "skc");
    let home = get("/").1;
    assert!(!home.contains("data-wizard") && home.contains(r#"data-motivator="skc""#));
    // The gear's form shows A2 chosen; saving it untouched keeps A2, another level moves.
    let list = get("/courses").1;
    assert!(list.contains(r#"value="A2" class="sr-only" data-testid="courses-settings-level-radio" checked"#));
    assert_eq!(post("/courses/spanish/settings", &[("start_unit", "A2"), ("motivator", "stats")]), 303);
    assert_eq!(project().unwrap().start_unit, "A2");
    assert_eq!(post("/courses/spanish/settings", &[("start_unit", "")]), 303);
    assert_eq!(project().unwrap().start_unit, "");
    // The level page is gone.
    assert_eq!(get("/start").0, 404);
}

/// Generate, the tutor and Add content with AI with saved keys, answered
/// by the dev simulator the default base URLs point at (as the seeded
/// dev keys are in dev mode): the app does not know it is not a real
/// provider. Without a key the dialog asks for one; there is no stand-in
/// and no "Mock AI responses" option any more (#298).
#[test]
fn ai_features_with_saved_keys_answered_by_the_simulator() {
    super::point_defaults_at_the_simulator();
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let state = super::test_state(false, None);
    let (ann, dev) = state
        .db
        .with(|c| {
            let ann = crate::auth::create_user(c, "ann@example.org", "x")?;
            let dev = crate::auth::create_user(c, "dora@example.org", "x")?;
            for u in [ann, dev] {
                crate::terms::accept(c, u, crate::terms::version())?;
            }
            crate::dev::set_developer(c, dev)?;
            crate::courses::open_project(c, dev, "spanish")?;
            Ok((ann, dev))
        })
        .unwrap();
    let cookie = |u| format!("sid={}", state.db.with(|c| crate::auth::create_session(c, u)).unwrap());
    let (ann_c, dev_c) = (cookie(ann), cookie(dev));
    let app = super::router(state.clone());
    let base = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    });
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    let get = |path: &str, c: &str| {
        let mut r = agent.get(&format!("{base}{path}")).header("cookie", c).call().unwrap();
        (r.status().as_u16(), r.body_mut().read_to_string().unwrap())
    };
    let post = |path: &str, c: &str, form: &[(&str, &str)]| {
        let mut r = agent.post(&format!("{base}{path}")).header("cookie", c).send_form(form.iter().copied()).unwrap();
        let loc = r.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
        (r.status().as_u16(), loc, r.body_mut().read_to_string().unwrap())
    };
    let tutor = |c: &str| {
        let mut r = agent.post(&format!("{base}/api/tutor/start")).header("cookie", c).send_json(serde_json::json!({"provider": "anthropic"})).unwrap();
        (r.status().as_u16(), r.body_mut().read_to_string().unwrap())
    };
    let save = |c: &str, body: serde_json::Value| {
        let mut r = agent.post(&format!("{base}/api/profile")).header("cookie", c).send_json(body).unwrap();
        (r.status().as_u16(), r.body_mut().read_to_string().unwrap())
    };
    // No Dev section and no Mock AI responses option, for anyone.
    for c in [&ann_c, &dev_c] {
        let html = get("/profile", c).1;
        assert!(!html.contains("profile-dev") && !html.contains("mock_ai"), "{html}");
        assert_eq!(save(c, serde_json::json!({"mock_ai": true})).0, 422);
    }
    // The dev seed's keys, one per provider.
    state.db.with(|c| crate::dev::save_dev_keys(c, &state.vault, dev)).unwrap();

    // "Create a course" lands on the editor with the Generate dialog open.
    let draft = |c: &str| {
        let (status, loc, _) = post("/courses/new", c, &[]);
        assert_eq!(status, 303);
        let id = loc.strip_prefix("/courses/").and_then(|l| l.strip_suffix("?generate=1")).expect("to the editor with the dialog").to_string();
        (id, get(&loc, c).1)
    };
    // No key: the dialog says so first, greyed out, so
    // nobody writes a prompt only to be stopped at Generate.
    let (_, page) = draft(&ann_c);
    assert!(page.contains(r#"data-testid="generate-no-key""#) && page.contains(r#"href="/profile#api-keys" data-testid="generate-profile-link""#), "{page}");
    assert!(page.contains(r#"<fieldset class="gen-fields" disabled data-testid="generate-disabled">"#), "{page}");
    assert!(!page.contains(r#"data-testid="generate-provider"#) && !page.contains(r#"data-testid="ai-disclosure""#), "{page}");
    let (id, page) = draft(&dev_c);
    assert_eq!(page.matches(r#"data-testid="generate-provider""#).count(), 3, "a choice of the three saved keys");
    assert!(page.contains(" open data-open-on-load>") && !page.contains("generate-no-key"), "{page}");
    // A saved key never goes to a custom base URL (#372), and a custom
    // base URL uses only the web ports (#373). Neither reaches the network.
    let wish = ("wish", "Volcanoes and how they erupt");
    let (status, _, body) = post(&format!("/courses/{id}/generate"), &dev_c, &[("provider", "anthropic"), wish, ("base_url", "https://8.8.8.8/v1")]);
    assert_eq!(status, 200);
    assert!(body.contains("A custom base URL needs its own API key"), "{body}");
    let custom = [("provider", "anthropic"), wish, ("base_url", "https://8.8.8.8:8443/v1"), ("api_key", "sk-pasted-for-that-server")];
    let (status, _, body) = post(&format!("/courses/{id}/generate"), &dev_c, &custom);
    assert_eq!(status, 200);
    assert!(body.contains("may only use port 443 (https) or 80 (http), not 8443"), "{body}");
    let (status, loc, body) = post(&format!("/courses/{id}/generate"), &dev_c, &[("provider", "anthropic"), ("wish", "Volcanoes and how they erupt"), ("lessons", "6")]);
    assert_eq!(status, 303, "{body}");
    // To the editor, whose modal follows the job.
    let job = loc.strip_prefix(&format!("/courses/{id}?job=")).expect("to the editor's progress modal").to_string();
    let mut finished = None;
    for _ in 0..100 {
        let s: serde_json::Value = serde_json::from_str(&get(&format!("/api/generate/{job}"), &dev_c).1).unwrap();
        if s["done"] == true {
            finished = Some(s);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let s = finished.expect("the generation did not finish");
    assert_eq!((s["error"].as_str(), s["lessons_done"].as_u64(), s["course_title"].as_str()), (None, Some(3), Some("Fractions")), "{s}");
    assert_eq!(s["course_id"].as_str(), Some(id.as_str()), "written into the draft");
    // Generated: the modal just closes, no more Generate dialog, and it can't be posted again.
    let page = get(&loc, &dev_c).1;
    assert!(!page.contains("course-ai-modal") && !page.contains(r#"id="gen-dialog""#), "{page}");
    assert_eq!(get(&format!("/courses/generate/{job}"), &dev_c).0, 303, "the old status page leads to the editor");
    let (status, loc, _) = post(&format!("/courses/{id}/generate"), &dev_c, &[("provider", "anthropic"), ("wish", "Volcanoes and how they erupt")]);
    assert!(status == 303 && loc.starts_with(&format!("/courses/{id}?notice=")), "{loc}");
    let (status, body) = tutor(&dev_c);
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("practise this lesson together"), "{body}");

    // Add content with AI: no status page, the editor's modal follows the
    // job (`?job=`). edit-plan.json's default rewrites and renames one
    // lesson (the Outline switch does not apply to an edit);
    // add-lesson.json's default adds "Comparing fractions";
    // revise-plan.json's `replace-all` replaces the whole course.
    let course = s["course_id"].as_str().unwrap().to_string();
    let wait = |job: &str| {
        for _ in 0..100 {
            let s: serde_json::Value = serde_json::from_str(&get(&format!("/api/generate/{job}"), &dev_c).1).unwrap();
            if s["done"] == true {
                return s;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        panic!("job {job} did not finish");
    };
    for (mode, text, shows) in [
        // A fact check (#1238): one lesson fixed, its why line above its diff, a Check line on one left alone.
        ("edit", "Check the facts in every lesson and fix what is wrong [fixture:check]", "the numerator counts parts out of the denominator"),
        ("edit", "More worked examples, please", "Simplifying fractions"),
        ("add", "Anything at all, please", "Comparing fractions"),
        ("change", "Anything at all [fixture:replace-all]", "New lesson 1: the course was replaced"),
    ] {
        let (status, loc, body) = post(&format!("/courses/{course}/ai"), &dev_c, &[("text", text), ("mode", mode), ("depth", if mode == "edit" { "outline" } else { "full" })]);
        assert_eq!(status, 303, "{body}");
        let (path, job) = loc.split_once("?job=").expect("back to the editor with the job");
        assert_eq!(path, format!("/courses/{course}"));
        let done = wait(job);
        assert!(done["error"].is_null(), "{mode}: {done}");
        if mode == "edit" && !text.contains("[fixture:check]") {
            assert_eq!((done["editing"].as_bool(), done["lessons_total"].as_u64()), (Some(true), Some(1)), "{done}");
            let lesson = state.db.with(|c| crate::courses::read_file(c, &course, "lessons/simplifying.json")).unwrap().unwrap();
            assert!(lesson.contains("rewritten from your feedback") && lesson.contains("Simplifying fractions"), "{lesson}");
        }
        // Done: the review of what it changed, in place of the job modal.
        let page = get(&loc, &dev_c).1;
        assert!(!page.contains(r#"data-testid="course-ai-modal""#), "{mode}: {page}");
        assert!(page.contains(r#"data-testid="course-review-modal""#) && page.contains(shows), "{mode}");
        if text.contains("[fixture:check]") {
            assert!(page.contains(r#"data-testid="course-review-why-line""#) && page.contains(r#"data-testid="course-review-left-alone""#), "{page}");
            assert!(page.contains("is a habit, not a rule"), "the Check line: {page}");
        }
        // The old status page sends this job to the editor; without ?job= the job is over and no modal shows.
        assert_eq!(get(&format!("/courses/generate/{job}"), &dev_c).0, 303);
        assert!(!get(&format!("/courses/{course}"), &dev_c).1.contains("course-ai-modal"));
    }

    // Edit with AI on one row (#1242): one request (edit-lesson.json), only that lesson changes.
    let files = state.db.with(|c| crate::courses::load_files(c, &course)).unwrap();
    let (_, lessons) = crate::arrange::lessons_of(&files).unwrap();
    let (path, title) = (lessons[0].path.clone(), lessons[0].value["title"].as_str().unwrap().to_string());
    let page = get(&format!("/courses/{course}"), &dev_c).1;
    assert!(page.contains(r#"data-testid="course-lessons-ai-edit""#) && page.contains(r#"data-testid="course-lessons-ai-dialog""#), "the row's icon and its dialog");
    let (status, loc, body) = post(&format!("/courses/{course}/ai"), &dev_c, &[("text", "Simpler words, please"), ("lesson", path.as_str())]);
    assert_eq!(status, 303, "{body}");
    let done = wait(loc.split_once("?job=").unwrap().1);
    assert!(done["error"].is_null(), "{done}");
    assert_eq!((done["editing"].as_bool(), done["lesson_title"].as_str(), done["lessons_done"].as_u64()), (Some(true), Some(title.as_str()), Some(1)), "{done}");
    let after = state.db.with(|c| crate::courses::load_files(c, &course)).unwrap();
    for f in &after {
        let was = &files.iter().find(|b| b.path == f.path).unwrap().content;
        assert_eq!(f.content.contains("edited from your request"), f.path == path, "{}", f.path);
        assert_eq!(&f.content == was, f.path != path, "only {path} changed: {}", f.path);
    }
    assert!(get(&format!("/courses/{course}"), &dev_c).1.contains("Edited with feedback: “Simpler words, please”"));
    // A reply that renames the lesson is refused and nothing changes.
    let (status, loc, body) = post(&format!("/courses/{course}/ai"), &dev_c, &[("text", "Anything at all [fixture:renames]"), ("lesson", path.as_str())]);
    assert_eq!(status, 303, "{body}");
    let done = wait(loc.split_once("?job=").unwrap().1);
    assert!(done["error"].as_str().is_some_and(|e| e.contains("changed the lesson's id")), "{done}");
    let pairs = |fs: &[crate::content::CourseFile]| fs.iter().map(|f| (f.path.clone(), f.content.clone())).collect::<Vec<_>>();
    assert_eq!(pairs(&state.db.with(|c| crate::courses::load_files(c, &course)).unwrap()), pairs(&after), "the course is unchanged");
    // A lesson that is not in the course: the dialog again, with why.
    let (status, _, body) = post(&format!("/courses/{course}/ai"), &dev_c, &[("text", "Simpler words, please"), ("lesson", "lessons/gone.json")]);
    assert_eq!(status, 200);
    assert!(body.contains(r#"data-reopen="lessons/gone.json""#) && body.contains("course-lessons-ai-error") && !body.contains(r#"data-testid="course-ai-error""#), "{body}");

    // The keys removed: the key is asked for again.
    state.db.with(|c| crate::secrets::remove_all(c, dev)).unwrap();
    let page = get(&format!("/courses/{course}"), &dev_c).1;
    assert!(page.contains("course-ai-no-key") && !page.contains("course-lessons-ai-edit") && !page.contains("course-lessons-ai-dialog"), "no Edit with AI on the rows");
    let (id, page) = draft(&dev_c);
    assert!(page.contains("generate-no-key"));
    let (status, _, body) = post(&format!("/courses/{id}/generate"), &dev_c, &[("provider", "anthropic"), ("wish", "Volcanoes and how they erupt"), ("lessons", "6")]);
    assert_eq!(status, 200);
    assert!(body.contains("Paste an API key for") && body.contains(" open data-open-on-load>"), "refused: the editor with the dialog open again: {body}");

    // A first generation that stopped has no editor: its card offers
    // Retry, which opens the dialog on the list with what was asked.
    let notes = "Volcanoes erupt where plates meet. ".repeat(2);
    state.db.with(|c| {
        crate::courses::restart_generation(c, &id, "Volcanoes", "Volcanoes for Monday", &notes)?;
        crate::courses::set_generating(c, &id, "gone")?;
        crate::courses::finish_generation(c, &id, Some("the API answered HTTP 401. invalid x-api-key"))
    })
    .unwrap();
    let (status, page) = get(&format!("/courses/{id}"), &dev_c);
    assert_eq!(status, 303, "no editor: {page}");
    assert_eq!(get(&format!("/courses/{id}/file?path=course.json"), &dev_c).0, 303);
    let list = get("/courses", &dev_c).1;
    assert!(list.contains(r#"data-testid="courses-card-stopped""#) && list.contains(&format!(r#"href="/courses?generate={id}" data-testid="courses-retry""#)), "{list}");
    assert!(!list.contains(r#"id="gen-dialog""#));
    let list = get(&format!("/courses?generate={id}"), &dev_c).1;
    assert!(list.contains(r#"id="gen-dialog""#) && list.contains(" open data-open-on-load>") && list.contains("Volcanoes for Monday</textarea>") && list.contains(notes.trim()), "{list}");
    assert!(list.contains(r#"data-testid="generate-error">Writing your course stopped. The AI provider didn&#39;t accept your API key."#), "{list}");
    // Refused again: back on the list, not the editor.
    let (status, _, body) = post(&format!("/courses/{id}/generate"), &dev_c, &[("provider", "anthropic"), ("wish", "Volcanoes and how they erupt"), ("lessons", "6")]);
    assert_eq!(status, 200);
    assert!(body.contains(r#"data-testid="courses-mine""#) && body.contains("Paste an API key for"), "{body}");
    let (status, body) = tutor(&dev_c);
    assert_eq!(status, 400);
    assert!(body.contains("key on file; add one in your profile"), "{body}");
}

/// The editor of a course whose job finished a moment ago (#183). The
/// generator writes the course row before it marks the job done, so a
/// page can read a row that still says "generating" and a job that is
/// done. It must not render a "being generated" page with no modal,
/// which nothing would ever refresh: the row is read again, and while
/// it still says busy the finished job's modal (Continue) is shown.
#[test]
fn editor_of_a_course_whose_job_just_finished() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let state = super::test_state(true, None);
    let ann = state
        .db
        .with(|c| {
            let ann = crate::auth::create_user(c, "ann@example.org", "x")?;
            crate::terms::accept(c, ann, crate::terms::version())?;
            Ok(ann)
        })
        .unwrap();
    let cookie = format!("sid={}", state.db.with(|c| crate::auth::create_session(c, ann)).unwrap());
    let app = super::router(state.clone());
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
    let r = agent.post(&format!("{base}/courses/new")).header("cookie", &cookie).send_form([("title", "Fractions")]).unwrap();
    let loc = r.headers().get("location").unwrap().to_str().unwrap().to_string();
    let id = loc.strip_prefix("/courses/").and_then(|l| l.strip_suffix("?generate=1")).expect("to the editor").to_string();

    // The row says generating; the job in memory is done.
    state.db.with(|c| crate::courses::set_generating(c, &id, "job1")).unwrap();
    state.jobs.insert_finished_for_test("job1", ann, &id);
    for path in [format!("/courses/{id}?job=job1"), format!("/courses/{id}")] {
        let (status, body) = get(&path);
        assert_eq!(status, 200, "{path}");
        assert!(body.contains("data-testid=\"course-ai-modal\"") && body.contains("data-state=\"done\""), "{path}: no modal on a busy row with a done job\n{body}");
        assert!(body.contains("Your course is ready") && body.contains("course-ai-modal-continue"), "{path}");
    }

    // The row caught up: the editor is the result, no modal.
    state.db.with(|c| crate::courses::finish_generation(c, &id, None)).unwrap();
    for path in [format!("/courses/{id}?job=job1"), format!("/courses/{id}")] {
        let (status, body) = get(&path);
        assert_eq!(status, 200);
        assert!(!body.contains("course-ai-modal") && body.contains("data-course-status=\"ready\""), "{path}\n{body}");
    }
}

/// Sharing a course (#451): the owner invites addresses, each gets a
/// link; signed out, the link leads to a login page that says what it
/// is for and comes back; signed in, the course is on the recipient's
/// list read only (no editor, no files); a confirmed address needs no
/// link; a forwarded link is refused; removing the share takes the
/// course off their list and out of their session.
#[test]
fn a_shared_course_is_on_the_recipients_list_read_only() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let state = super::test_state(false, None);
    let app = super::router(state.clone());
    let base = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap() });
        format!("http://{addr}")
    });
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    let get = |path: &str, cookie: &str| {
        let mut r = agent.get(&format!("{base}{path}")).header("cookie", cookie).call().unwrap();
        let loc = r.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
        (r.status().as_u16(), loc, r.body_mut().read_to_string().unwrap())
    };
    let post = |path: &str, cookie: &str, form: &[(&str, &str)]| {
        let r = agent.post(&format!("{base}{path}")).header("cookie", cookie).send_form(form.iter().copied()).unwrap();
        let set = r.headers().get("set-cookie").map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string()).unwrap_or_default();
        let loc = r.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
        (r.status().as_u16(), set, loc)
    };
    // Registering does not sign in (#379): the sign-in that follows carries the share link on.
    let register = |email: &str, share: &str| {
        let (status, _, _) = post("/register", "", &[("email", email), ("password", "long-enough-pw"), ("age", "on"), ("accept", "on"), ("share", share)]);
        assert_eq!(status, 200, "registering shows the check-your-mail page");
        post("/login", "", &[("email", email), ("password", "long-enough-pw"), ("share", share)])
    };
    let invitations = |to: &str| {
        let to = to.to_string();
        state
            .db
            .with(move |c| {
                let mut stmt = c.prepare("SELECT body_text FROM mail_outbox WHERE to_addr = ?1 AND purpose = 'course-share' ORDER BY id")?;
                let v = stmt.query_map([to], |r| r.get(0))?.collect::<Result<Vec<String>, _>>();
                v
            })
            .unwrap()
    };

    let (_, ann, _) = register("ann@example.org", "");
    let (status, _, loc) = post("/courses/new", &ann, &[("title", "Fractions")]);
    assert_eq!(status, 303);
    let id = loc.strip_prefix("/courses/").and_then(|l| l.strip_suffix("?generate=1")).expect("the editor").to_string();
    assert!(get(&format!("/courses/{id}"), &ann).2.contains(r#"data-testid="course-edit-share""#));

    // Inviting: the owner's own address refuses the list; otherwise one message per new address.
    let (status, _, loc) = post(&format!("/courses/{id}/share"), &ann, &[("emails", "bob@example.org, ann@example.org")]);
    assert!(status == 303 && loc.contains("share=1&notice=share-own"), "{loc}");
    // The POST target opened as a page goes back to the dialog, not a 405 (#667).
    let (status, loc, _) = get(&format!("/courses/{id}/share"), &ann);
    assert!(status == 303 && loc.ends_with(&format!("/courses/{id}?share=1")), "{status} {loc}");
    assert!(invitations("bob@example.org").is_empty());
    let (_, _, loc) = post(&format!("/courses/{id}/share"), &ann, &[("emails", "Bob@Example.org\ncarol@example.org")]);
    assert!(loc.contains("notice=share-sent-2-0"), "{loc}");
    let (_, _, loc) = post(&format!("/courses/{id}/share"), &ann, &[("emails", "bob@example.org")]);
    assert!(loc.contains("notice=share-sent-0-1"), "{loc}");
    let sent = invitations("bob@example.org");
    assert_eq!(sent.len(), 1, "one invitation per address");
    let link = sent[0].split_whitespace().find(|w| w.starts_with(&format!("{base}/courses/shared/"))).expect("the link").to_string();
    let token = link.rsplit('/').next().unwrap().to_string();
    assert_eq!(token.len(), 64);
    let editor = get(&format!("/courses/{id}"), &ann).2;
    assert!(editor.contains(r#"data-testid="course-share-row" data-email="bob@example.org" data-opened="false""#), "{editor}");
    assert!(editor.contains(r#"data-testid="course-edit-share">Share (2)"#), "{editor}");

    // Signed out, the link leads to the login page, which says what it is for; registering there comes back to it.
    let (status, loc, _) = get(&format!("/courses/shared/{token}"), "");
    assert_eq!((status, loc.as_str()), (303, format!("/login?share={token}").as_str()));
    let (status, _, page) = get(&format!("/login?share={token}"), "");
    assert_eq!(status, 200);
    assert!(page.contains(r#"data-testid="login-share-notice" data-course-title="Fractions">It looks like you don&#39;t have an account with us! Please login or signup to continue viewing your course: Fractions</p>"#), "{page}");
    assert!(page.contains(&format!(r#"<input type="hidden" name="share" value="{token}">"#)) && page.contains(&format!(r#"/register?share={token}" data-testid="login-register-link""#)), "{page}");
    let (_, _, page) = get(&format!("/register?share={token}"), "");
    assert!(page.contains(r#"data-testid="register-share-notice""#) && page.contains(&format!(r#"/login?share={token}" data-testid="register-login-link""#)), "{page}");
    assert_eq!(get("/courses/shared/nope", "").1, "/login?share=nope");
    assert!(!get("/login?share=nope", "").2.contains("login-share-notice"), "an unknown token: the usual page");
    // The owner opening their own link (from the inbox, say) does not take it: their course page instead.
    let (status, loc, _) = get(&format!("/courses/shared/{token}"), &ann);
    assert_eq!((status, loc.as_str()), (303, format!("/courses/{id}?share=1").as_str()));
    assert!(get(&format!("/courses/{id}"), &ann).2.contains(r#"data-email="bob@example.org" data-opened="false""#), "still unopened");
    let (status, bob, loc) = register("bob@example.org", &token);
    assert_eq!((status, loc.as_str()), (303, format!("/courses/shared/{token}").as_str()));
    let (status, loc, _) = get(&format!("/courses/shared/{token}"), &bob);
    assert_eq!((status, loc.as_str()), (303, "/"), "opened: home, with the course active");
    assert_eq!(get("/lessons", &bob).0, 200);

    // On bob's list, read only.
    let list = get("/courses", &bob).2;
    assert!(list.contains(&format!(r#"data-testid="courses-card" data-course-id="{id}""#)) && list.contains(r#"data-testid="courses-card-shared" data-owner="ann@example.org""#), "{list}");
    assert!(!list.contains(&format!(r#"href="/courses/{id}" data-testid="courses-edit""#)), "no editor link\n{list}");
    assert_eq!(get(&format!("/courses/{id}"), &bob).0, 404);
    assert_eq!(get(&format!("/courses/{id}/export.json"), &bob).0, 404);
    assert_eq!(post(&format!("/courses/{id}/share"), &bob, &[("emails", "dan@example.org")]).0, 404, "only the owner shares");
    assert_eq!(post(&format!("/courses/open/{id}"), &bob, &[]).0, 303, "Open on the card works");
    assert!(get(&format!("/courses/{id}"), &ann).2.contains(r#"data-email="bob@example.org" data-opened="true""#));

    // A forwarded link lets nobody else in; a log-in with it goes on to the link too.
    let (_, dan, _) = register("dan@example.org", "");
    let (status, loc, _) = get(&format!("/courses/shared/{token}"), &dan);
    assert!(status == 303 && loc == "/courses?notice=share-taken", "{loc}");
    assert!(!get("/courses", &dan).2.contains(&format!(r#"data-course-id="{id}""#)));
    let (status, _, loc) = post("/login", "", &[("email", "bob@example.org"), ("password", "long-enough-pw"), ("share", &token)]);
    assert_eq!((status, loc.as_str()), (303, format!("/courses/shared/{token}").as_str()));
    let (status, _, loc) = post("/login", "", &[("email", "bob@example.org"), ("password", "long-enough-pw")]);
    assert_eq!((status, loc.as_str()), (303, "/"));

    // Carol's confirmed address needs no link: the course is on her list as soon as it is shared.
    let (_, carol, _) = register("carol@example.org", "");
    assert!(!get("/courses", &carol).2.contains(&format!(r#"data-course-id="{id}""#)), "unconfirmed: not yet");
    state.db.with(|c| c.execute("UPDATE users SET email_verified_at = 5 WHERE email = 'carol@example.org'", [])).unwrap();
    let list = get("/courses", &carol).2;
    assert!(list.contains(r#"data-testid="courses-shared""#) && list.contains(&format!(r#"data-course-id="{id}""#)), "{list}");
    assert_eq!(post(&format!("/courses/open/{id}"), &carol, &[]).0, 303);

    // Removing the share: off bob's list, out of his session; the owner keeps it.
    let (status, _, loc) = post(&format!("/courses/{id}/share/remove"), &ann, &[("email", "bob@example.org")]);
    assert!(status == 303 && loc.contains("notice=share-removed"), "{loc}");
    assert!(!get("/courses", &bob).2.contains(&format!(r#"data-course-id="{id}""#)));
    assert_eq!(get("/lessons", &bob).1, "/courses", "no active course any more");
    // The old link says it no longer works (#1082): on the list signed in, on the login page signed out.
    let (status, loc, _) = get(&format!("/courses/shared/{token}"), &bob);
    assert_eq!((status, loc.as_str()), (303, "/courses?notice=share-gone"), "the link is gone with the share");
    assert!(get("/courses?notice=share-gone", &bob).2.contains("This link no longer works."));
    assert_eq!(get(&format!("/courses/shared/{token}"), "").1, format!("/login?share={token}"));
    let page = get(&format!("/login?share={token}"), "").2;
    assert!(page.contains(r#"data-testid="login-share-notice" data-course-title="">This link no longer works."#), "{page}");
    let (status, _, loc) = post("/login", "", &[("email", "bob@example.org"), ("password", "long-enough-pw"), ("share", &token)]);
    assert_eq!((status, loc.as_str()), (303, format!("/courses/shared/{token}").as_str()), "signing in goes on to the link, and so to the notice");
    assert_eq!(post(&format!("/courses/open/{id}"), &bob, &[]).0, 404);
    let editor = get(&format!("/courses/{id}"), &ann).2;
    assert!(!editor.contains(r#"data-email="bob@example.org""#) && editor.contains(r#"data-email="carol@example.org""#), "{editor}");
    assert!(get("/courses", &carol).2.contains(&format!(r#"data-course-id="{id}""#)), "carol's stays");
}

/// #633: a channel reaches someone with a provider, or dev mode's
/// mailbox standing in; the `log` provider alone does not.
#[test]
fn caps_follow_the_providers() {
    assert!(crate::Caps::reaches(false, false), "a provider");
    assert!(crate::Caps::reaches(true, true), "dev mode's mailbox");
    assert!(!crate::Caps::reaches(true, false), "log alone");
    assert_eq!(super::test_state(false, None).caps(), crate::Caps { mail: true });
    assert_eq!(super::test_state_without(false).caps(), crate::Caps { mail: false });
}

/// #633: on a server with no mail provider nothing offers to send a
/// message: no confirmation link or red !, no e-mailed second step (an account already on one cannot
/// sign in, and is told the setting), share links are shown to the
/// owner, and the data export is a Download button on the profile.
#[test]
fn without_mail_nothing_offers_to_send() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let state = super::test_state_without(false);
    let app = super::router(state.clone());
    let base = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap() });
        format!("http://{addr}")
    });
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    let get = |path: &str, cookie: &str| {
        let mut r = agent.get(&format!("{base}{path}")).header("cookie", cookie).call().unwrap();
        let loc = r.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
        (r.status().as_u16(), loc, r.body_mut().read_to_string().unwrap())
    };
    let post = |path: &str, cookie: &str, form: &[(&str, &str)]| {
        let mut r = agent.post(&format!("{base}{path}")).header("cookie", cookie).send_form(form.iter().copied()).unwrap();
        let set = r.headers().get("set-cookie").map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string()).unwrap_or_default();
        let loc = r.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
        (r.status().as_u16(), set, loc, r.body_mut().read_to_string().unwrap())
    };
    let count = |sql: &str| state.db.with(|c| c.query_row(sql, [], |r| r.get::<_, i64>(0))).unwrap();

    // Registering queues no confirmation mail and issues no link.
    let (status, ann, _, _) = post("/register", "", &[("email", "ann@example.org"), ("password", "long-enough-pw"), ("age", "on"), ("accept", "on")]);
    assert_eq!(status, 303);
    assert_eq!(count("SELECT COUNT(*) FROM mail_outbox") + count("SELECT COUNT(*) FROM email_verifications"), 0);

    // The profile: the (i) in place of the red !, no e-mailed second step.
    let profile = get("/profile", &ann).2;
    assert!(profile.contains(r#"data-testid="email-no-mail-info""#) && profile.contains(r#"data-testid="email-no-mail-dialog""#), "{profile}");
    for gone in [r#"data-testid="email-unverified""#, r#"data-testid="email-verify-resend""#, r#"data-method="email""#] {
        assert!(!profile.contains(gone), "{gone} is on the page");
    }
    assert!(profile.contains(r#"data-method="totp""#), "the authenticator app needs nothing");
    // Asked for anyway, the server says no.
    let (status, _, _, page) = post("/profile/two-factor", &ann, &[("method", "email"), ("password", "long-enough-pw")]);
    assert_eq!(status, 422);
    assert!(page.contains("SKRONGLE_MAIL_PROVIDERS"), "{page}");
    let (status, _, loc, _) = post("/profile/verify-email/resend", &ann, &[]);
    assert_eq!((status, loc.as_str()), (303, "/profile"), "nothing to send again");
    assert_eq!(count("SELECT COUNT(*) FROM mail_outbox"), 0);

    // Sharing: the dialog makes links, shows them, and mails nothing; a link works.
    let (_, _, loc, _) = post("/courses/new", &ann, &[("title", "Fractions")]);
    let id = loc.strip_prefix("/courses/").and_then(|l| l.strip_suffix("?generate=1")).expect("the editor").to_string();
    assert!(get(&format!("/courses/{id}"), &ann).2.contains("Make invitation links"));
    let (status, _, _, page) = post(&format!("/courses/{id}/share"), &ann, &[("emails", "bob@example.org")]);
    assert_eq!(status, 200);
    assert!(page.contains("1 invitation link made") && page.contains(r#"data-testid="course-share-link-copy""#), "{page}");
    let link = page.split(&format!(r#"value="{base}/courses/shared/"#)).nth(1).and_then(|r| r.split('"').next()).expect("the link").to_string();
    assert_eq!(link.len(), 64);
    assert_eq!(count("SELECT COUNT(*) FROM course_shares"), 1);
    assert_eq!(count("SELECT COUNT(*) FROM mail_outbox"), 0);
    let (status, bob, _, _) = post("/register", "", &[("email", "bob@example.org"), ("password", "long-enough-pw"), ("age", "on"), ("accept", "on")]);
    assert_eq!(status, 303);
    assert_eq!(get(&format!("/courses/shared/{link}"), &bob).1, "/", "the link opens the course");
    let (status, _, _, page) = post(&format!("/courses/{id}/share"), &ann, &[("emails", "bob@example.org")]);
    assert!(status == 200 && !page.contains(r#"data-testid="course-share-link""#) && page.contains("already had one"), "{page}");

    // Download my data: built without a mail, downloaded from the profile once.
    let (status, _, loc, _) = post("/profile/export", &ann, &[]);
    assert_eq!(status, 303);
    let queued = get(loc.split('#').next().unwrap_or_default(), &ann).2;
    assert!(!queued.contains("You will get an e-mail"), "the notice promises no mail: {queued}");
    assert!(get("/profile", &ann).2.contains("a Download button appears here"));
    assert_eq!(crate::exports::run_queued(&state.db, &state.exports.dir, None, std::time::Duration::ZERO), 1);
    assert_eq!(count("SELECT COUNT(*) FROM mail_outbox"), 0);
    let profile = get("/profile", &ann).2;
    assert!(profile.contains(r#"data-testid="profile-export-download""#), "{profile}");
    assert_eq!(post("/profile/export/download", &bob, &[]).2, "/profile?saved=export-gone#data", "bob has none");
    let (status, _, _, body) = post("/profile/export/download", &ann, &[]);
    assert_eq!(status, 200);
    assert!(body.contains("ann@example.org"));
    let (status, _, loc, _) = post("/profile/export/download", &ann, &[]);
    assert_eq!((status, loc.as_str()), (303, "/profile?saved=export-gone#data"), "once");
    assert!(!get("/profile", &ann).2.contains(r#"data-testid="profile-export-download""#));

    // An account whose second step went by a channel the server lost is refused, naming the setting.
    let sign_in = || post("/login", "", &[("email", "ann@example.org"), ("password", "long-enough-pw")]);
    state.db.with(|c| c.execute("UPDATE users SET two_factor = 'email' WHERE email = 'ann@example.org'", [])).unwrap();
    let (status, cookie, _, page) = sign_in();
    assert_eq!(status, 403);
    assert!(page.contains("SKRONGLE_MAIL_PROVIDERS") && !cookie.starts_with("sid="), "{page}");
    assert_eq!(count("SELECT COUNT(*) FROM two_factor_challenges"), 0, "no code was made");
    state.db.with(|c| c.execute("UPDATE users SET two_factor = '' WHERE email = 'ann@example.org'", [])).unwrap();
    assert_eq!(sign_in().0, 303);
}

/// #633: with providers the same pages offer what they always did.
#[test]
fn with_mail_everything_is_offered() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let state = super::test_state(false, None);
    let app = super::router(state.clone());
    let base = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap() });
        format!("http://{addr}")
    });
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    // Registering signs nobody in (#379): the session comes from /login.
    agent.post(&format!("{base}/register")).send_form([("email", "ann@example.org"), ("password", "long-enough-pw"), ("age", "on"), ("accept", "on")]).unwrap();
    let r = agent.post(&format!("{base}/login")).send_form([("email", "ann@example.org"), ("password", "long-enough-pw")]).unwrap();
    let ann = r.headers().get("set-cookie").map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string()).unwrap_or_default();
    let profile = agent.get(&format!("{base}/profile")).header("cookie", &ann).call().unwrap().body_mut().read_to_string().unwrap();
    for shown in [r#"data-testid="email-unverified""#, r#"data-testid="email-verify-resend""#, r#"data-method="email""#, r#"data-method="totp""#] {
        assert!(profile.contains(shown), "{shown} is missing");
    }
    assert!(!profile.contains("email-no-mail-info"));
    let mails: i64 = state.db.with(|c| c.query_row("SELECT COUNT(*) FROM mail_outbox WHERE purpose = 'email-verify'", [], |r| r.get(0))).unwrap();
    assert_eq!(mails, 1);
}

/// #558: every mode moves the same daily goal, and every mode resumes at
/// the lesson last worked on, opening it in the mode last used.
#[test]
fn every_mode_shares_the_daily_goal_and_where_you_left_off() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let state = super::test_state(false, None);
    let ann = state
        .db
        .with(|c| {
            let ann = crate::auth::create_user(c, "ann@example.org", "h")?;
            crate::terms::accept(c, ann, crate::terms::version())?;
            crate::courses::open_project(c, ann, "spanish")?;
            crate::courses::set_project_settings(c, ann, "spanish", &crate::courses::SettingsChange::default())?;
            Ok(ann)
        })
        .unwrap();
    let cookie = format!("sid={}", state.db.with(|c| crate::auth::create_session(c, ann)).unwrap());
    let course = state.courses.builtin_by_id("spanish").unwrap();
    let lessons = course.lessons();
    let app = super::router(state.clone());
    let base = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    });
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    let get = |path: &str| agent.get(&format!("{base}{path}")).header("cookie", &cookie).call().unwrap().body_mut().read_to_string().unwrap();
    let post = |path: &str, body: serde_json::Value| agent.post(&format!("{base}{path}")).header("cookie", &cookie).send_json(body).unwrap().status().as_u16();
    let ring = |html: &str, testid: &str| {
        let pat = format!(r#"data-testid="{testid}" data-points=""#);
        let rest = &html[html.find(&pat).unwrap_or_else(|| panic!("no {testid}")) + pat.len()..];
        rest[..rest.find('"').unwrap()].parse::<i64>().unwrap()
    };

    // Nothing yet: the first lesson, in Study mode, and an empty ring.
    let home = get("/");
    assert!(home.contains(&format!(r#"href="/lessons/{}" data-testid="home-continue""#, lessons[0].slug)), "{home}");
    assert_eq!(ring(&home, "home-goal-ring"), 0);

    // A quiz answer in Study mode fills both rings.
    let q = &lessons[0].quiz[0];
    assert_eq!(post("/api/answer", serde_json::json!({ "question_id": q.question_id, "answer": q.correct_answer })), 200);
    assert_eq!(ring(&get("/"), "home-goal-ring"), 1);
    assert_eq!(ring(&get("/sprint"), "sprint-goal-ring"), 1, "Sprint mode shows the same ring");

    // Opening a later lesson's Sprint session: every mode resumes there,
    // and the home page opens it in Sprint mode.
    let later = &lessons[5];
    get(&format!("/sprint/play?lesson={}", later.slug));
    let home = get("/");
    assert!(home.contains(&format!(r#"href="/sprint/play?lesson={}" data-testid="home-continue""#, later.slug)), "{home}");
    assert!(get("/sprint").contains(&format!(r#"data-lesson-id="{}" data-state="available" data-passed="false" data-current="true""#, later.slug)));
    let guide = get("/guide");
    assert!(guide.contains(&format!(r#"data-lesson-id="{}" data-state="available" data-passed="false" data-stars="0" data-current="true""#, later.slug)), "{guide}");
    // Its Guide stage is open although the lessons before it are not passed.
    let stage = agent.get(&format!("{base}/guide/{}", later.slug)).header("cookie", &cookie).call().unwrap();
    assert_eq!(stage.status().as_u16(), 200);
    assert!(get("/").contains(&format!(r#"href="/guide/{}" data-testid="home-continue""#, later.slug)), "the Guide is now the mode last used");

    // A review mix does not move it.
    get("/sprint/play?mode=review");
    get("/practice");
    assert!(get("/").contains(&format!(r#"data-testid="home-continue" data-lesson-id="{}""#, later.slug)));
    // No XP anywhere.
    for page in ["/", "/sprint", "/guide"] {
        assert!(!get(page).contains(" XP"), "{page} shows XP");
    }
}

/// #1063: a POST answered without reading its form (here a 404) keeps its
/// connection, even when the form arrives after the headers. Before, hyper
/// closed the connection over the unread bytes and the client got a reset,
/// sometimes before it had read the answer.
#[test]
fn a_body_the_handler_ignores_does_not_cost_the_connection() {
    use std::io::{BufRead, BufReader, Read, Write};
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let app = super::router(super::test_state(false, None));
    let addr = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        addr
    });
    let stream = std::net::TcpStream::connect(addr).unwrap();
    stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
    let mut out = stream.try_clone().unwrap();
    let mut answers = BufReader::new(stream);
    // One answer: its status line, then the body by Content-Length.
    let mut answer = || {
        let mut line = String::new();
        answers.read_line(&mut line).unwrap();
        let status = line.split(' ').nth(1).unwrap_or_default().to_string();
        let mut length = 0;
        loop {
            line.clear();
            answers.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
            if let Some(n) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                length = n.trim().parse().unwrap();
            }
        }
        let mut body = vec![0; length];
        answers.read_exact(&mut body).unwrap();
        status
    };
    for _ in 0..2 {
        out.write_all(b"POST /no-such-page HTTP/1.1\r\nhost: localhost\r\ncontent-type: application/x-www-form-urlencoded\r\ncontent-length: 2\r\n\r\n").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
        out.write_all(b"x=").unwrap();
        assert_eq!(answer(), "404");
    }
}

/// The manifest names the app, opens at the home page and points at icons
/// that exist (#345).
#[test]
fn web_manifest_points_at_real_icons() {
    let m = super::web_manifest_json();
    assert_eq!(m["name"], crate::APP_NAME);
    assert_eq!(m["display"], "standalone");
    assert_eq!(m["start_url"], format!("{}/", crate::base_path()));
    let icons = m["icons"].as_array().cloned().unwrap_or_default();
    assert_eq!(icons.len(), 2);
    for icon in icons {
        let src = icon["src"].as_str().unwrap_or_default();
        let name = src.split("/static/").nth(1).and_then(|n| n.split('?').next()).unwrap_or_default();
        assert!(super::Assets::get(name).is_some(), "missing icon {src}");
    }
}

/// Themes turned off (#1138, no `SKRONGLE_THEMES_ENABLED=1`): no page offers
/// or wears one, a course's stored theme is kept for when they are back,
/// and the waitlist page says nothing about them.
#[test]
fn themes_off_hides_every_theme() {
    use askama::Template;
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let Ok(mut s) = std::sync::Arc::try_unwrap(super::test_state(false, None)) else { unreachable!("a new state has one owner") }; // panic-ok: tests only
    s.themes = std::sync::Arc::new(crate::themes::Themes::off());
    let state = std::sync::Arc::new(s);
    let ann = state.db.with(|c| crate::auth::create_user(c, "ann@example.org", "x")).unwrap();
    state.db.with(|c| crate::terms::accept(c, ann, crate::terms::version())).unwrap();
    let cookie = format!("sid={}", state.db.with(|c| crate::auth::create_session(c, ann)).unwrap());
    let app = super::router(state.clone());
    let base = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    });
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    let get = |path: &str| agent.get(&format!("{base}{path}")).header("cookie", &cookie).call().unwrap().body_mut().read_to_string().unwrap();
    let post = |path: &str, form: &[(&str, &str)]| agent.post(&format!("{base}{path}")).header("cookie", &cookie).send_form(form.iter().copied()).unwrap().status().as_u16();
    let theme = || state.db.with(|c| c.query_row("SELECT theme FROM projects WHERE user_id = ?1 AND course_id = 'spanish'", [ann], |r| r.get::<_, String>(0))).unwrap();

    // The Start wizard has no Theme step, and a course that chose one before wears none.
    assert_eq!(post("/courses/open/spanish", &[]), 303);
    let page = get("/");
    assert!(page.contains("data-wizard") && !page.contains(r#"data-step="theme""#), "no Theme step in the wizard");
    state.db.with(|c| c.execute("UPDATE projects SET theme = 'realm' WHERE user_id = ?1", [ann])).unwrap();
    let page = get("/");
    assert!(!page.contains("data-theme=") && !page.contains("theme-banner") && !page.contains("theme-tag"), "the course wears no theme");
    assert!(get("/theme.css?t=realm").trim().is_empty());
    // The gear has no Theme row; saving keeps the stored choice, a theme can't be picked.
    assert!(!get("/courses").contains(r#"data-row="theme""#));
    assert_eq!(post("/courses/spanish/settings", &[("motivator", "skc"), ("next", "home")]), 303);
    assert_eq!(theme(), "realm");
    assert_eq!(post("/courses/spanish/settings", &[("theme", "realm")]), 400);
    // The waitlist page: no themes section.
    let html = crate::routes::waitlist::WaitlistTemplate::new(&state).render().unwrap();
    assert!(!html.contains("waitlist-theme") && !html.contains(&crate::t("waitlist.themes_h2")));
    // The docs (#1184): the theme pages are gone and no page, its sidebar,
    // llms.txt or the schema names themes.
    let status = |path: &str| agent.get(&format!("{base}{path}")).header("cookie", &cookie).call().unwrap().status().as_u16();
    assert_eq!((status("/docs/using-themes"), status("/docs/themes.md")), (404, 404));
    for path in ["/docs", "/docs/getting-started", "/docs/getting-started.md", "/docs/features", "/docs/course-format.md", "/docs/course.schema.json", "/llms.txt"] {
        assert!(!get(path).to_lowercase().contains("theme"), "{path} names themes while they are off");
    }
}

/// Character art off with themes (#1153): no profile picture, portrait or
/// background is drawn. The badge is the gear, a conversation line shows the
/// speaker's name only, the settings offer no picture or background (and
/// saving keeps the stored ones), and the verb drill gets no people.
#[test]
fn themes_off_draws_no_character_art() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let Ok(mut s) = std::sync::Arc::try_unwrap(super::test_state(false, None)) else { unreachable!("a new state has one owner") }; // panic-ok: tests only
    s.themes = std::sync::Arc::new(crate::themes::Themes::off());
    let state = std::sync::Arc::new(s);
    let ann = state.db.with(|c| crate::auth::create_user(c, "ann@example.org", "x")).unwrap();
    state.db.with(|c| crate::terms::accept(c, ann, crate::terms::version())).unwrap();
    let cookie = format!("sid={}", state.db.with(|c| crate::auth::create_session(c, ann)).unwrap());
    let app = super::router(state.clone());
    let base = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    });
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    let get = |path: &str| agent.get(&format!("{base}{path}")).header("cookie", &cookie).call().unwrap().body_mut().read_to_string().unwrap();
    let post = |path: &str, form: &[(&str, &str)]| agent.post(&format!("{base}{path}")).header("cookie", &cookie).send_form(form.iter().copied()).unwrap().status().as_u16();
    let look = || state.db.with(|c| c.query_row("SELECT avatar, avatar_color FROM projects WHERE user_id = ?1 AND course_id = 'spanish'", [ann], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))).unwrap();

    // The Start wizard has no picture step.
    assert_eq!(post("/courses/open/spanish", &[]), 303);
    let home = get("/");
    assert!(home.contains("data-wizard") && !home.contains(r#"data-step="picture""#), "no picture step");
    // Inside a course the badge is still the gear, and no portrait is drawn anywhere.
    assert!(home.contains("avatar-blank") && !home.contains("#av-") && !home.contains("--av:"), "no picture on the course home");
    // A conversation: each line has its speaker's name and no portrait.
    let lesson = get("/lessons/asking-questions");
    assert!(lesson.contains("lesson-conversation-speaker") && lesson.contains("Inés"), "the names stay");
    let convs = lesson.split(r#"data-testid="lesson-conversations""#).nth(1).unwrap_or_default().split("</section>").next().unwrap_or_default();
    assert!(convs.contains("Inés") && !convs.contains("avatar"), "no portraits: {convs}");
    // The gear: no picture or background rows; a save without them keeps the stored look.
    state.db.with(|c| c.execute("UPDATE projects SET avatar = 'kai', avatar_color = 4 WHERE user_id = ?1", [ann])).unwrap();
    let courses = get("/courses");
    assert!(!courses.contains(r#"data-row="picture""#) && !courses.contains(r#"data-row="background""#) && !courses.contains("cs-badge"));
    assert_eq!(post("/courses/spanish/settings", &[("motivator", "skc"), ("next", "home")]), 303);
    assert_eq!(look(), ("kai".to_string(), 4));
    // The verb drill: no people, so its slots keep the course's pictures.
    assert!(get("/play/conjugation").contains(r#"data-people="""#));
}
