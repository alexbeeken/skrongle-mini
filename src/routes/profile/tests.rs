use crate::content::CourseFile;

/// The privacy page is public; the export holds the learner's data and
/// no secrets; deleting the account needs the password (or the e-mail address,
/// for accounts without one), signs the
/// account out and schedules the deletion 14 days out (#970): signing in
/// again shows only the page that cancels it, and once due the hourly job
/// leaves nothing to sign in with.
#[test]
fn privacy_page_export_and_account_deletion_over_http() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let state = crate::routes::test_state(false);
    let hash = crate::auth::hash_password("right-password").unwrap();
    let (ann, gil) = state
        .db
        .with(|c| {
            let ann = crate::auth::create_user(c, "ann@example.org", &hash)?;
            let gil = crate::auth::create_user(c, "gil@example.org", "")?;
            let files = [CourseFile { path: "course.json".into(), content: "{\"id\": \"ann-c\", \"note\": \"ann-file\"}".into() }];
            crate::courses::create_course(c, ann, "ann-c", "Ann's", &files)?;
            c.execute(
                "INSERT INTO quiz_history (user_id, course_id, lesson_id, question_id, selected_answer, was_correct, answered_at) VALUES (?1, 'spanish', 'a1', 'q', 'ann-answer', 1, 1)",
                [ann],
            )?;
            Ok((ann, gil))
        })
        .unwrap();
    let key = "sk-ant-api03-0123456789abcdefghijklmnop";
    // A row from before the keys were the server's (#25): the export names
    // the provider and never carries the ciphertext.
    state.db.with(|c| c.execute("INSERT INTO user_api_keys (user_id, provider, ciphertext, updated_at) VALUES (?1, 'anthropic', ?2, 1)", rusqlite::params![ann, key.as_bytes()])).unwrap();
    let session = |u| {
        state.db.with(|c| crate::terms::accept(c, u, crate::terms::version())).unwrap();
        state.db.with(|c| crate::auth::create_session(c, u)).unwrap()
    };
    let (ann_t, gil_t) = (session(ann), session(gil));
    let app = crate::routes::router(state.clone());
    let base = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    });
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    let get = |path: &str, cookie: &str| {
        let mut r = agent.get(&format!("{base}{path}")).header("cookie", cookie).call().unwrap();
        (r.status().as_u16(), r.body_mut().read_to_string().unwrap())
    };
    let post = |path: &str, cookie: &str, form: &[(&str, &str)]| {
        let mut r = agent.post(&format!("{base}{path}")).header("cookie", cookie).send_form(form.iter().copied()).unwrap();
        let cookie = r.headers().get("set-cookie").map(|v| v.to_str().unwrap().to_string()).unwrap_or_default();
        (r.status().as_u16(), cookie, r.body_mut().read_to_string().unwrap())
    };

    // Deletion: the wrong password, refuse it.
    let (status, _, html) = post("/profile/delete", &format!("sid={ann_t}"), &[("password", "wrong")]);
    assert_eq!(status, 200);
    assert!(html.contains("profile-delete-error"));
    let before = crate::db::now();
    let (status, cookie, _) = post("/profile/delete", &format!("sid={ann_t}"), &[("password", "right-password")]);
    assert_eq!(status, 303);
    assert!(cookie.starts_with("sid=;") && cookie.contains("Max-Age=0"));
    let at: i64 = state.db.with(|c| c.query_row("SELECT delete_at FROM users WHERE id = ?1", [ann], |r| r.get(0))).unwrap();
    assert!(at >= before + 14 * 86_400 && at <= crate::db::now() + 14 * 86_400, "14 days out");
    assert_eq!(get("/profile", &format!("sid={ann_t}")).0, 303, "the old session is gone");
    let (_, html) = get(&format!("/login?deletion={at}"), "");
    assert!(html.contains("login-notice") && html.contains(&crate::profile::long_date(at)));

    // Signing in again works, but every page leads to the deletion page,
    // and Cancel deletion keeps the account.
    let r = agent.post(&format!("{base}/login")).send_form([("email", "ann@example.org"), ("password", "right-password")]).unwrap();
    assert_eq!(r.status().as_u16(), 303, "signed in");
    let sid = r.headers().get_all("set-cookie").iter().map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string()).find(|c| c.starts_with("sid=")).unwrap();
    let r = agent.get(&format!("{base}/profile")).header("cookie", &sid).call().unwrap();
    assert_eq!(r.status().as_u16(), 303);
    assert!(r.headers().get("location").unwrap().to_str().unwrap().ends_with("/account/deletion"));
    let r = agent.get(&format!("{base}/api/progress")).header("cookie", &sid).call().unwrap();
    assert_eq!(r.status().as_u16(), 403, "the API refuses too");
    let (status, html) = get("/account/deletion", &sid);
    assert_eq!(status, 200);
    assert!(html.contains("account-deletion-cancel") && html.contains(&crate::profile::long_date(at)));
    assert_eq!(post("/account/deletion/cancel", &sid, &[]).0, 303);
    assert_eq!(get("/profile", &sid).0, 200, "cancelled: the account works again");
    assert_eq!(get("/account/deletion", &sid).0, 303, "nothing to cancel");

    // Asked again; the hourly job deletes it only once it is due.
    let (status, _, _) = post("/profile/delete", &sid, &[("password", "right-password")]);
    assert_eq!(status, 303);
    assert_eq!(crate::account_deletion::sweep(&state, &state.db).unwrap(), "", "not due yet");
    state.db.with(|c| c.execute("UPDATE users SET delete_at = ?2 WHERE id = ?1", [ann, crate::db::now() - 1])).unwrap();
    assert!(crate::account_deletion::sweep(&state, &state.db).unwrap().starts_with("1 account"));
    let left: i64 = state
        .db
        .with(|c| c.query_row("SELECT (SELECT COUNT(*) FROM users WHERE id = ?1) + (SELECT COUNT(*) FROM courses) + (SELECT COUNT(*) FROM user_api_keys)", [ann], |r| r.get(0)))
        .unwrap();
    assert_eq!(left, 0);
    let (status, _, _) = post("/login", "", &[("email", "ann@example.org"), ("password", "right-password")]);
    assert_eq!(status, 200, "signing in fails (the form is shown again)");

    // An account without a password types its e-mail address (no second
    // step, so nothing more).
    let (status, _, html) = post("/profile/delete", &format!("sid={gil_t}"), &[("confirm", "nope")]);
    assert_eq!(status, 200);
    assert!(html.contains("Type your e-mail address"));
    assert_eq!(post("/profile/delete", &format!("sid={gil_t}"), &[("confirm", " GIL@example.org ")]).0, 303);
    let scheduled: bool = state.db.with(|c| c.query_row("SELECT delete_at IS NOT NULL FROM users WHERE id = ?1", [gil], |r| r.get(0))).unwrap();
    assert!(scheduled, "scheduled, not deleted at once");
}

