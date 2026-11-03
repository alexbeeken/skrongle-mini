use super::*;

#[test]
fn the_dev_authenticator_adds_replaces_and_removes() {
    let a = Authenticator::default();
    let key = crate::totp::Key::parse("otpauth://totp/Skrongle:ann@example.org?secret=JBSWY3DPEHPK3PXP&issuer=Skrongle").unwrap();
    let first = a.add(key.clone(), 1);
    let other = a.add(crate::totp::Key::parse("GEZDGNBVGY3TQOJQ").unwrap(), 2);
    assert_eq!(a.list().len(), 2);
    // The same QR again replaces the old entry.
    let again = a.add(key, 3);
    assert_ne!(again, first);
    assert_eq!(a.list().iter().map(|x| x.id).collect::<Vec<_>>(), vec![other, again]);
    assert!(a.get(first).is_none());
    assert!(a.remove(other));
    assert!(!a.remove(other));
    assert_eq!(a.list().len(), 1);
}

/// `--watch`: HTML pages get the reload poller; other responses do not.
#[test]
fn reload_poller_goes_into_html_only() {
    use axum::routing::get;
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
    let app = axum::Router::new()
        .route("/page", get(|| async { axum::response::Html("<p>hi</p>") }))
        .route("/text", get(|| async { "plain" }))
        .layer(axum::middleware::from_fn(inject_reload));
    let base = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    });
    let get = |path: &str| ureq::get(&format!("{base}{path}")).call().unwrap().body_mut().read_to_string().unwrap();
    let (page, text) = (get("/page"), get("/text"));
    assert!(page.starts_with("<p>hi</p><script>") && page.contains(boot_id()) && page.contains("/dev/boot"), "{page}");
    assert_eq!(text, "plain");
}

fn addr(s: &str) -> SocketAddr {
    s.parse().unwrap()
}

#[test]
fn dev_mode_needs_a_loopback_bind_and_no_secure_cookies() {
    assert_eq!(resolve(false, addr("127.0.0.1:8080"), false, None), Ok(false));
    assert_eq!(resolve(true, addr("127.0.0.1:8080"), false, None), Ok(true));
    assert_eq!(resolve(true, addr("[::1]:8080"), false, None), Ok(true));
    assert!(resolve(true, addr("0.0.0.0:8080"), false, None).unwrap_err().contains("not loopback"));
    assert!(resolve(true, addr("192.168.1.5:8080"), false, None).is_err());
    assert!(resolve(true, addr("127.0.0.1:8080"), true, None).unwrap_err().contains("SECURE_COOKIES"));
    // A deploy binds loopback behind its proxy and has a public URL.
    assert!(resolve(true, addr("127.0.0.1:8080"), false, Some("https://learn.example.org")).unwrap_err().contains("PUBLIC_URL"));
    assert_eq!(resolve(true, addr("127.0.0.1:8080"), false, Some("http://localhost:8080")), Ok(true));
}

/// The dev routes answer only direct local requests, never proxied ones.
#[test]
fn dev_routes_refuse_proxied_and_non_local_requests() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let app = crate::routes::router(crate::routes::test_state(true));
    let base = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    });
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    let get = |headers: &[(&str, &str)]| {
        let mut req = agent.get(&format!("{base}/dev/ai"));
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        req.call().unwrap().status().as_u16()
    };
    assert_eq!(get(&[]), 200);
    assert_eq!(get(&[("x-forwarded-for", "203.0.113.9")]), 404);
    assert_eq!(get(&[("x-real-ip", "203.0.113.9")]), 404);
    // A foreign Host is refused on every route in dev mode, before `local_only`.
    assert_eq!(get(&[("host", "learn.example.org")]), 421);
}

