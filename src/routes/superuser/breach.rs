//! The Breach card of `/superuser` (#1068, hosted only, like moderation):
//! sign every account (or a list) out, and e-mail a notice to every account
//! (or a list) (`breach.rs`). Both ask for a reason, written to the privacy
//! log, and for a typed word before they run.

use axum::extract::State;
use axum::response::{IntoResponse, Redirect, Response};
use axum::Form;
use serde::Deserialize;
use std::sync::Arc;

use super::moderates;
use crate::auth::{self, CurrentUser};
use crate::breach::{self, Who};
use crate::error::AppError;
use crate::privacy_requests::{self as requests, Action};
use crate::AppState;

/// What the card's dialogs say Everyone means.
pub struct Card {
    pub accounts: i64,
    pub confirmed: i64,
}

pub async fn card(state: &AppState) -> Result<Card, AppError> {
    let (accounts, confirmed) = state.db.call(|c| breach::counts(c)).await?;
    Ok(Card { accounts, confirmed })
}

/// The word to type before a mass action, in the page's language.
pub fn confirm_word() -> String {
    crate::t("superuser.breach.confirm_word")
}

/// The notice or error a breach action left (`?did=`), with its counts.
pub fn notice(did: &str, n: i64, m: i64, k: i64) -> Option<(Option<String>, Option<String>)> {
    let err = |key: &str| Some((None, Some(crate::t(key))));
    match did {
        "breach-signed-out" => Some((Some(format!("{} {}", crate::tn("superuser.breach.signed_out", n), crate::tn("superuser.breach.sessions_ended", m))), None)),
        "breach-sent" => {
            let mut s = crate::tn("superuser.breach.sent", n);
            if m > 0 {
                s = format!("{s} {}", crate::tn("superuser.breach.sent_already", m));
            }
            if k > 0 {
                s = format!("{s} {}", crate::tn("superuser.breach.sent_left", k));
            }
            Some((Some(s), None))
        }
        "breach-confirm" => Some((None, Some(crate::t1("superuser.breach.confirm_wrong", "word", confirm_word())))),
        "breach-no-list" => err("superuser.breach.no_list"),
        "breach-no-text" => err("superuser.breach.no_text"),
        _ => None,
    }
}

fn back(did: &str, n: usize, m: usize, k: usize) -> Response {
    Redirect::to(&crate::url(&format!("/superuser?did={did}&n={n}&m={m}&k={k}#breach"))).into_response()
}

fn refused(did: &str) -> Response {
    Redirect::to(&crate::url(&format!("/superuser?did={did}#breach"))).into_response()
}

#[derive(Deserialize)]
pub struct SignOutForm {
    /// `everyone` or `list`.
    #[serde(default)]
    pub who: String,
    #[serde(default)]
    pub list: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub confirm: String,
}

/// The accounts the form names, the reason, or the refusal to show.
fn checked(who: &str, list: &str, reason: &str, confirm: &str) -> Result<(Who, String), Response> {
    let Some(reason) = requests::reason(reason) else { return Err(refused("no-reason")) };
    if !confirm.trim().eq_ignore_ascii_case(confirm_word().trim()) {
        return Err(refused("breach-confirm"));
    }
    let who = if who == "everyone" {
        Who::Everyone
    } else {
        let list = breach::parse_list(list);
        if list.is_empty() {
            return Err(refused("breach-no-list"));
        }
        Who::List(list)
    };
    Ok((who, reason))
}

/// POST /superuser/breach/sign-out (who, list, reason, confirm): every
/// account, or the listed ones, is signed out; this session stays.
/// Superusers on the hosted site only: a 404 otherwise.
pub async fn sign_out(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    auth::SessionToken(token): auth::SessionToken,
    Form(form): Form<SignOutForm>,
) -> Result<Response, AppError> {
    if !moderates(&user) {
        return Err(AppError::NotFound);
    }
    let (who, reason) = match checked(&form.who, &form.list, &form.reason, &form.confirm) {
        Ok(v) => v,
        Err(r) => return Ok(r),
    };
    let (su, keep) = (user.id, token.unwrap_or_default());
    let out = state
        .db
        .call(move |c| {
            let out = breach::sign_out(c, &who, &keep)?;
            requests::record(c, su, Action::SignOut, &reason, crate::db::now())?;
            Ok(out)
        })
        .await?;
    tracing::warn!("superuser {su} signed out {} account(s), {} session(s) ended (breach tools)", out.accounts, out.sessions);
    Ok(back("breach-signed-out", out.accounts, out.sessions, 0))
}

#[derive(Deserialize)]
pub struct NoticeForm {
    #[serde(default)]
    pub who: String,
    #[serde(default)]
    pub list: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub confirm: String,
}

/// POST /superuser/breach/notice (who, list, subject, body, reason,
/// confirm): the notice is queued for every account with a confirmed
/// address that has not had this text. Superusers on the hosted site, on a
/// server that sends mail: a 404 otherwise.
pub async fn send_notice(State(state): State<Arc<AppState>>, user: CurrentUser, Form(form): Form<NoticeForm>) -> Result<Response, AppError> {
    if !moderates(&user) || !state.caps().mail {
        return Err(AppError::NotFound);
    }
    let subject: String = form.subject.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(breach::MAX_SUBJECT).collect();
    let body: String = form.body.replace("\r\n", "\n").trim().chars().take(breach::MAX_BODY).collect();
    if subject.is_empty() || body.is_empty() {
        return Ok(refused("breach-no-text"));
    }
    let (who, reason) = match checked(&form.who, &form.list, &form.reason, &form.confirm) {
        Ok(v) => v,
        Err(r) => return Ok(r),
    };
    let su = user.id;
    let ceiling = state.mail.ceiling();
    let sent = state
        .db
        .call(move |c| {
            let now = crate::db::now();
            let sent = breach::send_notice(c, &who, &subject, &body, now, ceiling)?;
            if sent.queued > 0 {
                requests::record(c, su, Action::Notice, &reason, now)?;
            }
            Ok(sent)
        })
        .await?;
    if sent.queued > 0 {
        state.mail.notify();
    }
    tracing::warn!("superuser {su} queued a breach notice for {} account(s) ({} had it, {} left for later)", sent.queued, sent.already, sent.left);
    Ok(back("breach-sent", sent.queued, sent.already, sent.left))
}
