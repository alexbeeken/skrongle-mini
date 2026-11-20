use super::*;
use crate::db::Db;

/// A teacher (the Teacher preset, so Hold students) and `n` of its
/// students, `<prefix>1` … `<prefix>n`, all with the password hash `hash`:
/// (teacher id, student ids). For the tests of #218 and #219 too.
pub(crate) fn teacher_with_students(c: &Connection, email: &str, prefix: &str, n: usize, hash: &str) -> rusqlite::Result<(i64, Vec<i64>)> {
    let teacher = crate::perms::create_teacher(c, email, hash)?;
    let rows: Vec<NewStudent> =
        (1..=n).map(|i| NewStudent { username: format!("{prefix}{i}"), password_hash: hash.to_string(), ..Default::default() }).collect();
    let ids = create(c, teacher, &rows, false)?.expect("the teacher holds students");
    Ok((teacher, ids))
}

fn new(username: &str) -> NewStudent {
    NewStudent { username: username.into(), password_hash: "h".into(), first_name: "Ana".into(), last_name: "García".into() }
}

#[test]
fn usernames_follow_the_rule_and_are_stored_lower_case() {
    for (raw, stored) in [("ana.garcia", "ana.garcia"), ("  Ana.Garcia ", "ana.garcia"), ("abc", "abc"), ("7th_grade-1", "7th_grade-1"), (&"a".repeat(32), &"a".repeat(32))] {
        assert_eq!(normalize_username(raw).as_deref(), Ok(stored), "{raw:?}");
    }
    for bad in ["ab", &"a".repeat(33), "ana@school", "ana garcia", ".ana", "-ana", "_ana", "anä", "ana/b", ""] {
        assert!(normalize_username(bad).is_err(), "{bad:?}");
    }
    assert!(normalize_username("x").unwrap_err().contains("3") && normalize_username("x").unwrap_err().contains("32"));
}

#[test]
fn a_name_suggests_a_free_username() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        assert_eq!(suggest_username(c, "Ana García", &[])?, "ana.garcia");
        assert_eq!(suggest_username(c, "  José   Müller-Ñúñez ", &[])?, "jose.mullernunez");
        assert_eq!(suggest_username(c, "Straße", &[])?, "strasse");
        assert_eq!(suggest_username(c, "Al", &[])?, "student", "too short for a username");
        assert_eq!(suggest_username(c, "李明", &[])?, "student", "nothing a username takes");
        let long = suggest_username(c, &"Bartholomew ".repeat(5), &[])?;
        assert!(long.len() <= USERNAME_MAX - 3 && normalize_username(&long).is_ok(), "{long}");
        // Taken on the server, or by another row of the same batch: a number on the end.
        let (t, _) = teacher_with_students(c, "t@example.org", "x", 0, "h")?;
        create(c, t, &[new("ana.garcia")], false)?.unwrap();
        assert_eq!(suggest_username(c, "Ana García", &[])?, "ana.garcia2");
        assert_eq!(suggest_username(c, "Ana García", &["ana.garcia2".into()])?, "ana.garcia3");
        Ok(())
    })
    .unwrap();
}

#[test]
fn made_passwords_are_eight_easy_characters() {
    let mut seen = std::collections::HashSet::new();
    for _ in 0..200 {
        let p = make_password();
        assert_eq!(p.chars().count(), 8, "{p}");
        assert!(p.bytes().all(|b| PASSWORD_CHARS.contains(&b)), "{p}");
        assert!(!p.contains(['0', 'o', '1', 'l', 'i']), "{p}");
        assert!(crate::auth::validate_password(&p).is_ok());
        seen.insert(p);
    }
    assert!(seen.len() > 190, "made passwords repeat");
}

