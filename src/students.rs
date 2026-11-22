//! Student accounts (#217, `docs/features.md` "Student accounts"): accounts a
//! teacher makes and holds. A student signs in with a username and a
//! password; it has no e-mail address, no two-factor sign-in and no
//! permissions, and it never turns into a normal account.
//!
//! `users.teacher_id` names the teacher (deleting the teacher's account
//! deletes its students, with everything in them), `users.username` the
//! sign-in name, stored lower case and unique on the server. An account may
//! hold students with the Hold students permission (`Perm::HoldStudents`),
//! or as an owner. The box holds at most `caps::student_limit()` of them,
//! counted apart from the normal accounts (#216).
//!
//! What a student is refused is one list, [`refused`], which the router's
//! `auth::student_gate` layer checks for every page and API route: a
//! refused path answers a student 404.

use rusqlite::{params, Connection, OptionalExtension};

use crate::perms::{Perm, Perms};

pub const USERNAME_MIN: usize = 3;
pub const USERNAME_MAX: usize = 32;

/// The username as stored (trimmed, lower case), or why it can't be one:
/// 3 to 32 of a-z, 0-9, `.`, `_` and `-`, starting with a letter or digit.
/// Never an `@`, so the login page tells a username from an address.
pub fn normalize_username(raw: &str) -> Result<String, String> {
    let name = raw.trim().to_lowercase();
    let len = name.chars().count();
    let allowed = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-');
    if !(USERNAME_MIN..=USERNAME_MAX).contains(&len) || !name.chars().all(allowed) || !name.starts_with(|c: char| c.is_ascii_alphanumeric()) {
        return Err(crate::t2("students.username_rule", "min", USERNAME_MIN, "max", USERNAME_MAX));
    }
    Ok(name)
}

/// One student account to make: the username as typed (checked by
/// [`create`]), the hash of its first password, and its names.
#[derive(Debug, Clone, Default)]
pub struct NewStudent {
    pub username: String,
    pub password_hash: String,
    pub first_name: String,
    pub last_name: String,
}

/// Why no student account was made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// The account may not hold students: no Hold students permission and
    /// not an owner, or it is a student itself.
    NotTeacher,
    /// The box would hold more students than `caps::student_limit()`.
    Full,
    /// This username breaks the rules (`normalize_username`), with why.
    BadUsername(String, String),
    /// Someone on the server has this username already, or it is in the
    /// same batch twice.
    Taken(String),
}

