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
    pub notice: Option<String>,
    /// A course's share link brought the visitor here (`?share=<token>`,
    /// `share.rs`): the page says so, and signing in or registering goes
    /// on to the link.
    pub share: Option<ShareNotice>,
    /// "Forgot your password?" (`/reset`, #379): only where the server
    /// can mail the link (not in Skrongle mini).
    pub reset: bool,
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
    /// Back from *Delete my account* (#970): when the account will be deleted.
    pub deletion: Option<String>,
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
    let (mode, invite, email) = (state.signup, invite.to_string(), email.map(str::to_string));
    state.db.call(move |c| signup::admit(c, mode, &invite, email.as_deref(), now())).await
}

/// The login page's notice after *Delete my account* (#970): the date it
/// goes, and that signing in before then can cancel it.
fn deletion_notice(at: Option<&str>) -> Option<String> {
    let at: i64 = at?.parse().ok().filter(|t| (1..32_503_680_000).contains(t))?;
    Some(crate::t1("auth.login.deletion_scheduled", "date", crate::profile::long_date(at)))
}

#[derive(Template)]
#[template(path = "register.html")]
pub struct RegisterTemplate {
    pub user: Option<CurrentUser>,
    pub error: Option<String>,
    pub email: String,
    /// Dev mode: offer a one-click login as the dev user.
    pub dev: bool,
    /// The age the Terms checkbox states (`SKRONGLE_MIN_AGE`).
    pub min_age: u32,
    /// "Hide all AI features" (#147) was on when the form came back with an error.
    pub no_ai: bool,
    /// "I am at least N years old" was ticked when the form came back.
    pub age: bool,
    /// "I agree to the Terms and Privacy Policy" was ticked when the form came back.
    pub accept: bool,
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
    /// "I agree to the Terms and Privacy Policy": "on" when ticked, absent otherwise.
    #[serde(default)]
    pub accept: String,
    /// "Hide all AI features" (#147): "on" when ticked, absent otherwise.
    #[serde(default)]
    pub no_ai: String,
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
    let notice = deletion_notice(q.deletion.as_deref());
    Ok(Page(LoginTemplate { user: None, reset: state.caps().mail, error: None, email: String::new(), dev: state.dev, notice, share }).into_response())
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
        dev: state.dev,
        min_age: crate::terms::min_age(),
        no_ai: false,
        age: false,
        accept: false,
        share,
        invite: if matches!(admitted, Admit::Invited(_) | Admit::Owner(_)) { invite } else { String::new() },
        owner_note: matches!(admitted, Admit::Owner(_)),
    })
    .into_response())
}

/// Parse an account form's body, after the caller's cheap checks (#370): an
/// address over its rate limit is refused before its body is read.
async fn form_body<T: serde::de::DeserializeOwned>(req: axum::extract::Request) -> Result<T, Response> {
    use axum::extract::FromRequest;
    Form::<T>::from_request(req, &()).await.map(|Form(f)| f).map_err(IntoResponse::into_response)
}

