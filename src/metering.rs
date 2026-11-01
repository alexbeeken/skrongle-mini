//! Hosted AI and its meter: a server-side AI key the learner does not need
//! to bring, a row in `ai_usage` for every model call, and the limits that
//! keep the server's bill bounded.
//!
//! - **Off by default.** Without `SKRONGLE_AI_KEY` nothing is hosted: every AI
//!   feature runs on the learner's own key exactly as before (calls are
//!   still recorded, at cost 0, and never limited).
//! - **The key** is read once at boot, removed from the process environment
//!   and kept in memory sealed by the vault (`secrets::Vault`), opened only
//!   to build a provider for one call. It never reaches a template or a log.
//! - **The model** per purpose comes from config (`SKRONGLE_AI_MODEL_TUTOR`,
//!   `SKRONGLE_AI_MODEL_COURSES`); **the cost** of a call from a price table in
//!   config (dollars per million tokens), stored in micro-dollars.
//! - **No allowance.** Every account may use the server's key; only the
//!   guards below limit it. Every hosted call is checked *before* it is
//!   made; a reply is never cut off.
//! - **Jobs** (course generation and revision) reserve their estimated cost
//!   up front (`reserve`), settle each call as it lands (`record`) and give
//!   back what they did not use when they end (`release`). Reservations live
//!   in memory, like the jobs themselves.
//! - **Guards.** A global daily spend cap (hosted calls stop for everyone
//!   until midnight UTC, with an error in the log) and a per-account rate
//!   limit (calls per minute).

use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Mutex;

use crate::ai::{self, ProviderKind, Usage};
use crate::db::{now, Db};
use crate::secrets::Vault;

/// One micro-dollar is a millionth of a dollar; every amount is stored so.
pub const MICROS_PER_DOLLAR: i64 = 1_000_000;

/// Whose key paid for a call.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum KeySource {
    /// The server's key: metered and limited.
    Hosted,
    /// The learner's own key: recorded at cost 0, never limited.
    #[default]
    Own,
}

impl KeySource {
    pub fn as_str(self) -> &'static str {
        match self {
            KeySource::Hosted => "hosted",
            KeySource::Own => "own",
        }
    }
}

// ---------- configuration ----------

/// Prices in micro-dollars per million tokens (so `$2.00 / MTok` is 2_000_000).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Price {
    pub input: i64,
    pub output: i64,
    pub cached: i64,
}

/// Anthropic's list prices, checked 2026-10-19 (dollars per million tokens:
/// input, output, cache read). `*` prices any model not
/// listed at the dearest rate (Claude Fable 5.1), so an unknown model is
/// never cheap. `SKRONGLE_AI_PRICES` adds or overrides entries.
pub const DEFAULT_PRICES: &str = "claude-haiku-5-5=0.10/0.50/0.01,claude-sonnet-5-5=2/10/0.10,claude-opus-5-5=4/20/0.20,*=10/50/1";
/// The model Skrongle AI tutors with: the cheapest current one (a turn costs
/// well under a tenth of a cent).
pub const DEFAULT_TUTOR_MODEL: &str = "claude-haiku-5-5";
/// The model Skrongle AI writes and revises courses with.
pub const DEFAULT_COURSES_MODEL: &str = "claude-sonnet-5-5";
/// Skrongle AI spend across all learners per UTC day, in dollars: the
/// operator's backstop against a bug or abuse, not a learner limit.
pub const DEFAULT_DAILY_CAP: &str = "10.00";
/// Skrongle AI calls one account may start per minute.
pub const DEFAULT_RATE_PER_MINUTE: u32 = 12;

#[derive(Clone, Debug)]
pub struct PriceTable {
    prices: HashMap<String, Price>,
}

impl PriceTable {
    /// `model=in/out/cached,…` in dollars per million tokens; `cached`
    /// may be left out (then it is the input price).
    pub fn parse(spec: &str) -> Result<PriceTable, String> {
        let mut t = PriceTable { prices: HashMap::new() };
        t.merge(spec)?;
        Ok(t)
    }

    pub fn merge(&mut self, spec: &str) -> Result<(), String> {
        for entry in spec.split(',').map(str::trim).filter(|e| !e.is_empty()) {
            let (model, nums) = entry.split_once('=').ok_or_else(|| format!("price entry {entry:?} needs model=in/out[/cached]"))?; // panic-ok: a form field name or message, not indexing
            let n: Vec<i64> = nums.split('/').map(|v| dollars_to_micros(v.trim())).collect::<Result<_, _>>()?;
            let price = match n.as_slice() {
                [i, o] => Price { input: *i, output: *o, cached: *i },
                [i, o, c] => Price { input: *i, output: *o, cached: *c },
                _ => return Err(format!("price entry {entry:?} needs model=in/out[/cached]")), // panic-ok: a form field name or message, not indexing
            };
            self.prices.insert(model.trim().to_string(), price);
        }
        Ok(())
    }