/// A database with accounts besides the dev user's is a deploy's: dev
/// mode stays off unless `SKRONGLE_DEV_FORCE=1` (#388).
#[test]
fn dev_mode_refuses_a_database_with_other_accounts() {
    let db = Db::open_in_memory().unwrap();
    assert_eq!(db.with(|c| other_accounts(c)).unwrap(), 0);
    db.with(|c| crate::auth::create_user(c, "Dev@Example.com", "x")).unwrap();
    assert_eq!(db.with(|c| other_accounts(c)).unwrap(), 0, "the dev user does not count");
    assert_eq!(check_accounts(0, false), Ok(()));
    db.with(|c| crate::auth::create_user(c, "ann@example.org", "x")).unwrap();
    let n = db.with(|c| other_accounts(c)).unwrap();
    assert_eq!(n, 1);
    assert!(check_accounts(n, false).unwrap_err().contains("SKRONGLE_DEV_FORCE=1"));
    assert_eq!(check_accounts(n, true), Ok(()));
}

/// In dev mode every route, not only `/dev/*`, answers only requests made
/// to localhost / 127.0.0.1 / [::1] (any port); others get 421 (#388).
/// Outside dev mode the Host is not checked.
#[test]
fn dev_mode_answers_only_local_hosts_on_every_route() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let serve = |dev: bool| {
        let app = crate::routes::router(crate::routes::test_state(dev));
        rt.block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            format!("http://{addr}")
        })
    };
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    let status = |base: &str, path: &str, host: Option<&str>| {
        let mut req = agent.get(&format!("{base}{path}"));
        if let Some(h) = host {
            req = req.header("host", h);
        }
        req.call().unwrap().status().as_u16()
    };
    let dev = serve(true);
    for path in ["/healthz", "/login", "/"] {
        let ok = status(&dev, path, None);
        assert_ne!(ok, 421, "{path} from 127.0.0.1");
        for host in ["localhost", "localhost:8123", "LOCALHOST:80", "127.0.0.1", "[::1]:8123"] {
            assert_eq!(status(&dev, path, Some(host)), ok, "{path} as {host}");
        }
        for host in ["attacker.example", "attacker.example:8123", "127.0.0.1.nip.io", "localhost.attacker.example"] {
            assert_eq!(status(&dev, path, Some(host)), 421, "{path} as {host}");
        }
    }
    let prod = serve(false);
    assert_eq!(status(&prod, "/healthz", Some("learn.example.org")), 200);
}

