use super::*;
use crate::db::Db;
use crate::signup::{admit, finish, Admit, Mode, Refusal};
use std::net::SocketAddr;

/// A sender that queues nothing and says yes, recording who it was asked to mail.
fn recorder(sent: &mut Vec<Invitation>) -> impl FnMut(&Connection, &Invitation) -> rusqlite::Result<bool> + '_ {
    move |_, inv| {
        sent.push(inv.clone());
        Ok(true)
    }
}

fn confirmed(c: &Connection, email: &str, now: i64) -> rusqlite::Result<()> {
    let Joined::Confirm(token) = join(c, email, now)? else { panic!("{email} joins") };
    assert!(confirm(c, &token, now)?);
    Ok(())
}

fn status_of(c: &Connection, email: &str, now: i64) -> Status {
    list(c, 1, now).unwrap().0.into_iter().find(|e| e.email == email).unwrap().status
}

/// Joining twice before confirming makes a new token (the old one stops
/// working); once confirmed, or with an account, joining sends nothing.
#[test]
fn joining_and_confirming() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let Joined::Confirm(first) = join(c, "ann@example.org", 1)? else { panic!() };
        let Joined::Confirm(second) = join(c, "ann@example.org", 2)? else { panic!() };
        assert_eq!(status_of(c, "ann@example.org", 2), Status::Unconfirmed);
        assert!(!confirm(c, &first, 3)?, "the older token is gone");
        assert!(!confirm(c, "nonsense", 3)?);
        assert!(confirm(c, &second, 3)?);
        assert!(!confirm(c, &second, 4)?, "single use");
        assert_eq!(join(c, "ann@example.org", 5)?, Joined::Nothing);
        assert_eq!(status_of(c, "ann@example.org", 5), Status::Waiting);
        let stored: Option<String> = c.query_row("SELECT confirm_hash FROM waitlist", [], |r| r.get(0))?;
        assert_eq!(stored, None, "the token's hash goes once used");

        crate::auth::create_user(c, "bo@example.org", "h")?;
        assert_eq!(join(c, "bo@example.org", 5)?, Joined::Nothing, "an account already");
        assert_eq!(counts(c)?, Counts { total: 1, unconfirmed: 0, waiting: 1, invited: 0, signed_up: 0 });
        Ok(())
    })
    .unwrap();
}

/// "Invite the next N" takes confirmed people in the order they joined,
/// skips the unconfirmed, the invited and those with an account, and
/// stops where the sender says no.
#[test]
fn inviting_the_next_ones() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let root = crate::auth::create_user(c, "root@example.org", "h")?;
        for e in ["a@example.org", "b@example.org", "c@example.org", "d@example.org"] {
            confirmed(c, e, 10)?;
        }
        join(c, "unconfirmed@example.org", 10)?;
        crate::auth::create_user(c, "b@example.org", "h")?;
        let mut sent = Vec::new();
        assert_eq!(invite_next(c, 2, root, 20, &mut recorder(&mut sent))?, 2);
        assert_eq!(sent.iter().map(|i| i.email.as_str()).collect::<Vec<_>>(), ["a@example.org", "c@example.org"]);
        assert_eq!(status_of(c, "a@example.org", 20), Status::Invited);
        assert_eq!(status_of(c, "d@example.org", 20), Status::Waiting);

        // The mailer's ceiling: nothing more after the first no.
        let mut no = |_: &Connection, _: &Invitation| Ok(false);
        assert_eq!(invite_next(c, 10, root, 21, &mut no)?, 0);
        assert_eq!(status_of(c, "d@example.org", 21), Status::Waiting);
        let open: i64 = c.query_row("SELECT COUNT(*) FROM signup_invites", [], |r| r.get(0))?;
        assert_eq!(open, 2, "the refused one left no invitation behind");

        // Each invitation is tied to its address and runs out.
        assert_eq!(invite_email(c, c.query_row("SELECT invite_id FROM waitlist WHERE email = 'a@example.org'", [], |r| r.get(0))?)?.as_deref(), Some("a@example.org"));
        assert_eq!(status_of(c, "a@example.org", 20 + INVITE_TTL_SECS), Status::Expired);
        Ok(())
    })
    .unwrap();
}

/// Resend replaces the old invitation: the old link stops working.
#[test]
fn resending_replaces_the_invitation() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let root = crate::auth::create_user(c, "root@example.org", "h")?;
        confirmed(c, "ann@example.org", 1)?;
        let id: i64 = c.query_row("SELECT id FROM waitlist", [], |r| r.get(0))?;
        let mut sent = Vec::new();
        assert!(invite_one(c, id, root, 2, &mut recorder(&mut sent))?);
        assert!(invite_one(c, id, root, 3, &mut recorder(&mut sent))?);
        assert_eq!(sent.len(), 2);
        assert_eq!(admit(c, Mode::Invite, &sent[0].token, None, 4)?, Err(Refusal::Unknown), "the first link is gone");
        assert!(matches!(admit(c, Mode::Invite, &sent[1].token, None, 4)?, Ok(Admit::Invited(_))));
        assert_eq!(c.query_row("SELECT invites_sent FROM waitlist", [], |r| r.get::<_, i64>(0))?, 2);

        join(c, "unconfirmed@example.org", 5)?;
        let other: i64 = c.query_row("SELECT id FROM waitlist WHERE email = 'unconfirmed@example.org'", [], |r| r.get(0))?;
        assert!(!invite_one(c, other, root, 5, &mut recorder(&mut sent))?, "not confirmed: no invitation");
        Ok(())
    })
    .unwrap();
}

