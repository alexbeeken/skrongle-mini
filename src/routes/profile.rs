//! `/profile`: the page behind the gear in the top-right corner. Personal
//! information, account details, password changes, this month's
//! AI usage (allowance, credits). The AI API keys and the switch that hides
//! the AI features are the server's, on the owner's Manage Users page (#25,
//! `routes/manage.rs`). It takes `CurrentUser`, not `Project`: a profile
//! exists before any course is chosen.

use askama::Template;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use axum::{Form, Json};
use serde::Deserialize;
use std::sync::Arc;

use super::Page;
use crate::auth::{self, CurrentUser};
use crate::error::AppError;
use crate::profile::{self, Profile};
use crate::AppState;

pub mod apps;

#[derive(Template)]
#[template(path = "profile.html")]
pub struct ProfileTemplate {
    pub user: Option<CurrentUser>,
    pub profile: Profile,
    pub display_name: String,
    /// How other learners see the name being edited ("Ana L."), for the privacy note.
    pub public_name: String,
    pub notice: Option<String>,
    /// Which form failed, so the message sits next to it.
    pub password_error: Option<String>,
    /// Dev mode: show the dev authenticator link.
    pub dev: bool,
    /// Two-factor sign-in: why the chosen method was refused.
    pub two_factor_error: Option<String>,
    /// The authenticator app's enrolment dialog opens on load (just started,
    /// or a wrong code), and why its code was refused.
    pub totp_open: bool,
    pub totp_error: Option<String>,
    /// The enrolment under way, decrypted only when its dialog opens.
    pub totp_key: Option<crate::totp::Key>,
    /// The "confirm it's you" dialog (turning two-factor off or switching
    /// it): open on load after a refusal, the method asked for (`''` off),
    /// whether it came from the app's Remove, and why it was refused.
    pub confirm_open: bool,
    /// The change the confirm dialog asks about: turning it on (`true`) or off.
    pub confirm_on: bool,
    pub confirm_remove: bool,
    pub confirm_error: Option<String>,
    /// The Connected apps section (#167, `profile/apps.rs`); `None` while
    /// it is not shown (the switch is off, or no Create courses).
    pub apps: Option<apps::Apps>,
}

impl ProfileTemplate {
    /// The signed-in account is an owner (`users.owner`): the profile says so.
    pub fn is_owner(&self) -> bool {
        self.user.as_ref().is_some_and(|u| u.owner)
    }

    /// Its study progress is not shown to the people who manage the server
    /// (the progress page on Manage Users, #147): an owner's never is, nor
    /// that of an account with the Privacy permission.
    pub fn progress_private(&self) -> bool {
        self.user.as_ref().is_some_and(|u| u.owner || u.perms.has(crate::perms::Perm::Privacy))
    }

    /// How this account proves it is its owner (`prove_owner`): `password`;
    /// an account without one a `code` from its app, or, without a second
    /// step, `none`.
    pub fn proof_kind(&self) -> &'static str {
        match (self.profile.has_password, self.profile.two_factor.enabled) {
            (true, _) => "password",
            (false, true) => "code",
            (false, false) => "none",
        }
    }

    /// The second step is on.
    pub fn two_factor_on(&self) -> bool {
        self.profile.two_factor.enabled
    }

    /// When the authenticator app was set up, if it is.
    pub fn totp_since(&self) -> Option<String> {
        self.profile.two_factor.totp_since.map(profile::long_date)
    }

    /// The enrolment under way: its key as the app is handed it.
    fn pending_key(&self) -> Option<crate::totp::Key> {
        self.totp_key.clone()
    }

    /// The confirm dialog's action: the method form, or the app's Remove.
    pub fn confirm_action(&self) -> String {
        crate::url(if self.confirm_remove { "/profile/two-factor/totp/remove" } else { "/profile/two-factor" })
    }

    /// What the confirm dialog says the change is.
    pub fn confirm_what(&self) -> String {
        if self.confirm_on && !self.confirm_remove {
            crate::t("profile.two_factor.turn_on_what")
        } else {
            crate::t("profile.two_factor.turn_off")
        }
    }

    /// The QR code of the link (#304), for the phone's authenticator app to scan.
    pub fn totp_qr(&self) -> String {
        crate::totp::qr_svg(&self.totp_uri(), &crate::t("profile.two_factor.qr_title")).unwrap_or_default()
    }

    /// The setup key in groups of four, as apps show it.
    pub fn totp_secret(&self) -> String {
        let raw = self.pending_key().map(|k| k.secret_base32()).unwrap_or_default();
        raw.as_bytes().chunks(4).map(|c| String::from_utf8_lossy(c).into_owned()).collect::<Vec<_>>().join(" ")
    }

    pub fn totp_uri(&self) -> String {
        self.pending_key().map(|k| k.uri()).unwrap_or_default()
    }
}

