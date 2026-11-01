//! The dev payment provider (#314): dev mode's stand-in for Stripe. It
//! speaks the parts of Stripe's REST API the app calls
//! (`/dev/payments/stripe/v1/…`, form-encoded POSTs with a Bearer key,
//! Stripe's JSON objects and error shape; `routes::dev_payments`), hosts the
//! pages Stripe hosts (a Checkout page with **Pay**, **Fail** and **Cancel**,
//! and a Billing Portal), and sends the signed webhooks Stripe sends to the
//! app's own `/billing/webhook`. The app only sees a base URL and keys: dev
//! mode points `SKRONGLE_STRIPE_API_URL` here with fake keys unless real
//! `SKRONGLE_STRIPE_*` settings are given (`SKRONGLE_DEV_STRIPE=0` turns it off).
//!
//! Everything it holds (customers, prices, Checkout sessions, subscriptions,
//! invoices and one-off payments, events, the log) is in memory for this
//! server run; the dev mailbox's Payments section (`/dev/payments`) shows it
//! and plays what happens later in a real account: a renewal paid or
//! failed, a cancellation, a refund, an event sent again.
//!
//! The functions here change the state and return the events to deliver;
//! the route layer delivers them (on an OS thread of its own) and logs each.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::billing::StripeConfig;

/// The secret key dev mode gives the app (Stripe's test-mode prefix).
pub const DEV_SECRET_KEY: &str = "sk_test_dev_simulator";
/// The webhook signing secret dev mode gives the app and signs with.
pub const DEV_WEBHOOK_SECRET: &str = "whsec_dev_simulator";
/// The monthly prices dev mode offers: the Plus plan's and the Basic plan's
/// (`billing::Plan`), so the Checkout page matches the chooser.
pub const DEV_PRICE_ID: &str = "price_dev_monthly";
pub const DEV_PRICE_AMOUNT: i64 = 1500;
pub const DEV_PRICE_ID_BASIC: &str = "price_dev_basic";
pub const DEV_PRICE_AMOUNT_BASIC: i64 = 300;
pub const DEV_PRICE_CURRENCY: &str = "cad";
pub const DEV_PRICE_LABEL: &str = "CA$15 a month";
/// One-off prices the simulator knows from the start (AI credit packs, #311).
pub const DEV_ONE_OFF_PRICES: [(&str, i64, &str, &str); 2] = [("price_dev_credits_5", 500, "usd", "AI credits $5"), ("price_dev_credits_10", 1000, "usd", "AI credits $10")];
/// What an unknown price costs when the simulator makes it up (cents).
pub const MADE_UP_AMOUNT: i64 = 500;
/// A subscription period: 30 days.
pub const PERIOD_SECS: i64 = 30 * 86_400;
/// The API version the events claim.
pub const API_VERSION: &str = "2025-03-31.basil";
/// How many entries the log and the event list keep.
pub const LOG_MAX: usize = 200;
const BODY_MAX: usize = 64 * 1024;

/// Dev mode stands in for Stripe unless real settings are given or
/// `SKRONGLE_DEV_STRIPE=0`.
pub fn wanted(dev: bool, configured: bool) -> bool {
    dev && !configured && crate::env("DEV_STRIPE").as_deref() != Some("0")
}

/// The Stripe settings dev mode gives the app: the simulator on this
/// server, fake keys, the dev monthly price. `public_url` is
/// `SKRONGLE_PUBLIC_URL` when set, else the loopback address.
pub fn dev_config(port: u16, public_url: Option<&str>) -> StripeConfig {
    let local = format!("http://127.0.0.1:{port}{}", crate::base_path());
    StripeConfig {
        secret_key: DEV_SECRET_KEY.into(),
        webhook_secret: DEV_WEBHOOK_SECRET.into(),
        price_id: DEV_PRICE_ID.into(),
        price_id_basic: Some(DEV_PRICE_ID_BASIC.into()),
        price_label: DEV_PRICE_LABEL.into(),
        public_url: public_url.map(|u| u.trim_end_matches('/').to_string()).unwrap_or_else(|| local.clone()),
        api_base: api_base(&local),
    }
}

/// The simulator's API base under an app root (`http://127.0.0.1:8123`).
pub fn api_base(root: &str) -> String {
    format!("{}/dev/payments/stripe", root.trim_end_matches('/'))
}

/// A key the simulator takes: any test-mode secret or restricted key.
pub fn key_ok(key: &str) -> bool {
    ["sk_test_", "rk_test_"].iter().any(|p| key.len() > p.len() && key.starts_with(p))
}

/// "€4.00", "$5.00", "12.00 CHF".
pub fn money(amount: i64, currency: &str) -> String {
    let n = format!("{}.{:02}", amount.abs() / 100, amount.abs() % 100);
    let sign = if amount < 0 { "-" } else { "" };
    match currency {
        "eur" => format!("{sign}€{n}"),
        "usd" => format!("{sign}${n}"),
        "cad" => format!("{sign}CA${n}"),
        "gbp" => format!("{sign}£{n}"),
        c => format!("{sign}{n} {}", c.to_uppercase()),
    }
}

// ---------- errors ----------

/// A Stripe API error: HTTP status and the `error` object's fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StripeError {
    pub status: u16,
    pub kind: &'static str,
    pub code: Option<&'static str>,
    pub param: Option<String>,
    pub message: String,
}

impl StripeError {
    pub fn invalid(param: &str, message: impl Into<String>) -> StripeError {
        StripeError { status: 400, kind: "invalid_request_error", code: Some("parameter_invalid"), param: Some(param.into()), message: message.into() }
    }

    pub fn missing_param(param: &str) -> StripeError {
        StripeError { status: 400, kind: "invalid_request_error", code: Some("parameter_missing"), param: Some(param.into()), message: format!("Missing required param: {param}.") }
    }

    pub fn no_such(what: &str, id: &str, param: &str) -> StripeError {
        StripeError { status: 404, kind: "invalid_request_error", code: Some("resource_missing"), param: Some(param.into()), message: format!("No such {what}: '{id}'") }
    }

    pub fn auth(key: &str) -> StripeError {
        let shown: String = key.chars().take(8).collect();
        let message = if key.is_empty() {
            "You did not provide an API key. You need to provide your API key in the Authorization header, using Bearer auth (e.g. 'Authorization: Bearer YOUR_SECRET_KEY').".to_string()
        } else {
            format!("Invalid API Key provided: {shown}***")
        };
        StripeError { status: 401, kind: "invalid_request_error", code: None, param: None, message }
    }

    pub fn body(&self) -> Value {
        let mut e = json!({ "type": self.kind, "message": self.message });
        if let Some(c) = self.code {
            e["code"] = json!(c); // panic-ok: IndexMut on a JSON object inserts the key, never panics
            e["doc_url"] = json!(format!("https://stripe.com/docs/error-codes/{}", c.replace('_', "-"))); // panic-ok: IndexMut on a JSON object inserts the key, never panics
        }
        if let Some(p) = &self.param {
            e["param"] = json!(p); // panic-ok: IndexMut on a JSON object inserts the key, never panics
        }
        json!({ "error": e })
    }
}

