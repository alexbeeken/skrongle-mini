//! Password reset, and what registration does with an address that already
//! has an account (#379).
//!
//! - `/reset` asks for an address and always answers "if that address has an
//!   account, a link was sent". The link (`/reset?token=…`) is 32 random
//!   bytes from the OS, hex, stored only as its SHA-256
//!   (`password_resets.token_hash`). It works once, for [`TOKEN_TTL_SECS`],
//!   and only while the account still has the address it was sent to; a new
//!   link replaces the unused older ones.
//! - Using it sets the new password, confirms the address (the link proved
//!   the learner reads its mail) and ends every session of the account. It
//!   does not sign anyone in: the login page (and two-factor sign-in, when
//!   on) comes next.
//! - Registering an address that has an account creates nothing: the page
//!   says "check your mail" as for a new account, and the address is sent
//!   [`taken_message`] (sign in, or reset the password). An unconfirmed
//!   password account older than [`unconfirmed_days`] is [`replaceable`]:
//!   registration deletes it (`privacy::delete_account`) and creates the
//!   new one, so nobody can hold an address they cannot read.

use axum::http::HeaderMap;
use rusqlite::{params, Connection, OptionalExtension};

use crate::error::AppError;
use crate::mail::Mail;
use crate::verify::{hash_token, public_base};
use crate::AppState;

/// How long a link works.
pub const TOKEN_TTL_SECS: i64 = 60 * 60;
/// Rows (used, expired or replaced) are deleted this long after they were made.
pub const KEEP_DAYS: i64 = 7;
/// The outbox `purpose` of the reset message.
pub const PURPOSE: &str = "password-reset";
/// The outbox `purpose` of "someone tried to register with your address".
pub const PURPOSE_TAKEN: &str = "register-taken";
/// `SKRONGLE_UNCONFIRMED_DAYS` when unset.
pub const DEFAULT_UNCONFIRMED_DAYS: i64 = 7;

/// After how many days an unconfirmed password account gives its address up
/// to a new registration (`SKRONGLE_UNCONFIRMED_DAYS`, default 7; `0` never).
pub fn unconfirmed_days() -> i64 {
    parse_days(crate::env("UNCONFIRMED_DAYS").as_deref())
}

fn parse_days(v: Option<&str>) -> i64 {
    v.and_then(|v| v.trim().parse::<i64>().ok()).filter(|d| *d >= 0).unwrap_or(DEFAULT_UNCONFIRMED_DAYS)
}

