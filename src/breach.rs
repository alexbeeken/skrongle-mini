//! Breach tools (#1068, hosted only): the mass actions of the incident
//! runbook (`ops/incident-response.md`) as Superuser buttons.
//!
//! - Sign out: every account, or the accounts a pasted list of addresses
//!   names. Their sessions, sign-ins waiting for a second-step code and
//!   unused password-reset links go, as `moderation::suspend` does for one
//!   account. The superuser's own session on this device stays.
//! - Notice: a plain-text e-mail (subject and body as typed) to every
//!   account with a confirmed address, or to the listed ones, through the
//!   mail outbox (`SKRONGLE_MAIL_PER_HOUR` applies). `breach_notices` keeps
//!   one row per account and notice text, in the same transaction as the
//!   mail, so pressing Send again with the same text mails nobody twice and
//!   carries on where the hourly ceiling stopped it.
//!
//! Both are written to the privacy log (`privacy_requests.rs`) with the
//! reason the superuser typed.

use rusqlite::{params, Connection};

/// The `mail_outbox` purpose of a breach notice.
pub const PURPOSE: &str = "breach-notice";
/// The longest subject and body kept (characters).
pub const MAX_SUBJECT: usize = 200;
pub const MAX_BODY: usize = 20_000;

/// Which accounts an action reaches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Who {
    Everyone,
    /// Addresses, lower case, each once.
    List(Vec<String>),
}

/// The addresses in a pasted list: separated by spaces, new lines, commas
/// or semicolons, lower-cased, each once, in the order given. Words with no
/// `@` are left out.
pub fn parse_list(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for w in text.split(|c: char| c.is_whitespace() || c == ',' || c == ';') {
        let w = w.trim().trim_matches(|c| c == '<' || c == '>').to_lowercase();
        if w.contains('@') && !out.contains(&w) {
            out.push(w);
        }
    }
    out
}

/// The accounts `who` names: id and address. With `confirmed`, only
/// accounts whose address is confirmed. A listed address no account has
/// is not in the result.
fn accounts(conn: &Connection, who: &Who, confirmed: bool) -> rusqlite::Result<Vec<(i64, String)>> {
    let filter = if confirmed { "COALESCE(email, '') <> '' AND email_verified_at IS NOT NULL" } else { "1" };
    let mut stmt = conn.prepare(&format!("SELECT id, COALESCE(email, '') FROM users WHERE {filter} ORDER BY id"))?;
    let all = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(match who {
        Who::Everyone => all,
        Who::List(list) => all.into_iter().filter(|(_, e)| list.contains(&e.to_lowercase())).collect(),
    })
}

/// How many accounts there are, and how many have a confirmed address:
/// what the card's dialogs say Everyone means.
pub fn counts(conn: &Connection) -> rusqlite::Result<(i64, i64)> {
    conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(COALESCE(email, '') <> '' AND email_verified_at IS NOT NULL), 0) FROM users",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
}

/// What [`sign_out`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignedOut {
    /// Accounts reached.
    pub accounts: usize,
    /// Sessions ended.
    pub sessions: usize,
}

/// Sign `who` out: their sessions (except the one whose token is `keep`,
/// the superuser's own), sign-ins waiting for a second-step code, and
/// unused password-reset links.
pub fn sign_out(conn: &Connection, who: &Who, keep: &str) -> rusqlite::Result<SignedOut> {
    let keep = crate::auth::hash_token(keep);
    let tx = conn.unchecked_transaction()?;
    let out = match who {
        Who::Everyone => {
            let accounts = tx.query_row("SELECT COUNT(*) FROM users", [], |r| r.get::<_, i64>(0))? as usize;
            let sessions = tx.execute("DELETE FROM sessions WHERE session_token <> ?1", [&keep])?;
            tx.execute("DELETE FROM two_factor_challenges", [])?;
            tx.execute("DELETE FROM password_resets WHERE used_at IS NULL", [])?;
            SignedOut { accounts, sessions }
        }
        Who::List(_) => {
            let ids = accounts(&tx, who, false)?;
            let mut sessions = 0;
            for (id, _) in &ids {
                sessions += tx.execute("DELETE FROM sessions WHERE user_id = ?1 AND session_token <> ?2", params![id, keep])?;
                tx.execute("DELETE FROM two_factor_challenges WHERE user_id = ?1", [id])?;
                tx.execute("DELETE FROM password_resets WHERE user_id = ?1 AND used_at IS NULL", [id])?;
            }
            SignedOut { accounts: ids.len(), sessions }
        }
    };
    tx.commit()?;
    Ok(out)
}

/// What [`send_notice`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Sent {
    /// Mails queued now.
    pub queued: usize,
    /// Accounts that had this notice already.
    pub already: usize,
    /// Accounts left over: the hourly ceiling is full for every hour a mail
    /// may wait for. Pressing Send again later carries on.
    pub left: usize,
}

/// What identifies one notice: the hash of its subject and body.
fn notice_key(subject: &str, body: &str) -> String {
    crate::auth::hash_token(&format!("{subject}\n{body}"))
}

/// Queue the notice for every account `who` names that has a confirmed
/// address and has not had this text yet, recording each in
/// `breach_notices` in the same transaction.
pub fn send_notice(conn: &Connection, who: &Who, subject: &str, body: &str, now: i64, ceiling: Option<crate::mail::Ceiling>) -> rusqlite::Result<Sent> {
    let key = notice_key(subject, body);
    let tx = conn.unchecked_transaction()?;
    let mut sent = Sent::default();
    let targets = accounts(&tx, who, true)?;
    let mut stopped = false;
    for (uid, email) in targets {
        let had: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM breach_notices WHERE user_id = ?1 AND notice = ?2)", params![uid, key], |r| r.get(0))?;
        if had {
            sent.already += 1;
            continue;
        }
        if stopped {
            sent.left += 1;
            continue;
        }
        match crate::mail::Mail::new(&email, subject, body, PURPOSE) {
            Ok(mail) => match crate::mail::enqueue(&tx, &mail.for_user(uid), now, ceiling)? {
                Ok(_) => sent.queued += 1,
                Err(crate::mail::Capped::Hourly) => {
                    stopped = true;
                    sent.left += 1;
                    continue;
                }
                Err(crate::mail::Capped::Address) => {}
            },
            // An address the mailer refuses: recorded, so it is not tried again.
            Err(e) => tracing::warn!("breach notice: no e-mail for user {uid}: {e}"),
        }
        tx.execute("INSERT OR IGNORE INTO breach_notices (user_id, notice, mailed_at) VALUES (?1, ?2, ?3)", params![uid, key, now])?;
    }
    tx.commit()?;
    Ok(sent)
}

#[cfg(test)]
mod tests;
