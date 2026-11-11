use crate::perms::{self, Perm, Perms};

/// Manage Users for a superuser, and the Permissions dialog's route (#139):
/// a Student gets a 404; a superuser gets the accounts and nothing else of
/// the page, and may post only `/manage/permissions`; the set stored is
/// normalised; nobody changes an owner's permissions, nor a superuser
/// their own.
#[test]
fn superusers_see_the_accounts_and_set_permissions() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let listener = rt.block_on(tokio::net::TcpListener::bind("127.0.0.1:0")).unwrap();
    let addr = listener.local_addr().unwrap();
    let state = crate::routes::test_state(false);
    let (root, sue, stu) = state
        .db
        .with(|c| {
            let root = crate::auth::create_user(c, "root@example.org", "h")?;
            crate::owner::make_owner(c, root)?;
            let sue = crate::auth::create_user(c, "sue@example.org", "h")?;
            perms::set_custom(c, sue, Perms::TEACHER.with(Perm::Superuser, true))?;
            let stu = crate::auth::create_user(c, "stu@example.org", "h")?;
            perms::apply_preset(c, stu, perms::STUDENT_PRESET)?;
            Ok((root, sue, stu))
        })
        .unwrap();
    let cookie = |u| format!("sid={}", state.db.with(|c| crate::auth::create_session(c, u)).unwrap());
    let (root_c, sue_c, stu_c) = (cookie(root), cookie(sue), cookie(stu));
    let stored = |u: i64| state.db.with(|c| c.query_row("SELECT perms, preset_id FROM users WHERE id = ?1", [u], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?)))).unwrap();
    let app = crate::routes::router(state.clone());
    rt.spawn(async move { axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap() });
    let base = format!("http://{addr}");
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    // (status, body)
    let get = |path: &str, cookie: &str| {
        let mut r = agent.get(&format!("{base}{path}")).header("cookie", cookie).call().unwrap();
        (r.status().as_u16(), r.body_mut().read_to_string().unwrap())
    };
    // (status, Location)
    let post = |path: &str, cookie: &str, form: &[(&str, &str)]| {
        let r = agent.post(&format!("{base}{path}")).header("cookie", cookie).send_form(form.iter().copied()).unwrap();
        (r.status().as_u16(), r.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string())
    };
    // The row of the list for one address.
    let row = |page: &str, email: &str| {
        let cell = format!("<td>{email}</td>");
        page.split("<tr ").find(|r| r.contains(&cell)).unwrap_or_else(|| panic!("no row for {email}: {page}")).to_string()
    };

    // A Student: no link, no page, no route.
    assert!(!get("/profile", &stu_c).1.contains("nav-manage"));
    assert_eq!(get("/manage", &stu_c).0, 404);
    assert_eq!(post("/manage/permissions", &stu_c, &[("email", "sue@example.org"), ("preset", "1")]).0, 404);
    assert_eq!(stored(sue), (127, None));

    // A superuser: the link, and the page with the accounts only.
    assert!(get("/profile", &sue_c).1.contains(r#"data-testid="nav-manage""#));
    let (status, page) = get("/manage", &sue_c);
    assert_eq!(status, 200);
    assert!(page.contains(r#"data-testid="manage-accounts-card""#) && page.contains(r#"data-testid="manage-permissions-dialog""#), "{page}");
    for owners_only in ["manage-ai-card", "manage-invites-card", "manage-api-key", "manage-account-temp-password", "manage-account-two-factor-off", "manage-account-make-owner", "manage-password-dialog", "manage-owner-dialog"] {
        assert!(!page.contains(owners_only), "{owners_only} is the owners': {page}");
    }
    // The Permissions column and button: not on an owner's row, nor one's own.
    let stu_row = row(&page, "stu@example.org");
    assert!(stu_row.contains(r#"data-preset="1" data-perms="0""#) && stu_row.contains(">Student</td>") && stu_row.contains("manage-account-permissions"), "{stu_row}");
    let sue_row = row(&page, "sue@example.org");
    assert!(sue_row.contains(r#"data-preset="custom" data-perms="127""#) && sue_row.contains(">Custom</td>") && !sue_row.contains("manage-account-permissions"), "{sue_row}");
    let root_row = row(&page, "root@example.org");
    assert!(root_row.contains(r#"data-preset="owner""#) && root_row.contains(">Owner</td>") && !root_row.contains("manage-account-permissions"), "{root_row}");
    // The dialog: the presets with their sets, then Custom; the seven
    // permissions in order, the dependent ones naming what they need.
    assert!(page.contains(r#"value="1" required data-preset="1" data-perms="0""#) && page.contains(r#"value="2" required data-preset="2" data-perms="63""#) && page.contains(r#"value="custom""#), "{page}");
    let at = |id: &str| page.find(&format!(r#"data-perm="{id}""#)).unwrap_or_else(|| panic!("no line for {id}"));
    assert!(Perm::ALL.windows(2).all(|w| at(w[0].id()) < at(w[1].id())));
    assert!(page.contains(r#"data-perm="ai_courses" data-bit="2" data-needs="create_courses""#) && page.contains(r#"data-perm="share_courses" data-bit="8" data-needs="create_courses""#), "{page}");
    assert_eq!(page.matches("data-needs=").count(), 2);
    assert!(page.contains(r#"name="perm_superuser""#) && !page.contains("manage-permission-ai-hidden"));

    // The owners' routes stay the owners'.
    for path in ["/manage/temp-password", "/manage/two-factor-off", "/manage/make-owner"] {
        assert_eq!(post(path, &sue_c, &[("email", "stu@example.org")]).0, 404, "{path}");
    }
    assert_eq!(post("/manage/invites", &sue_c, &[]).0, 404);
    assert_eq!(post("/manage/ai", &sue_c, &[("off", "1")]).0, 404);

    // Custom: the server normalises; a greyed-out toggle sent anyway does not count.
    let (status, loc) = post("/manage/permissions", &sue_c, &[("email", "stu@example.org"), ("preset", "custom"), ("perm_ai_courses", "on"), ("perm_share_courses", "on"), ("perm_ai_tutor", "on")]);
    assert_eq!((status, loc.as_str()), (303, "/manage?did=permissions&email=stu%40example.org"));
    assert_eq!(stored(stu), (4, None), "ai_courses and share_courses need create_courses");
    let (_, page) = get(&loc, &sue_c);
    assert!(page.contains(r#"data-testid="manage-notice""#) && page.contains("Permissions saved for stu@example.org."), "{page}");
    assert!(row(&page, "stu@example.org").contains(r#"data-preset="custom" data-perms="4""#));
    post("/manage/permissions", &sue_c, &[("email", " STU@example.org "), ("preset", "custom"), ("perm_create_courses", "on"), ("perm_ai_courses", "on"), ("perm_owner", "on")]);
    assert_eq!(stored(stu), (3, None));

    // A preset: its set is copied, the toggles sent with it ignored.
    assert_eq!(post("/manage/permissions", &sue_c, &[("email", "stu@example.org"), ("preset", "2"), ("perm_superuser", "on")]).0, 303);
    assert_eq!(stored(stu), (63, Some(perms::TEACHER_PRESET)));
    assert!(row(&get("/manage", &sue_c).1, "stu@example.org").contains(">Teacher</td>"));
    // No such preset, or none named: a 400 and nothing changed.
    assert_eq!(post("/manage/permissions", &sue_c, &[("email", "stu@example.org"), ("preset", "99")]).0, 400);
    assert_eq!(post("/manage/permissions", &sue_c, &[("email", "stu@example.org"), ("perm_superuser", "on")]).0, 400);
    assert_eq!(stored(stu), (63, Some(perms::TEACHER_PRESET)));
    // An address nobody has.
    let (_, loc) = post("/manage/permissions", &sue_c, &[("email", "nobody@example.org"), ("preset", "1")]);
    assert!(loc.starts_with("/manage?did=unknown"), "{loc}");

    // An owner's permissions are nobody's to change: not a superuser's, not an owner's.
    let root_before = stored(root);
    for by in [&sue_c, &root_c] {
        let (status, loc) = post("/manage/permissions", by, &[("email", "root@example.org"), ("preset", "1")]);
        assert_eq!((status, loc.as_str()), (303, "/manage?did=permissions-owner&email=root%40example.org"));
        assert!(get(&loc, by).1.contains(r#"data-testid="manage-error""#));
    }
    assert_eq!(stored(root), root_before);
    // A superuser does not change their own.
    for form in [&[("email", "sue@example.org"), ("preset", "1")][..], &[("email", "sue@example.org"), ("preset", "custom")][..]] {
        let (status, loc) = post("/manage/permissions", &sue_c, form);
        assert_eq!((status, loc.as_str()), (303, "/manage?did=permissions-self&email=sue%40example.org"));
        assert!(get(&loc, &sue_c).1.contains(r#"data-testid="manage-error""#));
    }
    assert_eq!(stored(sue), (127, None));

    // An owner: the whole page as before, with the Permissions button on the others' rows.
    let (_, page) = get("/manage", &root_c);
    for part in ["manage-accounts-card", "manage-ai-card", "manage-account-temp-password", "manage-password-dialog", "manage-permissions-dialog"] {
        assert!(page.contains(part), "{part}: {page}");
    }
    assert!(row(&page, "sue@example.org").contains("manage-account-permissions") && !row(&page, "root@example.org").contains("manage-account-permissions"));
    // With "Hide AI features" on, the two AI permissions say so.
    assert_eq!(post("/manage/ai", &root_c, &[("off", "1")]).0, 303);
    assert_eq!(get("/manage", &root_c).1.matches(r#"data-testid="manage-permission-ai-hidden""#).count(), 2);

    // An owner takes the superuser permission away: the page is gone for that account at once.
    assert_eq!(post("/manage/permissions", &root_c, &[("email", "sue@example.org"), ("preset", "2")]).0, 303);
    assert_eq!(get("/manage", &sue_c).0, 404);
    assert!(!get("/profile", &sue_c).1.contains("nav-manage"));
}
