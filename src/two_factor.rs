//! Two-factor sign-in (#292): after the password a second step asks for a
//! code from an authenticator app (RFC 6238, `crate::totp`; `users.two_factor`
//! is `'totp'` when on and `''` when off; anything else reads as off), enrolled by showing its `otpauth://` link and secret and
//! confirmed with a first code ([`totp_start`], [`totp_confirm`]). One app
//! per account.
//!
//! A sign-in that needs a code leaves a challenge (`two_factor_challenges`)
//! named by a random token in the browser's `twofa` cookie, stored only as
//! its SHA-256; the session is made only once the code is right. A
//! challenge lives [`CODE_TTL_SECS`] and allows [`MAX_ATTEMPTS`] tries. An
//! authenticator code is accepted
//! one step either side of now and never twice (`totp_keys.last_step`). The
//! TOTP secret must be readable to work codes out, so it is sealed with the
//! same vault as the AI API keys (`crate::secrets::seal_totp`, ChaCha20-
//! Poly1305, bound to its user): a copy of the database without the master
//! secret cannot produce codes.
//!
//! Turning the second step on or off needs
//! proof that it is the account's owner, not just a session ([`needs_proof`]):
//! the password, or, for an account without one, a code from the app
//! ([`check_confirm_code`]).

use rusqlite::{params, Connection, OptionalExtension};

use crate::secrets::{self, Vault};
use crate::totp;

/// How long a sign-in waits for its code.
pub const CODE_TTL_SECS: i64 = 10 * 60;
/// Wrong codes allowed per sign-in.
pub const MAX_ATTEMPTS: i64 = 5;
/// The sign-in challenge's cookie.
pub const COOKIE: &str = "twofa";
/// What `users.two_factor` holds while the second step is on; off is `''`.
const ON: &str = "totp";

/// What the profile shows about an account's second step.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    /// The second sign-in step is on (`users.two_factor = 'totp'`).
    pub enabled: bool,
    pub email: String,
    /// An authenticator app is set up (confirmed), since this time.
    pub totp_since: Option<i64>,
    /// An enrolment under way (its key: [`pending_key`]).
    pub totp_pending: bool,
}

pub fn settings(conn: &Connection, user_id: i64) -> rusqlite::Result<Settings> {
    let mut s = conn.query_row(
        "SELECT two_factor, email FROM users WHERE id = ?1",
        [user_id],
        |r| {
            Ok(Settings {
                enabled: r.get::<_, String>(0)? == ON,
                email: r.get(1)?,
                ..Settings::default()
            })
        },
    )?;
    let key: Option<Option<i64>> = conn.query_row("SELECT confirmed_at FROM totp_keys WHERE user_id = ?1", [user_id], |r| r.get(0)).optional()?;
    match key {
        Some(Some(at)) => s.totp_since = Some(at),
        Some(None) => s.totp_pending = true,
        None => {}
    }
    Ok(s)
}

/// Does turning the second step on or off need the password (or a code)?
/// Every change does (#374): turning it on as well, or a session holder
/// could put their own app in front of the owner's next sign-in. Choosing
/// the state it is already in does not.
pub fn needs_proof(current: bool, to: bool) -> bool {
    current != to
}

/// Turn the second step on (the app must be set up: [`totp_confirm`] does
/// both when it enrols one) or off, which removes the app's key too (a
/// secret no longer used is not kept).
pub fn set_enabled(conn: &Connection, user_id: i64, on: bool) -> rusqlite::Result<()> {
    if on {
        conn.execute("UPDATE users SET two_factor = ?2 WHERE id = ?1 AND EXISTS (SELECT 1 FROM totp_keys WHERE user_id = ?1 AND confirmed_at IS NOT NULL)", params![user_id, ON])?;
        Ok(())
    } else {
        totp_remove(conn, user_id)
    }
}

// ---------- codes ----------

/// What typing a code did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Checked {
    Ok,
    /// Wrong; this many tries are left.
    Wrong(i64),
    /// No code waiting: never sent, expired, used up or replaced.
    Gone,
}

// ---------- the authenticator app ----------

