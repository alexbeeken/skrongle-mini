//! Sending text messages (SMS): an outbox in SQLite and a chain of providers
//! tried in order, built like the e-mail stack (`src/mail.rs`) so the vendor
//! can be swapped by configuration alone.
//!
//! - Code that wants to text someone calls [`Texter::send_sms`] with a
//!   purpose, a phone number and the text. It checks the number, applies the
//!   number (and its country, [`Policy::countries`]), applies the
//!   per-account, per-number and server-wide limits ([`LIMITS`],
//!   [`Policy`]) and writes the row to `sms_outbox` before anything touches
//!   the network. The limits count a ledger, `sms_sends`, whose rows outlive
//!   the account (#362) and hold the number only as a SHA-256.
//! - The background job runner (`jobs.rs`) delivers due rows ([`Outbox`]),
//!   leasing each one first so it is never sent twice (#873), through the providers in
//!   `SKRONGLE_SMS_PROVIDERS` order; the first that accepts a message wins. Like
//!   mail: an unreachable provider is tried last for [`COOLDOWN`], a message
//!   nobody took is retried on [`RETRY_DELAYS`], and one every provider
//!   *rejected* fails at once.
//! - Providers know nothing about the outbox or the app: a new one is a
//!   [`Provider`] impl plus a `kind` in [`build_provider`]. The `http` kind
//!   posts a templated body to any URL (Twilio, Vonage, a webhook, …), so most
//!   vendors need no code at all.
//!
//! Without `SKRONGLE_SMS_PROVIDERS` the chain is the `log` provider: messages are
//! logged and marked sent, nothing leaves the server. `docs/self-hosting.md`
//! ("Text messages") is the operator's guide.

pub mod phone;

use std::sync::mpsc::Sender;
#[cfg(test)]
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use rusqlite::{params, Connection, OptionalExtension};

use crate::db::Db;
pub use crate::mail::{SendError, COOLDOWN, KEEP_DAYS, RETRY_DELAYS};

/// HTTP provider timeout, connect to last byte.
const HTTP_TIMEOUT: Duration = Duration::from_secs(20);
/// Longest text accepted (what Twilio takes; ten segments).
pub const MAX_CHARS: usize = 1600;

/// How many texts may be queued in a window. Counted from the `sms_sends`
/// ledger, so they hold across restarts, in dev mode and after the account
/// is deleted (#362); failed texts count too, since a provider may still
/// have charged for them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limit {
    pub max: i64,
    pub window: i64,
    pub per: Per,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Per {
    /// The account the text is about, whatever it is for.
    Account,
    /// The destination number, within the text's [`Kind`].
    Number,
    /// Every text the server sends, within the text's [`Kind`].
    Server,
}

/// Per account (texts with a `user_id`) and per destination number (every
/// text, so nobody can use the server to flood one phone). A number's
/// sign-in codes and its other texts have an allowance each (#363), so
/// verification texts to it cannot lock its owner out of signing in.
pub const LIMITS: &[Limit] = &[
    Limit { max: 5, window: 3600, per: Per::Account },
    Limit { max: 20, window: 86_400, per: Per::Account },
    Limit { max: 5, window: 3600, per: Per::Number },
    Limit { max: 10, window: 86_400, per: Per::Number },
];

/// The allowance a text draws on. Sign-in codes, and the codes that confirm
/// turning the second step off, go to a number already verified on that
/// account; everything else (a number's verification code, a dev test) is
/// `other`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    SignIn,
    Other,
}

impl Kind {
    pub fn of(purpose: &str) -> Kind {
        if purpose == crate::two_factor::PURPOSE_LOGIN || purpose == crate::two_factor::PURPOSE_CONFIRM {
            Kind::SignIn
        } else {
            Kind::Other
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::SignIn => "signin",
            Kind::Other => "other",
        }
    }
}

/// Server-wide rules from configuration (`SKRONGLE_SMS_COUNTRIES`,
/// `SKRONGLE_SMS_MAX_PER_HOUR`, `SKRONGLE_SMS_MAX_PER_DAY`): a cap on what the
/// whole server texts, for each [`Kind`] apart (so a flood of verification
/// texts cannot stop sign-in codes), and the countries it texts at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Policy {
    pub per_hour: i64,
    pub per_day: i64,
    /// Calling-code prefixes (digits after the +) a number must start with;
    /// `None`: any country.
    pub countries: Option<Vec<String>>,
}

/// The countries texted when `SKRONGLE_SMS_COUNTRIES` is unset: North America,
/// the UK and Ireland, western and northern Europe, Australia and New
/// Zealand. Premium-rate fraud mostly targets other ranges.
pub const DEFAULT_COUNTRIES: &str = "1,44,353,33,34,49,39,31,32,41,43,45,46,47,358,351,61,64";
pub const DEFAULT_PER_HOUR: i64 = 100;
pub const DEFAULT_PER_DAY: i64 = 500;

impl Default for Policy {
    fn default() -> Policy {
        Policy { per_hour: DEFAULT_PER_HOUR, per_day: DEFAULT_PER_DAY, countries: parse_countries(DEFAULT_COUNTRIES).unwrap() } // panic-ok: a constant list, parsed in a test
    }
}

