//! Sharing a course with other learners (#451). The owner types e-mail
//! addresses on the course's page; every address gets a link, which the
//! Share dialog shows the owner once to pass on (nothing is mailed).
//! A learner who opens the link signed in has the course on their course
//! list from then on, read only: they learn from it with their own progress,
//! and only the owner can change its files. Signed out, the link leads to
//! the login page, which says what the link is for and comes back here after
//! signing in or registering.
//!
//! The link is the only way in (#10): the first account to open it keeps
//! it, and every other account that opens it later is turned away, the
//! invited address included. Nothing confirms an e-mail address, so the
//! address on a share is only a label for the owner, never proof of who
//! someone is. `can_open` asks whether this account opened the link.
//!
//! Table `course_shares`: one row per (course, address). The link's token
//! is stored as its SHA-256 only.

use rusqlite::{params, Connection, OptionalExtension};

/// Addresses one course can be shared with.
pub const MAX_PER_COURSE: usize = 50;
/// Addresses one share form may list.
pub const MAX_PER_REQUEST: usize = 20;

/// One address a course is shared with.
#[derive(Debug, Clone)]
#[allow(dead_code)] // created_at is for the dialog's "invited … ago" to come
pub struct Share {
    pub id: i64,
    pub course_id: String,
    pub email: String,
    /// The account that opened the link (NULL until then).
    pub user_id: Option<i64>,
    pub created_at: i64,
    pub opened_at: Option<i64>,
}

impl Share {
    pub fn opened(&self) -> bool {
        self.opened_at.is_some()
    }
}

/// A course shared with a learner, for their course list.
#[derive(Debug, Clone)]
pub struct SharedCourse {
    pub course_id: String,
    /// Who shared it: the owner's name, or their address.
    pub owner: String,
}

/// The stored form of a link's token: SHA-256, hex.
pub fn hash_token(token: &str) -> String {
    crate::auth::hash_token(token)
}

fn share_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Share> {
    Ok(Share { id: r.get(0)?, course_id: r.get(1)?, email: r.get(2)?, user_id: r.get(3)?, created_at: r.get(4)?, opened_at: r.get(5)? })
}

const SHARE_COLS: &str = "id, course_id, email, user_id, created_at, opened_at";

/// Everyone a course is shared with, in the order they were added.
pub fn shares_of(conn: &Connection, course_id: &str) -> rusqlite::Result<Vec<Share>> {
    let mut stmt = conn.prepare_cached(&format!("SELECT {SHARE_COLS} FROM course_shares WHERE course_id = ?1 ORDER BY id"))?;
    let rows = stmt.query_map([course_id], share_row)?;
    rows.collect()
}

/// Share the course with `email` (lower case, validated by the caller).
/// The link's token, or `None` when that address already has one.
pub fn invite(conn: &Connection, course_id: &str, email: &str, now: i64) -> rusqlite::Result<Option<String>> {
    let token = crate::auth::generate_token();
    let n = conn.execute(
        "INSERT OR IGNORE INTO course_shares (course_id, email, token_hash, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![course_id, email, hash_token(&token), now],
    )?;
    Ok((n > 0).then_some(token))
}

