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
//! - **Allowance.** Each plan has a monthly allowance in dollars of AI cost
//!   (calendar month, UTC). Every hosted call is checked *before* it is made;
//!   a reply is never cut off. Spending order: the allowance, then the
//!   learner's prepaid credits (`users.ai_credits`).
//! - **Jobs** (course generation and revision) reserve their estimated cost
//!   up front (`reserve`), settle each call as it lands (`record`) and give
//!   back what they did not use when they end (`release`). Reservations live
//!   in memory, like the jobs themselves.
//! - **Guards.** A global daily spend cap (hosted calls stop for everyone
//!   until midnight UTC, with an error in the log) and a per-account rate
//!   limit (calls per minute).
//!
//! - **One key, our meter.** Anthropic has no per-end-user key or spend
//!   limit we could create from the app (workspaces are capped at 100 per
//!   organisation, API keys are made in the Console, spend limits are set
//!   there, and usage reports lag by minutes), so the server keeps one key
//!   and meters each learner here. The defaults and the margin are in
//!   docs/pricing.md.
//!
//! TODO(reconcile): once a month, compare the month's hosted `cost_micros`
//! with Anthropic's Cost API (`GET /v1/organizations/cost_report`, grouped by
//! workspace, with an Admin API key) for the server key's workspace; a gap
//! means the price table is out of date (docs/pricing.md, "Reconciling").

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
    /// The server's key: metered, limited, charged to the allowance.
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
/// input, output, cache read; docs/pricing.md). `*` prices any model not
/// listed at the dearest rate (Claude Fable 5.1), so an unknown model is
/// never cheap. `SKRONGLE_AI_PRICES` adds or overrides entries.
pub const DEFAULT_PRICES: &str = "claude-haiku-5-5=0.10/0.50/0.01,claude-sonnet-5-5=2/10/0.10,claude-opus-5-5=4/20/0.20,*=10/50/1";
/// The model Skrongle AI tutors with: the cheapest current one (a turn costs
/// well under a tenth of a cent).
pub const DEFAULT_TUTOR_MODEL: &str = "claude-haiku-5-5";
/// The model Skrongle AI writes and revises courses with.
pub const DEFAULT_COURSES_MODEL: &str = "claude-sonnet-5-5";
/// Monthly Skrongle AI of the Plus plan (CA$15 a month), the only plan with
/// an allowance (#931): $1.50 of list-price
/// cost, so a learner who uses all of it still leaves about $3 after
/// Stripe's fee (docs/pricing.md).
pub const DEFAULT_ALLOWANCE_PLUS: &str = "1.50";
/// Skrongle AI a new account gets once, as credits, when its address is
/// first confirmed (`SKRONGLE_AI_ALLOWANCE_FREE`, #931): none by default.
pub const DEFAULT_SIGNUP_CREDIT: &str = "0";
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

/// A prepaid credit pack sold through Stripe Checkout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreditPack {
    /// The Stripe one-off price (`price_…`) the learner pays.
    pub price_id: String,
    /// What the button says, e.g. "$3".
    pub label: String,
    /// AI cost the pack buys, in micro-dollars (less than the price: the markup).
    pub credit_micros: i64,
}

/// `price_id:credit dollars:label,…` (`SKRONGLE_AI_CREDIT_PACKS`).
pub fn parse_packs(spec: &str) -> Result<Vec<CreditPack>, String> {
    spec.split(',')
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .map(|e| {
            let mut parts = e.splitn(3, ':');
            let (Some(price_id), Some(credit), label) = (parts.next(), parts.next(), parts.next()) else {
                return Err(format!("credit pack {e:?} needs price_id:credit dollars[:label]")); // panic-ok: a form field name or message, not indexing
            };
            let credit_micros = dollars_to_micros(credit)?;
            let label = label.map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).unwrap_or_else(|| dollars(credit_micros));
            Ok(CreditPack { price_id: price_id.trim().to_string(), label, credit_micros })
        })
        .collect()
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
    /// `SKRONGLE_AI_ALLOWANCE_PLUS`: Plus's monthly Skrongle AI. Only Plus has
    /// an allowance (#931); Free and Basic bring their own keys or buy credits.
    pub allowance_plus: i64,
    /// `SKRONGLE_AI_ALLOWANCE_FREE`: credits a new account gets once, when its
    /// address is first confirmed ([`Meter::grant_signup_credit`]). Not monthly.
    pub signup_credit: i64,
    pub daily_cap: i64,
    pub rate_per_minute: u32,
    pub packs: Vec<CreditPack>,
    /// The hosted site (`billing::plans_on`, off in Skrongle mini): true, the plans and their
    /// allowances; false, no plans: every account is Plus, no monthly
    /// allowance applies (`UNMETERED`) and only the daily cap and the rate
    /// limit guard the server's key. Credit packs are not sold.
    pub plans: bool,
}