/// May this account hold students? An owner may, a normal account with
/// Hold students may; a student never does.
pub fn may_hold(conn: &Connection, user_id: i64) -> rusqlite::Result<bool> {
    let row: Option<(bool, i64, bool)> = conn
        .query_row("SELECT owner, perms, teacher_id IS NOT NULL FROM users WHERE id = ?1", [user_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .optional()?;
    Ok(row.is_some_and(|(owner, perms, student)| !student && crate::auth::allowed(owner, Perms::from_bits(perms), Perm::HoldStudents)))
}

pub fn is_student(conn: &Connection, user_id: i64) -> rusqlite::Result<bool> {
    conn.query_row("SELECT EXISTS(SELECT 1 FROM users WHERE id = ?1 AND teacher_id IS NOT NULL)", [user_id], |r| r.get(0))
}

/// How many student accounts this teacher holds.
#[cfg_attr(not(any(test, feature = "dev")), allow(dead_code))] // /dev/student and the tests; the Students page counts its list
pub fn count(conn: &Connection, teacher_id: i64) -> rusqlite::Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM users WHERE teacher_id = ?1", [teacher_id], |r| r.get(0))
}

/// How many student accounts the box holds, every teacher's.
pub fn count_all(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM users WHERE teacher_id IS NOT NULL", [], |r| r.get(0))
}

fn username_taken(conn: &Connection, username: &str) -> rusqlite::Result<bool> {
    conn.query_row("SELECT EXISTS(SELECT 1 FROM users WHERE username = ?1)", [username], |r| r.get(0))
}

/// Would [`create`] take these usernames for `teacher_id`? The usernames as
/// stored, or the refusal `create` would give, with nothing written: the
/// Students page asks before it hashes any password.
pub fn check(conn: &Connection, teacher_id: i64, usernames: &[&str], dev: bool) -> rusqlite::Result<Result<Vec<String>, Refused>> {
    if !may_hold(conn, teacher_id)? {
        return Ok(Err(Refused::NotTeacher));
    }
    // `students_full(n)` is "n is at the limit": the last new one must still
    // find room, so the count before it is what is asked.
    if !usernames.is_empty() && crate::caps::students_full(count_all(conn)? + usernames.len() as i64 - 1, dev) {
        return Ok(Err(Refused::Full));
    }
    let mut names: Vec<String> = Vec::with_capacity(usernames.len());
    for raw in usernames {
        let name = match normalize_username(raw) {
            Ok(n) => n,
            Err(why) => return Ok(Err(Refused::BadUsername(raw.to_string(), why))),
        };
        if names.contains(&name) || username_taken(conn, &name)? {
            return Ok(Err(Refused::Taken(name)));
        }
        names.push(name);
    }
    Ok(Ok(names))
}

/// Make student accounts for `teacher_id`: every one of `students`, or none.
/// Each username is checked and stored as [`normalize_username`] gives it;
/// the box may not go over its student limit (`caps::students_full`; the
/// dev server is never full). The new accounts' ids, in order.
pub fn create(conn: &Connection, teacher_id: i64, students: &[NewStudent], dev: bool) -> rusqlite::Result<Result<Vec<i64>, Refused>> {
    let usernames: Vec<&str> = students.iter().map(|s| s.username.as_str()).collect();
    let names = match check(conn, teacher_id, &usernames, dev)? {
        Ok(names) => names,
        Err(why) => return Ok(Err(why)),
    };
    if students.is_empty() {
        return Ok(Ok(Vec::new()));
    }
    // All or none: a savepoint works inside a caller's transaction too.
    conn.execute_batch("SAVEPOINT students_create")?;
    let made = (|| {
        let mut ids = Vec::with_capacity(students.len());
        let now = crate::db::now();
        for (s, name) in students.iter().zip(&names) {
            conn.execute(
                "INSERT INTO users (password_hash, created_at, username, teacher_id, first_name, last_name, perms, preset_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, NULL)",
                params![s.password_hash, now, name, teacher_id, s.first_name.trim(), s.last_name.trim()],
            )?;
            ids.push(conn.last_insert_rowid());
        }
        Ok(ids)
    })();
    match made {
        Ok(ids) => {
            conn.execute_batch("RELEASE students_create")?;
            Ok(Ok(ids))
        }
        Err(e) => {
            conn.execute_batch("ROLLBACK TO students_create; RELEASE students_create")?;
            Err(e)
        }
    }
}

/// The teacher changes a student's username. `Ok(false)` when the student
/// is not theirs.
pub fn rename(conn: &Connection, teacher_id: i64, student_id: i64, raw: &str) -> rusqlite::Result<Result<bool, Refused>> {
    let name = match normalize_username(raw) {
        Ok(n) => n,
        Err(why) => return Ok(Err(Refused::BadUsername(raw.to_string(), why))),
    };
    let other: Option<i64> = conn.query_row("SELECT id FROM users WHERE username = ?1", [&name], |r| r.get(0)).optional()?;
    if other.is_some_and(|o| o != student_id) {
        return Ok(Err(Refused::Taken(name)));
    }
    let n = conn.execute("UPDATE users SET username = ?3 WHERE id = ?2 AND teacher_id = ?1", params![teacher_id, student_id, name])?;
    Ok(Ok(n > 0))
}

/// The teacher gives a student a new password: the old one stops working
/// and every open session of the student ends. False when the student is
/// not theirs. Two plain statements, as `auth::create_session`: inside a
/// caller's transaction they commit or roll back with it.
pub fn set_password(conn: &Connection, teacher_id: i64, student_id: i64, password_hash: &str) -> rusqlite::Result<bool> {
    let n = conn.execute("UPDATE users SET password_hash = ?3 WHERE id = ?2 AND teacher_id = ?1", params![teacher_id, student_id, password_hash])?;
    if n > 0 {
        conn.execute("DELETE FROM sessions WHERE user_id = ?1", [student_id])?;
    }
    Ok(n > 0)
}

/// The teacher deletes students: each account goes with its progress and
/// sessions (every user table cascades from `users`). Ids that are not the
/// teacher's students are left alone. How many went.
pub fn delete(conn: &Connection, teacher_id: i64, student_ids: &[i64]) -> rusqlite::Result<usize> {
    let mut n = 0;
    for id in student_ids {
        // The courses given to it go with it (`course_shares.user_id` is
        // only set to NULL when an account goes, which would leave the rows).
        conn.execute(
            "DELETE FROM course_shares WHERE given = 1 AND user_id = ?2 AND EXISTS(SELECT 1 FROM users WHERE id = ?2 AND teacher_id = ?1)",
            params![teacher_id, id],
        )?;
        n += conn.execute("DELETE FROM users WHERE id = ?2 AND teacher_id = ?1", params![teacher_id, id])?;
    }
    Ok(n)
}

/// One of the teacher's students, as a list shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Student {
    pub id: i64,
    pub username: String,
    pub first_name: String,
    pub last_name: String,
}

