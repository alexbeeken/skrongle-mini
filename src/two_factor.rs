//! Two-factor sign-in (#292): after the password (or Google) a second step
//! asks for a code, from one method at a time (`users.two_factor`):
//!
//! - `email`: a 6-digit code e-mailed to the account's address, which must
//!   be confirmed (`users.email_verified_at`) before the method is offered;
//! - `totp`: an authenticator app (RFC 6238, `crate::totp`), enrolled by
//!   showing its `otpauth://` link and secret and confirmed with a first
//!   code ([`totp_start`], [`totp_confirm`]). One app per account.
//!
//! A sign-in that needs a code leaves a challenge (`two_factor_challenges`)
//! named by a random token in the browser's `twofa` cookie; the session is
//! made only once the code is right. E-mailed codes live
//! [`CODE_TTL_SECS`] and allow [`MAX_ATTEMPTS`] tries; they are stored as
//! SHA-256 with a random salt and, for a sign-in, with the cookie's token
//! too, which is itself stored only as its SHA-256: a copy of the database
//! alone cannot be searched for a code. An authenticator code is accepted
//! one step either side of now and never twice (`totp_keys.last_step`). The
//! TOTP secret must be readable to work codes out, so it is sealed with the
//! same vault as the AI API keys (`crate::secrets::seal_totp`, ChaCha20-
//! Poly1305, bound to its user): a copy of the database without the master
//! secret cannot produce codes.
//!
//! Turning the second step off, or switching it to another method, needs
//! proof that it is the account's owner, not just a session ([`needs_proof`]):
//! the password, or, for a Google-only account (no password), a code from
//! the current second step ([`issue_confirm_code`], [`check_confirm_code`]).

use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};

use crate::secrets::{self, Vault};
use crate::totp;

/// How long an e-mailed code works.
pub const CODE_TTL_SECS: i64 = 10 * 60;
/// Wrong codes allowed per sign-in (however many codes it sends).
pub const MAX_ATTEMPTS: i64 = 5;
/// The sign-in challenge's cookie.
pub const COOKIE: &str = "twofa";
/// Outbox purposes.
pub const PURPOSE_LOGIN: &str = "login-code";
pub const PURPOSE_CONFIRM: &str = "two-factor-confirm";
/// The account is told when its second step changes (#374).
pub const PURPOSE_CHANGED: &str = "two-factor-changed";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Email,
    Totp,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Email => "email",
            Method::Totp => "totp",
        }
    }

    pub fn parse(s: &str) -> Option<Method> {
        match s {
            "email" => Some(Method::Email),
            "totp" => Some(Method::Totp),
            _ => None,
        }
    }
}

/// Can this server deliver `m`'s codes (#633)? The authenticator app
/// needs nothing; e-mail needs its provider.
pub fn channel_on(m: Method, caps: crate::Caps) -> bool {
    match m {
        Method::Email => caps.mail,
        Method::Totp => true,
    }
}

/// Why `m` cannot be used here: its channel is off, naming the setting
/// that turns it on.
pub fn channel_off_text(m: Method) -> String {
    match m {
        Method::Email => crate::t1("auth.two_factor.channel_off_email", "setting", "SKRONGLE_MAIL_PROVIDERS"),
        Method::Totp => String::new(),
    }
}

/// What the profile shows about an account's second step.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    pub method: Option<Method>,
    pub email: String,
    pub email_verified: bool,
    /// An authenticator app is set up (confirmed), since this time.
    pub totp_since: Option<i64>,
    /// An enrolment under way (its key: [`pending_key`]).
    pub totp_pending: bool,
}