#[derive(Deserialize)]
pub struct ShowQuery {
    pub saved: Option<String>,
    /// `enrol`: an authenticator app's enrolment just started.
    pub totp: Option<String>,
}

#[derive(Deserialize)]
pub struct PasswordForm {
    #[serde(default)]
    pub current: String,
    #[serde(default)]
    pub new: String,
    #[serde(default)]
    pub confirm: String,
}

async fn load_profile(state: &AppState, user: &CurrentUser) -> Result<Profile, AppError> {
    let user_id = user.id;
    state.db.call(move |c| profile::load(c, user_id)).await?.ok_or(AppError::RedirectToLogin)
}

fn page(state: &AppState, user: CurrentUser, p: Profile, apps: Option<apps::Apps>) -> ProfileTemplate {
    ProfileTemplate {
        display_name: profile::display_name(&p.first_name, &p.last_name),
        public_name: profile::public_name(&p.first_name, &p.last_name),
        user: Some(user),
        profile: p,
        notice: None,
        password_error: None,
        two_factor_error: None,
        totp_open: false,
        totp_error: None,
        totp_key: None,
        confirm_open: false,
        confirm_on: false,
        confirm_remove: false,
        confirm_error: None,
        apps,
        dev: state.dev_mode(),
    }
}

/// GET /profile
pub async fn show(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Query(q): Query<ShowQuery>,
) -> Result<Response, AppError> {
    let p = load_profile(&state, &user).await?;
    let apps = apps::load(&state, &user).await?;
    let mut t = page(&state, user, p, apps);
    t.notice = match q.saved.as_deref() {
        Some("password") => Some(crate::t("profile.password.changed")),
        Some("password-apps") => Some(crate::t("apps.password_changed")),
        Some("app-revoked") => Some(crate::t("apps.revoked")),
        Some("two-factor") => Some(crate::t(if t.profile.two_factor.enabled { "profile.two_factor.is_on" } else { "profile.two_factor.is_off" })),
        Some("totp-removed") => Some(crate::t("profile.two_factor.totp_removed")),
        Some("signed-out") => Some(crate::t("profile.sessions.signed_out_notice")),
        _ => None,
    };
    if q.totp.as_deref() == Some("enrol") && t.profile.two_factor.totp_pending {
        t.totp_key = pending_totp_key(&state, user_id(&t)).await?;
        t.totp_open = t.totp_key.is_some();
    }
    Ok(Page(t).into_response())
}

/// What the page autosaves (`static/autosave.js`): any of the fields, as
/// they change. Left out, a field keeps its stored value.
#[derive(Deserialize)]
pub struct ProfileChanges {
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    /// The language the app speaks to this account in (`i18n::languages`).
    pub lang: Option<String>,
    /// Anything else (the AI API keys are the server's now, on Manage
    /// Users): refused, field by field.
    #[serde(flatten)]
    pub other: std::collections::HashMap<String, serde_json::Value>,
}

/// POST /api/profile: save the fields sent. All or nothing: a field that
/// does not validate comes back as `422 {"errors": {field: message}}` and
/// nothing is stored. Saved, the answer has what the page redraws: the
/// name.
pub async fn save(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Json(changes): Json<ProfileChanges>,
) -> Result<Response, AppError> {
    let mut errors = serde_json::Map::new();
    let mut check = |field: &str, r: Result<String, String>| match r {
        Ok(v) => Some(v),
        Err(e) => {
            errors.insert(field.into(), e.into());
            None
        }
    };
    let first = changes.first_name.as_deref().and_then(|v| check("first_name", profile::validate_name(&crate::t("profile.personal.first_name"), v)));
    let last = changes.last_name.as_deref().and_then(|v| check("last_name", profile::validate_name(&crate::t("profile.personal.last_name"), v)));
    for field in changes.other.keys() {
        check(field, Err(crate::t("profile.save.not_a_field")));
    }
    let lang = changes.lang.as_deref().map(str::trim).and_then(|v| {
        let ok = if crate::i18n::is_language(v) { Ok(v.to_string()) } else { Err(crate::t("profile.personal.language_unavailable")) };
        check("lang", ok)
    });
    if !errors.is_empty() {
        let body = serde_json::json!({ "errors": errors });
        return Ok((StatusCode::UNPROCESSABLE_ENTITY, Json(body)).into_response());
    }

    let uid = user.id;
    let p = state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            profile::update(&tx, uid, first.as_deref(), last.as_deref())?;
            if let Some(l) = &lang {
                profile::set_lang(&tx, uid, l)?;
            }
            let p = profile::load(&tx, uid)?;
            tx.commit()?;
            Ok(p)
        })
        .await?;
    let p = p.ok_or(AppError::Unauthorized)?;
    tracing::info!("user {uid} updated their profile");

    Ok(Json(serde_json::json!({ "display_name": profile::display_name(&p.first_name, &p.last_name) })).into_response())
}

