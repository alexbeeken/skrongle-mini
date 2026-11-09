//! Registration, login, logout.

use askama::Template;
use axum::extract::{Query, State};
use axum::http::header::{COOKIE, SET_COOKIE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{AppendHeaders, IntoResponse, Redirect, Response};
use axum::Form;
use serde::Deserialize;
use std::sync::Arc;

use super::Page;
use crate::auth::{self, CurrentUser, MaybeUser, SessionToken};
use crate::db::now;
use crate::error::AppError;
use crate::profile;
use crate::security::{self, ClientIp};
use crate::signup::{self, Admit, Refusal};
use crate::two_factor;
use crate::AppState;

mod code;
pub use code::*;

#[derive(Template)]
#[template(path = "login.html")]
pub struct LoginTemplate {
    pub user: Option<CurrentUser>,
    pub error: Option<String>,
    pub email: String,
    /// Dev mode: offer a one-click login as the dev user.
    pub dev: bool,
    /// A course's share link brought the visitor here (`?share=<token>`,
    /// `share.rs`): the page says so, and signing in or registering goes
    /// on to the link.
    pub share: Option<ShareNotice>,
}

impl LoginTemplate {
    /// The other sign-in page, keeping the share link if there is one.
    pub fn other_page_url(&self, path: &str) -> String {
        other_page_url(&self.share, path)
    }
}

/// The share link a sign-in or registration continues to.
#[derive(Clone)]
pub struct ShareNotice {
    pub token: String,
    pub title: String,
    pub message: String,
}

impl ShareNotice {
    /// Where to go once signed in.
    pub fn next(&self) -> String {
        format!("/courses/shared/{}", crate::urlenc::urlencode(&self.token))
    }

    /// The URL of `path` (the other sign-in page) that keeps the link: `?share=<token>`.
    pub fn page_url(&self, path: &str) -> String {
        crate::url(&format!("{path}?share={}", crate::urlenc::urlencode(&self.token)))
    }
}

/// The notice for `?share=<token>` (a form's `share` field): the course the
/// link leads to, or `None` for no token. A token that matches no share any
/// more (#1082) gets a notice that says the link no longer works, with no
/// title; signing in still goes on to the link, which leads to the course
/// list with the same notice.
async fn share_notice(state: &AppState, token: &str) -> Result<Option<ShareNotice>, AppError> {
    let token = token.trim().to_string();
    if token.is_empty() || token.len() > 128 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Ok(None);
    }
    let t = token.clone();
    let title = state
        .db
        .call(move |c| {
            let Some(share) = crate::share::by_token(c, &t)? else { return Ok(Some(None)) };
            Ok(crate::courses::course(c, &share.course_id)?.map(|r| Some(r.title)))
        })
        .await?;
    Ok(title.map(|title| match title {
        Some(title) => ShareNotice { message: crate::share::login_notice(&title), title, token },
        None => ShareNotice { message: crate::t("courses.share.link_gone"), title: String::new(), token },
    }))
}

/// `next` after signing in: the share link, else home.
fn after_sign_in(share: Option<&ShareNotice>) -> String {
    share.map(ShareNotice::next).unwrap_or_else(|| "/".to_string())
}

#[derive(Deserialize)]
pub struct LoginQuery {
    /// A course's share link (`share.rs`): the page says what it is for.
    pub share: Option<String>,
}

#[derive(Deserialize)]
pub struct RegisterQuery {
    pub share: Option<String>,
    /// An invitation link's token (`SKRONGLE_SIGNUP=invite`, `signup.rs`).
    pub invite: Option<String>,
}

/// `/register` when this server takes no new account (`signup.rs`): why,
/// and nothing else.
#[derive(Template)]
#[template(path = "register_closed.html")]
pub struct RegisterClosedTemplate {
    pub user: Option<CurrentUser>,
    pub message: String,
}

pub(crate) fn signup_refused(r: Refusal, status: StatusCode) -> Response {
    (status, Page(RegisterClosedTemplate { user: None, message: r.message() })).into_response()
}

/// `/login`, `/register` and `/logout` when a reverse proxy signs people in
/// (`proxy_auth.rs`): no form, only that the proxy does it.
#[derive(Template)]
#[template(path = "proxy_signin.html")]
pub struct ProxySignInTemplate {
    pub user: Option<CurrentUser>,
    /// After `/logout`: signed out, until the next page through the proxy.
    pub signed_out: bool,
}