/// The account an address names (any case): (id, the address as stored).
pub fn account_for(conn: &Connection, email: &str) -> rusqlite::Result<Option<(i64, String)>> {
    conn.query_row(
        "SELECT id, email FROM users WHERE lower(email) = lower(?1) ORDER BY id LIMIT 1",
        [email],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
}

/// The account with `email` that a new registration may replace: a password
/// account whose address was never confirmed, made at least `days` days
/// before `now`.
pub fn replaceable(conn: &Connection, email: &str, now: i64, days: i64) -> rusqlite::Result<Option<i64>> {
    if days <= 0 {
        return Ok(None);
    }
    conn.query_row(
            "SELECT id FROM users WHERE lower(email) = lower(?1) AND email_verified_at IS NULL AND google_sub IS NULL
               AND password_hash <> '' AND superuser = 0 AND created_at <= ?2 ORDER BY id LIMIT 1",
            params![email, now - days * 86_400],
            |r| r.get(0),
        )
        .optional()
}

/// A new link token for `user_id` at `email`; the unused ones sent before
/// stop working.
pub fn issue(conn: &Connection, user_id: i64, email: &str, now: i64) -> rusqlite::Result<String> {
    conn.execute("DELETE FROM password_resets WHERE user_id = ?1 AND used_at IS NULL", [user_id])?;
    let token = crate::auth::generate_token();
    conn.execute(
        "INSERT INTO password_resets (user_id, email, token_hash, created_at, expires_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![user_id, email, hash_token(&token), now, now + TOKEN_TTL_SECS],
    )?;
    Ok(token)
}

/// The unused, unexpired link `token` names: (row id, user id, the address).
fn find(conn: &Connection, token: &str, now: i64) -> rusqlite::Result<Option<(i64, i64, String)>> {
    if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Ok(None);
    }
    conn.query_row(
        "SELECT r.id, r.user_id, u.email FROM password_resets r JOIN users u ON u.id = r.user_id
         WHERE r.token_hash = ?1 AND r.used_at IS NULL AND r.expires_at > ?2 AND lower(u.email) = lower(r.email)",
        params![hash_token(&token.to_ascii_lowercase()), now],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .optional()
}

/// The address a working link is for (the form shows it); `None` for an
/// unknown, used, expired or replaced one.
pub fn valid(conn: &Connection, token: &str, now: i64) -> rusqlite::Result<Option<String>> {
    Ok(find(conn, token, now)?.map(|(_, _, email)| email))
}

/// Use a link: the account's password is `password_hash` now, its address
/// confirmed, the link used up and every session ended. `None` (nothing
/// changed) when the link does not work.
pub fn use_link(conn: &mut Connection, token: &str, password_hash: &str, now: i64) -> rusqlite::Result<Option<i64>> {
    let tx = conn.transaction()?;
    let Some((id, user_id, _)) = find(&tx, token, now)? else { return Ok(None) };
    tx.execute("UPDATE password_resets SET used_at = ?2 WHERE id = ?1", params![id, now])?;
    tx.execute("DELETE FROM password_resets WHERE user_id = ?1 AND used_at IS NULL", [user_id])?;
    crate::profile::set_password_hash(&tx, user_id, password_hash)?;
    crate::verify::mark_verified(&tx, user_id, now)?;
    tx.execute("DELETE FROM sessions WHERE user_id = ?1", [user_id])?;
    tx.commit()?;
    Ok(Some(user_id))
}

/// Delete rows older than [`KEEP_DAYS`] (hourly housekeeping): every one of
/// them has been used, has expired or was replaced.
pub fn purge(conn: &Connection, now: i64) -> rusqlite::Result<usize> {
    conn.execute("DELETE FROM password_resets WHERE created_at < ?1", [now - KEEP_DAYS * 86_400])
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The message with the reset link (`mail.password_reset.*`).
pub fn message(email: &str, link: &str, user_id: i64) -> Result<Mail, String> {
    let app = crate::APP_NAME;
    let subject = crate::t1("mail.password_reset.subject", "app", app);
    let text = crate::i18n::fill(&crate::t("mail.password_reset.text"), &[("app", &app), ("link", &link)]);
    let href = html_escape(link);
    let html = crate::i18n::fill(&crate::t("mail.password_reset.html"), &[("app", &app), ("link", &href)]);
    Ok(Mail::new(email, &subject, &text, PURPOSE)?.html(&html).for_user(user_id))
}

/// "Someone tried to register with your address": sign in, or reset the
/// password (`mail.register_taken.*`).
pub fn taken_message(email: &str, base: &str, user_id: i64) -> Result<Mail, String> {
    let app = crate::APP_NAME;
    let (login, reset) = (format!("{base}/login"), format!("{base}/reset"));
    let subject = crate::t1("mail.register_taken.subject", "app", app);
    let text = crate::i18n::fill(&crate::t("mail.register_taken.text"), &[("app", &app), ("login", &login), ("reset", &reset)]);
    let (l, r) = (html_escape(&login), html_escape(&reset));
    let html = crate::i18n::fill(&crate::t("mail.register_taken.html"), &[("app", &app), ("login", &l), ("reset", &r)]);
    Ok(Mail::new(email, &subject, &text, PURPOSE_TAKEN)?.html(&html).for_user(user_id))
}

/// The language an account picked (`users.lang`; `''` follows the browser):
/// a message to the account is written in it, else in the request's.
async fn lang_of(state: &AppState, user_id: i64) -> Result<String, AppError> {
    Ok(state
        .db
        .call(move |c| c.query_row("SELECT lang FROM users WHERE id = ?1", [user_id], |r| r.get::<_, String>(0)).optional())
        .await?
        .unwrap_or_default())
}

/// Issue a link for `user_id` at `email` and queue the message. In dev mode
/// the link is logged too.
pub async fn send(state: &AppState, headers: &HeaderMap, user_id: i64, email: &str) -> Result<(), AppError> {
    let e = email.to_string();
    let token = state.db.call(move |c| issue(c, user_id, &e, crate::db::now())).await?;
    let link = format!("{}/reset?token={token}", public_base(state, headers));
    let lang = lang_of(state, user_id).await?;
    let mail = crate::i18n::in_lang(&lang, || message(email, &link, user_id)).map_err(AppError::Internal)?;
    let id = state.mail.queue(&state.db, mail).await?;
    if state.dev {
        tracing::info!("dev: password reset link for user {user_id} (mail #{id}): {link}");
    }
    Ok(())
}

/// Queue [`taken_message`] to the account's address.
pub async fn send_taken(state: &AppState, headers: &HeaderMap, user_id: i64, email: &str) -> Result<(), AppError> {
    let (base, lang) = (public_base(state, headers), lang_of(state, user_id).await?);
    let mail = crate::i18n::in_lang(&lang, || taken_message(email, &base, user_id)).map_err(AppError::Internal)?;
    state.mail.queue(&state.db, mail).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash_of(c: &Connection, u: i64) -> String {
        c.query_row("SELECT password_hash FROM users WHERE id = ?1", [u], |r| r.get(0)).unwrap()
    }

    #[test]
    fn a_reset_link_works_once_in_time_and_ends_every_session() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let ann = crate::auth::create_user(c, "ann@example.org", "old")?;
            let (s1, s2) = (crate::auth::create_session(c, ann)?, crate::auth::create_session(c, ann)?);
            let t = issue(c, ann, "ann@example.org", 1000)?;
            assert_eq!(t.len(), 64);
            // Only the hash is stored.
            let stored: String = c.query_row("SELECT token_hash FROM password_resets", [], |r| r.get(0))?;
            assert_eq!(stored, hash_token(&t));
            assert_ne!(stored, t);

            assert_eq!(valid(c, "nonsense", 1001)?, None);
            assert_eq!(use_link(c, &"0".repeat(64), "new", 1001)?, None);
            assert_eq!(valid(c, &t, 1001)?.as_deref(), Some("ann@example.org"));
            assert_eq!(use_link(c, &t.to_ascii_uppercase(), "new", 1001)?, Some(ann));
            assert_eq!(hash_of(c, ann), "new");
            let verified: Option<i64> = c.query_row("SELECT email_verified_at FROM users WHERE id = ?1", [ann], |r| r.get(0))?;
            assert_eq!(verified, Some(1001), "the link proved the address");
            for s in [&s1, &s2] {
                assert!(crate::auth::user_for_session(c, s)?.is_none(), "every session ended");
            }
            assert_eq!(use_link(c, &t, "again", 1002)?, None, "single use");
            assert_eq!(hash_of(c, ann), "new");

            // Expired after an hour.
            let t = issue(c, ann, "ann@example.org", 2000)?;
            assert_eq!(use_link(c, &t, "late", 2000 + TOKEN_TTL_SECS)?, None);
            assert_eq!(hash_of(c, ann), "new");
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn a_new_reset_link_or_a_new_address_voids_the_old_one() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let ann = crate::auth::create_user(c, "ann@example.org", "old")?;
            let first = issue(c, ann, "ann@example.org", 10)?;
            let second = issue(c, ann, "ann@example.org", 20)?;
            assert_eq!(use_link(c, &first, "x", 30)?, None, "superseded");
            c.execute("UPDATE users SET email = 'anne@example.org' WHERE id = ?1", [ann])?;
            assert_eq!(use_link(c, &second, "x", 30)?, None, "sent to an address the account no longer has");
            assert_eq!(hash_of(c, ann), "old");

            // Purge: rows older than a week go.
            assert_eq!(purge(c, 20 + KEEP_DAYS * 86_400)?, 0);
            assert_eq!(purge(c, 21 + KEEP_DAYS * 86_400)?, 1);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn only_old_unconfirmed_password_accounts_are_replaceable() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let day = 86_400;
            let made = |email: &str, created: i64| -> rusqlite::Result<i64> {
                let id = crate::auth::create_user(c, email, "h")?;
                c.execute("UPDATE users SET created_at = ?2 WHERE id = ?1", params![id, created])?;
                Ok(id)
            };
            let now = 100 * day;
            let old = made("old@example.org", now - 7 * day)?;
            made("young@example.org", now - 6 * day)?;
            let confirmed = made("confirmed@example.org", now - 30 * day)?;
            crate::verify::mark_verified(c, confirmed, 1)?;
            let g = crate::auth::create_google_user(c, "g-1", "google@example.org")?;
            c.execute("UPDATE users SET created_at = 1, email_verified_at = NULL WHERE id = ?1", [g])?;

            assert_eq!(replaceable(c, "OLD@example.org", now, 7)?, Some(old));
            assert_eq!(replaceable(c, "old@example.org", now, 0)?, None, "0 turns it off");
            assert_eq!(replaceable(c, "old@example.org", now, 8)?, None);
            assert_eq!(replaceable(c, "young@example.org", now, 7)?, None);
            assert_eq!(replaceable(c, "confirmed@example.org", now, 7)?, None);
            assert_eq!(replaceable(c, "google@example.org", now, 7)?, None);
            assert_eq!(replaceable(c, "nobody@example.org", now, 7)?, None);
            assert_eq!(account_for(c, "Old@Example.org")?, Some((old, "old@example.org".into())));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn unconfirmed_days_reads_the_setting() {
        assert_eq!(parse_days(None), 7);
        assert_eq!(parse_days(Some(" 30 ")), 30);
        assert_eq!(parse_days(Some("0")), 0);
        assert_eq!(parse_days(Some("-1")), 7);
        assert_eq!(parse_days(Some("soon")), 7);
    }

    #[test]
    fn the_messages_carry_their_links() {
        let m = message("ann@example.org", "http://127.0.0.1:8080/reset?token=abc", 7).unwrap();
        assert_eq!((m.purpose, m.user_id), (PURPOSE, Some(7)));
        assert!(m.text.contains("http://127.0.0.1:8080/reset?token=abc"));
        assert!(m.html.unwrap().contains("href=\"http://127.0.0.1:8080/reset?token=abc\""));
        let m = taken_message("ann@example.org", "http://127.0.0.1:8080", 7).unwrap();
        assert_eq!(m.purpose, PURPOSE_TAKEN);
        assert!(m.text.contains("http://127.0.0.1:8080/login") && m.text.contains("http://127.0.0.1:8080/reset\n"));
        assert!(m.html.unwrap().contains("href=\"http://127.0.0.1:8080/reset\""));
    }
}
