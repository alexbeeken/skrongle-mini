//! `/manage`: the page for whoever runs the server (owners only,
//! `users.owner`). Every account, newest first, all on one page (the
//! server holds at most `caps::MAX_ACCOUNTS`), with
//! the actions on one: a temporary password, two-factor off and make
//! owner (#635, #16, `owner.rs`), reset e-mail verification (#220); and with
//! `SKRONGLE_SIGNUP=invite`, the invitation links (#634, `signup.rs`); and
//! while a material change of the terms is coming, a notice and the button
//! that e-mails it to every account (#966, `terms.rs`).
//! Anyone else gets a 404, as if the page did not exist; the account menu
//! links here only for owners.

use askama::Template;
use axum::extract::{Query, State};
use axum::http::{header, HeaderMap};
use axum::response::{IntoResponse, Redirect, Response};
use axum::Form;
use serde::Deserialize;
use std::sync::Arc;

use super::Page;
use crate::auth::{self, CurrentUser};
use crate::error::AppError;
use crate::profile;
use crate::AppState;

#[derive(Template)]
#[template(path = "manage.html")]
pub struct ManageTemplate {
    pub user: Option<CurrentUser>,
    /// What the last action did, or why it could not.
    pub notice: Option<String>,
    pub error: Option<String>,
    /// Dev mode with the dev mailbox: where a new confirmation link landed.
    pub inbox: Option<String>,
    /// `SKRONGLE_SIGNUP`: `open`, `invite` or `closed`.
    pub mode: &'static str,
    /// The server can send e-mail: without it there is no confirmation
    /// link to send, so Reset email verification is not offered (#633).
    pub mail: bool,
    /// An invitation link just made: shown this once.
    pub new_link: Option<String>,
    /// A temporary password just set: the account's address and the
    /// password, shown this once.
    pub temp_password: Option<(String, String)>,
    /// Open invitations: id, made, expires.
    pub invites: Vec<(i64, String, String)>,
    /// Every account, newest first.
    pub list: Vec<AccountRow>,
    /// How many accounts there are: the length of `list`.
    pub total: i64,
    /// The most accounts this server holds (`caps::MAX_ACCOUNTS`).
    pub max_accounts: usize,
    /// The server holds them all: no invitation form.
    pub full: bool,
    /// Accounts still to e-mail about a coming change of the terms: above 0,
    /// the page says the terms have changed and offers Send.
    pub terms_owed: i64,
}

pub struct AccountRow {
    pub email: String,
    pub created: String,
    /// '' when no session is on record.
    pub last_sign_in: String,
    pub owner: bool,
}

/// The page, for an owner.
async fn load(state: &AppState, user: CurrentUser) -> Result<ManageTemplate, AppError> {
    let now = crate::db::now();
    let (invites, accounts) = state.db.call(move |c| Ok((crate::signup::open_invites(c, now)?, crate::signup::accounts(c)?))).await?;
    let terms_owed = crate::terms::unannounced(state).await?;
    let total = accounts.len() as i64;
    Ok(ManageTemplate {
        user: Some(user),
        notice: None,
        error: None,
        inbox: None,
        mode: state.signup.as_str(),
        mail: state.caps().mail,
        new_link: None,
        temp_password: None,
        invites: invites.into_iter().map(|i| (i.id, profile::long_date(i.created_at), profile::long_date(i.expires_at))).collect(),
        list: accounts
            .into_iter()
            .map(|a| AccountRow {
                email: a.email,
                created: profile::long_date(a.created_at),
                last_sign_in: a.last_sign_in.map(profile::long_date).unwrap_or_default(),
                owner: a.owner,
            })
            .collect(),
        total,
        max_accounts: crate::caps::MAX_ACCOUNTS,
        full: crate::caps::accounts_full(total, state.dev),
        terms_owed,
    })
}

#[derive(Deserialize)]
pub struct ShowQuery {
    /// Back from an action: `reset`, `unknown`, `revoked`,
    /// `two-factor-off` or `owner`, about `email`; or `terms-sent`, `n` e-mails.
    pub did: Option<String>,
    pub email: Option<String>,
    pub n: Option<i64>,
}