    pub fn price(&self, model: &str) -> Price {
        self.prices.get(model).or_else(|| self.prices.get("*")).copied().unwrap_or(Price { input: 0, output: 0, cached: 0 })
    }

    /// `model` has a price of its own, not the `*` rate an unknown model
    /// gets (which is set high on purpose, so too high for an estimate).
    pub fn listed(&self, model: &str) -> bool {
        self.prices.contains_key(model)
    }

    /// The cost of `input` and `output` tokens at `p`, in micro-dollars,
    /// rounded up (an estimate's sum, `Meter::estimate` with a price in hand).
    pub fn cost_at(p: Price, input: u64, output: u64) -> i64 {
        let total = input as i128 * p.input as i128 + output as i128 * p.output as i128;
        ((total + 999_999) / 1_000_000) as i64
    }

    /// The cost of `usage` on `model`, in micro-dollars, rounded up.
    pub fn cost(&self, model: &str, usage: Usage) -> i64 {
        let p = self.price(model);
        let total = usage.input as i128 * p.input as i128 + usage.output as i128 * p.output as i128 + usage.cached as i128 * p.cached as i128;
        ((total + 999_999) / 1_000_000) as i64
    }
}

/// "0.10" / "$3" -> micro-dollars.
pub fn dollars_to_micros(v: &str) -> Result<i64, String> {
    let v = v.trim().trim_start_matches('$');
    let d: f64 = v.parse().map_err(|_| format!("{v:?} is not an amount in dollars"))?;
    if !d.is_finite() || d < 0.0 {
        return Err(format!("{v:?} is not an amount in dollars"));
    }
    Ok((d * MICROS_PER_DOLLAR as f64).round() as i64)
}

/// Cents, plus up to two more digits when they say something: "$0.05",
/// "$0.0496", "$12.30". A tutor turn costs fractions of a cent, so the
/// amount left visibly moves.
pub fn dollars(micros: i64) -> String {
    let sign = if micros < 0 { "-" } else { "" };
    let m = micros.abs();
    let mut s = format!("{:.4}", m as f64 / MICROS_PER_DOLLAR as f64);
    while s.ends_with('0') && s.len() - s.find('.').unwrap_or(0) > 3 {
        s.pop();
    }
    format!("{sign}${s}")
}

#[derive(Clone, Debug)]
pub struct MeterConfig {
    /// `SKRONGLE_AI_PROVIDER`: what kind of API the hosted key opens.
    pub provider: ProviderKind,
    /// `SKRONGLE_AI_BASE_URL`; empty: the provider's default.
    pub base_url: String,
    pub tutor_model: String,
    pub courses_model: String,
    pub prices: PriceTable,
    pub daily_cap: i64,
    pub rate_per_minute: u32,
}

impl Default for MeterConfig {
    fn default() -> MeterConfig {
        MeterConfig {
            provider: ProviderKind::Anthropic,
            base_url: String::new(),
            tutor_model: DEFAULT_TUTOR_MODEL.into(),
            courses_model: DEFAULT_COURSES_MODEL.into(),
            prices: PriceTable::parse(DEFAULT_PRICES).expect("default prices parse"), // panic-ok: the built-in defaults parse (metering tests check them)
            daily_cap: dollars_to_micros(DEFAULT_DAILY_CAP).unwrap(), // panic-ok: the built-in defaults parse (metering tests check them)
            rate_per_minute: DEFAULT_RATE_PER_MINUTE,
        }
    }
}

impl MeterConfig {
    /// The `SKRONGLE_AI_*` variables; a malformed one is an error at boot.
    pub fn from_env() -> Result<MeterConfig, String> {
        let mut c = MeterConfig::default();
        if let Some(p) = crate::env("AI_PROVIDER") {
            c.provider = ProviderKind::parse(&p).ok_or_else(|| format!("SKRONGLE_AI_PROVIDER={p}: expected anthropic, openai or google"))?;
        }
        c.base_url = crate::env("AI_BASE_URL").unwrap_or_default().trim_end_matches('/').to_string();
        if let Some(m) = crate::env("AI_MODEL_TUTOR") {
            c.tutor_model = m;
        }
        if let Some(m) = crate::env("AI_MODEL_COURSES") {
            c.courses_model = m;
        }
        if let Some(p) = crate::env("AI_PRICES") {
            c.prices.merge(&p).map_err(|e| format!("SKRONGLE_AI_PRICES: {e}"))?;
        }
        let money = |name: &str, into: &mut i64| -> Result<(), String> {
            if let Some(v) = crate::env(name) {
                *into = dollars_to_micros(&v).map_err(|e| format!("SKRONGLE_{name}: {e}"))?;
            }
            Ok(())
        };
        money("AI_DAILY_CAP", &mut c.daily_cap)?;
        if let Some(v) = crate::env("AI_RATE_PER_MINUTE") {
            c.rate_per_minute = v.parse().map_err(|_| format!("SKRONGLE_AI_RATE_PER_MINUTE={v}: expected a number"))?;
        }
        Ok(c)
    }

