//! The second sign-in step's pages: `/login/code` and its resend, cancel,
//! status and dev device (`two_factor.rs`). [`super::sign_in`] starts it.

use super::*;

#[derive(Template)]
#[template(path = "login_code.html")]
pub struct LoginCodeTemplate {
    pub user: Option<CurrentUser>,
    /// `sms`, `email` or `totp`.
    pub method: &'static str,
    /// Where the code went, partly hidden.
    pub sent_to: String,
    /// A texted code went by e-mail: the number opted out of texts (#969).
    pub fallback: bool,
    pub error: Option<String>,
    pub notice: Option<String>,
    /// Dev mode: the page floats the dev device for this sign-in
    /// (`/login/code/device`) at the bottom, so the code is a click away.
    pub dev: bool,
}

impl LoginCodeTemplate {
    /// Where the newest code went: `method`, or `email` for a fallback.
    pub fn channel(&self) -> &str {
        if self.fallback { "email" } else { self.method }
    }
}

#[derive(Deserialize)]
pub struct LoginCodeQuery {
    /// `1`: a new code was sent.
    pub sent: Option<String>,
    /// `1`: the code could not be sent.
    pub unsent: Option<String>,
    /// `/login/code/device`: `json` for the rows.
    pub format: Option<String>,
}

fn code_page(state: &AppState, p: &two_factor::Pending, error: Option<String>, notice: Option<String>) -> Response {
    let fallback = p.method == two_factor::Method::Sms && p.emailed;
    let page = Page(LoginCodeTemplate { user: None, method: p.method.as_str(), sent_to: p.sent_to(), fallback, error, notice, dev: state.dev }).into_response();
    ([(axum::http::header::CACHE_CONTROL, "no-store")], page).into_response()
}

/// Back to the login form, saying why the second step ended.
fn code_gone(state: &AppState, msg: &str) -> Response {
    let page = Page(LoginTemplate { user: None, reset: state.caps().mail, error: Some(msg.to_string()), email: String::new(), google: state.google.is_some(), dev: state.dev, google_mock: state.google_mock(), notice: None, share: None });
    ([twofa_cookie("", 0, state.secure_cookies)], page).into_response()
}

fn code_gone_msg() -> String {
    crate::t("auth.code.gone")
}

/// The app's CSP with `frame-ancestors 'self'`, for the dev device page framed by `/login/code`.
const FRAMED_CSP: &str = "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; \
img-src 'self' data: https:; font-src 'self'; connect-src 'self'; media-src 'self'; object-src 'none'; \
base-uri 'self'; frame-ancestors 'self'";