impl Policy {
    /// May `number` (E.164) be texted at all?
    pub fn allows(&self, number: &str) -> bool {
        let digits = number.trim_start_matches('+');
        match &self.countries {
            None => true,
            Some(codes) => codes.iter().any(|c| digits.starts_with(c.as_str())),
        }
    }

    fn limits(&self) -> impl Iterator<Item = Limit> + '_ {
        LIMITS.iter().copied().chain([
            Limit { max: self.per_hour, window: 3600, per: Per::Server },
            Limit { max: self.per_day, window: 86_400, per: Per::Server },
        ])
    }
}

/// `SKRONGLE_SMS_COUNTRIES`: calling codes separated by commas (`1,44,+49`),
/// or `*` for any country.
fn parse_countries(s: &str) -> Result<Option<Vec<String>>, String> {
    if s.trim() == "*" {
        return Ok(None);
    }
    let mut codes = Vec::new();
    for c in s.split(',').map(|c| c.trim().trim_start_matches('+')).filter(|c| !c.is_empty()) {
        if !c.bytes().all(|b| b.is_ascii_digit()) || c.len() > 6 || c.starts_with('0') {
            return Err(format!("SKRONGLE_SMS_COUNTRIES: {c:?} is not a calling code (digits, like 44 or 1)"));
        }
        codes.push(c.to_string());
    }
    if codes.is_empty() {
        return Err("SKRONGLE_SMS_COUNTRIES is empty; list calling codes (1,44) or * for any country".into());
    }
    Ok(Some(codes))
}

/// What the ledger stores for a number: its SHA-256, which is all the
/// limits need to match it.
pub fn number_key(number: &str) -> String {
    use sha2::{Digest, Sha256};
    let d = Sha256::digest(format!("sms-limit:{number}").as_bytes());
    d.iter().map(|b| format!("{b:02x}")).collect()
}

// ---------- messages ----------

/// Why [`Texter::send_sms`] did not queue a message.
#[derive(Debug)]
pub enum SmsError {
    /// Not a phone number, an empty or too long text.
    Invalid(String),
    /// Over one of [`LIMITS`]; nothing was queued. `retry_after` in seconds.
    RateLimited { retry_after: i64 },
    /// The number's country is not in `SKRONGLE_SMS_COUNTRIES`.
    Country,
    Db(crate::error::AppError),
}

impl std::fmt::Display for SmsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SmsError::Invalid(m) => f.write_str(m),
            SmsError::RateLimited { retry_after } => write!(f, "too many text messages; try again in {retry_after}s"),
            SmsError::Country => f.write_str("this server does not send texts to that country"),
            SmsError::Db(e) => write!(f, "{e:?}"),
        }
    }
}

/// A text to queue. Built by [`Sms::new`], which checks the number and text.
#[derive(Clone, Debug)]
pub struct Sms {
    /// E.164: `+` and 8 to 15 digits.
    pub to: String,
    pub text: String,
    /// What the message is for (`login-code`, …): logs and the outbox name it.
    pub purpose: &'static str,
    /// The account it is about; limits count per account, and deleting the
    /// account deletes the outbox row (its ledger row stays, `user_id` NULL).
    pub user_id: Option<i64>,
}

impl Sms {
    pub fn new(purpose: &'static str, to: &str, text: &str, user_id: Option<i64>) -> Result<Sms, SmsError> {
        let to = normalize_number(to).map_err(SmsError::Invalid)?;
        let text = text.trim();
        if text.is_empty() {
            return Err(SmsError::Invalid("the text is empty".into()));
        }
        if text.chars().count() > MAX_CHARS {
            return Err(SmsError::Invalid(format!("the text is longer than {MAX_CHARS} characters")));
        }
        Ok(Sms { to, text: text.to_string(), purpose, user_id })
    }
}

/// `+44 7700 900-123` → `+447700900123`. International format only: a number
/// without its country code is ambiguous, and every SMS API wants E.164.
pub fn normalize_number(s: &str) -> Result<String, String> {
    let s = s.trim();
    let Some(rest) = s.strip_prefix('+') else {
        return Err(crate::t("sms.number.needs_plus"));
    };
    let mut digits = String::from("+");
    for c in rest.chars() {
        match c {
            '0'..='9' => digits.push(c),
            ' ' | '-' | '.' | '(' | ')' => {}
            _ => return Err(crate::t1("sms.number.not_a_digit", "char", format!("{c:?}"))),
        }
    }
    match digits.len() - 1 {
        8..=15 if !digits.starts_with("+0") => Ok(digits),
        _ => Err(crate::t("sms.number.bad_length")),
    }
}

/// The one country code a learner's own number may have for now (#496): the
/// app texts US and Canada numbers only. [`normalize_number`] stays
/// international, since a provider's own number (`SMS_FROM`) and the dev
/// tools are not restricted.
pub const US_CODE: &str = "+1";