/// The monthly allowance of every account with plans off: a billion
/// dollars, so the allowance never refuses a call and only the daily cap
/// does (small enough that `AiStatus`'s percentages cannot overflow).
pub const UNMETERED: i64 = 1_000_000_000 * MICROS_PER_DOLLAR;

impl Default for MeterConfig {
    fn default() -> MeterConfig {
        MeterConfig {
            provider: ProviderKind::Anthropic,
            base_url: String::new(),
            tutor_model: DEFAULT_TUTOR_MODEL.into(),
            courses_model: DEFAULT_COURSES_MODEL.into(),
            prices: PriceTable::parse(DEFAULT_PRICES).expect("default prices parse"), // panic-ok: the built-in defaults parse (metering tests check them)
            allowance_plus: dollars_to_micros(DEFAULT_ALLOWANCE_PLUS).unwrap(), // panic-ok: the built-in defaults parse (metering tests check them)
            signup_credit: dollars_to_micros(DEFAULT_SIGNUP_CREDIT).unwrap(), // panic-ok: the built-in defaults parse (metering tests check them)
            daily_cap: dollars_to_micros(DEFAULT_DAILY_CAP).unwrap(), // panic-ok: the built-in defaults parse (metering tests check them)
            rate_per_minute: DEFAULT_RATE_PER_MINUTE,
            packs: Vec::new(),
            plans: true,
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
        money("AI_ALLOWANCE_PRO", &mut c.allowance_plus)?; // the old name, still read
        money("AI_ALLOWANCE_PLUS", &mut c.allowance_plus)?;
        money("AI_ALLOWANCE_FREE", &mut c.signup_credit)?;
        money("AI_DAILY_CAP", &mut c.daily_cap)?;
        if let Some(v) = crate::env("AI_RATE_PER_MINUTE") {
            c.rate_per_minute = v.parse().map_err(|_| format!("SKRONGLE_AI_RATE_PER_MINUTE={v}: expected a number"))?;
        }
        if let Some(v) = crate::env("AI_CREDIT_PACKS") {
            c.packs = parse_packs(&v).map_err(|e| format!("SKRONGLE_AI_CREDIT_PACKS: {e}"))?;
        }
        c.plans = crate::shape::current().has_plans();
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

/// Midnight UTC on the first of the month `t` falls in.
pub fn month_start(t: i64) -> i64 {
    // Howard Hinnant's civil-from-days, then days-from-civil for day 1.
    let z = t.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    // First of the month: same era/year-of-era/month, day 0 of the month.
    let first_doy = (153 * mp + 2) / 5;
    let first_doe = yoe * 365 + yoe / 4 - yoe / 100 + first_doy;
    (era * 146_097 + first_doe - 719_468) * 86_400
}

// ---------- the meter ----------

/// What a learner would use Skrongle AI for, each sized by its smallest
/// request (`Meter::cheapest`): Skrongle AI is offered for a use only when
/// the month's AI could pay for that much (#283).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Need {
    /// One tutor reply.
    Tutor,
    /// The smallest course: its outline and one lesson.
    Course,
    /// One lesson added to a course ("Add content with AI").
    Lesson,
}

/// Input tokens of the smallest tutor turn: the system prompt with a short
/// lesson and no conversation yet.
const TUTOR_PROMPT_TOKENS: u64 = 1_500;

/// Why a hosted call (or job) was not allowed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The allowance and credits do not cover it.
    OutOfAllowance { available: i64, needed: i64 },
    /// The server's daily spend cap is reached.
    DailyCap,
    /// Too many hosted calls from this account in the last minute.
    RateLimited,
}

impl Refusal {
    /// For the browser (`data-` attributes, the tutor's JSON).
    pub fn kind(&self) -> &'static str {
        match self {
            Refusal::OutOfAllowance { .. } => "allowance",
            Refusal::DailyCap => "daily-cap",
            Refusal::RateLimited => "rate",
        }
    }