    /// The model hosted calls for `purpose` use.
    pub fn model_for(&self, purpose: &str) -> &str {
        if purpose.starts_with("tutor") {
            &self.tutor_model
        } else {
            &self.courses_model
        }
    }
}

// ---------- calendar ----------

/// Midnight UTC of the day `t` falls in.
pub fn day_start(t: i64) -> i64 {
    t - t.rem_euclid(86_400)
}

// ---------- the meter ----------

/// Why a hosted call (or job) was not allowed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The server's daily spend cap is reached.
    DailyCap,
    /// Too many hosted calls from this account in the last minute.
    RateLimited,
}

impl Refusal {
    /// For the browser (`data-` attributes, the tutor's JSON).
    pub fn kind(&self) -> &'static str {
        match self {
            Refusal::DailyCap => "daily-cap",
            Refusal::RateLimited => "rate",
        }
    }

    /// What the learner reads. The way on (an own key) is a button beside it
    /// (`routes::hosted::OutOfAi`).
    pub fn message(&self) -> String {
        match self {
            Refusal::DailyCap => crate::t1("ai_refused.daily_cap", "hosted", crate::tutor::hosted_label()),
            Refusal::RateLimited => crate::t("ai_refused.rate"),
        }
    }

    /// Why a running job stopped (it keeps what it wrote).
    pub fn job_message(&self) -> String {
        crate::t1("ai_refused.job_daily_cap", "hosted", crate::tutor::hosted_label())
    }
}

/// Why `Meter::call` did not return a reply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallError {
    /// Not made: the day's cap or the rate limit.
    Refused(Refusal),
    /// Made, and it failed (the provider's error, or the database).
    Failed(String),
}

/// One model call to record.
#[derive(Clone, Debug)]
pub struct Record<'a> {
    pub user_id: i64,
    pub call: &'a ai::Call,
    pub provider: &'a str,
    pub model: &'a str,
    pub usage: Usage,
    pub source: KeySource,
}

struct HostedKey {
    sealed: Vec<u8>,
    vault: Vault,
}

struct Reservation {
    remaining: i64,
}

pub struct Meter {
    pub cfg: MeterConfig,
    hosted: Option<HostedKey>,
    reservations: Mutex<HashMap<String, Reservation>>,
    calls: Mutex<HashMap<i64, VecDeque<i64>>>,
    /// The day (its midnight) the cap alert was last logged.
    alerted_day: AtomicI64,
}

impl std::fmt::Debug for Meter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the key, not even sealed.
        f.debug_struct("Meter").field("hosted", &self.hosted()).field("cfg", &self.cfg).finish_non_exhaustive()
    }
}

impl Meter {
    /// `key`: the server's AI key, sealed here at once; `None` turns hosted AI off.
    pub fn new(cfg: MeterConfig, key: Option<String>, vault: &Vault) -> Meter {
        let hosted = key.filter(|k| !k.trim().is_empty()).map(|k| HostedKey { sealed: vault.seal_server_key(k.trim()), vault: vault.clone() });
        Meter {
            cfg,
            hosted,
            reservations: Mutex::new(HashMap::new()),
            calls: Mutex::new(HashMap::new()),
            alerted_day: AtomicI64::new(-1),
        }
    }

    /// No hosted AI: every call is the learner's own.
    pub fn off() -> Meter {
        let (vault, _) = Vault::load(Some("00".repeat(32)), std::path::Path::new("unused.db")).expect("fixed secret"); // panic-ok: a fixed 32-byte hex secret always loads
        Meter::new(MeterConfig::default(), None, &vault)
    }

    pub fn hosted(&self) -> bool {
        self.hosted.is_some()
    }