/// The dev pages need no sign-in (they are for whoever is
/// testing); the simulated provider's wire API needs one of its keys,
/// and answers the app's own client in both formats, quota error
/// included (#297).
#[test]
fn the_dev_pages_need_no_sign_in_and_the_simulator_speaks_the_wire_protocols() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let state = crate::routes::test_state(true);
    let (learner, dev) = state
        .db
        .with(|c| {
            let a = crate::auth::create_user(c, "ann@example.org", "x")?;
            let d = crate::auth::create_user(c, "dora@example.org", "x")?;
            crate::owner::make_owner(c, d)?;
            Ok((crate::auth::create_session(c, a)?, crate::auth::create_session(c, d)?))
        })
        .unwrap();
    let app = crate::routes::router(std::sync::Arc::clone(&state));
    let base = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    });
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    let get = |path: &str, cookie: Option<&str>, accept: &str| {
        let mut req = agent.get(&format!("{base}{path}")).header("accept", accept);
        if let Some(t) = cookie {
            req = req.header("cookie", &format!("{}={t}", crate::auth::SESSION_COOKIE));
        }
        req.call().unwrap().status().as_u16()
    };
    for path in ["/dev/authenticator", "/dev/ai", "/dev/ai/rows"] {
        assert!(matches!(get(path, None, "text/html"), 200 | 404), "{path}: signed out, a browser");
        assert!(matches!(get(path, None, "application/json"), 200 | 404), "{path}: signed out");
        assert!(matches!(get(path, Some(&learner), "application/json"), 200 | 404), "{path}: a learner");
        assert!(matches!(get(path, Some(&dev), "application/json"), 200 | 404), "{path}: the dev user");
    }
    assert_eq!(get("/dev/ai?format=json", None, "*/*"), 200);
    // The action routes too, signed out.
    let post = agent.post(&format!("{base}/dev/ai/profiles/learner")).send_form([("do", "exhaust")]).unwrap();
    assert!(post.status().is_redirection() || post.status().is_success(), "{}", post.status());
    assert!(state.dev_ai.profile(crate::dev_ai::Who::Learner).exhausted);
    let lift = agent.post(&format!("{base}/dev/ai/profiles/learner")).send_form([("do", "lift")]).unwrap();
    assert!(lift.status().is_redirection() || lift.status().is_success(), "{}", lift.status());
    assert!(!state.dev_ai.profile(crate::dev_ai::Who::Learner).exhausted);

    // The app's own clients against the simulator, as dev mode points them.
    let root = format!("{base}/dev/ai");
    let msgs = [crate::ai::ChatMessage { role: "user", content: "Hola".into() }];
    let call = crate::ai::Call::new("tutor-turn");
    let provider = |kind: crate::ai::ProviderKind, key: &str| crate::ai::Provider { base_url: simulator_base_url(&root, kind), ..kind.provider(key.to_string()) };
    for (kind, key) in [(crate::ai::ProviderKind::Anthropic, "sk-ant-dev-anthropic"), (crate::ai::ProviderKind::OpenAi, "sk-dev-openai"), (crate::ai::ProviderKind::Google, "AIza-dev-google")] {
        let reply = crate::ai::chat(&provider(kind, key), &call, "You are a tutor", &msgs, 100).unwrap();
        assert!(reply.text.contains("[[ask]]"), "{kind:?}: {}", reply.text);
    }
    let err = crate::ai::chat(&provider(crate::ai::ProviderKind::Anthropic, "sk-ant-api03-not-a-dev-key"), &call, "s", &msgs, 100).unwrap_err();
    assert_eq!(err, "the API answered HTTP 401. invalid x-api-key");
    let log = state.dev_ai.log();
    assert_eq!(log.len(), 4);
    assert_eq!((log[1].purpose.as_str(), log[1].format, log[1].status, log[1].fixture.as_str()), ("tutor-turn", crate::dev_ai::Format::Google, 200, "tutor-turn/default"));
    assert_eq!(state.dev_ai.profile(crate::dev_ai::Who::Learner).usage.requests, 3);
    // The providers #24 added speak OpenAI's format under paths of their own
    // (Ollama with no key at all) and get the fixtures' replies too.
    use crate::ai::ProviderKind as K;
    let compatible = [crate::ai::ChatMessage { role: "user", content: "Hola [fixture:compatible]".into() }];
    for (kind, key) in [(K::DeepSeek, "sk-dev-deepseek"), (K::Mistral, "sk-dev-mistral"), (K::Xai, "xai-dev-grok"), (K::OpenRouter, "sk-or-dev-router"), (K::Ollama, "")] {
        let p = crate::ai::Provider { model: kind.tutor_model().to_string(), ..provider(kind, key) };
        let reply = crate::ai::chat(&p, &call, "You are a tutor", &compatible, 100).unwrap();
        assert_eq!(reply.text, "This came back over the OpenAI-compatible wire. [[ask]]", "{kind:?}");
        let entry = &state.dev_ai.log()[0];
        assert_eq!((entry.format, entry.status, entry.fixture.as_str()), (crate::dev_ai::Format::OpenAi, 200, "tutor-turn/compatible"), "{kind:?}");
        assert!(entry.request.contains(&format!("\"model\": \"{}\"", kind.tutor_model())) || entry.request.contains(&format!("\"model\":\"{}\"", kind.tutor_model())), "{kind:?}: {}", entry.request);
        assert!(entry.request.contains("max_tokens") && !entry.request.contains("max_completion_tokens"), "{kind:?}");
    }
    assert_eq!(state.dev_ai.profile(crate::dev_ai::Who::Learner).usage.requests, 8);
    state.dev_ai.clear_log();
    // Over the limit: each provider's own quota error, which the learner reads as such.
    state.dev_ai.update(crate::dev_ai::Who::Learner, |p| p.exhausted = true);
    let err = crate::ai::chat(&provider(crate::ai::ProviderKind::Anthropic, "sk-ant-dev-anthropic"), &call, "s", &msgs, 100).unwrap_err();
    assert!(err.starts_with("the API answered HTTP 400. You have reached your specified API usage limits"), "{err}");
    assert!(crate::ai::plain_failure(&err).unwrap().contains("run out of credit"));
    let err = crate::ai::chat(&provider(crate::ai::ProviderKind::OpenAi, "sk-dev-openai"), &call, "s", &msgs, 100).unwrap_err();
    assert!(err.starts_with("the API answered HTTP 429. You exceeded your current quota"), "{err}");
    // An injected malformed body and a reply that is not JSON for a JSON call.
    state.dev_ai.update(crate::dev_ai::Who::Learner, |p| {
        p.exhausted = false;
        p.inject = Some(crate::dev_ai::Injection { fault: crate::dev_ai::Fault::Malformed, sticky: false, hang_secs: 0 });
    });
    let err = crate::ai::chat(&provider(crate::ai::ProviderKind::Anthropic, "sk-ant-dev-anthropic"), &call, "s", &msgs, 100).unwrap_err();
    assert!(err.contains("not JSON"), "{err}");
}