/// Only an account with Hold students (or an owner) holds students; a
/// batch is made whole or not at all, within the student limit (#216).
#[test]
fn students_are_made_all_or_none_within_the_cap() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let plain = crate::auth::create_user(c, "stu@example.org", "h")?;
        assert_eq!(create(c, plain, &[new("ana")], false)?, Err(Refused::NotTeacher), "a new account starts as Student");
        let owner = crate::auth::create_user(c, "root@example.org", "h")?;
        c.execute("UPDATE users SET owner = 1 WHERE id = ?1", [owner])?;
        assert!(create(c, owner, &[new("owner.s")], false)?.is_ok(), "an owner may whatever is stored");
        let (t, _) = teacher_with_students(c, "t@example.org", "x", 0, "h")?;
        assert!(may_hold(c, t)? && !may_hold(c, plain)?);
        // Hold students alone, without the rest of Teacher.
        let only = crate::auth::create_user(c, "only@example.org", "h")?;
        crate::perms::set_custom(c, only, Perms::STUDENT.with(Perm::HoldStudents, true))?;
        assert!(may_hold(c, only)?);

        // Bad and taken usernames refuse the whole batch.
        let users = |c: &Connection| c.query_row("SELECT COUNT(*) FROM users", [], |r| r.get::<_, i64>(0));
        let before = users(c)?;
        assert!(matches!(create(c, t, &[new("ana"), new("a b")], false)?, Err(Refused::BadUsername(..))));
        assert_eq!(create(c, t, &[new("ana"), new("ANA")], false)?, Err(Refused::Taken("ana".into())), "twice in the batch");
        assert_eq!(create(c, t, &[new("bea"), new("owner.s")], false)?, Err(Refused::Taken("owner.s".into())));
        assert_eq!(users(c)?, before, "nothing was made");

        let ids = create(c, t, &[new("Ana"), new("bea")], false)?.unwrap();
        let row: (Option<String>, String, i64, Option<i64>, String) =
            c.query_row("SELECT email, username, teacher_id, preset_id, first_name FROM users WHERE id = ?1", [ids[0]], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?;
        assert_eq!(row, (None, "ana".into(), t, None, "Ana".into()), "no address, stored lower case");
        assert!(is_student(c, ids[0])? && !is_student(c, t)?);
        assert!(!may_hold(c, ids[0])?, "a student never holds students");
        assert_eq!(create(c, ids[0], &[new("cat")], false)?, Err(Refused::NotTeacher));
        assert_eq!((count(c, t)?, count_all(c)?), (2, 3));
        assert_eq!(list(c, t)?.iter().map(|s| s.username.as_str()).collect::<Vec<_>>(), ["ana", "bea"]);

        // The cap: the box holds `student_limit()` students, every teacher's.
        let limit = crate::caps::student_limit() as i64;
        let room = limit - count_all(c)?;
        let batch: Vec<NewStudent> = (0..=room).map(|i| new(&format!("stu{i}"))).collect();
        assert_eq!(create(c, t, &batch, false)?, Err(Refused::Full), "one more than there is room for");
        assert_eq!(count_all(c)?, 3);
        assert!(create(c, t, &batch[..room as usize], false)?.is_ok(), "exactly the room left");
        assert_eq!(count_all(c)?, limit);
        assert_eq!(create(c, only, &[new("late")], false)?, Err(Refused::Full));
        assert!(create(c, only, &[new("late")], true)?.is_ok(), "the dev server is never full");
        // Students do not count against normal accounts.
        assert!(!crate::signup::is_full(c, false)?);
        Ok(())
    })
    .unwrap();
}