fn proxy_page(status: StatusCode, signed_out: bool) -> Response {
    (status, Page(ProxySignInTemplate { user: None, signed_out })).into_response()
}

/// May an account be made now, with this invitation token, for `email`
/// (`None` before the address is typed)?
async fn admit(state: &AppState, invite: &str, email: Option<&str>) -> Result<Result<Admit, Refusal>, AppError> {
    let (mode, invite, email, dev) = (state.signup, invite.to_string(), email.map(str::to_string), state.dev_mode());
    state.db.call(move |c| signup::admit(c, mode, &invite, email.as_deref(), now(), dev)).await
}

#[derive(Template)]
#[template(path = "register.html")]
pub struct RegisterTemplate {
    pub user: Option<CurrentUser>,
    pub error: Option<String>,
    pub email: String,
    /// Dev mode: offer a one-click login as the dev user.
    pub dev: bool,
    /// The age the sign-up checkbox states (`SKRONGLE_MIN_AGE`).
    pub min_age: u32,
    /// "I am at least N years old" was ticked when the form came back.
    pub age: bool,
    /// As on the login page: a share link brought the visitor here.
    pub share: Option<ShareNotice>,
    /// The invitation token the form carries ('' for none, `signup.rs`).
    pub invite: String,
    /// The server has no account yet and takes this one as its owner's.
    pub owner_note: bool,
}

impl RegisterTemplate {
    /// The other sign-in page, keeping the share link if there is one.
    pub fn other_page_url(&self, path: &str) -> String {
        other_page_url(&self.share, path)
    }
}

/// `path` (the other sign-in page) with the share link kept, or plain.
fn other_page_url(share: &Option<ShareNotice>, path: &str) -> String {
    share.as_ref().map_or_else(|| crate::url(path), |s| s.page_url(path))
}

#[derive(Deserialize)]
pub struct Credentials {
    pub email: String,
    pub password: String,
    /// The share link's token to continue to (`?share=` on the page).
    #[serde(default)]
    pub share: String,
}

#[derive(Deserialize)]
pub struct Registration {
    #[serde(default)]
    pub email: String,
    pub password: String,
    /// "I am at least N years old": "on" when ticked, absent otherwise.
    #[serde(default)]
    pub age: String,
    /// The share link's token to continue to (`?share=` on the page).
    #[serde(default)]
    pub share: String,
    /// The invitation token (`?invite=` on the page).
    #[serde(default)]
    pub invite: String,
}

pub async fn login_form(State(state): State<Arc<AppState>>, MaybeUser(user): MaybeUser, Query(q): Query<LoginQuery>) -> Result<Response, AppError> {
    let share = share_notice(&state, q.share.as_deref().unwrap_or("")).await?;
    if user.is_some() {
        return Ok(Redirect::to(&crate::url(&after_sign_in(share.as_ref()))).into_response());
    }
    if state.proxy_auth.is_some() {
        return Ok(proxy_page(StatusCode::OK, false));
    }
    Ok(Page(LoginTemplate { user: None, error: None, email: String::new(), dev: state.dev_mode(), share }).into_response())
}

pub async fn register_form(State(state): State<Arc<AppState>>, MaybeUser(user): MaybeUser, Query(q): Query<RegisterQuery>) -> Result<Response, AppError> {
    let share = share_notice(&state, q.share.as_deref().unwrap_or("")).await?;
    if user.is_some() {
        return Ok(Redirect::to(&crate::url(&after_sign_in(share.as_ref()))).into_response());
    }
    if state.proxy_auth.is_some() {
        return Ok(proxy_page(StatusCode::OK, false));
    }
    let invite = q.invite.as_deref().unwrap_or("").trim().to_string();
    let admitted = match admit(&state, &invite, None).await? {
        Ok(a) => a,
        Err(r) => return Ok(signup_refused(r, StatusCode::OK)),
    };
    let email = String::new();
    Ok(Page(RegisterTemplate {
        user: None,
        error: None,
        email,
        dev: state.dev_mode(),
        min_age: crate::signup::min_age(),
        age: false,
        share,
        invite: if matches!(admitted, Admit::Invited(_) | Admit::Owner(_)) { invite } else { String::new() },
        owner_note: matches!(admitted, Admit::Owner(_) | Admit::FirstOpen),
    })
    .into_response())
}

/// Parse an account form's body, after the caller's cheap checks (#370): an
/// address over its rate limit is refused before its body is read.
#[allow(clippy::result_large_err)] // the Err is the finished Response, returned to axum as is; boxing it would only be unboxed again
async fn form_body<T: serde::de::DeserializeOwned>(req: axum::extract::Request) -> Result<T, Response> {
    use axum::extract::FromRequest;
    Form::<T>::from_request(req, &()).await.map(|Form(f)| f).map_err(IntoResponse::into_response)
}