/// The teacher's students, by username.
pub fn list(conn: &Connection, teacher_id: i64) -> rusqlite::Result<Vec<Student>> {
    let mut stmt = conn.prepare_cached("SELECT id, COALESCE(username, ''), first_name, last_name FROM users WHERE teacher_id = ?1 ORDER BY username")?;
    let rows = stmt.query_map([teacher_id], |r| Ok(Student { id: r.get(0)?, username: r.get(1)?, first_name: r.get(2)?, last_name: r.get(3)? }))?;
    rows.collect()
}

/// One of the teacher's students, or `None` when the id is not theirs.
pub fn find(conn: &Connection, teacher_id: i64, student_id: i64) -> rusqlite::Result<Option<Student>> {
    conn.query_row(
        "SELECT id, COALESCE(username, ''), first_name, last_name FROM users WHERE id = ?2 AND teacher_id = ?1",
        params![teacher_id, student_id],
        |r| Ok(Student { id: r.get(0)?, username: r.get(1)?, first_name: r.get(2)?, last_name: r.get(3)? }),
    )
    .optional()
}

/// The teacher changes a student's first and last name (already checked
/// with `profile::validate_name`). False when the student is not theirs.
pub fn set_names(conn: &Connection, teacher_id: i64, student_id: i64, first_name: &str, last_name: &str) -> rusqlite::Result<bool> {
    let n = conn.execute("UPDATE users SET first_name = ?3, last_name = ?4 WHERE id = ?2 AND teacher_id = ?1", params![teacher_id, student_id, first_name, last_name])?;
    Ok(n > 0)
}

/// How a student is getting on, as the Students page shows it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Progress {
    /// Lessons passed, in every course (`lesson_passes.passed_at`, the one
    /// rule of `progress.rs`, as `progress::passed_by_course` counts them).
    pub passed: i64,
    /// When it last worked on a lesson, in any mode or course (the latest of
    /// `activity::last_touched`'s times); `None` for never.
    pub last_studied: Option<i64>,
}

/// Every student of the teacher's progress, by student id: two grouped
/// queries for the whole class, not two per student. A student with no
/// row has done nothing yet (`Progress::default()`).
pub fn progress(conn: &Connection, teacher_id: i64) -> rusqlite::Result<std::collections::HashMap<i64, Progress>> {
    let mut out: std::collections::HashMap<i64, Progress> = std::collections::HashMap::new();
    let mut stmt = conn.prepare_cached(
        "SELECT p.user_id, COUNT(*) FROM lesson_passes p JOIN users u ON u.id = p.user_id
         WHERE u.teacher_id = ?1 AND p.passed_at IS NOT NULL GROUP BY p.user_id",
    )?;
    for row in stmt.query_map([teacher_id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))? {
        let (id, n) = row?;
        out.entry(id).or_default().passed = n;
    }
    let mut stmt = conn.prepare_cached(
        "SELECT user_id, MAX(at) FROM (
           SELECT t.user_id, t.at FROM lesson_touches t JOIN users u ON u.id = t.user_id WHERE u.teacher_id = ?1
           UNION ALL
           SELECT p.user_id, p.seen_at FROM lesson_passes p JOIN users u ON u.id = p.user_id WHERE u.teacher_id = ?1 AND p.seen_at IS NOT NULL
         ) GROUP BY user_id",
    )?;
    for row in stmt.query_map([teacher_id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))? {
        let (id, at) = row?;
        out.entry(id).or_default().last_studied = Some(at);
    }
    Ok(out)
}

// ---------- courses given to students (#220) ----------
//
// A given course is a `course_shares` row with the student already in it
// (`user_id`, `opened_at`) and `given = 1` (schema v11). Everything a
// shared course does works for it unchanged: read only for the student,
// its own progress, the teacher's later changes reaching it at once. Its
// token is random and never shown, so no link leads to it; the Share
// dialog leaves it out and it does not count toward a course's links
// (`share::shares_of`). A student has no Privacy permission, so its
// teacher sees its progress (`progress`) without asking.

/// One course given to one of the teacher's students.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Given {
    pub student_id: i64,
    pub course_id: String,
    /// The course's title (`courses.title`).
    pub title: String,
}