// ---------- the objects ----------

#[derive(Clone, Debug)]
pub struct Price {
    pub id: String,
    pub amount: i64,
    pub currency: String,
    pub recurring: bool,
    pub product: String,
    /// Made up on first use (the app named a price the simulator did not know).
    pub made_up: bool,
}

#[derive(Clone, Debug)]
pub struct Customer {
    pub id: String,
    pub email: Option<String>,
    pub created: i64,
}

#[derive(Clone, Debug)]
pub struct Session {
    pub id: String,
    pub mode: String,
    /// open, complete or expired.
    pub status: String,
    /// unpaid or paid.
    pub payment_status: String,
    pub customer: Option<String>,
    pub customer_email: Option<String>,
    pub client_reference_id: Option<String>,
    pub price: String,
    pub quantity: i64,
    pub amount_total: i64,
    pub currency: String,
    pub success_url: String,
    pub cancel_url: String,
    pub metadata: BTreeMap<String, String>,
    pub subscription_metadata: BTreeMap<String, String>,
    pub payment_intent_metadata: BTreeMap<String, String>,
    pub subscription: Option<String>,
    pub payment_intent: Option<String>,
    pub created: i64,
    pub url: String,
    /// What the hosted page says after a declined card.
    pub declined: bool,
}

#[derive(Clone, Debug)]
pub struct Subscription {
    pub id: String,
    pub customer: String,
    /// active, past_due, canceled.
    pub status: String,
    pub price: String,
    pub quantity: i64,
    pub metadata: BTreeMap<String, String>,
    pub period_start: i64,
    pub period_end: i64,
    pub cancel_at_period_end: bool,
    pub created: i64,
    pub canceled_at: Option<i64>,
    /// The next renewal's payment fails.
    pub fail_next: bool,
    pub latest_invoice: Option<String>,
}

/// An invoice (a subscription's) or a one-off payment (Checkout in payment
/// mode): both are a payment intent with a charge.
#[derive(Clone, Debug)]
pub struct Payment {
    /// `in_…` for an invoice, else the payment intent's id.
    pub id: String,
    pub invoice: bool,
    pub customer: String,
    pub subscription: Option<String>,
    pub session: Option<String>,
    pub price: String,
    pub amount: i64,
    pub currency: String,
    /// paid, failed or refunded (partly refunded stays paid).
    pub status: String,
    pub refunded: i64,
    pub payment_intent: String,
    pub charge: String,
    pub created: i64,
    /// subscription_create, subscription_cycle, or "" for a one-off.
    pub billing_reason: String,
    pub metadata: BTreeMap<String, String>,
}

/// An event, as it is sent to the webhook.
#[derive(Clone, Debug)]
pub struct Event {
    pub id: String,
    pub kind: String,
    pub created: i64,
    pub object: Value,
}

impl Event {
    pub fn body(&self) -> Value {
        json!({
            "id": self.id, "object": "event", "api_version": API_VERSION, "created": self.created,
            "data": { "object": self.object }, "livemode": false, "pending_webhooks": 1,
            "request": { "id": null, "idempotency_key": null }, "type": self.kind,
        })
    }
}

/// One line of the log: a call to the API, a hosted page's action, or a
/// webhook delivered to the app.
#[derive(Clone, Debug)]
pub struct Entry {
    pub id: i64,
    pub at: i64,
    /// `api`, `page` or `webhook`.
    pub kind: &'static str,
    /// `POST /v1/checkout/sessions`, `checkout.session.completed`, `Pay cs_…`.
    pub what: String,
    /// The HTTP status: the simulator's answer (api), the app's (webhook); 0 for a page action.
    pub status: u16,
    pub summary: String,
    pub request: String,
    pub response: String,
    /// A webhook's event id (for Send again).
    pub event: Option<String>,
    /// A webhook sent with a broken signature.
    pub tampered: bool,
}

fn clip(s: &str) -> String {
    if s.len() <= BODY_MAX {
        return s.to_string();
    }
    let mut end = BODY_MAX;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n… ({} more bytes not kept)", &s[..end], s.len() - end) // panic-ok: end is a char boundary, checked just above
}

fn meta_json(m: &BTreeMap<String, String>) -> Value {
    Value::Object(m.iter().map(|(k, v)| (k.clone(), json!(v))).collect())
}

#[derive(Default)]
struct Inner {
    prices: Vec<Price>,
    customers: Vec<Customer>,
    sessions: Vec<Session>,
    subscriptions: Vec<Subscription>,
    payments: Vec<Payment>,
    events: VecDeque<Event>,
    log: VecDeque<Entry>,
    next_id: u64,
    next_entry: i64,
    seeded: bool,
    /// `Idempotency-Key` → the customer that request made, so a retried
    /// or racing `POST /v1/customers` returns the same one (#394).
    idempotent_customers: BTreeMap<String, String>,
}

/// What a Pay, a renewal or another action did: the events to deliver and,
/// for the hosted pages, where the browser goes next.
#[derive(Debug, Default)]
pub struct Outcome {
    pub events: Vec<Event>,
    pub redirect: Option<String>,
    pub note: String,
}

/// The simulator's state for this server run (`AppState::dev_payments`).
#[derive(Default)]
pub struct Simulator(Mutex<Inner>);

impl Inner {
    fn seed(&mut self) {
        if self.seeded {
            return;
        }
        self.seeded = true;
        self.prices.push(Price { id: DEV_PRICE_ID.into(), amount: DEV_PRICE_AMOUNT, currency: DEV_PRICE_CURRENCY.into(), recurring: true, product: format!("{} Plus", crate::APP_NAME), made_up: false });
        self.prices.push(Price { id: DEV_PRICE_ID_BASIC.into(), amount: DEV_PRICE_AMOUNT_BASIC, currency: DEV_PRICE_CURRENCY.into(), recurring: true, product: format!("{} Basic", crate::APP_NAME), made_up: false });
        for (id, amount, currency, product) in DEV_ONE_OFF_PRICES {
            self.prices.push(Price { id: id.into(), amount, currency: currency.into(), recurring: false, product: product.into(), made_up: false });
        }
    }

    /// `cus_dev3k9…`: a prefix and 14 characters unique to this run.
    fn new_id(&mut self, prefix: &str) -> String {
        self.next_id += 1;
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let digest = Sha256::new().chain_update(nanos.to_le_bytes()).chain_update(self.next_id.to_le_bytes()).chain_update(prefix.as_bytes()).finalize();
        const ALPHABET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
        let tail: String = digest.iter().take(14).map(|b| ALPHABET[(*b as usize) % ALPHABET.len()] as char).collect(); // panic-ok: the index is taken modulo the alphabet length
        format!("{prefix}_dev{tail}")
    }

    fn event(&mut self, kind: &str, object: Value, now: i64) -> Event {
        let e = Event { id: self.new_id("evt"), kind: kind.into(), created: now, object };
        self.events.push_front(e.clone());
        self.events.truncate(LOG_MAX);
        e
    }

    fn price(&self, id: &str) -> Option<&Price> {
        self.prices.iter().find(|p| p.id == id)
    }