/// What the profile's phone field takes (#496): the ten digits of a US or
/// Canada number, punctuated any way the learner likes (`(555) 555 - 5555`,
/// `555.555.5555`) and with or without the `+1` in front, as E.164
/// (`+15555555555`). A number of any other length is refused rather than
/// guessed at, so a number from elsewhere does not silently become a wrong
/// `+1` one.
pub fn normalize_us_number(s: &str) -> Result<String, String> {
    let s = s.trim();
    // Written with a country code, it can only be ours: nothing else would
    // reach a phone, and the ten digits of `+44 7700 900123` would otherwise
    // pass as a US number nobody can be texted on.
    if s.starts_with('+') && !s.starts_with(US_CODE) {
        return Err(crate::t("sms.number.us_only"));
    }
    let mut digits = String::new();
    for c in s.chars() {
        match c {
            '0'..='9' => digits.push(c),
            '+' | ' ' | '-' | '.' | '(' | ')' => {}
            _ => return Err(crate::t1("sms.number.not_a_digit", "char", format!("{c:?}"))),
        }
    }
    // The country code, typed or pasted from a full number: no area code
    // starts with 1, so an eleventh leading digit can only be it.
    if let Some(rest) = digits.strip_prefix('1') {
        if rest.len() == 10 {
            digits = rest.to_string();
        }
    }
    if digits.len() != 10 {
        return Err(crate::t("sms.number.us_length"));
    }
    // The area code and the exchange after it: 2-9 first, as the North
    // American plan has it (0 and 1 are the operator and long distance).
    let leads = |i: usize| digits.as_bytes().get(i).is_some_and(|b| (b'2'..=b'9').contains(b));
    if !leads(0) || !leads(3) {
        return Err(crate::t("sms.number.us_area"));
    }
    Ok(format!("{US_CODE}{digits}"))
}

/// `+15555555555` → `(555) 555 - 5555`: how the profile shows the number,
/// matching what the field formats as you type (`static/profile.js`).
/// Anything else (no number, or an international one stored before #496)
/// comes back as it is, so it is still shown and can be replaced.
pub fn format_us_number(e164: &str) -> String {
    let ten = e164.strip_prefix(US_CODE).filter(|d| d.len() == 10 && d.bytes().all(|b| b.is_ascii_digit()));
    let Some((area, exchange, line)) = ten.and_then(|d| Some((d.get(..3)?, d.get(3..6)?, d.get(6..)?))) else {
        return e164.to_string();
    };
    format!("({area}) {exchange} - {line}")
}

/// What a refusal says first when the number opted out of texts (replied
/// STOP, or its carrier did): [`opted_out`] makes one. The outbox then
/// marks the account's number (`users.phone_opted_out_at`, #969), and its
/// sign-in codes go by e-mail until a text to it goes through again.
pub const OPTED_OUT: &str = "opted out: ";

/// A provider's refusal because the number opted out of texts.
pub fn opted_out(why: &str) -> SendError {
    SendError::Rejected(format!("{OPTED_OUT}{why}"))
}

fn is_opted_out(e: &SendError) -> bool {
    matches!(e, SendError::Rejected(m) if m.starts_with(OPTED_OUT))
}

/// What a provider is handed: the message plus the outbox id, which HTTP
/// providers can send as an idempotency key.
#[derive(Clone, Debug)]
pub struct Outgoing {
    pub id: i64,
    pub to: String,
    pub text: String,
    pub purpose: String,
}

/// One way of sending texts. Blocking: it runs on the job runner's thread.
pub trait Provider: Send + Sync {
    /// The name from `SKRONGLE_SMS_PROVIDERS`, stored on the rows it sent.
    fn name(&self) -> &str;
    fn send(&self, sms: &Outgoing) -> Result<(), SendError>;
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

