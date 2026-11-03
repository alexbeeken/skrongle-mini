use super::*;

/// The session of the dev user every `state` has (a developer; the dev
/// pages themselves need no sign-in).
const DEV_TOKEN: &str = "d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0";

fn state(dev: bool) -> Arc<AppState> {
    let s = crate::routes::test_state(dev);
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

#[test]
fn dev_routes_404_unless_dev_mode_is_on() {
    let off = statuses(false, &["/dev/login", "/dev/reset", "/dev/ai", "/healthz"]);
    assert_eq!(off.iter().map(|s| s.0).collect::<Vec<_>>(), vec![404, 404, 404, 200]);
    let on = statuses(true, &["/dev/login?open=spanish&keys=fake&next=/courses/generate", "/dev/login?open=nope", "/dev/ai", "/dev/reset", "/dev/inbox", "/dev/mail"]);
    assert_eq!(on[0], (303, "/courses/generate +cookie".to_string()));
    assert_eq!(on[1].0, 400);
    assert_eq!(on[2].0, 200);
    assert_eq!(on[3].0, 303);
    assert_eq!((on[4].0, on[5].0), (404, 404), "no dev mailbox (#10)");
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
