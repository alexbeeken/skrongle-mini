//! `/billing/*`: Stripe Checkout for a plan or an AI credit pack, the
//! Billing Portal, cancelling (`cancel`) and Stripe's webhook. Stripe is the
//! only way to pay (#963: the pretend card payment of #315 is gone); a plan
//! changes only when a signed webhook says so. The profile page shows the
//! state; `crate::billing` holds the rules.

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use std::sync::Arc;

use crate::auth::CurrentUser;
use crate::billing::{self, Applied, Plan, Subscriber};
use crate::error::AppError;
use crate::AppState;

#[derive(serde::Deserialize)]
pub struct CancelForm {
    /// A `billing::CANCEL_REASONS` id, or empty.
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub feedback: String,
}

/// POST /billing/cancel {reason, feedback}: ask Stripe to end the
/// subscription when its paid month ends (the learner keeps the plan until
/// then; Stripe's webhook confirms it). The reason and feedback are kept.
pub async fn cancel(State(state): State<Arc<AppState>>, user: CurrentUser, axum::Form(form): axum::Form<CancelForm>) -> Result<Response, AppError> {
    if !form.reason.is_empty() && !billing::CANCEL_REASONS.iter().any(|(id, _)| *id == form.reason) {
        return Err(AppError::BadRequest("unknown reason".into()));
    }
    let reason = form.reason;
    let feedback: String = form.feedback.trim().chars().take(2000).collect();
    let uid = user.id;
    let Some(s) = state.db.call(move |c| billing::subscription(c, uid)).await?.filter(|s| s.is_premium() && !s.cancel_at_period_end) else {
        return Ok(Redirect::to(&crate::url("/profile#subscription")).into_response());
    };
    let cfg = stripe(&state)?;
    let id = s.subscription_id.clone();
    let plan = s.plan;
    tokio::task::spawn_blocking(move || billing::cancel_at_period_end(&cfg, &id))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .map_err(|e| {
            tracing::warn!("{e}");
            AppError::BadRequest(crate::t("billing.stripe_unreachable"))
        })?;
    state
        .db
        .call(move |c| {
            billing::mark_stripe_cancelling(c, uid)?;
            billing::record_cancellation(c, uid, plan, &reason, &feedback)
        })
        .await?;
    tracing::info!("user {uid} cancelled {}; it ends with the paid month", plan.as_str());
    Ok(Redirect::to(&crate::url("/profile?saved=cancelled#subscription")).into_response())
}

fn stripe(state: &AppState) -> Result<billing::StripeConfig, AppError> {
    state.stripe.clone().ok_or_else(|| AppError::BadRequest(crate::t("billing.not_offered")))
}

/// Run a Stripe call on the blocking pool (it is one short HTTP request,
/// like the Google token exchange) and redirect the browser to its URL.
async fn redirect_to_stripe(call: impl FnOnce() -> Result<String, String> + Send + 'static) -> Result<Response, AppError> {
    let url = tokio::task::spawn_blocking(call).await.map_err(|e| AppError::Internal(e.to_string()))?.map_err(|e| {
        tracing::warn!("{e}");
        AppError::BadRequest(crate::t("billing.stripe_unreachable"))
    })?;
    Ok(Redirect::to(&url).into_response())
}

/// Who is paying, for a Checkout: the account's random billing_ref and its
/// one Stripe customer, made before its first Checkout so every Checkout
/// (two tabs, a double submit, a subscription and a credit pack) shares it
/// (#394). Also the Stripe subscription on record, if any.
pub(crate) async fn subscriber(state: &AppState, cfg: &billing::StripeConfig, uid: i64) -> Result<(Subscriber, Option<billing::Subscription>), AppError> {
    let (sub, acct, email) = state
        .db
        .call(move |c| {
            let acct = billing::account(c, uid)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
            Ok((billing::subscription(c, uid)?, acct, crate::profile::load(c, uid)?.and_then(|p| p.email)))
        })
        .await?;
    let customer_id = match acct.stripe_customer {
        Some(c) => c,
        None => {
            let (cfg, r) = (cfg.clone(), acct.billing_ref.clone());
            let created = tokio::task::spawn_blocking(move || billing::create_customer(&cfg, &r, email.as_deref()))
                .await
                .map_err(|e| AppError::Internal(e.to_string()))?
                .map_err(|e| {
                    tracing::warn!("{e}");
                    AppError::BadRequest(crate::t("billing.stripe_unreachable"))
                })?;
            state.db.call(move |c| billing::set_customer(c, uid, &created)).await?
        }
    };
    Ok((Subscriber { billing_ref: acct.billing_ref, customer_id }, sub))
}

#[derive(serde::Deserialize)]
pub struct CheckoutForm {
    /// `basic` or `plus` (the plans modal's pick); Plus when absent.
    #[serde(default)]
    pub plan: String,
}