    /// What the learner reads. `credits`: buying credits is offered. The
    /// ways on (upgrade, a pack, an own key) are buttons beside it
    /// (`routes::hosted::OutOfAi`).
    pub fn message(&self, credits: bool) -> String {
        let buy = if credits { "_credits" } else { "" };
        let hosted = crate::tutor::hosted_label();
        match self {
            Refusal::OutOfAllowance { available, .. } if *available > 0 => crate::t1(&format!("billing.ai.not_enough{buy}"), "hosted", hosted),
            Refusal::OutOfAllowance { .. } => crate::t1(&format!("billing.ai.used_up{buy}"), "hosted", hosted),
            Refusal::DailyCap => crate::t1("billing.ai.daily_cap", "hosted", hosted),
            Refusal::RateLimited => crate::t("billing.ai.rate"),
        }
    }

    /// Why a running job stopped (it keeps what it wrote).
    pub fn job_message(&self, credits: bool) -> String {
        let hosted = crate::tutor::hosted_label();
        match self {
            Refusal::DailyCap => crate::t1("billing.ai.job_daily_cap", "hosted", hosted),
            _ if credits => crate::t1("billing.ai.job_used_up_credits", "hosted", hosted),
            _ => crate::t1("billing.ai.job_used_up", "hosted", hosted),
        }
    }
}

/// This month's AI for one learner.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AiStatus {
    /// The server offers hosted AI.
    pub hosted: bool,
    /// The learner's plan: only Plus has a monthly allowance (#931).
    pub plan: crate::billing::Plan,
    /// The plan's monthly allowance.
    pub allowance: i64,
    /// Hosted cost this month paid from the allowance.
    pub used: i64,
    /// Hosted cost this month paid from credits.
    pub credits_spent: i64,
    /// Held for running jobs.
    pub reserved: i64,
    /// Prepaid credit balance.
    pub credits: i64,
    /// Allowance not yet used (before reservations).
    pub allowance_left: i64,
    /// What the next hosted call may spend: allowance left plus credits, less reservations.
    pub available: i64,
    pub hosted_calls: i64,
    pub own_calls: i64,
    /// No plans on this server (Skrongle mini): no monthly allowance
    /// (`UNMETERED`), so there is no "left this month" to show.
    pub unmetered: bool,
}

impl AiStatus {
    /// Skrongle AI is offered to this learner: the server has its key, and
    /// the plan includes some each month (Plus) or the learner holds credits
    /// (any plan, #931). Free and Basic without credits are never offered it (#283).
    pub fn offered(&self) -> bool {
        self.hosted && (self.allowance > 0 || self.credits > 0)
    }

    /// Skrongle AI is offered, and this month's AI (the allowance plus
    /// credits) could pay for at least `cheapest` (`Meter::cheapest`): a plan
    /// whose allowance is smaller than the smallest request of a use is not
    /// offered it for that use (#283). Running out during the month is
    /// different: the choice stays and the refusal offers the ways on.
    pub fn covers(&self, cheapest: i64) -> bool {
        self.offered() && self.total() >= cheapest
    }

    /// Everything this month could spend: the allowance, plus credits held
    /// and already spent.
    pub fn total(&self) -> i64 {
        self.allowance + self.credits_spent + self.credits
    }