/// Start enrolling an authenticator app: a new secret (replacing an
/// enrolment under way), labelled with the app's name and the account's
/// address. `Err` while one is already set up: only one at a time.
pub fn totp_start(conn: &Connection, vault: &Vault, user_id: i64, now: i64) -> rusqlite::Result<Result<totp::Key, String>> {
    let s = settings(conn, user_id)?;
    if s.totp_since.is_some() {
        return Ok(Err(crate::t("auth.two_factor.app_already")));
    }
    let secret = totp::new_secret();
    conn.execute(
        "INSERT INTO totp_keys (user_id, secret, created_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(user_id) DO UPDATE SET secret = excluded.secret, created_at = excluded.created_at, confirmed_at = NULL, last_step = NULL",
        params![user_id, secrets::seal_totp(vault, user_id, &secret), now],
    )?;
    Ok(Ok(key_for(&s.email, user_id, secret)))
}

/// The key as the app is handed it: issuer the app's name, account the address.
pub fn key_for(email: &str, user_id: i64, secret: Vec<u8>) -> totp::Key {
    let account = if email.is_empty() { format!("account {user_id}") } else { email.to_string() };
    totp::Key::new(secret, crate::APP_NAME, &account)
}

/// The stored key (sealed, see the module doc), whether it is confirmed, and
/// the last step accepted. `None` with no key, or one that no longer opens.
fn stored_key(conn: &Connection, vault: &Vault, user_id: i64) -> rusqlite::Result<Option<(totp::Key, bool, Option<u64>)>> {
    let row: Option<(Vec<u8>, Option<i64>, Option<i64>, String)> = conn
        .query_row(
            "SELECT k.secret, k.confirmed_at, k.last_step, u.email FROM totp_keys k JOIN users u ON u.id = k.user_id WHERE k.user_id = ?1",
            [user_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    Ok(row.and_then(|(blob, confirmed, last, email)| Some((key_for(&email, user_id, secrets::open_totp(vault, user_id, &blob)?), confirmed.is_some(), last.map(|l| l as u64)))))
}

/// The enrolment under way, as the app is handed it (the setup dialog).
pub fn pending_key(conn: &Connection, vault: &Vault, user_id: i64) -> rusqlite::Result<Option<totp::Key>> {
    Ok(stored_key(conn, vault, user_id)?.and_then(|(key, confirmed, _)| (!confirmed).then_some(key)))
}

/// Check an authenticator code against the confirmed app, spending its
/// step; `false` when wrong, already used, or there is no app.
fn spend_totp_code(conn: &Connection, vault: &Vault, user_id: i64, code: &str, now: i64) -> rusqlite::Result<bool> {
    let Some((key, true, last)) = stored_key(conn, vault, user_id)? else { return Ok(false) };
    let Some(step) = key.verify(code, now.max(0) as u64, last) else { return Ok(false) };
    conn.execute("UPDATE totp_keys SET last_step = ?2 WHERE user_id = ?1", params![user_id, step as i64])?;
    Ok(true)
}

/// Finish enrolling: the app's first code turns it on as the second step. `false`: wrong code, or nothing to confirm.
pub fn totp_confirm(conn: &Connection, vault: &Vault, user_id: i64, code: &str, now: i64) -> rusqlite::Result<bool> {
    let Some((key, false, _)) = stored_key(conn, vault, user_id)? else { return Ok(false) };
    let Some(step) = key.verify(code, now.max(0) as u64, None) else { return Ok(false) };
    conn.execute("UPDATE totp_keys SET confirmed_at = ?2, last_step = ?3 WHERE user_id = ?1", params![user_id, now, step as i64])?;
    conn.execute("UPDATE users SET two_factor = ?2 WHERE id = ?1", params![user_id, ON])?;
    Ok(true)
}

/// Remove the authenticator app (or an enrolment under way). While it was
/// the second step, two-factor sign-in is off afterwards.
pub fn totp_remove(conn: &Connection, user_id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM totp_keys WHERE user_id = ?1", [user_id])?;
    conn.execute("UPDATE users SET two_factor = '' WHERE id = ?1 AND two_factor = ?2", params![user_id, ON])?;
    Ok(())
}

// ---------- signing in ----------

/// A sign-in waiting for its code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Challenge {
    /// For the `twofa` cookie; stored only as its SHA-256.
    pub token: String,
}

/// After the password: `None` when the account has no second step (sign
/// in straight away), else a challenge, replacing any older one of the
/// account's.
pub fn start_login(conn: &Connection, user_id: i64, now: i64) -> rusqlite::Result<Option<Challenge>> {
    let s = settings(conn, user_id)?;
    if !s.enabled {
        return Ok(None);
    }
    conn.execute("DELETE FROM two_factor_challenges WHERE user_id = ?1", [user_id])?;
    let token = crate::auth::generate_token();
    conn.execute(
        "INSERT INTO two_factor_challenges (user_id, token_hash, created_at, expires_at) VALUES (?1, ?2, ?3, ?4)",
        params![user_id, crate::auth::hash_token(&token), now, now + CODE_TTL_SECS],
    )?;
    Ok(Some(Challenge { token }))
}

/// A challenge still waiting, for its page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    pub id: i64,
    pub user_id: i64,
    pub email: String,
    pub attempts: i64,
}

