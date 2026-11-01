//! `/dev/payments/*`: the dev payment provider (`crate::dev_payments`),
//! mounted only in dev mode (and only for requests made on this machine,
//! like every `/dev` route).
//!
//! Three parts. Stripe's API at `/dev/payments/stripe/v1/…`, which the
//! app's own billing code calls with its secret key (no cookie; the key
//! decides). The pages Stripe hosts: Checkout (`/dev/payments/checkout/{id}`:
//! **Pay**, **Fail**, **Cancel**) and the Billing Portal
//! (`/dev/payments/portal/{id}`). And the dev mailbox's Payments section
//! (`/dev/payments`): the log of API calls and webhooks, what the simulator
//! holds, and an action per row. Every change sends its events, signed with
//! the app's webhook secret, to the app's `/billing/webhook`.

use askama::Template;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::{Form, Json};
use serde::Deserialize;
use std::sync::Arc;

use super::Page;
use crate::auth::{CurrentUser, MaybeUser};
use crate::dev_payments::{self, money, Entry, Event, Outcome, StripeError};
use crate::error::AppError;
use crate::AppState;

fn header<'a>(headers: &'a HeaderMap, name: &str) -> &'a str {
    headers.get(name).and_then(|v| v.to_str().ok()).unwrap_or("")
}

/// The app's root as the browser and the webhooks reach it: the Stripe
/// settings' public URL, else the request's host.
fn app_root(state: &AppState, headers: &HeaderMap) -> String {
    match &state.stripe {
        Some(c) => c.public_url.clone(),
        None => {
            let host = header(headers, "host");
            format!("http://{}{}", if host.is_empty() { "127.0.0.1" } else { host }, crate::base_path())
        }
    }
}

/// A form-encoded body's fields, in order.
fn fields(body: &[u8]) -> Vec<(String, String)> {
    String::from_utf8_lossy(body)
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (crate::dev_ai::urldecode(k), crate::dev_ai::urldecode(v))
        })
        .collect()
}

fn pretty(v: &serde_json::Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}

// ---------- webhooks ----------

/// Send `events` to the app's webhook, in order, on a thread of their own
/// (each is an HTTP request back to this server), and log each delivery.
/// `tampered`: the signature is broken, as a forger's would be.
pub async fn deliver(state: &AppState, events: Vec<Event>, tampered: bool) {
    if events.is_empty() {
        return;
    }
    let Some(cfg) = state.stripe.clone() else {
        for e in events {
            state.dev_payments.log_entry(Entry {
                id: 0,
                at: crate::db::now(),
                kind: "webhook",
                what: e.kind.clone(),
                status: 0,
                summary: "not sent: subscriptions are off on this server (no Stripe settings)".into(),
                request: pretty(&e.body()),
                response: String::new(),
                event: Some(e.id.clone()),
                tampered,
            });
        }
        return;
    };
    let url = format!("{}/billing/webhook", cfg.public_url);
    let secret = cfg.webhook_secret.clone();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let to = url.clone();
    std::thread::spawn(move || {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(20)))
            .http_status_as_error(false)
            .build()
            .into();
        let mut results = Vec::new();
        for e in events {
            let body = serde_json::to_string(&e.body()).unwrap_or_default();
            let mut sig = crate::billing::sign(&secret, body.as_bytes(), crate::db::now());
            if tampered {
                // Flip the signature's last digit.
                let last = if sig.ends_with('0') { '1' } else { '0' };
                sig.pop();
                sig.push(last);
            }
            let (status, reply) = match agent
                .post(&to)
                .header("content-type", "application/json; charset=utf-8")
                .header("stripe-signature", &sig)
                .header("user-agent", "Stripe/1.0 (+https://stripe.com/docs/webhooks)")
                .send(&body)
            {
                Ok(mut r) => (r.status().as_u16(), r.body_mut().read_to_string().unwrap_or_default()),
                Err(err) => (0, format!("not delivered: {err}")),
            };
            results.push((e, body, sig, status, reply));
        }
        let _ = tx.send(results);
    });
    let Ok(results) = rx.await else { return };
    for (e, body, sig, status, reply) in results {
        let ok = (200..300).contains(&status);
        let summary = match (ok, tampered) {
            (true, _) => e.id.clone(),
            (false, true) => format!("{} with a broken signature → {status}: {}", e.id, reply.trim()),
            (false, false) => format!("{} → {status}: {}", e.id, reply.trim()),
        };
        tracing::info!("dev-payments: webhook {} {} → {status}", e.kind, e.id);
        let body = serde_json::from_str::<serde_json::Value>(&body).map(|v| pretty(&v)).unwrap_or(body);
        state.dev_payments.log_entry(Entry {
            id: 0,
            at: crate::db::now(),
            kind: "webhook",
            what: e.kind.clone(),
            status,
            summary,
            request: format!("POST {url}\nStripe-Signature: {sig}\n\n{body}"),
            response: reply,
            event: Some(e.id.clone()),
            tampered,
        });
    }
}

// ---------- the dev login's ?plan= ----------

