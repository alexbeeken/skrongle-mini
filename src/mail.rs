//! Sending e-mail: an outbox in SQLite and a chain of providers tried in
//! order, so one provider being down (or refusing a message) falls back to
//! the next.
//!
//! - Code that wants to send builds a [`Mail`] and calls [`Mailer::queue`]
//!   (or [`enqueue`] inside a transaction it already holds). The row is in
//!   `mail_outbox` before anything touches the network, so a provider outage
//!   never fails a request and a restart loses nothing.
//! - The background job runner (`jobs.rs`) delivers due rows ([`Outbox`]),
//!   leasing each one first so it is never sent twice (#873). Each message goes to the
//!   providers in `SKRONGLE_MAIL_PROVIDERS` order; the first that accepts it
//!   wins. A provider that could not be reached is tried last for
//!   [`COOLDOWN`], so a dead primary does not slow every message down.
//! - A message no provider took is retried on [`RETRY_DELAYS`]; one every
//!   provider *rejected* (a bad address, say) fails at once.
//! - Providers know nothing about the outbox or the app: a new one is a
//!   [`Provider`] impl plus a `kind` in [`build_provider`].
//!
//! - Two caps keep the outbox from being used to flood an inbox or burn the
//!   sender domain's reputation (#395): [`PER_ADDRESS`] (verification mail to
//!   one address per day, counted from the outbox rows, which outlive the
//!   account for that day) and `SKRONGLE_MAIL_PER_HOUR`, a ceiling on the
//!   messages of one purpose per hour (#579). [`enqueue`] checks both before
//!   it inserts. The per-address cap says no; the hourly ceiling only delays:
//!   a message over it is queued for the next hour with room (at most
//!   [`MAX_WAIT_WINDOWS`] ahead), except sign-in codes ([`NO_WAIT`]), which
//!   would have expired by then. Each purpose has its own budget, so a burst
//!   of share invites never holds back a verification link or a code.
//!
//! Without `SKRONGLE_MAIL_PROVIDERS` the chain is the `log` provider: messages
//! are logged and marked sent, nothing leaves the server. `docs/self-hosting.md`
//! ("E-mail") is the operator's guide.

use std::sync::mpsc::Sender;
#[cfg(test)]
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use lettre::message::{Mailbox, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::extension::ClientId;
use lettre::{Address, Message, SmtpTransport, Transport};
use rusqlite::{params, Connection, OptionalExtension};

use crate::db::Db;
use crate::error::AppError;

mod brevo;
mod postmark;
mod resend;
pub mod sink;

/// How long a provider that could not be reached goes to the back of the chain.
pub const COOLDOWN: Duration = Duration::from_secs(300);
/// Wait before each retry of a message no provider took; after the last one
/// the message is `failed`.
pub const RETRY_DELAYS: &[i64] = &[60, 300, 1800, 7200, 43_200];
/// Sent and failed rows are deleted after this long (hourly housekeeping).
pub const KEEP_DAYS: i64 = 30;
/// SMTP connect/read timeout.
const SMTP_TIMEOUT: Duration = Duration::from_secs(20);
/// `SKRONGLE_MAIL_PER_HOUR` when unset (outside dev mode): messages of one
/// purpose (`email-verify`, `course-share`, …) queued per hour, whatever the
/// address; each purpose has a budget of its own (#579).
pub const DEFAULT_PER_HOUR: i64 = 3000;
/// The ceiling's window in seconds: an hour. `SKRONGLE_MAIL_WINDOW` sets
/// another (a minute, say, to watch a delayed message leave while testing).
/// Windows are aligned to multiples of it (clock hours by default).
pub const DEFAULT_WINDOW: i64 = 3600;
/// How many windows after the current one a message over the ceiling may
/// wait for: 12 hours by default, well inside a verification link's day.
/// Every one of them full, it is not queued ([`Capped::Hourly`]).
pub const MAX_WAIT_WINDOWS: i64 = 12;
/// Purposes whose message is no use an hour later (a code that expires in
/// minutes): over the ceiling they are not queued and the page says so.
pub const NO_WAIT: &[&str] = &[];
/// Per-address caps (#395): at most `max` messages of `purpose` to one
/// address in `window` seconds, whichever account asked. Registering a
/// stranger's address and pressing "Send again" reaches them at most this
/// often, and deleting and re-registering the account does not reset it.
pub const PER_ADDRESS: &[AddressCap] = &[];

pub struct AddressCap {
    pub purpose: &'static str,
    pub max: i64,
    pub window: i64,
}

/// Why [`enqueue`] queued nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capped {
    /// [`PER_ADDRESS`]: this address had its share of this kind of message.
    Address,
    /// `SKRONGLE_MAIL_PER_HOUR`: this purpose's budget is spent for this hour
    /// and the message cannot wait ([`NO_WAIT`]), or every hour it could
    /// wait for ([`MAX_WAIT_WINDOWS`]) is full too.
    Hourly,
}

/// The hourly ceiling (`SKRONGLE_MAIL_PER_HOUR`, `SKRONGLE_MAIL_WINDOW`): at
/// most `per_window` messages of one purpose are due in each `window`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ceiling {
    pub per_window: i64,
    /// Seconds; windows start at multiples of it.
    pub window: i64,
}