#[test]
fn the_simulator_stays_off_without_dev_mode() {
    // Whatever SKRONGLE_LLM_MODE says, a non-dev process calls the public APIs.
    std::env::set_var("SKRONGLE_LLM_MODE", "capture");
    assert_eq!(init_llm(false, addr("127.0.0.1:8080")), Ok(None));
    std::env::remove_var("SKRONGLE_LLM_MODE");
}

#[test]
fn fixture_tokens_are_found_anywhere_in_the_prompt() {
    assert_eq!(fixture_token(["sys", "Fractions [fixture:slow] please"]).as_deref(), Some("slow"));
    assert_eq!(fixture_token(["no token", "[fixture:]"]), None);
}

#[test]
fn seed_and_reset_the_dev_user() {
    let db = Db::open_in_memory().unwrap();
    let courses = crate::courses::Courses::load_builtin(Path::new("data")).unwrap();
    let cid = seed(&db, &courses, None).unwrap();
    assert_eq!(cid, SEED_COURSE);
    let uid = ensure_user(&db).unwrap();
    let passed = db.with(|c| crate::progress::passed_ids(c, uid, &cid)).unwrap();
    assert_eq!(passed.len(), 2);
    let verified = db.with(|c| c.query_row("SELECT email_verified_at IS NOT NULL FROM users WHERE id = ?1", [uid], |r| r.get::<_, bool>(0))).unwrap();
    assert!(verified, "the dev user's address is confirmed: no red ! on its profile");
    let owner = |c: &mut Connection| c.query_row("SELECT owner FROM users WHERE id = ?1", [uid], |r| r.get::<_, bool>(0));
    assert!(db.with(owner).unwrap(), "the dev user sees the Manage Users page");
    // Seeding again adds nothing.
    seed(&db, &courses, None).unwrap();
    assert_eq!(db.with(|c| crate::progress::load_all(c, uid, &cid)).unwrap().len(), 3);
    db.with(|c| {
        crate::courses::create_generating(c, uid, "mine-1", "Mine", "b", "")?;
        crate::profile::set_lang(c, uid, "en")?;
        let gone = reset_user(c, uid)?;
        assert_eq!(gone, vec!["mine-1".to_string()]);
        assert!(crate::progress::load_all(c, uid, &cid)?.is_empty());
        assert!(crate::courses::courses_owned_by(c, uid)?.is_empty());
        assert!(crate::courses::projects_of(c, uid)?.is_empty());
        let lang: String = c.query_row("SELECT lang FROM users WHERE id = ?1", [uid], |r| r.get(0))?;
        assert_eq!(lang, "", "a reset forgets the saved language, so the lang cookie decides (#1363)");
        Ok(())
    })
    .unwrap();
    assert!(db.with(owner).unwrap(), "a reset keeps the account an owner");
}
