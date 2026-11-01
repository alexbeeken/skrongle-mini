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

/// The Text section (#225): the dev phone's texts and the SMS outbox as
/// lists with read, archive and delete, a text's page as a conversation,
/// the form that sends a test text; without the phone it says so.
#[test]
fn dev_text_pages_list_open_archive_and_delete() {
    let phone = crate::sms::phone::Phone::new();
    let p = crate::sms::phone::provider(&phone);
    for (id, text) in [(1, "Your code is 111111"), (2, "Your code is 222222")] {
        p.send(&crate::sms::Outgoing { id, to: "+447700900123".into(), text: text.into(), purpose: "test".into() }).unwrap();
    }
    let mut with = state(true);
    Arc::get_mut(&mut with).unwrap().dev_phone = Some(phone.clone());
    let db = with.db.clone();
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
            let rows = |q: &str| -> serde_json::Value { agent.get(&url(&format!("/dev/inbox/rows?kind=sms{q}"))).call().unwrap().body_mut().read_json().unwrap() };

            let json: serde_json::Value = agent.get(&url("/dev/phone?to=%2B44%207700")).call().unwrap().body_mut().read_json().unwrap();
            assert_eq!(json["messages"].as_array().map(Vec::len), Some(2));
            let newest = json["messages"][0]["id"].as_i64().unwrap();
            let list = page(&url("/dev/phone"));
            assert!(list.contains(r#"data-testid="dev-phone-item""#) && list.contains(r#"data-testid="dev-box-kind-sms""#) && list.contains("devbox-frame phone"), "{list}");
            // Both texts are to one number: one row, the newest text.
            assert_eq!(list.matches(r#"data-testid="dev-phone-item""#).count(), 1, "{list}");
            assert!(list.contains("222222") && !list.contains("111111"), "{list}");
            assert_eq!(rows("")["in"]["unread"].as_i64(), Some(1));
            assert!(rows("&to=999")["in"]["html"].as_str().unwrap().contains("No texts to 999."));

            // Its page: the conversation with that number; opening marks it read.
            let one = page(&url(&format!("/dev/phone/{newest}")));
            assert!(one.contains("Your code is 111111") && one.contains("Your code is 222222") && one.contains(r#"data-testid="dev-text-to""#) && one.contains(r#"data-side="in""#) && one.contains(r#"data-testid="dev-text-compose""#), "{one}");
            // A reply from the phone's page is in the conversation (on both sides, below), not an inbox row.
            let number = json["messages"][0]["to"].as_str().unwrap().to_string();
            assert_eq!(agent.post(&url("/dev/phone/reply")).header("x-dev-box", "1").send_form([("to", number.as_str()), ("text", "Got it")]).unwrap().status().as_u16(), 204);
            assert_eq!(agent.post(&url("/dev/phone/reply")).header("x-dev-box", "1").send_form([("to", number.as_str()), ("text", "  ")]).unwrap().status().as_u16(), 400);
            let one = page(&url(&format!("/dev/phone/{newest}")));
            assert!(one.contains("Got it") && one.contains(r#"data-from="phone""#), "{one}");
            let json: serde_json::Value = agent.get(&url("/dev/phone?to=%2B44%207700")).call().unwrap().body_mut().read_json().unwrap();
            assert_eq!(json["messages"].as_array().map(Vec::len), Some(2));
            assert_eq!(rows("")["in"]["unread"].as_i64(), Some(0));
            let act = |path: String, action: &str| agent.post(&url(&path)).header("x-dev-box", "1").send_form([("do", action)]).unwrap().status().as_u16();
            assert_eq!(act(format!("/dev/phone/{newest}"), "archive"), 204);
            assert!(rows("")["in"]["html"].as_str().unwrap().contains(r#"data-archived="true""#));
            // Delete removes the conversation: both texts and the reply.
            assert_eq!(act(format!("/dev/phone/{newest}"), "delete"), 204);
            assert!(phone.list(None).is_empty() && phone.thread(&number).is_empty());
            assert_eq!(act(format!("/dev/phone/{newest}"), "delete"), 404);

            // The outbox: the form queues a text; its page; delete removes the row.
            assert_eq!(agent.post(&url("/dev/sms/send")).send_form([("to", "+15550100"), ("text", "")]).unwrap().status().as_u16(), 303);
            assert_eq!(agent.post(&url("/dev/sms/send")).send_form([("to", "0123"), ("text", "x")]).unwrap().status().as_u16(), 400);
            let out = page(&url("/dev/sms"));
            assert!(out.contains(r#"data-testid="dev-sms-message""#) && out.contains(r#"data-state="queued""#) && out.contains(r#"data-testid="dev-sms-send""#), "{out}");
            let sms: serde_json::Value = agent.get(&url("/dev/sms")).call().unwrap().body_mut().read_json().unwrap();
            let id = sms["messages"][0]["id"].as_i64().unwrap();
            assert!(sms["messages"][0]["text"].as_str().unwrap().contains("test text"));
            let one = page(&url(&format!("/dev/sms/{id}")));
            assert!(one.contains(r#"data-testid="dev-text-state""#) && one.contains("test text") && one.contains(r#"data-side="out""#) && one.contains(r#"data-testid="dev-text-compose""#), "{one}");
            // The outbox page is the conversation with that number: both rows to it, oldest first.
            assert_eq!(agent.post(&url("/dev/sms/send")).header("x-dev-box", "1").send_form([("to", "+15550100"), ("text", "A second one")]).unwrap().status().as_u16(), 204);
            let one = page(&url(&format!("/dev/sms/{id}")));
            assert_eq!(one.matches(r#"data-testid="dev-text-bubble""#).count(), 2, "{one}");
            assert!(one.find("test text").unwrap() < one.find("A second one").unwrap(), "{one}");
            // A second text to the same number joins its conversation: still one row (#252).
            let out = rows("")["out"]["html"].as_str().unwrap().to_string();
            assert_eq!(out.matches(r#"data-testid="dev-sms-message""#).count(), 1, "{out}");
            assert!(out.contains("A second one"), "{out}");
            assert_eq!(rows("")["out"]["unread"].as_i64(), Some(1)); // the second row, not opened
            assert_eq!(act(format!("/dev/sms/{id}"), "archive"), 204);
            assert_eq!(agent.post(&url("/dev/phone/clear")).send_empty().unwrap().status().as_u16(), 303);
            assert!(phone.list(None).is_empty());
            // Emptied (as a restart does), the phone has no copy of the sent rows;
            // on the outbox side they still come first, then what arrived after (there
            // is no job runner here: the row is marked sent and the provider handed the
            // new text by hand).
            db.with(|c| c.execute("UPDATE sms_outbox SET state = 'sent' WHERE to_number = '+15550100'", [])).unwrap();
            assert_eq!(agent.post(&url("/dev/sms/send")).header("x-dev-box", "1").send_form([("to", "+15550100"), ("text", "After the restart")]).unwrap().status().as_u16(), 204);
            let sms: serde_json::Value = agent.get(&url("/dev/sms")).call().unwrap().body_mut().read_json().unwrap();
            let id = sms["messages"][0]["id"].as_i64().unwrap();
            p.send(&crate::sms::Outgoing { id, to: "+15550100".into(), text: "After the restart".into(), purpose: "dev-test".into() }).unwrap();
            let one = page(&url(&format!("/dev/sms/{id}")));
            assert_eq!(one.matches(r#"data-testid="dev-text-bubble""#).count(), 3, "{one}");
            assert!(one.find("test text").unwrap() < one.find("A second one").unwrap() && one.find("A second one").unwrap() < one.find("After the restart").unwrap(), "{one}");
            // Delete removes every row to that number.
            assert_eq!(act(format!("/dev/sms/{id}"), "delete"), 204);
            assert_eq!(agent.get(&url(&format!("/dev/sms/{id}"))).call().unwrap().status().as_u16(), 404);
            assert!(rows("")["out"]["html"].as_str().unwrap().contains("No texts."));

            let off = |p: &str| format!("http://{off_addr}{p}");
            assert!(page(&off("/dev/phone")).contains(r#"data-testid="dev-phone-off""#));
            assert_eq!(agent.get(&off("/dev/phone/1")).call().unwrap().status().as_u16(), 404);
        })
        .await
        .unwrap()
    });
}

/// POST /dev/sms queues through `send_sms` (checks and limits included)
/// and GET /dev/sms shows the row, over the real router.
#[test]
fn dev_sms_queues_and_lists_texts() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let addr = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = crate::routes::router(state(true));
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        addr
    });
    let agent = dev_agent(true);
    let post = |to: &str| {
        let mut r = agent.post(&format!("http://{addr}/dev/sms")).send_json(serde_json::json!({ "to": to, "text": "Code 123456" })).unwrap();
        (r.status().as_u16(), r.body_mut().read_json::<serde_json::Value>().unwrap())
    };
    let (status, first) = post("+44 7700 900123");
    assert_eq!(status, 200, "{first}");
    assert_eq!(post("07700 900123").0, 400);
    for _ in 0..4 {
        assert_eq!(post("+447700900123").0, 200);
    }
    let (status, body) = post("+447700900123");
    assert_eq!(status, 429);
    assert!(body["retry_after"].as_i64().unwrap() > 0);
    let list: serde_json::Value = agent.get(&format!("http://{addr}/dev/sms")).call().unwrap().body_mut().read_json().unwrap();
    let messages = list["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 5);
    assert_eq!(messages[4]["id"], first["id"]);
    assert_eq!((messages[4]["to"].as_str(), messages[4]["text"].as_str(), messages[4]["state"].as_str()), (Some("+447700900123"), Some("Code 123456"), Some("queued")));
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
