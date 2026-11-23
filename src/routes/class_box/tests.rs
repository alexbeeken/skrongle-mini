//! The Class box page's routes (#227), against a real router: who gets the
//! page and its Backup card, the Right now counts, the password before a
//! backup (counted with the sign-in limits), and a download that restores
//! into a server that starts.

use super::*;
use crate::backup::tests::{class_db, fresh_dir, holds, untar};
use crate::students::tests::{teacher_with_students, Server};

fn session(s: &Server, user: i64) -> String {
    format!("sid={}", s.state.db.with(|c| crate::auth::create_session(c, user)).unwrap())
}

/// The `data-n` of the element with `data-testid="<id>"`.
fn data_n(page: &str, id: &str) -> i64 {
    let at = page.find(&format!(r#"data-testid="{id}""#)).unwrap_or_else(|| panic!("no {id} in {page}"));
    let rest = &page[at..];
    let n = &rest[rest.find(r#"data-n=""#).unwrap() + 8..];
    n[..n.find('"').unwrap()].parse().unwrap()
}

/// A student and an account that may not hold students get a 404 on every
/// Class box route; a teacher gets the page without Backup and the top bar
/// links it; an owner gets Backup too. The join card shows the address the
/// browser used; the poster is that card alone.
#[test]
fn a_teacher_has_the_page_and_an_owner_its_backup() {
    let s = Server::start();
    let (t, kids) = s.state.db.with(|c| teacher_with_students(c, "t@example.org", "kid", 2, "h")).unwrap();
    let (plain, owner) = s
        .state
        .db
        .with(|c| {
            let plain = crate::auth::create_user(c, "plain@example.org", "h")?;
            let owner = crate::auth::create_user(c, "owner@example.org", "h")?;
            crate::owner::make_owner(c, owner)?;
            Ok((plain, owner))
        })
        .unwrap();
    for cookie in [session(&s, kids[0]), session(&s, plain)] {
        for page in ["/class-box", "/class-box/poster"] {
            assert_eq!(s.get(page, &cookie).0, 404, "{page} for {cookie}");
        }
        assert_eq!(s.post("/class-box/backup", &cookie, &[("password", "h")]).0, 404);
    }

    let tc = session(&s, t);
    let (status, _, _, page) = s.get("/class-box", &tc);
    assert_eq!(status, 200);
    for id in ["nav-class-box", "class-box-join", "class-box-poster", "class-box-now", "class-box-files", "class-box-courses-link"] {
        assert!(page.contains(&format!(r#"data-testid="{id}""#)), "{id}");
    }
    assert!(!page.contains(r#"data-testid="class-box-backup""#), "Backup is an owner's");
    assert!(page.contains(&format!(">{}</span>", s.base)), "the address the browser used: {}", s.base);
    // One of the two students signed in a moment ago (the session above).
    assert_eq!((data_n(&page, "class-box-on-now"), data_n(&page, "class-box-today")), (1, 0));
    assert_eq!(data_n(&page, "class-box-courses") as usize, s.state.courses.builtin().len());
    assert!(page.contains(r#"data-of="2""#));
    assert_eq!(s.post("/class-box/backup", &tc, &[("password", "h")]).0, 404, "the backup is an owner's");

    let (status, _, _, poster) = s.get("/class-box/poster", &tc);
    assert_eq!(status, 200);
    assert!(poster.contains(r#"data-testid="class-box-join""#) && poster.contains(&s.base));
    assert!(!poster.contains(r#"data-testid="class-box-now""#), "the join card alone");

    let (status, _, _, page) = s.get("/class-box", &session(&s, owner));
    assert_eq!(status, 200);
    assert!(page.contains(r#"data-testid="class-box-backup""#));
    assert!(page.contains(r#"data-testid="class-box-backup-password""#), "it asks for the password");
    assert!(page.contains(&crate::t("class_box.backup_never")));
    assert!(page.contains(r#"data-testid="class-box-key-outside""#), "the test server's key is not a file");
}

/// Right now: the teacher's own students (an owner: every student),
/// on now from a sign-in, an answer or a lesson in the last 10 minutes,
/// today from an answer or a lesson; each read through an index.
#[test]
fn right_now_counts_sign_ins_and_study() {
    let db = crate::db::Db::open_in_memory().unwrap();
    let now = crate::db::now();
    let got = db
        .with(|c| {
            let (t, kids) = teacher_with_students(c, "t@example.org", "kid", 4, "h")?;
            let (_, other) = teacher_with_students(c, "u@example.org", "pupil", 1, "h")?;
            let sess = |user: i64, at: i64| {
                c.execute(
                    "INSERT INTO sessions (user_id, session_token, created_at, expires_at) VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![user, format!("tok-{user}-{at}"), at, at + 86_400],
                )
            };
            let touch = |user: i64, at: i64| crate::activity::touch_at(c, user, "spanish", "l1", crate::activity::Mode::Study, at);
            sess(kids[0], now - 60)?; // on now: signed in
            touch(kids[1], now - 120)?; // on now and today
            touch(kids[2], now - 2000)?; // today only
            sess(kids[3], now - 7200)?; // neither
            touch(kids[3], now - 90_000)?;
            sess(other[0], now)?; // another teacher's
            let today = now - 3000;
            Ok((counts(c, t, now - ON_NOW_SECS, today)?, counts(c, 0, now - ON_NOW_SECS, today)?))
        })
        .unwrap();
    assert_eq!((got.0.students, got.0.on_now, got.0.studied_today), (4, 2, 2), "the teacher's own");
    assert_eq!((got.1.students, got.1.on_now, got.1.studied_today), (5, 3, 2), "an owner's: every student");

    let sql = "SELECT COUNT(*),
                SUM(EXISTS(SELECT 1 FROM sessions s WHERE s.user_id = u.id AND s.created_at >= ?2)
                 OR EXISTS(SELECT 1 FROM quiz_history q WHERE q.user_id = u.id AND q.answered_at >= ?2)
                 OR EXISTS(SELECT 1 FROM lesson_touches t WHERE t.user_id = u.id AND t.at >= ?2))
         FROM users u WHERE u.teacher_id IS NOT NULL AND (?1 = 0 OR u.teacher_id = ?1)";
    let scans = db.with(|c| crate::db::table_scans(c, sql)).unwrap();
    assert!(scans.iter().all(|t| !["s", "q", "t"].contains(&t.split(' ').next().unwrap_or(""))), "{scans:?}");
}

/// The bytes of a POST's answer, with its headers.
fn post_bytes(s: &Server, path: &str, cookie: &str, form: &[(&str, &str)]) -> (u16, String, String, Vec<u8>) {
    let mut r = s.agent.post(&format!("{}{path}", s.base)).header("cookie", cookie).send_form(form.iter().copied()).unwrap();
    let h = |k: &str| r.headers().get(k).map(|v| v.to_str().unwrap().to_string()).unwrap_or_default();
    let (ctype, disposition) = (h("content-type"), h("content-disposition"));
    (r.status().as_u16(), ctype, disposition, r.body_mut().with_config().limit(64 << 20).read_to_vec().unwrap())
}

/// The owner's password, then the archive: it unpacks with the documented
/// command into a server that starts and signs its teacher in. The time is
/// kept; no snapshot is left. Wrong passwords count against sign-in.
#[test]
fn the_owner_downloads_a_backup_that_restores() {
    let dir = fresh_dir("route");
    let hash = crate::auth::hash_password("right-password").unwrap();
    let (db, path) = class_db(&dir, &hash);
    let state = Arc::new(AppState { db, db_path: path.clone(), ..Arc::into_inner(crate::routes::test_state(false)).unwrap() });
    let s = Server::start_with(state);
    let owner: i64 = s.state.db.with(|c| c.query_row("SELECT id FROM users WHERE owner = 1", [], |r| r.get(0))).unwrap();
    let oc = session(&s, owner);

    let (status, _, _, page) = s.post("/class-box/backup", &oc, &[("password", "wrong-password")]);
    assert_eq!(status, 403);
    assert!(page.contains(r#"data-testid="class-box-backup-error""#), "{page}");
    assert!(!page.contains(r#"data-testid="class-box-key-outside""#), "the key is a file here: it goes in the archive");

    let (status, ctype, disposition, archive) = post_bytes(&s, "/class-box/backup", &oc, &[("password", "right-password")]);
    assert_eq!(status, 200);
    assert_eq!(ctype, "application/gzip");
    assert!(disposition.starts_with("attachment; filename=\"skrongle-backup-") && disposition.ends_with(".tar.gz\""), "{disposition}");

    // The time is kept once the last piece is handed over.
    let mut last = None;
    for _ in 0..50 {
        last = s.state.db.with(|c| crate::backup::last_backup(c)).unwrap();
        if last.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(last.is_some_and(|at| (crate::db::now() - at).abs() < 60));
    let (_, _, _, page) = s.get("/class-box", &oc);
    assert!(page.contains(&crate::t1("class_box.backup_last", "when", crate::t("class_box.backup_today"))), "{page}");
    let left: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).filter(|n| n.contains(".backup-")).collect();
    assert!(left.is_empty(), "no snapshot left: {left:?}");

    // Restored as the docs say, it is a server that starts with the same accounts.
    let restored = untar(&archive, "route-restored");
    let rpath = restored.join("learn.db");
    assert_eq!(holds(&rpath), (3, 1, 1, 1));
    crate::db::check_not_newer(&rpath).unwrap();
    let (vault, from) = crate::secrets::Vault::load(None, &rpath).unwrap();
    assert!(from.ends_with("learn.db.key"), "{from}");
    let rdb = crate::db::Db::open(&rpath).unwrap();
    let back = Server::start_with(Arc::new(AppState { db: rdb, db_path: rpath, vault, ..Arc::into_inner(crate::routes::test_state(false)).unwrap() }));
    let (status, location, cookie, _) = back.login("t@example.org", "right-password");
    assert_eq!((status, location.as_str()), (303, "/"), "the teacher signs in on the restored server");
    let (status, _, _, students) = back.get("/students", &cookie);
    assert_eq!(status, 200);
    assert!(students.contains(r#"data-username="kid1""#), "with its student");

    // Ten tries in all (one was right): then even the right one waits. The
    // nine wrong ones count against sign-in: one more there blocks it.
    let max = crate::security::LOGIN_FAILURES_PER_USER.max;
    for i in 3..=max {
        assert_eq!(s.post("/class-box/backup", &oc, &[("password", "wrong-password")]).0, 403, "try {i}");
    }
    assert_eq!(s.post("/class-box/backup", &oc, &[("password", "right-password")]).0, 429);
    assert_eq!(s.login("owner@example.org", "wrong-password").0, 200, "the tenth wrong password");
    assert_eq!(s.login("owner@example.org", "right-password").0, 429, "the backup's tries counted against sign-in");

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&restored);
}