/// An owner cannot delete their own account (#15): the profile shows the
/// line saying why instead of the form, and a POST by hand, even with the
/// right password, is refused with a 403 and schedules nothing. Anyone else
/// still gets the form.
#[test]
fn an_owner_cannot_delete_their_own_account() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let state = crate::routes::test_state(false);
    let hash = crate::auth::hash_password("right-password").unwrap();
    let (root_t, bob_t, root) = state
        .db
        .with(|c| {
            let root = crate::auth::create_user(c, "root@example.org", &hash)?;
            let bob = crate::auth::create_user(c, "bob@example.org", &hash)?;
            crate::owner::make_owner(c, root)?;
            for u in [root, bob] {
                crate::terms::accept(c, u, crate::terms::version())?;
            }
            Ok((crate::auth::create_session(c, root)?, crate::auth::create_session(c, bob)?, root))
        })
        .unwrap();
    let app = crate::routes::router(state.clone());
    let base = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    });
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    let get = |cookie: &str| agent.get(&format!("{base}/profile")).header("cookie", cookie).call().unwrap().body_mut().read_to_string().unwrap();
    let (root_c, bob_c) = (format!("sid={root_t}"), format!("sid={bob_t}"));

    let page = get(&root_c);
    assert!(page.contains(r#"data-testid="profile-delete-owner""#) && !page.contains("profile-delete-form"), "{page}");
    let page = get(&bob_c);
    assert!(page.contains("profile-delete-form") && !page.contains("profile-delete-owner"), "{page}");

    let mut r = agent.post(&format!("{base}/profile/delete")).header("cookie", &root_c).send_form([("password", "right-password")]).unwrap();
    assert_eq!(r.status().as_u16(), 403);
    assert!(r.body_mut().read_to_string().unwrap().contains("can't be deleted here"));
    let at: Option<i64> = state.db.with(|c| c.query_row("SELECT delete_at FROM users WHERE id = ?1", [root], |r| r.get(0))).unwrap();
    assert_eq!(at, None, "nothing scheduled");
    assert!(get(&root_c).contains("profile-delete-owner"), "still signed in");
}

/// Account changes a stolen session must not make (#374, #375, #376):
/// turning two-factor on needs proof, signs out the other devices and
/// e-mails the account; "Sign out everywhere else" ends every other
/// session; an account without a password but with a second step deletes
/// with its code.
#[test]
fn account_changes_need_proof_and_sign_out_other_devices() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let listener = rt.block_on(tokio::net::TcpListener::bind("127.0.0.1:0")).unwrap();
    let addr = listener.local_addr().unwrap();
    let state = crate::routes::test_state(true);
    let hash = crate::auth::hash_password("right-password").unwrap();
    let (ann, gil) = state
        .db
        .with(|c| {
            let ann = crate::auth::create_user(c, "ann@example.org", &hash)?;
            let gil = crate::auth::create_user(c, "gil@example.org", "")?;
            for u in [ann, gil] {
                crate::terms::accept(c, u, crate::terms::version())?;
            }
            c.execute("UPDATE users SET email_verified_at = 1 WHERE id IN (?1, ?2)", [ann, gil])?;
            Ok((ann, gil))
        })
        .unwrap();
    let session = |u| state.db.with(|c| crate::auth::create_session(c, u)).unwrap();
    let alive = |t: &str| state.db.with(|c| Ok(crate::auth::user_for_session(c, t)?.is_some())).unwrap();
    let app = crate::routes::router(state.clone());
    rt.spawn(async move { axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap() });
    let base = format!("http://{addr}");
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    // (status, Location, first Set-Cookie pair, body)
    let send = |r: Result<ureq::http::Response<ureq::Body>, ureq::Error>| {
        let mut r = r.unwrap();
        let header = |k: &str| r.headers().get(k).map(|v| v.to_str().unwrap().to_string()).unwrap_or_default();
        let (location, cookie) = (header("location"), header("set-cookie").split(';').next().unwrap_or_default().to_string());
        (r.status().as_u16(), location, cookie, r.body_mut().read_to_string().unwrap())
    };
    let post = |path: &str, cookie: &str, form: &[(&str, &str)]| send(agent.post(&format!("{base}{path}")).header("cookie", cookie).send_form(form.iter().copied()));
    let (ann_t, ann_other) = (session(ann), session(ann));
    let ann_c = format!("sid={ann_t}");
    assert!(alive(&ann_t) && alive(&ann_other));

    // "Sign out everywhere else".
    let ann_other = session(ann);
    let (status, location, _, _) = post("/profile/sessions/end-others", &ann_c, &[]);
    assert_eq!((status, location.as_str()), (303, "/profile?saved=signed-out#account"));
    assert!(alive(&ann_t) && !alive(&ann_other));

    // Turning two-factor on needs the password (#374); done, it signs
    // out the other devices and e-mails the account.
    let ann_other = session(ann);
    let method = || state.db.with(|c| crate::two_factor::settings(c, ann)).unwrap().method;
    assert_eq!(post("/profile/two-factor", &ann_c, &[("method", "email")]).0, 403);
    assert_eq!(post("/profile/two-factor", &ann_c, &[("method", "totp")]).0, 403);
    assert!(!state.db.with(|c| crate::two_factor::settings(c, ann)).unwrap().totp_pending, "no enrolment without the password");
    assert_eq!(method(), None);
    assert_eq!(post("/profile/two-factor", &ann_c, &[("method", "email"), ("password", "right-password")]).0, 303);
    assert_eq!(method(), Some(crate::two_factor::Method::Email));
    assert!(alive(&ann_t) && !alive(&ann_other));
    let told: i64 = state.db.with(|c| c.query_row("SELECT COUNT(*) FROM mail_outbox WHERE user_id = ?1 AND purpose = 'two-factor-changed'", [ann], |r| r.get(0))).unwrap();
    assert_eq!(told, 1);

    // An account without a password and without a second step has nothing
    // to prove (the proxy that signs it in vouches for it); with one, a
    // change takes a code from it.
    let gil_t = session(gil);
    let gil_c = format!("sid={gil_t}");
    state.db.with(|c| c.execute("UPDATE users SET email_verified_at = 1 WHERE id = ?1", [gil])).unwrap();
    assert_eq!(post("/profile/two-factor", &gil_c, &[("method", "email")]).0, 303);
    let gil_method = || state.db.with(|c| crate::two_factor::settings(c, gil)).unwrap().method;
    assert_eq!(gil_method(), Some(crate::two_factor::Method::Email));
    assert_eq!(post("/profile/two-factor", &gil_c, &[("method", "")]).0, 403, "turning it off takes a code");

    // With a second step, deleting it takes a code from that step (#376).
    let (status, _, _, body) = post("/profile/delete", &gil_c, &[("confirm", "gil@example.org")]);
    assert_eq!(status, 200);
    assert!(body.contains("profile-delete-code") && body.contains("profile-delete-send"));
    let (status, _, _, body) = post("/profile/two-factor/code", &gil_c, &[("then", "delete")]);
    assert_eq!(status, 200);
    assert!(body.contains("profile-delete-sent"));
    let (_, _, code) = state.db.with(|c| crate::two_factor::issue_confirm_code(c, gil, crate::db::now())).unwrap().unwrap();
    let wrong = if code == "000000" { "111111" } else { "000000" };
    assert_eq!(post("/profile/delete", &gil_c, &[("confirm", "gil@example.org"), ("code", wrong)]).0, 200);
    assert!(alive(&gil_t), "not deleted");
    assert_eq!(post("/profile/delete", &gil_c, &[("confirm", "gil@example.org"), ("code", &code)]).0, 303);
    assert!(!alive(&gil_t));
}