    /// A provider said the number opted out of texts.
    pub fn opted_out(&self) -> bool {
        self.attempts.iter().any(|(_, e)| is_opted_out(e))
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

    /// Providers out of cooldown first (in configured order), then the ones
    /// still cooling down: nobody is skipped.
    fn order(&self, now: Instant) -> Vec<usize> {
        let down = self.down_since.lock().unwrap_or_else(|p| p.into_inner());
        let cooling = |i: usize| down[i].is_some_and(|t| now.duration_since(t) < COOLDOWN);
        let mut order: Vec<usize> = (0..self.providers.len()).filter(|&i| !cooling(i)).collect();
        order.extend((0..self.providers.len()).filter(|&i| cooling(i)));
        order
    }

    /// Send through the first provider that takes the message; its name on success.
    pub fn send(&self, sms: &Outgoing) -> Result<&str, ChainError> {
        self.send_at(sms, Instant::now())
    }

    fn send_at(&self, sms: &Outgoing, now: Instant) -> Result<&str, ChainError> {
        let mut attempts = Vec::new();
        for i in self.order(now) {
            let p = &self.providers[i];
            let result = p.send(sms);
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

/// The first limit `sms` would go over: seconds until a slot frees up.
fn over_limit(conn: &Connection, sms: &Sms, policy: &Policy, now: i64) -> rusqlite::Result<Option<i64>> {
    let kind = Kind::of(sms.purpose).as_str();
    let key = number_key(&sms.to);
    for l in policy.limits() {
        let since = now - l.window;
        let q = |sql: &str, p: &[&dyn rusqlite::ToSql]| conn.query_row(sql, p, |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?)));
        let (n, oldest) = match l.per {
            Per::Account => match sms.user_id {
                Some(u) => q("SELECT COUNT(*), MIN(created_at) FROM sms_sends WHERE user_id = ?1 AND created_at > ?2", &[&u, &since])?,
                None => continue,
            },
            Per::Number => q("SELECT COUNT(*), MIN(created_at) FROM sms_sends WHERE number_key = ?1 AND kind = ?2 AND created_at > ?3", &[&key, &kind, &since])?,
            Per::Server => q("SELECT COUNT(*), MIN(created_at) FROM sms_sends WHERE kind = ?1 AND created_at > ?2", &[&kind, &since])?,
        };
        if n >= l.max {
            return Ok(Some((oldest.unwrap_or(now) + l.window - now).max(1)));
        }
    }
    Ok(None)
}

/// Check the limits and put `sms` in the outbox, due now, and in the
/// ledger: the row id, or `Err(seconds)` when a limit says no. Count and
/// insert run in one `Db::call`, under the connection's lock, so two
/// requests cannot both slip under a limit.
pub fn enqueue(conn: &Connection, sms: &Sms, policy: &Policy, now: i64) -> rusqlite::Result<Result<i64, i64>> {
    if let Some(retry_after) = over_limit(conn, sms, policy, now)? {
        return Ok(Err(retry_after));
    }
    conn.execute(
        "INSERT INTO sms_sends (user_id, number_key, kind, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![sms.user_id, number_key(&sms.to), Kind::of(sms.purpose).as_str(), now],
    )?;
    conn.execute(
        "INSERT INTO sms_outbox (user_id, to_number, body_text, purpose, next_attempt_at, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![sms.user_id, sms.to, sms.text, sms.purpose, now],
    )?;
    Ok(Ok(conn.last_insert_rowid()))
}

/// The columns [`deliver_one`] reads from a leased row.
const LEASED_COLUMNS: &str = "id, to_number, body_text, purpose, attempts, user_id";

/// The next time a queued row is due (or its lease runs out), if any.
pub fn next_due(conn: &Connection) -> rusqlite::Result<Option<i64>> {
    crate::jobs::outbox_next_due(conn, "sms_outbox")
}

/// Lease the next due row for `worker`, send it through the chain and record
/// the outcome, clearing the lease: whether there was a row. A row whose
/// lease is held (being sent by this or another server) is not touched.
pub fn deliver_one(db: &Db, chain: &Chain, keep_bodies: bool, worker: &str, now: i64) -> rusqlite::Result<bool> {
    let leased = db.with(|c| {
        crate::jobs::lease_outbox(c, "sms_outbox", LEASED_COLUMNS, worker, now, |r| {
            Ok((Outgoing { id: r.get(0)?, to: r.get(1)?, text: r.get(2)?, purpose: r.get(3)? }, r.get::<_, i64>(4)?, r.get::<_, Option<i64>>(5)?))
        })
    })?;
    let Some((sms, attempts, user_id)) = leased else { return Ok(false) };
    let attempts = attempts + 1;
    match chain.send(&sms) {
        Ok(provider) => {
            tracing::info!("sms #{} ({}) sent via {provider}", sms.id, sms.purpose);
            // A text got through: the number takes texts again (texted START).
            if let Some(u) = user_id {
                db.with(|c| c.execute("UPDATE users SET phone_opted_out_at = NULL WHERE id = ?1 AND phone = ?2 AND phone_opted_out_at IS NOT NULL", params![u, sms.to]))?;
            }
            // Once sent, the text (a login code, say) is no use to anyone here.
            db.with(|c| {
                c.execute(
                    "UPDATE sms_outbox SET state = 'sent', sent_at = ?2, provider = ?3, attempts = ?4, last_error = '', locked_by = NULL, locked_until = NULL,
                            body_text = CASE WHEN ?5 THEN '' ELSE body_text END
                     WHERE id = ?1",
                    params![sms.id, now, provider, attempts, !keep_bodies],
                )
            })?;
        }
        Err(e) => {
            if let (true, Some(u)) = (e.opted_out(), user_id) {
                tracing::warn!("sms #{} ({}): the number opted out of texts; user {u}'s codes go by e-mail now", sms.id, sms.purpose);
                db.with(|c| c.execute("UPDATE users SET phone_opted_out_at = ?3 WHERE id = ?1 AND phone = ?2 AND phone_opted_out_at IS NULL", params![u, sms.to, now]))?;
            }
            let why = scrub(&e.summary());
            let retry = RETRY_DELAYS.get(attempts as usize - 1).filter(|_| !e.all_rejected());
            match retry {
                Some(delay) => tracing::warn!("sms #{} ({}): no provider took it ({why}); retrying in {delay}s", sms.id, sms.purpose),
                None => tracing::error!("sms #{} ({}) failed after {attempts} attempt(s): {why}", sms.id, sms.purpose),
            }
            db.with(|c| match retry {
                Some(delay) => c.execute(
                    "UPDATE sms_outbox SET attempts = ?2, next_attempt_at = ?3, last_error = ?4, locked_by = NULL, locked_until = NULL WHERE id = ?1",
                    params![sms.id, attempts, now + delay, why],
                ),
                // Never sent and never retried: its code is no use to anyone either (#361).
                None => c.execute(
                    "UPDATE sms_outbox SET state = 'failed', attempts = ?2, last_error = ?3, locked_by = NULL, locked_until = NULL,
                            body_text = CASE WHEN ?4 THEN '' ELSE body_text END
                     WHERE id = ?1",
                    params![sms.id, attempts, why, !keep_bodies],
                ),
            })?;
        }
    }
    Ok(true)
}

const OUTBOX_COLUMNS: &str = "id, user_id, to_number, body_text, purpose, state, attempts, provider, last_error, created_at, sent_at, locked_by, locked_until";

fn outbox_json(r: &rusqlite::Row) -> rusqlite::Result<serde_json::Value> {
    Ok(serde_json::json!({
        "id": r.get::<_, i64>(0)?, "user_id": r.get::<_, Option<i64>>(1)?, "to": r.get::<_, String>(2)?,
        "text": r.get::<_, String>(3)?, "purpose": r.get::<_, String>(4)?, "state": r.get::<_, String>(5)?,
        "attempts": r.get::<_, i64>(6)?, "provider": r.get::<_, String>(7)?, "last_error": r.get::<_, String>(8)?,
        "created_at": r.get::<_, i64>(9)?, "sent_at": r.get::<_, Option<i64>>(10)?,
        "locked_by": r.get::<_, Option<String>>(11)?, "locked_until": r.get::<_, Option<i64>>(12)?,
    }))
}

/// The newest outbox rows as JSON, for `/dev/sms`.
pub fn recent(conn: &Connection, limit: usize) -> rusqlite::Result<Vec<serde_json::Value>> {
    let mut stmt = conn.prepare(&format!("SELECT {OUTBOX_COLUMNS} FROM sms_outbox ORDER BY id DESC LIMIT ?1"))?;
    let rows = stmt.query_map([limit as i64], outbox_json)?;
    rows.collect()
}

/// Every outbox row to `to`, oldest first: the conversation on `/dev/sms/{id}`.
pub fn thread(conn: &Connection, to: &str) -> rusqlite::Result<Vec<serde_json::Value>> {
    let mut stmt = conn.prepare(&format!("SELECT {OUTBOX_COLUMNS} FROM sms_outbox WHERE to_number = ?1 ORDER BY id"))?;
    let rows = stmt.query_map([to], outbox_json)?;
    rows.collect()
}

/// One outbox row as JSON, for `/dev/sms/{id}`.
pub fn outbox_row(conn: &Connection, id: i64) -> rusqlite::Result<Option<serde_json::Value>> {
    conn.query_row(&format!("SELECT {OUTBOX_COLUMNS} FROM sms_outbox WHERE id = ?1"), [id], outbox_json).optional()
}

/// Delete every outbox row to `to` (the dev text outbox's Delete on a
/// conversation); the ids that went. Its ledger rows go too, which gives
/// that number its quota back.
pub fn delete_thread(conn: &Connection, to: &str) -> rusqlite::Result<Vec<i64>> {
    conn.execute("DELETE FROM sms_sends WHERE number_key = ?1", [number_key(to)])?;
    let mut stmt = conn.prepare("DELETE FROM sms_outbox WHERE to_number = ?1 RETURNING id")?;
    let ids = stmt.query_map([to], |r| r.get(0))?;
    ids.collect()
}

/// Delete sent and failed rows older than [`KEEP_DAYS`], and ledger rows
/// older than two days (hourly housekeeping). The limits look back one day
/// at most, so this never loosens them.
pub fn purge(conn: &Connection, now: i64) -> rusqlite::Result<usize> {
    conn.execute("DELETE FROM sms_sends WHERE created_at < ?1", [now - 2 * 86_400])?;
    conn.execute(PURGE_OLD, [now - KEEP_DAYS * 86_400])
}

/// [`purge`]'s outbox statement: the `created_at` range first, so only the
/// rows past their days are read (`idx_sms_outbox_created`, #611).
const PURGE_OLD: &str = "DELETE FROM sms_outbox WHERE created_at < ?1 AND state <> 'queued'";

/// Provider errors can quote the recipient ("+447700900123 is not a mobile
/// number"); logs and the outbox name rows by id. Any run of 7 or more digits
/// (with an optional +) becomes `<number>`.
fn scrub(msg: &str) -> String {
    let mut out = String::with_capacity(msg.len());
    let mut run = String::new();
    let flush = |run: &mut String, out: &mut String| {
        if run.chars().filter(char::is_ascii_digit).count() >= 7 {
            out.push_str("<number>");
        } else {
            out.push_str(run);
        }
        run.clear();
    };
    for c in msg.chars() {
        if c.is_ascii_digit() || (c == '+' && run.is_empty()) {
            run.push(c);
        } else {
            flush(&mut run, &mut out);
            out.push(c);
        }
    }
    flush(&mut run, &mut out);
    out
}

/// The last two digits only, for log lines.
fn masked(number: &str) -> String {
    let tail: String = number.chars().rev().take(2).collect::<Vec<_>>().into_iter().rev().collect();
    format!("…{tail}")
}

// ---------- the sender ----------

/// Handle to the outbox, kept in `AppState`: checks and queues texts and
/// wakes the job runner, which sends them ([`Outbox`]).
pub struct Texter {
    wake: Mutex<Sender<()>>,
    pub providers: Vec<String>,
    /// The countries and server-wide caps (`SKRONGLE_SMS_*`).
    pub policy: Policy,
    /// Texts reach someone: a provider is configured (or, in dev mode, the
    /// dev phone stands in for one). `false`: the chain is `log` alone.
    sends: bool,
}

impl Texter {
    /// The handle for `outbox`'s chain. `sends`: see [`Texter::can_send`].
    /// `wake` wakes the job runner ([`crate::jobs::Queue::waker`]).
    pub fn new(outbox: &Outbox, policy: Policy, sends: bool, wake: Sender<()>) -> Texter {
        let providers = outbox.chain.names().into_iter().map(str::to_string).collect();
        Texter { wake: Mutex::new(wake), providers, policy, sends }
    }

    /// A texter whose messages stay queued (router tests).
    #[cfg(test)]
    pub fn idle() -> Texter {
        Texter { wake: Mutex::new(mpsc::channel().0), providers: vec![], policy: Policy::default(), sends: true }
    }

    /// [`Texter::idle`] on a server without a text message provider (#633).
    #[cfg(test)]
    pub fn idle_unconfigured() -> Texter {
        Texter { sends: false, ..Texter::idle() }
    }

    /// Can this server send text messages? `false` when
    /// `SKRONGLE_SMS_PROVIDERS` is unset (the chain is `log` alone, outside dev
    /// mode's phone): every feature that would text someone hides itself (#633).
    pub fn can_send(&self) -> bool {
        self.sends
    }

    /// Queue a text to `to` (international format, `+44 7700 900123`) and
    /// wake the sender (the job runner): the outbox row id. `user_id` is the account it is
    /// about (`None`: about no account; only the per-number limit applies).
    pub async fn send_sms(&self, db: &Db, purpose: &'static str, to: &str, text: &str, user_id: Option<i64>) -> Result<i64, SmsError> {
        let sms = Sms::new(purpose, to, text, user_id)?;
        if !self.policy.allows(&sms.to) {
            tracing::warn!("sms ({purpose}) to {} refused: country not in SKRONGLE_SMS_COUNTRIES", masked(&sms.to));
            return Err(SmsError::Country);
        }
        let policy = self.policy.clone();
        let queued = db.call(move |c| enqueue(c, &sms, &policy, crate::db::now())).await.map_err(SmsError::Db)?;
        match queued {
            Ok(id) => {
                let _ = self.wake.lock().unwrap_or_else(|p| p.into_inner()).send(());
                Ok(id)
            }
            Err(retry_after) => {
                tracing::warn!("sms ({purpose}) to {} refused: over the limit, retry in {retry_after}s", masked(to.trim()));
                Err(SmsError::RateLimited { retry_after })
            }
        }
    }
}

/// Blank the text of every settled (sent or failed) row that still has one:
/// rows from before #361, or settled while texts were kept. Run at start
/// ([`Outbox::blank_at_start`]), except in dev mode, whose outbox page shows
/// texts.
pub fn blank_settled(conn: &Connection) -> rusqlite::Result<usize> {
    conn.execute(BLANK_SETTLED, [])
}

/// [`blank_settled`]'s statement. `body_text <> ''` is the WHERE of the
/// partial index `idx_sms_outbox_unblanked` (db.rs v58), which holds only
/// the rows still carrying a text, so the sweep does not read the whole
/// outbox (#612); `state IN` lets it search that index.
const BLANK_SETTLED: &str = "UPDATE sms_outbox SET body_text = '' WHERE body_text <> '' AND state IN ('sent', 'failed')";

/// What sends the outbox: the provider chain, and whether sent texts keep
/// their text (dev mode, so `/dev/sms` can show them). The job runner drains
/// it ([`Outbox::drain`]).
pub struct Outbox {
    chain: Chain,
    keep_bodies: bool,
}

impl Outbox {
    pub fn new(chain: Chain, keep_bodies: bool) -> Outbox {
        Outbox { chain, keep_bodies }
    }

    /// Outside dev mode, blank the settled rows that still hold text
    /// ([`blank_settled`]); run once at start.
    pub fn blank_at_start(&self, db: &Db) {
        if self.keep_bodies {
            return;
        }
        match db.with(|c| blank_settled(c)) {
            Ok(0) => {}
            Ok(n) => tracing::info!("sms: blanked the text of {n} settled outbox row(s)"),
            Err(e) => tracing::warn!("sms: {e}"),
        }
    }

    /// The job runner's handle on this outbox.
    pub fn drain(self) -> crate::jobs::Drain {
        crate::jobs::Drain::new("sms", move |db, worker, now| deliver_one(db, &self.chain, self.keep_bodies, worker, now), |db| db.with(|c| next_due(c)))
    }
}

// ---------- configuration ----------

/// What `SKRONGLE_SMS_*` configures.
pub struct Config {
    pub chain: Chain,
    pub policy: Policy,
    /// No `SKRONGLE_SMS_PROVIDERS`: the chain is `log` alone.
    pub logging_only: bool,
}

impl Config {
    pub fn from_env() -> Result<Config, String> {
        Config::from_lookup(&|name| crate::env(name))
    }

    /// `env(name)` looks up `SKRONGLE_<name>`.
    fn from_lookup(env: &dyn Fn(&str) -> Option<String>) -> Result<Config, String> {
        let mut policy = Policy::default();
        if let Some(c) = env("SMS_COUNTRIES") {
            policy.countries = parse_countries(&c)?;
        }
        for (var, slot) in [("SMS_MAX_PER_HOUR", &mut policy.per_hour), ("SMS_MAX_PER_DAY", &mut policy.per_day)] {
            if let Some(v) = env(var) {
                *slot = v.trim().parse::<i64>().ok().filter(|n| *n >= 0).ok_or(format!("SKRONGLE_{var}: {v:?} is not a whole number"))?;
            }
        }
        let names: Vec<String> = env("SMS_PROVIDERS")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect();
        if names.is_empty() {
            return Ok(Config { chain: Chain::new(vec![Box::new(LogProvider { name: "log".into() })]), policy, logging_only: true });
        }
        let from = env("SMS_FROM").unwrap_or_default();
        let mut providers: Vec<Box<dyn Provider>> = Vec::new();
        let mut kinds = Vec::new();
        for name in &names {
            if !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                return Err(format!("SKRONGLE_SMS_PROVIDERS: {name:?} is not a name of letters, digits and _"));
            }
            if names.iter().filter(|n| *n == name).count() > 1 {
                return Err(format!("SKRONGLE_SMS_PROVIDERS names {name} twice"));
            }
            let var = |key: &str| format!("SMS_{}_{key}", name.to_ascii_uppercase());
            let get = |key: &str| env(&var(key));
            let kind = get("KIND").unwrap_or_else(|| name.clone());
            providers.push(build_provider(name, &kind, &from, &get).map_err(|e| format!("SMS provider {name}: {e}"))?);
            kinds.push(kind);
        }
        if kinds.len() > 1 && kinds.iter().any(|k| k == "log") {
            return Err("SKRONGLE_SMS_PROVIDERS: log cannot be in a chain; it would mark texts sent without sending them".into());
        }
        Ok(Config { chain: Chain::new(providers), policy, logging_only: false })
    }
}

/// The provider kinds. `get(key)` reads `SKRONGLE_SMS_<NAME>_<key>`; `from` is
/// `SKRONGLE_SMS_FROM`, which `_FROM` overrides.
fn build_provider(name: &str, kind: &str, from: &str, get: &dyn Fn(&str) -> Option<String>) -> Result<Box<dyn Provider>, String> {
    let n = name.to_ascii_uppercase();
    match kind {
        "log" => Ok(Box::new(LogProvider { name: name.to_string() })),
        "http" => {
            let url = get("URL").ok_or(format!("SKRONGLE_SMS_{n}_URL is not set"))?;
            let uri: http::Uri = url.parse().map_err(|e| format!("SKRONGLE_SMS_{n}_URL is not a URL ({e})"))?;
            let host = uri.host().unwrap_or_default();
            match uri.scheme_str() {
                Some("https") => {}
                Some("http") if matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "::1") => {}
                Some("http") => return Err(format!("SKRONGLE_SMS_{n}_URL must be https:// (http:// only for a server on this machine, not {host})")),
                _ => return Err(format!("SKRONGLE_SMS_{n}_URL must start with https://")),
            }
            let format = match get("FORMAT").as_deref() {
                None | Some("json") => BodyFormat::Json,
                Some("form") => BodyFormat::Form,
                Some(other) => return Err(format!("SKRONGLE_SMS_{n}_FORMAT must be json or form, not {other:?}")),
            };
            let template = get("BODY").unwrap_or_else(|| format.default_template().to_string());
            for needed in ["{to}", "{text}"] {
                if !template.contains(needed) {
                    return Err(format!("SKRONGLE_SMS_{n}_BODY has no {needed}"));
                }
            }
            if format == BodyFormat::Json {
                let sample = Outgoing { id: 1, to: "+10000000000".into(), text: "a \"quoted\"\nline".into(), purpose: "check".into() };
                serde_json::from_str::<serde_json::Value>(&render(&template, format, "+10000000000", &sample))
                    .map_err(|e| format!("SKRONGLE_SMS_{n}_BODY is not JSON once filled in ({e}); placeholders go inside quotes: \"{{to}}\""))?;
            }
            let mut headers = Vec::new();
            match (get("AUTH"), get("USER"), get("PASS")) {
                (Some(auth), None, None) => headers.push((get("AUTH_HEADER").unwrap_or_else(|| "Authorization".into()), auth)),
                (None, Some(u), Some(p)) => headers.push(("Authorization".into(), basic_auth(&u, &p))),
                (None, None, None) => {}
                (Some(_), _, _) => return Err(format!("set SKRONGLE_SMS_{n}_AUTH or SKRONGLE_SMS_{n}_USER and _PASS, not both")),
                _ => return Err(format!("set both SKRONGLE_SMS_{n}_USER and SKRONGLE_SMS_{n}_PASS, or neither")),
            }
            for (h, v) in &headers {
                http::HeaderName::try_from(h.as_str()).map_err(|_| format!("SKRONGLE_SMS_{n}_AUTH_HEADER: {h:?} is not a header name"))?;
                http::HeaderValue::try_from(v.as_str()).map_err(|_| format!("SKRONGLE_SMS_{n}_AUTH is not a valid header value"))?;
            }
            let from = get("FROM").unwrap_or_else(|| from.to_string());
            let mut provider = HttpProvider::new(name, &url, &from, format, &template, headers);
            if let Some(m) = get("OPT_OUT") {
                provider.opt_out = m.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect();
            }
            Ok(Box::new(provider))
        }
        other => Err(format!("unknown kind {other:?} (set SKRONGLE_SMS_{n}_KIND to http or log)")),
    }
}

