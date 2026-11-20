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
#[cfg_attr(not(any(test, feature = "dev")), allow(dead_code))] // made by /dev/student and the tests until the Students page (#218)
#[derive(Debug, Clone, Default)]
pub struct NewStudent {
    pub username: String,
    pub password_hash: String,
    pub first_name: String,
    pub last_name: String,
}

/// Why no student account was made.
#[cfg_attr(not(any(test, feature = "dev")), allow(dead_code))] // made by /dev/student and the tests until the Students page (#218)
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
#[cfg_attr(not(any(test, feature = "dev")), allow(dead_code))] // made by /dev/student and the tests until the Students page (#218)
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
#[cfg_attr(not(test), allow(dead_code))] // the Students page (#218) calls it
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

/// Make student accounts for `teacher_id`: every one of `students`, or none.
/// Each username is checked and stored as [`normalize_username`] gives it;
/// the box may not go over its student limit (`caps::students_full`; the
/// dev server is never full). The new accounts' ids, in order.
#[cfg_attr(not(any(test, feature = "dev")), allow(dead_code))] // made by /dev/student and the tests until the Students page (#218)
pub fn create(conn: &Connection, teacher_id: i64, students: &[NewStudent], dev: bool) -> rusqlite::Result<Result<Vec<i64>, Refused>> {
    if !may_hold(conn, teacher_id)? {
        return Ok(Err(Refused::NotTeacher));
    }
    if students.is_empty() {
        return Ok(Ok(Vec::new()));
    }
    // `students_full(n)` is "n is at the limit": the last new one must still
    // find room, so the count before it is what is asked.
    if crate::caps::students_full(count_all(conn)? + students.len() as i64 - 1, dev) {
        return Ok(Err(Refused::Full));
    }
    let mut names: Vec<String> = Vec::with_capacity(students.len());
    for s in students {
        let name = match normalize_username(&s.username) {
            Ok(n) => n,
            Err(why) => return Ok(Err(Refused::BadUsername(s.username.clone(), why))),
        };
        if names.contains(&name) || username_taken(conn, &name)? {
            return Ok(Err(Refused::Taken(name)));
        }
        names.push(name);
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
#[cfg_attr(not(test), allow(dead_code))] // the Students page (#218) calls it
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
#[cfg_attr(not(test), allow(dead_code))] // the Students page (#218) calls it
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
#[cfg_attr(not(test), allow(dead_code))] // the Students page (#218) calls it
pub fn delete(conn: &Connection, teacher_id: i64, student_ids: &[i64]) -> rusqlite::Result<usize> {
    let mut n = 0;
    for id in student_ids {
        n += conn.execute("DELETE FROM users WHERE id = ?2 AND teacher_id = ?1", params![teacher_id, id])?;
    }
    Ok(n)
}

/// One of the teacher's students, as a list shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))] // the Students page (#218) reads it
pub struct Student {
    pub id: i64,
    pub username: String,
    pub first_name: String,
    pub last_name: String,
}

/// The teacher's students, by username.
#[cfg_attr(not(test), allow(dead_code))] // the Students page (#218) calls it
pub fn list(conn: &Connection, teacher_id: i64) -> rusqlite::Result<Vec<Student>> {
    let mut stmt = conn.prepare_cached("SELECT id, COALESCE(username, ''), first_name, last_name FROM users WHERE teacher_id = ?1 ORDER BY username")?;
    let rows = stmt.query_map([teacher_id], |r| Ok(Student { id: r.get(0)?, username: r.get(1)?, first_name: r.get(2)?, last_name: r.get(3)? }))?;
    rows.collect()
}

/// A username made from a name (`Ana García` gives `ana.garcia`), in use
/// by nobody on the server and not in `also_taken` (the other rows of the
/// same batch): a number goes on the end until it is free.
#[cfg_attr(not(test), allow(dead_code))] // the Students page (#218) calls it
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
#[cfg_attr(not(test), allow(dead_code))] // the Students page (#218) calls it
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
