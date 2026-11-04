//! Sharing a course with other learners (#451, #16). The owner makes
//! invitation links on the course's page, each with a name of their choice
//! (or none) to tell it from the others; the Share dialog shows a new link
//! once, for the owner to pass on however they like (nothing is mailed:
//! this server has no e-mail). A learner who opens the link signed in has
//! the course on their course list from then on, read only: they learn from
//! it with their own progress, and only the owner can change its files.
//! Signed out, the link leads to the login page, which says what the link
//! is for and comes back here after signing in or registering.
//!
//! The link is the only way in (#10): the first account to open it keeps
//! it, and every other account that opens it later is turned away. The
//! name on a link is a label for the owner, never proof of who someone is.
//! `can_open` asks whether this account opened a link of the course.
//!
//! Table `course_shares`: one row per link. The link's token is stored as
//! its SHA-256 only.

use rusqlite::{params, Connection, OptionalExtension};

/// Links one course can have.
pub const MAX_PER_COURSE: usize = 50;
/// The longest name a link may carry (characters); the rest is cut.
pub const MAX_LABEL: usize = 60;

/// One invitation link of a course.
#[derive(Debug, Clone)]
#[allow(dead_code)] // created_at is for the dialog's "made … ago" to come
pub struct Share {
    pub id: i64,
    pub course_id: String,
    /// The name the owner gave the link, or "".
    pub label: String,
    /// The account that opened the link (NULL until then).
    pub user_id: Option<i64>,
    pub created_at: i64,
    pub opened_at: Option<i64>,
    /// Who opened it, for the owner: that account's name, or its address.
    pub opened_by: Option<String>,
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
    let user_id: Option<i64> = r.get(3)?;
    let opened_by = match user_id {
        Some(_) => Some(owner_label(r.get(6)?, r.get(7)?, r.get(8)?)),
        None => None,
    };
    Ok(Share { id: r.get(0)?, course_id: r.get(1)?, label: r.get(2)?, user_id, created_at: r.get(4)?, opened_at: r.get(5)?, opened_by })
}

/// A share with the account that opened it (`u.*` NULL until then).
const SHARE_SELECT: &str = "SELECT s.id, s.course_id, s.label, s.user_id, s.created_at, s.opened_at,
            COALESCE(u.first_name, ''), COALESCE(u.last_name, ''), COALESCE(u.email, '')
         FROM course_shares s LEFT JOIN users u ON u.id = s.user_id";

/// Every link of a course, oldest first.
pub fn shares_of(conn: &Connection, course_id: &str) -> rusqlite::Result<Vec<Share>> {
    let mut stmt = conn.prepare_cached(&format!("{SHARE_SELECT} WHERE s.course_id = ?1 ORDER BY s.id"))?;
    let rows = stmt.query_map([course_id], share_row)?;
    rows.collect()
}

/// A new invitation link for the course, named `label` ("" for none; the
/// caller trims and cuts it). The link's token.
pub fn invite(conn: &Connection, course_id: &str, label: &str, now: i64) -> rusqlite::Result<String> {
    let token = crate::auth::generate_token();
    conn.execute(
        "INSERT INTO course_shares (course_id, label, token_hash, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![course_id, label, hash_token(&token), now],
    )?;
    Ok(token)
}

/// Take the link `share_id` of the course away: the row goes, and so does
/// the recipient's project in it (the course leaves their list; their
/// progress rows stay for the day it is shared again). True when there was
/// such a link.
pub fn remove(conn: &Connection, course_id: &str, share_id: i64) -> rusqlite::Result<bool> {
    let row: Option<(i64, Option<i64>)> = conn
        .query_row("SELECT id, user_id FROM course_shares WHERE course_id = ?1 AND id = ?2", params![course_id, share_id], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    let Some((id, user_id)) = row else { return Ok(false) };
    conn.execute("DELETE FROM course_shares WHERE id = ?1", [id])?;
    // Whoever the link let in, unless another link of theirs still does.
    let still: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM course_shares WHERE course_id = ?1 AND user_id = ?2)",
        params![course_id, user_id],
        |r| r.get(0),
    )?;
    if !still {
        conn.execute(REMOVE_PROJECTS_SQL, params![course_id, user_id])?;
        conn.execute(REMOVE_ACTIVE_SQL, params![course_id, user_id])?;
    }
    Ok(true)
}

