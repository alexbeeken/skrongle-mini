//! The waitlist for a slow release (#934), on the hosted site only:
//! Skrongle mini has no e-mail and makes invitation links instead.
//!
//! While the waitlist is on (Superuser → Release, kept in
//! `release_settings`, no restart), the home page is one e-mail field and
//! sign-up works only through an invitation: [`mode`] makes `open` act as
//! `invite`. Whoever joins gets a link to confirm the address (double
//! opt-in); only confirmed addresses are invited. A superuser invites one
//! person, or the next N in the order they joined: each gets an e-mail
//! with an invitation tied to their address (`signup_invites.email`), good
//! for [`INVITE_TTL_SECS`], and an account made through it is marked
//! `users.beta`. Nothing on the site shows who was invited or how many.
//! Turning the waitlist off is the launch: the normal home page returns.
//!
//! Table `waitlist`: one row per address. The confirmation token is
//! stored as its SHA-256 only, and cleared once used.

use rusqlite::{params, Connection, OptionalExtension};
use std::sync::atomic::{AtomicBool, Ordering};

/// The confirmation e-mail's purpose (outbox, logs, per-address cap).
pub const PURPOSE_CONFIRM: &str = "waitlist-confirm";
/// The invitation e-mail's purpose.
pub const PURPOSE_INVITE: &str = "waitlist-invite";
/// How long an e-mailed invitation works; Resend makes a new one.
pub const INVITE_TTL_SECS: i64 = 14 * 86_400;
/// An address not confirmed after this long leaves the list (hourly housekeeping).
pub const UNCONFIRMED_TTL_SECS: i64 = 30 * 86_400;
/// Rows the Release page lists at a time.
pub const PER_PAGE: i64 = 50;
/// The most "Invite the next N" sends in one press.
pub const MAX_BATCH: i64 = 1000;
/// `SKRONGLE_WAITLIST_LOGIN=1`: the waitlist page shows "Already invited?
/// Log in" (for the soft launch). Without it, Log in is not shown anywhere
/// while the waitlist is on; `/login` still works.
pub const LOGIN_VAR: &str = "WAITLIST_LOGIN";

/// The switch and the login link (`AppState::waitlist`).
#[derive(Debug, Default)]
pub struct Switch {
    on: AtomicBool,
    login_link: AtomicBool,
}

impl Switch {
    /// At startup: the stored switch, and `SKRONGLE_WAITLIST_LOGIN`.
    pub fn load(&self, conn: &Connection, login_link: bool) -> rusqlite::Result<()> {
        self.login_link.store(login_link, Ordering::Relaxed);
        self.on.store(stored(conn)?, Ordering::Relaxed);
        Ok(())
    }

    pub fn is_on(&self) -> bool {
        self.on.load(Ordering::Relaxed)
    }

    /// Turn the waitlist on or off, stored and at once.
    pub fn set(&self, conn: &Connection, on: bool) -> rusqlite::Result<()> {
        conn.execute("INSERT INTO release_settings (id, waitlist) VALUES (1, ?1) ON CONFLICT (id) DO UPDATE SET waitlist = excluded.waitlist", [on])?;
        self.on.store(on, Ordering::Relaxed);
        Ok(())
    }
}

/// The switch as stored (off when never set).
pub fn stored(conn: &Connection) -> rusqlite::Result<bool> {
    Ok(conn.query_row("SELECT waitlist FROM release_settings WHERE id = 1", [], |r| r.get(0)).optional()?.unwrap_or(false))
}

tokio::task_local! {
    /// The switch and the login link for the request being handled, for
    /// templates (the header's Log in and Create account) and handlers.
    static CURRENT: (bool, bool);
}

/// Is the waitlist on, for the request being handled? Off outside one.
pub fn on() -> bool {
    CURRENT.try_with(|c| c.0).unwrap_or(false)
}

/// Does the page show Log in? Always with the waitlist off.
pub fn shows_login() -> bool {
    CURRENT.try_with(|&(on, login)| !on || login).unwrap_or(true)
}

