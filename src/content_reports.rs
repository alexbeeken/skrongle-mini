//! Content reports and copyright notices (#968, Terms on copyright and on
//! harmful content). Two ways in: a learner reports a course someone shared
//! with them (the course's "shared with you" dialog on the course list), and
//! anyone, signed in or not, sends a copyright notice from `/legal/copyright`.
//! Each one is a row of `content_reports` and an e-mail to the operator
//! (`SKRONGLE_LEGAL_EMAIL`, or every superuser when it is unset). On the
//! Superuser page an open report can be forwarded to the course's owner (the
//! reporter's address only on a copyright notice, where the law expects it)
//! and resolved. What the operator does with them: `docs/content-reports.md`.
//!
//! Hosted only: both need mail, so Skrongle mini has neither (the copyright
//! page shows the contact address).

use rusqlite::{params, Connection, OptionalExtension};

use crate::mail::Mail;

/// The operator's copy of a new report or notice.
pub const PURPOSE: &str = "content-report";
/// The copy forwarded to the course's owner.
pub const PURPOSE_FORWARD: &str = "content-report-forward";
/// The longest text a report keeps, in characters (the rest is cut).
pub const MAX_TEXT: usize = 5000;
/// The longest one-line field (name, work, where).
pub const MAX_LINE: usize = 500;

/// Why someone reports a course.
pub const REASONS: &[&str] = &["harmful", "copyright", "other"];

/// A report or notice, as stored.
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub id: i64,
    /// The reporter's account; `None` for a notice sent signed out.
    pub user_id: Option<i64>,
    /// The course it is about, when known (`None`: a notice whose "where"
    /// names no course of this server).
    pub course_id: Option<String>,
    /// The course's title when the report came in.
    pub course_title: String,
    /// Where the material is, as typed on the copyright form.
    pub location: String,
    /// One of [`REASONS`].
    pub reason: String,
    /// The reporter's address.
    pub email: String,
    /// The claimant's name (copyright form).
    pub name: String,
    /// The work they say is copied (copyright form).
    pub work: String,
    pub details: String,
    /// The good-faith statement was ticked (copyright form).
    pub good_faith: bool,
    pub created_at: i64,
    pub forwarded_at: Option<i64>,
}

impl Report {
    pub fn is_copyright(&self) -> bool {
        self.reason == "copyright"
    }
}

/// Cut `s` to `max` characters, trimmed.
pub fn clip(s: &str, max: usize) -> String {
    s.trim().chars().take(max).collect()
}

/// Store a new report; its id.
pub fn insert(conn: &Connection, r: &Report) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO content_reports (user_id, course_id, course_title, location, reason, email, name, work, details, good_faith, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![r.user_id, r.course_id, r.course_title, r.location, r.reason, r.email, r.name, r.work, r.details, r.good_faith, r.created_at],
    )?;
    Ok(conn.last_insert_rowid())
}

const COLS: &str = "id, user_id, course_id, course_title, location, reason, email, name, work, details, good_faith, created_at, forwarded_at";

fn row(r: &rusqlite::Row) -> rusqlite::Result<Report> {
    Ok(Report {
        id: r.get(0)?,
        user_id: r.get(1)?,
        course_id: r.get(2)?,
        course_title: r.get(3)?,
        location: r.get(4)?,
        reason: r.get(5)?,
        email: r.get(6)?,
        name: r.get(7)?,
        work: r.get(8)?,
        details: r.get(9)?,
        good_faith: r.get(10)?,
        created_at: r.get(11)?,
        forwarded_at: r.get(12)?,
    })
}

/// The open reports, oldest first (the one waiting longest on top).
pub fn open(conn: &Connection) -> rusqlite::Result<Vec<Report>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLS} FROM content_reports WHERE state = 'open' ORDER BY created_at, id LIMIT 200"))?;
    let rows = stmt.query_map([], row)?.collect();
    rows
}

pub fn get(conn: &Connection, id: i64) -> rusqlite::Result<Option<Report>> {
    conn.query_row(&format!("SELECT {COLS} FROM content_reports WHERE id = ?1"), [id], row).optional()
}

pub fn mark_forwarded(conn: &Connection, id: i64, now: i64) -> rusqlite::Result<()> {
    conn.execute("UPDATE content_reports SET forwarded_at = ?2 WHERE id = ?1", params![id, now])?;
    Ok(())
}

/// Resolved: off the open list. `false` when there is no such open report.
pub fn resolve(conn: &Connection, id: i64, now: i64) -> rusqlite::Result<bool> {
    Ok(conn.execute("UPDATE content_reports SET state = 'resolved', resolved_at = ?2 WHERE id = ?1 AND state = 'open'", params![id, now])? > 0)
}