pub fn settings(conn: &Connection, user_id: i64) -> rusqlite::Result<Settings> {
    let mut s = conn.query_row(
        "SELECT two_factor, email, email_verified_at IS NOT NULL FROM users WHERE id = ?1",
        [user_id],
        |r| {
            Ok(Settings {
                method: Method::parse(&r.get::<_, String>(0)?),
                email: r.get(1)?,
                email_verified: r.get(2)?,
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

/// Does going from the current method to `to` need the password (or a
/// code, or Google)? Every change does (#374): turning it on as well, or a
/// session holder could put their own app in front of the
/// owner's next sign-in. Only choosing the method already in use does not.
pub fn needs_proof(current: Option<Method>, to: Option<Method>) -> bool {
    current != to
}

impl Settings {
    /// Why `m` cannot be chosen now, if it cannot.
    pub fn refusal(&self, m: Method) -> Option<String> {
        match m {
            Method::Email if self.email.is_empty() || !self.email_verified => Some(crate::t("auth.two_factor.needs_email")),
            Method::Totp if self.totp_since.is_none() => Some(crate::t("auth.two_factor.needs_app")),
            _ => None,
        }
    }
}

/// Switch the second step to `method` (`None`: off). Only one is in use:
/// choosing another replaces it, and leaving the authenticator app removes
/// its key (a secret no longer used is not kept). `Err` says why the method
/// cannot be chosen.
pub fn set_method(conn: &Connection, user_id: i64, method: Option<Method>) -> rusqlite::Result<Result<(), String>> {
    let s = settings(conn, user_id)?;
    if let Some(why) = method.and_then(|m| s.refusal(m)) {
        return Ok(Err(why));
    }
    conn.execute("UPDATE users SET two_factor = ?2 WHERE id = ?1", params![user_id, method.map(Method::as_str).unwrap_or("")])?;
    if method != Some(Method::Totp) {
        conn.execute("DELETE FROM totp_keys WHERE user_id = ?1", [user_id])?;
    }
    Ok(Ok(()))
}

// ---------- codes ----------

/// Six random digits, uniformly.
pub fn new_code() -> String {
    loop {
        let n = crate::random::u32();
        // Reject the top sliver so every code is equally likely.
        if n < u32::MAX - (u32::MAX % 1_000_000) {
            return format!("{:06}", n % 1_000_000);
        }
    }
}

fn new_salt() -> String {
    let mut b = [0u8; 16];
    crate::random::fill(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn hash(parts: &[&str]) -> String {
    Sha256::digest(parts.join(":").as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// Only the digits of what was typed (people paste "123 456").
fn digits(code: &str) -> String {
    code.chars().filter(char::is_ascii_digit).collect()
}

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

/// Finish enrolling: the app's first code turns it on as the second step
/// (replacing another method). `false`: wrong code, or nothing to confirm.
pub fn totp_confirm(conn: &Connection, vault: &Vault, user_id: i64, code: &str, now: i64) -> rusqlite::Result<bool> {
    let Some((key, false, _)) = stored_key(conn, vault, user_id)? else { return Ok(false) };
    let Some(step) = key.verify(code, now.max(0) as u64, None) else { return Ok(false) };
    conn.execute("UPDATE totp_keys SET confirmed_at = ?2, last_step = ?3 WHERE user_id = ?1", params![user_id, now, step as i64])?;
    conn.execute("UPDATE users SET two_factor = 'totp' WHERE id = ?1", [user_id])?;
    Ok(true)
}

/// Remove the authenticator app (or an enrolment under way). While it was
/// the second step, two-factor sign-in is off afterwards.
pub fn totp_remove(conn: &Connection, user_id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM totp_keys WHERE user_id = ?1", [user_id])?;
    conn.execute("UPDATE users SET two_factor = '' WHERE id = ?1 AND two_factor = 'totp'", [user_id])?;
    Ok(())
}

// ---------- signing in ----------

/// A sign-in waiting for its code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Challenge {
    /// For the `twofa` cookie; stored only as its SHA-256.
    pub token: String,
    pub method: Method,
    /// E-mailed codes: the code to send and where to.
    pub send: Option<(String, String)>,
}

/// After the password: `None` when the account has no second step (sign
/// in straight away), else a challenge, replacing any older one of the
/// account's.
pub fn start_login(conn: &Connection, user_id: i64, now: i64) -> rusqlite::Result<Option<Challenge>> {
    let s = settings(conn, user_id)?;
    let Some(method) = s.method else { return Ok(None) };
    conn.execute("DELETE FROM two_factor_challenges WHERE user_id = ?1", [user_id])?;
    let token = crate::auth::generate_token();
    conn.execute(
        "INSERT INTO two_factor_challenges (user_id, token_hash, method, created_at, expires_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![user_id, crate::auth::hash_token(&token), method.as_str(), now, now + CODE_TTL_SECS],
    )?;
    let send = new_login_code(conn, &token, now)?;
    Ok(Some(Challenge { token, method, send }))
}

/// A new e-mailed code for the challenge (the old one stops
/// working): `(to, code)`. `None` for an authenticator, or no such
/// challenge. The tries are per sign-in, not per code: a resend keeps the
/// wrong codes already typed, so sending again buys no more guesses (#359).
pub fn new_login_code(conn: &Connection, token: &str, now: i64) -> rusqlite::Result<Option<(String, String)>> {
    let Some(p) = pending(conn, token, now)? else { return Ok(None) };
    let to = match p.method {
        Method::Totp => return Ok(None),
        Method::Email => p.email,
    };
    let (code, salt) = (new_code(), new_salt());
    conn.execute(
        "UPDATE two_factor_challenges SET code_hash = ?2, salt = ?3, expires_at = ?4 WHERE id = ?1",
        params![p.id, hash(&[&salt, token, &code]), salt, now + CODE_TTL_SECS],
    )?;
    Ok(Some((to, code)))
}

/// A challenge still waiting, for its page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    pub id: i64,
    pub user_id: i64,
    pub method: Method,
    pub email: String,
    pub attempts: i64,
}

impl Pending {
    /// Where the newest code went, partly hidden.
    pub fn sent_to(&self) -> String {
        masked(self.method, &self.email)
    }
}

pub fn pending(conn: &Connection, token: &str, now: i64) -> rusqlite::Result<Option<Pending>> {
    if token.len() != 64 {
        return Ok(None);
    }
    conn.query_row(
        "SELECT c.id, c.user_id, c.method, u.email, c.attempts
         FROM two_factor_challenges c JOIN users u ON u.id = c.user_id
         WHERE c.token_hash = ?1 AND c.expires_at > ?2 AND c.attempts < ?3",
        params![crate::auth::hash_token(token), now, MAX_ATTEMPTS],
        |r| {
            Ok((
                (r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?),
                (r.get::<_, String>(3)?, r.get::<_, i64>(4)?),
            ))
        },
    )
    .optional()
    .map(|row| {
        row.and_then(|((id, user_id, method), (email, attempts))| {
            Some(Pending { id, user_id, method: Method::parse(&method)?, email, attempts })
        })
    })
}

/// Check the code typed for a sign-in: `(Checked::Ok, user id)` signs in
/// (the challenge is used up). An authenticator code is checked against
/// the app's key now, so a key removed meanwhile fails.
pub fn check_login(conn: &Connection, vault: &Vault, token: &str, code: &str, now: i64) -> rusqlite::Result<(Checked, Option<i64>)> {
    let Some(p) = pending(conn, token, now)? else { return Ok((Checked::Gone, None)) };
    let ok = match p.method {
        Method::Totp => spend_totp_code(conn, vault, p.user_id, code, now)?,
        Method::Email => {
            let (want, salt): (String, String) = conn.query_row("SELECT code_hash, salt FROM two_factor_challenges WHERE id = ?1", [p.id], |r| Ok((r.get(0)?, r.get(1)?)))?;
            !want.is_empty() && totp::constant_time_eq(hash(&[&salt, token, &digits(code)]).as_bytes(), want.as_bytes())
        }
    };
    if ok {
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

/// A Google-only account turning its e-mailed second step off (or
/// switching it): a new code to the current method's address
/// (older ones stop working), as `(method, to, code)`. `None` with no such
/// method in use; an authenticator app needs no code sent.
pub fn issue_confirm_code(conn: &Connection, user_id: i64, now: i64) -> rusqlite::Result<Option<(Method, String, String)>> {
    let s = settings(conn, user_id)?;
    let to = match s.method {
        Some(Method::Email) => s.email,
        Some(Method::Totp) | None => return Ok(None),
    };
    let (code, salt) = (new_code(), new_salt());
    conn.execute(
        "INSERT INTO two_factor_confirmations (user_id, method, code_hash, salt, created_at, expires_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(user_id) DO UPDATE SET method = excluded.method, code_hash = excluded.code_hash, salt = excluded.salt,
           attempts = 0, created_at = excluded.created_at, expires_at = excluded.expires_at",
        params![user_id, s.method.map(Method::as_str).unwrap_or_default(), hash(&[&salt, &code]), salt, now, now + CODE_TTL_SECS],
    )?;
    Ok(s.method.map(|m| (m, to, code)))
}

/// Check a code from the current second step: the authenticator's (each
/// step once, like signing in; `Wrong(_)` counts nothing here, the route's
/// rate limit does) or the one [`issue_confirm_code`] sent (10 minutes, 5
/// tries, and only for the method it was sent for).
pub fn check_confirm_code(conn: &Connection, vault: &Vault, user_id: i64, code: &str, now: i64) -> rusqlite::Result<Checked> {
    let s = settings(conn, user_id)?;
    let Some(method) = s.method else { return Ok(Checked::Gone) };
    if method == Method::Totp {
        return Ok(if spend_totp_code(conn, vault, user_id, code, now)? { Checked::Ok } else { Checked::Wrong(MAX_ATTEMPTS) });
    }
    let row: Option<(String, String, i64)> = conn
        .query_row(
            "SELECT code_hash, salt, attempts FROM two_factor_confirmations WHERE user_id = ?1 AND method = ?2 AND expires_at > ?3 AND attempts < ?4",
            params![user_id, method.as_str(), now, MAX_ATTEMPTS],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let Some((want, salt, attempts)) = row else { return Ok(Checked::Gone) };
    if totp::constant_time_eq(hash(&[&salt, &digits(code)]).as_bytes(), want.as_bytes()) {
        conn.execute("DELETE FROM two_factor_confirmations WHERE user_id = ?1", [user_id])?;
        return Ok(Checked::Ok);
    }
    conn.execute("UPDATE two_factor_confirmations SET attempts = attempts + 1 WHERE user_id = ?1", [user_id])?;
    let left = MAX_ATTEMPTS - attempts - 1;
    Ok(if left > 0 { Checked::Wrong(left) } else { Checked::Gone })
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

/// Hourly housekeeping: expired codes and challenges, and enrolments
/// abandoned for a day.
pub fn purge(conn: &Connection, now: i64) -> rusqlite::Result<usize> {
    let a = conn.execute("DELETE FROM two_factor_challenges WHERE expires_at < ?1", [now])?;
    let c = conn.execute(PURGE_ENROLMENTS, [now - 86_400])?;
    let d = conn.execute("DELETE FROM two_factor_confirmations WHERE expires_at < ?1", [now])?;
    Ok(a + c + d)
}

// ---------- the messages ----------

/// Where a code went, partly hidden: `a•••@example.org`.
pub fn masked(method: Method, email: &str) -> String {
    match method {
        Method::Email => match email.split_once('@') {
            Some((name, domain)) => format!("{}•••@{domain}", name.chars().next().unwrap_or('•')),
            None => crate::t("auth.code.your_email"),
        },
        Method::Totp => crate::t("auth.code.your_authenticator"),
    }
}

pub fn confirm_mail(email: &str, code: &str, user_id: i64) -> Result<crate::mail::Mail, String> {
    let app = crate::APP_NAME;
    // Never the code in the subject: subjects show in notifications and lists (#361).
    let subject = crate::t1("mail.two_factor_confirm.subject", "app", app);
    let text = crate::t2("mail.two_factor_confirm.text", "app", app, "code", code);
    let html = crate::t2("mail.two_factor_confirm.html", "app", app, "code", code);
    Ok(crate::mail::Mail::new(email, &subject, &text, PURPOSE_CONFIRM)?.html(&html).for_user(user_id))
}

/// Tells the account that its second sign-in step changed (#374), so an
/// owner whose session someone else used hears of it.
pub fn changed_mail(email: &str, method: Option<Method>, user_id: i64) -> Result<crate::mail::Mail, String> {
    let app = crate::APP_NAME;
    let now = crate::t(match method {
        None => "mail.two_factor_changed.now_off",
        Some(Method::Email) => "mail.two_factor_changed.now_email",
        Some(Method::Totp) => "mail.two_factor_changed.now_totp",
    });
    let subject = crate::t1("mail.two_factor_changed.subject", "app", app);
    let text = crate::t2("mail.two_factor_changed.text", "app", app, "now", &now);
    let html = crate::t2("mail.two_factor_changed.html", "app", app, "now", &now);
    Ok(crate::mail::Mail::new(email, &subject, &text, PURPOSE_CHANGED)?.html(&html).for_user(user_id))
}

pub fn login_mail(email: &str, code: &str, user_id: i64) -> Result<crate::mail::Mail, String> {
    let app = crate::APP_NAME;
    let subject = crate::t1("mail.login_code.subject", "app", app);
    let text = crate::t2("mail.login_code.text", "app", app, "code", code);
    let html = crate::t2("mail.login_code.html", "app", app, "code", code);
    Ok(crate::mail::Mail::new(email, &subject, &text, PURPOSE_LOGIN)?.html(&html).for_user(user_id))
}

#[cfg(test)]
mod tests;