/// `Basic base64(user:pass)`, as Twilio and most SMS APIs take their keys.
fn basic_auth(user: &str, pass: &str) -> String {
    let b64url = crate::oauth::b64url_encode(format!("{user}:{pass}").as_bytes());
    let mut b64: String = b64url.chars().map(|c| match c {
        '-' => '+',
        '_' => '/',
        c => c,
    }).collect();
    while b64.len() % 4 != 0 {
        b64.push('=');
    }
    format!("Basic {b64}")
}

// ---------- providers ----------

/// Logs the message instead of sending it: no provider configured, or dev.
/// The text stays in the outbox in dev mode, for `/dev/sms`.
pub struct LogProvider {
    name: String,
}

impl Provider for LogProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn send(&self, sms: &Outgoing) -> Result<(), SendError> {
        tracing::info!("sms #{} ({}) to {} logged, not sent: {} characters (no SKRONGLE_SMS_PROVIDERS)", sms.id, sms.purpose, masked(&sms.to), sms.text.chars().count());
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyFormat {
    /// `application/json`; placeholders are filled in JSON-escaped (inside quotes).
    Json,
    /// `application/x-www-form-urlencoded`; placeholders are percent-encoded.
    Form,
}

impl BodyFormat {
    fn default_template(self) -> &'static str {
        match self {
            BodyFormat::Json => r#"{"to":"{to}","from":"{from}","text":"{text}","id":"{id}","purpose":"{purpose}"}"#,
            // Twilio's Messages API field names.
            BodyFormat::Form => "To={to}&From={from}&Body={text}",
        }
    }

