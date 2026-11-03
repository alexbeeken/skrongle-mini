//! The second sign-in step's pages: `/login/code` and its cancel and dev
//! device (`two_factor.rs`). [`super::sign_in`] starts it.

use super::*;

#[derive(Template)]
#[template(path = "login_code.html")]
pub struct LoginCodeTemplate {
    pub user: Option<CurrentUser>,
    pub error: Option<String>,
    /// Dev mode: the page floats the dev device for this sign-in
    /// (`/login/code/device`) at the bottom, so the code is a click away.
    pub dev: bool,
}

#[derive(Deserialize)]
pub struct LoginCodeQuery {
    /// `/login/code/device`: `json` for the rows.
    pub format: Option<String>,
}

fn code_page(state: &AppState, error: Option<String>) -> Response {
    let page = Page(LoginCodeTemplate { user: None, error, dev: state.dev }).into_response();
    ([(axum::http::header::CACHE_CONTROL, "no-store")], page).into_response()
}

/// Back to the login form, saying why the second step ended.
fn code_gone(state: &AppState, msg: &str) -> Response {
    let page = Page(LoginTemplate { user: None, error: Some(msg.to_string()), email: String::new(), dev: state.dev, share: None });
    ([twofa_cookie("", 0, state.secure_cookies)], page).into_response()
}

fn code_gone_msg() -> String {
    crate::t("auth.code.gone")
}

/// The app's CSP with `frame-ancestors 'self'`, for the dev device page framed by `/login/code`.
const FRAMED_CSP: &str = "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; \
img-src 'self' data: https:; font-src 'self'; connect-src 'self'; media-src 'self'; object-src 'none'; \
base-uri 'self'; frame-ancestors 'self'";

/// What the dev device shows for a sign-in: one authenticator account, with
/// the code picked out so the page can fill it in.
pub struct DeviceRow {
    pub key: String,
    /// The authenticator's issuer, or its account.
    pub title: String,
    pub body: String,
    pub code: String,
    /// When the code changes (unix seconds) and how long it lasts.
    pub expires: u64,
    pub remaining: u64,
    pub period: u64,
}

impl DeviceRow {
    /// The code in groups of three, as apps show it.
    pub fn grouped(&self) -> String {
        self.code.as_bytes().chunks(3).map(|c| String::from_utf8_lossy(c).into_owned()).collect::<Vec<_>>().join(" ")
    }
}

#[derive(Template)]
#[template(path = "login_code_device.html")]
pub struct LoginCodeDeviceTemplate {
    /// Whose code it is, in full (the dev device is the learner's own).
    pub to: String,
    pub now: u64,
    pub rows: Vec<DeviceRow>,
}

/// GET /login/code/device (dev mode, local clients): the dev device the
/// pending sign-in's code comes from, as a bare page for the frame at the
/// bottom of `/login/code`: the dev authenticator's entry for the account's
/// address, with its code and a *Use* button. The `twofa` cookie names the
/// sign-in, so a learner who proved the password sees their own device only.
/// Polled by the frame (`static/devbox.js`); `?format=json` gives the rows.
pub async fn login_code_device(State(state): State<Arc<AppState>>, headers: HeaderMap, Query(q): Query<LoginCodeQuery>) -> Result<Response, AppError> {
    let Some(token) = twofa_token(&headers) else { return Err(AppError::NotFound) };
    let Some(p) = state.db.call(move |c| two_factor::pending(c, &token, now())).await? else { return Err(AppError::NotFound) };
    let now = crate::db::now();
    let unix = now as u64;
    let rows: Vec<DeviceRow> = state
        .dev_authenticator
        .list()
        .into_iter()
        .filter(|a| a.key.account.eq_ignore_ascii_case(&p.email))
        .map(|a| DeviceRow { key: format!("totp{}", a.id), title: if a.key.issuer.is_empty() { a.key.account.clone() } else { a.key.issuer.clone() }, body: a.key.account.clone(), code: a.key.code_at(unix), expires: unix + a.key.remaining(unix), remaining: a.key.remaining(unix), period: a.key.period })
        .collect();
    let to = p.email.clone();
    if q.format.as_deref() == Some("json") {
        let rows: Vec<_> = rows.iter().map(|r| serde_json::json!({ "key": r.key, "title": r.title, "body": r.body, "code": r.code, "expires": r.expires })).collect();
        return Ok(axum::Json(serde_json::json!({ "to": to, "now": unix, "rows": rows })).into_response());
    }
    let page = Page(LoginCodeDeviceTemplate { to, now: unix, rows }).into_response();
    // The one page the app lets itself frame: the headers middleware keeps
    // a CSP a handler set (security.rs), and this route exists in dev mode only.
    Ok((
        [
            (axum::http::header::CACHE_CONTROL, "no-store"),
            (axum::http::header::CONTENT_SECURITY_POLICY, FRAMED_CSP),
            (axum::http::header::X_FRAME_OPTIONS, "SAMEORIGIN"),
        ],
        page,
    )
        .into_response())
}