/// Renaming, a new password (every session ends), deleting; and deleting
/// the teacher takes its students with everything in them.
#[test]
fn a_teacher_looks_after_its_own_students_only() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let (t, ids) = teacher_with_students(c, "t@example.org", "kid", 2, "h")?;
        let (other, theirs) = teacher_with_students(c, "o@example.org", "their", 1, "h")?;
        let (ana, bea) = (ids[0], ids[1]);

        assert_eq!(rename(c, t, ana, " Ana.G ")?, Ok(true));
        assert_eq!(rename(c, t, bea, "ana.g")?, Err(Refused::Taken("ana.g".into())));
        assert_eq!(rename(c, t, ana, "ana.g")?, Ok(true), "its own name again");
        assert!(matches!(rename(c, t, ana, "a@b")?, Err(Refused::BadUsername(..))));
        assert_eq!(rename(c, other, ana, "mine")?, Ok(false), "not theirs");
        assert_eq!(crate::auth::find_user_credentials(c, "ANA.G")?.map(|r| r.0), Some(ana));

        let s1 = crate::auth::create_session(c, ana)?;
        assert!(set_password(c, t, ana, "new-hash")?);
        assert!(crate::auth::user_for_session(c, &s1)?.is_none(), "every session ends");
        assert_eq!(crate::auth::find_user_credentials(c, "ana.g")?, Some((ana, "new-hash".into())));
        assert!(!set_password(c, other, ana, "x")?);

        assert_eq!(delete(c, t, &[theirs[0], bea])?, 1, "another teacher's student is left alone");
        assert!(!is_student(c, bea)? && is_student(c, theirs[0])?);

        // The teacher's account goes: its students go with their rows.
        c.execute("INSERT INTO lesson_passes (user_id, course_id, lesson_id, seen_at, updated_at) VALUES (?1, 'spanish', 'l1', 1, 1)", [ana])?;
        crate::auth::create_session(c, ana)?;
        c.execute("DELETE FROM users WHERE id = ?1", [t])?;
        let left = |table: &str| c.query_row(&format!("SELECT COUNT(*) FROM {table} WHERE user_id = ?1"), [ana], |r| r.get::<_, i64>(0));
        assert!(!is_student(c, ana)?);
        assert_eq!((left("lesson_passes")?, left("sessions")?), (0, 0));
        assert_eq!(count(c, other)?, 1, "another teacher's students stay");
        Ok(())
    })
    .unwrap();
}

/// A student keeps two live sessions (#216): a third sign-in ends the oldest.
#[test]
fn a_student_keeps_two_sessions() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let (t, ids) = teacher_with_students(c, "t@example.org", "kid", 1, "h")?;
        let tokens: Vec<String> = (0..3).map(|_| crate::auth::create_session(c, ids[0])).collect::<Result<_, _>>()?;
        assert!(crate::auth::user_for_session(c, &tokens[0])?.is_none());
        for tk in &tokens[1..] {
            let u = crate::auth::user_for_session(c, tk)?.unwrap();
            assert!(u.student && u.username == "kid1" && u.email.is_empty());
            assert!(u.ai_off && !u.apps_on, "no AI and no apps for a student");
            assert!(crate::perms::Perm::ALL.iter().all(|p| !u.can(*p)));
        }
        let teacher: Vec<String> = (0..4).map(|_| crate::auth::create_session(c, t)).collect::<Result<_, _>>()?;
        assert!(teacher.iter().all(|tk| crate::auth::user_for_session(c, tk).unwrap().is_some()), "a teacher keeps four");
        assert!(!crate::auth::user_for_session(c, &teacher[0])?.unwrap().student);
        Ok(())
    })
    .unwrap();
}

#[test]
fn the_refused_paths() {
    for p in [
        "/courses/new", "/courses/generate", "/courses/generate/5", "/courses/shared/abc", "/courses/mine", "/courses/mine/file", "/courses/mine/copy",
        "/courses/mine/share", "/courses/mine/ai", "/courses/mine/export.json", "/api/courses/mine/files", "/api/generate/1", "/api/review-quiet",
        "/tutor", "/api/tutor/start", "/oauth/authorize", "/mcp", "/api/v1/whoami", "/profile/apps/token", "/profile/two-factor",
        "/profile/two-factor/totp/confirm", "/manage", "/manage/permissions", "/api/manage/keys", "/superuser", "/students", "/students/3",
    ] {
        assert!(refused(p), "{p}");
    }
    for p in [
        "/", "/courses", "/courses/open/spanish", "/courses/spanish/settings", "/courses/spanish/placement", "/courses/spanish/stat/xp",
        "/lessons", "/api/answer", "/profile", "/api/profile", "/profile/password", "/profile/sessions/end-others", "/logout", "/play",
        "/sprint", "/guide", "/vocabulary", "/tutorial", "/managed", "/coursesx",
    ] {
        assert!(!refused(p), "{p}");
    }
}