/// An e-mailed invitation lets in only its own address (any case), and the
/// account it makes is a beta account and signed up on the list.
#[test]
fn an_invitation_is_for_its_address() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let root = crate::auth::create_user(c, "root@example.org", "h")?;
        confirmed(c, "ann@example.org", 1)?;
        let mut sent = Vec::new();
        invite_next(c, 1, root, 2, &mut recorder(&mut sent))?;
        let token = &sent[0].token;
        assert_eq!(admit(c, Mode::Invite, token, Some("eve@example.org"), 3)?, Err(Refusal::WrongEmail));
        let Ok(a) = admit(c, Mode::Invite, token, Some("Ann@Example.org"), 3)? else { panic!("the address it was sent to") };

        // Even past `admit`, `finish` refuses another address.
        let eve = crate::auth::create_user(c, "eve@example.org", "h")?;
        assert!(!finish(c, a, eve, 3)?);
        let ann = crate::auth::create_user(c, "ann@example.org", "h")?;
        assert!(finish(c, a, ann, 3)?);
        let beta = |id: i64| c.query_row("SELECT beta FROM users WHERE id = ?1", [id], |r| r.get::<_, bool>(0)).unwrap();
        assert!(beta(ann) && !beta(eve) && !beta(root));
        assert_eq!(status_of(c, "ann@example.org", 3), Status::SignedUp);

        // A plain invitation link (#634) makes no beta account.
        let link = crate::signup::make(c, root, 4)?;
        let Ok(b) = admit(c, Mode::Invite, &link, Some("bo@example.org"), 4)? else { panic!() };
        let bo = crate::auth::create_user(c, "bo@example.org", "h")?;
        assert!(finish(c, b, bo, 4)? && !beta(bo));
        Ok(())
    })
    .unwrap();
}

/// Someone on the list who signs up after the launch (no invitation) is
/// marked signed up all the same.
#[test]
fn signing_up_after_the_launch_counts() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        crate::auth::create_user(c, "root@example.org", "h")?;
        confirmed(c, "ann@example.org", 1)?;
        let ann = crate::auth::create_user(c, "ann@example.org", "h")?;
        assert!(finish(c, Admit::Open, ann, 2)?);
        assert_eq!(status_of(c, "ann@example.org", 2), Status::SignedUp);
        Ok(())
    })
    .unwrap();
}

#[test]
fn unconfirmed_addresses_leave_after_a_while() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        join(c, "old@example.org", 0)?;
        confirmed(c, "kept@example.org", 0)?;
        join(c, "new@example.org", UNCONFIRMED_TTL_SECS)?;
        assert_eq!(purge(c, UNCONFIRMED_TTL_SECS + 1)?, 1);
        assert_eq!(counts(c)?.total, 2);
        Ok(())
    })
    .unwrap();
}

/// The open `signup` mode acts as `invite` only while the waitlist is on.
#[test]
fn the_switch_turns_open_into_invite() {
    let db = Db::open_in_memory().unwrap();
    let switch = Switch::default();
    db.with(|c| {
        assert!(!stored(c)?);
        switch.set(c, true)?;
        assert!(stored(c)? && switch.is_on());
        let fresh = Switch::default();
        fresh.load(c, false)?;
        assert!(fresh.is_on(), "a restart keeps it");
        Ok(())
    })
    .unwrap();
    let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
    assert_eq!(mode(Mode::Open), Mode::Open, "outside a request: off");
    rt.block_on(CURRENT.scope((true, false), async {
        assert_eq!(mode(Mode::Open), Mode::Invite);
        assert_eq!(mode(Mode::Closed), Mode::Closed);
        assert!(on() && !shows_login());
    }));
    rt.block_on(CURRENT.scope((true, true), async { assert!(shows_login()) }));
}