/// GET /manage
pub async fn show(State(state): State<Arc<AppState>>, user: CurrentUser, Query(q): Query<ShowQuery>) -> Result<Response, AppError> {
    if !user.owner {
        return Err(AppError::NotFound);
    }
    let mut t = load(&state, user).await?;
    let email = q.email.as_deref().unwrap_or_default();
    match q.did.as_deref() {
        Some("reset") => {
            t.notice = Some(crate::t1("manage.reset_done", "email", email));
            if state.dev_inbox.is_some() {
                t.inbox = Some(crate::url(&format!("/dev/inbox/latest?to={}", crate::urlenc::urlencode(email))));
            }
        }
        Some("unknown") => t.error = Some(crate::t1("manage.unknown", "email", format!("{email:?}"))),
        Some("revoked") => t.notice = Some(crate::t("manage.invite_revoked")),
        Some("two-factor-off") => t.notice = Some(crate::t1("manage.two_factor_off_done", "email", email)),
        Some("owner") => t.notice = Some(crate::t1("manage.make_owner_done", "email", email)),
        Some("terms-sent") => t.notice = Some(crate::tn("manage.terms_sent", q.n.unwrap_or(0))),
        _ => {}
    }
    Ok(Page(t).into_response())
}

/// Back to the page after an action: what it did, about which address.
fn done(did: &str, email: &str) -> Response {
    let to = format!("/manage?did={did}&email={}", crate::urlenc::urlencode(email));
    Redirect::to(&crate::url(&to)).into_response()
}

#[derive(Deserialize)]
pub struct AccountForm {
    #[serde(default)]
    pub email: String,
}

/// POST /manage/reset-email-verification (email=…): the account with
/// that address is unconfirmed again and gets a new link. Owners only:
/// anyone else gets a 404.
pub async fn reset_email_verification(State(state): State<Arc<AppState>>, user: CurrentUser, headers: HeaderMap, Form(form): Form<AccountForm>) -> Result<Response, AppError> {
    // Without mail there is no link to send: the action is not offered (#633).
    if !user.owner || !state.caps().mail {
        return Err(AppError::NotFound);
    }
    let email = form.email.trim().to_lowercase();
    let did = match crate::verify::reset(&state, &headers, &email).await? {
        Some(target) => {
            tracing::info!("owner {} reset the e-mail verification of user {target}", user.id);
            "reset"
        }
        None => "unknown",
    };
    Ok(done(did, &email))
}

/// POST /manage/invites: a new invitation link (`SKRONGLE_SIGNUP=invite`),
/// shown on the page this once; only its hash is kept. Owners only, and
/// only in invite mode: a 404 otherwise.
pub async fn make_invite(State(state): State<Arc<AppState>>, user: CurrentUser, headers: HeaderMap) -> Result<Response, AppError> {
    if !user.owner || state.signup != crate::signup::Mode::Invite {
        return Err(AppError::NotFound);
    }
    let (uid, dev) = (user.id, state.dev);
    let token = state
        .db
        .call(move |c| Ok(if crate::signup::is_full(c, dev)? { None } else { Some(crate::signup::make(c, uid, crate::db::now())?) }))
        .await?;
    let Some(token) = token else {
        let mut t = load(&state, user).await?;
        t.error = Some(crate::t("auth.signup.full"));
        return Ok(Page(t).into_response());
    };
    tracing::info!("owner {uid} made an invitation link");
    let site = crate::verify::public_base(&state, &headers);
    let link = format!("{site}/register?invite={token}");
    let mut t = load(&state, user).await?;
    t.new_link = Some(link);
    // The link is a key to the server: not to be cached anywhere.
    Ok(([(header::CACHE_CONTROL, "no-store")], Page(t)).into_response())
}

#[derive(Deserialize)]
pub struct RevokeInviteForm {
    #[serde(default)]
    pub id: i64,
}