pub async fn login(State(state): State<Arc<AppState>>, ClientIp(ip): ClientIp, req: axum::extract::Request) -> Result<Response, AppError> {
    if state.proxy_auth.is_some() {
        return Ok(proxy_page(StatusCode::FORBIDDEN, false));
    }
    let too_many = |email: String, share: Option<_>| {
        let page = LoginTemplate { user: None, error: Some(crate::t("auth.login.too_many_attempts")), email, dev: state.dev_mode(), share };
        (StatusCode::TOO_MANY_REQUESTS, Page(page)).into_response()
    };
    if state.limits.blocked(security::LOGIN_PER_IP, &ip, now()) {
        tracing::warn!("sign-in rate limit hit from {ip}");
        return Ok(too_many(String::new(), None));
    }
    let form: Credentials = match form_body(req).await {
        Ok(f) => f,
        Err(r) => return Ok(r),
    };
    let email = form.email.trim().to_string();
    let share = share_notice(&state, &form.share).await?;
    let fail = |msg: &str| {
        Page(LoginTemplate {
            user: None,
            error: Some(msg.to_string()),
            email: email.clone(),
            dev: state.dev_mode(),
            share: share.clone(),
        })
        .into_response()
    };

    // Nobody's address or password is this long: no lookup, no hashing, and
    // no rate-limit key is made for it.
    if email.len() > profile::EMAIL_MAX || auth::password_too_long(&form.password) {
        return Ok(fail(&crate::t("auth.login.incorrect")));
    }
    // Before any hashing: a password-guessing run costs the Pi argon2 time too.
    let t = now();
    if !state.limits.hit(security::LOGIN_PER_IP, &ip, t) || state.limits.blocked(security::LOGIN_FAILURES_PER_USER, &email, t) {
        tracing::warn!("sign-in rate limit hit for an account from {ip}");
        return Ok(too_many(email, share));
    }

    let e = email.clone();
    let creds = state.db.call(move |c| auth::find_user_credentials(c, &e)).await?;

    // Verify on the blocking pool: argon2 takes ~100 ms on the Pi and must not
    // stall the (two) async workers. Unknown users still pay the hashing cost so
    // timing doesn't leak whether an account exists.
    let password = form.password;
    let (user_id, hash) = match creds {
        Some((id, hash)) => (Some(id), hash),
        None => (None, state.dummy_hash.clone()),
    };
    let ok = auth::verify_password_async(password, hash).await?;

    let Some(user_id) = user_id.filter(|_| ok) else {
        let _ = state.limits.hit(security::LOGIN_FAILURES_PER_USER, &email, t);
        return Ok(fail(&crate::t("auth.login.incorrect")));
    };

    tracing::info!("user {user_id} gave the right password");
    sign_in(&state, user_id, Vec::new(), share.as_ref()).await
}