// ---------- two-factor sign-in (two_factor.rs) ----------

/// The profile page again, with something to say (a refused code, …).
async fn profile_with(state: &AppState, user: CurrentUser, status: StatusCode, f: impl FnOnce(&mut ProfileTemplate)) -> Result<Response, AppError> {
    let p = load_profile(state, &user).await?;
    let apps = apps::load(state, &user).await?;
    let mut t = page(state, user, p, apps);
    f(&mut t);
    Ok((status, Page(t)).into_response())
}

fn user_id(t: &ProfileTemplate) -> i64 {
    t.user.as_ref().map(|u| u.id).unwrap_or_default()
}

/// The authenticator app being set up, decrypted for its dialog.
async fn pending_totp_key(state: &AppState, uid: i64) -> Result<Option<crate::totp::Key>, AppError> {
    let vault = state.vault.clone();
    state.db.call(move |c| crate::two_factor::pending_key(c, &vault, uid)).await
}

#[derive(Deserialize)]
pub struct TwoFactorForm {
    /// `on` or `off` (`users.two_factor` is stored as `totp` or `''`).
    #[serde(default)]
    pub two_factor: String,
    /// Turning two-factor off or switching it: the account's password, or
    /// (an account without one) a code from the app.
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub code: String,
}

fn parse_on(v: &str) -> Result<bool, AppError> {
    match v {
        "on" => Ok(true),
        "" | "off" => Ok(false),
        _ => Err(AppError::BadRequest("two_factor is on or off".into())),
    }
}

/// Is it the account's owner, not just someone with the session? The
/// password, checked like `/login` (argon2 off the async workers, limited
/// per account, a wrong one counting against sign-in, refused while sign-in
/// is blocked); an account without one (signed in through a proxy) gives a
/// code from its current second step instead, or without one nothing: the
/// proxy vouches for it on every request. Every account change that a stolen session must not
/// make goes through here. `Some((status, message))` refuses.
async fn prove_owner(state: &AppState, user: &CurrentUser, password: &str, code: &str) -> Result<Option<(StatusCode, String)>, AppError> {
    use crate::security::{LOGIN_FAILURES_PER_USER, PASSWORD_PER_USER};
    let (uid, now) = (user.id, crate::db::now());
    if !state.limits.hit(PASSWORD_PER_USER, &uid.to_string(), now) || state.limits.blocked(LOGIN_FAILURES_PER_USER, &user.email, now) {
        tracing::warn!("account change rate limit hit for user {uid}");
        return Ok(Some((StatusCode::TOO_MANY_REQUESTS, crate::t("profile.two_factor.too_many_attempts"))));
    }
    let hash = state.db.call(move |c| profile::password_hash(c, uid)).await?.ok_or(AppError::RedirectToLogin)?;
    if !hash.is_empty() {
        if password.is_empty() {
            return Ok(Some((StatusCode::FORBIDDEN, crate::t("profile.two_factor.type_password"))));
        }
        let ok = !auth::password_too_long(password) && {
            auth::verify_password_async(password.to_string(), hash).await?
        };
        if !ok {
            let _ = state.limits.hit(LOGIN_FAILURES_PER_USER, &user.email, now);
            tracing::info!("user {uid} gave a wrong password to change their account");
            return Ok(Some((StatusCode::FORBIDDEN, crate::t("profile.two_factor.password_wrong"))));
        }
        return Ok(None);
    }
    if !state.db.call(move |c| crate::two_factor::settings(c, uid)).await?.enabled {
        // No password and no second step: nothing to ask.
        return Ok(None);
    }
    if code.trim().is_empty() {
        return Ok(Some((StatusCode::FORBIDDEN, crate::t("profile.two_factor.type_code"))));
    }
    let (code, vault) = (code.to_string(), state.vault.clone());
    let checked = state.db.call(move |c| crate::two_factor::check_confirm_code(c, &vault, uid, &code, now)).await?;
    use crate::two_factor::Checked;
    Ok(match checked {
        Checked::Ok => None,
        Checked::Wrong(left) if left < crate::two_factor::MAX_ATTEMPTS => {
            let _ = state.limits.hit(LOGIN_FAILURES_PER_USER, &user.email, now);
            Some((StatusCode::FORBIDDEN, crate::tn("profile.two_factor.code_wrong_tries", left)))
        }
        Checked::Wrong(_) => {
            let _ = state.limits.hit(LOGIN_FAILURES_PER_USER, &user.email, now);
            Some((StatusCode::FORBIDDEN, crate::t("profile.two_factor.code_wrong_newest")))
        }
        Checked::Gone => Some((StatusCode::FORBIDDEN, crate::t("profile.two_factor.code_gone"))),
    })
}

