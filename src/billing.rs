//! Paid monthly plans: which plan a learner is on (`Plan`) and the Stripe
//! plumbing.
//!
//! A learner's plan (`plan_sql`, `CurrentUser::plan`) is **Plus** while they
//! have a Stripe subscription at the Plus price in a paying status
//! (`PREMIUM_STATUSES`) or an unexpired grant (`plan_grants`: given by hand
//! or by the dev toggle); **Basic** with a paying subscription at the Basic
//! price; **Free** otherwise. "Premium" means any paid plan; a gated handler
//! takes `RequirePremium`.
//!
//! Stripe is the only way to pay, and the source of truth (#963): the app
//! learns the outcome of Checkout only from signed webhooks
//! (`customer.subscription.*`), never from the browser returning, and the
//! plan comes from the subscription's price (`StripeConfig::plan_for_price`),
//! never from metadata. Cancelling here asks Stripe to end the subscription
//! at the period's end. Calls to Stripe are form-encoded POSTs through
//! `ureq`; no SDK. (The pretend card payment of #315 and its
//! `plan_subscriptions` table are gone, v83.)
//!
//! The same plumbing sells prepaid AI credit packs (one-off Checkout
//! payments, `credits_checkout_url`); their webhooks
//! (`checkout.session.completed`, `charge.refunded`) are applied by
//! `metering::apply_credit_event`.

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::Duration;

use crate::auth::CurrentUser;
use crate::db::now;
use crate::error::AppError;
use crate::AppState;

/// Stripe statuses that keep the paid features on. `past_due` stays premium
/// while Stripe retries the card; it moves the subscription to `canceled` or
/// `unpaid` when it gives up.
pub const PREMIUM_STATUSES: [&str; 3] = ["active", "trialing", "past_due"];

/// The plans. Free has no Skrongle AI; Basic (CA$3 a month) supports the
/// project and gets the paid features as they land; Plus (CA$15 a month) also
/// includes Skrongle AI each month (`metering::MeterConfig::allowance_plus`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Plan {
    #[default]
    Free,
    Basic,
    Plus,
}

impl Plan {
    /// The plans on sale, cheapest first.
    pub const PAID: [Plan; 2] = [Plan::Basic, Plan::Plus];

    /// The id stored and posted: `free`, `basic`, `plus`.
    pub fn as_str(self) -> &'static str {
        match self {
            Plan::Free => "free",
            Plan::Basic => "basic",
            Plan::Plus => "plus",
        }
    }

    pub fn parse(s: &str) -> Option<Plan> {
        match s {
            "free" => Some(Plan::Free),
            "basic" => Some(Plan::Basic),
            "plus" => Some(Plan::Plus),
            "pro" => Some(Plan::Plus), // the name before #920 (Stripe metadata, `subscriptions.plan`'s default)
            _ => None,
        }
    }

    /// Any paid plan ("premium").
    pub fn paid(self) -> bool {
        self != Plan::Free
    }

    /// "Free", "Skrongle Basic", "Skrongle Plus".
    pub fn label(self) -> String {
        match self {
            Plan::Free => crate::t("billing.plan.free.name"),
            Plan::Basic => crate::t1("billing.plan.basic.name", "app", crate::APP_NAME),
            Plan::Plus => crate::t1("billing.plan.plus.name", "app", crate::APP_NAME),
        }
    }

    /// "Free", "Basic", "Plus": the plan without the product name.
    pub fn short(self) -> String {
        match self {
            Plan::Free => crate::t("billing.plan.free.name"),
            Plan::Basic => "Basic".to_string(),
            Plan::Plus => "Plus".to_string(),
        }
    }

    pub fn price_cents(self) -> i64 {
        match self {
            Plan::Free => 0,
            Plan::Basic => 300,
            Plan::Plus => 1500,
        }
    }

    /// "CA$3 a month", "CA$15 a month" (prices are in Canadian dollars);
    /// "Free" for Free.
    pub fn price_label(self) -> String {
        match self {
            Plan::Free => crate::t("billing.plan.free.name"),
            p => crate::t1("billing.plan.price", "price", format!("CA${}", p.price_cents() / 100)),
        }
    }

    /// One line under the name on the plan's card.
    pub fn blurb(self) -> String {
        crate::t1(&format!("billing.plan.{}.blurb", self.as_str()), "hosted", crate::tutor::hosted_label())
    }

    /// What the card lists (`billing.plan.<plan>.perks`, one per line).
    pub fn perks(self) -> Vec<String> {
        let text = crate::t1(&format!("billing.plan.{}.perks", self.as_str()), "hosted", crate::tutor::hosted_label());
        text.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect()
    }
}

/// The one switch every plan gate asks: does this server have plans? The
/// hosted site does, Skrongle mini never does (#636, `shape::Shape::has_plans`):
/// there every account has every feature and the server's AI key, when it
/// has one, is shared by everyone up to `SKRONGLE_AI_DAILY_CAP`. It lives
/// on the meter's config (`MeterConfig::plans`), so the AI meter,
/// which has no `AppState`, reads the same value.
pub fn plans_on(state: &AppState) -> bool {
    state.meter.cfg.plans
}

/// The plan a learner counts as: their own with plans on, Plus for every
/// account with plans off.
pub fn effective_plan(plans: bool, plan: Plan) -> Plan {
    if plans { plan } else { Plan::Plus }
}

/// SQL expression: the plan of user `uid` (a column or parameter) at time
/// `now`, as `'plus'`, `'basic'` or `'free'`. Plus: a paying Stripe
/// subscription for Plus (`subscriptions.plan`, from the subscription's
/// price) or an unexpired grant; Basic: a paying Stripe subscription for
/// Basic. Used by `plan_of`, `premium_sql` and the session lookup in `auth.rs`.
pub fn plan_sql(uid: &str, now: &str) -> String {
    let stripe = |plan: &str| format!("EXISTS(SELECT 1 FROM subscriptions WHERE user_id = {uid} AND plan = '{plan}' AND status IN ('active', 'trialing', 'past_due'))");
    format!(
        "(CASE WHEN {stripe_plus}
                    OR EXISTS(SELECT 1 FROM plan_grants WHERE user_id = {uid} AND (expires_at IS NULL OR expires_at > {now})) THEN 'plus'
               WHEN {stripe_basic} THEN 'basic'
               ELSE 'free' END)",
        stripe_plus = stripe("plus"),
        stripe_basic = stripe("basic"),
    )
}

