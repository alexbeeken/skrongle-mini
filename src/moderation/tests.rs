use super::*;

#[test]
fn ranges_parse_and_match() {
    let one = Net::parse(" 203.0.113.7 ").unwrap();
    assert_eq!(one.to_string(), "203.0.113.7");
    assert!(one.contains("203.0.113.7".parse().unwrap()));
    assert!(!one.contains("203.0.113.8".parse().unwrap()));
    // The host bits of a range are dropped; an IPv4-mapped client matches it.
    let range = Net::parse("198.51.100.77/24").unwrap();
    assert_eq!(range.to_string(), "198.51.100.0/24");
    assert!(range.contains("198.51.100.200".parse().unwrap()));
    assert!(range.contains("::ffff:198.51.100.1".parse().unwrap()));
    assert!(!range.contains("198.51.101.1".parse().unwrap()));
    let v6 = Net::parse("2001:db8:1::/48").unwrap();
    assert!(v6.contains("2001:db8:1:2::9".parse().unwrap()));
    assert!(!v6.contains("2001:db8:2::1".parse().unwrap()));
    assert!(!v6.contains("203.0.113.7".parse().unwrap()));
    // Garbage, and ranges wide enough to block much of the internet, are refused.
    for bad in [
        "",
        "example.org",
        "1.2.3",
        "1.2.3.4/33",
        "10.0.0.0/7",
        "::/0",
        "2001:db8::/15",
        "1.2.3.4/x",
    ] {
        assert_eq!(Net::parse(bad), None, "{bad}");
    }
}

#[test]
fn the_blocklist_follows_the_table() {
    let db = crate::db::Db::open_in_memory().unwrap();
    let list = Blocklist::default();
    db.with(|c| {
        assert!(block_net(c, Net::parse("203.0.113.0/24").unwrap(), 1)?);
        assert!(
            !block_net(c, Net::parse("203.0.113.9/24").unwrap(), 2)?,
            "the same range twice"
        );
        list.reload(c)?;
        assert!(list.blocks("203.0.113.50".parse().unwrap()));
        assert!(!list.blocks("192.0.2.1".parse().unwrap()));
        let (id, cidr) = blocked(c)?.remove(0);
        assert_eq!(cidr, "203.0.113.0/24");
        assert_eq!(unblock(c, id)?, Some(cidr));
        assert_eq!(unblock(c, id)?, None);
        list.reload(c)?;
        assert!(!list.blocks("203.0.113.50".parse().unwrap()));
        Ok(())
    })
    .unwrap();
}