/// The whole release over HTTP: a superuser turns the waitlist on; the
/// home page is the e-mail field with no Log in or Create account; joining
/// mails a confirmation link; "Invite the next" mails an invitation; it
/// signs up only its own address, as a beta account; turning the waitlist
/// off brings the normal home page back. Anyone else gets a 404.
#[test]
fn a_release_over_http() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let state = crate::routes::test_state(false);
    let app = crate::routes::router(state.clone());
    let base = rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await.unwrap() });
        format!("http://{addr}")
    });
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
    let get = |path: &str, cookie: &str| {
        let mut r = agent.get(&format!("{base}{path}")).header("cookie", cookie).call().unwrap();
        let loc = r.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
        (r.status().as_u16(), r.body_mut().read_to_string().unwrap(), loc)
    };
    let post = |path: &str, cookie: &str, form: &[(&str, &str)]| {
        let mut r = agent.post(&format!("{base}{path}")).header("cookie", cookie).send_form(form.iter().copied()).unwrap();
        let set = r.headers().get("set-cookie").map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string()).unwrap_or_default();
        let loc = r.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
        (r.status().as_u16(), set, loc, r.body_mut().read_to_string().unwrap_or_default())
    };
    let register = |email: &str, invite: &str| {
        post("/register", "", &[("email", email), ("password", "long-enough-pw"), ("age", "on"), ("accept", "on"), ("invite", invite)])
    };
    let login = |email: &str| post("/login", "", &[("email", email), ("password", "long-enough-pw")]).1;
    register("root@example.org", "");
    register("bob@example.org", "");
    state.db.with(|c| c.execute("UPDATE users SET superuser = (email = 'root@example.org'), email_verified_at = 5", [])).unwrap();
    let (root, bob) = (login("root@example.org"), login("bob@example.org"));
    // The last mail to `to`: its text.
    let last_mail = |to: &str| -> String {
        state.db.with(|c| c.query_row("SELECT body_text FROM mail_outbox WHERE to_addr = ?1 ORDER BY id DESC LIMIT 1", [to], |r| r.get(0))).unwrap()
    };
    let link_in = |text: &str, path: &str| -> String {
        let at = text.find(path).unwrap_or_else(|| panic!("no {path} link in {text}"));
        text[at..].split_whitespace().next().unwrap().to_string()
    };

    for (path, form) in [("/superuser/release/switch", &[("to", "on")][..]), ("/superuser/release/invite-next", &[("n", "5")][..]), ("/superuser/release/invite", &[("id", "1")][..])] {
        assert_eq!(post(path, &bob, form).0, 404, "{path}");
    }
    assert_eq!(get("/superuser/release", &bob).0, 404);
    assert!(get("/superuser", &root).1.contains("superuser-release-card"));
    assert!(get("/", "").1.contains("landing-register"), "off: the normal home page");

    assert_eq!(post("/superuser/release/switch", &root, &[("to", "on")]).2, "/superuser/release?did=on");
    let (_, home, _) = get("/", "");
    assert!(home.contains("waitlist-form"), "{home}");
    for gone in ["nav-register", "nav-login", "waitlist-login", "landing-register"] {
        assert!(!home.contains(gone), "{gone} hidden while the waitlist is on");
    }
    assert!(!get("/login", "").1.contains("/register"), "the login page has no sign-up link either");
    assert_eq!(get("/register", "").2, "/", "no invitation: the waitlist");
    assert_eq!(register("eve@example.org", "").2, "/");

    // Joining: the same page whoever joins; a confirmation link for a new address only.
    let joined = post("/waitlist", "", &[("email", "Ann@Example.org")]);
    assert!(joined.3.contains("waitlist-joined"));
    assert!(post("/waitlist", "", &[("email", "bob@example.org")]).3.contains("waitlist-joined"), "an account: the same page");
    assert!(post("/waitlist", "", &[("email", "not an address")]).3.contains("waitlist-error"));
    let confirm = link_in(&last_mail("ann@example.org"), "/waitlist/confirm?token=");
    let path = confirm.trim_start_matches(base.as_str()).to_string();
    assert!(get(&path, "").1.contains("waitlist-confirmed"));
    assert!(get(&path, "").1.contains("waitlist-bad-link"), "single use");

    let page = get("/superuser/release", &root).1;
    assert!(page.contains(r#"data-email="ann@example.org" data-status="waiting""#), "{page}");
    assert_eq!(post("/superuser/release/invite-next", &root, &[("n", "100")]).2, "/superuser/release?did=invited-n&n=1");
    let invite = link_in(&last_mail("ann@example.org"), "/register?invite=");
    let token = invite.split("invite=").nth(1).unwrap();
    assert!(get(&format!("/register?invite={token}"), "").1.contains(r#"value="ann@example.org""#), "the address is filled in");
    assert_eq!(register("eve@example.org", token).0, 403, "another address");
    assert_eq!(register("ann@example.org", token).0, 200, "the check-your-mail page");
    let beta: bool = state.db.with(|c| c.query_row("SELECT beta FROM users WHERE email = 'ann@example.org'", [], |r| r.get(0))).unwrap();
    assert!(beta);
    assert!(get("/superuser/release", &root).1.contains(r#"data-email="ann@example.org" data-status="signed-up""#));

    assert_eq!(post("/superuser/release/switch", &root, &[("to", "off")]).2, "/superuser/release?did=off");
    assert!(get("/", "").1.contains("landing-register"), "the launch: the normal home page");
    assert!(post("/waitlist", "", &[("email", "late@example.org")]).2.ends_with('/'), "the waitlist form is gone");
}