    /// A provider on the server's key for `purpose`: the same client, wire
    /// format and error handling as a learner's own key; only the key and
    /// `source` differ. `None` without hosted AI.
    pub fn provider(&self, purpose: &str) -> Option<ai::Provider> {
        let h = self.hosted.as_ref()?;
        let key = h.vault.open_server_key(&h.sealed)?;
        let kind = self.cfg.provider;
        Some(ai::Provider {
            kind,
            base_url: if self.cfg.base_url.is_empty() { kind.default_base_url() } else { self.cfg.base_url.clone() },
            api_key: key,
            model: self.cfg.model_for(purpose).to_string(),
            source: KeySource::Hosted,
            effort: None,
        })
    }

    /// The cost of a call of this size on `model`, for checks and reservations.
    pub fn estimate(&self, model: &str, input_tokens: u64, output_tokens: u64) -> i64 {
        self.cfg.prices.cost(model, Usage { input: input_tokens, output: output_tokens, cached: 0 })
    }

    fn lock_res(&self) -> std::sync::MutexGuard<'_, HashMap<String, Reservation>> {
        self.reservations.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn reserved_total(&self) -> i64 {
        self.lock_res().values().map(|r| r.remaining).sum()
    }

    /// What is left of a job's reservation.
    pub fn reservation(&self, job: &str) -> Option<i64> {
        self.lock_res().get(job).map(|r| r.remaining)
    }

    /// Hosted spend since midnight UTC, across everyone: the day's running
    /// total (`ai_spend_days`, moved by triggers on `ai_usage`), one row.
    pub fn spent_today(&self, conn: &Connection) -> rusqlite::Result<i64> {
        Ok(conn.query_row("SELECT cost_micros FROM ai_spend_days WHERE day = ?1", [day_start(now())], |r| r.get(0)).optional()?.unwrap_or(0))
    }

    /// Count a hosted request from this account against the per-minute limit.
    pub fn rate_limit(&self, user_id: i64) -> Result<(), Refusal> {
        self.rate_limit_at(user_id, now())
    }

    fn rate_limit_at(&self, user_id: i64, t: i64) -> Result<(), Refusal> {
        let mut calls = self.calls.lock().unwrap_or_else(|p| p.into_inner());
        calls.retain(|_, q| q.back().is_some_and(|&last| last > t - 60));
        let q = calls.entry(user_id).or_default();
        while q.front().is_some_and(|&first| first <= t - 60) {
            q.pop_front();
        }
        if q.len() >= self.cfg.rate_per_minute as usize {
            return Err(Refusal::RateLimited);
        }
        q.push_back(t);
        Ok(())
    }

    /// May a hosted call of about `estimate` be made now? Inside a job
    /// (`job`), the job's reservation was held up front, so only the day's
    /// cap can stop it; a call outside a job must fit under the cap with
    /// everything reserved.
    pub fn check_call(&self, conn: &Connection, job: Option<&str>, estimate: i64) -> rusqlite::Result<Result<(), Refusal>> {
        let today = self.spent_today(conn)?;
        let held = if job.is_some() { 0 } else { self.reserved_total() + estimate };
        if today + held > self.cfg.daily_cap || (job.is_some() && today >= self.cfg.daily_cap) {
            self.alert(today);
            return Ok(Err(Refusal::DailyCap));
        }
        Ok(Ok(()))
    }

    /// Hold `estimate` for job `job` until it ends (`release`). Refused when
    /// the day's cap would be passed.
    pub fn reserve(&self, conn: &Connection, job: &str, estimate: i64) -> rusqlite::Result<Result<(), Refusal>> {
        match self.check_call(conn, None, estimate)? {
            Ok(()) => {
                self.lock_res().insert(job.to_string(), Reservation { remaining: estimate });
                Ok(Ok(()))
            }
            Err(r) => Ok(Err(r)),
        }
    }

    /// The job ended: give back what it did not spend. Returns that amount.
    pub fn release(&self, job: &str) -> i64 {
        let r = self.lock_res().remove(job).map(|r| r.remaining).unwrap_or(0);
        if r > 0 {
            tracing::debug!("ai meter: job {job} released {}", dollars(r));
        }
        r
    }