pub async fn register(State(state): State<Arc<AppState>>, ClientIp(ip): ClientIp, headers: HeaderMap, req: axum::extract::Request) -> Result<Response, AppError> {
    if state.proxy_auth.is_some() {
        return Ok(proxy_page(StatusCode::FORBIDDEN, false));
    }
    if state.limits.blocked(security::REGISTER_PER_IP, &ip, now()) {
        tracing::warn!("registration rate limit hit from {ip}");
        let page = RegisterTemplate {
            user: None,
            error: Some(crate::t("auth.register.too_many_signups")),
            email: String::new(),
            dev: state.dev_mode(),
            min_age: crate::signup::min_age(),
            age: false,
                share: None,
            invite: String::new(),
            owner_note: false,
        };
        return Ok((StatusCode::TOO_MANY_REQUESTS, Page(page)).into_response());
    }
    let form: Registration = match form_body(req).await {
        Ok(f) => f,
        Err(r) => return Ok(r),
    };
    let email_input = form.email.trim().to_string();
    let share = share_notice(&state, &form.share).await?;
    // Over the limit: refused before the invitation is looked at.
    let within_limit = state.limits.hit(security::REGISTER_PER_IP, &ip, now());
    let admitted = if within_limit {
        match admit(&state, &form.invite, Some(&email_input)).await? {
            Ok(a) => Some(a),
            Err(r) => return Ok(signup_refused(r, StatusCode::FORBIDDEN)),
        }
    } else {
        None
    };
    // A form error on an empty server still says whose account this is (#665).
    let owner_note = matches!(admitted, Some(Admit::Owner(_) | Admit::FirstOpen));
    let fail = |msg: String| {
        Page(RegisterTemplate {
            user: None,
            error: Some(msg),
            email: email_input.clone(),
            dev: state.dev_mode(),
            min_age: crate::signup::min_age(),
            age: !form.age.is_empty(),
            share: share.clone(),
            invite: form.invite.trim().to_string(),
            owner_note,
        })
        .into_response()
    };

    if !within_limit {
        tracing::warn!("registration rate limit hit from {ip}");
        return Ok((StatusCode::TOO_MANY_REQUESTS, fail(crate::t("auth.register.too_many_signups"))).into_response());
    }

    if let Err(e) = crate::signup::AgeGate::current().check(!form.age.is_empty(), "register") {
        return Ok(fail(e));
    }

    let email = match profile::validate_email(&email_input) {
        Ok(e) => e,
        Err(e) => return Ok(fail(e)),
    };
    if let Err(e) = auth::validate_password(&form.password) {
        return Ok(fail(e));
    }
    let password = form.password;
    // The language picked on the register page becomes the account's; with
    // no pick the account follows the browser's language until the profile says.
    let lang = crate::i18n::chosen(&headers).unwrap_or_default();
    let hash = auth::hash_password_async(password).await?;

    // Checked next to the insert, on one connection (so nothing runs in
    // between): another sign-up may have taken the address, or the
    // invitation, while we hashed. The sign-up mode's word comes first, so
    // a refusal never depends on the address.
    let (t, e) = (now(), email.clone());
    let (mode, invite, dev) = (state.signup, form.invite.clone(), state.dev_mode());
    let outcome = state
        .db
        .call(move |c| {
            let admitted = match signup::admit(c, mode, &invite, Some(&e), t, dev)? {
                Ok(a) => a,
                Err(r) => return Ok(Signup::Refused(r)),
            };
            let tx = c.transaction()?;
            if profile::email_taken(&tx, &e, None)? {
                return Ok(Signup::Taken);
            }
            let id = auth::create_user(&tx, &e, &hash)?;
            if !signup::finish(&tx, admitted, id, t)? {
                return Ok(Signup::Refused(Refusal::Used));
            }
            crate::signup::record_age_confirmed(&tx, id, true)?;
            if !lang.is_empty() {
                profile::set_lang(&tx, id, &lang)?;
            }
            let session = auth::create_session(&tx, id)?;
            tx.commit()?;
            Ok(Signup::New(session))
        })
        .await?;
    match outcome {
        Signup::New(token) => {
            tracing::info!("a new user registered");
            Ok(([auth::set_session_cookie(&token, state.secure_cookies)], Redirect::to(&crate::url(&after_sign_in(share.as_ref())))).into_response())
        }
        Signup::Taken => Ok(fail(crate::t("auth.register.email_taken"))),
        Signup::Refused(r) => Ok(signup_refused(r, StatusCode::FORBIDDEN)),
    }
}

/// What `register` did on the database thread.
enum Signup {
    /// The account was made and signed in, with this session token.
    New(String),
    /// The address already has an account: nothing was made.
    Taken,
    /// The sign-up mode said no (`signup.rs`).
    Refused(Refusal),
}

#[derive(Deserialize)]
pub struct LangForm {
    pub lang: String,
    /// The app path to return to (`/login`, `/register`); home otherwise.
    #[serde(default)]
    pub back: String,
}

/// POST /lang: the language picker. Remembered in a cookie for a year and,
/// signed in, on the account (`users.lang`), which wins from then on.
pub async fn set_lang(
    State(state): State<Arc<AppState>>,
    MaybeUser(user): MaybeUser,
    Form(f): Form<LangForm>,
) -> Result<Response, AppError> {
    if !crate::i18n::is_language(&f.lang) {
        return Err(AppError::BadRequest(crate::t("lang.unknown")));
    }
    if let Some(u) = user {
        let lang = f.lang.clone();
        state.db.call(move |c| profile::set_lang(c, u.id, &lang)).await?;
    }
    let back = if f.back.starts_with('/') && !f.back.starts_with("//") { crate::url(&f.back) } else { crate::url("/") };
    Ok(([crate::i18n::set_cookie(&f.lang, state.secure_cookies)], Redirect::to(&back)).into_response())
}

