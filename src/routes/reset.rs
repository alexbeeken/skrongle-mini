//! GET/POST /reset: password reset (#379, `src/reset.rs`). Asking for a
//! link answers the same whether the address has an account; the link's
//! form sets a new password and ends every session.

use askama::Template;
use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Form;
use serde::Deserialize;
use std::sync::Arc;

use super::Page;
use crate::auth::{self, CurrentUser, MaybeUser};
use crate::db::now;
use crate::error::AppError;
use crate::security::{self, ClientIp};
use crate::AppState;

#[derive(Template)]
#[template(path = "reset.html")]
pub struct ResetTemplate {
    pub user: Option<CurrentUser>,
    /// `ask` (the address form), `sent`, `choose` (the new password form),
    /// `invalid` (the link does not work) or `done`.
    pub step: &'static str,
    /// The link's token, carried by the new password form.
    pub token: String,
    /// `ask`/`sent`: the address typed; `choose`: the account's.
    pub email: String,
    pub error: Option<String>,
}

impl ResetTemplate {
    /// The address in bold, escaped, for the `_html` sentences; the new
    /// password form's carries the `reset-account` testid.
    pub fn email_html(&self) -> String {
        let testid = if self.step == "choose" { " data-testid=\"reset-account\"" } else { "" };
        format!("<strong{testid}>{}</strong>", crate::error::html_escape(&self.email))
    }
}

#[derive(Deserialize)]
pub struct ResetQuery {
    #[serde(default)]
    pub token: String,
}

#[derive(Deserialize)]
pub struct ResetForm {
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub password: String,
}

/// The page, never cached, and with no referrer: its URL may hold the token.
fn page(status: StatusCode, t: ResetTemplate) -> Response {
    (status, [(header::CACHE_CONTROL, "no-store"), (header::REFERRER_POLICY, "no-referrer")], Page(t)).into_response()
}

fn step(user: Option<CurrentUser>, step: &'static str, token: String, email: String, error: Option<String>) -> ResetTemplate {
    ResetTemplate { user, step, token, email, error }
}

/// A reset needs mail: without a provider (Skrongle mini, #633) the page
/// does not exist, and the owner sets a temporary password instead
/// (`superuser.rs`, #635). The login page's link follows `LoginTemplate::reset`.
fn offered(state: &AppState) -> Result<(), AppError> {
    if state.caps().mail {
        Ok(())
    } else {
        Err(AppError::NotFound)
    }
}

/// GET /reset: ask for the address; `?token=…` (the link in the message)
/// shows the new password form, or says the link does not work.
pub async fn form(State(state): State<Arc<AppState>>, MaybeUser(user): MaybeUser, Query(q): Query<ResetQuery>) -> Result<Response, AppError> {
    offered(&state)?;
    let token = q.token.trim().to_string();
    if token.is_empty() {
        return Ok(page(StatusCode::OK, step(user, "ask", String::new(), String::new(), None)));
    }
    let tk = token.clone();
    Ok(match state.db.call(move |c| crate::reset::valid(c, &tk, now())).await? {
        Some(email) => page(StatusCode::OK, step(user, "choose", token, email, None)),
        None => page(StatusCode::OK, step(user, "invalid", String::new(), String::new(), None)),
    })
}

/// POST /reset: with `token`, set the new `password`; without, send a link
/// to `email` if it has an account (the page says the same either way).
pub async fn submit(
    State(state): State<Arc<AppState>>,
    MaybeUser(user): MaybeUser,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Form(form): Form<ResetForm>,
) -> Result<Response, AppError> {
    offered(&state)?;
    let t = now();
    let token = form.token.trim().to_string();
    if !token.is_empty() {
        return set_password(&state, user, &ip, token, form.password).await;
    }

    let typed = form.email.trim().to_string();
    if !state.limits.hit(security::RESET_PER_IP, &ip, t) {
        tracing::warn!("password reset rate limit hit from {ip}");
        let msg = crate::t("auth.reset.too_many_requests");
        return Ok(page(StatusCode::TOO_MANY_REQUESTS, step(user, "ask", String::new(), typed, Some(msg))));
    }
    let email = match crate::profile::validate_email(&typed) {
        Ok(e) => e,
        Err(e) => return Ok(page(StatusCode::OK, step(user, "ask", String::new(), typed, Some(e)))),
    };
    let e = email.clone();
    if let Some((user_id, stored)) = state.db.call(move |c| crate::reset::account_for(c, &e)).await? {
        if state.limits.hit(security::RESET_PER_ADDRESS, &stored, t) {
            tracing::info!("user {user_id} was sent a password reset link");
            if let Err(e) = crate::reset::send(&state, &headers, user_id, &stored).await {
                tracing::warn!("could not queue the password reset e-mail for user {user_id}: {e:?}");
            }
        } else {
            tracing::warn!("password reset limit hit for user {user_id}: nothing sent");
        }
    }
    Ok(page(StatusCode::OK, step(user, "sent", String::new(), email, None)))
}