    /// Write the `ai_usage` row for a call that was made. Inside a job a
    /// hosted call also draws down the job's reservation. Returns the call's cost.
    pub fn record(&self, conn: &Connection, rec: &Record<'_>) -> rusqlite::Result<i64> {
        let hosted = rec.source == KeySource::Hosted;
        let cost = if hosted { self.cfg.prices.cost(rec.model, rec.usage) } else { 0 };
        let job = (!rec.call.job_id.is_empty()).then_some(rec.call.job_id.as_str());
        conn.execute(
            "INSERT INTO ai_usage (user_id, course_id, job_id, purpose, provider, model, input_tokens, output_tokens, cached_tokens,
                                   cost_micros, credit_micros, key_source, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0, ?11, ?12)",
            params![
                rec.user_id,
                rec.call.course_id,
                rec.call.job_id,
                rec.call.purpose,
                rec.provider,
                rec.model,
                rec.usage.input as i64,
                rec.usage.output as i64,
                rec.usage.cached as i64,
                cost,
                rec.source.as_str(),
                now()
            ],
        )?;
        if let (true, Some(j)) = (hosted, job) {
            if let Some(r) = self.lock_res().get_mut(j) {
                r.remaining = (r.remaining - cost).max(0);
            }
        }
        if hosted {
            let today = self.spent_today(conn)?;
            if today >= self.cfg.daily_cap {
                self.alert(today);
            }
        }
        Ok(cost)
    }

    /// Write the `ai_usage` row of a call the server made for itself on its
    /// own key (the content check's review, #1037): under the course owner
    /// `user_id` so it goes with the account, with its cost. Its purpose
    /// keeps it out of the owner's month (migration 76), so it counts only
    /// in the day's spend. Returns the cost.
    pub fn record_own_cost(&self, conn: &Connection, user_id: i64, call: &ai::Call, model: &str, usage: Usage) -> rusqlite::Result<i64> {
        let cost = self.cfg.prices.cost(model, usage);
        conn.execute(
            "INSERT INTO ai_usage (user_id, course_id, job_id, purpose, provider, model, input_tokens, output_tokens, cached_tokens,
                                   cost_micros, credit_micros, key_source, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0, 'hosted', ?11)",
            params![
                user_id,
                call.course_id,
                call.job_id,
                call.purpose,
                self.cfg.provider.id(),
                model,
                usage.input as i64,
                usage.output as i64,
                usage.cached as i64,
                cost,
                now()
            ],
        )?;
        let today = self.spent_today(conn)?;
        if today >= self.cfg.daily_cap {
            self.alert(today);
        }
        Ok(cost)
    }

    /// Has the server's key reached the day's cap? The server's own calls
    /// wait for the next day then.
    pub fn over_daily_cap(&self, conn: &Connection) -> rusqlite::Result<bool> {
        let today = self.spent_today(conn)?;
        if today >= self.cfg.daily_cap {
            self.alert(today);
            return Ok(true);
        }
        Ok(false)
    }

    /// The one gate every model call goes through, own key or the server's:
    /// with the server's key the day's cap is checked
    /// *before* `send` runs, so a reply is never cut off; every call that
    /// answers gets its `ai_usage` row. `estimate`: the call's cost if it is
    /// not part of a job (inside a job, `call.job_id`, its reservation
    /// covers it). Blocking: the caller is on its own OS thread (a job's, the
    /// tutor's), and the database is reached through `Db::with`.
    pub fn call(&self, db: &Db, user_id: i64, p: &ai::Provider, call: &ai::Call, estimate: i64, send: impl FnOnce() -> Result<ai::Reply, String>) -> Result<ai::Reply, CallError> {
        let hosted = p.source == KeySource::Hosted;
        if hosted {
            let job = (!call.job_id.is_empty()).then_some(call.job_id.as_str());
            db.with(|c| self.check_call(c, job, estimate)).map_err(|e| CallError::Failed(format!("database: {e}")))?.map_err(CallError::Refused)?;
        }
        let reply = send().map_err(|e| match ai::hosted_failure(&e).filter(|_| hosted) {
            // The server's own account (its key, its credit): not the learner's to fix.
            Some(said) => {
                tracing::error!("hosted AI: the provider refused the server's key for {}: {e}", call.purpose);
                CallError::Failed(said.to_string())
            }
            None => CallError::Failed(e),
        })?;
        let rec = Record { user_id, call, provider: p.kind.id(), model: &p.model, usage: reply.usage, source: p.source };
        if let Err(e) = db.with(|c| self.record(c, &rec)) {
            tracing::warn!("ai meter: could not record a {} call: {e}", call.purpose);
        }
        Ok(reply)
    }

    /// Log the cap once per UTC day (the operator's alert).
    fn alert(&self, today: i64) {
        let day = day_start(now());
        if self.alerted_day.swap(day, Ordering::SeqCst) != day {
            tracing::error!(
                "ALERT: hosted AI reached its daily spending cap ({} of {}); hosted calls are refused until midnight UTC (SKRONGLE_AI_DAILY_CAP)",
                dollars(today),
                dollars(self.cfg.daily_cap)
            );
        }
    }
}

#[cfg(test)]
mod tests;