    fn customer(&self, id: &str) -> Option<&Customer> {
        self.customers.iter().find(|c| c.id == id)
    }

    fn sub(&mut self, id: &str) -> Option<&mut Subscription> {
        self.subscriptions.iter_mut().find(|s| s.id == id)
    }

    /// A customer the app names that this run does not know (it was made
    /// before a restart): made again, as Stripe would still have it.
    fn ensure_customer(&mut self, id: &str, now: i64) -> Customer {
        if let Some(c) = self.customer(id) {
            return c.clone();
        }
        let c = Customer { id: id.into(), email: None, created: now };
        self.customers.push(c.clone());
        c
    }

    fn new_customer(&mut self, email: Option<String>, now: i64) -> (Customer, Event) {
        let c = Customer { id: self.new_id("cus"), email, created: now };
        self.customers.push(c.clone());
        let e = self.event("customer.created", customer_json(&c), now);
        (c, e)
    }

    fn sub_json(&self, s: &Subscription) -> Value {
        let price = self.price(&s.price).cloned();
        let price_json = price.as_ref().map(price_json).unwrap_or(json!({ "id": s.price, "object": "price" }));
        json!({
            "id": s.id, "object": "subscription", "customer": s.customer, "status": s.status,
            "metadata": meta_json(&s.metadata), "cancel_at_period_end": s.cancel_at_period_end,
            "cancel_at": if s.cancel_at_period_end { json!(s.period_end) } else { Value::Null },
            "canceled_at": s.canceled_at, "ended_at": if s.status == "canceled" { json!(s.canceled_at) } else { Value::Null },
            "created": s.created, "start_date": s.created, "currency": price.as_ref().map(|p| p.currency.clone()).unwrap_or_else(|| "eur".into()),
            "current_period_start": s.period_start, "current_period_end": s.period_end,
            "latest_invoice": s.latest_invoice, "livemode": false, "collection_method": "charge_automatically",
            "items": { "object": "list", "has_more": false, "total_count": 1, "url": format!("/v1/subscription_items?subscription={}", s.id), "data": [{
                "id": format!("si_{}", s.id.trim_start_matches("sub_")), "object": "subscription_item", "subscription": s.id,
                "price": price_json, "quantity": s.quantity,
                "current_period_start": s.period_start, "current_period_end": s.period_end,
            }] },
        })
    }

    fn session_json(&self, s: &Session) -> Value {
        let price = self.price(&s.price).cloned();
        json!({
            "id": s.id, "object": "checkout.session", "mode": s.mode, "status": s.status, "payment_status": s.payment_status,
            "customer": s.customer, "customer_email": s.customer_email, "client_reference_id": s.client_reference_id,
            "customer_details": s.customer_email.as_ref().map(|e| json!({ "email": e })),
            "amount_total": s.amount_total, "amount_subtotal": s.amount_total, "currency": s.currency,
            "success_url": s.success_url, "cancel_url": s.cancel_url, "url": if s.status == "open" { json!(s.url) } else { Value::Null },
            "metadata": meta_json(&s.metadata), "subscription": s.subscription, "payment_intent": s.payment_intent,
            "invoice": self.payments.iter().find(|p| p.invoice && p.session.as_deref() == Some(s.id.as_str())).map(|p| p.id.clone()),
            "created": s.created, "expires_at": s.created + 86_400, "livemode": false,
            "line_items": { "object": "list", "has_more": false, "data": [{ "object": "item", "quantity": s.quantity, "amount_total": s.amount_total, "currency": s.currency, "price": price.as_ref().map(price_json), "description": price.map(|p| p.product) }] },
        })
    }

    /// A payment's invoice object (for invoices) or its payment intent.
    fn payment_json(&self, p: &Payment) -> Value {
        if p.invoice {
            return json!({
                "id": p.id, "object": "invoice", "customer": p.customer, "subscription": p.subscription,
                "parent": p.subscription.as_ref().map(|s| json!({ "type": "subscription_details", "subscription_details": { "subscription": s } })),
                "status": if p.status == "failed" { "open" } else { "paid" }, "paid": p.status != "failed",
                "amount_due": p.amount, "amount_paid": if p.status == "failed" { 0 } else { p.amount },
                "amount_remaining": if p.status == "failed" { p.amount } else { 0 }, "total": p.amount, "currency": p.currency,
                "billing_reason": p.billing_reason, "payment_intent": p.payment_intent, "charge": p.charge,
                "attempt_count": 1, "attempted": true, "created": p.created, "livemode": false,
                "lines": { "object": "list", "has_more": false, "data": [{ "object": "line_item", "amount": p.amount, "currency": p.currency, "pricing": { "price_details": { "price": p.price } }, "price": { "id": p.price } }] },
            });
        }
        self.payment_intent_json(p)
    }

    fn payment_intent_json(&self, p: &Payment) -> Value {
        let status = if p.status == "failed" { "requires_payment_method" } else { "succeeded" };
        let mut v = json!({
            "id": p.payment_intent, "object": "payment_intent", "amount": p.amount, "amount_received": if p.status == "failed" { 0 } else { p.amount },
            "currency": p.currency, "customer": p.customer, "status": status, "latest_charge": p.charge,
            "invoice": if p.invoice { json!(p.id) } else { Value::Null }, "metadata": meta_json(&p.metadata), "created": p.created, "livemode": false,
        });
        if p.status == "failed" {
            v["last_payment_error"] = json!({ "type": "card_error", "code": "card_declined", "decline_code": "generic_decline", "message": "Your card was declined." }); // panic-ok: IndexMut on a JSON object inserts the key, never panics
        }
        v
    }

    fn charge_json(&self, p: &Payment) -> Value {
        json!({
            "id": p.charge, "object": "charge", "amount": p.amount, "amount_captured": if p.status == "failed" { 0 } else { p.amount },
            "amount_refunded": p.refunded, "refunded": p.refunded >= p.amount && p.amount > 0, "currency": p.currency,
            "customer": p.customer, "payment_intent": p.payment_intent, "invoice": if p.invoice { json!(p.id) } else { Value::Null },
            "paid": p.status != "failed", "status": if p.status == "failed" { "failed" } else { "succeeded" },
            "failure_code": if p.status == "failed" { json!("card_declined") } else { Value::Null },
            "metadata": meta_json(&p.metadata), "created": p.created, "livemode": false,
            "payment_method_details": { "type": "card", "card": { "brand": "visa", "last4": if p.status == "failed" { "0002" } else { "4242" } } },
        })
    }

    /// Charge a payment: an invoice for `sub`'s next period, or a one-off.
    #[allow(clippy::too_many_arguments)]
    fn charge(&mut self, invoice: bool, customer: &str, sub: Option<&str>, session: Option<&str>, price: &str, amount: i64, currency: &str, ok: bool, reason: &str, metadata: BTreeMap<String, String>, now: i64) -> Payment {
        let pi = self.new_id("pi");
        let p = Payment {
            id: if invoice { self.new_id("in") } else { pi.clone() },
            invoice,
            customer: customer.into(),
            subscription: sub.map(str::to_string),
            session: session.map(str::to_string),
            price: price.into(),
            amount,
            currency: currency.into(),
            status: if ok { "paid".into() } else { "failed".into() },
            refunded: 0,
            payment_intent: pi,
            charge: self.new_id("ch"),
            created: now,
            billing_reason: reason.into(),
            metadata,
        };
        self.payments.push(p.clone());
        p
    }