/// Middleware: run the request with the switch as it is now.
pub async fn middleware(axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::AppState>>, req: axum::extract::Request, next: axum::middleware::Next) -> axum::response::Response {
    let now = (state.waitlist.is_on(), state.waitlist.login_link.load(Ordering::Relaxed));
    CURRENT.scope(now, next.run(req)).await
}

/// The sign-up mode in force: `SKRONGLE_SIGNUP`, except that `open` acts
/// as `invite` while the waitlist is on.
pub fn mode(env: crate::signup::Mode) -> crate::signup::Mode {
    if env == crate::signup::Mode::Open && on() {
        crate::signup::Mode::Invite
    } else {
        env
    }
}

/// What joining did. The page says "check your e-mail" whichever it is,
/// so nobody learns whether an address is on the list.
#[derive(Debug, PartialEq, Eq)]
pub enum Joined {
    /// New, or not confirmed yet: mail this confirmation token.
    Confirm(String),
    /// Already confirmed, or already has an account: nothing to send.
    Nothing,
}

/// Put `email` (already checked and lower-cased) on the list, or give an
/// unconfirmed one a new confirmation token.
pub fn join(conn: &Connection, email: &str, now: i64) -> rusqlite::Result<Joined> {
    let has_account: bool = conn.query_row("SELECT EXISTS (SELECT 1 FROM users WHERE lower(email) = lower(?1))", [email], |r| r.get(0))?;
    let confirmed: Option<Option<i64>> = conn.query_row("SELECT confirmed_at FROM waitlist WHERE email = ?1", [email], |r| r.get(0)).optional()?;
    if has_account || matches!(confirmed, Some(Some(_))) {
        return Ok(Joined::Nothing);
    }
    let token = crate::auth::generate_token();
    conn.execute(
        "INSERT INTO waitlist (email, joined_at, confirm_hash) VALUES (?1, ?2, ?3)
         ON CONFLICT (email) DO UPDATE SET confirm_hash = excluded.confirm_hash",
        params![email, now, crate::auth::hash_token(&token)],
    )?;
    Ok(Joined::Confirm(token))
}

/// Confirm the address the token was sent to. False for an unknown or used token.
pub fn confirm(conn: &Connection, token: &str, now: i64) -> rusqlite::Result<bool> {
    let token = token.trim();
    if token.is_empty() || token.len() > 128 {
        return Ok(false);
    }
    Ok(conn.execute(
        "UPDATE waitlist SET confirmed_at = ?2, confirm_hash = NULL WHERE confirm_hash = ?1 AND confirmed_at IS NULL",
        params![crate::auth::hash_token(token), now],
    )? == 1)
}

/// After an account is made (inside `signup::finish`'s transaction): its
/// address leaves the waiting part of the list, and an account made
/// through an e-mailed invitation (`invite`) is a beta account.
pub fn signed_up(conn: &Connection, invite: Option<i64>, user_id: i64, now: i64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE waitlist SET signed_up_at = ?2, user_id = ?1 WHERE signed_up_at IS NULL AND email = (SELECT lower(email) FROM users WHERE id = ?1)",
        params![user_id, now],
    )?;
    if let Some(id) = invite {
        conn.execute("UPDATE users SET beta = 1 WHERE id = ?1 AND EXISTS (SELECT 1 FROM signup_invites WHERE id = ?2 AND email IS NOT NULL)", params![user_id, id])?;
    }
    Ok(())
}

/// One invitation to mail: the address and the token for its link.
#[derive(Debug, Clone)]
pub struct Invitation {
    pub email: String,
    pub token: String,
}

/// Mail one invitation. The caller builds the message (it knows the site).
pub type Sender<'a> = dyn FnMut(&Connection, &Invitation) -> rusqlite::Result<bool> + 'a;