/// Autosave: each field saves on its own, a bad one stores nothing and
/// says which field, and the answer carries the name the page shows.
#[test]
fn profile_fields_save_as_they_change() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let state = crate::routes::test_state(false);
    let ann = state.db.with(|c| crate::auth::create_user(c, "ann@example.org", "x")).unwrap();
    state.db.with(|c| crate::terms::accept(c, ann, crate::terms::version())).unwrap();
    let cookie = format!("sid={}", state.db.with(|c| crate::auth::create_session(c, ann)).unwrap());
    let app = crate::routes::router(state.clone());
    let base = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    });
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    let save = |c: &str, body: serde_json::Value| {
        let mut r = agent.post(&format!("{base}/api/profile")).header("cookie", c).send_json(body).unwrap();
        let status = r.status().as_u16();
        (status, serde_json::from_str::<serde_json::Value>(&r.body_mut().read_to_string().unwrap()).unwrap_or_default())
    };
    let stored = || state.db.with(|c| crate::profile::load(c, ann)).unwrap().unwrap();

    let (status, body) = save(&cookie, serde_json::json!({"first_name": "  Ana "}));
    assert_eq!((status, body["display_name"].as_str()), (200, Some("Ana")));
    let (status, body) = save(&cookie, serde_json::json!({"last_name": "López"}));
    assert_eq!((status, body["display_name"].as_str()), (200, Some("Ana López")));

    // One bad field: nothing is stored, and the answer names the field.
    // The picture is a course's setting now (#565), not the profile's.
    let (status, body) = save(&cookie, serde_json::json!({"first_name": "Bea", "last_name": "x".repeat(61), "avatar": "kai"}));
    assert_eq!(status, 422);
    assert!(body["errors"]["last_name"].as_str().unwrap().contains("too long"), "{body}");
    assert!(body["errors"]["avatar"].as_str().unwrap().contains("not part of the profile") && body["errors"].get("first_name").is_none(), "{body}");
    let p = stored();
    assert_eq!((p.first_name.as_str(), p.last_name.as_str()), ("Ana", "López"));
    let (status, body) = save(&cookie, serde_json::json!({"avatar_color": 4}));
    assert!(status == 422 && body["errors"]["avatar_color"].is_string(), "{body}");
    // The old "Mock AI responses" option is not part of the profile any more (#298).
    let (status, body) = save(&cookie, serde_json::json!({"mock_ai": true}));
    assert_eq!(status, 422);
    assert!(body["errors"]["mock_ai"].as_str().unwrap().contains("not part of the profile"), "{body}");
    // The AI API keys are the server's, on Manage Users (#25): the profile refuses them.
    let (status, body) = save(&cookie, serde_json::json!({"key_anthropic": "sk-ant-api03-abcdWXYZ"}));
    assert!(status == 422 && body["errors"]["key_anthropic"].as_str().unwrap().contains("not part of the profile"), "{body}");
    assert_eq!(state.db.with(|c| crate::secrets::saved_key_count(c)).unwrap(), 0);
    // The theme is a course's setting now (#577): the profile refuses it.
    let (status, refused) = save(&cookie, serde_json::json!({"theme": ""}));
    assert!(status == 422 && refused["errors"]["theme"].is_string(), "{refused}");
    // The motivator is a course's setting now (#574): the profile refuses it.
    let (status, refused) = save(&cookie, serde_json::json!({"motivator": crate::motivators::DEFAULT}));
    assert!(status == 422 && refused["errors"]["motivator"].is_string(), "{refused}");
    // Signed out: refused, as JSON.
    assert_eq!(save("", serde_json::json!({"first_name": "Eve"})).0, 401);
    assert_eq!(stored().first_name, "Ana");
}