    /// How much of `total` is gone (spent or held by running jobs), 0–100.
    /// Learners see this bar, never dollar amounts.
    pub fn percent_used(&self) -> u8 {
        let total = self.total();
        if total <= 0 {
            return 100;
        }
        let gone = (total - self.available).clamp(0, total);
        // Round up, so a bar is never shown empty once something was spent.
        ((gone * 100 + total - 1) / total).min(100) as u8
    }

    pub fn percent_left(&self) -> u8 {
        100 - self.percent_used()
    }

    /// What `micros` is of this month's total, as whole percent, at least 1.
    pub fn percent_of(&self, micros: i64) -> u8 {
        let total = self.total();
        if total <= 0 {
            return 100;
        }
        ((micros * 100 + total - 1) / total).clamp(1, 100) as u8
    }
}

/// Why `Meter::call` did not return a reply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallError {
    /// Not made: the learner's allowance, the day's cap or the rate limit.
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
    user_id: i64,
    remaining: i64,
}

pub struct Meter {
    pub cfg: MeterConfig,
    hosted: Option<HostedKey>,
    /// Buying credits is possible (packs configured and Stripe on).
    credits_offered: bool,
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
    pub fn new(cfg: MeterConfig, key: Option<String>, vault: &Vault, stripe: bool) -> Meter {
        let hosted = key.filter(|k| !k.trim().is_empty()).map(|k| HostedKey { sealed: vault.seal_server_key(k.trim()), vault: vault.clone() });
        let credits_offered = stripe && !cfg.packs.is_empty() && cfg.plans;
        Meter {
            cfg,
            hosted,
            credits_offered,
            reservations: Mutex::new(HashMap::new()),
            calls: Mutex::new(HashMap::new()),
            alerted_day: AtomicI64::new(-1),
        }
    }

    /// No hosted AI: every call is the learner's own.
    pub fn off() -> Meter {
        let (vault, _) = Vault::load(Some("00".repeat(32)), std::path::Path::new("unused.db")).expect("fixed secret"); // panic-ok: a fixed 32-byte hex secret always loads
        Meter::new(MeterConfig::default(), None, &vault, false)
    }

    pub fn hosted(&self) -> bool {
        self.hosted.is_some()
    }