    fn content_type(self) -> &'static str {
        match self {
            BodyFormat::Json => "application/json",
            BodyFormat::Form => "application/x-www-form-urlencoded",
        }
    }
}

/// Fill `{to}`, `{from}`, `{text}`, `{id}` and `{purpose}` in one pass, so a
/// text that itself says `{to}` is sent as written. Anything else in braces
/// stays as it is (JSON objects).
fn render(template: &str, format: BodyFormat, from: &str, sms: &Outgoing) -> String {
    let id = sms.id.to_string();
    let fields: [(&str, &str); 5] = [("{to}", &sms.to), ("{from}", from), ("{text}", &sms.text), ("{id}", &id), ("{purpose}", &sms.purpose)];
    let escape = |v: &str| match format {
        BodyFormat::Json => {
            let quoted = serde_json::to_string(v).unwrap_or_default();
            quoted[1..quoted.len() - 1].to_string()
        }
        BodyFormat::Form => crate::oauth::urlencode(v),
    };
    let mut out = String::with_capacity(template.len() + sms.text.len());
    let mut rest = template;
    'scan: while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        for (key, value) in fields {
            if let Some(after) = rest.strip_prefix(key) {
                out.push_str(&escape(value));
                rest = after;
                continue 'scan;
            }
        }
        out.push('{');
        rest = &rest[1..];
    }
    out.push_str(rest);
    out
}