/// The AI features are the server's (#25): only an owner sees, saves and
/// removes the API keys, on Manage Users, and every account uses them; the
/// owner's "Hide AI features for everyone" hides the tutor, generation and
/// Use AI from every account, whose routes then refuse (pages a 403 page,
/// JSON a 403), and showing them again brings them back.
#[test]
fn the_ai_keys_and_the_hide_ai_switch_are_the_owners_over_http() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let state = crate::routes::test_state(true);
    let app = crate::routes::router(state.clone());
    let base = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    });
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    let get = |path: &str, cookie: &str| {
        let mut r = agent.get(&format!("{base}{path}")).header("cookie", cookie).call().unwrap();
        let location = r.headers().get("location").map(|v| v.to_str().unwrap().to_string()).unwrap_or_default();
        (r.status().as_u16(), location, r.body_mut().read_to_string().unwrap())
    };
    let post = |path: &str, cookie: &str, form: &[(&str, &str)]| {
        let mut r = agent.post(&format!("{base}{path}")).header("cookie", cookie).send_form(form.iter().copied()).unwrap();
        let location = r.headers().get("location").map(|v| v.to_str().unwrap().to_string()).unwrap_or_default();
        (r.status().as_u16(), location, r.body_mut().read_to_string().unwrap())
    };
    let keys = |cookie: &str, body: serde_json::Value| {
        let mut r = agent.post(&format!("{base}/api/manage/keys")).header("cookie", cookie).send_json(body).unwrap();
        let status = r.status().as_u16();
        (status, serde_json::from_str::<serde_json::Value>(&r.body_mut().read_to_string().unwrap()).unwrap_or_default())
    };
    let account = |email: &str, owner: bool| {
        let id = state.db.with(|c| crate::auth::create_user(c, email, "x")).unwrap();
        state.db.with(|c| {
            crate::terms::accept(c, id, crate::terms::version())?;
            c.execute("UPDATE users SET active_course = 'spanish', owner = ?2 WHERE id = ?1", rusqlite::params![id, owner])?;
            Ok(())
        })
        .unwrap();
        format!("sid={}", state.db.with(|c| crate::auth::create_session(c, id)).unwrap())
    };
    let ann = account("ann@example.org", true);
    let bob = account("bob@example.org", false);

    // Nobody has a keys card on the profile, and the old routes are gone.
    for c in [&ann, &bob] {
        let profile = get("/profile", c).2;
        assert!(!profile.contains("api-key") && !profile.contains("consent-ai"), "{profile}");
    }
    assert_eq!(post("/profile/ai", &ann, &[("off", "1")]).0, 404);
    // Only the owner sees Manage Users' AI section and may change it.
    let manage = get("/manage", &ann).2;
    assert!(manage.contains(r#"data-testid="manage-ai-card""#) && manage.contains(r#"data-ai-off="false""#), "{manage}");
    assert_eq!(manage.matches(r#"data-testid="manage-api-key""#).count(), crate::secrets::Provider::ALL.len());
    assert_eq!(get("/manage", &bob).0, 404);
    assert_eq!(keys(&bob, serde_json::json!({"key_anthropic": "sk-ant-api03-abcdWXYZ"})).0, 404);
    assert_eq!(post("/manage/ai", &bob, &[("off", "1")]).0, 404);
    assert_eq!(post("/manage/api-keys/remove", &bob, &[("provider", "anthropic")]).0, 404);

    // No key yet: Bob is told to ask the owner, Ann is sent to Manage Users.
    assert!(get("/tutor", &bob).2.contains("Ask the owner of this server"));
    assert!(get("/tutor", &ann).2.contains(r#"href="/manage#ai""#));
    // A key: checked as before, stored for the server, answered with its last four characters only.
    let (status, body) = keys(&ann, serde_json::json!({"key_openai": "sk-ant-wrong-box"}));
    assert!(status == 422 && body["errors"]["key_openai"].is_string(), "{body}");
    assert_eq!(keys(&ann, serde_json::json!({"key_nobody": "x"})).0, 422);
    let (status, body) = keys(&ann, serde_json::json!({"key_anthropic": "sk-ant-api03-abcdWXYZ"}));
    assert_eq!((status, body["keys"]["anthropic"].as_str()), (200, Some("WXYZ")), "{body}");
    assert!(!body.to_string().contains("abcd"));
    assert!(get("/manage", &ann).2.contains("…WXYZ"));
    // Bob uses it: his tutor page offers it.
    let tutor = get("/tutor", &bob).2;
    assert!(!tutor.contains("tutor-no-key") && tutor.contains("ai-disclosure"), "{tutor}");
    assert!(get("/", &bob).2.contains("home-start-tutor"));

    // Hidden for everyone: no tutor, no generation, no AI form, for Bob and for Ann.
    let (status, location, _) = post("/manage/ai", &ann, &[("off", "1")]);
    assert_eq!((status, location.as_str()), (303, "/manage?did=ai-off#ai"));
    assert!(state.db.with(|c| crate::settings::ai_off(c)).unwrap());
    assert!(get("/manage?did=ai-off", &ann).2.contains("AI features are hidden for every account"));
    for c in [&ann, &bob] {
        let home = get("/", c).2;
        assert!(!home.contains("home-start-tutor") && !home.contains("/tutor"), "the home page offers the tutor");
        let courses = get("/courses", c).2;
        assert!(!courses.contains("Have AI write it") && courses.contains("courses-create"), "the picker offers generation");
        let lesson = get("/lessons/adjectives-agreement-position", c).2;
        assert!(!lesson.contains("lesson-tutor") && !lesson.contains("/tutor?"), "the lesson page offers the tutor");
    }
    let new_course = |cookie: &str| {
        let (_, loc, _) = post("/courses/new", cookie, &[]);
        loc.strip_prefix("/courses/").and_then(|l| l.strip_suffix("?generate=1")).expect("to the editor").to_string()
    };
    let bob_course = new_course(&bob);
    let editor = get(&format!("/courses/{bob_course}?generate=1"), &bob).2;
    assert!(editor.contains(r#"data-testid="course-edit""#) && !editor.contains(r#"data-testid="generate""#) && !editor.contains(r#"data-testid="course-ai""#), "{editor}");
    // The routes refuse: a page says so with a 403, JSON gets a 403.
    for path in ["/tutor", "/courses/generate/nope"] {
        let (status, _, body) = get(path, &bob);
        assert_eq!(status, 403, "{path}");
        assert!(body.contains("turned off on this server"), "{body}");
    }
    assert_eq!(post(&format!("/courses/{bob_course}/generate"), &bob, &[("provider", "anthropic"), ("wish", "x")]).0, 403);
    assert_eq!(post("/courses/spanish/ai", &bob, &[("text", "add a lesson"), ("mode", "add")]).0, 403);
    for path in ["/api/tutor/session?id=x", "/api/generate/nope"] {
        let (status, _, body) = get(path, &bob);
        assert_eq!(status, 403, "{path}");
        assert!(body.contains("AI features are off"), "{body}");
    }
    let status = agent.post(&format!("{base}/api/tutor/start")).header("cookie", &bob).send_json(serde_json::json!({"provider": "anthropic", "lesson": ""})).unwrap().status().as_u16();
    assert_eq!(status, 403);
    // The keys stay, for when the features are shown again.
    assert_eq!(state.db.with(|c| crate::secrets::saved_key_count(c)).unwrap(), 1);
    assert!(get("/manage", &ann).2.contains(r#"data-ai-off="true""#));
    assert_eq!(post("/manage/ai", &ann, &[("off", "maybe")]).0, 400);

    // Shown again: Bob has them back.
    let (status, location, _) = post("/manage/ai", &ann, &[("off", "0")]);
    assert_eq!((status, location.as_str()), (303, "/manage?did=ai-on#ai"));
    assert!(get("/", &bob).2.contains("home-start-tutor"));
    assert!(get(&format!("/courses/{bob_course}?generate=1"), &bob).2.contains(r#"data-testid="generate""#));

    // Removing the key: the forms ask for one again.
    let (status, location, _) = post("/manage/api-keys/remove", &ann, &[("provider", "anthropic")]);
    assert_eq!((status, location.as_str()), (303, "/manage?did=key-removed&provider=anthropic#ai"));
    assert!(get("/manage?did=key-removed&provider=anthropic", &ann).2.contains("key removed"));
    assert_eq!(state.db.with(|c| crate::secrets::saved_key_count(c)).unwrap(), 0);
    assert!(get("/tutor", &bob).2.contains("tutor-no-key"));
    assert_eq!(post("/manage/ai", "", &[("off", "1")]).0, 303, "signed out: to the login page");
}