// ---------- messages ----------

/// A message to send. Build it with [`Mail::new`], which checks the address.
#[derive(Clone, Debug)]
pub struct Mail {
    pub to: String,
    pub subject: String,
    pub text: String,
    /// An HTML alternative to `text`.
    pub html: Option<String>,
    /// What the message is for (`password-reset`, …): logs and the outbox name it.
    pub purpose: &'static str,
    /// The account it is about; deleting the account deletes the row.
    pub user_id: Option<i64>,
}

#[allow(dead_code)] // the first callers (password reset, verification) come with their features
impl Mail {
    pub fn new(to: &str, subject: &str, text: &str, purpose: &'static str) -> Result<Mail, String> {
        let to = to.trim();
        to.parse::<Address>().map_err(|e| format!("not an e-mail address ({e})"))?;
        if subject.contains(['\r', '\n']) {
            return Err("the subject is one line".into());
        }
        Ok(Mail { to: to.to_string(), subject: subject.to_string(), text: text.to_string(), html: None, purpose, user_id: None })
    }

    pub fn html(mut self, html: &str) -> Mail {
        self.html = Some(html.to_string());
        self
    }

    pub fn for_user(mut self, user_id: i64) -> Mail {
        self.user_id = Some(user_id);
        self
    }
}

/// What a provider is handed: the message plus the outbox id, which HTTP
/// providers can use as an idempotency key.
#[derive(Clone, Debug)]
pub struct Outgoing {
    pub id: i64,
    pub to: String,
    pub subject: String,
    pub text: String,
    pub html: Option<String>,
    pub purpose: String,
    /// Where a reply goes (`SKRONGLE_SUPPORT_EMAIL`), when it isn't the sender.
    pub reply_to: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SendError {
    /// The provider answered and refused this message (a 5xx: unknown
    /// recipient, unverified sender, …). Another provider may still take it;
    /// this one is not put in cooldown.
    Rejected(String),
    /// The provider could not be reached or failed for now (connection, TLS,
    /// timeout, a 4xx). It goes to the back of the chain for [`COOLDOWN`].
    Unavailable(String),
}

impl std::fmt::Display for SendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SendError::Rejected(m) => write!(f, "rejected: {m}"),
            SendError::Unavailable(m) => write!(f, "unavailable: {m}"),
        }
    }
}

/// One way of sending mail. Blocking: it runs on the job runner's thread.
pub trait Provider: Send + Sync {
    /// The name from `SKRONGLE_MAIL_PROVIDERS`, stored on the rows it sent.
    fn name(&self) -> &str;
    fn send(&self, from: &Mailbox, mail: &Outgoing) -> Result<(), SendError>;
}

// ---------- the chain ----------

/// The providers in fallback order, with when each last could not be reached.
pub struct Chain {
    providers: Vec<Box<dyn Provider>>,
    down_since: Mutex<Vec<Option<Instant>>>,
}

/// Every provider failed: why, per provider, in the order they were tried.
#[derive(Debug)]
pub struct ChainError {
    pub attempts: Vec<(String, SendError)>,
}

impl ChainError {
    /// Every provider refused the message itself: retrying cannot help.
    pub fn all_rejected(&self) -> bool {
        self.attempts.iter().all(|(_, e)| matches!(e, SendError::Rejected(_)))
    }

    pub fn summary(&self) -> String {
        self.attempts.iter().map(|(n, e)| format!("{n}: {e}")).collect::<Vec<_>>().join("; ")
    }
}

impl Chain {
    pub fn new(providers: Vec<Box<dyn Provider>>) -> Chain {
        let n = providers.len();
        Chain { providers, down_since: Mutex::new(vec![None; n]) }
    }

    pub fn names(&self) -> Vec<&str> {
        self.providers.iter().map(|p| p.name()).collect()
    }

    /// The order to try now: providers out of cooldown first (in configured
    /// order), then the ones still cooling down. Nobody is skipped, so a
    /// message is never held back only because every provider failed lately.
    fn order(&self, now: Instant) -> Vec<usize> {
        let down = self.down_since.lock().unwrap_or_else(|p| p.into_inner());
        let cooling = |i: usize| down[i].is_some_and(|t| now.duration_since(t) < COOLDOWN);
        let mut order: Vec<usize> = (0..self.providers.len()).filter(|&i| !cooling(i)).collect();
        order.extend((0..self.providers.len()).filter(|&i| cooling(i)));
        order
    }

    /// Send through the first provider that takes the message; its name on success.
    pub fn send(&self, from: &Mailbox, mail: &Outgoing) -> Result<&str, ChainError> {
        self.send_at(from, mail, Instant::now())
    }

    fn send_at(&self, from: &Mailbox, mail: &Outgoing, now: Instant) -> Result<&str, ChainError> {
        let mut attempts = Vec::new();
        for i in self.order(now) {
            let p = &self.providers[i];
            let result = p.send(from, mail);
            let mut down = self.down_since.lock().unwrap_or_else(|p| p.into_inner());
            match result {
                Ok(()) => {
                    down[i] = None;
                    return Ok(p.name());
                }
                Err(e) => {
                    if matches!(e, SendError::Unavailable(_)) {
                        down[i] = Some(now);
                    }
                    attempts.push((p.name().to_string(), e));
                }
            }
        }
        Err(ChainError { attempts })
    }
}

