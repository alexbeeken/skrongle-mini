//! Deleting an account when the learner asks, after a grace period (#970).
//!
//! - *Delete my account* on the profile does not delete at once: it sets
//!   `users.delete_at` to [`GRACE_DAYS`] from now, ends
//!   every session and pending second step of the account, and, where the
//!   server sends mail, mails the learner the date.
//! - Until then the account can be signed in to as usual (password, second
//!   step), but the `CurrentUser` extractor sends every page to
//!   `/account/deletion` (`routes/account_deletion.rs`), whose one action is
//!   *Cancel deletion*; the other is signing out. Its courses are not shared
//!   out meanwhile (`share.rs`).
//! - The hourly job [`crate::jobs::HOUSEKEEPING`] deletes every account whose
//!   time has come, with the same full deletion as before
//!   (`privacy::delete_account`, plus what the server holds in memory and on
//!   disk). An owner's delete (#967) stays immediate.

use rusqlite::{params, Connection};

use crate::db::Db;
use crate::AppState;

/// Days between asking and the deletion.
pub const GRACE_DAYS: i64 = 14;

/// The outbox purpose of the mail that gives the date.
pub const PURPOSE: &str = "account-deletion";

/// Accounts deleted per run of the hourly job; the rest wait an hour.
const BATCH: i64 = 50;

/// When an account asked to be deleted at `now` is deleted: [`GRACE_DAYS`] later.
pub fn due_date(now: i64) -> i64 {
    now + GRACE_DAYS * 86_400
}

/// Schedule the deletion: set `delete_at`, end every session and pending
/// second step, and with `mail` queue the mail with the date, in one
/// transaction. The date, or `None` when there is no such account.
pub fn schedule(conn: &mut Connection, user_id: i64, now: i64, mail: bool) -> rusqlite::Result<Option<i64>> {
    let tx = conn.transaction()?;
    let at = due_date(now);
    if tx.execute("UPDATE users SET delete_at = ?2 WHERE id = ?1", params![user_id, at])? == 0 {
        return Ok(None);
    }
    tx.execute("DELETE FROM sessions WHERE user_id = ?1", [user_id])?;
    tx.execute("DELETE FROM two_factor_challenges WHERE user_id = ?1", [user_id])?;
    if mail {
        let email: String = tx.query_row("SELECT COALESCE(email, '') FROM users WHERE id = ?1", [user_id], |r| r.get(0))?;
        match message(&email, at, user_id) {
            // No hourly ceiling: the owner's proof bounds these.
            Ok(m) => {
                if let Err(capped) = crate::mail::enqueue(&tx, &m, now, None)? {
                    tracing::warn!("user {user_id}: deletion mail not queued ({capped:?})");
                }
            }
            Err(e) => tracing::warn!("user {user_id}: no deletion mail ({e})"),
        }
    }
    tx.commit()?;
    Ok(Some(at))
}

/// Cancel a scheduled deletion. `false` when none was scheduled.
pub fn cancel(conn: &Connection, user_id: i64) -> rusqlite::Result<bool> {
    Ok(conn.execute("UPDATE users SET delete_at = NULL WHERE id = ?1 AND delete_at IS NOT NULL", [user_id])? > 0)
}

/// The accounts due for deletion at `now`, the longest overdue first.
pub fn due(conn: &Connection, now: i64) -> rusqlite::Result<Vec<i64>> {
    let mut stmt = conn.prepare_cached("SELECT id FROM users WHERE delete_at IS NOT NULL AND delete_at <= ?1 ORDER BY delete_at LIMIT ?2")?;
    let ids = stmt.query_map(params![now, BATCH], |r| r.get(0))?.collect();
    ids
}

/// The mail that says when the account goes, and how to keep it.
pub fn message(email: &str, at: i64, user_id: i64) -> Result<crate::mail::Mail, String> {
    let app = crate::APP_NAME;
    let date = crate::profile::long_date(at);
    let subject = crate::t2("mail.account_deletion.subject", "app", app, "date", &date);
    let text = crate::t2("mail.account_deletion.text", "app", app, "date", &date);
    Ok(crate::mail::Mail::new(email, &subject, &text, PURPOSE)?.for_user(user_id))
}

/// Forget what lives in memory for an account about to be deleted, so
/// nothing writes for it afterwards.
pub fn forget_in_memory(state: &AppState, uid: i64) {
    state.jobs.drop_user(uid);
    state.tutors.drop_user(uid);
    state.games.drop_user(uid);
}

/// After `privacy::delete_account`: drop its courses from the cache.
pub fn forget_deleted(state: &AppState, deleted: &crate::privacy::Deleted) {
    for c in &deleted.courses {
        state.courses.invalidate(c);
    }
}

/// The hourly job: delete every account whose time has come.
pub fn sweep(state: &AppState, db: &Db) -> Result<String, String> {
    let now = crate::db::now();
    let ids = db.with(|c| due(c, now)).map_err(|e| e.to_string())?;
    let mut n = 0;
    for uid in ids {
        forget_in_memory(state, uid);
        // Checked again next to the delete: a sign-in may have cancelled it since.
        let deleted = db
            .with(|c| {
                let still: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM users WHERE id = ?1 AND delete_at IS NOT NULL AND delete_at <= ?2)", params![uid, now], |r| r.get(0))?;
                if still {
                    crate::privacy::delete_account(c, uid)
                } else {
                    Ok(None)
                }
            })
            .map_err(|e| e.to_string())?;
        if let Some(d) = deleted {
            forget_deleted(state, &d);
            tracing::info!("user {uid}: account deleted as they asked ({} courses)", d.courses.len());
            n += 1;
        }
    }
    Ok(if n == 0 { String::new() } else { format!("{n} account(s) deleted as their owners asked") })
}

#[cfg(test)]
mod tests;