pub async fn logout(
    State(state): State<Arc<AppState>>,
    SessionToken(token): SessionToken,
) -> Result<Response, AppError> {
    if let Some(token) = token {
        state.db.call(move |c| auth::delete_session(c, &token)).await?;
    }
    // Behind a signing-in proxy `/login` would sign them straight back in: say so instead.
    if state.proxy_auth.is_some() {
        return Ok(([auth::clear_session_cookie()], proxy_page(StatusCode::OK, true)).into_response());
    }
    Ok(([auth::clear_session_cookie()], Redirect::to(&crate::url("/login"))).into_response())
}

// ---------- the second step: two-factor sign-in (two_factor.rs) ----------

fn twofa_cookie(value: &str, max_age: i64, secure: bool) -> (axum::http::HeaderName, HeaderValue) {
    let v = format!(
        "{}={value}; Path={}; HttpOnly; SameSite=Lax; Max-Age={max_age}{}",
        two_factor::COOKIE,
        crate::cookie_path(),
        if secure { "; Secure" } else { "" }
    );
    (SET_COOKIE, HeaderValue::from_str(&v).expect("valid cookie header"))
}

fn twofa_token(headers: &HeaderMap) -> Option<String> {
    headers.get_all(COOKIE).iter().find_map(|v| {
        v.to_str().ok()?.split(';').find_map(|pair| {
            let (k, v) = pair.trim().split_once('=')?;
            (k.trim() == two_factor::COOKIE && !v.trim().is_empty()).then(|| v.trim().to_string())
        })
    })
}

/// The password said who this is: sign them in, or, with two-factor
/// sign-in on, ask for the authenticator's code at `/login/code`. `cookies` go
/// out either way. `share`: the course's share link to land on instead of
/// home; a sign-in that goes through its second step carries it in the
/// share cookie (#1016).
async fn sign_in(state: &AppState, user_id: i64, mut cookies: Vec<(axum::http::HeaderName, HeaderValue)>, share: Option<&ShareNotice>) -> Result<Response, AppError> {
    let challenge = state.db.call(move |c| two_factor::start_login(c, user_id, now())).await?;
    let Some(ch) = challenge else {
        let token = state.db.call(move |c| auth::create_session(c, user_id)).await?;
        tracing::info!("user {user_id} logged in");
        cookies.push(auth::set_session_cookie(&token, state.secure_cookies));
        if share.is_some() {
            cookies.push(named_cookie(SHARE_COOKIE, "", 0, state.secure_cookies));
        }
        return Ok((AppendHeaders(cookies), Redirect::to(&crate::url(&after_sign_in(share)))).into_response());
    };
    tracing::info!("user {user_id} is asked for an authenticator code");
    cookies.push(twofa_cookie(&ch.token, two_factor::CODE_TTL_SECS, state.secure_cookies));
    if let Some(s) = share {
        cookies.push(named_cookie(SHARE_COOKIE, &s.token, two_factor::CODE_TTL_SECS, state.secure_cookies));
    }
    Ok((AppendHeaders(cookies), Redirect::to(&crate::url("/login/code"))).into_response())
}

// ---------- cookies that carry an invitation or share link ----------

/// The cookie that carries a share link's token through the second sign-in
/// step (#1016), to land on the link after.
const SHARE_COOKIE: &str = "share_next";

/// An invitation's or share link's token: hex, as `auth::generate_token` makes them.
fn is_hex_token(t: &str) -> bool {
    !t.is_empty() && t.len() <= 128 && t.bytes().all(|b| b.is_ascii_hexdigit())
}

fn named_cookie(name: &str, value: &str, max_age: i64, secure: bool) -> (axum::http::HeaderName, HeaderValue) {
    let v = format!("{name}={value}; Path={}; HttpOnly; SameSite=Lax; Max-Age={max_age}{}", crate::cookie_path(), if secure { "; Secure" } else { "" });
    // Every part is ASCII: a fixed name, a hex token and numbers.
    (SET_COOKIE, HeaderValue::from_str(&v).unwrap_or_else(|_| HeaderValue::from_static("")))
}

/// The hex token in cookie `name` ('' for none).
fn hex_cookie(headers: &HeaderMap, name: &str) -> String {
    headers
        .get_all(COOKIE)
        .iter()
        .find_map(|v| {
            v.to_str().ok()?.split(';').find_map(|pair| {
                let (k, v) = pair.trim().split_once('=')?;
                (k.trim() == name && is_hex_token(v.trim())).then(|| v.trim().to_string())
            })
        })
        .unwrap_or_default()
}