    /// The events for a payment that went through or failed.
    fn payment_events(&mut self, p: &Payment, now: i64) -> Vec<Event> {
        let ok = p.status != "failed";
        let mut out = Vec::new();
        let pi = self.payment_intent_json(p);
        let charge = self.charge_json(p);
        out.push(self.event(if ok { "charge.succeeded" } else { "charge.failed" }, charge, now));
        out.push(self.event(if ok { "payment_intent.succeeded" } else { "payment_intent.payment_failed" }, pi, now));
        if p.invoice {
            let inv = self.payment_json(p);
            if ok {
                out.push(self.event("invoice.paid", inv.clone(), now));
                out.push(self.event("invoice.payment_succeeded", inv, now));
            } else {
                out.push(self.event("invoice.payment_failed", inv, now));
            }
        }
        out
    }

    fn sub_event(&mut self, kind: &str, id: &str, now: i64) -> Event {
        let s = self.subscriptions.iter().find(|s| s.id == id).cloned().expect("subscription exists"); // panic-ok: the id was looked up just before, under the same lock
        let obj = self.sub_json(&s);
        self.event(kind, obj, now)
    }
}

fn customer_json(c: &Customer) -> Value {
    json!({ "id": c.id, "object": "customer", "email": c.email, "created": c.created, "livemode": false, "metadata": {} })
}

fn price_json(p: &Price) -> Value {
    json!({
        "id": p.id, "object": "price", "active": true, "currency": p.currency, "unit_amount": p.amount,
        "type": if p.recurring { "recurring" } else { "one_time" },
        "recurring": if p.recurring { json!({ "interval": "month", "interval_count": 1, "usage_type": "licensed" }) } else { Value::Null },
        "product": format!("prod_{}", p.id.trim_start_matches("price_")), "nickname": p.product, "livemode": false,
    })
}

/// A form body's fields, in order, as Stripe reads them.
pub struct Fields<'a>(pub &'a [(String, String)]);

impl Fields<'_> {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str()).filter(|v| !v.is_empty())
    }

    /// `prefix[name]=value` pairs: a metadata hash.
    pub fn hash(&self, prefix: &str) -> BTreeMap<String, String> {
        let open = format!("{prefix}[");
        self.0
            .iter()
            .filter_map(|(k, v)| k.strip_prefix(&open).and_then(|r| r.strip_suffix(']')).filter(|n| !n.contains('[')).map(|n| (n.to_string(), v.clone())))
            .collect()
    }

    fn known(&self, allowed: &[&str]) -> Result<(), StripeError> {
        for (k, _) in self.0 {
            let root = k.split('[').next().unwrap_or(k);
            if !allowed.contains(&root) {
                return Err(StripeError { status: 400, kind: "invalid_request_error", code: Some("parameter_unknown"), param: Some(k.clone()), message: format!("Received unknown parameter: {k}") });
            }
        }
        Ok(())
    }
}

/// A summary row of what the simulator holds, for the page.
pub struct Snapshot {
    pub customers: Vec<Customer>,
    pub sessions: Vec<Session>,
    pub subscriptions: Vec<Subscription>,
    pub payments: Vec<Payment>,
    pub prices: Vec<Price>,
    pub events: Vec<Event>,
}