async fn set_password(state: &AppState, user: Option<CurrentUser>, ip: &str, token: String, password: String) -> Result<Response, AppError> {
    let t = now();
    let tk = token.clone();
    let Some(email) = state.db.call(move |c| crate::reset::valid(c, &tk, t)).await? else {
        return Ok(page(StatusCode::OK, step(user, "invalid", String::new(), String::new(), None)));
    };
    if !state.limits.hit(security::RESET_SET_PER_IP, ip, t) {
        tracing::warn!("password reset rate limit hit from {ip}");
        let msg = crate::t("auth.reset.too_many_tries");
        return Ok(page(StatusCode::TOO_MANY_REQUESTS, step(user, "choose", token, email, Some(msg))));
    }
    if let Err(e) = auth::validate_password(&password) {
        return Ok(page(StatusCode::OK, step(user, "choose", token, email, Some(e))));
    }
    let hash = auth::hash_password_async(password).await?;
    let done = state.db.call(move |c| crate::reset::use_link(c, &token, &hash, now())).await?;
    let Some(user_id) = done else {
        return Ok(page(StatusCode::OK, step(user, "invalid", String::new(), String::new(), None)));
    };
    tracing::info!("user {user_id} set a new password with a reset link; every session ended");
    // This browser's session (if it had one) ended with the others.
    let res = page(StatusCode::OK, step(None, "done", String::new(), String::new(), None));
    Ok(([auth::clear_session_cookie()], res).into_response())
}