// ---------- the outbox ----------

/// [`PER_ADDRESS`]'s count: one address's mail of one purpose since a time
/// (`idx_mail_outbox_addr`, #607).
const ADDRESS_COUNT: &str = "SELECT COUNT(*) FROM mail_outbox WHERE lower(to_addr) = lower(?1) AND purpose = ?2 AND created_at > ?3";
/// The ceiling's count: one purpose's mail due in one window
/// (`idx_mail_outbox_scheduled`, #608).
const WINDOW_COUNT: &str = "SELECT COUNT(*) FROM mail_outbox WHERE purpose = ?1 AND scheduled_at >= ?2 AND scheduled_at < ?3";

/// Whether [`PER_ADDRESS`] says no to `m` now.
fn over_address_cap(conn: &Connection, m: &Mail, now: i64) -> rusqlite::Result<bool> {
    for cap in PER_ADDRESS.iter().filter(|c| c.purpose == m.purpose) {
        let n: i64 = conn.query_row(ADDRESS_COUNT, params![m.to, cap.purpose, now - cap.window], |r| r.get(0))?;
        if n >= cap.max {
            return Ok(true);
        }
    }
    Ok(false)
}

/// When a message of `purpose` queued at `now` is due under `ceiling`
/// (`None`: no ceiling): `now` while this window has room for its purpose,
/// else the start of the first later window with room, at most
/// [`MAX_WAIT_WINDOWS`] ahead ([`NO_WAIT`] purposes: none).
fn schedule(conn: &Connection, purpose: &str, now: i64, ceiling: Option<Ceiling>) -> rusqlite::Result<Result<i64, Capped>> {
    let Some(c) = ceiling else { return Ok(Ok(now)) };
    let window = c.window.max(1);
    let start = now - now.rem_euclid(window);
    let ahead = if NO_WAIT.contains(&purpose) { 0 } else { MAX_WAIT_WINDOWS };
    for k in 0..=ahead {
        let from = start + k * window;
        let n: i64 = conn.query_row(WINDOW_COUNT, params![purpose, from, from + window], |r| r.get(0))?;
        if n < c.per_window {
            return Ok(Ok(from.max(now)));
        }
    }
    Ok(Err(Capped::Hourly))
}

/// Check the caps and put a message in the outbox: the row id, or
/// `Err(Capped)` when a cap says no. It is due now, or, over the hourly
/// ceiling, when a later hour has room for its purpose ([`schedule`]).
/// Count and insert run under one connection lock, so two requests cannot
/// both slip under a cap. The job runner sends it once it is due;
/// [`Mailer::notify`] wakes it at once.
pub fn enqueue(conn: &Connection, m: &Mail, now: i64, ceiling: Option<Ceiling>) -> rusqlite::Result<Result<i64, Capped>> {
    if over_address_cap(conn, m, now)? {
        return Ok(Err(Capped::Address));
    }
    let due = match schedule(conn, m.purpose, now, ceiling)? {
        Ok(due) => due,
        Err(capped) => return Ok(Err(capped)),
    };
    conn.execute(
        "INSERT INTO mail_outbox (user_id, to_addr, subject, body_text, body_html, purpose, next_attempt_at, scheduled_at, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, ?8)",
        params![m.user_id, m.to, m.subject, m.text, m.html.as_deref().unwrap_or(""), m.purpose, due, now],
    )?;
    let id = conn.last_insert_rowid();
    if due > now {
        tracing::info!("mail #{id} ({}) waits {}s: SKRONGLE_MAIL_PER_HOUR reached for this hour", m.purpose, due - now);
    }
    Ok(Ok(id))
}

/// Before `user_id`'s account is deleted (inside its transaction): its rows
/// that [`PER_ADDRESS`] still counts are kept, about no account and with
/// their bodies emptied, so deleting and re-registering does not reset the
/// cap. Anything still queued is not sent. [`purge`] deletes them once
/// their window is over.
pub fn keep_for_caps(conn: &Connection, user_id: i64, now: i64) -> rusqlite::Result<usize> {
    let mut kept = 0;
    for cap in PER_ADDRESS {
        kept += conn.execute(
            "UPDATE mail_outbox SET user_id = NULL, body_text = '', body_html = '',
                    last_error = CASE WHEN state = 'queued' THEN 'account deleted' ELSE last_error END,
                    state = CASE WHEN state = 'queued' THEN 'failed' ELSE state END
             WHERE user_id = ?1 AND purpose = ?2 AND created_at > ?3",
            params![user_id, cap.purpose, now - cap.window],
        )?;
    }
    Ok(kept)
}

/// The columns [`deliver_one`] reads from a leased row.
const LEASED_COLUMNS: &str = "id, to_addr, subject, body_text, body_html, purpose, attempts";

/// The next time a queued row is due (or its lease runs out), if any.
pub fn next_due(conn: &Connection) -> rusqlite::Result<Option<i64>> {
    crate::jobs::outbox_next_due(conn, "mail_outbox")
}