impl Simulator {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        let mut g = self.0.lock().unwrap_or_else(|p| p.into_inner());
        g.seed();
        g
    }

    pub fn snapshot(&self) -> Snapshot {
        let g = self.lock();
        fn rev<T: Clone>(v: &[T]) -> Vec<T> {
            v.iter().rev().cloned().collect()
        }
        Snapshot {
            customers: rev(&g.customers),
            sessions: rev(&g.sessions),
            subscriptions: rev(&g.subscriptions),
            payments: rev(&g.payments),
            prices: g.prices.clone(),
            events: g.events.iter().cloned().collect(),
        }
    }

    /// Forget everything (the prices go back to the dev ones).
    pub fn reset(&self) {
        let mut g = self.0.lock().unwrap_or_else(|p| p.into_inner());
        *g = Inner::default();
    }

    // ----- the log -----

    pub fn log_entry(&self, mut e: Entry) -> i64 {
        let mut g = self.lock();
        g.next_entry += 1;
        e.id = g.next_entry;
        e.request = clip(&e.request);
        e.response = clip(&e.response);
        let id = e.id;
        g.log.push_front(e);
        g.log.truncate(LOG_MAX);
        id
    }

    pub fn log(&self) -> Vec<Entry> {
        self.lock().log.iter().cloned().collect()
    }

    pub fn entry(&self, id: i64) -> Option<Entry> {
        self.lock().log.iter().find(|e| e.id == id).cloned()
    }

    pub fn clear_log(&self) {
        self.lock().log.clear();
    }

    pub fn event(&self, id: &str) -> Option<Event> {
        self.lock().events.iter().find(|e| e.id == id).cloned()
    }

    pub fn session(&self, id: &str) -> Option<Session> {
        self.lock().sessions.iter().find(|s| s.id == id).cloned()
    }

    pub fn price(&self, id: &str) -> Option<Price> {
        self.lock().price(id).cloned()
    }

    pub fn customer(&self, id: &str) -> Option<Customer> {
        self.lock().customer(id).cloned()
    }

    pub fn subscriptions_of(&self, customer: &str) -> Vec<Subscription> {
        self.lock().subscriptions.iter().filter(|s| s.customer == customer).rev().cloned().collect()
    }

    // ----- the API -----

    /// `POST /v1/customers`: the account's one customer, made before its
    /// first Checkout. The same `Idempotency-Key` returns the same customer.
    pub fn create_customer(&self, form: &[(String, String)], idempotency_key: Option<&str>, now: i64) -> Result<(Value, Vec<Event>), StripeError> {
        let f = Fields(form);
        f.known(&["email", "metadata", "name", "expand"])?;
        let mut g = self.lock();
        if let Some(c) = idempotency_key.and_then(|k| g.idempotent_customers.get(k).cloned()).and_then(|id| g.customer(&id).cloned()) {
            return Ok((customer_json(&c), Vec::new()));
        }
        let (c, e) = g.new_customer(f.get("email").map(str::to_string), now);
        if let Some(k) = idempotency_key {
            g.idempotent_customers.insert(k.to_string(), c.id.clone());
        }
        Ok((customer_json(&c), vec![e]))
    }

    /// `POST /v1/checkout/sessions`. `hosted` is where the hosted page is
    /// (`<public URL>/dev/payments/checkout`).
    pub fn create_checkout_session(&self, form: &[(String, String)], hosted: &str, now: i64) -> Result<Value, StripeError> {
        let f = Fields(form);
        f.known(&[
            "mode", "line_items", "success_url", "cancel_url", "client_reference_id", "customer", "customer_email", "metadata",
            "subscription_data", "payment_intent_data", "allow_promotion_codes", "billing_address_collection", "locale", "expand",
            "payment_method_types", "customer_creation", "automatic_tax", "ui_mode",
        ])?;
        let mode = f.get("mode").ok_or_else(|| StripeError::missing_param("mode"))?.to_string();
        if !["subscription", "payment"].contains(&mode.as_str()) {
            return Err(StripeError::invalid("mode", format!("Invalid mode: must be one of payment, setup, or subscription (the dev simulator does payment and subscription; got {mode})")));
        }
        let price_id = f.get("line_items[0][price]").ok_or_else(|| StripeError::missing_param("line_items[0][price]"))?.to_string(); // panic-ok: a form field name or message, not indexing
        let quantity = match f.get("line_items[0][quantity]") { // panic-ok: a form field name or message, not indexing
            None => 1,
            Some(q) => q.parse::<i64>().ok().filter(|q| *q > 0).ok_or_else(|| StripeError::invalid("line_items[0][quantity]", "Invalid integer: quantity must be at least 1"))?, // panic-ok: a form field name or message, not indexing
        };
        if f.0.iter().any(|(k, _)| k.starts_with("line_items[1]")) { // panic-ok: a form field name or message, not indexing
            return Err(StripeError::invalid("line_items[1]", "The dev simulator takes one line item.")); // panic-ok: a form field name or message, not indexing
        }
        let success_url = f.get("success_url").ok_or_else(|| StripeError::missing_param("success_url"))?.to_string();
        let cancel_url = f.get("cancel_url").unwrap_or("").to_string();
        if f.get("customer").is_some() && f.get("customer_email").is_some() {
            return Err(StripeError::invalid("customer_email", "You may only specify one of these parameters: customer, customer_email."));
        }
        let mut g = self.lock();
        let price = match g.price(&price_id).cloned() {
            Some(p) => p,
            None => {
                let p = Price { id: price_id.clone(), amount: MADE_UP_AMOUNT, currency: "eur".into(), recurring: mode == "subscription", product: format!("{price_id} (made up by the dev simulator)"), made_up: true };
                g.prices.push(p.clone());
                p
            }
        };
        match (mode.as_str(), price.recurring) {
            ("subscription", false) => return Err(StripeError::invalid("line_items[0][price]", "You must provide at least one recurring price in `subscription` mode when using prices.")), // panic-ok: a form field name or message, not indexing
            ("payment", true) => return Err(StripeError::invalid("line_items[0][price]", "You specified `payment` mode but passed a recurring price. Either switch to `subscription` mode or use only one-time prices.")), // panic-ok: a form field name or message, not indexing
            _ => {}
        }
        let customer = match f.get("customer") {
            Some(c) => Some(g.ensure_customer(c, now).id),
            None => None,
        };
        let id = g.new_id("cs_test");
        let s = Session {
            url: format!("{}/{id}", hosted.trim_end_matches('/')),
            id,
            mode,
            status: "open".into(),
            payment_status: "unpaid".into(),
            customer_email: f.get("customer_email").map(str::to_string).or_else(|| customer.as_deref().and_then(|c| g.customer(c)).and_then(|c| c.email.clone())),
            customer,
            client_reference_id: f.get("client_reference_id").map(str::to_string),
            price: price.id.clone(),
            quantity,
            amount_total: price.amount * quantity,
            currency: price.currency.clone(),
            success_url,
            cancel_url,
            metadata: f.hash("metadata"),
            subscription_metadata: f.hash("subscription_data[metadata]"), // panic-ok: a form field name or message, not indexing
            payment_intent_metadata: f.hash("payment_intent_data[metadata]"), // panic-ok: a form field name or message, not indexing
            subscription: None,
            payment_intent: None,
            created: now,
            declined: false,
        };
        let v = g.session_json(&s);
        g.sessions.push(s);
        Ok(v)
    }

    pub fn retrieve(&self, kind: &str, id: &str) -> Result<Value, StripeError> {
        let g = self.lock();
        let found = match kind {
            "checkout/sessions" => g.sessions.iter().find(|s| s.id == id).map(|s| g.session_json(s)),
            "subscriptions" => g.subscriptions.iter().find(|s| s.id == id).map(|s| g.sub_json(s)),
            "customers" => g.customer(id).map(customer_json),
            "invoices" => g.payments.iter().find(|p| p.invoice && p.id == id).map(|p| g.payment_json(p)),
            "payment_intents" => g.payments.iter().find(|p| p.payment_intent == id).map(|p| g.payment_intent_json(p)),
            "charges" => g.payments.iter().find(|p| p.charge == id).map(|p| g.charge_json(p)),
            "prices" => g.price(id).map(price_json),
            _ => None,
        };
        let what = match kind {
            "checkout/sessions" => "checkout.session",
            "subscriptions" => "subscription",
            "customers" => "customer",
            "invoices" => "invoice",
            "payment_intents" => "payment_intent",
            "charges" => "charge",
            _ => "price",
        };
        found.ok_or_else(|| StripeError::no_such(what, id, "id"))
    }

    /// `POST /v1/billing_portal/sessions`.
    pub fn create_portal_session(&self, form: &[(String, String)], hosted: &str, now: i64) -> Result<Value, StripeError> {
        let f = Fields(form);
        f.known(&["customer", "return_url", "configuration", "locale", "flow_data", "expand"])?;
        let customer = f.get("customer").ok_or_else(|| StripeError::missing_param("customer"))?.to_string();
        let mut g = self.lock();
        g.ensure_customer(&customer, now);
        let id = g.new_id("bps");
        // The portal page finds its customer and return URL from the id.
        let return_url = f.get("return_url").unwrap_or("").to_string();
        g.sessions.push(Session {
            id: id.clone(),
            mode: "portal".into(),
            status: "open".into(),
            payment_status: String::new(),
            customer: Some(customer.clone()),
            customer_email: None,
            client_reference_id: None,
            price: String::new(),
            quantity: 0,
            amount_total: 0,
            currency: String::new(),
            success_url: return_url.clone(),
            cancel_url: return_url.clone(),
            metadata: BTreeMap::new(),
            subscription_metadata: BTreeMap::new(),
            payment_intent_metadata: BTreeMap::new(),
            subscription: None,
            payment_intent: None,
            created: now,
            url: format!("{}/{id}", hosted.trim_end_matches('/')),
            declined: false,
        });
        let url = format!("{}/{id}", hosted.trim_end_matches('/'));
        Ok(json!({ "id": id, "object": "billing_portal.session", "customer": customer, "return_url": return_url, "url": url, "created": now, "livemode": false }))
    }

    /// `POST /v1/subscriptions/{id}`: `cancel_at_period_end`, `metadata[…]`.
    pub fn update_subscription(&self, id: &str, form: &[(String, String)], now: i64) -> Result<(Value, Vec<Event>), StripeError> {
        let f = Fields(form);
        f.known(&["cancel_at_period_end", "metadata", "expand", "proration_behavior"])?;
        let mut g = self.lock();
        let s = g.sub(id).ok_or_else(|| StripeError::no_such("subscription", id, "id"))?;
        if s.status == "canceled" {
            return Err(StripeError::invalid("id", "A canceled subscription can only update its cancellation_details and metadata."));
        }
        if let Some(v) = f.get("cancel_at_period_end") {
            s.cancel_at_period_end = v == "true";
        }
        for (k, v) in f.hash("metadata") {
            s.metadata.insert(k, v);
        }
        let e = g.sub_event("customer.subscription.updated", id, now);
        Ok((e.object.clone(), vec![e]))
    }

    /// `DELETE /v1/subscriptions/{id}`: cancel now.
    pub fn delete_subscription(&self, id: &str, now: i64) -> Result<(Value, Vec<Event>), StripeError> {
        let out = self.cancel_now(id, now).map_err(|_| StripeError::no_such("subscription", id, "id"))?;
        let obj = out.events.first().map(|e| e.object.clone()).unwrap_or(Value::Null);
        Ok((obj, out.events))
    }

    /// `POST /v1/refunds`: `payment_intent` or `charge`, `amount` (default: what is left).
    pub fn create_refund(&self, form: &[(String, String)], now: i64) -> Result<(Value, Vec<Event>), StripeError> {
        let f = Fields(form);
        f.known(&["payment_intent", "charge", "amount", "reason", "metadata", "expand"])?;
        let target = f.get("payment_intent").or(f.get("charge")).ok_or_else(|| StripeError::missing_param("payment_intent"))?.to_string();
        let amount = match f.get("amount") {
            Some(a) => Some(a.parse::<i64>().ok().filter(|a| *a > 0).ok_or_else(|| StripeError::invalid("amount", "Invalid integer"))?),
            None => None,
        };
        let id = {
            let g = self.lock();
            g.payments.iter().find(|p| p.payment_intent == target || p.charge == target).map(|p| p.id.clone())
        };
        let id = id.ok_or_else(|| StripeError::no_such("payment_intent", &target, "payment_intent"))?;
        let out = self.refund(&id, amount, now).map_err(|m| StripeError { status: 400, kind: "invalid_request_error", code: Some("charge_already_refunded"), param: None, message: m })?;
        let refund = out.events.iter().find(|e| e.kind == "refund.created").map(|e| e.object.clone()).unwrap_or(Value::Null);
        Ok((refund, out.events))
    }

    // ----- the hosted pages and the Payments section -----

    /// **Pay** on the Checkout page. `ok: false` is **Fail**: the card is
    /// declined, the session stays open and the page says so.
    pub fn pay(&self, session_id: &str, ok: bool, now: i64) -> Result<Outcome, String> {
        let mut g = self.lock();
        let Some(s) = g.sessions.iter().find(|s| s.id == session_id && s.mode != "portal").cloned() else {
            return Err(format!("no Checkout session {session_id}"));
        };
        if s.status != "open" {
            return Err(format!("{session_id} is {}: start again from the app", s.status));
        }
        let mut events = Vec::new();
        let customer = match &s.customer {
            Some(c) => g.ensure_customer(c, now),
            None => {
                let (c, e) = g.new_customer(s.customer_email.clone(), now);
                events.push(e);
                c
            }
        };
        if !ok {
            let p = g.charge(false, &customer.id, None, Some(&s.id), &s.price, s.amount_total, &s.currency, false, "", s.payment_intent_metadata.clone(), now);
            // A declined card in Checkout leaves no payment behind; only the failed intent's events.
            g.payments.retain(|x| x.id != p.id);
            let pi = g.payment_intent_json(&p);
            events.push(g.event("payment_intent.payment_failed", pi, now));
            if let Some(x) = g.sessions.iter_mut().find(|x| x.id == s.id) {
                x.declined = true;
                x.customer = Some(customer.id.clone());
            }
            return Ok(Outcome { events, redirect: None, note: format!("{session_id}: card declined") });
        }
        let (sub_id, pi_id);
        if s.mode == "subscription" {
            let id = g.new_id("sub");
            let sub = Subscription {
                id: id.clone(),
                customer: customer.id.clone(),
                status: "active".into(),
                price: s.price.clone(),
                quantity: s.quantity,
                metadata: s.subscription_metadata.clone(),
                period_start: now,
                period_end: now + PERIOD_SECS,
                cancel_at_period_end: false,
                created: now,
                canceled_at: None,
                fail_next: false,
                latest_invoice: None,
            };
            g.subscriptions.push(sub);
            let p = g.charge(true, &customer.id, Some(&id), Some(&s.id), &s.price, s.amount_total, &s.currency, true, "subscription_create", BTreeMap::new(), now);
            if let Some(x) = g.sub(&id) {
                x.latest_invoice = Some(p.id.clone());
            }
            events.push(g.sub_event("customer.subscription.created", &id, now));
            events.extend(g.payment_events(&p, now));
            sub_id = Some(id);
            pi_id = None;
        } else {
            let p = g.charge(false, &customer.id, None, Some(&s.id), &s.price, s.amount_total, &s.currency, true, "", s.payment_intent_metadata.clone(), now);
            events.extend(g.payment_events(&p, now));
            sub_id = None;
            pi_id = Some(p.payment_intent.clone());
        }
        let done = {
            let x = g.sessions.iter_mut().find(|x| x.id == s.id).expect("session exists"); // panic-ok: the id was looked up just before, under the same lock
            x.status = "complete".into();
            x.payment_status = "paid".into();
            x.customer = Some(customer.id.clone());
            if x.customer_email.is_none() {
                x.customer_email = customer.email.clone();
            }
            x.subscription = sub_id;
            x.payment_intent = pi_id;
            x.declined = false;
            x.clone()
        };
        let obj = g.session_json(&done);
        events.push(g.event("checkout.session.completed", obj, now));
        let redirect = done.success_url.replace("{CHECKOUT_SESSION_ID}", &done.id);
        Ok(Outcome { events, redirect: Some(redirect), note: format!("{session_id}: paid {}", money(done.amount_total, &done.currency)) })
    }

    /// **Cancel** on the Checkout page: back to the app; the session stays open.
    pub fn abandon(&self, session_id: &str) -> Result<Outcome, String> {
        let g = self.lock();
        let s = g.sessions.iter().find(|s| s.id == session_id).ok_or_else(|| format!("no Checkout session {session_id}"))?;
        let to = if s.cancel_url.is_empty() { s.success_url.clone() } else { s.cancel_url.clone() };
        Ok(Outcome { events: Vec::new(), redirect: Some(to), note: format!("{session_id}: cancelled on the Checkout page") })
    }

    /// The next period's payment: paid, or failed when "Fail the next
    /// payment" is set (the subscription goes past_due). A subscription set
    /// to cancel at the period end ends instead.
    pub fn renew(&self, sub_id: &str, now: i64) -> Result<Outcome, String> {
        let mut g = self.lock();
        let s = g.sub(sub_id).ok_or_else(|| format!("no subscription {sub_id}"))?.clone();
        if s.status == "canceled" {
            return Err(format!("{sub_id} is canceled"));
        }
        if s.cancel_at_period_end {
            drop(g);
            let mut out = self.cancel_now(sub_id, now)?;
            out.note = format!("{sub_id}: the period ended; it was set to cancel, so it is canceled");
            return Ok(out);
        }
        let price = g.price(&s.price).cloned();
        let (amount, currency) = price.map(|p| (p.amount * s.quantity, p.currency)).unwrap_or((MADE_UP_AMOUNT, "eur".into()));
        let ok = !s.fail_next;
        let p = g.charge(true, &s.customer, Some(sub_id), None, &s.price, amount, &currency, ok, "subscription_cycle", BTreeMap::new(), now);
        {
            let x = g.sub(sub_id).expect("subscription exists"); // panic-ok: the id was looked up just before, under the same lock
            x.fail_next = false;
            x.latest_invoice = Some(p.id.clone());
            if ok {
                x.status = "active".into();
                x.period_start = now;
                x.period_end = now + PERIOD_SECS;
            } else {
                x.status = "past_due".into();
            }
        }
        let mut events = g.payment_events(&p, now);
        events.push(g.sub_event("customer.subscription.updated", sub_id, now));
        let note = if ok { format!("{sub_id}: renewed, {} paid", money(amount, &currency)) } else { format!("{sub_id}: the renewal's payment failed; past due") };
        Ok(Outcome { events, redirect: None, note })
    }

    pub fn set_fail_next(&self, sub_id: &str, on: bool) -> Result<Outcome, String> {
        let mut g = self.lock();
        let s = g.sub(sub_id).ok_or_else(|| format!("no subscription {sub_id}"))?;
        s.fail_next = on;
        let note = if on { format!("{sub_id}: the next payment will fail") } else { format!("{sub_id}: the next payment goes through") };
        Ok(Outcome { note, ..Default::default() })
    }

    /// Mark a failed invoice paid (the learner fixed the card): its
    /// subscription is active again with a new period.
    pub fn mark_paid(&self, payment_id: &str, now: i64) -> Result<Outcome, String> {
        let mut g = self.lock();
        let p = g.payments.iter_mut().find(|p| p.id == payment_id).ok_or_else(|| format!("no payment {payment_id}"))?;
        if p.status != "failed" {
            return Err(format!("{payment_id} is {}, not failed", p.status));
        }
        p.status = "paid".into();
        p.created = now;
        let p = p.clone();
        let mut events = g.payment_events(&p, now);
        if let Some(sid) = &p.subscription {
            if let Some(x) = g.sub(sid) {
                if x.status == "past_due" {
                    x.status = "active".into();
                    x.period_start = now;
                    x.period_end = now + PERIOD_SECS;
                }
            }
            if g.subscriptions.iter().any(|s| &s.id == sid && s.status != "canceled") {
                let sid = sid.clone();
                events.push(g.sub_event("customer.subscription.updated", &sid, now));
            }
        }
        Ok(Outcome { events, redirect: None, note: format!("{payment_id}: paid") })
    }

    /// Cancel at once (`customer.subscription.deleted`).
    pub fn cancel_now(&self, sub_id: &str, now: i64) -> Result<Outcome, String> {
        let mut g = self.lock();
        let s = g.sub(sub_id).ok_or_else(|| format!("no subscription {sub_id}"))?;
        if s.status == "canceled" {
            return Err(format!("{sub_id} is already canceled"));
        }
        s.status = "canceled".into();
        s.canceled_at = Some(now);
        s.cancel_at_period_end = false;
        let e = g.sub_event("customer.subscription.deleted", sub_id, now);
        Ok(Outcome { events: vec![e], redirect: None, note: format!("{sub_id}: canceled") })
    }

    /// Cancel at the period end, or take that back (the portal's buttons).
    pub fn cancel_at_period_end(&self, sub_id: &str, on: bool, now: i64) -> Result<Outcome, String> {
        let form = [("cancel_at_period_end".to_string(), on.to_string())];
        let (_, events) = self.update_subscription(sub_id, &form, now).map_err(|e| e.message)?;
        let note = if on { format!("{sub_id}: cancels at the period end") } else { format!("{sub_id}: renews again") };
        Ok(Outcome { events, redirect: None, note })
    }

    /// Refund a payment (all of what is left, or `amount` cents):
    /// `charge.refunded` and `refund.created`.
    pub fn refund(&self, payment_id: &str, amount: Option<i64>, now: i64) -> Result<Outcome, String> {
        let mut g = self.lock();
        let p = g.payments.iter_mut().find(|p| p.id == payment_id).ok_or_else(|| format!("no payment {payment_id}"))?;
        if p.status == "failed" {
            return Err(format!("{payment_id} failed: nothing to refund"));
        }
        let left = p.amount - p.refunded;
        if left <= 0 {
            return Err(format!("Charge {} has already been refunded.", p.charge));
        }
        let back = amount.unwrap_or(left).min(left);
        p.refunded += back;
        if p.refunded >= p.amount {
            p.status = "refunded".into();
        }
        let p = p.clone();
        let refund_id = g.new_id("re");
        let refund = json!({ "id": refund_id, "object": "refund", "amount": back, "currency": p.currency, "charge": p.charge, "payment_intent": p.payment_intent, "status": "succeeded", "created": now });
        let charge = g.charge_json(&p);
        let events = vec![g.event("charge.refunded", charge, now), g.event("refund.created", refund, now)];
        Ok(Outcome { events, redirect: None, note: format!("{payment_id}: refunded {}", money(back, &p.currency)) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    fn kinds(events: &[Event]) -> Vec<&str> {
        events.iter().map(|e| e.kind.as_str()).collect()
    }

    const HOSTED: &str = "http://127.0.0.1:1/dev/payments/checkout";

    fn subscribe(sim: &Simulator) -> (Value, Outcome) {
        let f = form(&[
            ("mode", "subscription"),
            ("line_items[0][price]", DEV_PRICE_ID),
            ("line_items[0][quantity]", "1"),
            ("success_url", "http://app/profile?billing=done"),
            ("cancel_url", "http://app/profile?billing=cancelled"),
            ("client_reference_id", "7"),
            ("subscription_data[metadata][user_id]", "7"),
            ("customer_email", "ana@example.org"),
        ]);
        let s = sim.create_checkout_session(&f, HOSTED, 1000).unwrap();
        let out = sim.pay(s["id"].as_str().unwrap(), true, 1000).unwrap();
        (s, out)
    }

    #[test]
    fn checkout_in_subscription_mode_makes_a_customer_a_subscription_and_the_events_the_app_reads() {
        let sim = Simulator::default();
        let (s, out) = subscribe(&sim);
        assert_eq!((s["object"].as_str(), s["status"].as_str(), s["amount_total"].as_i64()), (Some("checkout.session"), Some("open"), Some(DEV_PRICE_AMOUNT)));
        assert!(s["url"].as_str().unwrap().starts_with(&format!("{HOSTED}/cs_test_dev")), "{s}");
        assert_eq!(out.redirect.as_deref(), Some("http://app/profile?billing=done"));
        assert_eq!(
            kinds(&out.events),
            ["customer.created", "customer.subscription.created", "charge.succeeded", "payment_intent.succeeded", "invoice.paid", "invoice.payment_succeeded", "checkout.session.completed"]
        );
        // What billing::apply_event reads off the subscription.
        let sub = &out.events[1].object;
        assert_eq!(sub["status"], "active");
        assert_eq!(sub["metadata"]["user_id"], "7");
        assert_eq!(sub["items"]["data"][0]["price"]["id"], DEV_PRICE_ID);
        assert_eq!(sub["items"]["data"][0]["current_period_end"].as_i64(), Some(1000 + PERIOD_SECS));
        assert!(sub["customer"].as_str().unwrap().starts_with("cus_dev"));
        let done = &out.events.last().unwrap().object;
        assert_eq!((done["status"].as_str(), done["payment_status"].as_str(), done["subscription"].as_str()), (Some("complete"), Some("paid"), sub["id"].as_str()));
        // Paying twice is refused: the session is complete.
        assert!(sim.pay(s["id"].as_str().unwrap(), true, 1001).is_err());
        assert_eq!(sim.event(&out.events[0].id).unwrap().kind, "customer.created");
    }

    #[test]
    fn renewals_fail_on_request_and_recover_when_paid() {
        let sim = Simulator::default();
        let (_, out) = subscribe(&sim);
        let sub = out.events[1].object["id"].as_str().unwrap().to_string();
        let r = sim.renew(&sub, 2000).unwrap();
        assert_eq!(kinds(&r.events), ["charge.succeeded", "payment_intent.succeeded", "invoice.paid", "invoice.payment_succeeded", "customer.subscription.updated"]);
        assert_eq!(r.events[4].object["current_period_end"].as_i64(), Some(2000 + PERIOD_SECS));

        sim.set_fail_next(&sub, true).unwrap();
        let r = sim.renew(&sub, 3000).unwrap();
        assert_eq!(kinds(&r.events), ["charge.failed", "payment_intent.payment_failed", "invoice.payment_failed", "customer.subscription.updated"]);
        assert_eq!(r.events[3].object["status"], "past_due");
        let invoice = r.events[2].object["id"].as_str().unwrap().to_string();
        assert!(!sim.subscriptions_of(r.events[3].object["customer"].as_str().unwrap())[0].fail_next, "used up");

        let r = sim.mark_paid(&invoice, 4000).unwrap();
        assert_eq!(r.events.last().unwrap().object["status"], "active");
        assert!(sim.mark_paid(&invoice, 4000).is_err());

        // Cancel at the period end: the next renewal ends it.
        let r = sim.cancel_at_period_end(&sub, true, 5000).unwrap();
        assert_eq!(r.events[0].object["cancel_at_period_end"], true);
        let r = sim.renew(&sub, 6000).unwrap();
        assert_eq!(kinds(&r.events), ["customer.subscription.deleted"]);
        assert_eq!(r.events[0].object["status"], "canceled");
        assert!(sim.renew(&sub, 7000).is_err() && sim.cancel_now(&sub, 7000).is_err());
    }

    #[test]
    fn one_off_payments_work_for_credit_packs_and_refund() {
        let sim = Simulator::default();
        // A price the simulator does not know (#311's packs come from SKRONGLE_AI_CREDIT_PACKS): made up.
        let f = form(&[
            ("mode", "payment"),
            ("line_items[0][price]", "price_pack3"),
            ("line_items[0][quantity]", "1"),
            ("success_url", "http://app/profile?credits=done"),
            ("cancel_url", "http://app/profile?credits=cancelled"),
            ("metadata[kind]", "ai-credits"),
            ("metadata[user_id]", "7"),
            ("metadata[credit_micros]", "2000000"),
            ("payment_intent_data[metadata][user_id]", "7"),
        ]);
        let s = sim.create_checkout_session(&f, HOSTED, 1000).unwrap();
        assert_eq!(s["amount_total"].as_i64(), Some(MADE_UP_AMOUNT));
        assert!(sim.price("price_pack3").unwrap().made_up);
        let id = s["id"].as_str().unwrap();

        // Fail: declined, still open, no payment kept.
        let out = sim.pay(id, false, 1000).unwrap();
        assert_eq!((kinds(&out.events), out.redirect.is_none()), (vec!["customer.created", "payment_intent.payment_failed"], true));
        assert!(sim.session(id).unwrap().declined && sim.snapshot().payments.is_empty());

        let out = sim.pay(id, true, 1001).unwrap();
        assert_eq!(kinds(&out.events), ["charge.succeeded", "payment_intent.succeeded", "checkout.session.completed"]);
        let done = &out.events[2].object;
        assert_eq!((done["mode"].as_str(), done["payment_status"].as_str(), done["metadata"]["credit_micros"].as_str()), (Some("payment"), Some("paid"), Some("2000000")));
        let pi = done["payment_intent"].as_str().unwrap().to_string();
        assert_eq!(out.events[1].object["metadata"]["user_id"], "7");

        let (refund, events) = sim.create_refund(&form(&[("payment_intent", &pi)]), 1002).unwrap();
        assert_eq!(refund["amount"].as_i64(), Some(MADE_UP_AMOUNT));
        assert_eq!((events[0].kind.as_str(), events[0].object["amount_refunded"].as_i64(), events[0].object["payment_intent"].as_str()), ("charge.refunded", Some(MADE_UP_AMOUNT), Some(pi.as_str())));
        assert!(sim.create_refund(&form(&[("payment_intent", &pi)]), 1003).is_err());
    }

    #[test]
    fn the_api_answers_with_stripes_errors() {
        let sim = Simulator::default();
        let bad = |pairs: &[(&str, &str)]| sim.create_checkout_session(&form(pairs), HOSTED, 1).unwrap_err();
        assert_eq!(bad(&[("line_items[0][price]", DEV_PRICE_ID), ("success_url", "x")]).code, Some("parameter_missing"));
        assert_eq!(bad(&[("mode", "payment"), ("line_items[0][price]", DEV_PRICE_ID), ("success_url", "x")]).param.as_deref(), Some("line_items[0][price]"));
        assert_eq!(bad(&[("mode", "subscription"), ("line_items[0][price]", "price_dev_credits_5"), ("success_url", "x")]).status, 400);
        assert_eq!(bad(&[("mode", "subscription"), ("line_items[0][price]", DEV_PRICE_ID), ("success_url", "x"), ("bogus", "1")]).code, Some("parameter_unknown"));
        let e = sim.retrieve("subscriptions", "sub_nope").unwrap_err();
        assert_eq!((e.status, e.code), (404, Some("resource_missing")));
        assert_eq!(e.body()["error"]["message"], "No such subscription: 'sub_nope'");
        assert_eq!(StripeError::auth("sk_live_abcdef").body()["error"]["message"], "Invalid API Key provided: sk_live_***");
        assert!(key_ok("sk_test_dev_simulator") && key_ok("rk_test_x") && !key_ok("sk_live_x") && !key_ok("sk_test_"));
        // A customer from before a restart is made again for the portal.
        let p = sim.create_portal_session(&form(&[("customer", "cus_old"), ("return_url", "http://app/profile")]), "http://h/portal", 1).unwrap();
        assert!(p["url"].as_str().unwrap().starts_with("http://h/portal/bps_dev") && sim.customer("cus_old").is_some());
        assert_eq!(money(400, "eur"), "€4.00");
        assert_eq!(money(-1050, "usd"), "-$10.50");
    }
}
