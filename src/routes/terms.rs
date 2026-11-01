//! `/terms`: the Terms of Service page (public), its Markdown, and the gate
//! a signed-in learner passes when they have not accepted the current
//! version (`AppError::TermsRequired` sends them here). The notice of a
//! newer built-in version (`terms_notice.html`, #966) accepts through the
//! same form, or, for a minor change, through `/terms/dismiss`.

use askama::Template;
use axum::extract::{Query, State};
use axum::http::{header, HeaderValue};
use axum::response::{IntoResponse, Redirect, Response};
use axum::Form;
use serde::Deserialize;
use std::sync::Arc;

use super::Page;
use crate::auth::{CurrentUser, MaybeUser};
use crate::error::AppError;
use crate::terms;
use crate::AppState;

#[derive(Template)]
#[template(path = "terms.html")]
pub struct TermsTemplate {
    pub user: Option<CurrentUser>,
    pub html: String,
    pub version: &'static str,
}

/// GET /terms
pub async fn page(State(state): State<Arc<AppState>>, MaybeUser(user): MaybeUser) -> Page<TermsTemplate> {
    let html = super::docs::render_doc(&terms::markdown(state.stripe.is_some()));
    Page(TermsTemplate { user, html, version: terms::version() })
}

/// GET /terms.md: the same text as Markdown.
pub async fn markdown(State(state): State<Arc<AppState>>) -> Response {
    (
        [(header::CONTENT_TYPE, HeaderValue::from_static("text/markdown; charset=utf-8"))],
        terms::markdown(state.stripe.is_some()),
    )
        .into_response()
}

#[derive(Template)]
#[template(path = "terms_accept.html")]
pub struct AcceptTemplate {
    pub user: Option<CurrentUser>,
    pub next: String,
    pub min_age: u32,
    pub version: &'static str,
    /// The learner accepted an earlier version (else: none yet).
    pub updated: bool,
    pub error: Option<String>,
}

#[derive(Deserialize)]
pub struct NextQuery {
    pub next: Option<String>,
}

/// Is `n` a path inside this app, safe to redirect to? It must start with
/// one `/` and hold no backslash, whitespace or control character: browsers
/// drop tabs and newlines and read `\` as `/`, so `/<tab>/evil.example`
/// would become `//evil.example`, another host (#358).
pub(crate) fn is_app_path(n: &str) -> bool {
    n.starts_with('/') && !n.starts_with("//") && !n.chars().any(|c| c == '\\' || c.is_whitespace() || c.is_control())
}

/// Only app-relative paths, so `next` can't send the browser elsewhere.
fn safe_next(next: Option<&str>) -> String {
    match next {
        Some(n) if is_app_path(n) && !n.starts_with("/terms/accept") => n.to_string(),
        _ => "/".to_string(),
    }
}

async fn gate(state: &AppState, user: CurrentUser, next: String, error: Option<String>) -> Result<Response, AppError> {
    let uid = user.id;
    let updated = state.db.call(move |c| terms::latest(c, uid)).await?.is_some();
    Ok(Page(AcceptTemplate { user: Some(user), next, min_age: terms::min_age(), version: terms::version(), updated, error }).into_response())
}

/// GET /terms/accept?next=/somewhere
pub async fn accept_form(
    State(state): State<Arc<AppState>>,
    MaybeUser(user): MaybeUser,
    Query(q): Query<NextQuery>,
) -> Result<Response, AppError> {
    let next = safe_next(q.next.as_deref());
    let Some(user) = user else { return Err(AppError::RedirectToLogin) };
    if user.terms_ok {
        return Ok(Redirect::to(&crate::url(&next)).into_response());
    }
    gate(&state, user, next, None).await
}

#[derive(Deserialize)]
pub struct AcceptForm {
    #[serde(default)]
    pub accept: String,
    #[serde(default)]
    pub next: String,
}