/// Any SMS API that takes one HTTP POST per message: Twilio, Vonage,
/// MessageBird, Plivo, an own webhook, … configured by URL, auth header and
/// a body template.
pub struct HttpProvider {
    name: String,
    url: String,
    from: String,
    format: BodyFormat,
    template: String,
    headers: Vec<(String, String)>,
    /// A refusal whose body contains one of these means the number opted
    /// out of texts (`SKRONGLE_SMS_<NAME>_OPT_OUT`, comma-separated). Twilio's
    /// error 21610 ("attempt to send to unsubscribed recipient") by default.
    pub opt_out: Vec<String>,
    agent: ureq::Agent,
}

impl HttpProvider {
    pub fn new(name: &str, url: &str, from: &str, format: BodyFormat, template: &str, headers: Vec<(String, String)>) -> HttpProvider {
        let agent = ureq::Agent::config_builder().timeout_global(Some(HTTP_TIMEOUT)).http_status_as_error(false).max_redirects(0).build().into();
        HttpProvider { name: name.into(), url: url.into(), from: from.into(), format, template: template.into(), headers, opt_out: vec!["21610".into()], agent }
    }
}

impl Provider for HttpProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn send(&self, sms: &Outgoing) -> Result<(), SendError> {
        let body = render(&self.template, self.format, &self.from, sms);
        let mut req = self.agent.post(&self.url).header("Content-Type", self.format.content_type()).header("Idempotency-Key", &format!("sms-{}", sms.id));
        for (h, v) in &self.headers {
            req = req.header(h.as_str(), v.as_str());
        }
        let mut resp = req.send(body.as_str()).map_err(|e| SendError::Unavailable(e.to_string()))?;
        let status = resp.status().as_u16();
        if (200..300).contains(&status) {
            return Ok(());
        }
        let text = resp.body_mut().with_config().limit(4096).read_to_string().unwrap_or_default();
        let snippet: String = text.chars().take(200).collect();
        let msg = format!("HTTP {status}: {}", snippet.trim());
        // 5xx, 408 and 429 say nothing about this message: try again later. Any
        // other answer (400 bad number, 401 bad key, …) refuses it.
        if status >= 500 || status == 408 || status == 429 {
            Err(SendError::Unavailable(msg))
        } else if self.opt_out.iter().any(|m| text.contains(m.as_str())) {
            Err(opted_out(&msg))
        } else {
            Err(SendError::Rejected(msg))
        }
    }
}

#[cfg(test)]
mod tests;