/// The profile again with the confirm dialog open.
async fn confirm_again(state: &AppState, user: CurrentUser, status: StatusCode, on: bool, remove: bool, error: Option<String>) -> Result<Response, AppError> {
    profile_with(state, user, status, move |t| {
        t.confirm_open = true;
        t.confirm_on = on;
        t.confirm_remove = remove;
        t.confirm_error = error;
    })
    .await
}

/// After two-factor sign-in changed (#374, #375): every other session ends.
async fn two_factor_changed(state: &AppState, user: &CurrentUser, keep: Option<String>) -> Result<(), AppError> {
    let (uid, keep) = (user.id, keep.unwrap_or_default());
    let ended = state.db.call(move |c| auth::delete_other_sessions(c, uid, &keep)).await?;
    tracing::info!("user {uid}: two-factor sign-in changed, {ended} other sessions signed out");
    Ok(())
}

/// POST /profile/two-factor (two_factor = on | off, password | code): the second sign-in
/// step. Every change, turning it on too, needs proof that
/// it is the owner (`prove_owner`, #374), then signs out the other devices.
/// Turning it on, until an app is set up,
/// starts its enrolment instead (the dialog with its key opens).
pub async fn set_two_factor(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    auth::SessionToken(token): auth::SessionToken,
    Form(f): Form<TwoFactorForm>,
) -> Result<Response, AppError> {
    use crate::two_factor;
    let on = parse_on(&f.two_factor)?;
    let uid = user.id;
    let current = state.db.call(move |c| two_factor::settings(c, uid)).await?;
    if !two_factor::needs_proof(current.enabled, on) {
        return Ok(Redirect::to(&crate::url("/profile?saved=two-factor#two-factor")).into_response());
    }
    if let Some((status, why)) = prove_owner(&state, &user, &f.password, &f.code).await? {
        return confirm_again(&state, user, status, on, false, Some(why)).await;
    }
    if on && current.totp_since.is_none() {
        let vault = state.vault.clone();
        state.db.call(move |c| two_factor::totp_start(c, &vault, uid, crate::db::now())).await?.map_err(AppError::Internal)?;
        tracing::info!("user {uid} started setting up an authenticator app");
        return Ok(Redirect::to(&crate::url("/profile?totp=enrol#two-factor")).into_response());
    }
    // On with the app already set up, or off.
    state.db.call(move |c| two_factor::set_enabled(c, uid, on)).await?;
    tracing::info!("user {uid} set two-factor sign-in {}", if on { "on" } else { "off" });
    two_factor_changed(&state, &user, token).await?;
    Ok(Redirect::to(&crate::url("/profile?saved=two-factor#two-factor")).into_response())
}

/// POST /profile/two-factor/totp/confirm (code): the app's first code
/// finishes the enrolment (which `prove_owner` let start) and makes it the
/// second step; the other devices are signed out and the account told.
pub async fn confirm_totp(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    auth::SessionToken(token): auth::SessionToken,
    Form(f): Form<CodeForm>,
) -> Result<Response, AppError> {
    let uid = user.id;
    if !state.limits.hit(crate::security::PASSWORD_PER_USER, &uid.to_string(), crate::db::now()) {
        let key = pending_totp_key(&state, uid).await?;
        return profile_with(&state, user, StatusCode::TOO_MANY_REQUESTS, |t| {
            t.totp_open = key.is_some();
            t.totp_key = key;
            t.totp_error = Some(crate::t("profile.two_factor.too_many_attempts"));
        })
        .await;
    }
    let (code, vault) = (f.code, state.vault.clone());
    if state.db.call(move |c| crate::two_factor::totp_confirm(c, &vault, uid, &code, crate::db::now())).await? {
        tracing::info!("user {uid} set up an authenticator app");
        two_factor_changed(&state, &user, token).await?;
        return Ok(Redirect::to(&crate::url("/profile?saved=two-factor#two-factor")).into_response());
    }
    let key = pending_totp_key(&state, uid).await?;
    profile_with(&state, user, StatusCode::UNPROCESSABLE_ENTITY, |t| {
        t.totp_open = key.is_some();
        t.totp_key = key;
        t.totp_error = Some(crate::t("profile.two_factor.code_wrong_newest"));
    })
    .await
}