/// What the dev device shows for a sign-in: one text, mail or
/// authenticator account, with the code picked out so the page can fill it in.
pub struct DeviceRow {
    pub key: String,
    /// The text, the mail's subject, or the authenticator's account.
    pub title: String,
    pub body: String,
    pub code: String,
    pub ago: String,
    /// The authenticator's: when the code changes (unix seconds) and how
    /// long it lasts; 0 otherwise.
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

/// The first run of six digits in a message: the code it carries.
fn code_in(text: &str) -> String {
    text.split(|c: char| !c.is_ascii_digit()).find(|run| run.len() == 6).unwrap_or_default().to_string()
}

#[derive(Template)]
#[template(path = "login_code_device.html")]
pub struct LoginCodeDeviceTemplate {
    /// `sms`, `email` or `totp`.
    pub method: &'static str,
    /// Where the code goes, in full (the dev device is the learner's own).
    pub to: String,
    pub now: u64,
    pub rows: Vec<DeviceRow>,
    /// The device is off (no dev phone, no dev inbox).
    pub off: bool,
}

/// GET /login/code/device (dev mode, local clients): the dev device the
/// pending sign-in's code went to, as a bare page for the frame at the
/// bottom of `/login/code`: the texts the dev phone got for that number,
/// the mail the dev inbox got for that address, or the dev authenticator's
/// entry for that address, each with its code and a *Use* button. The
/// `twofa` cookie names the sign-in, so a learner who proved the password
/// sees their own device only; the dev mailbox itself stays for developers.
/// Polled by the frame (`static/devbox.js`); `?format=json` gives the rows.
pub async fn login_code_device(State(state): State<Arc<AppState>>, headers: HeaderMap, Query(q): Query<LoginCodeQuery>) -> Result<Response, AppError> {
    let Some(token) = twofa_token(&headers) else { return Err(AppError::NotFound) };
    let Some(p) = state.db.call(move |c| two_factor::pending(c, &token, now())).await? else { return Err(AppError::NotFound) };
    let now = crate::db::now();
    let unix = now as u64;
    // A texted code that went by e-mail is in the dev inbox.
    let channel = if p.emailed { two_factor::Method::Email } else { p.method };
    let (to, rows, off): (String, Vec<DeviceRow>, bool) = match channel {
        two_factor::Method::Sms => {
            let rows = state
                .dev_phone
                .as_ref()
                .map(|ph| ph.list(Some(&p.phone)))
                .unwrap_or_default()
                .into_iter()
                .take(5)
                .map(|m| DeviceRow { key: format!("sms{}", m.id), title: m.to.clone(), body: m.text.clone(), code: code_in(&m.text), ago: crate::routes::time_ago(m.received_at, now), expires: 0, remaining: 0, period: 0 })
                .collect();
            (p.phone.clone(), rows, state.dev_phone.is_none())
        }
        two_factor::Method::Email => {
            let rows = state
                .dev_inbox
                .as_ref()
                .map(|i| i.list(Some(&p.email)))
                .unwrap_or_default()
                .into_iter()
                .take(5)
                .map(|m| DeviceRow { key: format!("mail{}", m.id), title: m.subject.clone(), body: m.text.lines().find(|l| !l.trim().is_empty()).unwrap_or_default().to_string(), code: code_in(&m.text), ago: crate::routes::time_ago(m.received_at, now), expires: 0, remaining: 0, period: 0 })
                .collect();
            (p.email.clone(), rows, state.dev_inbox.is_none())
        }
        two_factor::Method::Totp => {
            let rows = state
                .dev_authenticator
                .list()
                .into_iter()
                .filter(|a| a.key.account.eq_ignore_ascii_case(&p.email))
                .map(|a| DeviceRow { key: format!("totp{}", a.id), title: if a.key.issuer.is_empty() { a.key.account.clone() } else { a.key.issuer.clone() }, body: a.key.account.clone(), code: a.key.code_at(unix), ago: String::new(), expires: unix + a.key.remaining(unix), remaining: a.key.remaining(unix), period: a.key.period })
                .collect();
            (p.email.clone(), rows, false)
        }
    };
    if q.format.as_deref() == Some("json") {
        let rows: Vec<_> = rows.iter().map(|r| serde_json::json!({ "key": r.key, "title": r.title, "body": r.body, "code": r.code, "expires": r.expires })).collect();
        return Ok(axum::Json(serde_json::json!({ "method": channel.as_str(), "to": to, "now": unix, "rows": rows })).into_response());
    }
    let page = Page(LoginCodeDeviceTemplate { method: channel.as_str(), to, now: unix, rows, off }).into_response();
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
pub async fn login_code_form(State(state): State<Arc<AppState>>, MaybeUser(user): MaybeUser, headers: HeaderMap, Query(q): Query<LoginCodeQuery>) -> Result<Response, AppError> {
    if user.is_some() {
        return Ok(Redirect::to(&crate::url("/")).into_response());
    }
    let Some(token) = twofa_token(&headers) else { return Ok(code_gone(&state, &code_gone_msg())) };
    let Some(p) = state.db.call(move |c| two_factor::pending(c, &token, now())).await? else { return Ok(code_gone(&state, &code_gone_msg())) };
    let error = q.unsent.is_some().then(|| crate::t("auth.code.unsent"));
    let notice = q.sent.is_some().then(|| crate::t("auth.code.sent"));
    Ok(code_page(&state, &p, error, notice))
}

/// GET /login/code/status: `{"texts_stopped": true}` once the number a
/// sign-in code was texted to turns out to have opted out of texts (the
/// provider refused it after the page was shown). The page polls it
/// (`static/login_code.js`) and then asks for a new code, which goes by
/// e-mail (#969).
pub async fn login_code_status(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Result<Response, AppError> {
    let Some(token) = twofa_token(&headers) else { return Err(AppError::NotFound) };
    let Some(p) = state.db.call(move |c| two_factor::pending(c, &token, now())).await? else { return Err(AppError::NotFound) };
    let stopped = p.by_email() && !p.emailed;
    Ok(([(axum::http::header::CACHE_CONTROL, "no-store")], axum::Json(serde_json::json!({ "texts_stopped": stopped }))).into_response())
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
            let joined = finish_join(&state, user_id, state.google_joins.take(&token, t)).await?;
            let session = state.db.call(move |c| auth::create_session(c, user_id)).await?;
            tracing::info!("user {user_id} logged in with a second step");
            let share = share_notice(&state, &hex_cookie(&headers, SHARE_COOKIE)).await?;
            let to = if joined { JOINED_PROFILE.to_string() } else { after_sign_in(share.as_ref()) };
            let cookies = [twofa_cookie("", 0, state.secure_cookies), auth::set_session_cookie(&session, state.secure_cookies), named_cookie(SHARE_COOKIE, "", 0, state.secure_cookies)];
            Ok((AppendHeaders(cookies), Redirect::to(&crate::url(&to))).into_response())
        }
        two_factor::Checked::Wrong(left) => {
            tracing::info!("user {} typed a wrong sign-in code", user_id.unwrap_or_default());
            let _ = state.limits.hit(security::LOGIN_FAILURES_PER_USER, &email, t);
            let Some(p) = state.db.call(move |c| two_factor::pending(c, &token, t)).await? else { return Ok(code_gone(&state, &code_gone_msg())) };
            let msg = crate::tn("auth.code.wrong", left);
            Ok((StatusCode::UNPROCESSABLE_ENTITY, code_page(&state, &p, Some(msg), None)).into_response())
        }
        two_factor::Checked::Gone => {
            state.google_joins.take(&token, t);
            // With a user id this was the last wrong code; it counts too.
            if let Some(uid) = user_id {
                tracing::warn!("user {uid} used up the tries for a sign-in code");
                let _ = state.limits.hit(security::LOGIN_FAILURES_PER_USER, &email, t);
            }
            Ok(code_gone(&state, &crate::t("auth.code.too_many_wrong")))
        }
    }
}

/// POST /login/code/resend: a new texted or e-mailed code (the old one
/// stops working). Limited per account like "Send the e-mail again"; past
/// it nothing is sent but the page says the same.
pub async fn login_code_resend(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Result<Response, AppError> {
    let t = now();
    let Some(token) = twofa_token(&headers) else { return Ok(code_gone(&state, &code_gone_msg())) };
    let tk = token.clone();
    let Some(p) = state.db.call(move |c| two_factor::pending(c, &tk, t)).await? else { return Ok(code_gone(&state, &code_gone_msg())) };
    if state.limits.blocked(security::LOGIN_FAILURES_PER_USER, &p.email, t) {
        tracing::warn!("sign-in code resend refused: user {} is blocked for now", p.user_id);
        return Ok((StatusCode::TOO_MANY_REQUESTS, code_gone(&state, &crate::t("auth.login.too_many_attempts"))).into_response());
    }
    let mut sent = true;
    if state.limits.hit(security::TWO_FACTOR_SEND_PER_USER, &p.user_id.to_string(), t) {
        let by = if p.by_email() { two_factor::Method::Email } else { p.method };
        if let Some((to, code)) = state.db.call(move |c| two_factor::new_login_code(c, &token, t)).await? {
            sent = send_code(&state, p.user_id, by, &to, &code).await;
        }
    } else {
        tracing::warn!("sign-in code resend limit hit for user {}", p.user_id);
    }
    Ok(Redirect::to(&crate::url(if sent { "/login/code?sent=1" } else { "/login/code?unsent=1" })).into_response())
}

/// POST /login/code/cancel: give up the sign-in, back to the login form
/// (still for the share link, if one brought the visitor).
pub async fn login_code_cancel(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Result<Response, AppError> {
    if let Some(token) = twofa_token(&headers) {
        state.google_joins.take(&token, now());
        state.db.call(move |c| two_factor::cancel(c, &token)).await?;
    }
    let share = share_notice(&state, &hex_cookie(&headers, SHARE_COOKIE)).await?;
    let cookies = [twofa_cookie("", 0, state.secure_cookies), named_cookie(SHARE_COOKIE, "", 0, state.secure_cookies)];
    Ok((AppendHeaders(cookies), Redirect::to(&other_page_url(&share, "/login"))).into_response())
}