// The account a share let in, found by id (#622), never by a scan of `users`.
const REMOVE_PROJECTS_SQL: &str = "DELETE FROM projects WHERE course_id = ?1 AND user_id = ?2
         AND user_id NOT IN (SELECT owner_id FROM courses WHERE id = ?1)";
const REMOVE_ACTIVE_SQL: &str = "UPDATE users SET active_course = NULL WHERE active_course = ?1 AND id = ?2
         AND id NOT IN (SELECT owner_id FROM courses WHERE id = ?1)";

/// The share a link's token names.
pub fn by_token(conn: &Connection, token: &str) -> rusqlite::Result<Option<Share>> {
    conn.query_row(&format!("{SHARE_SELECT} WHERE s.token_hash = ?1"), [hash_token(token)], share_row).optional()
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
/// else in).
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

/// May this account open the course through a share?
pub fn can_open(conn: &Connection, user_id: i64, course_id: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM course_shares s JOIN users u ON u.id = ?1
            JOIN courses c ON c.id = s.course_id JOIN users o ON o.id = c.owner_id
            WHERE s.course_id = ?2
              AND s.user_id = u.id)",
        params![user_id, course_id],
        |r| r.get(0),
    )
}

/// The shares an account opened, one indexed lookup (#621,
/// `idx_course_shares_user`), one row per course.
const SHARED_WITH_SQL: &str = "SELECT s.course_id, o.first_name, o.last_name, COALESCE(o.email, ''), MAX(s.id)
         FROM (SELECT id, course_id FROM course_shares WHERE user_id = ?1) s
         JOIN courses c ON c.id = s.course_id JOIN users o ON o.id = c.owner_id
         WHERE c.owner_id != ?1
         GROUP BY s.course_id
         ORDER BY MAX(s.id) DESC";

/// The courses shared with this account (links it opened), newest share first, leaving out the ones taken down or of a
/// owner waiting to be deleted (#970). The caller skips the ones that do not load.
pub fn shared_with(conn: &Connection, user_id: i64) -> rusqlite::Result<Vec<SharedCourse>> {
    let mut stmt = conn.prepare_cached(SHARED_WITH_SQL)?;
    let rows = stmt.query_map([user_id], |r| {
        Ok(SharedCourse { course_id: r.get(0)?, owner: owner_label(r.get(1)?, r.get(2)?, r.get(3)?) })
    })?;
    rows.collect()
}

/// How an account is named to someone else: "First Last", or the address
/// when the profile has no name.
pub fn owner_label(first: String, last: String, email: String) -> String {
    let name = format!("{} {}", first.trim(), last.trim()).trim().to_string();
    if name.is_empty() {
        email
    } else {
        name
    }
}