/// Route guard for paid features: add `_: RequirePremium` to a handler's
/// arguments. A free learner's browser request is sent to the profile's
/// subscription section, an `/api/*` request gets a 402; signed-out
/// requests are rejected as by `CurrentUser`. With plans off (`plans_on`)
/// every account is Plus (`CurrentUser::plan`), so it always passes.
#[allow(dead_code)] // no feature is gated yet
pub struct RequirePremium;

impl FromRequestParts<Arc<AppState>> for RequirePremium {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &Arc<AppState>) -> Result<Self, Self::Rejection> {
        let user = CurrentUser::from_request_parts(parts, state).await?;
        match (user.premium, parts.uri.path().starts_with("/api/")) {
            (true, _) => Ok(RequirePremium),
            (false, true) => Err(AppError::ApiPaymentRequired),
            (false, false) => Err(AppError::PaymentRequired),
        }
    }
}

// ---------- Configuration ----------

/// `SKRONGLE_STRIPE_SECRET_KEY`, `SKRONGLE_STRIPE_WEBHOOK_SECRET`,
/// `SKRONGLE_STRIPE_PRICE_ID` and `SKRONGLE_PUBLIC_URL`, all four or none;
/// `SKRONGLE_STRIPE_PRICE_ID_BASIC` too on the hosted site (`main.rs`
/// refuses to start without it).
#[derive(Clone)]
pub struct StripeConfig {
    pub secret_key: String,
    pub webhook_secret: String,
    /// The recurring monthly price of the Plus plan (`price_…`).
    pub price_id: String,
    /// The Basic plan's monthly price (`SKRONGLE_STRIPE_PRICE_ID_BASIC`);
    /// unset (or the same as `price_id`), Basic is not sold (the dev
    /// stand-in has both).
    pub price_id_basic: Option<String>,
    /// What the profile says it costs, e.g. "€4 a month" (`SKRONGLE_STRIPE_PRICE_LABEL`).
    pub price_label: String,
    /// Public URL of the app including the base path, for Stripe's return links.
    pub public_url: String,
    /// `https://api.stripe.com`; `SKRONGLE_STRIPE_API_URL` points it at a mock.
    pub api_base: String,
}

/// Never prints the secret key or the webhook secret.
impl std::fmt::Debug for StripeConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StripeConfig")
            .field("secret_key", &"<redacted>")
            .field("webhook_secret", &"<redacted>")
            .field("price_id", &self.price_id)
            .field("price_id_basic", &self.price_id_basic)
            .field("price_label", &self.price_label)
            .field("public_url", &self.public_url)
            .field("api_base", &self.api_base)
            .finish()
    }
}

impl StripeConfig {
    /// The Stripe price Checkout sells `plan` at; `None`: not on sale here
    /// (Free, or Basic without a price of its own).
    pub fn price_for(&self, plan: Plan) -> Option<&str> {
        match plan {
            Plan::Free => None,
            Plan::Basic => self.price_id_basic.as_deref().filter(|b| *b != self.price_id),
            Plan::Plus => Some(&self.price_id),
        }
    }

    /// The plan a subscription at `price_id` pays for: what was paid decides,
    /// never the metadata (#963). `None`: not one of our plans' prices.
    pub fn plan_for_price(&self, price_id: &str) -> Option<Plan> {
        Plan::PAID.into_iter().find(|&p| !price_id.is_empty() && self.price_for(p) == Some(price_id))
    }
}

pub const STRIPE_API: &str = "https://api.stripe.com";

// ---------- Persistence ----------

/// What the app knows of a learner's Stripe subscription.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Subscription {
    pub customer_id: String,
    pub subscription_id: String,
    /// Stripe's status: incomplete, trialing, active, past_due, canceled, unpaid, …
    pub status: String,
    pub price_id: String,
    /// The plan it pays for (from its price, `StripeConfig::plan_for_price`).
    pub plan: Plan,
    pub current_period_end: Option<i64>,
    pub cancel_at_period_end: bool,
}

impl Subscription {
    pub fn is_premium(&self) -> bool {
        PREMIUM_STATUSES.contains(&self.status.as_str())
    }
}

pub fn subscription(conn: &Connection, user_id: i64) -> rusqlite::Result<Option<Subscription>> {
    conn.query_row(
        "SELECT customer_id, subscription_id, status, price_id, current_period_end, cancel_at_period_end, plan
         FROM subscriptions WHERE user_id = ?1",
        [user_id],
        |r| {
            Ok(Subscription {
                customer_id: r.get(0)?,
                subscription_id: r.get(1)?,
                status: r.get(2)?,
                price_id: r.get(3)?,
                current_period_end: r.get(4)?,
                cancel_at_period_end: r.get(5)?,
                plan: Plan::parse(&r.get::<_, String>(6)?).filter(|p| p.paid()).unwrap_or(Plan::Plus),
            })
        },
    )
    .optional()
}

/// A grant: premium without paying, until `expires_at` (never when `None`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grant {
    pub expires_at: Option<i64>,
    pub note: String,
}

pub fn grant(conn: &Connection, user_id: i64) -> rusqlite::Result<Option<Grant>> {
    conn.query_row(
        "SELECT expires_at, note FROM plan_grants WHERE user_id = ?1 AND (expires_at IS NULL OR expires_at > ?2)",
        params![user_id, now()],
        |r| Ok(Grant { expires_at: r.get(0)?, note: r.get(1)? }),
    )
    .optional()
}

pub fn set_grant(conn: &Connection, user_id: i64, expires_at: Option<i64>, note: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO plan_grants (user_id, granted_at, expires_at, note) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(user_id) DO UPDATE SET granted_at = excluded.granted_at, expires_at = excluded.expires_at, note = excluded.note",
        params![user_id, now(), expires_at, note],
    )?;
    Ok(())
}

pub fn revoke_grant(conn: &Connection, user_id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM plan_grants WHERE user_id = ?1", [user_id])?;
    Ok(())
}