/// GET /login/code: the second step's form.
pub async fn login_code_form(State(state): State<Arc<AppState>>, MaybeUser(user): MaybeUser, headers: HeaderMap) -> Result<Response, AppError> {
    if user.is_some() {
        return Ok(Redirect::to(&crate::url("/")).into_response());
    }
    let Some(token) = twofa_token(&headers) else { return Ok(code_gone(&state, &code_gone_msg())) };
    let Some(_) = state.db.call(move |c| two_factor::pending(c, &token, now())).await? else { return Ok(code_gone(&state, &code_gone_msg())) };
    Ok(code_page(&state, None))
}

#[derive(Deserialize)]
pub struct CodeForm {
    #[serde(default)]
    pub code: String,
}

/// POST /login/code (code): the right code signs in; a wrong one says how
/// many tries are left; the last wrong one ends the sign-in.
pub async fn login_code(State(state): State<Arc<AppState>>, ClientIp(ip): ClientIp, headers: HeaderMap, Form(form): Form<CodeForm>) -> Result<Response, AppError> {
    let t = now();
    let Some(token) = twofa_token(&headers) else { return Ok(code_gone(&state, &code_gone_msg())) };
    if !state.limits.hit(security::LOGIN_PER_IP, &ip, t) {
        tracing::warn!("sign-in rate limit hit for a code from {ip}");
        return Ok((StatusCode::TOO_MANY_REQUESTS, code_gone(&state, &crate::t("auth.login.too_many_attempts"))).into_response());
    }
    // Wrong codes count against the account like wrong passwords, and a
    // blocked account gets no more tries here either (#359).
    let tk = token.clone();
    let Some(email) = state.db.call(move |c| two_factor::pending(c, &tk, t)).await?.map(|p| p.email) else { return Ok(code_gone(&state, &code_gone_msg())) };
    if state.limits.blocked(security::LOGIN_FAILURES_PER_USER, &email, t) {
        tracing::warn!("sign-in code refused: the account is blocked for now");
        return Ok((StatusCode::TOO_MANY_REQUESTS, code_gone(&state, &crate::t("auth.login.too_many_attempts"))).into_response());
    }
    let (tk, code, vault) = (token.clone(), form.code, state.vault.clone());
    let (checked, user_id) = state.db.call(move |c| two_factor::check_login(c, &vault, &tk, &code, t)).await?;
    match checked {
        two_factor::Checked::Ok => {
            let user_id = user_id.ok_or_else(|| AppError::Internal("a passed code without its user".into()))?;
            let session = state.db.call(move |c| auth::create_session(c, user_id)).await?;
            tracing::info!("user {user_id} logged in with a second step");
            let share = share_notice(&state, &hex_cookie(&headers, SHARE_COOKIE)).await?;
            let to = after_sign_in(share.as_ref());
            let cookies = [twofa_cookie("", 0, state.secure_cookies), auth::set_session_cookie(&session, state.secure_cookies), named_cookie(SHARE_COOKIE, "", 0, state.secure_cookies)];
            Ok((AppendHeaders(cookies), Redirect::to(&crate::url(&to))).into_response())
        }
        two_factor::Checked::Wrong(left) => {
            tracing::info!("user {} typed a wrong sign-in code", user_id.unwrap_or_default());
            let _ = state.limits.hit(security::LOGIN_FAILURES_PER_USER, &email, t);
            let Some(_) = state.db.call(move |c| two_factor::pending(c, &token, t)).await? else { return Ok(code_gone(&state, &code_gone_msg())) };
            let msg = crate::tn("auth.code.wrong", left);
            Ok((StatusCode::UNPROCESSABLE_ENTITY, code_page(&state, Some(msg))).into_response())
        }
        two_factor::Checked::Gone => {
            // With a user id this was the last wrong code; it counts too.
            if let Some(uid) = user_id {
                tracing::warn!("user {uid} used up the tries for a sign-in code");
                let _ = state.limits.hit(security::LOGIN_FAILURES_PER_USER, &email, t);
            }
            Ok(code_gone(&state, &crate::t("auth.code.too_many_wrong")))
        }
    }
}

/// POST /login/code/cancel: give up the sign-in, back to the login form
/// (still for the share link, if one brought the visitor).
pub async fn login_code_cancel(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Result<Response, AppError> {
    if let Some(token) = twofa_token(&headers) {
        state.db.call(move |c| two_factor::cancel(c, &token)).await?;
    }
    let share = share_notice(&state, &hex_cookie(&headers, SHARE_COOKIE)).await?;
    let cookies = [twofa_cookie("", 0, state.secure_cookies), named_cookie(SHARE_COOKIE, "", 0, state.secure_cookies)];
    Ok((AppendHeaders(cookies), Redirect::to(&other_page_url(&share, "/login"))).into_response())
}