/// The name typed into the share form, as stored: trimmed, inner runs of
/// whitespace made one space, at most `MAX_LABEL` characters.
pub fn clean_label(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(MAX_LABEL).collect()
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
    fn a_link_lets_one_account_in_and_its_name_is_only_a_label() {
        let db = Db::open_in_memory().unwrap();
        db.with(|c| {
            let ann = user(c, "ann@example.org");
            let bob = user(c, "bob@example.org");
            let carol = user(c, "carol@example.org");
            let dan = user(c, "dan@example.org");
            course(c, ann, "fractions-1");
            let token = invite(c, "fractions-1", "Bob", 10).unwrap();
            // Two links with the same name, and unnamed ones: all fine.
            let token_b2 = invite(c, "fractions-1", "Bob", 11).unwrap();
            let token_none = invite(c, "fractions-1", "", 11).unwrap();
            assert_ne!(token, token_b2);
            assert_eq!(shares_of(c, "fractions-1").unwrap().len(), 3);
            assert!(!can_open(c, bob, "fractions-1").unwrap(), "the link is needed");

            let share = by_token(c, &token).unwrap().expect("the share by its token");
            assert_eq!((share.label.as_str(), share.opened_by.as_deref()), ("Bob", None));
            assert!(by_token(c, "nope").unwrap().is_none());
            // The owner opening their own link (a preview) does not take it.
            assert_eq!(claim(c, &share, ann, 11).unwrap(), Claim::Owner);
            assert_eq!(by_token(c, &token).unwrap().unwrap().user_id, None);
            // A forwarded link: whoever opens it first keeps it, whatever its name says.
            assert_eq!(claim(c, &share, dan, 12).unwrap(), Claim::Ok);
            assert!(can_open(c, dan, "fractions-1").unwrap());
            let share = by_token(c, &token).unwrap().unwrap();
            assert_eq!((share.user_id, share.opened_at), (Some(dan), Some(12)));
            assert_eq!(share.opened_by.as_deref(), Some("dan@example.org"), "no name: the address");
            // The person it was named for comes too late: turned away like anyone else.
            assert_eq!(claim(c, &share, bob, 13).unwrap(), Claim::Taken);
            assert!(!can_open(c, bob, "fractions-1").unwrap());
            assert!(shared_with(c, bob).unwrap().is_empty());
            assert_eq!(claim(c, &share, dan, 14).unwrap(), Claim::Ok);

            // An account with the same address as a link's name: nothing until a link is opened.
            assert!(!can_open(c, carol, "fractions-1").unwrap());
            assert!(shared_with(c, carol).unwrap().is_empty());
            assert_eq!(claim(c, &by_token(c, &token_none).unwrap().unwrap(), carol, 21).unwrap(), Claim::Ok);
            let mine: Vec<String> = shared_with(c, carol).unwrap().into_iter().map(|s| s.course_id).collect();
            assert_eq!(mine, vec!["fractions-1".to_string()]);
            assert_eq!(shared_with(c, carol).unwrap()[0].owner, "ann@example.org", "no name: the address");
            c.execute("UPDATE users SET first_name = 'Ann', last_name = 'Lee' WHERE id = ?1", [ann]).unwrap();
            assert_eq!(shared_with(c, carol).unwrap()[0].owner, "Ann Lee");
            assert!(shared_with(c, ann).unwrap().is_empty(), "the owner's own course is not shared with them");
            c.execute("UPDATE users SET first_name = 'Carol' WHERE id = ?1", [carol]).unwrap();
            assert_eq!(by_token(c, &token_none).unwrap().unwrap().opened_by.as_deref(), Some("Carol"));

            // Two links opened by the same account: one course on their list; removing
            // one link keeps the course until the other goes too.
            assert_eq!(claim(c, &by_token(c, &token_b2).unwrap().unwrap(), carol, 22).unwrap(), Claim::Ok);
            assert_eq!(shared_with(c, carol).unwrap().len(), 1);
            let b2 = by_token(c, &token_b2).unwrap().unwrap();
            assert!(remove(c, "fractions-1", b2.id).unwrap());
            assert!(can_open(c, carol, "fractions-1").unwrap(), "the other link still lets her in");
            let none = by_token(c, &token_none).unwrap().unwrap();
            assert!(remove(c, "fractions-1", none.id).unwrap());
            assert!(!can_open(c, carol, "fractions-1").unwrap());
            assert!(shared_with(c, carol).unwrap().is_empty());
            assert!(by_token(c, &token_none).unwrap().is_none(), "the link went with the share");
            assert!(!remove(c, "fractions-1", none.id).unwrap(), "already gone");
            assert!(!remove(c, "other-course", share.id).unwrap(), "a link of another course is not this course's to remove");
            assert!(can_open(c, dan, "fractions-1").unwrap(), "dan's link stays");
            Ok::<(), rusqlite::Error>(())
        })
        .unwrap();
    }

    #[test]
    fn a_links_name_is_trimmed_and_cut() {
        assert_eq!(clean_label("  Bob   from \n work  "), "Bob from work");
        assert_eq!(clean_label(""), "");
        assert_eq!(clean_label(&"é".repeat(100)).chars().count(), MAX_LABEL);
    }
}