/// The SQL test of a course the teacher may give: their own, not being
/// made or failed, and loading (no stored load error). `?1` is the
/// teacher, `?2` the course. The caller also checks it loads now.
const GIVABLE_SQL: &str = "SELECT EXISTS(SELECT 1 FROM courses WHERE id = ?2 AND owner_id = ?1 AND state IN ('ready', 'revising') AND error IS NULL)";

/// The teacher's own courses it may give, `(id, title)`, by title. The
/// routes leave out the ones that do not load now (`ContentStore`).
pub fn givable(conn: &Connection, teacher_id: i64) -> rusqlite::Result<Vec<(String, String)>> {
    let mut courses: Vec<(String, String)> = crate::courses::courses_owned_by(conn, teacher_id)?
        .into_iter()
        .filter(|c| matches!(c.state, crate::courses::CourseState::Ready | crate::courses::CourseState::Revising) && c.error.is_none())
        .map(|c| (c.id, c.title))
        .collect();
    courses.sort_by_key(|c| c.1.to_lowercase());
    Ok(courses)
}

/// The teacher gives one of its own courses to one of its students: from
/// now on it is one of the student's subjects. Giving it again changes
/// nothing. False when the student is not the teacher's or the course is
/// not one it may give.
pub fn give(conn: &Connection, teacher_id: i64, student_id: i64, course_id: &str) -> rusqlite::Result<bool> {
    let student: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM users WHERE id = ?2 AND teacher_id = ?1)", params![teacher_id, student_id], |r| r.get(0))?;
    let course: bool = conn.query_row(GIVABLE_SQL, params![teacher_id, course_id], |r| r.get(0))?;
    if !student || !course {
        return Ok(false);
    }
    let now = crate::db::now();
    conn.execute(
        "INSERT OR IGNORE INTO course_shares (course_id, label, token_hash, user_id, created_at, opened_at, given)
         VALUES (?1, '', ?2, ?3, ?4, ?4, 1)",
        params![course_id, crate::share::hash_token(&crate::auth::generate_token()), student_id, now],
    )?;
    Ok(true)
}

/// The teacher takes a given course away from its student: it leaves the
/// student's subjects (and is no longer its active course). The student's
/// progress rows stay, for the day it is given again. False when it was
/// not given to the teacher's student.
pub fn take_away(conn: &Connection, teacher_id: i64, student_id: i64, course_id: &str) -> rusqlite::Result<bool> {
    let n = conn.execute(
        "DELETE FROM course_shares WHERE given = 1 AND course_id = ?3 AND user_id = ?2
           AND EXISTS(SELECT 1 FROM users WHERE id = ?2 AND teacher_id = ?1)
           AND EXISTS(SELECT 1 FROM courses WHERE id = ?3 AND owner_id = ?1)",
        params![teacher_id, student_id, course_id],
    )?;
    if n > 0 {
        crate::share::forget(conn, course_id, Some(student_id))?;
    }
    Ok(n > 0)
}

/// Every course the teacher gave to its students, one row per student and
/// course, by course title. The students a course is given to, or the
/// courses a student has: filter on `course_id` or `student_id`.
pub fn given(conn: &Connection, teacher_id: i64) -> rusqlite::Result<Vec<Given>> {
    let mut stmt = conn.prepare_cached(
        "SELECT s.user_id, c.id, c.title FROM users u
           JOIN course_shares s ON s.user_id = u.id AND s.given = 1
           JOIN courses c ON c.id = s.course_id AND c.owner_id = ?1
         WHERE u.teacher_id = ?1 ORDER BY c.title COLLATE NOCASE, c.id",
    )?;
    let rows = stmt.query_map([teacher_id], |r| Ok(Given { student_id: r.get(0)?, course_id: r.get(1)?, title: r.get(2)? }))?;
    rows.collect()
}

/// The courses given to this student (its subjects), each with its
/// teacher's name (`share::owner_label`), oldest given first. The caller
/// leaves out the ones that do not load.
pub fn subjects(conn: &Connection, student_id: i64) -> rusqlite::Result<Vec<crate::share::SharedCourse>> {
    let mut stmt = conn.prepare_cached(
        "SELECT s.course_id, o.first_name, o.last_name, COALESCE(o.email, '')
         FROM course_shares s JOIN courses c ON c.id = s.course_id JOIN users o ON o.id = c.owner_id
         WHERE s.user_id = ?1 AND s.given = 1 AND c.state != 'generating' ORDER BY s.id",
    )?;
    let rows = stmt.query_map([student_id], |r| {
        Ok(crate::share::SharedCourse { course_id: r.get(0)?, owner: crate::share::owner_label(r.get(1)?, r.get(2)?, r.get(3)?) })
    })?;
    rows.collect()
}

