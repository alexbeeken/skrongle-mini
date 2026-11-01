//! One error type for the whole app, convertible into an HTTP response.

use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use std::fmt;

#[derive(Debug)]
pub enum AppError {
    NotFound,
    BadRequest(String),
    /// Bad request on a JSON API: answered as `{"error": "..."}`.
    ApiBadRequest(String),
    /// Browser request without a valid session: redirect to the login page.
    RedirectToLogin,
    /// Signed in, but no (valid) course chosen yet: redirect to the course picker.
    RedirectToCourses,
    /// API request without a valid session.
    Unauthorized,
    /// API request with no (valid) course any more, e.g. its share was
    /// removed mid-session (#1083): a 410 the page scripts answer by going
    /// to the course list, the way they answer a 401 with the login page.
    ApiNoCourse,
    /// A paid feature asked for by a free learner (`billing::RequirePremium`):
    /// redirect to the profile's subscription section.
    PaymentRequired,
    /// The same on an API route: a 402 JSON body.
    ApiPaymentRequired,
    /// Signed in, but the current Terms of Service are not accepted yet:
    /// redirect to the gate, which returns to this app path afterwards.
    TermsRequired(String),
    /// The same on an API route: a 403 JSON body.
    ApiTermsRequired,
    /// Signed in to an account scheduled for deletion (#970): redirect to
    /// `/account/deletion`, where the learner may cancel it or sign out.
    DeletionScheduled,
    /// The same on an API route: a 403 JSON body.
    ApiDeletionScheduled,
    /// Signed in with a temporary password the owner set (#635): redirect
    /// to `/password/change`, which returns to this app path afterwards.
    PasswordChangeRequired(String),
    /// The same on an API route: a 403 JSON body.
    ApiPasswordChangeRequired,
    /// An AI feature asked for by an account with "Hide all AI
    /// features" on (`CurrentUser::require_ai`): redirect to the profile
    /// section that turns them back on.
    AiOff,
    /// The same on an API route: a 403 JSON body.
    ApiAiOff,
    /// Over a rate limit on a JSON API (`security::RateLimits`): a 429 with
    /// `{"error": "..."}`.
    ApiTooManyRequests(String),
    Db(rusqlite::Error),
    Internal(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::NotFound => f.write_str("not found"),
            AppError::BadRequest(m) => write!(f, "bad request: {m}"),
            AppError::ApiBadRequest(m) => write!(f, "bad request: {m}"),
            AppError::RedirectToLogin => f.write_str("login required"),
            AppError::RedirectToCourses => f.write_str("no course chosen"),
            AppError::Unauthorized => f.write_str("unauthorized"),
            AppError::ApiNoCourse => f.write_str("no course chosen"),
            AppError::PaymentRequired | AppError::ApiPaymentRequired => f.write_str("subscription required"),
            AppError::TermsRequired(_) | AppError::ApiTermsRequired => f.write_str("terms not accepted"),
            AppError::PasswordChangeRequired(_) | AppError::ApiPasswordChangeRequired => f.write_str("a new password is needed"),
            AppError::DeletionScheduled | AppError::ApiDeletionScheduled => f.write_str("the account is scheduled for deletion"),
            AppError::AiOff | AppError::ApiAiOff => f.write_str("AI features are off"),
            AppError::ApiTooManyRequests(m) => write!(f, "too many requests: {m}"),
            AppError::Db(e) => write!(f, "database error: {e}"),
            AppError::Internal(m) => write!(f, "internal error: {m}"),
        }
    }
}

impl std::error::Error for AppError {}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        AppError::Db(e)
    }
}