/// Stop sharing the course with `email`: the row goes, and so does the
/// recipient's project in it (the course leaves their list; their progress
/// rows stay for the day it is shared again). True when there was a share.
pub fn remove(conn: &Connection, course_id: &str, email: &str) -> rusqlite::Result<bool> {
    let row: Option<(i64, Option<i64>)> = conn
        .query_row("SELECT id, user_id FROM course_shares WHERE course_id = ?1 AND email = ?2", params![course_id, email], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    let Some((id, user_id)) = row else { return Ok(false) };
    conn.execute("DELETE FROM course_shares WHERE id = ?1", [id])?;
    // Whoever the share let in: the account that opened the link.
    conn.execute(REMOVE_PROJECTS_SQL, params![course_id, user_id])?;
    conn.execute(REMOVE_ACTIVE_SQL, params![course_id, user_id])?;
    Ok(true)
}

// The account a share let in, found by id (#622), never by a scan of `users`.
const REMOVE_PROJECTS_SQL: &str = "DELETE FROM projects WHERE course_id = ?1 AND user_id = ?2
         AND user_id NOT IN (SELECT owner_id FROM courses WHERE id = ?1)";
const REMOVE_ACTIVE_SQL: &str = "UPDATE users SET active_course = NULL WHERE active_course = ?1 AND id = ?2
         AND id NOT IN (SELECT owner_id FROM courses WHERE id = ?1)";

/// The share a link's token names.
pub fn by_token(conn: &Connection, token: &str) -> rusqlite::Result<Option<Share>> {
    conn.query_row(&format!("SELECT {SHARE_COLS} FROM course_shares WHERE token_hash = ?1"), [hash_token(token)], share_row).optional()
}

/// What opening a link signed in does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Claim {
    /// The course is theirs to open now.
    Ok,
    /// Another account opened this link first; it works for that account only.
    Taken,
    /// The course's owner opened their own link (a preview):
    /// it is not claimed, so the invited learner can still use it.
    Owner,
}

/// The signed-in account opens the link: the share is theirs from now on
/// (the first account to open it keeps it, so a forwarded link lets nobody
/// else in, the invited address included).
pub fn claim(conn: &Connection, share: &Share, user_id: i64, now: i64) -> rusqlite::Result<Claim> {
    let owner: Option<i64> = conn.query_row("SELECT owner_id FROM courses WHERE id = ?1", [&share.course_id], |r| r.get(0)).optional()?;
    if owner == Some(user_id) {
        return Ok(Claim::Owner);
    }
    match share.user_id {
        Some(u) if u == user_id => return Ok(Claim::Ok),
        Some(_) => return Ok(Claim::Taken),
        None => {}
    }
    conn.execute(
        "UPDATE course_shares SET user_id = ?2, opened_at = COALESCE(opened_at, ?3) WHERE id = ?1",
        params![share.id, user_id, now],
    )?;
    Ok(Claim::Ok)
}

/// May this account open the course through a share? Not while its owner
/// is waiting to be deleted (#970).
pub fn can_open(conn: &Connection, user_id: i64, course_id: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM course_shares s JOIN users u ON u.id = ?1
            JOIN courses c ON c.id = s.course_id JOIN users o ON o.id = c.owner_id
            WHERE s.course_id = ?2 AND o.delete_at IS NULL
              AND s.user_id = u.id)",
        params![user_id, course_id],
        |r| r.get(0),
    )
}

/// The shares an account opened, one indexed lookup (#621,
/// `idx_course_shares_user`).
const SHARED_WITH_SQL: &str = "SELECT s.course_id, o.first_name, o.last_name, COALESCE(o.email, ''), s.id
         FROM (SELECT id, course_id FROM course_shares WHERE user_id = ?1) s
         JOIN courses c ON c.id = s.course_id JOIN users o ON o.id = c.owner_id
         WHERE c.owner_id != ?1 AND o.delete_at IS NULL
         ORDER BY s.id DESC";

/// The courses shared with this account (links it opened), newest share first, leaving out the ones taken down or of a
/// owner waiting to be deleted (#970). The caller skips the ones that do not load.
pub fn shared_with(conn: &Connection, user_id: i64) -> rusqlite::Result<Vec<SharedCourse>> {
    let mut stmt = conn.prepare_cached(SHARED_WITH_SQL)?;
    let rows = stmt.query_map([user_id], |r| {
        Ok(SharedCourse { course_id: r.get(0)?, owner: owner_label(r.get(1)?, r.get(2)?, r.get(3)?) })
    })?;
    rows.collect()
}

