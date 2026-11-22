//! The Students page's routes (#218), against a real router: adding (all or
//! none, the cap, a taken username), the sheet shown once, rename, a new
//! password, delete, and the two 404s.

use crate::students::tests::{teacher_with_students, Server};
use crate::students::{self, NewStudent};

/// A teacher with `n` students (`kid1` …, password `kid-password`) and a
/// session: (teacher id, student ids, the teacher's cookie).
fn teacher(s: &Server, email: &str, prefix: &str, n: usize) -> (i64, Vec<i64>, String) {
    let hash = crate::auth::hash_password("kid-password").unwrap();
    let (t, ids, sid) = s
        .state
        .db
        .with(|c| {
            let (t, ids) = teacher_with_students(c, email, prefix, n, &hash)?;
            Ok((t, ids, crate::auth::create_session(c, t)?))
        })
        .unwrap();
    (t, ids, format!("sid={sid}"))
}

/// The values of every `data-testid="<id>"` element's text in `page`, for
/// simple one-line elements.
fn texts(page: &str, id: &str) -> Vec<String> {
    let marker = format!(r#"data-testid="{id}""#);
    page.split(&marker)
        .skip(1)
        .map(|rest| {
            let after = &rest[rest.find('>').map_or(0, |i| i + 1)..];
            after[..after.find('<').unwrap_or(after.len())].trim().to_string()
        })
        .collect()
}

fn users(s: &Server) -> i64 {
    s.state.db.with(|c| c.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))).unwrap()
}