pub fn pending(conn: &Connection, token: &str, now: i64) -> rusqlite::Result<Option<Pending>> {
    if token.len() != 64 {
        return Ok(None);
    }
    conn.query_row(
        "SELECT c.id, c.user_id, u.email, c.attempts
         FROM two_factor_challenges c JOIN users u ON u.id = c.user_id
         WHERE c.token_hash = ?1 AND c.expires_at > ?2 AND c.attempts < ?3",
        params![crate::auth::hash_token(token), now, MAX_ATTEMPTS],
        |r| Ok(Pending { id: r.get(0)?, user_id: r.get(1)?, email: r.get(2)?, attempts: r.get(3)? }),
    )
    .optional()
}

/// Check the code typed for a sign-in: `(Checked::Ok, user id)` signs in
/// (the challenge is used up). An authenticator code is checked against
/// the app's key now, so a key removed meanwhile fails.
pub fn check_login(conn: &Connection, vault: &Vault, token: &str, code: &str, now: i64) -> rusqlite::Result<(Checked, Option<i64>)> {
    let Some(p) = pending(conn, token, now)? else { return Ok((Checked::Gone, None)) };
    if spend_totp_code(conn, vault, p.user_id, code, now)? {
        conn.execute("DELETE FROM two_factor_challenges WHERE id = ?1", [p.id])?;
        return Ok((Checked::Ok, Some(p.user_id)));
    }
    conn.execute("UPDATE two_factor_challenges SET attempts = attempts + 1 WHERE id = ?1", [p.id])?;
    let left = MAX_ATTEMPTS - p.attempts - 1;
    if left <= 0 {
        conn.execute("DELETE FROM two_factor_challenges WHERE id = ?1", [p.id])?;
        return Ok((Checked::Gone, Some(p.user_id)));
    }
    Ok((Checked::Wrong(left), Some(p.user_id)))
}

// ---------- proving it is the owner, without a password ----------

/// Check a code from the authenticator app, each
/// step once, like signing in (`Wrong(_)` counts nothing here, the route's
/// rate limit does).
pub fn check_confirm_code(conn: &Connection, vault: &Vault, user_id: i64, code: &str, now: i64) -> rusqlite::Result<Checked> {
    if !settings(conn, user_id)?.enabled {
        return Ok(Checked::Gone);
    }
    Ok(if spend_totp_code(conn, vault, user_id, code, now)? { Checked::Ok } else { Checked::Wrong(MAX_ATTEMPTS) })
}

/// Give up a sign-in waiting for its code ("Cancel").
pub fn cancel(conn: &Connection, token: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM two_factor_challenges WHERE token_hash = ?1", [crate::auth::hash_token(token)])?;
    Ok(())
}

/// Authenticator enrolments abandoned for a day. Only unconfirmed keys are
/// in the partial index `idx_totp_keys_pending` (db.rs v58), so the hourly
/// purge does not read every account's key (#613).
const PURGE_ENROLMENTS: &str = "DELETE FROM totp_keys WHERE confirmed_at IS NULL AND created_at < ?1";

/// Hourly housekeeping: expired challenges, and enrolments abandoned for a
/// day.
pub fn purge(conn: &Connection, now: i64) -> rusqlite::Result<usize> {
    let a = conn.execute("DELETE FROM two_factor_challenges WHERE expires_at < ?1", [now])?;
    let c = conn.execute(PURGE_ENROLMENTS, [now - 86_400])?;
    Ok(a + c)
}

#[cfg(test)]
mod tests;
