//! The Privacy requests card of `/superuser` (#981, hosted only, like
//! moderation): send an account its data, forget an address with no
//! account, and the log of these and of Delete account
//! (`privacy_requests.rs`).

use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::Response;
use axum::Form;
use serde::Deserialize;
use std::sync::Arc;

use super::{done, moderates};
use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::privacy_requests::{self as requests, Action, Forgot};
use crate::AppState;

/// One log row on the page.
pub struct LogRow {
    pub when: String,
    pub superuser: String,
    pub action: String,
    pub action_label: String,
    pub reason: String,
}

/// The log for the page.
pub async fn log(state: &AppState) -> Result<Vec<LogRow>, AppError> {
    let rows = state.db.call(|c| requests::recent(c)).await?;
    Ok(rows
        .into_iter()
        .map(|r| LogRow {
            when: crate::profile::long_date(r.created_at),
            action_label: crate::t(&format!("superuser.privacy.action_{}", r.action.replace('-', "_"))),
            superuser: r.superuser,
            action: r.action,
            reason: r.reason,
        })
        .collect())
}

/// The notice or error a privacy action left (`?did=`), or `None` when the
/// key is not one of these.
pub fn notice(did: &str, email: &str) -> Option<(Option<String>, Option<String>)> {
    let ok = |key: &str| Some((Some(crate::t1(key, "email", email)), None));
    let err = |key: &str| Some((None, Some(crate::t1(key, "email", email))));
    match did {
        "export-queued" => ok("superuser.privacy.export_queued"),
        "export-busy" => err("superuser.privacy.export_busy"),
        "export-soon" => err("superuser.privacy.export_soon"),
        // The forgotten address is not echoed back in the URL or the page.
        "forgot" => Some((Some(crate::t("superuser.privacy.forgot")), None)),
        "has-account" => Some((None, Some(crate::t("superuser.privacy.has_account")))),
        "no-reason" => Some((None, Some(crate::t("superuser.privacy.no_reason")))),
        _ => None,
    }
}

#[derive(Deserialize)]
pub struct RequestForm {
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub reason: String,
}

/// POST /superuser/privacy/export (email, reason): the account's data
/// export, as from its own profile. The link goes to the account's own
/// address only. Superusers on the hosted site only: a 404 otherwise.
pub async fn export(State(state): State<Arc<AppState>>, user: CurrentUser, headers: HeaderMap, Form(form): Form<RequestForm>) -> Result<Response, AppError> {
    if !moderates(&user) {
        return Err(AppError::NotFound);
    }
    let email = form.email.trim().to_lowercase();
    let Some(reason) = requests::reason(&form.reason) else { return Ok(done("no-reason", &email)) };
    let e = email.clone();
    let Some(target) = state.db.call(move |c| crate::owner::find(c, &e)).await? else {
        return Ok(done("unknown", &email));
    };
    let base = crate::verify::public_base(&state, &headers);
    let su = user.id;
    let r = state
        .db
        .call(move |c| {
            let now = crate::db::now();
            let r = crate::exports::request(c, target, &base, now)?;
            if matches!(r, crate::exports::Requested::Queued(_)) {
                requests::record(c, su, Action::Export, &reason, now)?;
            }
            Ok(r)
        })
        .await?;
    let did = match r {
        crate::exports::Requested::Queued(id) => {
            state.queue.notify();
            tracing::info!("superuser {su} started export #{id} for user {target} (privacy request)");
            "export-queued"
        }
        crate::exports::Requested::InFlight => "export-busy",
        crate::exports::Requested::TooSoon(_) => "export-soon",
    };
    Ok(done(did, &email))
}

/// POST /superuser/privacy/forget (email, reason): the course invitations
/// to an address no account has, and the mails to it, are deleted.
pub async fn forget(State(state): State<Arc<AppState>>, user: CurrentUser, Form(form): Form<RequestForm>) -> Result<Response, AppError> {
    if !moderates(&user) {
        return Err(AppError::NotFound);
    }
    let email = form.email.trim().to_lowercase();
    let Some(reason) = requests::reason(&form.reason) else { return Ok(done("no-reason", "")) };
    if !email.contains('@') {
        return Ok(done("unknown", &email));
    }
    let (e, su) = (email.clone(), user.id);
    let r = state
        .db
        .call(move |c| {
            let r = requests::forget(c, &e)?;
            if let Forgot::Done { .. } = r {
                requests::record(c, su, Action::Forget, &reason, crate::db::now())?;
            }
            Ok(r)
        })
        .await?;
    Ok(match r {
        Forgot::HasAccount => done("has-account", ""),
        Forgot::Done { shares, mails } => {
            tracing::info!("superuser {su} forgot an address ({shares} invitations, {mails} mails)");
            done("forgot", "")
        }
    })
}