/// How a recipient sees the owner: "First Last", or the address when the
/// profile has no name.
pub fn owner_label(first: String, last: String, email: String) -> String {
    let name = format!("{} {}", first.trim(), last.trim()).trim().to_string();
    if name.is_empty() {
        email
    } else {
        name
    }
}

/// The addresses typed into the share form: separated by commas, semicolons,
/// spaces or new lines; each one validated and lower-cased, duplicates
/// dropped. The first bad one stops the whole list, so nothing is half sent.
pub fn parse_addresses(text: &str) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    for raw in text.split(|c: char| c == ',' || c == ';' || c.is_whitespace()).filter(|s| !s.is_empty()) {
        let email = crate::profile::validate_email(raw).map_err(|e| crate::t2("courses.share.bad_address", "address", raw, "error", e))?;
        if !out.contains(&email) {
            out.push(email);
        }
    }
    if out.is_empty() {
        return Err(crate::t("courses.share.at_least_one"));
    }
    if out.len() > MAX_PER_REQUEST {
        return Err(crate::t1("courses.share.too_many_request", "max", MAX_PER_REQUEST));
    }
    Ok(out)
}

/// The message on the login and register pages when a share link brought
/// the visitor there.
pub fn login_notice(title: &str) -> String {
    crate::t1("courses.share.login_notice", "title", title)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn user(c: &Connection, email: &str) -> i64 {
        crate::auth::create_user(c, email, "x").unwrap()
    }

    fn course(c: &mut Connection, owner: i64, id: &str) {
        let files = crate::courses::starter_files(id, "Fractions");
        crate::courses::create_course(c, owner, id, "Fractions", &files).unwrap();
    }

    #[test]
    fn a_link_lets_one_account_in_and_the_address_is_only_a_label() {
        let db = Db::open_in_memory().unwrap();
        db.with(|c| {
            let ann = user(c, "ann@example.org");
            let bob = user(c, "bob@example.org");
            let carol = user(c, "carol@example.org");
            let dan = user(c, "dan@example.org");
            course(c, ann, "fractions-1");
            let token = invite(c, "fractions-1", "bob@example.org", 10).unwrap().expect("a new share");
            assert!(invite(c, "fractions-1", "bob@example.org", 11).unwrap().is_none(), "one share per address");
            assert!(!can_open(c, bob, "fractions-1").unwrap(), "the link is needed");

            let share = by_token(c, &token).unwrap().expect("the share by its token");
            assert!(by_token(c, "nope").unwrap().is_none());
            // The owner opening their own link (a preview) does not take it.
            assert_eq!(claim(c, &share, ann, 11).unwrap(), Claim::Owner);
            assert_eq!(by_token(c, &token).unwrap().unwrap().user_id, None);
            // A forwarded link: whoever opens it first keeps it, even another address.
            assert_eq!(claim(c, &share, dan, 12).unwrap(), Claim::Ok);
            assert!(can_open(c, dan, "fractions-1").unwrap());
            let share = by_token(c, &token).unwrap().unwrap();
            assert_eq!((share.user_id, share.opened_at), (Some(dan), Some(12)));
            // The invited address comes too late: turned away like anyone else.
            assert_eq!(claim(c, &share, bob, 13).unwrap(), Claim::Taken);
            assert!(!can_open(c, bob, "fractions-1").unwrap());
            assert!(shared_with(c, bob).unwrap().is_empty());
            assert_eq!(claim(c, &share, dan, 14).unwrap(), Claim::Ok);

            // An address with an account: nothing until its link is opened.
            let token_c = invite(c, "fractions-1", "carol@example.org", 20).unwrap().unwrap();
            assert!(!can_open(c, carol, "fractions-1").unwrap());
            assert!(shared_with(c, carol).unwrap().is_empty());
            assert_eq!(claim(c, &by_token(c, &token_c).unwrap().unwrap(), carol, 21).unwrap(), Claim::Ok);
            let mine: Vec<String> = shared_with(c, carol).unwrap().into_iter().map(|s| s.course_id).collect();
            assert_eq!(mine, vec!["fractions-1".to_string()]);
            assert_eq!(shared_with(c, carol).unwrap()[0].owner, "ann@example.org", "no name: the address");
            c.execute("UPDATE users SET first_name = 'Ann', last_name = 'Lee' WHERE id = ?1", [ann]).unwrap();
            assert_eq!(shared_with(c, carol).unwrap()[0].owner, "Ann Lee");
            assert!(shared_with(c, ann).unwrap().is_empty(), "the owner's own course is not shared with them");

            // Removing a share takes the course off the list and out of the session of the account that opened it.
            crate::courses::open_project(c, dan, "fractions-1").unwrap();
            crate::courses::open_project(c, ann, "fractions-1").unwrap();
            assert!(remove(c, "fractions-1", "bob@example.org").unwrap());
            assert!(!remove(c, "fractions-1", "bob@example.org").unwrap());
            assert!(!can_open(c, dan, "fractions-1").unwrap());
            let active = |u: i64| c.query_row("SELECT active_course FROM users WHERE id = ?1", [u], |r| r.get::<_, Option<String>>(0)).unwrap();
            assert_eq!(active(dan), None);
            assert_eq!(active(ann).as_deref(), Some("fractions-1"), "the owner keeps it");
            let projects: i64 = c.query_row("SELECT COUNT(*) FROM projects WHERE course_id = 'fractions-1'", [], |r| r.get(0)).unwrap();
            assert_eq!(projects, 1);
            assert_eq!(shares_of(c, "fractions-1").unwrap().len(), 1);

            // Deleting the course deletes its shares; deleting the recipient keeps the share, unclaimed.
            assert!(by_token(c, &token).unwrap().is_none(), "removed");
            c.execute("DELETE FROM users WHERE id = ?1", [carol]).unwrap();
            assert_eq!(shares_of(c, "fractions-1").unwrap()[0].user_id, None);
            crate::courses::delete_course(c, "fractions-1").unwrap();
            assert!(shares_of(c, "fractions-1").unwrap().is_empty());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn share_lookups_read_through_indexes() {
        let db = Db::open_in_memory().unwrap();
        db.with(|c| {
            for sql in [SHARED_WITH_SQL, REMOVE_PROJECTS_SQL, REMOVE_ACTIVE_SQL] {
                assert_eq!(crate::db::table_scans(c, sql).unwrap(), Vec::<String>::new(), "{sql}");
            }
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn a_share_shows_once() {
        let db = Db::open_in_memory().unwrap();
        db.with(|c| {
            let ann = user(c, "ann@example.org");
            let carol = user(c, "carol@example.org");
            course(c, ann, "fractions-1");
            // Carol opens the link made for her own address: one entry.
            let token = invite(c, "fractions-1", "carol@example.org", 10).unwrap().unwrap();
            let share = by_token(c, &token).unwrap().unwrap();
            assert_eq!(claim(c, &share, carol, 11).unwrap(), Claim::Ok);
            assert_eq!(shared_with(c, carol).unwrap().len(), 1);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn addresses_are_split_checked_and_deduplicated() {
        assert_eq!(parse_addresses("Bob@Example.org, carol@example.org\nbob@example.org; dan@example.org").unwrap(), vec!["bob@example.org", "carol@example.org", "dan@example.org"]);
        assert!(parse_addresses("  \n").unwrap_err().contains("at least one"));
        assert!(parse_addresses("bob@example.org, not-an-address").unwrap_err().contains("not-an-address"));
        let many: Vec<String> = (0..=MAX_PER_REQUEST).map(|i| format!("u{i}@example.org")).collect();
        assert!(parse_addresses(&many.join(" ")).unwrap_err().contains("at a time"));
    }

    #[test]
    fn the_login_notice_names_the_course() {
        assert!(login_notice("Fractions").starts_with("It looks like you don't have an account with us!"));
    }
}