/// Invite the confirmed person `id` (again, for Resend): any earlier open
/// invitation of theirs stops working, a new one is made and `send` mails
/// it, all in one transaction. False when there is nobody to invite (not
/// confirmed, signed up already) or `send` could not queue it.
pub fn invite_one(conn: &Connection, id: i64, created_by: i64, now: i64, send: &mut Sender) -> rusqlite::Result<bool> {
    let tx = conn.unchecked_transaction()?;
    let email: Option<String> = tx
        .query_row("SELECT email FROM waitlist WHERE id = ?1 AND confirmed_at IS NOT NULL AND signed_up_at IS NULL", [id], |r| r.get(0))
        .optional()?;
    let Some(email) = email else { return Ok(false) };
    if !invite_row(&tx, id, &email, created_by, now, send)? {
        return Ok(false);
    }
    tx.commit()?;
    Ok(true)
}

/// Invite the next `n` confirmed people not invited yet, in the order they
/// joined. Stops early when the mailer's hourly ceiling is full. How many
/// were invited.
pub fn invite_next(conn: &Connection, n: i64, created_by: i64, now: i64, send: &mut Sender) -> rusqlite::Result<usize> {
    let tx = conn.unchecked_transaction()?;
    let todo: Vec<(i64, String)> = tx
        .prepare(
            "SELECT id, email FROM waitlist WHERE confirmed_at IS NOT NULL AND invited_at IS NULL AND signed_up_at IS NULL
             AND NOT EXISTS (SELECT 1 FROM users WHERE lower(users.email) = waitlist.email) ORDER BY id LIMIT ?1",
        )?
        .query_map([n.clamp(0, MAX_BATCH)], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut invited = 0;
    for (id, email) in todo {
        if !invite_row(&tx, id, &email, created_by, now, send)? {
            break;
        }
        invited += 1;
    }
    tx.commit()?;
    Ok(invited)
}

fn invite_row(conn: &Connection, id: i64, email: &str, created_by: i64, now: i64, send: &mut Sender) -> rusqlite::Result<bool> {
    conn.execute(
        "DELETE FROM signup_invites WHERE used_at IS NULL AND id = (SELECT invite_id FROM waitlist WHERE id = ?1)",
        [id],
    )?;
    let token = crate::auth::generate_token();
    conn.execute(
        "INSERT INTO signup_invites (token_hash, created_by, created_at, expires_at, email) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![crate::auth::hash_token(&token), created_by, now, now + INVITE_TTL_SECS, email],
    )?;
    let invite = conn.last_insert_rowid();
    if !send(conn, &Invitation { email: email.to_string(), token })? {
        // A batch keeps the ones before it: only this invitation goes.
        conn.execute("DELETE FROM signup_invites WHERE id = ?1", [invite])?;
        return Ok(false);
    }
    conn.execute(
        "UPDATE waitlist SET invite_id = ?2, invited_at = ?3, invites_sent = invites_sent + 1 WHERE id = ?1",
        params![id, invite, now],
    )?;
    Ok(true)
}

/// Where a person on the list is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Unconfirmed,
    Waiting,
    /// Invited; the invitation still works.
    Invited,
    /// Invited, but the invitation ran out (or was purged).
    Expired,
    SignedUp,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Unconfirmed => "unconfirmed",
            Status::Waiting => "waiting",
            Status::Invited => "invited",
            Status::Expired => "expired",
            Status::SignedUp => "signed-up",
        }
    }
}

/// One person on the list.
#[derive(Debug, Clone)]
pub struct Entry {
    pub id: i64,
    pub email: String,
    pub joined_at: i64,
    pub invited_at: Option<i64>,
    pub invites_sent: i64,
    pub status: Status,
}

/// How many are in each state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Counts {
    pub total: i64,
    pub unconfirmed: i64,
    pub waiting: i64,
    pub invited: i64,
    pub signed_up: i64,
}

pub fn counts(conn: &Connection) -> rusqlite::Result<Counts> {
    conn.query_row(
        "SELECT COUNT(*),
                COALESCE(SUM(confirmed_at IS NULL), 0),
                COALESCE(SUM(confirmed_at IS NOT NULL AND invited_at IS NULL AND signed_up_at IS NULL), 0),
                COALESCE(SUM(invited_at IS NOT NULL AND signed_up_at IS NULL), 0),
                COALESCE(SUM(signed_up_at IS NOT NULL), 0)
         FROM waitlist",
        [],
        |r| Ok(Counts { total: r.get(0)?, unconfirmed: r.get(1)?, waiting: r.get(2)?, invited: r.get(3)?, signed_up: r.get(4)? }),
    )
}