/// A username made from a name (`Ana García` gives `ana.garcia`), in use
/// by nobody on the server and not in `also_taken` (the other rows of the
/// same batch): a number goes on the end until it is free.
pub fn suggest_username(conn: &Connection, name: &str, also_taken: &[String]) -> rusqlite::Result<String> {
    let words: Vec<String> = name.split_whitespace().map(fold).filter(|w| !w.is_empty()).collect();
    let mut base = words.join(".");
    base.truncate(USERNAME_MAX - 3);
    let base = base.trim_matches(|c| matches!(c, '.' | '_' | '-')).to_string();
    let base = if base.chars().count() < USERNAME_MIN { "student".to_string() } else { base };
    let mut candidate = base.clone();
    let mut n = 1;
    while also_taken.contains(&candidate) || username_taken(conn, &candidate)? {
        n += 1;
        candidate = format!("{base}{n}");
    }
    Ok(candidate)
}

/// A word in the characters a username takes: lower case, accents dropped,
/// anything else left out.
fn fold(word: &str) -> String {
    const ACCENTS: &[(&str, &str)] = &[
        ("a", "àáâãäåā"),
        ("c", "çć"),
        ("e", "èéêëē"),
        ("i", "ìíîïī"),
        ("n", "ñń"),
        ("o", "òóôõöøō"),
        ("u", "ùúûüū"),
        ("y", "ýÿ"),
        ("ss", "ß"),
    ];
    let mut out = String::new();
    for c in word.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
        } else if let Some((plain, _)) = ACCENTS.iter().find(|(_, marked)| marked.contains(c)) {
            out.push_str(plain);
        }
    }
    out
}

/// The letters and digits a made password is drawn from: none that are
/// easy to mix up (no `0`, `o`, `1`, `l`, `i`).
pub const PASSWORD_CHARS: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";

/// A first password to hand to a student: eight of [`PASSWORD_CHARS`].
/// Only its hash is kept.
pub fn make_password() -> String {
    let mut bytes = [0u8; 8];
    crate::random::fill(&mut bytes);
    // 256 is not a multiple of 31: draw again past the last whole multiple,
    // so every character is as likely as the others.
    let limit = 256 - 256 % PASSWORD_CHARS.len();
    let mut out = String::with_capacity(8);
    let mut i = 0;
    while out.len() < 8 {
        if i == bytes.len() {
            crate::random::fill(&mut bytes);
            i = 0;
        }
        let b = usize::from(bytes[i]); // panic-ok: i < bytes.len(), reset above
        i += 1;
        if b < limit {
            out.push(char::from(PASSWORD_CHARS[b % PASSWORD_CHARS.len()])); // panic-ok: a remainder of the length
        }
    }
    out
}

/// The key the sign-in limits count a sign-in name under: a username in
/// lower case (it is looked up in any case), an address as it was typed.
pub fn sign_in_key(login: &str) -> String {
    let login = login.trim();
    if login.contains('@') { login.to_string() } else { login.to_lowercase() }
}

/// Paths a student account never reaches: they answer 404, page or API.
/// Making, copying, editing, sharing or generating a course and opening an
/// invitation link (`/courses/...` but a course's own settings, stat and
/// placement test); the tutor and every AI feature; API keys, connected
/// apps and the MCP server; two-factor sign-in; Manage Users. (Mini has no
/// account deletion of its own.) `path` is the app path, without the base
/// path. The router's `auth::student_gate` layer checks it before any
/// handler, so a new route needs no check of its own: add its path here.
pub fn refused(path: &str) -> bool {
    const REFUSED: &[&str] = &[
        "/courses/new",
        "/courses/generate",
        "/courses/shared",
        "/api/courses",
        "/api/generate",
        "/api/review-quiet",
        "/tutor",
        "/api/tutor",
        "/oauth",
        "/mcp",
        "/api/v1",
        "/profile/apps",
        "/profile/two-factor",
        "/manage",
        "/api/manage",
        "/superuser",
        "/students",
    ];
    if REFUSED.iter().any(|p| path == *p || path.strip_prefix(p).is_some_and(|rest| rest.starts_with('/'))) {
        return true;
    }
    // A course's own pages: a student opens one, sets it up, takes its
    // placement test and reads its stats; the editor, its files, copies,
    // shares and AI runs are the owner's.
    match path.strip_prefix("/courses/").map(|rest| rest.split('/').collect::<Vec<_>>()).as_deref() {
        None | Some(["open", _]) => false,
        Some([_, "settings" | "placement"] | [_, "stat", _]) => false,
        Some(_) => true,
    }
}

#[cfg(test)]
pub(crate) mod tests;