/// The made course a notice's "where" names: the first word of it (a link
/// to the course, or its id) that is the id of a course on this server, or
/// the token of a share link to one (`/courses/shared/<token>`, the only
/// link to a made course someone outside it is likely to have).
pub fn find_course(conn: &Connection, location: &str) -> rusqlite::Result<Option<(String, String)>> {
    let mut stmt = conn.prepare("SELECT id, title FROM courses WHERE id = ?1")?;
    let mut shared = conn.prepare("SELECT c.id, c.title FROM course_shares s JOIN courses c ON c.id = s.course_id WHERE s.token_hash = ?1")?;
    for word in location.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-')).filter(|w| w.len() > 7) {
        if let Some(found) = stmt.query_row([word.to_ascii_lowercase()], |r| Ok((r.get(0)?, r.get(1)?))).optional()? {
            return Ok(Some(found));
        }
        if let Some(found) = shared.query_row([crate::share::hash_token(word)], |r| Ok((r.get(0)?, r.get(1)?))).optional()? {
            return Ok(Some(found));
        }
    }
    Ok(None)
}

/// The owner of a made course: their account and address (`None` for a
/// course without one, or an owner without an address).
pub fn owner(conn: &Connection, course_id: &str) -> rusqlite::Result<Option<(i64, String)>> {
    conn.query_row(
        "SELECT u.id, u.email FROM courses c JOIN users u ON u.id = c.owner_id WHERE c.id = ?1 AND COALESCE(u.email, '') <> ''",
        [course_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
}

/// Who hears of a new report: `SKRONGLE_LEGAL_EMAIL` when it is set, else
/// every superuser with an address.
pub fn operator_addresses(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    let legal = crate::terms::legal_email();
    if legal != crate::terms::EMAIL_PLACEHOLDER && legal.contains('@') {
        return Ok(vec![legal]);
    }
    let mut stmt = conn.prepare("SELECT email FROM users WHERE superuser = 1 AND COALESCE(email, '') <> '' ORDER BY id")?;
    let rows = stmt.query_map([], |r| r.get(0))?.collect();
    rows
}

/// The reason in words, in the request's language.
pub fn reason_label(reason: &str) -> String {
    crate::t(match reason {
        "harmful" => "reports.reason.harmful",
        "copyright" => "reports.reason.copyright",
        _ => "reports.reason.other",
    })
}

/// The report as plain text lines, for both messages. `with_reporter`: the
/// reporter's name and address are in it.
fn summary(r: &Report, with_reporter: bool) -> String {
    let none = crate::t("reports.mail.none");
    let or_none = |s: &str| if s.trim().is_empty() { none.clone() } else { s.to_string() };
    let mut lines = vec![
        format!("{}: {}", crate::t("reports.mail.course"), or_none(&r.course_title)),
        format!("{}: {}", crate::t("reports.mail.reason"), reason_label(&r.reason)),
    ];
    if !r.location.is_empty() {
        lines.push(format!("{}: {}", crate::t("reports.mail.location"), r.location));
    }
    if !r.work.is_empty() {
        lines.push(format!("{}: {}", crate::t("reports.mail.work"), r.work));
    }
    if with_reporter {
        if !r.name.is_empty() {
            lines.push(format!("{}: {}", crate::t("reports.mail.name"), r.name));
        }
        lines.push(format!("{}: {}", crate::t("reports.mail.email"), r.email));
    }
    if r.is_copyright() && r.good_faith {
        lines.push(crate::t("reports.copyright.good_faith"));
    }
    lines.push(String::new());
    lines.push(r.details.clone());
    lines.join("\n")
}

/// The operator's copy: everything, and the link to the Superuser page.
pub fn operator_message(to: &str, r: &Report, link: &str) -> Result<Mail, String> {
    let app = crate::APP_NAME;
    let subject = crate::i18n::fill(&crate::t("reports.mail.operator_subject"), &[("app", &app), ("reason", &reason_label(&r.reason))]);
    let text = crate::i18n::fill(&crate::t("reports.mail.operator_text"), &[("app", &app), ("report", &summary(r, true)), ("link", &link)]);
    Mail::new(to, &subject, &text, PURPOSE)
}

/// The owner's copy of the notice (the Terms promise it on a copyright
/// notice): the reporter's name and address only on a copyright notice.
pub fn forward_message(to: &str, owner_id: i64, r: &Report) -> Result<Mail, String> {
    let app = crate::APP_NAME;
    let title = if r.course_title.is_empty() { crate::t("reports.mail.none") } else { r.course_title.clone() };
    let subject = crate::i18n::fill(&crate::t("reports.mail.owner_subject"), &[("app", &app), ("title", &title)]);
    let text = crate::i18n::fill(&crate::t("reports.mail.owner_text"), &[("app", &app), ("title", &title), ("report", &summary(r, r.is_copyright()))]);
    Ok(Mail::new(to, &subject, &text, PURPOSE_FORWARD)?.for_user(owner_id))
}

#[cfg(test)]
mod tests;