/// `/dev/login?plan=basic|plus|free`: put learner `uid` on `plan` the way a
/// learner gets there, through the simulator (#963: nothing else sets a
/// plan). Whatever pays now is cancelled at once; a paid plan is then a
/// Checkout at its price, paid, with the signed webhooks delivered before
/// this returns. Needs the simulator to be the app's Stripe.
pub async fn set_plan(state: &AppState, uid: i64, plan: crate::billing::Plan) -> Result<(), AppError> {
    let now = crate::db::now();
    let current = state.db.call(move |c| crate::billing::subscription(c, uid)).await?.filter(|s| s.is_premium());
    if let Some(s) = current {
        if s.plan == plan && !s.cancel_at_period_end {
            return Ok(());
        }
        match state.dev_payments.cancel_now(&s.subscription_id, now) {
            Ok(out) => {
                log_page_action(state, "Dev login: plan", &out.note);
                deliver(state, out.events, false).await;
            }
            // Not the simulator's (it keeps nothing across server runs): end it here.
            Err(_) => {
                state.db.call(move |c| c.execute("UPDATE subscriptions SET status = 'canceled' WHERE user_id = ?1", [uid]).map(|_| ())).await?;
            }
        }
    }
    if !plan.paid() {
        return Ok(());
    }
    let cfg = match &state.stripe {
        Some(c) if c.api_base.contains("/dev/payments/stripe") => c.clone(),
        _ => return Err(AppError::BadRequest("plan= needs the dev payment provider: leave SKRONGLE_STRIPE_* unset and SKRONGLE_DEV_STRIPE on".into())),
    };
    let (who, _) = super::billing::subscriber(state, &cfg, uid).await?;
    let url = tokio::task::spawn_blocking(move || crate::billing::checkout_url(&cfg, &who, plan))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .map_err(AppError::BadRequest)?;
    let session = url.rsplit('/').next().unwrap_or_default().to_string();
    let out = state.dev_payments.pay(&session, true, now).map_err(AppError::BadRequest)?;
    log_page_action(state, "Dev login: plan", &out.note);
    deliver(state, out.events, false).await;
    Ok(())
}

fn log_page_action(state: &AppState, what: &str, note: &str) {
    state.dev_payments.log_entry(Entry {
        id: 0,
        at: crate::db::now(),
        kind: "page",
        what: what.to_string(),
        status: 0,
        summary: note.to_string(),
        request: String::new(),
        response: String::new(),
        event: None,
        tampered: false,
    });
}

// ---------- Stripe's API ----------

fn stripe_json(status: u16, body: serde_json::Value) -> Response {
    let mut res = (StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR), Json(body)).into_response();
    if let Ok(v) = format!("req_dev{}", crate::db::now()).parse() {
        res.headers_mut().insert("request-id", v);
    }
    res
}

/// Any method on `/dev/payments/stripe/v1/{*path}`: the calls the app makes
/// (create a Checkout session, a Billing Portal session) and the ones a
/// Stripe client may make around them (retrieve an object, update or cancel
/// a subscription, refund). Answers in Stripe's shapes, errors included.
pub async fn api(State(state): State<Arc<AppState>>, method: Method, Path(path): Path<String>, headers: HeaderMap, body: Bytes) -> Response {
    let now = crate::db::now();
    let form = fields(&body);
    let key = header(&headers, "authorization").strip_prefix("Bearer ").unwrap_or("").trim().to_string();
    let root = app_root(&state, &headers);
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    let result: Result<(serde_json::Value, Vec<Event>), StripeError> = if !dev_payments::key_ok(&key) {
        Err(StripeError::auth(&key))
    } else {
        let sim = &state.dev_payments;
        match (&method, parts.as_slice()) {
            (&Method::POST, ["customers"]) => sim.create_customer(&form, headers.get("idempotency-key").and_then(|v| v.to_str().ok()), now),
            (&Method::POST, ["checkout", "sessions"]) => sim.create_checkout_session(&form, &format!("{root}/dev/payments/checkout"), now).map(|v| (v, Vec::new())),
            (&Method::POST, ["billing_portal", "sessions"]) => sim.create_portal_session(&form, &format!("{root}/dev/payments/portal"), now).map(|v| (v, Vec::new())),
            (&Method::GET, ["checkout", "sessions", id]) => sim.retrieve("checkout/sessions", id).map(|v| (v, Vec::new())),
            (&Method::GET, [kind @ ("subscriptions" | "customers" | "invoices" | "payment_intents" | "charges" | "prices"), id]) => sim.retrieve(kind, id).map(|v| (v, Vec::new())),
            (&Method::POST, ["subscriptions", id]) => sim.update_subscription(id, &form, now),
            (&Method::DELETE, ["subscriptions", id]) => sim.delete_subscription(id, now),
            (&Method::POST, ["refunds"]) => sim.create_refund(&form, now),
            _ => Err(StripeError {
                status: 404,
                kind: "invalid_request_error",
                code: None,
                param: None,
                message: format!("Unrecognized request URL ({method}: /v1/{}). The dev simulator speaks the calls listed on /dev/payments.", path.trim_matches('/')),
            }),
        }
    };
    let (status, reply, events) = match result {
        Ok((v, events)) => (200, v, events),
        Err(e) => (e.status, e.body(), Vec::new()),
    };
    let summary = match status {
        200 => reply["id"].as_str().map(|id| format!("{} {id}", reply["object"].as_str().unwrap_or("")) + &reply["url"].as_str().map(|u| format!(" · {u}")).unwrap_or_default()).unwrap_or_default(), // panic-ok: serde_json Value indexing gives Null, never panics
        _ => reply["error"]["message"].as_str().unwrap_or("").to_string(), // panic-ok: serde_json Value indexing gives Null, never panics
    };
    let request = form.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("\n");
    state.dev_payments.log_entry(Entry {
        id: 0,
        at: now,
        kind: "api",
        what: format!("{method} /v1/{}", path.trim_matches('/')),
        status,
        summary,
        request: format!("Authorization: Bearer {}…\n\n{request}", key.chars().take(12).collect::<String>()),
        response: pretty(&reply),
        event: None,
        tampered: false,
    });
    tracing::info!("dev-payments: {method} /v1/{} → {status}", path.trim_matches('/'));
    deliver(&state, events, false).await;
    stripe_json(status, reply)
}