    pub fn credits_offered(&self) -> bool {
        self.credits_offered
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

    /// The plan's monthly allowance: Plus's, none for Free and Basic;
    /// `UNMETERED` for everyone with plans off.
    pub fn allowance(&self, plan: crate::billing::Plan) -> i64 {
        if !self.cfg.plans {
            UNMETERED
        } else if plan == crate::billing::Plan::Plus {
            self.cfg.allowance_plus
        } else {
            0
        }
    }

    /// The estimate of the smallest request of `need` on the server's key:
    /// what a month's AI must hold for Skrongle AI to be offered for it.
    pub fn cheapest(&self, need: Need) -> i64 {
        let job = |lessons: usize, plan: bool| {
            let (input, output) = ai::job_tokens(lessons, plan, 0);
            self.estimate(&self.cfg.courses_model, input, output)
        };
        match need {
            Need::Tutor => self.estimate(&self.cfg.tutor_model, TUTOR_PROMPT_TOKENS, u64::from(crate::tutor::REPLY_MAX_TOKENS)),
            Need::Course => job(1, true),
            Need::Lesson => job(1, false),
        }
    }

    /// The cost of a call of this size on `model`, for checks and reservations.
    pub fn estimate(&self, model: &str, input_tokens: u64, output_tokens: u64) -> i64 {
        self.cfg.prices.cost(model, Usage { input: input_tokens, output: output_tokens, cached: 0 })
    }

    fn lock_res(&self) -> std::sync::MutexGuard<'_, HashMap<String, Reservation>> {
        self.reservations.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn reserved_by(&self, user_id: i64, except_job: Option<&str>) -> i64 {
        self.lock_res().iter().filter(|(id, r)| r.user_id == user_id && Some(id.as_str()) != except_job).map(|(_, r)| r.remaining).sum()
    }

    fn reserved_total(&self) -> i64 {
        self.lock_res().values().map(|r| r.remaining).sum()
    }

    /// What is left of a job's reservation.
    pub fn reservation(&self, job: &str) -> Option<i64> {
        self.lock_res().get(job).map(|r| r.remaining)
    }

    /// A new account's one-time Skrongle AI (#931): `signup_credit` added to
    /// its credits the first time the meter looks at it with its address
    /// confirmed (a link or a password reset), so every way of
    /// confirming counts. `users.signup_credit_at` records it; accounts made
    /// before v87 have it set already and get nothing. With the setting at 0,
    /// plans off or no server key, nothing is given and nothing is recorded.
    pub fn grant_signup_credit(&self, conn: &Connection, user_id: i64) -> rusqlite::Result<bool> {
        if self.cfg.signup_credit <= 0 || !self.cfg.plans || !self.hosted() {
            return Ok(false);
        }
        let n = conn.execute(
            "UPDATE users SET ai_credits = ai_credits + ?2, signup_credit_at = ?3
             WHERE id = ?1 AND signup_credit_at IS NULL AND email_verified_at IS NOT NULL",
            params![user_id, self.cfg.signup_credit, now()],
        )?;
        if n > 0 {
            tracing::info!("user {user_id} got the sign-up credit ({})", dollars(self.cfg.signup_credit));
        }
        Ok(n > 0)
    }

    pub fn status(&self, conn: &Connection, user_id: i64) -> rusqlite::Result<AiStatus> {
        self.status_except(conn, user_id, None)
    }

    fn status_except(&self, conn: &Connection, user_id: i64, except_job: Option<&str>) -> rusqlite::Result<AiStatus> {
        self.grant_signup_credit(conn, user_id)?;
        let plan = crate::billing::effective_plan(self.cfg.plans, crate::billing::plan_of(conn, user_id)?);
        let since = month_start(now());
        // The month's running totals (`ai_usage_months`, moved by triggers on
        // `ai_usage`): one row, however many calls the month has had.
        let (spent, credits_spent, hosted_calls, own_calls): (i64, i64, i64, i64) = conn
            .query_row(
                "SELECT cost_micros, credit_micros, hosted_calls, own_calls FROM ai_usage_months WHERE user_id = ?1 AND month = ?2",
                params![user_id, since],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?
            .unwrap_or((0, 0, 0, 0));
        // Credits count on every plan: Plus's allowance comes first, and on
        // Free and Basic credits are the only Skrongle AI (#931).
        let credits = credit_balance(conn, user_id)?;
        let allowance = self.allowance(plan);
        let used = spent - credits_spent;
        let allowance_left = (allowance - used).max(0);
        let reserved = self.reserved_by(user_id, except_job);
        Ok(AiStatus {
            hosted: self.hosted(),
            plan,
            allowance,
            used,
            credits_spent,
            reserved,
            credits,
            allowance_left,
            available: (allowance_left + credits - reserved).max(0),
            hosted_calls,
            own_calls,
            unmetered: !self.cfg.plans,
        })
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
    /// (`job`), the job's reservation counts as available to it, and the call
    /// goes ahead while anything is left (its estimate was reserved up front).
    pub fn check_call(&self, conn: &Connection, user_id: i64, job: Option<&str>, estimate: i64) -> rusqlite::Result<Result<(), Refusal>> {
        let today = self.spent_today(conn)?;
        let status = self.status_except(conn, user_id, job)?;
        if job.is_some() {
            // The job's own reservation is left out of `status.reserved`, so
            // `available` already includes what the job holds.
            if today >= self.cfg.daily_cap {
                self.alert(today);
                return Ok(Err(Refusal::DailyCap));
            }
            let available = status.available;
            return Ok(if available > 0 { Ok(()) } else { Err(Refusal::OutOfAllowance { available, needed: estimate }) });
        }
        if today + self.reserved_total() + estimate > self.cfg.daily_cap {
            self.alert(today);
            return Ok(Err(Refusal::DailyCap));
        }
        Ok(if status.available >= estimate { Ok(()) } else { Err(Refusal::OutOfAllowance { available: status.available, needed: estimate }) })
    }

    /// Hold `estimate` for job `job` until it ends (`release`). Refused when
    /// the learner cannot cover it or the day's cap would be passed.
    pub fn reserve(&self, conn: &Connection, user_id: i64, job: &str, estimate: i64) -> rusqlite::Result<Result<(), Refusal>> {
        match self.check_call(conn, user_id, None, estimate)? {
            Ok(()) => {
                self.lock_res().insert(job.to_string(), Reservation { user_id, remaining: estimate });
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

    /// Write the `ai_usage` row for a call that was made. A hosted call is
    /// charged to the allowance first, then to credits; inside a job it also
    /// draws down the job's reservation. Returns the call's cost.
    pub fn record(&self, conn: &Connection, rec: &Record<'_>) -> rusqlite::Result<i64> {
        let hosted = rec.source == KeySource::Hosted;
        let cost = if hosted { self.cfg.prices.cost(rec.model, rec.usage) } else { 0 };
        let job = (!rec.call.job_id.is_empty()).then_some(rec.call.job_id.as_str());
        let mut from_credits = 0;
        if hosted && cost > 0 {
            let status = self.status_except(conn, rec.user_id, job)?;
            let rest = cost - cost.min(status.allowance_left);
            from_credits = rest.min(status.credits);
            if from_credits > 0 {
                conn.execute("UPDATE users SET ai_credits = ai_credits - ?2 WHERE id = ?1", params![rec.user_id, from_credits])?;
            }
        }
        conn.execute(
            "INSERT INTO ai_usage (user_id, course_id, job_id, purpose, provider, model, input_tokens, output_tokens, cached_tokens,
                                   cost_micros, credit_micros, key_source, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
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
                from_credits,
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
    /// `user_id` so it goes with the account, with its cost, never from
    /// credits. Its purpose keeps it out of the owner's month (migration 76),
    /// so it counts only in the day's spend. Returns the cost.
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
    /// with the server's key the allowance (and the day's cap) is checked
    /// *before* `send` runs, so a reply is never cut off; every call that
    /// answers gets its `ai_usage` row. `estimate`: the call's cost if it is
    /// not part of a job (inside a job, `call.job_id`, its reservation
    /// covers it). Blocking: the caller is on its own OS thread (a job's, the
    /// tutor's), and the database is reached through `Db::with`.
    pub fn call(&self, db: &Db, user_id: i64, p: &ai::Provider, call: &ai::Call, estimate: i64, send: impl FnOnce() -> Result<ai::Reply, String>) -> Result<ai::Reply, CallError> {
        let hosted = p.source == KeySource::Hosted;
        if hosted {
            let job = (!call.job_id.is_empty()).then_some(call.job_id.as_str());
            db.with(|c| self.check_call(c, user_id, job, estimate)).map_err(|e| CallError::Failed(format!("database: {e}")))?.map_err(CallError::Refused)?;
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

// ---------- credits ----------

pub fn credit_balance(conn: &Connection, user_id: i64) -> rusqlite::Result<i64> {
    Ok(conn.query_row("SELECT ai_credits FROM users WHERE id = ?1", [user_id], |r| r.get::<_, i64>(0)).optional()?.unwrap_or(0))
}

/// Add (or, negative, take away, never below zero) credits.
pub fn add_credits(conn: &Connection, user_id: i64, micros: i64) -> rusqlite::Result<()> {
    conn.execute("UPDATE users SET ai_credits = MAX(0, ai_credits + ?2) WHERE id = ?1", params![user_id, micros])?;
    Ok(())
}

/// What a credits webhook did.
#[derive(Debug, PartialEq, Eq)]
pub struct CreditChange {
    pub user_id: i64,
    /// Credits added (positive) or taken back by a refund (negative).
    pub micros: i64,
}

/// Apply a verified Stripe event about credit packs: a paid Checkout of a
/// pack adds its credits once (`ai_credit_purchases` keys on the session),
/// a refund takes back the refunded share. `None`: not a credits event, or
/// one already applied.
pub fn apply_credit_event(conn: &Connection, event: &serde_json::Value) -> rusqlite::Result<Option<CreditChange>> {
    let obj = &event["data"]["object"]; // panic-ok: serde_json Value indexing gives Null, never panics
    match event["type"].as_str().unwrap_or("") { // panic-ok: serde_json Value indexing gives Null, never panics
        "checkout.session.completed" | "checkout.session.async_payment_succeeded" => {
            if obj["mode"].as_str() != Some("payment") || obj["metadata"]["kind"].as_str() != Some("ai-credits") || obj["payment_status"].as_str() != Some("paid") { // panic-ok: serde_json Value indexing gives Null, never panics
                return Ok(None);
            }
            let session = obj["id"].as_str().unwrap_or(""); // panic-ok: serde_json Value indexing gives Null, never panics
            // The account is named by its random billing_ref (never the row
            // id, which a later account may be given after a deletion, #364).
            let billing_ref = obj["metadata"]["billing_ref"].as_str().unwrap_or(""); // panic-ok: serde_json Value indexing gives Null, never panics
            let micros = obj["metadata"]["credit_micros"].as_str().and_then(|v| v.parse::<i64>().ok()).filter(|m| *m > 0); // panic-ok: serde_json Value indexing gives Null, never panics
            let Some(micros) = micros else { return Ok(None) };
            if session.is_empty() || billing_ref.is_empty() {
                return Ok(None);
            }
            let user_id: Option<i64> = conn.query_row("SELECT id FROM users WHERE billing_ref = ?1", [billing_ref], |r| r.get(0)).optional()?;
            let Some(user_id) = user_id else { return Ok(None) };
            let inserted = conn.execute(
                "INSERT INTO ai_credit_purchases (session_id, user_id, payment_intent, price_id, amount_cents, credit_micros, refunded_micros, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7) ON CONFLICT(session_id) DO NOTHING",
                params![
                    session,
                    user_id,
                    obj["payment_intent"].as_str().unwrap_or(""), // panic-ok: serde_json Value indexing gives Null, never panics
                    obj["metadata"]["pack"].as_str().unwrap_or(""), // panic-ok: serde_json Value indexing gives Null, never panics
                    obj["amount_total"].as_i64().unwrap_or(0), // panic-ok: serde_json Value indexing gives Null, never panics
                    micros,
                    now()
                ],
            )?;
            if inserted == 0 {
                return Ok(None);
            }
            add_credits(conn, user_id, micros)?;
            Ok(Some(CreditChange { user_id, micros }))
        }
        "charge.refunded" => {
            let Some(pi) = obj["payment_intent"].as_str().filter(|p| !p.is_empty()) else { return Ok(None) }; // panic-ok: serde_json Value indexing gives Null, never panics
            let row: Option<(String, i64, i64, i64, i64)> = conn
                .query_row(
                    "SELECT session_id, user_id, amount_cents, credit_micros, refunded_micros FROM ai_credit_purchases WHERE payment_intent = ?1",
                    [pi],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
                )
                .optional()?;
            let Some((session, user_id, amount, credit, refunded)) = row else { return Ok(None) };
            let charged = obj["amount"].as_i64().filter(|a| *a > 0).unwrap_or(amount.max(1)); // panic-ok: serde_json Value indexing gives Null, never panics
            let back = obj["amount_refunded"].as_i64().unwrap_or(0).clamp(0, charged); // panic-ok: serde_json Value indexing gives Null, never panics
            let target = ((credit as i128 * back as i128) / charged as i128) as i64;
            let delta = target - refunded;
            if delta <= 0 {
                return Ok(None);
            }
            conn.execute("UPDATE ai_credit_purchases SET refunded_micros = ?2 WHERE session_id = ?1", params![session, target])?;
            add_credits(conn, user_id, -delta)?;
            Ok(Some(CreditChange { user_id, micros: -delta }))
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests;