/// POST /terms/accept (accept=on, next)
pub async fn accept(
    State(state): State<Arc<AppState>>,
    MaybeUser(user): MaybeUser,
    Form(f): Form<AcceptForm>,
) -> Result<Response, AppError> {
    let next = safe_next(Some(&f.next));
    let Some(user) = user else { return Err(AppError::RedirectToLogin) };
    if f.accept.is_empty() {
        return gate(&state, user, next, Some(crate::t("terms.accept.tick"))).await;
    }
    let uid = user.id;
    // The box states the minimum age too.
    state
        .db
        .call(move |c| {
            terms::accept(c, uid, terms::version())?;
            crate::privacy::record_age_confirmed(c, uid, true)
        })
        .await?;
    tracing::info!("user {} accepted terms {}", user.id, terms::version());
    Ok(Redirect::to(&crate::url(&next)).into_response())
}

#[derive(Deserialize)]
pub struct DismissForm {
    #[serde(default)]
    pub next: String,
}

/// POST /terms/dismiss (next): close the notice of a minor change, which
/// accepts it (continuing to use the service is acceptance). A material
/// change cannot be dismissed: the gate instead.
pub async fn dismiss(
    State(state): State<Arc<AppState>>,
    MaybeUser(user): MaybeUser,
    Form(f): Form<DismissForm>,
) -> Result<Response, AppError> {
    let next = safe_next(Some(&f.next));
    let Some(user) = user else { return Err(AppError::RedirectToLogin) };
    match &user.terms_notice {
        Some(n) if !n.material => {
            let uid = user.id;
            state.db.call(move |c| terms::accept(c, uid, terms::version())).await?;
            tracing::info!("user {} dismissed the notice of terms {}", user.id, terms::version());
            Ok(Redirect::to(&crate::url(&next)).into_response())
        }
        _ if user.terms_ok => Ok(Redirect::to(&crate::url(&next)).into_response()),
        _ => Err(AppError::TermsRequired(next)),
    }
}

#[cfg(test)]
mod tests {
    use super::safe_next;

    #[test]
    fn next_stays_in_the_app() {
        assert_eq!(safe_next(Some("/lessons?x=1")), "/lessons?x=1");
        assert_eq!(safe_next(Some("//evil.example")), "/");
        assert_eq!(safe_next(Some("https://evil.example")), "/");
        assert_eq!(safe_next(Some("/terms/accept?next=/x")), "/");
        assert_eq!(safe_next(None), "/");
        // Browsers drop tabs and newlines, leaving `//evil.example` (#358).
        assert_eq!(safe_next(Some("/\t/evil.example")), "/");
        assert_eq!(safe_next(Some("/\n/evil.example")), "/");
        assert_eq!(safe_next(Some("/\r/evil.example")), "/");
        assert_eq!(safe_next(Some("/\\evil.example")), "/");
        assert_eq!(safe_next(Some("/ /evil.example")), "/");
        assert_eq!(safe_next(Some("/\u{0}/evil.example")), "/");
        assert_eq!(safe_next(Some("/courses/spanish?lesson=1#top")), "/courses/spanish?lesson=1#top");
    }