#[test]
fn the_sign_in_key_is_a_username_in_lower_case() {
    assert_eq!(sign_in_key(" Ana.Garcia "), "ana.garcia");
    assert_eq!(sign_in_key("Ann@Example.org"), "Ann@Example.org", "an address as typed, as before");
}

/// Manage Users lists normal accounts only, with how many students each
/// holds; students are counted apart from the normal accounts' cap.
#[test]
fn manage_users_counts_students_on_their_teachers_row() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let (t, _) = teacher_with_students(c, "t@example.org", "kid", 3, "h")?;
        let list = crate::signup::accounts(c)?;
        assert_eq!(list.len(), 1);
        assert_eq!((list[0].id, list[0].students), (t, 3));
        Ok(())
    })
    .unwrap();
}

// ---------- the routes ----------

struct Server {
    rt: tokio::runtime::Runtime,
    base: String,
    agent: ureq::Agent,
    state: std::sync::Arc<crate::AppState>,
}

impl Server {
    fn start() -> Server {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let state = crate::routes::test_state(false);
        let app = crate::routes::router(state.clone());
        let base = rt.block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move { axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap() });
            format!("http://{addr}")
        });
        let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
        Server { rt, base, agent, state }
    }

    /// (status, Location, the first Set-Cookie's `name=value`, body)
    fn send(r: Result<ureq::http::Response<ureq::Body>, ureq::Error>) -> (u16, String, String, String) {
        let mut r = r.unwrap();
        let header = |k: &str| r.headers().get(k).map(|v| v.to_str().unwrap().to_string()).unwrap_or_default();
        let (location, cookie) = (header("location"), header("set-cookie").split(';').next().unwrap_or_default().to_string());
        (r.status().as_u16(), location, cookie, r.body_mut().read_to_string().unwrap_or_default())
    }

    fn get(&self, path: &str, cookie: &str) -> (u16, String, String, String) {
        Self::send(self.agent.get(&format!("{}{path}", self.base)).header("cookie", cookie).call())
    }

    fn post(&self, path: &str, cookie: &str, form: &[(&str, &str)]) -> (u16, String, String, String) {
        Self::send(self.agent.post(&format!("{}{path}", self.base)).header("cookie", cookie).send_form(form.iter().copied()))
    }

    fn post_json(&self, path: &str, cookie: &str, body: &str) -> (u16, String, String, String) {
        Self::send(self.agent.post(&format!("{}{path}", self.base)).header("cookie", cookie).header("content-type", "application/json").send(body))
    }

    fn login(&self, name: &str, password: &str) -> (u16, String, String, String) {
        self.post("/login", "", &[("email", name), ("password", password)])
    }
}