/// Lease the next due row for `worker`, send it through the chain and record
/// the outcome, clearing the lease: whether there was a row. A row whose
/// lease is held (being sent by this or another server) is not touched.
pub fn deliver_one(db: &Db, chain: &Chain, from: &Mailbox, reply_to: Option<&str>, keep_bodies: bool, worker: &str, now: i64) -> rusqlite::Result<bool> {
    let leased = db.with(|c| {
        crate::jobs::lease_outbox(c, "mail_outbox", LEASED_COLUMNS, worker, now, |r| {
            let html: String = r.get(4)?;
            Ok((
                Outgoing { id: r.get(0)?, to: r.get(1)?, subject: r.get(2)?, text: r.get(3)?, html: (!html.is_empty()).then_some(html), purpose: r.get(5)?, reply_to: reply_to.map(str::to_string) },
                r.get::<_, i64>(6)?,
            ))
        })
    })?;
    let Some((mail, attempts)) = leased else { return Ok(false) };
    let attempts = attempts + 1;
    match chain.send(from, &mail) {
        Ok(provider) => {
            tracing::info!("mail #{} ({}) sent via {provider}", mail.id, mail.purpose);
            db.with(|c| {
                c.execute(
                    "UPDATE mail_outbox SET state = 'sent', sent_at = ?2, provider = ?3, attempts = ?4, last_error = '', locked_by = NULL, locked_until = NULL WHERE id = ?1",
                    params![mail.id, now, provider, attempts],
                )?;
                if !keep_bodies {
                    blank(c, mail.id)?;
                }
                Ok(())
            })?;
        }
        Err(e) => {
            let why = scrub(&e.summary());
            let retry = RETRY_DELAYS.get(attempts as usize - 1).filter(|_| !e.all_rejected());
            match retry {
                Some(delay) => tracing::warn!("mail #{} ({}): no provider took it ({why}); retrying in {delay}s", mail.id, mail.purpose),
                None => tracing::error!("mail #{} ({}) failed after {attempts} attempt(s): {why}", mail.id, mail.purpose),
            }
            db.with(|c| match retry {
                Some(delay) => c
                    .execute(
                        "UPDATE mail_outbox SET attempts = ?2, next_attempt_at = ?3, last_error = ?4, locked_by = NULL, locked_until = NULL WHERE id = ?1",
                        params![mail.id, attempts, now + delay, why],
                    )
                    .map(drop),
                None => {
                    c.execute(
                        "UPDATE mail_outbox SET state = 'failed', attempts = ?2, last_error = ?3, locked_by = NULL, locked_until = NULL WHERE id = ?1",
                        params![mail.id, attempts, why],
                    )?;
                    // Never sent and never retried: its code or link is no use to anyone either.
                    if !keep_bodies {
                        blank(c, mail.id)?;
                    }
                    Ok(())
                }
            })?;
        }
    }
    Ok(true)
}

/// Purposes whose message carries a code or a link that signs in, proves
/// who you are or confirms something. Once such a row is settled (sent or
/// failed) its subject is blanked too, not only its body (#361); their
/// subjects never carry the code, but they go all the same.
/// The first three are no longer sent (#10) but stay, word for word, in
/// the partial index `idx_mail_outbox_unblanked` (db.rs v58) they match.
pub const SECRET_PURPOSES: &[&str] = &["login-code", "two-factor-confirm", "email-verify", crate::exports::PURPOSE];

/// `'a', 'b'` for an `IN (…)`; the purposes are constants without quotes.
fn sql_list(items: &[&str]) -> String {
    items.iter().map(|p| format!("'{p}'")).collect::<Vec<_>>().join(", ")
}

/// Blank a settled row: its bodies always (a sent message's text is no use
/// to anyone here), its subject too when the purpose is one of
/// [`SECRET_PURPOSES`]. Purpose, recipient and dates stay.
fn blank(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    let secret = sql_list(SECRET_PURPOSES);
    conn.execute(
        &format!("UPDATE mail_outbox SET body_text = '', body_html = '', subject = CASE WHEN purpose IN ({secret}) THEN '' ELSE subject END WHERE id = ?1"),
        [id],
    )?;
    Ok(())
}

/// [`blank_settled`]'s statement. Its second condition is, word for word,
/// the WHERE of the partial index `idx_mail_outbox_unblanked` (db.rs v58),
/// so it reads only the rows that still carry something, not the whole
/// outbox (#610); `state IN` rather than `<> 'queued'` lets it search that
/// index instead of scanning it.
fn blank_settled_sql() -> String {
    let secret = sql_list(SECRET_PURPOSES);
    format!(
        "UPDATE mail_outbox SET body_text = '', body_html = '', subject = CASE WHEN purpose IN ({secret}) THEN '' ELSE subject END
         WHERE state IN ('sent', 'failed') AND (body_text <> '' OR body_html <> '' OR (purpose IN ({secret}) AND subject <> ''))"
    )
}

/// Blank every settled row that still holds a body, or the subject of a
/// [`SECRET_PURPOSES`] message: rows from before #361, or settled while the
/// server kept bodies. The sender runs it when it starts, except in dev mode,
/// whose outbox page shows bodies. How many rows it changed.
pub fn blank_settled(conn: &Connection) -> rusqlite::Result<usize> {
    conn.execute(&blank_settled_sql(), [])
}