/// Page `page` (from 1, clamped) of the list in the order people joined,
/// and how many pages there are.
pub fn list(conn: &Connection, page: i64, now: i64) -> rusqlite::Result<(Vec<Entry>, i64, i64)> {
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM waitlist", [], |r| r.get(0))?;
    let pages = ((total + PER_PAGE - 1) / PER_PAGE).max(1);
    let page = page.clamp(1, pages);
    let mut stmt = conn.prepare_cached(
        "SELECT w.id, w.email, w.joined_at, w.confirmed_at IS NOT NULL, w.invited_at, w.invites_sent, w.signed_up_at IS NOT NULL,
                COALESCE((SELECT i.expires_at > ?3 FROM signup_invites i WHERE i.id = w.invite_id), 0)
         FROM waitlist w ORDER BY w.id LIMIT ?1 OFFSET ?2",
    )?;
    let rows = stmt.query_map(params![PER_PAGE, (page - 1) * PER_PAGE, now], |r| {
        let (confirmed, invited_at, signed_up, live): (bool, Option<i64>, bool, bool) = (r.get(3)?, r.get(4)?, r.get(6)?, r.get(7)?);
        let status = match (signed_up, invited_at, confirmed) {
            (true, _, _) => Status::SignedUp,
            (false, Some(_), _) if live => Status::Invited,
            (false, Some(_), _) => Status::Expired,
            (false, None, true) => Status::Waiting,
            (false, None, false) => Status::Unconfirmed,
        };
        Ok(Entry { id: r.get(0)?, email: r.get(1)?, joined_at: r.get(2)?, invited_at, invites_sent: r.get(5)?, status })
    })?;
    Ok((rows.collect::<rusqlite::Result<_>>()?, page, pages))
}

/// The address an e-mailed invitation is tied to, if any.
pub fn invite_email(conn: &Connection, invite: i64) -> rusqlite::Result<Option<String>> {
    Ok(conn.query_row("SELECT email FROM signup_invites WHERE id = ?1", [invite], |r| r.get(0)).optional()?.flatten())
}

/// The confirmation e-mail: `link` confirms the address.
pub fn confirm_mail(email: &str, link: &str) -> Result<crate::mail::Mail, String> {
    mail(email, "mail.waitlist_confirm", link, PURPOSE_CONFIRM)
}

/// The invitation e-mail: `link` opens the register page with the invitation.
pub fn invite_mail(email: &str, link: &str) -> Result<crate::mail::Mail, String> {
    mail(email, "mail.waitlist_invite", link, PURPOSE_INVITE)
}

fn mail(email: &str, key: &str, link: &str, purpose: &'static str) -> Result<crate::mail::Mail, String> {
    let (app, days) = (crate::APP_NAME, (INVITE_TTL_SECS / 86_400).to_string());
    let vars = |link: &str| [("app", app.to_string()), ("link", link.to_string()), ("days", days.clone()), ("email", email.to_string())];
    let fill = |part: &str, vars: &[(&str, String)]| {
        let refs: Vec<(&str, &dyn std::fmt::Display)> = vars.iter().map(|(k, v)| (*k, v as &dyn std::fmt::Display)).collect();
        crate::i18n::fill(&crate::t(&format!("{key}.{part}")), &refs)
    };
    let esc = crate::error::html_escape;
    let html_vars = [("app", app.to_string()), ("link", esc(link)), ("days", days.clone()), ("email", esc(email))];
    crate::mail::Mail::new(email, &fill("subject", &vars(link)), &fill("text", &vars(link)), purpose).map(|m| m.html(&fill("html", &html_vars)))
}

/// Addresses never confirmed leave the list after [`UNCONFIRMED_TTL_SECS`] (hourly housekeeping).
pub fn purge(conn: &Connection, now: i64) -> rusqlite::Result<usize> {
    conn.execute("DELETE FROM waitlist WHERE confirmed_at IS NULL AND joined_at < ?1", [now - UNCONFIRMED_TTL_SECS])
}

#[cfg(test)]
mod tests;