impl From<askama::Error> for AppError {
    fn from(e: askama::Error) -> Self {
        AppError::Internal(format!("template error: {e}"))
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            AppError::NotFound => error_page(StatusCode::NOT_FOUND, &crate::t("error.not_found")),
            AppError::BadRequest(m) => error_page(StatusCode::BAD_REQUEST, &m),
            AppError::ApiBadRequest(m) => (
                StatusCode::BAD_REQUEST,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                serde_json::json!({ "error": m }).to_string(),
            )
                .into_response(),
            AppError::RedirectToLogin => Redirect::to(&crate::url("/login")).into_response(),
            AppError::RedirectToCourses => Redirect::to(&crate::url("/courses")).into_response(),
            AppError::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                r#"{"error":"unauthorized"}"#,
            )
                .into_response(),
            AppError::ApiNoCourse => (
                StatusCode::GONE,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                r#"{"error":"choose a course first"}"#,
            )
                .into_response(),
            AppError::PaymentRequired => Redirect::to(&crate::url("/profile?upgrade=1#subscription")).into_response(),
            AppError::ApiPaymentRequired => (
                StatusCode::PAYMENT_REQUIRED,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                r#"{"error":"subscription required"}"#,
            )
                .into_response(),
            AppError::TermsRequired(next) => {
                Redirect::to(&format!("{}?next={}", crate::url("/terms/accept"), crate::urlenc::urlencode(&next))).into_response()
            }
            AppError::ApiTermsRequired => (
                StatusCode::FORBIDDEN,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                r#"{"error":"accept the terms of service first"}"#,
            )
                .into_response(),
            AppError::DeletionScheduled => Redirect::to(&crate::url("/account/deletion")).into_response(),
            AppError::ApiDeletionScheduled => (
                StatusCode::FORBIDDEN,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                r#"{"error":"the account is scheduled for deletion"}"#,
            )
                .into_response(),
            AppError::PasswordChangeRequired(next) => {
                Redirect::to(&format!("{}?next={}", crate::url("/password/change"), crate::urlenc::urlencode(&next))).into_response()
            }
            AppError::ApiPasswordChangeRequired => (
                StatusCode::FORBIDDEN,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                r#"{"error":"choose a new password first"}"#,
            )
                .into_response(),
            AppError::AiOff => Redirect::to(&crate::url("/profile?ai=off#consent")).into_response(),
            AppError::ApiAiOff => (
                StatusCode::FORBIDDEN,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                r#"{"error":"AI features are off in your profile"}"#,
            )
                .into_response(),
            AppError::ApiTooManyRequests(m) => (
                StatusCode::TOO_MANY_REQUESTS,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                serde_json::json!({ "error": m }).to_string(),
            )
                .into_response(),
            AppError::Db(e) => {
                tracing::error!("database error: {e}");
                error_page(StatusCode::INTERNAL_SERVER_ERROR, &crate::t("error.database"))
            }
            AppError::Internal(m) => {
                tracing::error!("internal error: {m}");
                error_page(StatusCode::INTERNAL_SERVER_ERROR, &crate::t("error.internal"))
            }
        }
    }
}

/// What an error page says, left on its response for `in_layout`, which
/// renders it inside the site's layout (top bar, account, course). The plain
/// page below is what is sent when that cannot be done.
#[derive(Clone, Debug)]
pub struct ErrorPage(pub String);

fn error_page(status: StatusCode, msg: &str) -> Response {
    let body = format!(
        "<!doctype html><html lang=\"{lang}\"><head><meta charset=\"utf-8\"><title>{code}</title>\
         <link rel=\"stylesheet\" href=\"{css}\"></head>\
         <body><main class=\"container narrow\"><h1>{code}</h1><p>{msg}</p>\
         <p><a href=\"{root}\">{back}</a></p></main></body></html>",
        lang = crate::i18n::lang(),
        back = html_escape(&crate::t("error.back_to_start")),
        css = crate::routes::asset_url("style.css"),
        root = crate::url("/"),
        code = status.as_u16(),
        msg = html_escape(msg),
    );
    let mut res = (status, Html(body)).into_response();
    res.extensions_mut().insert(ErrorPage(msg.to_string()));
    res
}

#[derive(askama::Template)]
#[template(path = "error.html")]
struct ErrorTemplate {
    user: Option<crate::auth::CurrentUser>,
    code: u16,
    msg: String,
}

/// Middleware: an error page (404 and the rest of `error_page`) is shown in
/// the normal layout, with the signed-in learner's top bar and course and
/// links back into the app, keeping its status code (#397). Only error
/// responses pay for the session lookup.
pub async fn in_layout(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::AppState>>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let headers = req.headers().clone();
    let api = req.uri().path().starts_with("/api/");
    let res = next.run(req).await;
    let res = if api { res } else { form_only_page(res) };
    let Some(ErrorPage(msg)) = res.extensions().get::<ErrorPage>().cloned() else {
        return res;
    };
    let user = crate::auth::user_from_headers(&state, &headers).await;
    let code = res.status();
    match askama::Template::render(&ErrorTemplate { user, code: code.as_u16(), msg }) {
        Ok(html) => {
            let (mut parts, _) = res.into_parts();
            parts.headers.remove(axum::http::header::CONTENT_LENGTH);
            Response::from_parts(parts, axum::body::Body::from(html))
        }
        Err(e) => {
            tracing::error!("error page template: {e}");
            res
        }
    }
}

/// A page opened by address whose route only takes a form (GET /logout,
/// /logout, ...) gets axum's bare 405 with an empty body (#1253). Give it
/// the error page instead, keeping the 405 status and the `Allow` header.
/// A 405 a handler wrote itself, with a body, is left alone; so are API
/// routes (the caller skips them), whose clients read the status only.
fn form_only_page(res: Response) -> Response {
    use axum::body::HttpBody;
    if res.status() != StatusCode::METHOD_NOT_ALLOWED || res.body().size_hint().exact() != Some(0) {
        return res;
    }
    let page = error_page(StatusCode::METHOD_NOT_ALLOWED, &crate::t("error.form_only"));
    let (mut parts, _) = res.into_parts();
    let (page_parts, body) = page.into_parts();
    for (name, value) in &page_parts.headers {
        parts.headers.insert(name.clone(), value.clone());
    }
    parts.extensions.extend(page_parts.extensions);
    Response::from_parts(parts, body)
}

pub fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}