/// Whether the stored plan is a paid one (tests; the app reads `plan_of`).
#[cfg(test)]
pub fn is_premium(conn: &Connection, user_id: i64) -> rusqlite::Result<bool> {
    Ok(plan_of(conn, user_id)?.paid())
}

/// The plan user `user_id` is on now (`plan_sql`), as stored; with plans
/// off every account counts as Plus (`effective_plan`).
pub fn plan_of(conn: &Connection, user_id: i64) -> rusqlite::Result<Plan> {
    let p: String = conn.query_row(&format!("SELECT {}", plan_sql("?1", "?2")), params![user_id, now()], |r| r.get(0))?;
    Ok(Plan::parse(&p).unwrap_or_default())
}

// ---------- Cancelling ----------

/// The same day next month (the last day of it when that day does not exist).
pub fn add_month(t: i64) -> i64 {
    let (y, m, d) = crate::profile::civil(t);
    let (y, m) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    let last = crate::profile::days_in_month(y, m);
    let secs = t.rem_euclid(86_400);
    crate::profile::days_from_civil(y, m, d.min(last)) * 86_400 + secs
}

/// The reasons the cancel dialog offers (id, label): at most four.
pub const CANCEL_REASONS: [(&str, &str); 4] = [
    ("price", "billing.cancel_reason.price"),
    ("unused", "billing.cancel_reason.unused"),
    ("missing", "billing.cancel_reason.missing"),
    ("problems", "billing.cancel_reason.problems"),
];

/// Keep why a learner cancelled (`plan_cancellations`): a reason id from
/// `CANCEL_REASONS` or `''`, and their words, if any.
pub fn record_cancellation(conn: &Connection, user_id: i64, plan: Plan, reason: &str, feedback: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO plan_cancellations (user_id, plan, reason, feedback, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![user_id, plan.as_str(), reason, feedback, now()],
    )?;
    Ok(())
}

/// Note locally that a Stripe subscription ends at the period's end (Stripe's
/// webhook confirms it).
pub fn mark_stripe_cancelling(conn: &Connection, user_id: i64) -> rusqlite::Result<()> {
    conn.execute("UPDATE subscriptions SET cancel_at_period_end = 1, updated_at = ?2 WHERE user_id = ?1", params![user_id, now()])?;
    Ok(())
}

/// What Stripe knows the account by: the random `billing_ref` Checkout puts
/// in the subscription's metadata (never the row id), and the one Stripe
/// customer the account's Checkouts share once it exists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BillingAccount {
    pub billing_ref: String,
    pub stripe_customer: Option<String>,
    pub created_at: i64,
}

pub fn account(conn: &Connection, user_id: i64) -> rusqlite::Result<Option<BillingAccount>> {
    conn.query_row(
        "SELECT billing_ref, COALESCE(stripe_customer, (SELECT customer_id FROM subscriptions WHERE user_id = users.id)), created_at
         FROM users WHERE id = ?1",
        [user_id],
        |r| Ok(BillingAccount { billing_ref: r.get(0)?, stripe_customer: r.get(1)?, created_at: r.get(2)? }),
    )
    .optional()
}

/// Remember the account's Stripe customer unless it already has one, and
/// return the one stored (so two Checkouts racing end up with the same).
pub fn set_customer(conn: &Connection, user_id: i64, customer_id: &str) -> rusqlite::Result<String> {
    conn.execute("UPDATE users SET stripe_customer = ?1 WHERE id = ?2 AND stripe_customer IS NULL", params![customer_id, user_id])?;
    conn.query_row("SELECT COALESCE(stripe_customer, '') FROM users WHERE id = ?1", [user_id], |r| r.get(0))
}

fn user_for_ref(conn: &Connection, billing_ref: &str) -> rusqlite::Result<Option<i64>> {
    conn.query_row("SELECT id FROM users WHERE billing_ref = ?1", [billing_ref], |r| r.get(0)).optional()
}

fn user_for_customer(conn: &Connection, customer_id: &str) -> rusqlite::Result<Option<i64>> {
    conn.query_row(
        "SELECT id FROM users WHERE stripe_customer = ?1
         UNION ALL SELECT user_id FROM subscriptions WHERE customer_id = ?1 LIMIT 1",
        [customer_id],
        |r| r.get(0),
    )
    .optional()
}

/// What the profile's subscription section shows.
#[derive(Clone, Debug, Default)]
pub struct SubscriptionView {
    pub plan: Plan,
    /// Any paid plan.
    pub premium: bool,
    /// Where the plan comes from: `stripe`, `grant` or `free`.
    pub source: &'static str,
    /// The paid month's end, as a date: "19 November 2026".
    pub period_end: Option<String>,
    /// Cancelled: the plan ends on `period_end` instead of renewing.
    pub cancelled: bool,
    /// The Cancel button shows: a paid plan that still renews.
    pub cancellable: bool,
    /// Stripe is retrying the card.
    pub past_due: bool,
    /// A Stripe customer exists, so the Billing Portal can open (card, invoices).
    pub stripe_portal: bool,
    /// Plus by grant: "no end date" or "until <date>".
    pub grant_note: Option<String>,
}

impl SubscriptionView {
    /// The plans the chooser offers, with the one held marked.
    pub fn cards(&self) -> Vec<PlanCard> {
        Plan::PAID.iter().map(|&plan| PlanCard { plan, current: plan == self.plan && !self.cancelled }).collect()
    }

    /// What a cancellation says: the plan kept until `period_end`, then Free.
    pub fn cancel_note(&self) -> String {
        match &self.period_end {
            Some(d) => crate::t2("billing.cancel_note", "plan", self.plan.label(), "date", d),
            None => crate::t("billing.cancel_note_free"),
        }
    }
}

/// One plan in the chooser.
#[derive(Clone, Debug)]
pub struct PlanCard {
    pub plan: Plan,
    /// The learner is on it (and it renews).
    pub current: bool,
}