#[derive(Deserialize)]
pub struct CodeForm {
    #[serde(default)]
    pub code: String,
}

/// POST /profile/two-factor/totp/remove (password | code): forget the
/// authenticator app. While it is the second step that turns two-factor
/// sign-in off, so it needs the password like choosing Off.
pub async fn remove_totp(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    auth::SessionToken(token): auth::SessionToken,
    Form(f): Form<TwoFactorForm>,
) -> Result<Response, AppError> {
    let uid = user.id;
    let current = state.db.call(move |c| crate::two_factor::settings(c, uid)).await?;
    let in_use = current.enabled;
    if in_use {
        if let Some((status, why)) = prove_owner(&state, &user, &f.password, &f.code).await? {
            return confirm_again(&state, user, status, false, true, Some(why)).await;
        }
    }
    state.db.call(move |c| crate::two_factor::totp_remove(c, uid)).await?;
    tracing::info!("user {uid} removed their authenticator app");
    if in_use {
        two_factor_changed(&state, &user, token).await?;
    }
    Ok(Redirect::to(&crate::url("/profile?saved=totp-removed#two-factor")).into_response())
}

/// POST /profile/password
pub async fn change_password(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    auth::SessionToken(token): auth::SessionToken,
    Form(form): Form<PasswordForm>,
) -> Result<Response, AppError> {
    let p = load_profile(&state, &user).await?;
    let apps = apps::load(&state, &user).await?;
    let fail = |state: &AppState, user: CurrentUser, p: Profile, msg: &str| {
        let mut t = page(state, user, p, apps);
        t.password_error = Some(msg.to_string());
        Page(t).into_response()
    };
    if !p.has_password {
        return Ok(fail(&state, user, p, &crate::t("profile.password.none")));
    }
    if form.new != form.confirm {
        return Ok(fail(&state, user, p, &crate::t("profile.password.mismatch")));
    }
    if let Err(e) = auth::validate_password(&form.new) {
        return Ok(fail(&state, user, p, &e));
    }
    // A stolen session must not become a fast way to guess the current
    // password: checked like every proof of ownership, on the sign-in's
    // per-account counter too (#378).
    if let Some((status, why)) = prove_owner(&state, &user, &form.current, "").await? {
        // A wrong password is the form again with its error (200, as ever); only the limit is a 429.
        let (status, msg) = if status == StatusCode::FORBIDDEN { (StatusCode::OK, crate::t("profile.password.current_wrong")) } else { (status, why) };
        let page = fail(&state, user, p, &msg);
        return Ok((status, page).into_response());
    }

    let uid = user.id;
    let new = form.new;
    let new_hash = auth::hash_password_async(new).await?;
    // Every other session (another device, or whoever had the old password)
    // ends, and every connected app is disconnected (#167).
    let (ended, apps) = state
        .db
        .call(move |c| {
            profile::set_password_hash(c, uid, &new_hash)?;
            let ended = auth::delete_other_sessions(c, uid, token.as_deref().unwrap_or(""))?;
            Ok((ended, auth::api::revoke_all_for_user(c, uid)?))
        })
        .await?;
    tracing::info!("user {} changed their password ({ended} other sessions signed out, {apps} connected apps revoked)", user.id);
    let saved = if apps > 0 { "password-apps" } else { "password" };
    Ok(Redirect::to(&crate::url(&format!("/profile?saved={saved}"))).into_response())
}

/// POST /profile/sessions/end-others: "Sign out everywhere else" (#375).
/// Every other session of the account ends; this one stays. Needs no
/// proof: it only takes access away.
pub async fn end_other_sessions(State(state): State<Arc<AppState>>, user: CurrentUser, auth::SessionToken(token): auth::SessionToken) -> Result<Response, AppError> {
    let (uid, keep) = (user.id, token.unwrap_or_default());
    let ended = state.db.call(move |c| auth::delete_other_sessions(c, uid, &keep)).await?;
    tracing::info!("user {uid} signed out everywhere else ({ended} sessions)");
    Ok(Redirect::to(&crate::url("/profile?saved=signed-out#account")).into_response())
}


#[cfg(test)]
mod tests;