pub async fn login(State(state): State<Arc<AppState>>, ClientIp(ip): ClientIp, req: axum::extract::Request) -> Result<Response, AppError> {
    if state.proxy_auth.is_some() {
        return Ok(proxy_page(StatusCode::FORBIDDEN, false));
    }
    let too_many = |email: String, share: Option<_>| {
        let page = LoginTemplate { user: None, reset: state.caps().mail, error: Some(crate::t("auth.login.too_many_attempts")), email, dev: state.dev, notice: None, share };
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
            reset: state.caps().mail,
            error: Some(msg.to_string()),
            email: email.clone(),
            dev: state.dev,
            notice: None,
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
    let ok = tokio::task::spawn_blocking(move || auth::verify_password(&password, &hash))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

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
            dev: state.dev,
            min_age: crate::terms::min_age(),
            no_ai: false,
            age: false,
            accept: false,
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
    let owner_note = matches!(admitted, Some(Admit::Owner(_)));
    let fail = |msg: String| {
        Page(RegisterTemplate {
            user: None,
            error: Some(msg),
            email: email_input.clone(),
            dev: state.dev,
            min_age: crate::terms::min_age(),
            no_ai: !form.no_ai.is_empty(),
            age: !form.age.is_empty(),
            accept: !form.accept.is_empty(),
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

    if let Err(e) = crate::privacy::AgeGate::current().check(!form.age.is_empty(), !form.accept.is_empty(), "register") {
        return Ok(fail(e));
    }

    let email = match profile::validate_email(&email_input) {
        Ok(e) => e,
        Err(e) => return Ok(fail(e)),
    };
    if let Err(e) = auth::validate_password(&form.password) {
        return Ok(fail(e));
    }
    // With mail, from here on the answer is "check your mail" whether or
    // not the address has an account (#379): the page, the status and the
    // work done (the argon2 hash, one message queued) are the same either
    // way. Only the sign-up mode (`signup.rs`) still refuses, and it never
    // looks at the address. Without mail (Skrongle mini, #633) there is no
    // message to hide behind: a taken address is said so, and the new
    // account is signed in at once.
    let can_mail = state.caps().mail;
    let password = form.password;
    let no_ai = !form.no_ai.is_empty();
    // The language picked on the register page becomes the account's; with
    // no pick the account follows the browser's language until the profile says.
    let lang = crate::i18n::chosen(&headers).unwrap_or_default();
    let hash = tokio::task::spawn_blocking(move || auth::hash_password(&password))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))??;

    // An unconfirmed account old enough gives the address up (only where a
    // confirmation mail could have been read). What lives in memory for it
    // goes first, as on /profile/delete.
    let (t, days) = (now(), if can_mail { crate::reset::unconfirmed_days() } else { 0 });
    let e = email.clone();
    let stale = state.db.call(move |c| crate::reset::replaceable(c, &e, t, days)).await?;
    if let Some(old) = stale {
        state.jobs.drop_user(old);
        state.tutors.drop_user(old);
        state.games.drop_user(old);
    }

    // Checked next to the insert, on one connection (so nothing runs in
    // between): another sign-up may have taken the address, or the
    // invitation, while we hashed. The sign-up mode's word comes first, so
    // a refusal never depends on the address and never deletes anything.
    let e = email.clone();
    let (mode, invite) = (state.signup, form.invite.clone());
    let (outcome, deleted) = state
        .db
        .call(move |c| {
            let admitted = match signup::admit(c, mode, &invite, Some(&e), t)? {
                Ok(a) => a,
                Err(r) => return Ok((Signup::Refused(r), None)),
            };
            let mut deleted = None;
            if let Some(old) = stale {
                if crate::reset::replaceable(c, &e, t, days)? == Some(old) {
                    deleted = crate::privacy::delete_account(c, old)?.map(|d| (old, d));
                }
            }
            let tx = c.transaction()?;
            if let Some((id, stored)) = crate::reset::account_for(&tx, &e)? {
                return Ok((Signup::Taken(id, stored), deleted));
            }
            let id = auth::create_user(&tx, &e, &hash)?;
            if !signup::finish(&tx, admitted, id, t)? {
                return Ok((Signup::Refused(Refusal::Used), deleted));
            }
            crate::terms::accept(&tx, id, crate::terms::version())?;
            crate::privacy::record_age_confirmed(&tx, id, true)?;
            profile::set_ai_off(&tx, id, no_ai)?;
            if !lang.is_empty() {
                profile::set_lang(&tx, id, &lang)?;
            }
            let session = if can_mail { None } else { Some(auth::create_session(&tx, id)?) };
            tx.commit()?;
            Ok((Signup::New(id, session), deleted))
        })
        .await?;
    if let Some((old, d)) = deleted {
        for c in &d.courses {
            state.courses.invalidate(c);
        }
        for th in &d.themes {
            state.themes.forget(th);
            state.courses.forget_theme(th);
        }
        tracing::info!("unconfirmed user {old} was replaced by a new registration of its address");
    }
    match outcome {
        Signup::New(user_id, session) => {
            tracing::info!("a new user registered");
            crate::verify::send_or_log(&state, &headers, user_id, &email).await;
            if let Some(token) = session {
                return Ok(([auth::set_session_cookie(&token, state.secure_cookies)], Redirect::to(&crate::url(&after_sign_in(share.as_ref())))).into_response());
            }
        }
        Signup::Taken(_, _) if !can_mail => return Ok(fail(crate::t("auth.register.email_taken"))),
        Signup::Taken(user_id, stored) => {
            tracing::info!("a registration named the address of user {user_id}: nothing created, the address is told");
            if state.limits.hit(security::REGISTER_TAKEN_PER_ADDRESS, &stored, t) {
                if let Err(e) = crate::reset::send_taken(&state, &headers, user_id, &stored).await {
                    tracing::warn!("could not queue the already-registered e-mail for user {user_id}: {e:?}");
                }
            }
        }
        Signup::Refused(r) => return Ok(signup_refused(r, StatusCode::FORBIDDEN)),
    }
    Ok(Page(RegisterSentTemplate { user: None, email, share }).into_response())
}

/// What `register` did on the database thread.
enum Signup {
    /// The account was made: send the confirmation mail. Without mail it
    /// is signed in at once, with this session token.
    New(i64, Option<String>),
    /// The address (as stored) already has this account: nothing was made,
    /// and the page says the same as for a new one (#379).
    Taken(i64, String),
    /// The sign-up mode said no (`signup.rs`).
    Refused(Refusal),
}

#[derive(Template)]
#[template(path = "register_sent.html")]
pub struct RegisterSentTemplate {
    pub user: Option<CurrentUser>,
    /// The address typed; the page is the same whether it had an account.
    pub email: String,
    /// A course's share link the sign-up came from: the Sign in link keeps it.
    pub share: Option<ShareNotice>,
}

impl RegisterSentTemplate {
    /// The address in bold, escaped, for the page's `_html` sentence.
    pub fn email_html(&self) -> String {
        format!("<strong data-testid=\"register-sent-email\">{}</strong>", crate::error::html_escape(&self.email))
    }
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
/// sign-in on, send the code and ask for it at `/login/code`. `cookies` go
/// out either way. `share`: the course's share link to land on instead of
/// home; a sign-in that goes through its second step carries it in the
/// share cookie (#1016).
async fn sign_in(state: &AppState, user_id: i64, mut cookies: Vec<(axum::http::HeaderName, HeaderValue)>, share: Option<&ShareNotice>) -> Result<Response, AppError> {
    // The account's second step goes by a channel this server lost (a
    // self-hoster removed the provider): no code could reach it, so the
    // sign-in is refused, naming the setting (#633).
    let method = state.db.call(move |c| two_factor::settings(c, user_id)).await?.method;
    if let Some(m) = method.filter(|m| !two_factor::channel_on(*m, state.caps())) {
        tracing::warn!("user {user_id} cannot sign in: their two-factor method ({}) needs a provider this server lacks", m.as_str());
        let page = Page(LoginTemplate {
            user: None,
            reset: state.caps().mail,
            error: Some(two_factor::channel_off_text(m)),
            email: String::new(),
            dev: state.dev,
            notice: None,
            share: None,
        });
        return Ok((StatusCode::FORBIDDEN, AppendHeaders(cookies), page).into_response());
    }
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
    tracing::info!("user {user_id} asked for a {} code", ch.method.as_str());
    let sent = match ch.send {
        Some((to, code)) => send_code(state, user_id, ch.method, &to, &code).await,
        None => true,
    };
    cookies.push(twofa_cookie(&ch.token, two_factor::CODE_TTL_SECS, state.secure_cookies));
    if let Some(s) = share {
        cookies.push(named_cookie(SHARE_COOKIE, &s.token, two_factor::CODE_TTL_SECS, state.secure_cookies));
    }
    let to = if sent { "/login/code" } else { "/login/code?unsent=1" };
    Ok((AppendHeaders(cookies), Redirect::to(&crate::url(to))).into_response())
}

/// E-mail a sign-in code. `false` when it could not be queued (a bad
/// address); the page then says so.
async fn send_code(state: &AppState, user_id: i64, method: two_factor::Method, to: &str, code: &str) -> bool {
    let r = match method {
        two_factor::Method::Email => match two_factor::login_mail(to, code, user_id) {
            Ok(mail) => state.mail.queue(&state.db, mail).await.map(|_| ()).map_err(|e| format!("{e:?}")),
            Err(e) => Err(e),
        },
        two_factor::Method::Totp => Ok(()),
    };
    match r {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!("could not send user {user_id} a sign-in code: {e}");
            false
        }
    }
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

// ---------- e-mail verification ----------

#[derive(Template)]
#[template(path = "verify_email.html")]
pub struct VerifyEmailTemplate {
    pub user: Option<CurrentUser>,
    /// The link confirmed an address just now.
    pub confirmed: bool,
}

#[derive(Deserialize)]
pub struct VerifyParams {
    #[serde(default)]
    pub token: String,
}

/// GET /verify-email?token=…: the link in the confirmation e-mail. Works
/// signed in or not (it may be opened in another browser) and signs nobody in.
pub async fn verify_email(
    State(state): State<Arc<AppState>>,
    MaybeUser(mut user): MaybeUser,
    Query(q): Query<VerifyParams>,
) -> Result<Response, AppError> {
    let token = q.token.trim().to_string();
    let outcome = state.db.call(move |c| crate::verify::confirm(c, &token, now())).await?;
    let confirmed = match outcome {
        crate::verify::Outcome::Confirmed { user_id } => {
            tracing::info!("user {user_id} confirmed their e-mail address");
            if let Some(u) = user.as_mut().filter(|u| u.id == user_id) {
                u.email_verified = true;
            }
            true
        }
        crate::verify::Outcome::Invalid => false,
    };
    let page = Page(VerifyEmailTemplate { user, confirmed }).into_response();
    Ok(([(axum::http::header::CACHE_CONTROL, "no-store")], page).into_response())
}
