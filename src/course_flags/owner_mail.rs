//! The e-mail a superuser sends a course's owner to say why the course was
//! flagged or taken down (#1035). Sent by hand only, from the course's
//! dialog in the course table: [`draft`] fills the form in the owner's
//! language, the superuser edits it and presses Send. Nothing here sends on
//! its own. For a flagged course the draft only says that it was flagged
//! and links to the site, where the owner sees what to fix: never the
//! words, the quotes or the AI's report, and never who reported the course.
//! Hosted only: it needs e-mail, which Skrongle mini doesn't have.

use rusqlite::{params, Connection, OptionalExtension};

use super::{get, Flag, Source, State};

/// The outbox's name for it.
pub const PURPOSE: &str = "course-flag-owner";

/// The longest subject and message kept, in characters.
pub const SUBJECT_MAX: usize = 200;
pub const TEXT_MAX: usize = 5000;

/// Why the course doesn't open: what the e-mail explains.
#[derive(Debug, Clone, PartialEq)]
pub enum Why {
    /// The term check flagged it.
    Terms,
    /// The AI check flagged it.
    Ai,
    /// A superuser took it down (#967).
    Down,
}

impl Why {
    /// Taken down wins (nothing opens it either way); then a flag that
    /// blocks. `None` for a course that opens: there is nothing to explain.
    pub fn of(taken_down: bool, flag: Option<&Flag>) -> Option<Why> {
        if taken_down {
            return Some(Why::Down);
        }
        let f = flag.filter(|f| f.state == State::Flagged)?;
        Some(match f.source {
            Source::Terms => Why::Terms,
            Source::Ai => Why::Ai,
        })
    }

    /// The kind of e-mail, for the log line.
    pub fn kind(&self) -> &'static str {
        match self {
            Why::Terms => "flagged by the term list",
            Why::Ai => "flagged by the AI check",
            Why::Down => "taken down",
        }
    }
}

/// The subject and text of the e-mail about the course `title`, in the
/// current language (wrap it in `i18n::in_lang` for the owner's). `site`
/// is where links in e-mail point (`SKRONGLE_PUBLIC_URL`); a flagged
/// course's e-mail links to its course list there.
pub fn draft(title: &str, why: &Why, site: &str) -> (String, String) {
    let app = crate::APP_NAME;
    let fill = crate::i18n::fill;
    let (reason, fix) = match why {
        Why::Terms | Why::Ai => (crate::t("superuser.courses.mail.why_flagged"), fill(&crate::t("superuser.courses.mail.fix_flagged"), &[("link", &format!("{site}/courses"))])),
        Why::Down => (crate::t("superuser.courses.mail.why_down"), crate::t("superuser.courses.mail.fix_down")),
    };
    let subject = fill(&crate::t("superuser.courses.mail.subject"), &[("app", &app), ("title", &title)]);
    let contact = crate::terms::legal_email();
    let text = fill(&crate::t("superuser.courses.mail.text"), &[("app", &app), ("title", &title), ("why", &reason), ("fix", &fix), ("contact", &contact)]);
    (subject, text)
}

/// The owner of a made course and why it doesn't open, for sending.
pub struct Owner {
    pub id: i64,
    /// '' for an account with no address.
    pub email: String,
    pub why: Option<Why>,
}

/// The course's owner and why it doesn't open; `None` for no such course.
pub fn owner(conn: &Connection, course_id: &str) -> rusqlite::Result<Option<Owner>> {
    let found = conn
        .query_row(
            "SELECT u.id, COALESCE(u.email, ''), c.taken_down_at IS NOT NULL FROM courses c JOIN users u ON u.id = c.owner_id WHERE c.id = ?1",
            [course_id],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, bool>(2)?)),
        )
        .optional()?;
    let Some((id, email, down)) = found else { return Ok(None) };
    let flag = get(conn, course_id)?;
    Ok(Some(Owner { id, email, why: Why::of(down, flag.as_ref()) }))
}

/// Remember that the owner was e-mailed at `now`.
pub fn mark_emailed(conn: &Connection, course_id: &str, now: i64) -> rusqlite::Result<bool> {
    Ok(conn.execute("UPDATE courses SET owner_emailed_at = ?2 WHERE id = ?1", params![course_id, now])? > 0)
}

/// A subject as sent: one line, at most [`SUBJECT_MAX`] characters.
pub fn clean_subject(s: &str) -> String {
    s.split(['\r', '\n']).map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ").chars().take(SUBJECT_MAX).collect()
}

/// A message as sent: trimmed, line ends as `\n`, at most [`TEXT_MAX`] characters.
pub fn clean_text(s: &str) -> String {
    s.replace("\r\n", "\n").trim().chars().take(TEXT_MAX).collect()
}
