//! E-mail verification: a one-time link proves the learner owns the address
//! the account signs in with.
//!
//! - A link is sent on registration with a password, on "Send the e-mail
//!   again" (rate limited), and when a superuser resets the account's
//!   verification ([`reset`]).
//! - The token is 32 random bytes from the OS, hex in the link and stored
//!   only as its SHA-256 (`email_verifications.token_hash`). It works once,
//!   for [`TOKEN_TTL_SECS`], and only while the account still has the
//!   address it was sent to; a new link replaces the unused older ones.
//! - Opening it sets `users.email_verified_at`. It does not sign anyone in.
//!
//! Nothing is gated on verification yet; the state exists so features can be.
//!
//! A server without a mail provider (#633) sends no link: [`send`] logs and
//! returns, and the profile shows no red ! for an address it cannot confirm.

use axum::http::HeaderMap;
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};

use crate::error::AppError;
use crate::mail::Mail;
use crate::AppState;

/// How long a link works.
pub const TOKEN_TTL_SECS: i64 = 24 * 60 * 60;
/// Rows (used, expired or replaced) are deleted this long after they were made.
pub const KEEP_DAYS: i64 = 7;
/// The outbox `purpose` of the message.
pub const PURPOSE: &str = "email-verify";

/// The stored form of a token: SHA-256, hex.
pub fn hash_token(token: &str) -> String {
    Sha256::digest(token.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// A new link token for `user_id` at `email`; the unused ones sent before
/// stop working.
pub fn issue(conn: &Connection, user_id: i64, email: &str, now: i64) -> rusqlite::Result<String> {
    conn.execute("DELETE FROM email_verifications WHERE user_id = ?1 AND used_at IS NULL", [user_id])?;
    let token = crate::auth::generate_token();
    conn.execute(
        "INSERT INTO email_verifications (user_id, email, token_hash, created_at, expires_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![user_id, email, hash_token(&token), now, now + TOKEN_TTL_SECS],
    )?;
    Ok(token)
}

/// What opening a link did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The account's address is confirmed now.
    Confirmed { user_id: i64 },
    /// Unknown, used, expired, replaced, or sent to an address the account
    /// no longer has. The page says the same for all of them.
    Invalid,
}

/// Use a link's token: mark it used and the account's address confirmed.
pub fn confirm(conn: &mut Connection, token: &str, now: i64) -> rusqlite::Result<Outcome> {
    if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Ok(Outcome::Invalid);
    }
    let tx = conn.transaction()?;
    let found: Option<(i64, i64)> = tx
        .query_row(
            "SELECT v.id, v.user_id FROM email_verifications v JOIN users u ON u.id = v.user_id
             WHERE v.token_hash = ?1 AND v.used_at IS NULL AND v.expires_at > ?2 AND lower(u.email) = lower(v.email)",
            params![hash_token(&token.to_ascii_lowercase()), now],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((id, user_id)) = found else { return Ok(Outcome::Invalid) };
    tx.execute("UPDATE email_verifications SET used_at = ?2 WHERE id = ?1", params![id, now])?;
    mark_verified(&tx, user_id, now)?;
    tx.commit()?;
    Ok(Outcome::Confirmed { user_id })
}

/// The account's current address is confirmed (a link).
/// Keeps the first confirmation time.
pub fn mark_verified(conn: &Connection, user_id: i64, now: i64) -> rusqlite::Result<()> {
    conn.execute("UPDATE users SET email_verified_at = COALESCE(email_verified_at, ?2) WHERE id = ?1", params![user_id, now])?;
    Ok(())
}

/// The address changed: it is not confirmed any more, and links sent to the
/// old one stop working (tests).
#[cfg(test)]
pub fn mark_unverified(conn: &Connection, user_id: i64) -> rusqlite::Result<()> {
    conn.execute("UPDATE users SET email_verified_at = NULL WHERE id = ?1", [user_id])?;
    conn.execute("DELETE FROM email_verifications WHERE user_id = ?1 AND used_at IS NULL", [user_id])?;
    Ok(())
}

/// Delete rows older than [`KEEP_DAYS`] (hourly housekeeping): every one of
/// them has been used, has expired or was replaced.
pub fn purge(conn: &Connection, now: i64) -> rusqlite::Result<usize> {
    conn.execute("DELETE FROM email_verifications WHERE created_at < ?1", [now - KEEP_DAYS * 86_400])
}

/// How many of these one address gets a day at most, which the message
/// states to someone who did not sign up (#395): no promise that nothing
/// else comes, since anyone can type the address again, but the per-address
/// cap ([`crate::mail::PER_ADDRESS`]) bounds it.
fn max_per_day() -> i64 {
    crate::mail::PER_ADDRESS.iter().find(|c| c.purpose == PURPOSE).map_or(10, |c| c.max)
}

/// The message with the link.
pub fn message(email: &str, link: &str, user_id: i64) -> Result<Mail, String> {
    let app = crate::APP_NAME;
    let max = max_per_day();
    let subject = crate::t1("mail.verify_email.subject", "app", app);
    let text = crate::i18n::fill(&crate::t("mail.verify_email.text"), &[("app", &app), ("link", &link), ("max", &max)]);
    let href = html_escape(link);
    let html = crate::i18n::fill(&crate::t("mail.verify_email.html"), &[("app", &app), ("link", &href), ("max", &max)]);
    Ok(Mail::new(email, &subject, &text, PURPOSE)?.html(&html).for_user(user_id))
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Where links point: `SKRONGLE_PUBLIC_URL` when set, else the host the request
/// was made to (the forwarded one behind a proxy). The scheme is the
/// proxy's `X-Forwarded-Proto`, else https when the site says it is served
/// over HTTPS (`SKRONGLE_SECURE_COOKIES=1`), else http. Includes the base path.
pub fn public_base(state: &AppState, headers: &HeaderMap) -> String {
    if let Some(u) = state.public_url.as_deref() {
        return u.trim_end_matches('/').to_string();
    }
    let get = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).map(|v| v.split(',').next().unwrap_or("").trim().to_string());
    let valid = |h: &String| !h.is_empty() && h.bytes().all(|b| b.is_ascii_graphic() && !b"/\\?#@".contains(&b));
    let host = get("x-forwarded-host").filter(valid).or_else(|| get("host").filter(valid)).unwrap_or_else(|| "localhost".into());
    let scheme = match get("x-forwarded-proto").as_deref() {
        Some("https") => "https",
        Some("http") => "http",
        _ if state.secure_cookies => "https",
        _ => "http",
    };
    format!("{scheme}://{host}{}", crate::base_path())
}

/// Issue a token for `user_id` at `email` and queue the message. In dev mode
/// the link is logged too. Without a mail provider nothing is issued or
/// queued (#633).
pub async fn send(state: &AppState, headers: &HeaderMap, user_id: i64, email: &str) -> Result<(), AppError> {
    if !state.mail.can_send() {
        tracing::info!("no verification e-mail for user {user_id}: this server cannot send mail (SKRONGLE_MAIL_PROVIDERS)");
        return Ok(());
    }
    let e = email.to_string();
    let token = state.db.call(move |c| issue(c, user_id, &e, crate::db::now())).await?;
    let link = format!("{}/verify-email?token={token}", public_base(state, headers));
    let mail = message(email, &link, user_id).map_err(AppError::Internal)?;
    // Over the hourly ceiling the message waits for a later hour (#579). A
    // cap that says no (this address's daily share, or every hour it could
    // wait for full) sends nothing, but the page says the same, so it tells
    // nobody about the address.
    let Ok(id) = state.mail.try_queue(&state.db, mail).await? else {
        tracing::warn!("no verification e-mail for user {user_id}: a mail cap said no");
        return Ok(());
    };
    if state.dev {
        tracing::info!("dev: verification link for user {user_id} (mail #{id}): {link}");
    }
    Ok(())
}

/// Through `idx_users_email_lower` (#606), like every lookup by address.
const RESET_SQL: &str = "SELECT id, email FROM users WHERE lower(email) = lower(?1)";

/// The superuser action (#220): the account with `email` (any case) is
/// unconfirmed again and gets a new link, as if it had just registered.
/// `None` when no account has that address.
pub async fn reset(state: &AppState, headers: &HeaderMap, email: &str) -> Result<Option<i64>, AppError> {
    let e = email.trim().to_lowercase();
    let found = state
        .db
        .call(move |c| {
            let Some((id, email)) = c
                .query_row(RESET_SQL, [&e], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
                .optional()?
            else {
                return Ok(None);
            };
            c.execute("UPDATE users SET email_verified_at = NULL WHERE id = ?1", [id])?;
            Ok(Some((id, email)))
        })
        .await?;
    let Some((id, email)) = found else { return Ok(None) };
    send(state, headers, id, &email).await?;
    Ok(Some(id))
}

/// [`send`], logging instead of failing: the request it belongs to (a
/// registration, a profile save) has already succeeded.
pub async fn send_or_log(state: &AppState, headers: &HeaderMap, user_id: i64, email: &str) {
    if let Err(e) = send(state, headers, user_id, email).await {
        tracing::warn!("could not queue the verification e-mail for user {user_id}: {e:?}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_finds_the_account_through_the_index() {
        let db = crate::db::Db::open_in_memory().unwrap();
        let scans = db.with(|c| crate::db::table_scans(c, RESET_SQL)).unwrap();
        assert_eq!(scans, Vec::<String>::new());
    }

    fn verified(c: &Connection, u: i64) -> Option<i64> {
        c.query_row("SELECT email_verified_at FROM users WHERE id = ?1", [u], |r| r.get(0)).unwrap()
    }

    #[test]
    fn a_token_confirms_once_and_only_in_time() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let ann = crate::auth::create_user(c, "ann@example.org", "h")?;
            let t = issue(c, ann, "ann@example.org", 1000)?;
            assert_eq!(t.len(), 64);
            // Only the hash is stored.
            let stored: String = c.query_row("SELECT token_hash FROM email_verifications", [], |r| r.get(0))?;
            assert_eq!(stored, hash_token(&t));
            assert_ne!(stored, t);
            assert_eq!(verified(c, ann), None);

            assert_eq!(confirm(c, "nonsense", 1001)?, Outcome::Invalid);
            assert_eq!(confirm(c, &"0".repeat(64), 1001)?, Outcome::Invalid);
            assert_eq!(confirm(c, &t, 1001)?, Outcome::Confirmed { user_id: ann });
            assert_eq!(verified(c, ann), Some(1001));
            assert_eq!(confirm(c, &t, 1002)?, Outcome::Invalid, "single use");

            // Expired.
            let bob = crate::auth::create_user(c, "bob@example.org", "h")?;
            let t = issue(c, bob, "bob@example.org", 1000)?;
            assert_eq!(confirm(c, &t, 1000 + TOKEN_TTL_SECS)?, Outcome::Invalid);
            assert_eq!(verified(c, bob), None);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn a_new_link_or_a_new_address_retires_the_old_links() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let ann = crate::auth::create_user(c, "ann@example.org", "h")?;
            let first = issue(c, ann, "ann@example.org", 10)?;
            let second = issue(c, ann, "ann@example.org", 20)?;
            assert_eq!(confirm(c, &first, 30)?, Outcome::Invalid, "superseded");
            assert_eq!(confirm(c, &second, 30)?, Outcome::Confirmed { user_id: ann });

            let bob = crate::auth::create_user(c, "bob@example.org", "h")?;
            let t = issue(c, bob, "bob@example.org", 10)?;
            // The address changed after the link went out: the link is void.
            c.execute("UPDATE users SET email = 'bobby@example.org' WHERE id = ?1", [bob])?;
            assert_eq!(confirm(c, &t, 11)?, Outcome::Invalid);
            let t = issue(c, bob, "bobby@example.org", 12)?;
            // Mixed case in the link is the same token.
            assert_eq!(confirm(c, &t.to_ascii_uppercase(), 13)?, Outcome::Confirmed { user_id: bob });
            mark_unverified(c, bob)?;
            assert_eq!(verified(c, bob), None);

            // Purge: rows older than a week go, newer ones stay. Left: ann's
            // second link (20) and bob's last (12); the others were replaced.
            assert_eq!(purge(c, 12 + KEEP_DAYS * 86_400)?, 0);
            assert_eq!(purge(c, 13 + KEEP_DAYS * 86_400)?, 1);
            assert_eq!(purge(c, 21 + KEEP_DAYS * 86_400)?, 1);
            assert_eq!(c.query_row("SELECT COUNT(*) FROM email_verifications", [], |r| r.get::<_, i64>(0))?, 0);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn the_message_carries_the_link() {
        let m = message("ann@example.org", "http://127.0.0.1:8080/verify-email?token=abc", 7).unwrap();
        assert_eq!(m.purpose, PURPOSE);
        assert_eq!(m.user_id, Some(7));
        assert_eq!(m.subject, format!("Confirm your e-mail address for {}", crate::APP_NAME));
        assert!(m.text.contains("http://127.0.0.1:8080/verify-email?token=abc"));
        assert!(m.html.as_deref().unwrap().contains("href=\"http://127.0.0.1:8080/verify-email?token=abc\""));
        // No promise that nothing else will be sent (#395): the cap is stated instead.
        for body in [&m.text, m.html.as_ref().unwrap()] {
            assert!(!body.contains("nothing else will be sent"), "{body}");
            assert!(body.contains("at most 10 of these messages to one address a day"), "{body}");
        }
    }

    /// Without a mail provider no link is issued and nothing is queued (#633).
    #[tokio::test]
    async fn no_mail_provider_sends_no_link() {
        let state = crate::routes::test_state_without(false, true);
        let uid = state.db.with(|c| crate::auth::create_user(c, "ann@example.org", "h")).unwrap();
        send(&state, &HeaderMap::new(), uid, "ann@example.org").await.unwrap();
        let (links, mails): (i64, i64) =
            state.db.with(|c| c.query_row("SELECT (SELECT COUNT(*) FROM email_verifications), (SELECT COUNT(*) FROM mail_outbox)", [], |r| Ok((r.get(0)?, r.get(1)?)))).unwrap();
        assert_eq!((links, mails), (0, 0));
        // With one, the link goes out as before.
        let state = crate::routes::test_state(false, None);
        let uid = state.db.with(|c| crate::auth::create_user(c, "ann@example.org", "h")).unwrap();
        send(&state, &HeaderMap::new(), uid, "ann@example.org").await.unwrap();
        let mails: i64 = state.db.with(|c| c.query_row("SELECT COUNT(*) FROM mail_outbox WHERE purpose = ?1", [PURPOSE], |r| r.get(0))).unwrap();
        assert_eq!(mails, 1);
    }

    #[test]
    fn links_use_the_public_url_or_the_request_host() {
        let mut h = HeaderMap::new();
        h.insert("host", "127.0.0.1:8123".parse().unwrap());
        let state = crate::routes::test_state(false, None);
        assert_eq!(public_base(&state, &h), "http://127.0.0.1:8123");
        h.insert("x-forwarded-host", "learn.example.org".parse().unwrap());
        h.insert("x-forwarded-proto", "https".parse().unwrap());
        assert_eq!(public_base(&state, &h), "https://learn.example.org");
        h.insert("x-forwarded-host", "evil.example/path".parse().unwrap());
        assert_eq!(public_base(&state, &h), "https://127.0.0.1:8123", "a host with a path is ignored");
        let mut state = std::sync::Arc::try_unwrap(state).ok().unwrap();
        state.public_url = Some("https://skrongle.example.org/".into());
        assert_eq!(public_base(&state, &h), "https://skrongle.example.org");
    }
}