pub fn view(conn: &Connection, user_id: i64, stripe: Option<&StripeConfig>) -> rusqlite::Result<SubscriptionView> {
    let plan = plan_of(conn, user_id)?;
    let sub = subscription(conn, user_id)?;
    let grant = grant(conn, user_id)?;
    let date = |t: i64| crate::profile::long_date(t);
    let mut v = SubscriptionView {
        plan,
        premium: plan.paid(),
        source: "free",
        stripe_portal: stripe.is_some() && sub.is_some(),
        grant_note: grant.as_ref().map(|g| match g.expires_at {
            Some(t) => crate::t1("billing.grant_until", "date", date(t)),
            None => crate::t("billing.grant_no_end"),
        }),
        ..SubscriptionView::default()
    };
    // The same order as `plan_sql`: Stripe, then a grant.
    match (sub.as_ref().filter(|s| s.is_premium()), grant) {
        (Some(s), _) => {
            v.source = "stripe";
            v.period_end = s.current_period_end.map(date);
            v.cancelled = s.cancel_at_period_end;
            v.cancellable = !s.cancel_at_period_end;
            v.past_due = s.status == "past_due";
        }
        (None, Some(_)) => v.source = "grant",
        (None, None) => {}
    }
    Ok(v)
}

// ---------- Webhooks ----------

/// How far a webhook's timestamp may be from our clock (Stripe's default).
const SIGNATURE_TOLERANCE_SECS: i64 = 300;

/// Check a `Stripe-Signature` header (`t=…,v1=…[,v1=…]`) against the raw body.
pub fn verify_signature(secret: &str, header: &str, body: &[u8], now: i64) -> Result<(), &'static str> {
    let mut t = None;
    let mut sigs = Vec::new();
    for part in header.split(',') {
        match part.trim().split_once('=') {
            Some(("t", v)) => t = v.parse::<i64>().ok(),
            Some(("v1", v)) => sigs.push(v),
            _ => {}
        }
    }
    let t = t.ok_or("no timestamp in the signature header")?;
    if sigs.is_empty() {
        return Err("no v1 signature in the signature header");
    }
    if (now - t).abs() > SIGNATURE_TOLERANCE_SECS {
        return Err("signature timestamp is too old");
    }
    let mut payload = format!("{t}.").into_bytes();
    payload.extend_from_slice(body);
    let expected = hex(&hmac_sha256(secret.as_bytes(), &payload));
    if sigs.iter().any(|s| constant_time_eq(s.as_bytes(), expected.as_bytes())) {
        Ok(())
    } else {
        Err("signature does not match")
    }
}

/// A `Stripe-Signature` header for `body` at time `t`, as Stripe would send
/// it (the dev simulator's webhooks, tests).
pub fn sign(secret: &str, body: &[u8], t: i64) -> String {
    let mut payload = format!("{t}.").into_bytes();
    payload.extend_from_slice(body);
    format!("t={t},v1={}", hex(&hmac_sha256(secret.as_bytes(), &payload)))
}

fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut k = [0u8; BLOCK];
    if key.len() > BLOCK {
        k[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let (mut ipad, mut opad) = ([0x36u8; BLOCK], [0x5cu8; BLOCK]);
    for i in 0..BLOCK {
        ipad[i] ^= k[i];
        opad[i] ^= k[i];
    }
    let inner = Sha256::new().chain_update(ipad).chain_update(msg).finalize();
    Sha256::new().chain_update(opad).chain_update(inner).finalize().into()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// What a webhook did, for the log.
#[derive(Debug, PartialEq, Eq)]
pub enum Applied {
    Updated { user_id: i64, status: String },
    /// AI credits bought (positive) or refunded (negative), in micro-dollars.
    Credits { user_id: i64, micros: i64 },
    /// Not an event we act on, an older event than one already applied, or
    /// a subscription we cannot tie to a user.
    Ignored(String),
}

/// Apply a verified Stripe event. Only `customer.subscription.*` events
/// change anything: their object is the whole subscription, so each one
/// simply overwrites the row, and events older than the last applied one
/// (Stripe does not promise order) are skipped. The plan is the one `cfg`
/// sells at the subscription's price (#963): a paying subscription at any
/// other price changes nothing, whatever its metadata says.
pub fn apply_event(conn: &Connection, cfg: &StripeConfig, event: &serde_json::Value) -> rusqlite::Result<Applied> {
    if let Some(ch) = crate::metering::apply_credit_event(conn, event)? {
        return Ok(Applied::Credits { user_id: ch.user_id, micros: ch.micros });
    }
    let kind = event["type"].as_str().unwrap_or("");
    if !kind.starts_with("customer.subscription.") {
        return Ok(Applied::Ignored(format!("event type {kind}")));
    }
    let created = event["created"].as_i64().unwrap_or(0);
    let sub = &event["data"]["object"];
    let s = |k: &str| sub[k].as_str().unwrap_or("").to_string();
    let (subscription_id, customer_id, status) = (s("id"), s("customer"), s("status"));
    if subscription_id.is_empty() || customer_id.is_empty() || status.is_empty() {
        return Ok(Applied::Ignored("subscription without id, customer or status".into()));
    }
    // Checkout puts the account's random `billing_ref` in the subscription's
    // metadata; the customer id is the fallback for subscriptions made some
    // other way. Subscriptions from before `billing_ref` carry the row id,
    // which a later account may have been given (#364): it counts only for
    // the account that already has this customer.
    let metadata = &sub["metadata"]; // panic-ok: serde_json Value indexing gives Null, never panics
    let user_id = if let Some(r) = metadata["billing_ref"].as_str() { // panic-ok: serde_json Value indexing gives Null, never panics
        user_for_ref(conn, r)?
    } else if let Some(id) = metadata["user_id"].as_str().and_then(|v| v.parse::<i64>().ok()) { // panic-ok: serde_json Value indexing gives Null, never panics
        account(conn, id)?.filter(|a| a.stripe_customer.as_deref() == Some(customer_id.as_str())).map(|_| id)
    } else {
        user_for_customer(conn, &customer_id)?
    };
    let acct = match user_id {
        Some(id) => account(conn, id)?.map(|a| (id, a)),
        None => None,
    };
    let Some((user_id, acct)) = acct else {
        return Ok(Applied::Ignored(format!("no user for subscription {subscription_id}")));
    };
    // One account, one Stripe customer: a subscription of another customer,
    // or one made before the account existed, belongs to someone else.
    if acct.stripe_customer.as_deref().is_some_and(|c| c != customer_id) {
        return Ok(Applied::Ignored(format!("{subscription_id} is for customer {customer_id}, not user {user_id}'s")));
    }
    if sub["created"].as_i64().is_some_and(|t| t < acct.created_at) { // panic-ok: serde_json Value indexing gives Null, never panics
        return Ok(Applied::Ignored(format!("{subscription_id} predates user {user_id}")));
    }
    let item = &sub["items"]["data"][0];
    let price_id = item["price"]["id"].as_str().unwrap_or("").to_string();
    // What was paid decides the plan (#963), never `metadata.plan`. A paying
    // subscription at a price we do not sell gives nothing; one that no
    // longer pays is still recorded (it ends whatever plan it gave).
    let plan = match cfg.plan_for_price(&price_id) {
        Some(p) => p,
        None if PREMIUM_STATUSES.contains(&status.as_str()) => {
            tracing::warn!("billing: {subscription_id} (user {user_id}) pays price {price_id:?}, which is not a plan's price; ignored");
            return Ok(Applied::Ignored(format!("{subscription_id} is at price {price_id:?}, not a plan's")));
        }
        None => Plan::Plus,
    };
    // Newer API versions keep the period on the item, older ones on the subscription.
    let period_end = item["current_period_end"].as_i64().or_else(|| sub["current_period_end"].as_i64());
    let cancel_at_period_end = sub["cancel_at_period_end"].as_bool().unwrap_or(false);

    let existing: Option<(String, String, i64)> = conn
        .query_row("SELECT subscription_id, status, event_at FROM subscriptions WHERE user_id = ?1", [user_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .optional()?;
    if let Some((old_id, old_status, event_at)) = &existing {
        if created < *event_at {
            return Ok(Applied::Ignored(format!("event older than the last one applied for user {user_id}")));
        }
        // Two subscriptions for one learner (two checkouts at once): the one
        // still paying wins over the other ending, and a second one paying
        // is a double charge someone has to refund: say so loudly and keep
        // the first (both are on the one customer, so the Billing Portal
        // shows the learner both).
        if old_id != &subscription_id && PREMIUM_STATUSES.contains(&old_status.as_str()) {
            if PREMIUM_STATUSES.contains(&status.as_str()) {
                tracing::warn!(
                    "billing: user {user_id} has two paying subscriptions, {old_id} and {subscription_id} (customer {customer_id}); keeping {old_id}: refund or cancel the other in Stripe"
                );
                return Ok(Applied::Ignored(format!("{subscription_id} is a second paying subscription beside {old_id}")));
            }
            return Ok(Applied::Ignored(format!("{subscription_id} ended while {old_id} is still paying")));
        }
    }
    set_customer(conn, user_id, &customer_id)?;
    conn.execute(
        "INSERT INTO subscriptions (user_id, customer_id, subscription_id, status, price_id, current_period_end,
                                    cancel_at_period_end, event_at, updated_at, plan)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(user_id) DO UPDATE SET customer_id = excluded.customer_id, subscription_id = excluded.subscription_id,
            status = excluded.status, price_id = excluded.price_id, current_period_end = excluded.current_period_end,
            cancel_at_period_end = excluded.cancel_at_period_end, event_at = excluded.event_at, updated_at = excluded.updated_at,
            plan = excluded.plan",
        params![user_id, customer_id, subscription_id, status, price_id, period_end, cancel_at_period_end, created, now(), plan.as_str()],
    )?;
    Ok(Applied::Updated { user_id, status })
}

// ---------- Stripe API (blocking: call from spawn_blocking) ----------

fn post(cfg: &StripeConfig, path: &str, form: &[(&str, &str)], idempotency_key: Option<&str>) -> Result<serde_json::Value, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .http_status_as_error(false)
        .build()
        .into();
    let mut req = agent
        .post(&format!("{}{path}", cfg.api_base.trim_end_matches('/')))
        .header("Authorization", &format!("Bearer {}", cfg.secret_key));
    if let Some(key) = idempotency_key {
        req = req.header("Idempotency-Key", key);
    }
    let mut resp = req
        .send_form(form.iter().copied())
        .map_err(|e| format!("Stripe request failed: {e}"))?;
    let status = resp.status();
    let body = resp.body_mut().read_to_string().map_err(|e| format!("Stripe response unreadable: {e}"))?;
    let json: serde_json::Value = serde_json::from_str(&body).map_err(|e| format!("Stripe response is not JSON: {e}"))?;
    if !status.is_success() {
        let msg = json["error"]["message"].as_str().unwrap_or("no message");
        return Err(format!("Stripe answered HTTP {}: {msg}", status.as_u16()));
    }
    Ok(json)
}

fn session_url(json: serde_json::Value) -> Result<String, String> {
    json["url"].as_str().map(str::to_string).ok_or_else(|| "Stripe returned no session URL".to_string())
}

/// Create the account's Stripe customer before its first Checkout, so every
/// Checkout of the account (two tabs, a double submit) shares it (#394). The
/// idempotency key makes two racing requests return the same customer.
pub fn create_customer(cfg: &StripeConfig, billing_ref: &str, email: Option<&str>) -> Result<String, String> {
    let mut form = vec![("metadata[billing_ref]", billing_ref)]; // panic-ok: a form field name, not indexing
    if let Some(e) = email {
        form.push(("email", e));
    }
    let json = post(cfg, "/v1/customers", &form, Some(&format!("customer-{billing_ref}")))?;
    json["id"].as_str().map(str::to_string).ok_or_else(|| "Stripe returned no customer id".to_string()) // panic-ok: serde_json Value indexing gives Null, never panics
}

/// Who is subscribing, for Checkout.
pub struct Subscriber {
    /// The account's random reference, put in the subscription's metadata.
    pub billing_ref: String,
    /// The account's one Stripe customer (`create_customer`).
    pub customer_id: String,
}

/// Start a Checkout session for `plan`'s monthly price; returns the URL to
/// send the learner to. The webhook reads the plan off the price; the
/// metadata's `plan` is only for people reading the Stripe dashboard.
pub fn checkout_url(cfg: &StripeConfig, who: &Subscriber, plan: Plan) -> Result<String, String> {
    let price = cfg.price_for(plan).ok_or_else(|| format!("{} is not on sale here", plan.as_str()))?;
    let success = format!("{}/profile?billing=done#subscription", cfg.public_url);
    let cancel = format!("{}/profile?billing=cancelled#subscription", cfg.public_url);
    let form = [
        ("mode", "subscription"),
        ("line_items[0][price]", price),
        ("line_items[0][quantity]", "1"),
        ("success_url", success.as_str()),
        ("cancel_url", cancel.as_str()),
        ("client_reference_id", who.billing_ref.as_str()),
        ("subscription_data[metadata][billing_ref]", who.billing_ref.as_str()), // panic-ok: a form field name, not indexing
        ("subscription_data[metadata][plan]", plan.as_str()), // panic-ok: a form field name, not indexing
        ("customer", who.customer_id.as_str()),
    ];
    session_url(post(cfg, "/v1/checkout/sessions", &form, None)?)
}

/// Start a Checkout session for one AI credit pack (a one-off payment) on
/// the account's one customer. The metadata says what the webhook adds:
/// the account's random billing_ref (never the row id, #364) and the credits.
pub fn credits_checkout_url(cfg: &StripeConfig, who: &Subscriber, pack: &crate::metering::CreditPack) -> Result<String, String> {
    let micros = pack.credit_micros.to_string();
    let success = format!("{}/profile?credits=done#ai-usage", cfg.public_url);
    let cancel = format!("{}/profile?credits=cancelled#ai-usage", cfg.public_url);
    let form = [
        ("mode", "payment"),
        ("line_items[0][price]", pack.price_id.as_str()), // panic-ok: a form field name, not indexing
        ("line_items[0][quantity]", "1"), // panic-ok: a form field name, not indexing
        ("success_url", success.as_str()),
        ("cancel_url", cancel.as_str()),
        ("client_reference_id", who.billing_ref.as_str()),
        ("customer", who.customer_id.as_str()),
        ("metadata[kind]", "ai-credits"), // panic-ok: a form field name, not indexing
        ("metadata[billing_ref]", who.billing_ref.as_str()), // panic-ok: a form field name, not indexing
        ("metadata[credit_micros]", micros.as_str()), // panic-ok: a form field name, not indexing
        ("metadata[pack]", pack.price_id.as_str()), // panic-ok: a form field name, not indexing
        ("payment_intent_data[metadata][billing_ref]", who.billing_ref.as_str()), // panic-ok: a form field name, not indexing
    ];
    session_url(post(cfg, "/v1/checkout/sessions", &form, None)?)
}

/// A Billing Portal session for an existing customer: change card, see invoices, cancel.
pub fn portal_url(cfg: &StripeConfig, customer_id: &str) -> Result<String, String> {
    let back = format!("{}/profile#subscription", cfg.public_url);
    session_url(post(cfg, "/v1/billing_portal/sessions", &[("customer", customer_id), ("return_url", back.as_str())], None)?)
}

/// Ask Stripe to end a subscription when its paid period ends (the profile's
/// Cancel). The webhook that follows records it.
pub fn cancel_at_period_end(cfg: &StripeConfig, subscription_id: &str) -> Result<(), String> {
    post(cfg, &format!("/v1/subscriptions/{subscription_id}"), &[("cancel_at_period_end", "true")], None).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Config and provider structs print `<redacted>` for their secrets, so a
    /// stray `{:?}` in a log line leaks nothing.
    #[test]
    fn debug_output_never_shows_secrets() {
        let stripe = StripeConfig {
            secret_key: "sk_live_SECRET1".into(),
            webhook_secret: "whsec_SECRET2".into(),
            price_id: "price_1".into(),
            price_id_basic: None,
            price_label: "Monthly".into(),
            public_url: "https://x".into(),
            api_base: STRIPE_API.into(),
        };
        let google = crate::oauth::GoogleConfig::new("cid".into(), "SECRET3".into(), "https://x/cb".into());
        let provider = crate::ai::ProviderKind::Anthropic.provider("sk-ant-SECRET4".into());
        let printed = format!("{stripe:?} {google:?} {provider:?}");
        for secret in ["SECRET1", "SECRET2", "SECRET3", "SECRET4"] {
            assert!(!printed.contains(secret), "{secret} leaked: {printed}");
        }
        assert!(printed.contains("price_1") && printed.contains("cid") && printed.contains("<redacted>"));
    }

    #[test]
    fn hmac_matches_rfc_4231() {
        // Test case 2 of RFC 4231.
        let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
        assert_eq!(hex(&mac), "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843");
        // Test case 6: a key longer than the block.
        let mac = hmac_sha256(&[0xaa; 131], b"Test Using Larger Than Block-Size Key - Hash Key First");
        assert_eq!(hex(&mac), "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54");
    }

    #[test]
    fn signatures() {
        let body = br#"{"id":"evt_1"}"#;
        let mut payload = b"1000.".to_vec();
        payload.extend_from_slice(body);
        let sig = hex(&hmac_sha256(b"whsec_test", &payload));
        let header = format!("t=1000,v1=deadbeef,v1={sig},v0=ignored");
        assert_eq!(verify_signature("whsec_test", &header, body, 1100), Ok(()));
        assert!(verify_signature("whsec_other", &header, body, 1100).is_err());
        assert!(verify_signature("whsec_test", &header, b"{\"id\":\"evt_2\"}", 1100).is_err());
        assert!(verify_signature("whsec_test", &header, body, 1000 + SIGNATURE_TOLERANCE_SECS + 1).is_err());
        assert!(verify_signature("whsec_test", &format!("v1={sig}"), body, 1000).is_err());
        assert!(verify_signature("whsec_test", "t=1000", body, 1000).is_err());
    }

    /// Plus at `price_m`, Basic at `price_b`.
    fn cfg() -> StripeConfig {
        StripeConfig {
            secret_key: "sk_test_x".into(),
            webhook_secret: "whsec_test".into(),
            price_id: "price_m".into(),
            price_id_basic: Some("price_b".into()),
            price_label: "Monthly".into(),
            public_url: "https://x".into(),
            api_base: STRIPE_API.into(),
        }
    }

    fn event(created: i64, sub_id: &str, status: &str, metadata: serde_json::Value) -> serde_json::Value {
        json!({
            "id": format!("evt_{created}"),
            "type": "customer.subscription.updated",
            "created": created,
            "data": { "object": {
                "id": sub_id, "customer": "cus_1", "status": status, "metadata": metadata,
                "cancel_at_period_end": false,
                "items": { "data": [ { "price": { "id": "price_m" }, "current_period_end": 2_000_000 } ] }
            } }
        })
    }

    #[test]
    fn subscription_events_drive_the_plan() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let ana = crate::auth::create_user(c, "ana@example.org", "h")?;
            assert!(!is_premium(c, ana)?);
            let meta = json!({ "billing_ref": account(c, ana)?.unwrap().billing_ref });

            assert_eq!(apply_event(c, &cfg(), &event(10, "sub_a", "active", meta.clone()))?, Applied::Updated { user_id: ana, status: "active".into() });
            assert!(is_premium(c, ana)?);
            let s = subscription(c, ana)?.unwrap();
            assert_eq!((s.customer_id.as_str(), s.price_id.as_str(), s.current_period_end), ("cus_1", "price_m", Some(2_000_000)));

            // Out of order: an older event does not undo a newer one.
            assert!(matches!(apply_event(c, &cfg(), &event(5, "sub_a", "incomplete", meta.clone()))?, Applied::Ignored(_)));
            assert!(is_premium(c, ana)?);

            // Without metadata the customer id finds the learner.
            apply_event(c, &cfg(), &event(20, "sub_a", "past_due", json!({})))?;
            assert!(is_premium(c, ana)?);
            apply_event(c, &cfg(), &event(30, "sub_a", "canceled", json!({})))?;
            assert!(!is_premium(c, ana)?);

            // A second subscription ending does not end the one still paying.
            apply_event(c, &cfg(), &event(40, "sub_b", "active", meta.clone()))?;
            assert!(matches!(apply_event(c, &cfg(), &event(50, "sub_a", "canceled", meta.clone()))?, Applied::Ignored(_)));
            assert!(is_premium(c, ana)?);

            // Unknown users and other event types change nothing.
            assert!(matches!(apply_event(c, &cfg(), &event(60, "sub_x", "active", json!({ "billing_ref": "nobody" })))?, Applied::Ignored(_)));
            assert!(matches!(apply_event(c, &cfg(), &event(60, "sub_x", "active", json!({ "user_id": "999" })))?, Applied::Ignored(_)));
            assert!(matches!(apply_event(c, &cfg(), &json!({ "type": "invoice.paid", "created": 70 }))?, Applied::Ignored(_)));
            Ok(())
        })
        .unwrap();
    }

    /// #394: a second subscription paying beside the first is kept out (and
    /// logged as a warning); the first stays on the account.
    #[test]
    fn a_second_paying_subscription_keeps_the_first() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let ana = crate::auth::create_user(c, "ana@example.org", "h")?;
            let meta = json!({ "billing_ref": account(c, ana)?.unwrap().billing_ref });
            apply_event(c, &cfg(), &event(10, "sub_a", "active", meta.clone()))?;
            assert!(matches!(apply_event(c, &cfg(), &event(20, "sub_b", "active", meta.clone()))?, Applied::Ignored(_)));
            assert_eq!(subscription(c, ana)?.unwrap().subscription_id, "sub_a");
            // The first ending lets the second one in when it next reports.
            apply_event(c, &cfg(), &event(30, "sub_a", "canceled", meta.clone()))?;
            apply_event(c, &cfg(), &event(40, "sub_b", "active", meta.clone()))?;
            assert_eq!(subscription(c, ana)?.unwrap().subscription_id, "sub_b");
            assert!(is_premium(c, ana)?);
            Ok(())
        })
        .unwrap();
    }

    /// #364: account ids are never reused, and a deleted account's Stripe
    /// events (its billing_ref, its old row id, its customer) never reach
    /// the next account; nor does a subscription of another customer or one
    /// older than the account.
    #[test]
    fn a_deleted_accounts_subscription_never_reaches_the_next_account() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let ana = crate::auth::create_user(c, "ana@example.org", "h")?;
            let ana_ref = account(c, ana)?.unwrap().billing_ref;
            assert_eq!(ana_ref.len(), 32);
            let ana_meta = json!({ "billing_ref": ana_ref });
            apply_event(c, &cfg(), &event(10, "sub_a", "active", ana_meta.clone()))?;
            assert_eq!(account(c, ana)?.unwrap().stripe_customer.as_deref(), Some("cus_1"));
            // Cancelled at period end, then the account is deleted.
            assert!(crate::privacy::delete_account(c, ana)?.is_some());

            let bob = crate::auth::create_user(c, "bob@example.org", "h")?;
            assert_ne!(bob, ana, "the deleted account's id was handed out again");
            let bob_ref = account(c, bob)?.unwrap().billing_ref;
            assert_ne!(bob_ref, ana_ref);
            // Stripe keeps reporting the deleted account's subscription: by
            // its billing_ref, by its old row id (as before billing_ref), and
            // by its customer alone.
            for meta in [ana_meta.clone(), json!({ "user_id": ana.to_string() }), json!({ "user_id": bob.to_string() }), json!({})] {
                assert!(matches!(apply_event(c, &cfg(), &event(20, "sub_a", "active", meta.clone()))?, Applied::Ignored(_)), "{meta}");
            }
            assert!(subscription(c, bob)?.is_none() && !is_premium(c, bob)?);

            // Bob's own subscription, on his own customer, applies…
            let bob_meta = json!({ "billing_ref": bob_ref });
            let mut own = event(30, "sub_b", "active", bob_meta.clone());
            own["data"]["object"]["customer"] = json!("cus_2");
            own["data"]["object"]["created"] = json!(now() + 5);
            assert_eq!(apply_event(c, &cfg(), &own)?, Applied::Updated { user_id: bob, status: "active".into() });
            // …but not one naming him on another customer, nor one older than his account.
            let mut other_customer = own.clone();
            other_customer["created"] = json!(40);
            other_customer["data"]["object"]["customer"] = json!("cus_1");
            assert!(matches!(apply_event(c, &cfg(), &other_customer)?, Applied::Ignored(_)));
            let mut before_bob = own.clone();
            before_bob["created"] = json!(50);
            before_bob["data"]["object"]["created"] = json!(account(c, bob)?.unwrap().created_at - 100);
            assert!(matches!(apply_event(c, &cfg(), &before_bob)?, Applied::Ignored(_)));
            assert_eq!(subscription(c, bob)?.unwrap().customer_id, "cus_2");
            Ok(())
        })
        .unwrap();
    }

    /// #963: the plan comes from the price paid. Metadata naming Plus on a
    /// Basic-price subscription gives Basic; a paying subscription at a price
    /// that is not a plan's gives nothing; the session lookup agrees.
    #[test]
    fn the_price_paid_decides_the_plan_not_the_metadata() {
        let k = cfg();
        assert_eq!((k.plan_for_price("price_m"), k.plan_for_price("price_b"), k.plan_for_price("price_x"), k.plan_for_price("")), (Some(Plan::Plus), Some(Plan::Basic), None, None));
        assert_eq!((k.price_for(Plan::Free), k.price_for(Plan::Basic), k.price_for(Plan::Plus)), (None, Some("price_b"), Some("price_m")));
        // No Basic price of its own: Basic is not sold, and no price buys it.
        let plus_only = StripeConfig { price_id_basic: None, ..cfg() };
        assert_eq!((plus_only.price_for(Plan::Basic), plus_only.plan_for_price("price_b")), (None, None));
        let same = StripeConfig { price_id_basic: Some("price_m".into()), ..cfg() };
        assert_eq!((same.price_for(Plan::Basic), same.plan_for_price("price_m")), (None, Some(Plan::Plus)));

        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let ana = crate::auth::create_user(c, "ana@example.org", "h")?;
            let token = crate::auth::create_session(c, ana)?;
            let forged = json!({ "billing_ref": account(c, ana)?.unwrap().billing_ref, "plan": "plus" });
            let at = |created: i64, status: &str, price: &str| {
                let mut e = event(created, "sub_a", status, forged.clone());
                e["data"]["object"]["items"]["data"][0]["price"]["id"] = json!(price);
                e
            };
            // A price that is not a plan's: nothing, whatever the metadata says.
            assert!(matches!(apply_event(c, &cfg(), &at(10, "active", "price_x"))?, Applied::Ignored(_)));
            assert_eq!(plan_of(c, ana)?, Plan::Free);
            assert!(subscription(c, ana)?.is_none());
            // The Basic price with metadata saying Plus: Basic.
            apply_event(c, &cfg(), &at(20, "active", "price_b"))?;
            assert_eq!(plan_of(c, ana)?, Plan::Basic);
            assert_eq!(subscription(c, ana)?.unwrap().plan, Plan::Basic);
            let me = crate::auth::user_for_session(c, &token)?.unwrap();
            assert!(me.premium && me.plan == Plan::Basic);
            // Switched to the Plus price (the portal): Plus.
            apply_event(c, &cfg(), &at(30, "active", "price_m"))?;
            assert_eq!(plan_of(c, ana)?, Plan::Plus);
            // Ended at a price we no longer sell: still recorded, so the plan ends.
            assert_eq!(apply_event(c, &cfg(), &at(40, "canceled", "price_old"))?, Applied::Updated { user_id: ana, status: "canceled".into() });
            assert_eq!(plan_of(c, ana)?, Plan::Free);
            assert!(!crate::auth::user_for_session(c, &token)?.unwrap().premium);

            record_cancellation(c, ana, Plan::Plus, "price", "too dear")?;
            let (reason, feedback): (String, String) =
                c.query_row("SELECT reason, feedback FROM plan_cancellations WHERE user_id = ?1", [ana], |r| Ok((r.get(0)?, r.get(1)?)))?;
            assert_eq!((reason.as_str(), feedback.as_str()), ("price", "too dear"));
            Ok(())
        })
        .unwrap();
    }

    /// The setting itself is parsed per shape in `shape.rs`.
    #[test]
    fn the_plans_setting() {
        assert_eq!(effective_plan(true, Plan::Free), Plan::Free);
        assert_eq!(effective_plan(true, Plan::Basic), Plan::Basic);
        assert_eq!(effective_plan(false, Plan::Free), Plan::Plus);
    }

    #[test]
    fn add_month_keeps_the_day_or_clamps_it() {
        // 2026-01-31 00:00 UTC -> 2026-02-28; 2026-12-19 12:00 -> 2027-01-19 12:00.
        let jan31 = crate::profile::days_from_civil(2026, 1, 31) * 86_400;
        assert_eq!(crate::profile::civil(add_month(jan31)), (2026, 2, 28));
        let dec19 = crate::profile::days_from_civil(2026, 12, 19) * 86_400 + 43_200;
        assert_eq!(add_month(dec19), crate::profile::days_from_civil(2027, 1, 19) * 86_400 + 43_200);
        assert_eq!(Plan::Plus.price_label(), "CA$15 a month");
        assert_eq!(Plan::Basic.price_label(), "CA$3 a month");
        assert_eq!(Plan::parse("plus"), Some(Plan::Plus));
    }

    #[test]
    fn grants() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let ana = crate::auth::create_user(c, "ana@example.org", "h")?;
            set_grant(c, ana, Some(now() - 1), "expired")?;
            assert!(!is_premium(c, ana)? && grant(c, ana)?.is_none());
            set_grant(c, ana, None, "friend")?;
            assert!(is_premium(c, ana)?);
            assert_eq!(plan_of(c, ana)?, Plan::Plus, "a grant is Plus");
            assert_eq!(grant(c, ana)?.unwrap().note, "friend");
            revoke_grant(c, ana)?;
            assert!(!is_premium(c, ana)?);
            // The session lookup agrees.
            let token = crate::auth::create_session(c, ana)?;
            assert!(!crate::auth::user_for_session(c, &token)?.unwrap().premium);
            set_grant(c, ana, None, "")?;
            assert!(crate::auth::user_for_session(c, &token)?.unwrap().premium);
            Ok(())
        })
        .unwrap();
    }
}