/// The login field takes a username, in any case; the sign-in limits count
/// it as they count an address; the login page says a student asks its
/// teacher for a new password.
#[test]
fn a_student_signs_in_with_its_username() {
    let s = Server::start();
    let hash = crate::auth::hash_password("right-password").unwrap();
    s.state.db.with(|c| teacher_with_students(c, "t@example.org", "ana.garcia", 1, &hash)).unwrap();
    let _ = &s.rt;

    let (status, _, _, page) = s.get("/login", "");
    assert_eq!(status, 200);
    assert!(page.contains(r#"data-testid="login-student-note""#) && page.contains("asks their teacher"), "{page}");
    assert!(page.contains(r#"type="text" name="email""#), "the field takes a username");

    let (status, location, cookie, _) = s.login("ANA.GARCIA1", "right-password");
    assert_eq!((status, location.as_str()), (303, "/"));
    assert!(cookie.starts_with("sid="));
    let (status, _, _, profile) = s.get("/profile", &cookie);
    assert_eq!(status, 200);
    assert!(profile.contains(r#"data-testid="profile-username""#) && profile.contains(r#"value="ana.garcia1""#), "{profile}");

    let (status, _, cookie, page) = s.login("ana.garcia1", "wrong-password");
    assert_eq!((status, cookie.as_str()), (200, ""));
    assert!(page.contains(r#"data-testid="login-error""#));
    assert_eq!(s.login("nobody", "right-password").0, 200, "an unknown username is just wrong");

    // Ten wrong passwords for the username, in any case: then even the right one waits.
    for i in 1..crate::security::LOGIN_FAILURES_PER_USER.max {
        let name = if i % 2 == 0 { "Ana.Garcia1" } else { "ana.garcia1" };
        assert_eq!(s.login(name, "wrong-password").0, 200, "try {i}");
    }
    assert_eq!(s.login("ana.garcia1", "right-password").0, 429, "the username's tries are used up");
    assert_eq!(s.login("ANA.GARCIA1", "right-password").0, 429, "in any case");
}

/// Every refused path answers a student 404, page and API, and only a
/// student; the profile shows the username, the language and the password,
/// and saves the language only.
#[test]
fn a_student_is_refused_what_is_not_for_it() {
    let s = Server::start();
    let (teacher, kid) = s
        .state
        .db
        .with(|c| {
            let (t, ids) = teacher_with_students(c, "t@example.org", "kid", 1, "h")?;
            Ok((crate::auth::create_session(c, t)?, crate::auth::create_session(c, ids[0])?))
        })
        .unwrap();
    let (tc, kc) = (format!("sid={teacher}"), format!("sid={kid}"));

    for page in ["/courses/new", "/courses/generate", "/tutor", "/manage", "/superuser", "/courses/shared/abcd", "/oauth/authorize?client_id=x"] {
        assert_eq!(s.get(page, &kc).0, 404, "student {page}");
    }
    for page in ["/courses/new", "/courses/generate", "/tutor"] {
        assert_ne!(s.get(page, &tc).0, 404, "teacher {page}");
    }
    for api in ["/api/tutor/session", "/api/courses/x/files", "/api/generate/1"] {
        assert_eq!(s.get(api, &kc).0, 404, "student {api}");
    }
    for (form, fields) in [
        ("/profile/two-factor", &[("two_factor", "on")][..]),
        ("/profile/apps/token", &[("name", "x")][..]),
        ("/courses/new", &[("title", "Mine")][..]),
        ("/manage/permissions", &[("email", "t@example.org")][..]),
    ] {
        assert_eq!(s.post(form, &kc, fields).0, 404, "student {form}");
    }
    assert_eq!(s.post_json("/api/manage/keys", &kc, "{}").0, 404);
    for open in ["/courses", "/profile"] {
        assert_eq!(s.get(open, &kc).0, 200, "student {open}");
    }

    let (_, _, _, profile) = s.get("/profile", &kc);
    assert!(profile.contains(r#"data-testid="profile-username""#) && profile.contains(r#"data-testid="profile-lang""#) && profile.contains(r#"data-testid="profile-password-open""#));
    for gone in ["profile-email", "profile-two-factor", "profile-first-name", "profile-apps", "profile-privacy-info"] {
        assert!(!profile.contains(&format!(r#"data-testid="{gone}""#)), "{gone} on a student's profile");
    }
    let (status, _, _, body) = s.post_json("/api/profile", &kc, r#"{"first_name": "Ana"}"#);
    assert_eq!(status, 422, "{body}");
    assert_eq!(s.post_json("/api/profile", &kc, r#"{"lang": "es"}"#).0, 200);
    assert_eq!(s.post_json("/api/profile", &tc, r#"{"first_name": "Tia"}"#).0, 200, "a teacher still names itself");
}