/// A student and an account that may not hold students get a 404 on every
/// Students route; the top bar links the page only for a teacher.
#[test]
fn only_a_teacher_has_the_page() {
    let s = Server::start();
    let (_, kids, tc) = teacher(&s, "t@example.org", "kid", 1);
    let (kid, plain) = s
        .state
        .db
        .with(|c| {
            let plain = crate::auth::create_user(c, "plain@example.org", "h")?;
            Ok((crate::auth::create_session(c, kids[0])?, crate::auth::create_session(c, plain)?))
        })
        .unwrap();
    let id = kids[0].to_string();
    for cookie in [format!("sid={kid}"), format!("sid={plain}")] {
        for page in ["/students", "/students/sheet", &format!("/students/{id}")] {
            assert_eq!(s.get(page, &cookie).0, 404, "{page} for {cookie}");
        }
        for form in ["/students/add", "/students/sheet", "/students/passwords", "/students/delete", &format!("/students/{id}/password")] {
            assert_eq!(s.post(form, &cookie, &[("names", "Ana"), ("id", &id)]).0, 404, "{form} for {cookie}");
        }
        let (_, _, _, home) = s.get("/profile", &cookie);
        assert!(!home.contains(r#"data-testid="nav-students""#), "no Students link");
    }
    let (status, _, _, page) = s.get("/students", &tc);
    assert_eq!(status, 200);
    assert!(page.contains(r#"data-testid="nav-students""#));
    assert_eq!(texts(&page, "students-count"), [format!("1 of {}", crate::caps::student_limit())]);
    assert_eq!(texts(&page, "students-username"), ["kid1"]);
    assert_eq!(texts(&page, "students-passed"), ["0"]);
    assert_eq!(texts(&page, "students-last-studied"), [""], "Never sits in a span");
    assert!(page.contains("Never"));
}

/// Paste names, get rows with free usernames; save makes every account or
/// none; the sheet shows each password once, `no-store`, and its address
/// opened again shows none.
#[test]
fn adding_makes_all_or_none_and_the_sheet_shows_once() {
    let s = Server::start();
    let (t, _, tc) = teacher(&s, "t@example.org", "kid", 0);
    s.state.db.with(|c| students::create(c, t, &[NewStudent { username: "bea.ruiz".into(), password_hash: "h".into(), ..Default::default() }], false)).unwrap().unwrap();

    let (status, _, _, rows) = s.post("/students/add", &tc, &[("names", "Ana García\n\n  Bea   Ruiz \nCai\n")]);
    assert_eq!(status, 200, "{rows}");
    assert_eq!(texts(&rows, "students-add-name"), ["Ana García", "Bea Ruiz", "Cai"]);
    for (u, want) in [("ana.garcia", true), ("bea.ruiz2", true), ("cai", true)] {
        assert_eq!(rows.contains(&format!(r#"name="username" value="{u}""#)), want, "{u}");
    }
    let (status, _, _, page) = s.post("/students/add", &tc, &[("names", "  \n ")]);
    assert_eq!(status, 200);
    assert_eq!(texts(&page, "students-error").len(), 1, "no names");

    // A taken username, a bad one, a short password: nothing made.
    let before = users(&s);
    for (u2, p2) in [("bea.ruiz", ""), ("a b", ""), ("bea.ruiz2", "short")] {
        let (status, _, _, page) = s.post(
            "/students/sheet",
            &tc,
            &[("name", "Ana García"), ("username", "ana.garcia"), ("password", ""), ("name", "Bea Ruiz"), ("username", u2), ("password", p2)],
        );
        assert_eq!(status, 200);
        assert_eq!(texts(&page, "students-error").len(), 1, "{u2} {p2}: {page}");
        assert!(page.contains(r#"data-testid="students-rows-form""#), "the rows again");
        assert_eq!(users(&s), before, "{u2} {p2}: nothing made");
    }

    // Saved: the sheet, once, never cached.
    let r = s
        .agent
        .post(&format!("{}/students/sheet", s.base))
        .header("cookie", &tc)
        .send_form([("name", "Ana García"), ("username", "Ana.Garcia"), ("password", "typed-pass"), ("name", "Cai"), ("username", "cai.x"), ("password", "")])
        .unwrap();
    assert_eq!(r.status().as_u16(), 200);
    assert_eq!(r.headers().get("cache-control").unwrap(), "no-store");
    let sheet = r.into_body().read_to_string().unwrap();
    assert_eq!(texts(&sheet, "students-slip-username"), ["ana.garcia", "cai.x"]);
    let passwords = texts(&sheet, "students-slip-password");
    assert_eq!(passwords[0], "typed-pass");
    assert!(passwords[1].len() == 8 && passwords[1].bytes().all(|b| students::PASSWORD_CHARS.contains(&b)), "{passwords:?}");
    assert_eq!(texts(&sheet, "students-slip-address"), [s.base.clone(), s.base.clone()]);
    assert_eq!(users(&s), before + 2);
    assert_eq!(s.login("cai.x", &passwords[1]).0, 303, "the made password signs in");
    assert_eq!(s.login("ana.garcia", "typed-pass").0, 303);

    let (status, _, _, again) = s.get("/students/sheet", &tc);
    assert_eq!(status, 200);
    assert!(again.contains(r#"data-testid="students-sheet-gone""#));
    assert!(!again.contains("typed-pass") && !again.contains(&passwords[1]), "shown once");
    let (_, _, _, list) = s.get("/students", &tc);
    assert_eq!(texts(&list, "students-count"), [format!("3 of {}", crate::caps::student_limit())]);
    let stored: String = s.state.db.with(|c| c.query_row("SELECT first_name || '|' || last_name FROM users WHERE username = 'ana.garcia'", [], |r| r.get(0))).unwrap();
    assert_eq!(stored, "Ana|García");
}

/// The box's student limit: more names than there is room for are refused
/// on the list, a batch over it on save; at the limit the page says so and
/// names the setting.
#[test]
fn the_cap_refuses_a_batch_that_does_not_fit() {
    let s = Server::start();
    let limit = crate::caps::student_limit();
    let (t, _, tc) = teacher(&s, "t@example.org", "kid", limit - 1);

    let (_, _, _, page) = s.get("/students", &tc);
    assert!(page.contains("You can add 1 more."), "{page}");
    let (status, _, _, page) = s.post("/students/add", &tc, &[("names", "Ana\nBea")]);
    assert_eq!(status, 200);
    assert_eq!(texts(&page, "students-error"), ["Too many names: the box takes 1 more student."]);
    assert!(page.contains("Ana\nBea"), "the names stay");

    let before = users(&s);
    let (_, _, _, page) = s.post("/students/sheet", &tc, &[("name", "Ana"), ("username", "ana.x"), ("password", ""), ("name", "Bea"), ("username", "bea.x"), ("password", "")]);
    assert!(texts(&page, "students-error")[0].contains(&format!("{limit} student accounts")), "{page}");
    assert_eq!(users(&s), before);

    let (status, _, _, _) = s.post("/students/sheet", &tc, &[("name", "Ana"), ("username", "ana.x"), ("password", "")]);
    assert_eq!(status, 200);
    assert_eq!(s.state.db.with(|c| students::count(c, t)).unwrap(), limit as i64);
    let (_, _, _, page) = s.get("/students", &tc);
    assert!(page.contains(r#"data-testid="students-full""#) && page.contains("SKRONGLE_STUDENT_LIMIT"));
    assert!(!page.contains(r#"data-testid="students-add-form""#));
}

/// A student's page: rename (a taken name refused), a new password typed or
/// made (the old one stops working; the sheet shows it), delete with a
/// confirm; the ticked students' New passwords and Delete picked, each
/// with a confirm; another teacher's student is a 404 or left alone.
#[test]
fn a_teacher_renames_sets_passwords_and_deletes() {
    let s = Server::start();
    let (_, kids, tc) = teacher(&s, "t@example.org", "kid", 3);
    let (_, theirs, _) = teacher(&s, "o@example.org", "their", 1);
    let (a, b, c) = (kids[0].to_string(), kids[1].to_string(), kids[2].to_string());

    let (status, _, _, page) = s.get(&format!("/students/{a}"), &tc);
    assert_eq!(status, 200);
    assert!(page.contains(r#"value="kid1""#));
    assert_eq!(s.get(&format!("/students/{}", theirs[0]), &tc).0, 404, "another teacher's");
    assert_eq!(s.get("/students/999999", &tc).0, 404);

    let (status, location, _, _) = s.post(&format!("/students/{a}"), &tc, &[("username", "Ana.G"), ("first_name", "Ana"), ("last_name", "García")]);
    assert_eq!((status, location.as_str()), (303, format!("/students/{a}?did=saved").as_str()));
    let (_, _, _, page) = s.get(&format!("/students/{a}?did=saved"), &tc);
    assert_eq!(texts(&page, "students-one-name"), ["Ana García"]);
    assert!(page.contains(r#"data-testid="students-notice""#));
    let (status, _, _, page) = s.post(&format!("/students/{b}"), &tc, &[("username", "ana.g"), ("first_name", "Bea"), ("last_name", "")]);
    assert_eq!(status, 200);
    assert_eq!(texts(&page, "students-error"), ["The username ana.g is taken. Choose another."]);
    let stored: String = s.state.db.with(|c| c.query_row("SELECT first_name FROM users WHERE id = ?1", [kids[1]], |r| r.get(0))).unwrap();
    assert_eq!(stored, "", "nothing saved");
    assert_eq!(s.post(&format!("/students/{}", theirs[0]), &tc, &[("username", "mine")]).0, 404);

    // A typed password; then a made one.
    assert_eq!(s.login("ana.g", "kid-password").0, 303);
    let (status, _, _, sheet) = s.post(&format!("/students/{a}/password"), &tc, &[("password", "brand-new-pw")]);
    assert_eq!(status, 200);
    assert_eq!(texts(&sheet, "students-slip-password"), ["brand-new-pw"]);
    assert_eq!(s.login("ana.g", "kid-password").0, 200, "the old one is refused");
    assert_eq!(s.login("ana.g", "brand-new-pw").0, 303);
    let (_, _, _, page) = s.post(&format!("/students/{a}/password"), &tc, &[("password", "short")]);
    assert_eq!(texts(&page, "students-error").len(), 1);
    let (_, _, _, sheet) = s.post(&format!("/students/{a}/password"), &tc, &[("password", "")]);
    assert_eq!(texts(&sheet, "students-slip-password")[0].len(), 8);
    assert_eq!(s.post(&format!("/students/{}/password", theirs[0]), &tc, &[("password", "")]).0, 404);

    // New passwords for the ticked: a confirm first, then the sheet.
    let (status, _, _, confirm) = s.post("/students/passwords", &tc, &[("id", &b), ("id", &c), ("id", &theirs[0].to_string())]);
    assert_eq!(status, 200);
    assert_eq!(texts(&confirm, "students-confirm-question"), ["Give 2 students new passwords?"], "another teacher's is left out");
    let (status, _, _, sheet) = s.post("/students/passwords", &tc, &[("id", &b), ("id", &c), ("confirm", "1")]);
    assert_eq!(status, 200);
    assert_eq!(texts(&sheet, "students-slip-username"), ["kid2", "kid3"]);
    assert_eq!(s.login("kid2", "kid-password").0, 200);
    let (_, location, _, _) = s.post("/students/passwords", &tc, &[("confirm", "1")]);
    assert_eq!(location, "/students?did=none-picked&n=0");

    // Delete: from the student's page and from the list, each confirmed.
    let (status, _, _, confirm) = s.post(&format!("/students/{c}/delete"), &tc, &[]);
    assert_eq!(status, 200);
    assert!(confirm.contains(r#"data-testid="students-confirm-yes""#));
    let (_, _, _, nothing) = s.post("/students/delete", &tc, &[("id", &c)]);
    assert!(nothing.contains(r#"data-testid="students-confirm""#), "no confirm, no delete");
    assert_eq!(s.state.db.with(|cn| students::count(cn, kids_teacher(cn, kids[2]))).unwrap(), 3);
    let (status, location, _, _) = s.post("/students/delete", &tc, &[("id", &c), ("id", &b), ("id", &theirs[0].to_string()), ("confirm", "1")]);
    assert_eq!((status, location.as_str()), (303, "/students?did=deleted&n=2"));
    let (_, _, _, list) = s.get("/students?did=deleted&n=2", &tc);
    assert_eq!(texts(&list, "students-notice"), ["✓ 2 students deleted."]);
    assert_eq!(texts(&list, "students-username"), ["ana.g"]);
    assert!(s.state.db.with(|cn| students::is_student(cn, theirs[0])).unwrap(), "another teacher's stays");
}

fn kids_teacher(c: &rusqlite::Connection, student: i64) -> i64 {
    c.query_row("SELECT teacher_id FROM users WHERE id = ?1", [student], |r| r.get(0)).unwrap()
}

/// Lessons passed and last studied: counted from `lesson_passes` and
/// `lesson_touches` over every course; a week quiet is bold.
#[test]
fn the_list_shows_passes_and_last_studied() {
    let s = Server::start();
    let (t, kids, tc) = teacher(&s, "t@example.org", "kid", 3);
    let now = crate::db::now();
    s.state
        .db
        .with(|c| {
            for (lesson, course) in [("l1", "spanish"), ("l2", "spanish"), ("l1", "other")] {
                c.execute("INSERT INTO lesson_passes (user_id, course_id, lesson_id, seen_at, passed_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4, ?4)", rusqlite::params![kids[0], course, lesson, now - 3600])?;
            }
            c.execute("INSERT INTO lesson_passes (user_id, course_id, lesson_id, seen_at, updated_at) VALUES (?1, 'spanish', 'l3', ?2, ?2)", rusqlite::params![kids[0], now - 3600])?;
            c.execute("INSERT INTO lesson_touches (user_id, course_id, lesson_id, mode, at) VALUES (?1, 'spanish', 'l1', 'study', ?2)", rusqlite::params![kids[1], now - 9 * 86_400])?;
            Ok(())
        })
        .unwrap();
    let progress = s.state.db.with(|c| students::progress(c, t)).unwrap();
    assert_eq!(progress[&kids[0]], students::Progress { passed: 3, last_studied: Some(now - 3600) });
    assert_eq!(progress[&kids[1]].passed, 0);
    assert!(!progress.contains_key(&kids[2]));

    let (_, _, _, page) = s.get("/students", &tc);
    assert_eq!(texts(&page, "students-passed"), ["3", "0", "0"]);
    let quiet: Vec<&str> = page.split(r#"data-testid="students-last-studied" data-quiet=""#).skip(1).map(|r| &r[..4]).collect();
    assert_eq!(quiet, ["fals", "true", "fals"]);
    assert!(page.contains("<strong>9 days ago</strong>"), "{page}");
}

/// Give course (#220): tick students, pick a course, Give; the Courses
/// column names it; the Share dialog does not list the students; a
/// student's page ticks and unticks its courses. Only the teacher's own
/// course that loads can be given.
#[test]
fn a_teacher_gives_courses_from_the_list_and_a_students_page() {
    let s = Server::start();
    let (t, kids, tc) = teacher(&s, "t@example.org", "kid", 3);
    let (a, b) = (kids[0].to_string(), kids[1].to_string());
    let page = s.get("/students", &tc).3;
    assert!(page.contains(r#"data-testid="students-no-givable""#) && !page.contains(r#"data-testid="students-give""#), "no course of its own yet");
    assert_eq!(texts(&page, "students-courses").len(), 3);

    s.state
        .db
        .with(|c| {
            crate::students::tests::course_of(c, t, "maths-1", "Maths")?;
            crate::students::tests::course_of(c, t, "art-1", "Art")?;
            let other = crate::auth::create_user(c, "o@example.org", "h")?;
            crate::students::tests::course_of(c, other, "theirs-1", "Theirs")
        })
        .unwrap();
    let (_, _, _, page) = s.get("/students", &tc);
    assert!(page.contains(r#"<option value="art-1">Art</option><option value="maths-1">Maths</option>"#), "{page}");
    assert!(!page.contains("theirs-1"), "another account's course is not offered");

    let (status, location, _, _) = s.post("/students/give", &tc, &[("id", &a), ("id", &b), ("course", "maths-1")]);
    assert_eq!((status, location.as_str()), (303, "/students?did=given&n=2&course=maths-1"));
    let (_, _, _, page) = s.get(&location, &tc);
    assert!(texts(&page, "students-notice")[0].contains("Given Maths to the ticked students (2)."), "{page}");
    let courses = texts(&page, "students-courses");
    assert_eq!(courses[..2], ["Maths", "Maths"]);
    assert!(page.contains("None yet"), "the third has none");
    assert_eq!(s.post("/students/give", &tc, &[("id", &a), ("course", "art-1")]).0, 303);
    assert_eq!(texts(&s.get("/students", &tc).3, "students-courses")[0], "Art · Maths");

    // Refused: nothing ticked, no course, another's course, a ready-made one.
    assert_eq!(s.post("/students/give", &tc, &[("course", "maths-1")]).1, "/students?did=none-picked&n=0");
    assert_eq!(s.post("/students/give", &tc, &[("id", &a)]).1, "/students?did=no-course&n=0");
    assert_eq!(s.post("/students/give", &tc, &[("id", &a), ("course", "theirs-1")]).0, 404);
    assert_eq!(s.post("/students/give", &tc, &[("id", &a), ("course", "spanish")]).0, 404);

    // The Share dialog lists links only, not the students.
    let (status, _, _, editor) = s.get("/courses/maths-1?share=1", &tc);
    assert_eq!(status, 200);
    assert!(!editor.contains(r#"data-testid="course-share-row""#), "{editor}");

    // The student's page: both courses, ticked as given; untick one.
    let (_, _, _, one) = s.get(&format!("/students/{a}"), &tc);
    assert!(one.contains(r#"value="art-1" checked data-testid="students-course""#) && one.contains(r#"value="maths-1" checked"#), "{one}");
    let (status, location, _, _) = s.post(&format!("/students/{a}/courses"), &tc, &[("course", "art-1")]);
    assert_eq!((status, location), (303, format!("/students/{a}?did=courses")));
    let have: Vec<String> = s.state.db.with(|c| students::subjects(c, kids[0])).unwrap().into_iter().map(|g| g.course_id).collect();
    assert_eq!(have, ["art-1"]);
    let (_, _, _, one) = s.get(&format!("/students/{a}?did=courses"), &tc);
    assert!(one.contains("Courses saved.") && one.contains(r#"value="maths-1" data-testid="students-course""#), "{one}");
    // Another teacher's student: a 404.
    let (_, others, oc) = teacher(&s, "t2@example.org", "oth", 1);
    assert_eq!(s.post(&format!("/students/{a}/courses"), &oc, &[("course", "art-1")]).0, 404);
    assert_eq!(s.post("/students/give", &oc, &[("id", &others[0].to_string()), ("course", "art-1")]).0, 404);
}