// ---------- the hosted pages ----------

#[derive(Template)]
#[template(path = "dev_payments_checkout.html")]
pub struct CheckoutTemplate {
    pub id: String,
    pub action: String,
    pub product: String,
    /// "€4.00 per month", "$5.00".
    pub amount: String,
    pub mode: String,
    pub email: String,
    pub status: String,
    pub declined: bool,
    pub back: String,
}

/// GET /dev/payments/checkout/{id}: the Checkout page Stripe would host.
pub async fn checkout_page(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Result<Response, AppError> {
    let s = state.dev_payments.session(&id).filter(|s| s.mode != "portal").ok_or(AppError::NotFound)?;
    let price = state.dev_payments.price(&s.price);
    let recurring = price.as_ref().is_some_and(|p| p.recurring);
    Ok(Page(CheckoutTemplate {
        action: crate::url(&format!("/dev/payments/checkout/{id}")),
        product: price.map(|p| p.product).unwrap_or_else(|| s.price.clone()),
        amount: format!("{}{}", money(s.amount_total, &s.currency), if recurring { " per month" } else { "" }),
        mode: s.mode.clone(),
        email: s.customer_email.clone().or_else(|| s.customer.as_deref().and_then(|c| state.dev_payments.customer(c)).and_then(|c| c.email)).unwrap_or_default(),
        status: s.status.clone(),
        declined: s.declined,
        back: if s.cancel_url.is_empty() { s.success_url.clone() } else { s.cancel_url.clone() },
        id,
    })
    .into_response())
}

#[derive(Deserialize)]
pub struct DoForm {
    #[serde(rename = "do", default)]
    pub action: String,
    #[serde(default)]
    pub amount: String,
}

/// POST /dev/payments/checkout/{id} (`do=pay|fail|cancel`). Pay sends the
/// webhooks, then the browser goes to the success URL; Fail declines the
/// card and shows the page again; Cancel goes to the cancel URL.
pub async fn checkout_act(State(state): State<Arc<AppState>>, Path(id): Path<String>, Form(f): Form<DoForm>) -> Result<Response, AppError> {
    let now = crate::db::now();
    let out = match f.action.as_str() {
        "pay" => state.dev_payments.pay(&id, true, now),
        "fail" => state.dev_payments.pay(&id, false, now),
        "cancel" => state.dev_payments.abandon(&id),
        other => return Err(AppError::BadRequest(format!("do {other:?}: pay, fail or cancel"))),
    }
    .map_err(AppError::BadRequest)?;
    log_page_action(&state, &format!("Checkout: {}", f.action), &out.note);
    let Outcome { events, redirect, .. } = out;
    deliver(&state, events, false).await;
    Ok(Redirect::to(&redirect.unwrap_or_else(|| crate::url(&format!("/dev/payments/checkout/{id}")))).into_response())
}

pub struct PortalSub {
    pub id: String,
    pub product: String,
    pub amount: String,
    pub status: String,
    pub period_end: String,
    pub cancel_at_period_end: bool,
    /// A failed invoice "Update card" pays.
    pub unpaid_invoice: Option<String>,
}

#[derive(Template)]
#[template(path = "dev_payments_portal.html")]
pub struct PortalTemplate {
    pub id: String,
    pub action: String,
    pub customer: String,
    pub email: String,
    pub subs: Vec<PortalSub>,
    pub back: String,
}

fn portal_subs(state: &AppState, customer: &str) -> Vec<PortalSub> {
    let snap = state.dev_payments.snapshot();
    state
        .dev_payments
        .subscriptions_of(customer)
        .into_iter()
        .map(|s| {
            let price = snap.prices.iter().find(|p| p.id == s.price);
            PortalSub {
                product: price.map(|p| p.product.clone()).unwrap_or_else(|| s.price.clone()),
                amount: price.map(|p| format!("{} per month", money(p.amount * s.quantity, &p.currency))).unwrap_or_default(),
                status: s.status.replace('_', " "),
                period_end: crate::profile::long_date(s.period_end),
                cancel_at_period_end: s.cancel_at_period_end,
                unpaid_invoice: snap.payments.iter().find(|p| p.subscription.as_deref() == Some(s.id.as_str()) && p.status == "failed").map(|p| p.id.clone()),
                id: s.id,
            }
        })
        .collect()
}

/// GET /dev/payments/portal/{id}: the Billing Portal Stripe would host.
pub async fn portal_page(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Result<Response, AppError> {
    let s = state.dev_payments.session(&id).filter(|s| s.mode == "portal").ok_or(AppError::NotFound)?;
    let customer = s.customer.clone().unwrap_or_default();
    Ok(Page(PortalTemplate {
        action: crate::url(&format!("/dev/payments/portal/{id}")),
        email: state.dev_payments.customer(&customer).and_then(|c| c.email).unwrap_or_default(),
        subs: portal_subs(&state, &customer),
        back: s.success_url.clone(),
        customer,
        id,
    })
    .into_response())
}

#[derive(Deserialize)]
pub struct PortalForm {
    #[serde(rename = "do")]
    pub action: String,
    pub subscription: String,
}

/// POST /dev/payments/portal/{id} (`do=cancel|resume|cancel-now|update-card`, `subscription`).
pub async fn portal_act(State(state): State<Arc<AppState>>, Path(id): Path<String>, Form(f): Form<PortalForm>) -> Result<Response, AppError> {
    let now = crate::db::now();
    let s = state.dev_payments.session(&id).filter(|s| s.mode == "portal").ok_or(AppError::NotFound)?;
    let customer = s.customer.unwrap_or_default();
    let sub = state.dev_payments.subscriptions_of(&customer).into_iter().find(|x| x.id == f.subscription).ok_or(AppError::NotFound)?;
    let sim = &state.dev_payments;
    let out = match f.action.as_str() {
        "cancel" => sim.cancel_at_period_end(&sub.id, true, now),
        "resume" => sim.cancel_at_period_end(&sub.id, false, now),
        "cancel-now" => sim.cancel_now(&sub.id, now),
        "update-card" => match portal_subs(&state, &customer).into_iter().find(|x| x.id == sub.id).and_then(|x| x.unpaid_invoice) {
            Some(inv) => sim.mark_paid(&inv, now),
            None => sim.set_fail_next(&sub.id, false),
        },
        other => return Err(AppError::BadRequest(format!("do {other:?}: cancel, resume, cancel-now or update-card"))),
    }
    .map_err(AppError::BadRequest)?;
    log_page_action(&state, &format!("Portal: {}", f.action), &out.note);
    deliver(&state, out.events, false).await;
    Ok(Redirect::to(&crate::url(&format!("/dev/payments/portal/{id}"))).into_response())
}

// ---------- the dev mailbox's Payments section ----------

pub struct LogRow {
    pub id: i64,
    pub href: String,
    pub ago: String,
    pub kind: &'static str,
    pub what: String,
    pub status: u16,
    /// `ok`, `pending` or `error`, for the badge's colour.
    pub state: &'static str,
    pub summary: String,
    pub event: String,
    pub tampered: bool,
}

fn log_row(e: &Entry, now: i64) -> LogRow {
    LogRow {
        id: e.id,
        href: crate::url(&format!("/dev/payments/log/{}", e.id)),
        ago: super::time_ago(e.at, now),
        kind: e.kind,
        what: e.what.clone(),
        status: e.status,
        state: match e.status {
            0 if e.kind == "page" => "ok",
            200..=299 => "ok",
            _ => "error",
        },
        summary: e.summary.clone(),
        event: e.event.clone().unwrap_or_default(),
        tampered: e.tampered,
    }
}

#[derive(Template)]
#[template(path = "dev_payments_log.html")]
pub struct LogRows {
    pub rows: Vec<LogRow>,
}

pub struct SubRow {
    pub id: String,
    pub customer: String,
    pub email: String,
    pub user: String,
    pub status: String,
    pub price: String,
    pub period: String,
    pub cancel_at_period_end: bool,
    pub fail_next: bool,
    pub canceled: bool,
    pub action: String,
}

pub struct PaymentRow {
    pub id: String,
    pub what: String,
    pub customer: String,
    pub amount: String,
    pub refunded: String,
    pub status: String,
    pub ago: String,
    pub action: String,
    pub can_refund: bool,
    pub failed: bool,
}

pub struct CustomerRow {
    pub id: String,
    pub email: String,
    pub subs: usize,
    pub ago: String,
}

pub struct SessionRow {
    pub id: String,
    pub href: String,
    pub mode: String,
    pub what: String,
    pub email: String,
    pub status: String,
    pub ago: String,
}

#[derive(Template)]
#[template(path = "dev_payments_state.html")]
pub struct StateRows {
    pub subs: Vec<SubRow>,
    pub payments: Vec<PaymentRow>,
    pub customers: Vec<CustomerRow>,
    pub sessions: Vec<SessionRow>,
}

fn state_rows(state: &AppState, now: i64) -> StateRows {
    let snap = state.dev_payments.snapshot();
    let email = |c: &str| snap.customers.iter().find(|x| x.id == c).and_then(|x| x.email.clone()).unwrap_or_default();
    let price_label = |id: &str, qty: i64| match snap.prices.iter().find(|p| p.id == id) {
        Some(p) => format!("{} · {}{}", p.product, money(p.amount * qty.max(1), &p.currency), if p.recurring { " a month" } else { "" }),
        None => id.to_string(),
    };
    StateRows {
        subs: snap
            .subscriptions
            .iter()
            .map(|s| SubRow {
                id: s.id.clone(),
                customer: s.customer.clone(),
                email: email(&s.customer),
                user: s.metadata.get("user_id").cloned().unwrap_or_default(),
                status: s.status.clone(),
                price: price_label(&s.price, s.quantity),
                period: if s.status == "canceled" {
                    s.canceled_at.map(|t| format!("ended {}", crate::profile::long_date(t))).unwrap_or_default()
                } else {
                    format!("{} {}", if s.cancel_at_period_end { "ends" } else { "renews" }, crate::profile::long_date(s.period_end))
                },
                cancel_at_period_end: s.cancel_at_period_end,
                fail_next: s.fail_next,
                canceled: s.status == "canceled",
                action: crate::url(&format!("/dev/payments/subscriptions/{}", s.id)),
            })
            .collect(),
        payments: snap
            .payments
            .iter()
            .map(|p| PaymentRow {
                id: p.id.clone(),
                what: if p.invoice { format!("Invoice ({})", p.billing_reason.replace('_', " ")) } else { "One-off payment".into() } + " · " + &price_label(&p.price, 1),
                customer: email(&p.customer).chars().next().map(|_| email(&p.customer)).unwrap_or_else(|| p.customer.clone()),
                amount: money(p.amount, &p.currency),
                refunded: if p.refunded > 0 { money(p.refunded, &p.currency) } else { String::new() },
                status: p.status.clone(),
                ago: super::time_ago(p.created, now),
                action: crate::url(&format!("/dev/payments/payments/{}", p.id)),
                can_refund: p.status != "failed" && p.refunded < p.amount,
                failed: p.status == "failed",
            })
            .collect(),
        customers: snap
            .customers
            .iter()
            .map(|c| CustomerRow { id: c.id.clone(), email: c.email.clone().unwrap_or_default(), subs: snap.subscriptions.iter().filter(|s| s.customer == c.id).count(), ago: super::time_ago(c.created, now) })
            .collect(),
        sessions: snap
            .sessions
            .iter()
            .filter(|s| s.mode != "portal")
            .map(|s| SessionRow {
                href: s.url.clone(),
                mode: s.mode.clone(),
                what: price_label(&s.price, s.quantity),
                email: s.customer_email.clone().unwrap_or_default(),
                status: if s.declined && s.status == "open" { "open, card declined".into() } else { s.status.clone() },
                ago: super::time_ago(s.created, now),
                id: s.id.clone(),
            })
            .collect(),
    }
}

#[derive(Template)]
#[template(path = "dev_payments.html")]
pub struct PaymentsTemplate {
    pub user: Option<CurrentUser>,
    pub mode: String,
    pub on: bool,
    pub api_base: String,
    pub webhook_url: String,
    pub secret_key: String,
    pub webhook_secret: String,
    pub price_id: String,
    pub prices: Vec<(String, String)>,
    pub state: StateRows,
    pub log: LogRows,
    pub notice: String,
}

impl PaymentsTemplate {
    /// The section, for `dev_box_kinds.html`.
    pub fn section(&self) -> &'static str {
        "payments"
    }
}

fn mode(state: &AppState) -> String {
    match &state.stripe {
        Some(c) if c.api_base.contains("/dev/payments/stripe") => "dev mode: the app's Stripe calls come here, with the simulator's keys".into(),
        Some(c) => format!("SKRONGLE_STRIPE_* is set: the app's Stripe calls go to {}, not here", c.api_base),
        None => "off (SKRONGLE_DEV_STRIPE=0): subscriptions are not offered; the API here still answers".into(),
    }
}

#[derive(Deserialize)]
pub struct Params {
    pub format: Option<String>,
    #[serde(default)]
    pub notice: String,
}

fn wants_html(headers: &HeaderMap, format: Option<&str>) -> bool {
    match format {
        Some("json") => false,
        Some("html") => true,
        _ => header(headers, "accept").contains("text/html"),
    }
}

fn snapshot_json(state: &AppState) -> serde_json::Value {
    let s = state.dev_payments.snapshot();
    serde_json::json!({
        "mode": mode(state),
        "on": state.stripe.is_some(),
        "api_base": state.stripe.as_ref().map(|c| c.api_base.clone()),
        "customers": s.customers.iter().map(|c| serde_json::json!({ "id": c.id, "email": c.email, "created": c.created })).collect::<Vec<_>>(),
        "sessions": s.sessions.iter().map(|x| serde_json::json!({ "id": x.id, "mode": x.mode, "status": x.status, "url": x.url, "customer": x.customer, "price": x.price, "amount_total": x.amount_total, "declined": x.declined })).collect::<Vec<_>>(),
        "subscriptions": s.subscriptions.iter().map(|x| serde_json::json!({ "id": x.id, "customer": x.customer, "status": x.status, "price": x.price, "metadata": x.metadata, "current_period_end": x.period_end, "cancel_at_period_end": x.cancel_at_period_end, "fail_next": x.fail_next })).collect::<Vec<_>>(),
        "payments": s.payments.iter().map(|p| serde_json::json!({ "id": p.id, "invoice": p.invoice, "customer": p.customer, "subscription": p.subscription, "amount": p.amount, "currency": p.currency, "status": p.status, "refunded": p.refunded, "payment_intent": p.payment_intent })).collect::<Vec<_>>(),
        "events": s.events.iter().map(|e| serde_json::json!({ "id": e.id, "type": e.kind, "created": e.created })).collect::<Vec<_>>(),
        "log": state.dev_payments.log().iter().map(|e| serde_json::json!({ "id": e.id, "at": e.at, "kind": e.kind, "what": e.what, "status": e.status, "summary": e.summary, "event": e.event, "tampered": e.tampered })).collect::<Vec<_>>(),
    })
}

/// GET /dev/payments: the Payments section (`?format=json`: everything the simulator holds, and the log).
pub async fn page(State(state): State<Arc<AppState>>, MaybeUser(user): MaybeUser, headers: HeaderMap, Query(q): Query<Params>) -> Response {
    if !wants_html(&headers, q.format.as_deref()) {
        return Json(snapshot_json(&state)).into_response();
    }
    let now = crate::db::now();
    let cfg = state.stripe.clone();
    Page(PaymentsTemplate {
        user,
        mode: mode(&state),
        on: cfg.is_some(),
        api_base: cfg.as_ref().map(|c| c.api_base.clone()).unwrap_or_else(|| dev_payments::api_base(&app_root(&state, &headers))),
        webhook_url: format!("{}/billing/webhook", app_root(&state, &headers)),
        secret_key: dev_payments::DEV_SECRET_KEY.into(),
        webhook_secret: dev_payments::DEV_WEBHOOK_SECRET.into(),
        price_id: cfg.map(|c| c.price_id).unwrap_or_default(),
        prices: state.dev_payments.snapshot().prices.iter().map(|p| (p.id.clone(), format!("{} · {}{}{}", p.product, money(p.amount, &p.currency), if p.recurring { " a month" } else { " once" }, if p.made_up { " (made up)" } else { "" }))).collect(),
        state: state_rows(&state, now),
        log: LogRows { rows: state.dev_payments.log().iter().map(|e| log_row(e, now)).collect() },
        notice: q.notice,
    })
    .into_response()
}

/// GET /dev/payments/rows: the tables and the log as HTML, for the page's poll.
pub async fn rows(State(state): State<Arc<AppState>>) -> Result<Response, AppError> {
    let now = crate::db::now();
    let log = LogRows { rows: state.dev_payments.log().iter().map(|e| log_row(e, now)).collect() };
    Ok(Json(serde_json::json!({ "log": log.render()?, "state": state_rows(&state, now).render()?, "count": log.rows.len() })).into_response())
}

/// Back to the page (204 for the page's own `data-live` posts).
fn back(headers: &HeaderMap, notice: &str) -> Response {
    if headers.contains_key("x-dev-box") {
        return StatusCode::NO_CONTENT.into_response();
    }
    let url = if notice.is_empty() { crate::url("/dev/payments") } else { format!("{}?notice={}", crate::url("/dev/payments"), crate::oauth::urlencode(notice)) };
    Redirect::to(&url).into_response()
}

async fn finish(state: &AppState, headers: &HeaderMap, what: &str, out: Result<Outcome, String>) -> Result<Response, AppError> {
    let out = out.map_err(AppError::BadRequest)?;
    log_page_action(state, what, &out.note);
    let note = out.note.clone();
    deliver(state, out.events, false).await;
    Ok(back(headers, &note))
}

/// POST /dev/payments/subscriptions/{id} (`do=renew|fail-next|pay-next|cancel|cancel-at-end|resume`).
pub async fn subscription_act(State(state): State<Arc<AppState>>, headers: HeaderMap, Path(id): Path<String>, Form(f): Form<DoForm>) -> Result<Response, AppError> {
    let now = crate::db::now();
    let sim = &state.dev_payments;
    let out = match f.action.as_str() {
        "renew" => sim.renew(&id, now),
        "fail-next" => sim.set_fail_next(&id, true),
        "pay-next" => sim.set_fail_next(&id, false),
        "cancel" => sim.cancel_now(&id, now),
        "cancel-at-end" => sim.cancel_at_period_end(&id, true, now),
        "resume" => sim.cancel_at_period_end(&id, false, now),
        other => return Err(AppError::BadRequest(format!("do {other:?}: renew, fail-next, pay-next, cancel, cancel-at-end or resume"))),
    };
    finish(&state, &headers, &format!("Subscription: {}", f.action), out).await
}

/// POST /dev/payments/payments/{id} (`do=mark-paid|refund`, `amount` in cents for a part refund).
pub async fn payment_act(State(state): State<Arc<AppState>>, headers: HeaderMap, Path(id): Path<String>, Form(f): Form<DoForm>) -> Result<Response, AppError> {
    let now = crate::db::now();
    let out = match f.action.as_str() {
        "mark-paid" => state.dev_payments.mark_paid(&id, now),
        "refund" => {
            let amount = match f.amount.trim() {
                "" => None,
                a => Some(a.parse::<i64>().ok().filter(|a| *a > 0).ok_or_else(|| AppError::BadRequest(format!("amount {a:?}: cents, more than 0")))?),
            };
            state.dev_payments.refund(&id, amount, now)
        }
        other => return Err(AppError::BadRequest(format!("do {other:?}: mark-paid or refund"))),
    };
    finish(&state, &headers, &format!("Payment: {}", f.action), out).await
}

/// POST /dev/payments/events/{id} (`do=resend|tamper`): send an event to the
/// webhook again, as it was, or with a broken signature.
pub async fn event_act(State(state): State<Arc<AppState>>, headers: HeaderMap, Path(id): Path<String>, Form(f): Form<DoForm>) -> Result<Response, AppError> {
    let e = state.dev_payments.event(&id).ok_or(AppError::NotFound)?;
    let tampered = match f.action.as_str() {
        "resend" => false,
        "tamper" => true,
        other => return Err(AppError::BadRequest(format!("do {other:?}: resend or tamper"))),
    };
    let note = format!("{} {id} sent again{}", e.kind, if tampered { " with a broken signature" } else { "" });
    log_page_action(&state, "Event: send again", &note);
    deliver(&state, vec![e], tampered).await;
    Ok(back(&headers, &note))
}

/// POST /dev/payments/clear: empty the log.
pub async fn clear(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    state.dev_payments.clear_log();
    back(&headers, "")
}

/// POST /dev/payments/reset: forget every customer, session, subscription,
/// payment and event (the app's database keeps what it was told).
pub async fn reset(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    state.dev_payments.reset();
    back(&headers, "The simulator forgot everything; the app's database still has what the webhooks told it.")
}

#[derive(Template)]
#[template(path = "dev_payments_entry.html")]
pub struct EntryTemplate {
    pub user: Option<CurrentUser>,
    pub row: LogRow,
    pub request: String,
    pub response: String,
    pub resend: String,
}

/// GET /dev/payments/log/{id}: one log entry with its request and reply.
pub async fn entry(State(state): State<Arc<AppState>>, MaybeUser(user): MaybeUser, Path(id): Path<i64>) -> Result<Response, AppError> {
    let e = state.dev_payments.entry(id).ok_or(AppError::NotFound)?;
    Ok(Page(EntryTemplate {
        user,
        resend: e.event.as_ref().map(|ev| crate::url(&format!("/dev/payments/events/{ev}"))).unwrap_or_default(),
        row: log_row(&e, crate::db::now()),
        request: e.request.clone(),
        response: e.response.clone(),
    })
    .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole loop over HTTP: the plans modal offers Stripe Checkout
    /// beside the card fields, /billing/checkout goes to the simulator's
    /// Checkout, Pay sends signed webhooks that make the learner Plus, a
    /// failed renewal shows on the profile, the portal cancels, and a
    /// tampered signature is refused.
    #[test]
    fn subscribe_renew_fail_and_cancel_through_the_simulator() {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let listener = rt.block_on(tokio::net::TcpListener::bind("127.0.0.1:0")).unwrap();
        let port = listener.local_addr().unwrap().port();
        let state = crate::routes::test_state(true, Some(dev_payments::dev_config(port, None)));
        let app = crate::routes::router(state.clone());
        rt.spawn(async move { axum::serve(listener, app).await.unwrap() });
        let base = format!("http://127.0.0.1:{port}");
        let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
        let mut cookie = String::new();
        let get = |path: &str, cookie: &mut String| {
            let mut r = agent.get(&format!("{base}{path}")).header("cookie", cookie.as_str()).call().unwrap();
            if let Some(c) = r.headers().get("set-cookie").and_then(|v| v.to_str().ok()) {
                *cookie = c.split(';').next().unwrap().to_string();
            }
            let loc = r.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
            (r.status().as_u16(), loc, r.body_mut().read_to_string().unwrap())
        };
        let post = |path: &str, cookie: &str, form: &[(&str, &str)]| {
            let path = path.strip_prefix(&base).unwrap_or(path).to_string();
            let mut r = agent.post(&format!("{base}{path}")).header("cookie", cookie).send_form(form.iter().copied()).unwrap();
            let loc = r.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
            (r.status().as_u16(), loc, r.body_mut().read_to_string().unwrap())
        };
        get("/dev/login", &mut cookie);
        // The plans modal pays through Stripe Checkout only (#963: no card fields).
        let profile = get("/profile", &mut cookie).2;
        assert!(profile.contains("profile-plan-choose") && profile.contains("/billing/checkout\" class=\"plans-form\"") && !profile.contains("card_number"), "{profile}");

        // Pay with Stripe: the app asks the simulator for a session at the plan's price and sends the browser to its page.
        let (code, checkout, _) = post("/billing/checkout", &cookie, &[("plan", "basic")]);
        assert_eq!(code, 303);
        let page = get(checkout.strip_prefix(&base).unwrap(), &mut cookie).2;
        assert!(page.contains("CA$3.00 per month") && page.contains(&format!("{} Basic", crate::APP_NAME)), "{page}");
        let (code, checkout, _) = post("/billing/checkout", &cookie, &[("plan", "plus")]);
        assert_eq!(code, 303);
        assert!(checkout.starts_with(&format!("{base}/dev/payments/checkout/cs_test_dev")), "{checkout}");
        let page = get(checkout.strip_prefix(&base).unwrap(), &mut cookie).2;
        assert!(page.contains("CA$15.00 per month") && page.contains(&format!("{} Plus", crate::APP_NAME)) && page.contains("dev-checkout-pay"), "{page}");
        // Fail first: declined, nothing changes.
        let (_, again, _) = post(&checkout, "", &[("do", "fail")]);
        assert!(get(again.as_str(), &mut cookie).2.contains("Your card was declined."));
        assert!(get("/profile", &mut cookie).2.contains(r#"data-testid="profile-plan">Free"#));
        // Pay: the webhooks arrive before the browser is back.
        let (_, success, _) = post(&checkout, "", &[("do", "pay")]);
        assert_eq!(success, format!("{base}/profile?billing=done#subscription"));
        let profile = get("/profile?billing=done", &mut cookie).2;
        let plus = format!(r#"data-testid="profile-plan">{} Plus"#, crate::APP_NAME);
        assert!(profile.contains(&plus) && profile.contains(r#"data-source="stripe""#) && profile.contains(r#"profile-renewal-label">Renewal date</dt>"#) && profile.contains("profile-billing-manage"), "{profile}");
        let snap = state.dev_payments.snapshot();
        let sub = snap.subscriptions[0].id.clone();
        let hooks: Vec<_> = state.dev_payments.log().into_iter().filter(|e| e.kind == "webhook").collect();
        assert!(hooks.iter().any(|e| e.what == "customer.subscription.created" && e.status == 200), "{hooks:?}");
        assert!(hooks.iter().all(|e| e.status == 200), "{hooks:?}");

        // A failed renewal: past due on the profile (still premium while Stripe retries).
        assert_eq!(post(&format!("/dev/payments/subscriptions/{sub}"), "", &[("do", "fail-next")]).0, 303);
        assert_eq!(post(&format!("/dev/payments/subscriptions/{sub}"), "", &[("do", "renew")]).0, 303);
        let profile = get("/profile", &mut cookie).2;
        assert!(profile.contains(r#"data-premium="true""#) && profile.contains("The last payment failed"), "{profile}");

        // The portal: cancel at the period end, then the renewal ends it.
        let (_, portal, _) = post("/billing/portal", &cookie, &[]);
        assert!(portal.starts_with(&format!("{base}/dev/payments/portal/bps_dev")), "{portal}");
        assert!(get(portal.strip_prefix(&base).unwrap(), &mut cookie).2.contains(&sub));
        post(&portal, "", &[("do", "update-card"), ("subscription", &sub)]);
        let profile = get("/profile", &mut cookie).2;
        assert!(profile.contains(r#"profile-renewal-label">Renewal date</dt>"#) && !profile.contains("The last payment failed"), "{profile}");
        post(&portal, "", &[("do", "cancel"), ("subscription", &sub)]);
        let profile = get("/profile", &mut cookie).2;
        assert!(profile.contains(r#"profile-renewal-label">Ends on</dt>"#) && !profile.contains("profile-cancel-open"), "{profile}");
        post(&format!("/dev/payments/subscriptions/{sub}"), "", &[("do", "renew")]);
        let profile = get("/profile", &mut cookie).2;
        assert!(profile.contains(r#"data-testid="profile-plan">Free"#) && profile.contains(r#"data-source="free""#), "{profile}");

        // A broken signature is refused by the app's webhook.
        let ev = state.dev_payments.snapshot().events[0].id.clone();
        post(&format!("/dev/payments/events/{ev}"), "", &[("do", "tamper")]);
        let last = state.dev_payments.log().into_iter().find(|e| e.kind == "webhook").unwrap();
        assert!(last.tampered && last.status == 400, "{last:?}");

        // Stripe's errors: a wrong key, an unknown path.
        let r = agent.post(&format!("{base}/dev/payments/stripe/v1/checkout/sessions")).header("authorization", "Bearer sk_live_nope").send_form([("mode", "payment")]).unwrap();
        assert_eq!(r.status().as_u16(), 401);
        let r = agent.get(&format!("{base}/dev/payments/stripe/v1/nothing")).header("authorization", "Bearer sk_test_x1").call().unwrap();
        assert_eq!(r.status().as_u16(), 404);
        let r = agent.get(&format!("{base}/dev/payments/stripe/v1/subscriptions/{sub}")).header("authorization", "Bearer sk_test_x1").call().unwrap();
        assert_eq!(r.status().as_u16(), 200);
        // The page and its JSON.
        let page = get("/dev/payments?format=html", &mut cookie).2;
        assert!(page.contains("dev-payments-sub") && page.contains("dev-box-kind-payments"), "{page}");
        let json: serde_json::Value = serde_json::from_str(&get("/dev/payments?format=json", &mut cookie).2).unwrap();
        assert_eq!(json["subscriptions"][0]["status"], "canceled");

        // The dev login's ?plan= goes through the simulator too: a paid Checkout, then a switch, then free.
        let plan_of = |state: &Arc<AppState>| {
            let uid: i64 = state.db.with(|c| c.query_row("SELECT id FROM users WHERE email = ?1", [crate::dev::DEV_EMAIL], |r| r.get(0))).unwrap();
            state.db.with(|c| crate::billing::plan_of(c, uid)).unwrap()
        };
        assert_eq!(get("/dev/login?plan=basic", &mut cookie).0, 303);
        assert_eq!(plan_of(&state), crate::billing::Plan::Basic);
        assert_eq!(get("/dev/login?plan=plus", &mut cookie).0, 303);
        assert_eq!(plan_of(&state), crate::billing::Plan::Plus);
        let subs = state.dev_payments.snapshot().subscriptions;
        assert_eq!(subs.iter().filter(|s| s.status == "active").count(), 1, "the Basic one was cancelled first");
        assert_eq!(get("/dev/login?plan=free", &mut cookie).0, 303);
        assert_eq!(plan_of(&state), crate::billing::Plan::Free);
    }
}