/// POST /manage/invites/revoke (id=…): an open invitation stops working.
/// Owners only: a 404 otherwise.
pub async fn revoke_invite(State(state): State<Arc<AppState>>, user: CurrentUser, Form(form): Form<RevokeInviteForm>) -> Result<Response, AppError> {
    if !user.owner {
        return Err(AppError::NotFound);
    }
    let id = form.id;
    if state.db.call(move |c| crate::signup::revoke(c, id)).await? {
        tracing::info!("owner {} revoked invitation {id}", user.id);
    }
    Ok(Redirect::to(&crate::url("/manage?did=revoked")).into_response())
}

/// POST /manage/temp-password (email=…): the account gets a generated
/// temporary password (`owner.rs`, #635), shown on the page this once,
/// which it must change at its next sign-in; its sessions end (an owner
/// resetting their own keeps this one). Owners only: a 404 otherwise.
/// The log line never holds the password.
pub async fn set_temp_password(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    auth::SessionToken(token): auth::SessionToken,
    Form(form): Form<AccountForm>,
) -> Result<Response, AppError> {
    if !user.owner {
        return Err(AppError::NotFound);
    }
    let email = form.email.trim().to_lowercase();
    let e = email.clone();
    let Some(target) = state.db.call(move |c| crate::owner::find(c, &e)).await? else {
        return Ok(done("unknown", &email));
    };
    let password = crate::owner::temp_password();
    let hash = auth::hash_password_async(password.clone()).await?;
    let keep = if target == user.id { token } else { None };
    let ended = state.db.call(move |c| crate::owner::set_temporary_password(c, target, &hash, keep.as_deref())).await?;
    tracing::info!("owner {} set a temporary password for user {target} ({ended} sessions signed out)", user.id);
    let mut t = load(&state, user).await?;
    t.temp_password = Some((email, password));
    // The password is a key to the account: not to be cached anywhere.
    Ok(([(header::CACHE_CONTROL, "no-store")], Page(t)).into_response())
}

/// POST /manage/make-owner (email=…): the account becomes an owner too
/// (`owner::make_owner`): it sees this page and its actions. Owners only: a
/// 404 otherwise.
pub async fn make_owner(State(state): State<Arc<AppState>>, user: CurrentUser, Form(form): Form<AccountForm>) -> Result<Response, AppError> {
    if !user.owner {
        return Err(AppError::NotFound);
    }
    let email = form.email.trim().to_lowercase();
    let e = email.clone();
    let Some(target) = state.db.call(move |c| crate::owner::find(c, &e)).await? else {
        return Ok(done("unknown", &email));
    };
    state.db.call(move |c| crate::owner::make_owner(c, target)).await?;
    tracing::info!("owner {} made user {target} an owner", user.id);
    Ok(done("owner", &email))
}

/// POST /manage/two-factor-off (email=…): the account's two-factor
/// sign-in is turned off (`owner::disable_two_factor`). Owners only: a
/// 404 otherwise.
pub async fn turn_off_two_factor(State(state): State<Arc<AppState>>, user: CurrentUser, Form(form): Form<AccountForm>) -> Result<Response, AppError> {
    if !user.owner {
        return Err(AppError::NotFound);
    }
    let email = form.email.trim().to_lowercase();
    let e = email.clone();
    let Some(target) = state.db.call(move |c| crate::owner::find(c, &e)).await? else {
        return Ok(done("unknown", &email));
    };
    state.db.call(move |c| crate::owner::disable_two_factor(c, target)).await?;
    tracing::info!("owner {} turned off two-factor sign-in for user {target}", user.id);
    Ok(done("two-factor-off", &email))
}

/// POST /manage/terms-announce: e-mail the coming material change of the
/// terms to every account that has not had it (`terms::announce_upcoming`).
/// Owners only, on a server that can send mail: a 404 otherwise.
pub async fn announce_terms(State(state): State<Arc<AppState>>, user: CurrentUser) -> Result<Response, AppError> {
    if !user.owner || !state.caps().mail {
        return Err(AppError::NotFound);
    }
    let n = crate::terms::announce_upcoming(&state).await?;
    tracing::info!("owner {} sent the terms change e-mail to {n} account(s)", user.id);
    Ok(Redirect::to(&crate::url(&format!("/manage?did=terms-sent&n={n}"))).into_response())
}
