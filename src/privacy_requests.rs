//! Privacy requests that arrive by e-mail (#981, hosted only): what a
//! superuser does on `/superuser` for someone who can't or doesn't use
//! Profile → Your data, and the log of it.
//!
//! - Send an account its data: the profile's export (`exports::request`)
//!   for another account. Its link goes to that account's own address only,
//!   never to the address a request came from, and opens only signed in.
//! - Forget an address: for a person with no account whose address is in a
//!   course share invitation. The invitations and the mails to it go.
//! - Every one of these, and Delete account, asks for a reason; the log
//!   (table `privacy_actions`) keeps the action, the reason, the superuser
//!   and when. It names no learner: the reason should point at the private
//!   request log instead.

use rusqlite::{params, Connection};

/// The longest reason kept (characters); longer ones are cut.
pub const MAX_REASON: usize = 200;
/// Log rows shown on the page, newest first.
pub const LOG_ROWS: i64 = 50;

/// What a log row records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// An account's data export was started for it.
    Export,
    /// An address with no account was forgotten.
    Forget,
    /// Every account, or a list, was signed out (breach tools, #1068).
    SignOut,
    /// A breach notice was e-mailed (#1068).
    Notice,
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Action::Export => "export",
            Action::Forget => "forget",
            Action::SignOut => "sign-out",
            Action::Notice => "notice",
        }
    }
}

/// The reason as typed, trimmed and cut to [`MAX_REASON`]; `None` when empty.
pub fn reason(typed: &str) -> Option<String> {
    let r: String = typed.trim().chars().take(MAX_REASON).collect();
    let r = r.trim_end().to_string();
    (!r.is_empty()).then_some(r)
}

/// Add a row to the log.
pub fn record(conn: &Connection, superuser: i64, action: Action, reason: &str, now: i64) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO privacy_actions (superuser_id, action, reason, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![superuser, action.as_str(), reason, now],
    )?;
    Ok(())
}

/// One row of the log, for the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogRow {
    pub created_at: i64,
    /// The superuser's address, or '' once their account is gone.
    pub superuser: String,
    /// [`Action::as_str`].
    pub action: String,
    pub reason: String,
}

/// The newest [`LOG_ROWS`] rows.
pub fn recent(conn: &Connection) -> rusqlite::Result<Vec<LogRow>> {
    let mut stmt = conn.prepare(
        "SELECT p.created_at, COALESCE(u.email, ''), p.action, p.reason
         FROM privacy_actions p LEFT JOIN users u ON u.id = p.superuser_id
         ORDER BY p.id DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map([LOG_ROWS], |r| Ok(LogRow { created_at: r.get(0)?, superuser: r.get(1)?, action: r.get(2)?, reason: r.get(3)? }))?;
    rows.collect()
}

/// What [`forget`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Forgot {
    /// An account has the address: nothing was deleted (the account holder
    /// deletes their own account, or a superuser deletes it).
    HasAccount,
    /// The invitations (`shares`) and outbox mails (`mails`) deleted.
    Done { shares: usize, mails: usize },
}

/// Forget an address no account has: every course share invitation to it
/// (opened or not), and every mail to it still in the outbox. `email` is
/// compared lower-cased, as invitations are stored.
pub fn forget(conn: &mut Connection, email: &str) -> rusqlite::Result<Forgot> {
    let email = email.trim().to_lowercase();
    if crate::owner::find(conn, &email)?.is_some() {
        return Ok(Forgot::HasAccount);
    }
    let tx = conn.transaction()?;
    let shares = tx.execute("DELETE FROM course_shares WHERE email = ?1", [&email])?;
    let mails = tx.execute("DELETE FROM mail_outbox WHERE lower(to_addr) = ?1", [&email])?;
    tx.commit()?;
    Ok(Forgot::Done { shares, mails })
}

#[cfg(test)]
mod tests;