    /// The gate over HTTP (#966): /terms is public. An account that never
    /// accepted is sent to /terms/accept; one behind by a material change
    /// sees the notice over GET pages, is refused anything else (API: 403)
    /// and accepts in the notice; one behind by a minor change can dismiss it.
    #[test]
    fn the_gate_holds_until_accepted() {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let state = crate::routes::test_state(false, None);
        let ann = state.db.with(|c| crate::auth::create_user(c, "ann@example.org", "x")).unwrap();
        let bea = state.db.with(|c| crate::auth::create_user(c, "bea@example.org", "x")).unwrap();
        let cat = state.db.with(|c| crate::auth::create_user(c, "cat@example.org", "x")).unwrap();
        state.db.with(|c| crate::terms::accept(c, ann, "older")).unwrap();
        state.db.with(|c| crate::terms::accept(c, bea, crate::dev::MINOR_TERMS_VERSION)).unwrap();
        let sid = |u| format!("sid={}", state.db.with(|c| crate::auth::create_session(c, u)).unwrap());
        let (cookie, bea_c, cat_c) = (sid(ann), sid(bea), sid(cat));
        let app = crate::routes::router(state.clone());
        let base = rt.block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            format!("http://{addr}")
        });
        let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).build().into();
        let get = |path: &str, c: &str| {
            let mut r = agent.get(&format!("{base}{path}")).header("cookie", c).call().unwrap();
            let loc = r.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
            (r.status().as_u16(), loc, r.body_mut().read_to_string().unwrap())
        };
        let post = |path: &str, c: &str, form: &[(&str, &str)]| {
            let r = agent.post(&format!("{base}{path}")).header("cookie", c).send_form(form.iter().copied()).unwrap();
            let loc = r.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
            (r.status().as_u16(), loc)
        };
        let (code, _, body) = get("/terms", "");
        assert_eq!(code, 200);
        assert!(body.contains("Terms and Conditions") && !body.contains("terms-notice"));
        assert_eq!(get("/terms.md", "").0, 200);
        assert_eq!(get("/docs/terms", "").0, 303);

        // Never accepted: the full-page gate.
        let (code, loc, _) = get("/courses", &cat_c);
        assert_eq!((code, loc.as_str()), (303, "/terms/accept?next=%2Fcourses"));

        // A material change: the notice over the page, nothing else.
        let (code, _, body) = get("/courses", &cookie);
        assert_eq!(code, 200);
        assert!(body.contains("data-tier=\"material\"") && body.contains("data-testid=\"terms-accept-check\""), "{body}");
        assert!(body.contains("name=\"next\" value=\"/courses\"") && !body.contains("terms-notice-dismiss"));
        // Not over the profile, where the account can be closed.
        let (code, _, body) = get("/profile", &cookie);
        assert!(code == 200 && !body.contains("data-testid=\"terms-gate\""));
        assert_eq!(get("/api/progress", &cookie).0, 403);
        // Sprint and the tutor have their own layout, without the notice: the gate.
        let (code, loc, _) = get("/sprint", &cookie);
        assert_eq!((code, loc.as_str()), (303, "/terms/accept?next=%2Fsprint"));
        let (code, loc, _) = get("/tutor", &cookie);
        assert_eq!((code, loc.as_str()), (303, "/terms/accept?next=%2Ftutor"));
        assert_eq!(post("/profile/ai", &cookie, &[("ai_off", "1")]), (303, "/terms/accept?next=%2F".to_string()));
        // It cannot be dismissed.
        let (code, loc) = post("/terms/dismiss", &cookie, &[("next", "/courses")]);
        assert_eq!((code, loc.as_str()), (303, "/terms/accept?next=%2Fcourses"));
        assert_eq!(post("/terms/accept", &cookie, &[("accept", ""), ("next", "/courses")]).0, 200);
        assert_eq!(post("/terms/accept", &cookie, &[("accept", "on"), ("next", "/courses")]), (303, "/courses".to_string()));
        let (code, _, body) = get("/courses", &cookie);
        assert!(code == 200 && !body.contains("data-testid=\"terms-gate\""));
        assert_ne!(get("/api/progress", &cookie).0, 403);

        // A minor change: the notice, which blocks nothing and is dismissed.
        let (code, _, body) = get("/courses", &bea_c);
        assert_eq!(code, 200);
        assert!(body.contains("data-tier=\"minor\"") && body.contains("data-testid=\"terms-notice-dismiss\"") && !body.contains("terms-accept-check"));
        assert_ne!(get("/api/progress", &bea_c).0, 403);
        assert_eq!(post("/terms/dismiss", &bea_c, &[("next", "/courses")]), (303, "/courses".to_string()));
        let (_, _, body) = get("/courses", &bea_c);
        assert!(!body.contains("data-testid=\"terms-gate\""));
        let latest = state.db.with(|c| crate::terms::latest(c, bea)).unwrap().unwrap();
        assert_eq!(latest.version, crate::terms::version());
        let (_, _, body) = get("/profile", &bea_c);
        assert!(body.contains(&format!("data-terms-version=\"{}\"", crate::terms::version())));
    }
}