const OUTBOX_COLUMNS: &str =
    "id, user_id, to_addr, subject, body_text, body_html, purpose, state, attempts, provider, last_error, created_at, sent_at, next_attempt_at, locked_by, locked_until";

fn outbox_json(r: &rusqlite::Row) -> rusqlite::Result<serde_json::Value> {
    Ok(serde_json::json!({
        "id": r.get::<_, i64>(0)?, "user_id": r.get::<_, Option<i64>>(1)?, "to": r.get::<_, String>(2)?,
        "subject": r.get::<_, String>(3)?, "text": r.get::<_, String>(4)?, "html": r.get::<_, String>(5)?,
        "purpose": r.get::<_, String>(6)?, "state": r.get::<_, String>(7)?, "attempts": r.get::<_, i64>(8)?,
        "provider": r.get::<_, String>(9)?, "last_error": r.get::<_, String>(10)?,
        "created_at": r.get::<_, i64>(11)?, "sent_at": r.get::<_, Option<i64>>(12)?,
        "next_attempt_at": r.get::<_, i64>(13)?,
        "locked_by": r.get::<_, Option<String>>(14)?, "locked_until": r.get::<_, Option<i64>>(15)?,
    }))
}

/// The newest outbox rows as JSON, for `/dev/mail`.
pub fn recent(conn: &Connection, limit: usize) -> rusqlite::Result<Vec<serde_json::Value>> {
    let mut stmt = conn.prepare(&format!("SELECT {OUTBOX_COLUMNS} FROM mail_outbox ORDER BY id DESC LIMIT ?1"))?;
    let rows = stmt.query_map([limit as i64], outbox_json)?;
    rows.collect()
}

/// One outbox row as JSON, for `/dev/mail/{id}`.
pub fn outbox_row(conn: &Connection, id: i64) -> rusqlite::Result<Option<serde_json::Value>> {
    conn.query_row(&format!("SELECT {OUTBOX_COLUMNS} FROM mail_outbox WHERE id = ?1"), [id], outbox_json).optional()
}

/// Delete one outbox row (the dev outbox's Delete); whether it was there.
pub fn delete_row(conn: &Connection, id: i64) -> rusqlite::Result<bool> {
    Ok(conn.execute("DELETE FROM mail_outbox WHERE id = ?1", [id])? > 0)
}

/// Delete sent and failed rows older than [`KEEP_DAYS`], and the rows a
/// deleted account left for [`PER_ADDRESS`] once their window is over
/// (hourly housekeeping).
pub fn purge(conn: &Connection, now: i64) -> rusqlite::Result<usize> {
    let mut n = conn.execute(PURGE_OLD, [now - KEEP_DAYS * 86_400])?;
    for cap in PER_ADDRESS {
        n += conn.execute(PURGE_KEPT, params![cap.purpose, now - cap.window])?;
    }
    Ok(n)
}

/// [`purge`]'s statements. The range on `created_at` comes first so the
/// planner reads only the rows past their days (`idx_mail_outbox_created`,
/// #609). A deleted account's leftovers are found by `user_id IS NULL`
/// (few rows), not by purpose, which every verification row shares.
const PURGE_OLD: &str = "DELETE FROM mail_outbox WHERE created_at < ?1 AND state <> 'queued'";
const PURGE_KEPT: &str =
    "DELETE FROM mail_outbox INDEXED BY idx_mail_outbox_user WHERE user_id IS NULL AND purpose = ?1 AND state <> 'queued' AND created_at <= ?2";