/// POST /billing/checkout {plan}: off to Stripe to subscribe to the plan
/// (the plans modal's Purchase). A learner whose Stripe subscription still
/// pays goes to the Billing Portal instead: changing plan or taking back a
/// cancellation is the portal's job.
pub async fn checkout(State(state): State<Arc<AppState>>, user: CurrentUser, axum::Form(form): axum::Form<CheckoutForm>) -> Result<Response, AppError> {
    let cfg = stripe(&state)?;
    let plan = if form.plan.is_empty() {
        Plan::Plus
    } else {
        Plan::parse(&form.plan).filter(|p| cfg.price_for(*p).is_some()).ok_or_else(|| AppError::BadRequest(crate::t("billing.choose_plan")))?
    };
    let uid = user.id;
    let paying = state.db.call(move |c| billing::subscription(c, uid)).await?;
    if let Some(s) = paying.as_ref().filter(|s| s.is_premium()) {
        let customer = s.customer_id.clone();
        tracing::info!("user {uid} already pays for {}: off to the Billing Portal", s.plan.as_str());
        return redirect_to_stripe(move || billing::portal_url(&cfg, &customer)).await;
    }
    // A payment still being confirmed (3-D Secure, a slow card) would end
    // in a second subscription (#394).
    if paying.is_some_and(|s| s.status == "incomplete") {
        return Err(AppError::BadRequest(
            "A payment for this account is still being confirmed. Finish it, or wait a minute and try again.".into(),
        ));
    }
    let (who, _) = subscriber(&state, &cfg, uid).await?;
    tracing::info!("user {} is going to Stripe Checkout for {}", user.id, plan.as_str());
    redirect_to_stripe(move || billing::checkout_url(&cfg, &who, plan)).await
}

#[derive(serde::Deserialize)]
pub struct CreditsForm {
    /// The pack's Stripe price id (`SKRONGLE_AI_CREDIT_PACKS`).
    #[serde(default)]
    pub pack: String,
}

/// POST /billing/credits {pack}: off to Stripe to buy a pack of AI credits.
/// They are added when Stripe's webhook confirms the payment. Packs are sold
/// on every plan: on Free and Basic they are the only Skrongle AI (#931).
pub async fn credits(State(state): State<Arc<AppState>>, user: CurrentUser, axum::Form(form): axum::Form<CreditsForm>) -> Result<Response, AppError> {
    let cfg = stripe(&state)?;
    let Some(pack) = state.meter.cfg.packs.iter().find(|p| p.price_id == form.pack).cloned() else {
        return Err(AppError::BadRequest(crate::t("billing.pack_not_offered")));
    };
    let (who, _) = subscriber(&state, &cfg, user.id).await?;
    tracing::info!("user {} is going to Stripe Checkout for AI credits ({})", user.id, pack.label);
    redirect_to_stripe(move || billing::credits_checkout_url(&cfg, &who, &pack)).await
}

/// POST /billing/portal: off to Stripe to change card, see invoices or cancel.
pub async fn portal(State(state): State<Arc<AppState>>, user: CurrentUser) -> Result<Response, AppError> {
    let cfg = stripe(&state)?;
    let uid = user.id;
    let Some(sub) = state.db.call(move |c| billing::subscription(c, uid)).await? else {
        return Ok(Redirect::to(&crate::url("/profile#subscription")).into_response());
    };
    redirect_to_stripe(move || billing::portal_url(&cfg, &sub.customer_id)).await
}

/// POST /billing/webhook: Stripe's events, verified by signature. A 4xx
/// tells Stripe not to retry; a 5xx (database trouble) makes it retry.
pub async fn webhook(State(state): State<Arc<AppState>>, headers: HeaderMap, body: Bytes) -> Response {
    let Some(cfg) = state.stripe.as_ref() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let sig = headers.get("stripe-signature").and_then(|v| v.to_str().ok()).unwrap_or("");
    if let Err(why) = billing::verify_signature(&cfg.webhook_secret, sig, &body, crate::db::now()) {
        tracing::warn!("billing webhook rejected: {why}");
        return (StatusCode::BAD_REQUEST, "bad signature").into_response();
    }
    let event: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return (StatusCode::BAD_REQUEST, "not JSON").into_response(),
    };
    let id = event["id"].as_str().unwrap_or("?").to_string();
    let cfg = cfg.clone();
    match state.db.call(move |c| billing::apply_event(c, &cfg, &event)).await {
        Ok(Applied::Updated { user_id, status }) => tracing::info!("billing: {id}: user {user_id} subscription is {status}"),
        Ok(Applied::Credits { user_id, micros }) => {
            tracing::info!("billing: {id}: user {user_id} AI credits {}{}", if micros >= 0 { "+" } else { "" }, crate::metering::dollars(micros))
        }
        Ok(Applied::Ignored(why)) => tracing::debug!("billing: {id} ignored: {why}"),
        Err(e) => {
            tracing::error!("billing: {id} not applied: {e}");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }
    StatusCode::OK.into_response()
}

#[cfg(test)]
mod tests;