#[cfg(test)]
mod tests {
    /// The whole flow over HTTP: asking says the same for any address and
    /// mails only an account's; the link sets the password once, ends every
    /// session, and the new password signs in.
    #[test]
    fn password_reset_over_http() {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let state = crate::routes::test_state(false);
        let hash = crate::auth::hash_password("old-password").unwrap();
        let ann = state.db.with(|c| crate::auth::create_user(c, "ann@example.org", &hash)).unwrap();
        let session = state.db.with(|c| crate::auth::create_session(c, ann)).unwrap();
        let app = crate::routes::router(state.clone());
        let base = rt.block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move { axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap() });
            format!("http://{addr}")
        });
        let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
        let get = |path: &str| {
            let mut r = agent.get(&format!("{base}{path}")).call().unwrap();
            (r.status().as_u16(), r.body_mut().read_to_string().unwrap())
        };
        let post = |path: &str, form: &[(&str, &str)]| {
            let mut r = agent.post(&format!("{base}{path}")).send_form(form.iter().copied()).unwrap();
            (r.status().as_u16(), r.body_mut().read_to_string().unwrap())
        };
        let mails = || -> Vec<String> {
            state
                .db
                .with(|c| {
                    let mut stmt = c.prepare("SELECT body_text FROM mail_outbox WHERE purpose = 'password-reset' ORDER BY id")?;
                    let v = stmt.query_map([], |r| r.get(0))?.collect::<Result<Vec<String>, _>>();
                    v
                })
                .unwrap()
        };
        let token_in = |body: &str| body.split(&format!("{base}/reset?token=")).nth(1).unwrap().split_whitespace().next().unwrap().to_string();

        // The login page links here; the page asks for the address.
        assert!(get("/login").1.contains(r#"data-testid="login-reset-link""#));
        let (status, html) = get("/reset");
        assert_eq!(status, 200);
        assert!(html.contains(r#"data-testid="reset-ask-form""#));

        // An unknown address and a real one get the same page; only the real one gets mail.
        let (s1, unknown) = post("/reset", &[("email", "nobody@example.org")]);
        let (s2, known) = post("/reset", &[("email", " ANN@example.org ")]);
        assert_eq!((s1, s2), (200, 200));
        assert_eq!(unknown.replace("nobody@example.org", "X"), known.replace("ann@example.org", "X"), "the same page either way");
        assert!(known.contains(r#"data-testid="reset-sent""#));
        let sent = mails();
        assert_eq!(sent.len(), 1);
        let token = token_in(&sent[0]);

        // The link: the form, for the account's address; a bad one is refused.
        let (_, html) = get(&format!("/reset?token={token}"));
        assert!(html.contains(r#"data-testid="reset-choose-form""#) && html.contains("ann@example.org"), "{html}");
        assert!(get("/reset?token=0123").1.contains(r#"data-testid="reset-invalid""#));
        assert!(post("/reset", &[("token", &token), ("password", "short")]).1.contains(r#"data-testid="reset-error""#));
        let (status, html) = post("/reset", &[("token", &token), ("password", "brand-new-password")]);
        assert_eq!(status, 200);
        assert!(html.contains(r#"data-testid="reset-done""#), "{html}");
        assert!(post("/reset", &[("token", &token), ("password", "another-password")]).1.contains(r#"data-testid="reset-invalid""#), "single use");

        // Every session ended; the old password fails and the new one signs in.
        assert!(state.db.with(|c| crate::auth::user_for_session(c, &session)).unwrap().is_none());
        assert_eq!(post("/login", &[("email", "ann@example.org"), ("password", "old-password")]).0, 200);
        assert_eq!(post("/login", &[("email", "ann@example.org"), ("password", "brand-new-password")]).0, 303);

        // Asking: limited per address, silently (the page is the same).
        for _ in 1..crate::security::RESET_PER_ADDRESS.max {
            post("/reset", &[("email", "ann@example.org")]);
        }
        assert_eq!(mails().len(), crate::security::RESET_PER_ADDRESS.max as usize);
        assert!(post("/reset", &[("email", "ann@example.org")]).1.contains(r#"data-testid="reset-sent""#));
        assert_eq!(mails().len(), crate::security::RESET_PER_ADDRESS.max as usize, "nothing more sent");
        // And per network.
        for _ in (2 + crate::security::RESET_PER_ADDRESS.max)..crate::security::RESET_PER_IP.max {
            post("/reset", &[("email", "nobody@example.org")]);
        }
        assert_eq!(post("/reset", &[("email", "nobody@example.org")]).0, 429);
    }

    /// Registering an address that has an account looks like a new
    /// registration: the same page, no cookie, nothing created; the address
    /// is told instead. An unconfirmed account older than the setting is
    /// replaced; a younger or confirmed one is not.
    #[test]
    fn registering_a_taken_address_over_http() {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let state = crate::routes::test_state(false);
        let app = crate::routes::router(state.clone());
        let base = rt.block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move { axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await.unwrap() });
            format!("http://{addr}")
        });
        let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
        let register = |email: &str, pw: &str| {
            let mut r = agent
                .post(&format!("{base}/register"))
                .send_form([("email", email), ("password", pw), ("age", "on")])
                .unwrap();
            let cookie = r.headers().get("set-cookie").is_some();
            (r.status().as_u16(), cookie, r.body_mut().read_to_string().unwrap())
        };
        let count = |sql: &'static str| state.db.with(move |c| c.query_row(sql, [], |r| r.get::<_, i64>(0))).unwrap();
        let ids = || count("SELECT COALESCE(SUM(id), 0) FROM users WHERE email = 'ann@example.org'");

        let (status, cookie, new) = register("ann@example.org", "first-password");
        assert_eq!((status, cookie), (200, false), "a new account is not signed in either");
        assert!(new.contains(r#"data-testid="register-sent""#));
        let first = ids();
        assert_eq!(count("SELECT COUNT(*) FROM mail_outbox WHERE purpose = 'email-verify'"), 1);

        // Again, any case: the same answer, nothing created, a "you have an account" message.
        let (status, cookie, again) = register("ANN@example.org", "second-password");
        assert_eq!((status, cookie), (200, false));
        assert_eq!(again, new, "no on-page difference");
        assert_eq!(count("SELECT COUNT(*) FROM users"), 1);
        assert_eq!(ids(), first);
        assert_eq!(count("SELECT COUNT(*) FROM mail_outbox WHERE purpose = 'register-taken' AND to_addr = 'ann@example.org'"), 1);
        let (_, hash) = state.db.with(|c| crate::auth::find_user_credentials(c, "ann@example.org")).unwrap().unwrap();
        assert!(crate::auth::verify_password("first-password", &hash), "the password is unchanged");

        // A week old and never confirmed: the address is given to the new registration.
        state.db.with(|c| c.execute("UPDATE users SET created_at = created_at - 7 * 86400, first_name = 'Old'", [])).unwrap();
        let (status, _, page) = register("ann@example.org", "second-password");
        assert_eq!((status, page), (200, new.clone()));
        assert_eq!(count("SELECT COUNT(*) FROM users"), 1);
        assert_eq!(count("SELECT COUNT(*) FROM users WHERE first_name = 'Old'"), 0, "the old account was deleted, a new one made");
        let (_, hash) = state.db.with(|c| crate::auth::find_user_credentials(c, "ann@example.org")).unwrap().unwrap();
        assert!(crate::auth::verify_password("second-password", &hash));

        // Confirmed, it stays however old it is.
        state.db.with(|c| c.execute("UPDATE users SET created_at = 1, email_verified_at = 2, first_name = 'Kept'", [])).unwrap();
        register("ann@example.org", "third-password");
        assert_eq!(count("SELECT COUNT(*) FROM users WHERE first_name = 'Kept'"), 1);
    }
}