/// Provider error texts can quote the recipient ("550 <ann@x>: no such
/// user"); logs and the outbox name rows by id, not address.
fn scrub(msg: &str) -> String {
    msg.split(' ')
        .map(|w| if w.contains('@') { "<address>" } else { w })
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------- the sender ----------

/// Handle to the outbox, kept in `AppState`: queues mail and wakes the job
/// runner, which sends it ([`Outbox`]).
pub struct Mailer {
    wake: Mutex<Sender<()>>,
    pub providers: Vec<String>,
    /// `SKRONGLE_MAIL_PER_HOUR`; `None`: no ceiling.
    ceiling: Option<Ceiling>,
    /// Mail reaches someone: a provider is configured (or, in dev mode, the
    /// dev mailbox stands in for one). `false`: the chain is `log` alone.
    sends: bool,
}

impl Mailer {
    /// The handle for `outbox`'s chain. `ceiling` is `SKRONGLE_MAIL_PER_HOUR`
    /// ([`enqueue`]). `sends`: see [`Mailer::can_send`]. `wake` wakes the job
    /// runner ([`crate::jobs::Queue::waker`]).
    pub fn new(outbox: &Outbox, ceiling: Option<Ceiling>, sends: bool, wake: Sender<()>) -> Mailer {
        let providers = outbox.chain.names().into_iter().map(str::to_string).collect();
        Mailer { wake: Mutex::new(wake), providers, ceiling, sends }
    }

    /// A mailer whose messages stay queued (router tests).
    #[cfg(test)]
    pub fn idle() -> Mailer {
        Mailer { wake: Mutex::new(mpsc::channel().0), providers: vec![], ceiling: None, sends: true }
    }

    /// [`Mailer::idle`] on a server without a mail provider (#633).
    #[cfg(test)]
    pub fn idle_unconfigured() -> Mailer {
        Mailer { sends: false, ..Mailer::idle() }
    }

    /// Can this server send e-mail? `false` when `SKRONGLE_MAIL_PROVIDERS` is
    /// unset (the chain is `log` alone, outside dev mode's mailbox): every
    /// feature that would mail someone hides itself (#633).
    pub fn can_send(&self) -> bool {
        self.sends
    }

    /// A handle that wakes the sender (the job runner), for a thread that
    /// queues mail with [`enqueue`] itself (the data export, exports.rs).
    pub fn waker(&self) -> Sender<()> {
        self.wake.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// `SKRONGLE_MAIL_PER_HOUR`, for code that queues with [`enqueue`] itself.
    pub fn ceiling(&self) -> Option<Ceiling> {
        self.ceiling
    }

    /// Wake the sender: something was queued.
    pub fn notify(&self) {
        let _ = self.wake.lock().unwrap_or_else(|p| p.into_inner()).send(());
    }

    /// Queue `mail` and wake the sender: the row id, or `Err(Capped)` when
    /// a cap said no (nothing was queued). Over the hourly ceiling it is
    /// queued all the same, for a later hour ([`enqueue`]).
    pub async fn try_queue(&self, db: &Db, mail: Mail) -> Result<Result<i64, Capped>, AppError> {
        let (purpose, ceiling) = (mail.purpose, self.ceiling);
        let id = db.call(move |c| enqueue(c, &mail, crate::db::now(), ceiling)).await?;
        match id {
            Ok(_) => self.notify(),
            Err(Capped::Address) => tracing::warn!("mail ({purpose}) not queued: that address had its share today"),
            Err(Capped::Hourly) => tracing::warn!("mail ({purpose}) not queued: SKRONGLE_MAIL_PER_HOUR reached and it cannot wait"),
        }
        Ok(id)
    }

    /// [`Mailer::try_queue`], with a cap as an error.
    pub async fn queue(&self, db: &Db, mail: Mail) -> Result<i64, AppError> {
        self.try_queue(db, mail).await?.map_err(|c| AppError::Internal(format!("mail not queued: {c:?} cap")))
    }
}

/// What sends the outbox: the provider chain, and whether sent messages keep
/// their text (dev mode, so `/dev/mail` can show them). The job runner
/// drains it ([`Outbox::drain`]).
pub struct Outbox {
    chain: Chain,
    from: Mailbox,
    reply_to: Option<String>,
    keep_bodies: bool,
}

impl Outbox {
    pub fn new(chain: Chain, from: Mailbox, reply_to: Option<Mailbox>, keep_bodies: bool) -> Outbox {
        Outbox { chain, from, reply_to: reply_to.map(|m| m.email.to_string()), keep_bodies }
    }

    /// Outside dev mode, blank the settled rows that still hold text
    /// ([`blank_settled`]); run once at start.
    pub fn blank_at_start(&self, db: &Db) {
        if self.keep_bodies {
            return;
        }
        match db.with(|c| blank_settled(c)) {
            Ok(0) => {}
            Ok(n) => tracing::info!("mail: blanked the text of {n} settled outbox row(s)"),
            Err(e) => tracing::warn!("mail: {e}"),
        }
    }

    /// The job runner's handle on this outbox.
    pub fn drain(self) -> crate::jobs::Drain {
        crate::jobs::Drain::new(
            "mail",
            move |db, worker, now| deliver_one(db, &self.chain, &self.from, self.reply_to.as_deref(), self.keep_bodies, worker, now),
            |db| db.with(|c| next_due(c)),
        )
    }
}

// ---------- configuration ----------

/// What `SKRONGLE_MAIL_*` configures.
pub struct Config {
    pub from: Mailbox,
    /// `SKRONGLE_SUPPORT_EMAIL`: the Reply-To of every message, so a reply
    /// to the no-reply sender reaches support.
    pub reply_to: Option<Mailbox>,
    pub chain: Chain,
    /// No `SKRONGLE_MAIL_PROVIDERS`: the chain is `log` alone.
    pub logging_only: bool,
    /// `SKRONGLE_MAIL_PER_HOUR` as set (0: no ceiling); `None` when unset.
    /// [`Config::per_hour`] picks the default.
    pub per_hour_set: Option<i64>,
    /// `SKRONGLE_MAIL_WINDOW`: the ceiling's window in seconds.
    pub window: i64,
}

/// `SKRONGLE_MAIL_WINDOW`: seconds, at least 1; unset or blank, an hour.
fn parse_window(v: Option<String>) -> Result<i64, String> {
    match v.map(|v| v.trim().to_string()).filter(|v| !v.is_empty()) {
        None => Ok(DEFAULT_WINDOW),
        Some(v) => v.parse::<i64>().ok().filter(|n| *n >= 1).ok_or_else(|| format!("SKRONGLE_MAIL_WINDOW must be a number of seconds (1 or more), got {v:?}")),
    }
}

impl Config {
    pub fn from_env() -> Result<Config, String> {
        let mut config = Config::from_lookup(&|name| crate::env(name))?;
        config.window = parse_window(std::env::var("SKRONGLE_MAIL_WINDOW").ok())?;
        Ok(config)
    }

    /// [`Config::per_hour`] over [`Config::window`], for [`Mailer::start`].
    pub fn ceiling(&self, dev: bool) -> Option<Ceiling> {
        self.per_hour(dev).map(|per_window| Ceiling { per_window, window: self.window })
    }

    /// The hourly ceiling: `SKRONGLE_MAIL_PER_HOUR` (0: none), else
    /// [`DEFAULT_PER_HOUR`], except in dev mode, which has none unless set.
    pub fn per_hour(&self, dev: bool) -> Option<i64> {
        match self.per_hour_set {
            Some(0) => None,
            Some(n) => Some(n),
            None if dev => None,
            None => Some(DEFAULT_PER_HOUR),
        }
    }

    /// `env(name)` looks up `SKRONGLE_<name>`.
    fn from_lookup(env: &dyn Fn(&str) -> Option<String>) -> Result<Config, String> {
        let per_hour_set = match env("MAIL_PER_HOUR").map(|v| v.trim().to_string()).filter(|v| !v.is_empty()) {
            Some(v) => Some(v.parse::<i64>().ok().filter(|n| *n >= 0).ok_or_else(|| format!("SKRONGLE_MAIL_PER_HOUR must be a whole number (0: no ceiling), got {v:?}"))?),
            None => None,
        };
        let names: Vec<String> = env("MAIL_PROVIDERS")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect();
        let from = match env("MAIL_FROM") {
            Some(f) => f.parse::<Mailbox>().map_err(|e| format!("SKRONGLE_MAIL_FROM: not an address like \"Skrongle <noreply@example.org>\" ({e})"))?,
            None if names.is_empty() => "Skrongle <noreply@localhost>".parse().expect("valid mailbox"),
            None => return Err("SKRONGLE_MAIL_PROVIDERS needs SKRONGLE_MAIL_FROM, the sender address".into()),
        };
        let reply_to = match env("SUPPORT_EMAIL").map(|v| v.trim().to_string()).filter(|v| !v.is_empty()) {
            Some(v) => Some(v.parse::<Mailbox>().map_err(|e| format!("SKRONGLE_SUPPORT_EMAIL: not an address like support@example.org ({e})"))?),
            None => None,
        };
        if names.is_empty() {
            return Ok(Config { from, reply_to, chain: Chain::new(vec![Box::new(LogProvider { name: "log".into() })]), logging_only: true, per_hour_set, window: DEFAULT_WINDOW });
        }
        let mut providers: Vec<Box<dyn Provider>> = Vec::new();
        let mut kinds = Vec::new();
        for name in &names {
            if !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                return Err(format!("SKRONGLE_MAIL_PROVIDERS: {name:?} is not a name of letters, digits and _"));
            }
            if names.iter().filter(|n| *n == name).count() > 1 {
                return Err(format!("SKRONGLE_MAIL_PROVIDERS names {name} twice"));
            }
            let var = |key: &str| format!("MAIL_{}_{key}", name.to_ascii_uppercase());
            let get = |key: &str| env(&var(key));
            let kind = get("KIND").unwrap_or_else(|| name.clone());
            providers.push(build_provider(name, &kind, &from, &get).map_err(|e| format!("mail provider {name}: {e}"))?);
            kinds.push(kind);
        }
        if kinds.len() > 1 && kinds.iter().any(|k| k == "log") {
            return Err("SKRONGLE_MAIL_PROVIDERS: log cannot be in a chain; it would mark mail sent without sending it".into());
        }
        Ok(Config { from, reply_to, chain: Chain::new(providers), logging_only: false, per_hour_set, window: DEFAULT_WINDOW })
    }
}

/// Whether an HTTP provider's `API_URL` may carry its key: any `https://`
/// URL, or `http://` only when the parsed host is this machine (`localhost`,
/// `127.0.0.1`, `[::1]`). `http://127.0.0.1.evil.example` and
/// `http://localhost:80@evil.example` are remote hosts and do not count (#393).
fn api_url_is_safe(url: &str) -> bool {
    if url.starts_with("https://") {
        return true;
    }
    let Some(rest) = url.strip_prefix("http://") else { return false };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains('@') {
        return false;
    }
    let (host, port) = if authority.starts_with('[') {
        match authority.find(']') {
            Some(end) => authority.split_at(end + 1),
            None => return false,
        }
    } else {
        authority.find(':').map_or((authority, ""), |i| authority.split_at(i))
    };
    let port_ok = port.is_empty() || port.strip_prefix(':').is_some_and(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
    port_ok && matches!(host.to_ascii_lowercase().as_str(), "localhost" | "127.0.0.1" | "[::1]")
}

/// The provider kinds. `get(key)` reads `SKRONGLE_MAIL_<NAME>_<key>`.
fn build_provider(name: &str, kind: &str, from: &Mailbox, get: &dyn Fn(&str) -> Option<String>) -> Result<Box<dyn Provider>, String> {
    let n = name.to_ascii_uppercase();
    match kind {
        "log" => Ok(Box::new(LogProvider { name: name.to_string() })),
        "smtp" => {
            let host = get("HOST").ok_or(format!("SKRONGLE_MAIL_{n}_HOST is not set"))?;
            let tls = match get("TLS").as_deref() {
                None | Some("starttls") => SmtpTls::StartTls,
                Some("tls") => SmtpTls::Implicit,
                Some("none") if matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1") => SmtpTls::None,
                Some("none") => return Err(format!("SKRONGLE_MAIL_{n}_TLS=none is only for a server on this machine, not {host}")),
                Some(other) => return Err(format!("SKRONGLE_MAIL_{n}_TLS must be starttls, tls or none, not {other:?}")),
            };
            let port = match get("PORT") {
                Some(p) => p.parse().map_err(|_| format!("SKRONGLE_MAIL_{n}_PORT must be a port number, not {p:?}"))?,
                None => tls.default_port(),
            };
            let credentials = match (get("USER"), get("PASS")) {
                (Some(u), Some(p)) => Some(Credentials::new(u, p)),
                (None, None) => None,
                _ => return Err(format!("set both SKRONGLE_MAIL_{n}_USER and SKRONGLE_MAIL_{n}_PASS, or neither")),
            };
            Ok(Box::new(SmtpProvider::new(name, &host, port, tls, credentials, from.email.domain())?))
        }
        "resend" => Ok(Box::new(resend::ResendProvider::from_config(name, get)?)),
        "postmark" => Ok(Box::new(postmark::from_config(name, get)?)),
        "brevo" => Ok(Box::new(brevo::BrevoProvider::from_config(name, get)?)),
        other => Err(format!("unknown kind {other:?} (set SKRONGLE_MAIL_{n}_KIND to smtp, resend, postmark, brevo or log)")),
    }
}

// ---------- providers ----------

/// Logs the message instead of sending it: no provider configured, or dev.
pub struct LogProvider {
    name: String,
}

impl Provider for LogProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn send(&self, _from: &Mailbox, mail: &Outgoing) -> Result<(), SendError> {
        // The purpose only: a subject or body can carry a code or a link (#361).
        tracing::info!("mail #{} ({}) logged, not sent (no SKRONGLE_MAIL_PROVIDERS)", mail.id, mail.purpose);
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmtpTls {
    /// Plain connection upgraded with STARTTLS, which is required (port 587).
    StartTls,
    /// TLS from the first byte (port 465).
    Implicit,
    /// No encryption: a relay on this machine only (e.g. a local test server).
    None,
}

impl SmtpTls {
    fn default_port(self) -> u16 {
        match self {
            SmtpTls::StartTls => 587,
            SmtpTls::Implicit => 465,
            SmtpTls::None => 25,
        }
    }
}

/// Any SMTP submission server: Amazon SES, and the SMTP endpoints of
/// Postmark, Resend, Brevo, Mailgun, …
pub struct SmtpProvider {
    name: String,
    transport: SmtpTransport,
    /// The sender's domain, for the Message-ID.
    domain: String,
}

impl SmtpProvider {
    pub fn new(name: &str, host: &str, port: u16, tls: SmtpTls, credentials: Option<Credentials>, domain: &str) -> Result<SmtpProvider, String> {
        let builder = match tls {
            SmtpTls::StartTls => SmtpTransport::starttls_relay(host).map_err(|e| e.to_string())?,
            SmtpTls::Implicit => SmtpTransport::relay(host).map_err(|e| e.to_string())?,
            SmtpTls::None => SmtpTransport::builder_dangerous(host),
        };
        let mut builder = builder.port(port).timeout(Some(SMTP_TIMEOUT)).hello_name(ClientId::Domain(domain.to_string()));
        if let Some(c) = credentials {
            builder = builder.credentials(c);
        }
        Ok(SmtpProvider { name: name.to_string(), transport: builder.build(), domain: domain.to_string() })
    }

    fn message(&self, from: &Mailbox, mail: &Outgoing) -> Result<Message, SendError> {
        let to: Mailbox = mail.to.parse().map_err(|e| SendError::Rejected(format!("bad recipient ({e})")))?;
        let mut builder = Message::builder().from(from.clone()).to(to);
        if let Some(r) = &mail.reply_to {
            builder = builder.reply_to(r.parse().map_err(|e| SendError::Rejected(format!("bad reply-to ({e})")))?);
        }
        let builder = builder
            .subject(mail.subject.clone())
            .date_now()
            .message_id(Some(format!("<mail-{}.{}@{}>", mail.id, crate::db::now(), self.domain)));
        let built = match &mail.html {
            Some(html) => builder.multipart(MultiPart::alternative_plain_html(mail.text.clone(), html.clone())),
            None => builder.singlepart(SinglePart::plain(mail.text.clone())),
        };
        built.map_err(|e| SendError::Rejected(format!("could not build the message ({e})")))
    }
}

impl Provider for SmtpProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn send(&self, from: &Mailbox, mail: &Outgoing) -> Result<(), SendError> {
        let message = self.message(from, mail)?;
        self.transport.send(&message).map(|_| ()).map_err(|e| {
            // 5xx: this server refuses this message for good. Everything else
            // (no connection, TLS, timeout, 4xx) says nothing about the message.
            if e.is_permanent() {
                SendError::Rejected(e.to_string())
            } else {
                SendError::Unavailable(e.to_string())
            }
        })
    }
}

#[cfg(test)]
mod tests;