/// The superuser page's moderation (#967), over HTTP: only superusers may
/// post it; a course taken down opens for nobody and its owner sees why;
/// a suspended account is signed out, cannot sign in and shares nothing;
/// a deleted account is gone; a blocked address gets a 403 everywhere.
#[test]
fn superusers_moderate_over_http() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let state = crate::routes::test_state(false, None);
    let app = crate::routes::router(state.clone());
    let base = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap()
        });
        format!("http://{addr}")
    });
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .max_redirects(0)
        .build()
        .into();
    let get = |path: &str, cookie: &str| {
        let mut r = agent
            .get(&format!("{base}{path}"))
            .header("cookie", cookie)
            .call()
            .unwrap();
        (r.status().as_u16(), r.body_mut().read_to_string().unwrap())
    };
    let post = |path: &str, cookie: &str, form: &[(&str, &str)]| {
        let mut r = agent
            .post(&format!("{base}{path}"))
            .header("cookie", cookie)
            .send_form(form.iter().copied())
            .unwrap();
        let set = r
            .headers()
            .get("set-cookie")
            .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string())
            .unwrap_or_default();
        let loc = r
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        (
            r.status().as_u16(),
            set,
            loc,
            r.body_mut().read_to_string().unwrap(),
        )
    };
    let account = |email: &str| {
        post(
            "/register",
            "",
            &[
                ("email", email),
                ("password", "long-enough-pw"),
                ("age", "on"),
                ("accept", "on"),
            ],
        );
        post(
            "/login",
            "",
            &[("email", email), ("password", "long-enough-pw")],
        )
        .1
    };
    let (root, bob, carol) = (
        account("root@example.org"),
        account("bob@example.org"),
        account("carol@example.org"),
    );
    let bob_id = state
        .db
        .with(|c| {
            c.execute(
                "UPDATE users SET superuser = 1 WHERE email = 'root@example.org'",
                [],
            )?;
            c.execute("UPDATE users SET email_verified_at = 5", [])?;
            let bob: i64 = c.query_row(
                "SELECT id FROM users WHERE email = 'bob@example.org'",
                [],
                |r| r.get(0),
            )?;
            crate::courses::create_course(
                c,
                bob,
                "bobs-course",
                "Bob's course",
                &crate::courses::starter_files("bobs-course", "Bob's course"),
            )?;
            crate::share::invite(c, "bobs-course", "carol@example.org", 1)?;
            Ok(bob)
        })
        .unwrap();
    let carol_sees = || {
        get("/courses", &carol)
            .1
            .contains(r#"data-course-id="bobs-course""#)
    };
    assert!(carol_sees());

    // Not a superuser: every action is a 404.
    for path in [
        "/superuser/courses/take-down",
        "/superuser/courses/put-back",
        "/superuser/suspend",
        "/superuser/unsuspend",
        "/superuser/delete-account",
        "/superuser/block-ip",
        "/superuser/unblock-ip",
        "/superuser/privacy/export",
        "/superuser/privacy/forget",
    ] {
        assert_eq!(
            post(
                path,
                &bob,
                &[
                    ("course", "bobs-course"),
                    ("email", "carol@example.org"),
                    ("ip", "203.0.113.1"),
                    ("id", "1")
                ]
            )
            .0,
            404,
            "{path}"
        );
    }
    let page = get("/superuser", &root).1;
    assert!(
        page.contains("superuser-courses-card")
            && page.contains("superuser-blocked-card")
            && page.contains("superuser-account-suspend"),
        "{page}"
    );

    // Take the course down: carol loses it, bob sees why, the page lists it.
    let (status, _, loc, _) = post(
        "/superuser/courses/take-down",
        &root,
        &[("course", " bobs-course ")],
    );
    assert_eq!(
        (status, loc.as_str()),
        (303, "/superuser?did=taken-down&about=bobs-course")
    );
    assert!(get(&loc, &root)
        .1
        .contains(r#"data-testid="superuser-course-row" data-course-id="bobs-course" data-state="down""#));
    assert!(!carol_sees());
    let mine = get("/courses", &bob).1;
    assert!(
        mine.contains("courses-card-removed")
            && mine.contains("taken down for breaking the Terms")
            && !mine.contains(r#"action="/courses/open/bobs-course""#),
        "{mine}"
    );
    let (status, editor) = get("/courses/bobs-course", &bob);
    assert!(
        status == 403 && editor.contains("taken down for breaking the Terms"),
        "{status} {editor}"
    );
    assert!(post(
        "/superuser/courses/take-down",
        &root,
        &[("course", "no-such-course")]
    )
    .2
    .contains("did=no-course"));
    // Put back: the share is still there.
    assert!(post(
        "/superuser/courses/put-back",
        &root,
        &[("course", "bobs-course")]
    )
    .2
    .contains("did=put-back"));
    assert!(carol_sees());
    assert_eq!(get("/courses/bobs-course", &bob).0, 200);

    // Suspend bob: signed out, sign-in refused with the notice, his course not shared out.
    assert!(post(
        "/superuser/suspend",
        &root,
        &[("email", "root@example.org")]
    )
    .2
    .contains("did=yourself"));
    assert!(
        post("/superuser/suspend", &root, &[("email", "BOB@example.org")])
            .2
            .contains("did=suspended")
    );
    assert_eq!(get("/profile", &bob).0, 303);
    let (status, _, _, body) = post(
        "/login",
        "",
        &[("email", "bob@example.org"), ("password", "long-enough-pw")],
    );
    assert!(
        status == 403 && body.contains("suspended for breaking the Terms"),
        "{status} {body}"
    );
    assert!(!carol_sees());
    assert!(get("/superuser", &root)
        .1
        .contains("superuser-account-unsuspend"));
    assert!(post(
        "/superuser/unsuspend",
        &root,
        &[("email", "bob@example.org")]
    )
    .2
    .contains("did=unsuspended"));
    let bob = post(
        "/login",
        "",
        &[("email", "bob@example.org"), ("password", "long-enough-pw")],
    )
    .1;
    assert_eq!(get("/profile", &bob).0, 200);
    assert!(carol_sees());

    // Delete bob: a reason is required; then the account and its course
    // are gone, and the address is free.
    assert!(post(
        "/superuser/delete-account",
        &root,
        &[("email", "bob@example.org"), ("reason", " ")]
    )
    .2
    .contains("did=no-reason"));
    assert!(post(
        "/superuser/delete-account",
        &root,
        &[("email", "bob@example.org"), ("reason", "terms breach, case 3")]
    )
    .2
    .contains("did=deleted"));
    assert!(!state
        .db
        .with(|c| c.query_row(
            "SELECT EXISTS(SELECT 1 FROM users WHERE id = ?1)",
            [bob_id],
            |r| r.get::<_, bool>(0)
        ))
        .unwrap());
    assert!(!carol_sees());
    assert_eq!(
        post(
            "/superuser/delete-account",
            &root,
            &[("email", "bob@example.org"), ("reason", "again")]
        )
        .2,
        "/superuser?did=unknown&email=bob%40example.org"
    );

    // Privacy requests (#981): carol's data goes to her own address; an
    // address with an account can't be forgotten, one without can; each
    // action is in the log with its reason.
    let su = |path: &str, email: &str, reason: &str| post(path, &root, &[("email", email), ("reason", reason)]).2;
    assert!(su("/superuser/privacy/export", "carol@example.org", "").contains("did=no-reason"));
    assert_eq!(su("/superuser/privacy/export", "Carol@example.org", "request 7"), "/superuser?did=export-queued&email=carol%40example.org");
    assert!(su("/superuser/privacy/export", "carol@example.org", "request 7").contains("did=export-busy"));
    assert_eq!(su("/superuser/privacy/forget", "carol@example.org", "request 8"), "/superuser?did=has-account&email=");
    assert_eq!(su("/superuser/privacy/forget", "dave@example.org", "request 9"), "/superuser?did=forgot&email=");
    let log = state
        .db
        .with(|c| {
            let mut s = c.prepare("SELECT action || ':' || reason FROM privacy_actions ORDER BY id")?;
            let v: Vec<String> = s.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
            Ok(v)
        })
        .unwrap();
    assert_eq!(log, ["delete-account:terms breach, case 3", "export:request 7", "forget:request 9"]);
    let page = get("/superuser", &root).1;
    assert!(page.contains(r#"data-testid="superuser-privacy-log-row" data-action="forget""#) && page.contains("request 7"), "{page}");

    // Block: a bad range is refused; a range is listed; this test's own address gets a 403 everywhere.
    assert!(post("/superuser/block-ip", &root, &[("ip", "10.0.0.0/4")])
        .2
        .contains("did=bad-ip"));
    assert!(
        post("/superuser/block-ip", &root, &[("ip", "203.0.113.9/24")])
            .2
            .contains("did=blocked&about=203.0.113.0%2F24")
    );
    assert!(get("/superuser", &root)
        .1
        .contains(r#"data-ip="203.0.113.0/24""#));
    assert_eq!(get("/courses", &carol).0, 200);
    post("/superuser/block-ip", &root, &[("ip", "127.0.0.1")]);
    assert_eq!(get("/login", "").0, 403);
    assert_eq!(get("/superuser", &root).0, 403);
    state
        .db
        .with(|c| c.execute("DELETE FROM blocked_ips WHERE cidr = '127.0.0.1'", []))
        .unwrap();
    state.db.with(|c| state.blocked.reload(c)).unwrap();
    assert_eq!(get("/login", "").0, 200);
}
