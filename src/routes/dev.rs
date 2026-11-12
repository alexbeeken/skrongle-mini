//! `/dev/*`: local testing routes, mounted only in dev mode (`crate::dev`).

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Redirect, Response};
use axum::{Form, Json};
use serde::Deserialize;
use std::sync::Arc;

use super::Page;
use crate::auth::{self, CurrentUser, MaybeUser};
use crate::dev;
use crate::error::AppError;
use crate::AppState;

#[derive(Deserialize)]
pub struct LoginParams {
    /// A course to open (`?open` alone: the seed course).
    pub open: Option<String>,
    /// `dev` (or the older `fake`): the simulated provider's keys, saved as the server's,
    /// for all three providers (`dev_ai::LEARNER_KEYS`), so the AI forms are
    /// unlocked.
    pub keys: Option<String>,
    /// Where to land, an app path such as `/courses/generate`.
    pub next: Option<String>,
    /// With `open`: `1` shows the course's first-visit settings wizard on
    /// its home page again (#574). Without it the course counts as set up,
    /// so a login lands on the home page itself.
    pub wizard: Option<String>,
}

/// Only app-relative paths, so `next` can't send the browser elsewhere.
fn landing(next: Option<&str>) -> String {
    match next {
        Some(n) if super::password::is_app_path(n) => crate::url(n),
        _ => crate::url("/"),
    }
}

async fn dev_user(state: &AppState) -> Result<i64, AppError> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || dev::ensure_user(&db))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .map_err(AppError::Internal)
}

/// GET /dev/login[?open=spanish[&wizard=1]][&keys=fake|dev][&next=/courses/generate]
/// Log in as the dev user (created on first use).
pub async fn login(State(state): State<Arc<AppState>>, Query(q): Query<LoginParams>) -> Result<Response, AppError> {
    let uid = dev_user(&state).await?;
    if let Some(open) = q.open.as_deref() {
        let id = if open.is_empty() { dev::SEED_COURSE.to_string() } else { open.to_string() };
        let cid = id.clone();
        let ok = state.courses.is_builtin(&id)
            || state.db.call(move |c| crate::courses::course(c, &cid)).await?.is_some_and(|r| r.owner_id == uid && !r.is_generating());
        if !ok || state.courses.get(&state.db, &id).await?.is_none() {
            return Err(AppError::BadRequest(format!("no course '{id}' the dev user can open")));
        }
        let wizard = q.wizard.as_deref() == Some("1");
        state
            .db
            .call(move |c| {
                crate::courses::open_project(c, uid, &id)?;
                if wizard {
                    c.execute("UPDATE projects SET set_up = 0 WHERE user_id = ?1 AND course_id = ?2", rusqlite::params![uid, id])?;
                } else {
                    crate::courses::set_project_settings(c, uid, &id, &crate::courses::SettingsChange::default())?;
                }
                Ok(())
            })
            .await?;
    }
    match q.keys.as_deref() {
        None | Some("") => {}
        Some("fake" | "dev") => {
            let vault = state.vault.clone();
            state.db.call(move |c| dev::save_dev_keys(c, &vault)).await?;
        }
        Some(other) => return Err(AppError::BadRequest(format!("keys={other}: keys=fake or keys=dev"))),
    }
    let token = state.db.call(move |c| auth::create_session(c, uid)).await?;
    tracing::info!("dev: logged in as '{}'", dev::DEV_EMAIL);
    Ok(([auth::set_session_cookie(&token, state.secure_cookies)], Redirect::to(&landing(q.next.as_deref()))).into_response())
}

#[derive(Deserialize)]
pub struct VocabBody {
    /// The lesson whose words to collect.
    pub lesson: String,
    /// How many days ago they were collected: 1 or more makes them due today.
    pub days_ago: i64,
}

/// POST /dev/vocab {"lesson": slug, "days_ago": n}: collect a lesson's words
/// for whoever is signed in as if `n` days ago, so a spec can see words due
/// (#1360) without waiting a day. `{"collected": n}`.
pub async fn vocab(State(state): State<Arc<AppState>>, p: auth::Project, Json(body): Json<VocabBody>) -> Result<Response, AppError> {
    let lesson = p.course.lesson_by_slug(&body.lesson).ok_or(AppError::NotFound)?;
    let (uid, tz, cid, lid) = (p.user.id, p.user.tz, p.course_id().to_string(), lesson.id.clone());
    let words: Vec<String> = lesson.vocabulary.iter().map(|v| v.term.clone()).collect();
    let t = crate::db::now() - body.days_ago.clamp(0, 400) * 86_400;
    let n = state
        .db
        .call(move |c| crate::vocab::record_learned_at(c, uid, &cid, &lid, &words.iter().map(String::as_str).collect::<Vec<_>>(), t, tz))
        .await?
        .len();
    Ok(Json(serde_json::json!({ "collected": n })).into_response())
}

#[derive(Deserialize)]
pub struct ResetParams {
    pub next: Option<String>,
}

/// GET /dev/reset[?next=…]: wipe the dev user's progress and own courses.
pub async fn reset(State(state): State<Arc<AppState>>, Query(q): Query<ResetParams>) -> Result<Response, AppError> {
    let uid = dev_user(&state).await?;
    let gone = state.db.call(move |c| dev::reset_user(c, uid)).await?;
    for id in &gone {
        state.courses.invalidate(id);
    }
    tracing::info!("dev: reset '{}' ({} courses deleted)", dev::DEV_EMAIL, gone.len());
    let next = q.next.unwrap_or_else(|| "/courses?notice=dev-reset".into());
    Ok(Redirect::to(&landing(Some(&next))).into_response())
}

/// JSON, unless a browser asks for a page (`Accept: text/html`); `format`
/// (`json`, `html`) overrides.
fn wants_html(headers: &axum::http::HeaderMap, format: Option<&str>) -> bool {
    match format {
        Some("json") => false,
        Some("html") => true,
        _ => headers.get(axum::http::header::ACCEPT).and_then(|v| v.to_str().ok()).is_some_and(|a| a.contains("text/html")),
    }
}

#[derive(Deserialize)]
pub struct FormatParams {
    pub format: Option<String>,
}

#[derive(Deserialize)]
pub struct BoxActForm {
    /// `delete`.
    #[serde(rename = "do")]
    pub action: String,
}

/// The page's own requests (`X-Dev-Box: 1`, from devbox.js) get 204; a
/// plain form post goes back to `list`.
fn done(headers: &axum::http::HeaderMap, list: &str) -> Response {
    if headers.contains_key("x-dev-box") {
        axum::http::StatusCode::NO_CONTENT.into_response()
    } else {
        Redirect::to(&crate::url(list)).into_response()
    }
}

// ---------- the dev authenticator: an authenticator app's TOTP codes ----------
//
// A section of the dev pages: an authenticator app (like
// Google Authenticator) holding TOTP keys (`dev::Authenticator`, in
// memory). Add one by pasting an `otpauth://totp/…` link or a setup key, or
// by "scanning": a link to `/dev/authenticator?uri=otpauth://…` opens the
// page with it filled in, one click from added. Codes are worked out here
// from the secret, as an app does (`crate::totp`); the page counts down
// to the next one and polls `/dev/authenticator/rows` (static-dev/devbox.js).

/// What the add form opens with when nothing was scanned, and what it
/// offers again when the list goes empty (static-dev/devbox.js): an example
/// account, one click from added.
pub const EXAMPLE_TOTP_URI: &str = "otpauth://totp/Skrongle:ann@example.org?secret=JBSWY3DPEHPK3PXP&issuer=Skrongle";

/// One account's row (`dev_authenticator_rows.html`).
pub struct AuthRow {
    pub id: i64,
    pub issuer: String,
    pub account: String,
    pub code: String,
    /// When `code` stops being the current one (unix seconds).
    pub expires: u64,
    pub remaining: u64,
    pub period: u64,
    /// Not the defaults (SHA-1, 6 digits, 30 s): shown under the name.
    pub settings: String,
}

impl AuthRow {
    /// The code in two halves, as apps show it (`123 456`).
    pub fn grouped(&self) -> String {
        let mid = self.code.len() / 2;
        format!("{} {}", &self.code[..mid], &self.code[mid..])
    }

    /// The avatar's letter.
    pub fn initial(&self) -> String {
        let name = if self.issuer.is_empty() { &self.account } else { &self.issuer };
        name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".into())
    }
}

#[derive(Template)]
#[template(path = "dev_authenticator_rows.html")]
pub struct AuthRows {
    pub rows: Vec<AuthRow>,
}

#[derive(Template)]
#[template(path = "dev_authenticator.html")]
pub struct AuthenticatorTemplate {
    pub user: Option<CurrentUser>,
    /// A "scanned" link (`?uri=`), filled into the add form.
    pub scanned: String,
    /// The add form's value: `scanned`, else `EXAMPLE_TOTP_URI`.
    pub prefill: String,
    pub now: u64,
    pub list: AuthRows,
}

impl AuthenticatorTemplate {
    /// The section, for `dev_box_kinds.html`.
    pub fn section(&self) -> &'static str {
        "auth"
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn auth_rows(state: &AppState, now: u64) -> AuthRows {
    let rows = state
        .dev.authenticator
        .list()
        .into_iter()
        .map(|a| {
            let k = &a.key;
            let defaults = k.algorithm == crate::totp::Algorithm::Sha1 && k.digits == crate::totp::DEFAULT_DIGITS && k.period == crate::totp::DEFAULT_PERIOD;
            AuthRow {
                id: a.id,
                issuer: k.issuer.clone(),
                account: k.account.clone(),
                code: k.code_at(now),
                expires: now + k.remaining(now),
                remaining: k.remaining(now),
                period: k.period,
                settings: if defaults { String::new() } else { format!("{}, {} digits, {} s", k.algorithm.as_str(), k.digits, k.period) },
            }
        })
        .collect();
    AuthRows { rows }
}

#[derive(Deserialize)]
pub struct AuthenticatorParams {
    pub format: Option<String>,
    /// A scanned `otpauth://` link to fill in.
    pub uri: Option<String>,
}

/// GET /dev/authenticator: the dev authenticator's accounts with their
/// current codes. JSON for scripts (`{"now", "accounts": [{id, issuer,
/// account, code, remaining, period, digits, algorithm, secret}]}`); for a
/// browser, the dev pages' Authenticator section. `?uri=otpauth://…`
/// is the "scan": the add form opens filled in with it; without one it
/// opens with an example account (`EXAMPLE_TOTP_URI`).
pub async fn authenticator(
    State(state): State<Arc<AppState>>,
    MaybeUser(user): MaybeUser,
    headers: axum::http::HeaderMap,
    Query(q): Query<AuthenticatorParams>,
) -> Result<Response, AppError> {
    let now = unix_now();
    if !wants_html(&headers, q.format.as_deref()) {
        let accounts: Vec<_> = state
            .dev.authenticator
            .list()
            .iter()
            .map(|a| {
                let k = &a.key;
                serde_json::json!({ "id": a.id, "issuer": k.issuer, "account": k.account, "code": k.code_at(now), "remaining": k.remaining(now), "period": k.period, "digits": k.digits, "algorithm": k.algorithm.as_str(), "secret": k.secret_base32() })
            })
            .collect();
        return Ok(Json(serde_json::json!({ "now": now, "accounts": accounts })).into_response());
    }
    let scanned = q.uri.unwrap_or_default();
    let prefill = if scanned.is_empty() { EXAMPLE_TOTP_URI.to_string() } else { scanned.clone() };
    Ok(Page(AuthenticatorTemplate { user, scanned, prefill, now, list: auth_rows(&state, now) }).into_response())
}

/// GET /dev/authenticator/rows: the list as HTML and the server's clock,
/// for the page's poll: `{"html", "now"}`.
pub async fn authenticator_rows(State(state): State<Arc<AppState>>) -> Result<Response, AppError> {
    let now = unix_now();
    Ok(Json(serde_json::json!({ "html": auth_rows(&state, now).render()?, "now": now })).into_response())
}

#[derive(Deserialize)]
pub struct AuthenticatorAddForm {
    /// An `otpauth://totp/…` link or a base32 setup key.
    #[serde(default)]
    pub uri: String,
}

/// POST /dev/authenticator (form `uri`: an `otpauth://totp/…` link or a
/// base32 setup key): add the account, then back to the page (204 for the
/// page's own posts, `{"id"}` as JSON with `Accept: application/json`);
/// 400 with why as text.
pub async fn authenticator_add(State(state): State<Arc<AppState>>, headers: axum::http::HeaderMap, Form(f): Form<AuthenticatorAddForm>) -> Result<Response, AppError> {
    let key = match crate::totp::Key::parse(&f.uri) {
        Ok(k) => k,
        Err(why) if headers.contains_key("x-dev-box") => return Ok((axum::http::StatusCode::BAD_REQUEST, why).into_response()),
        Err(why) => return Err(AppError::BadRequest(why)),
    };
    let id = state.dev.authenticator.add(key, crate::db::now());
    if headers.get(axum::http::header::ACCEPT).and_then(|v| v.to_str().ok()).is_some_and(|a| a.contains("application/json")) {
        return Ok(Json(serde_json::json!({ "id": id })).into_response());
    }
    Ok(done(&headers, "/dev/authenticator"))
}

/// POST /dev/authenticator/{id} (`do=delete`): remove the account, as an
/// app's "Remove" does; then back to the page (204 for the page's own posts).
pub async fn authenticator_act(State(state): State<Arc<AppState>>, headers: axum::http::HeaderMap, Path(id): Path<i64>, Form(f): Form<BoxActForm>) -> Result<Response, AppError> {
    if f.action != "delete" {
        return Err(AppError::BadRequest(format!("unknown action {}", f.action)));
    }
    if !state.dev.authenticator.remove(id) {
        return Err(AppError::NotFound);
    }
    Ok(done(&headers, "/dev/authenticator"))
}

// ---------- /dev/jobs: the background job table (dev/jobs_page.rs) ----------

/// The `/dev/*` routes, mounted only in dev mode (`routes::router`); and
/// even then only for requests made on this machine, not through a proxy.
pub fn routes() -> axum::Router<Arc<AppState>> {
    use axum::routing::{get, post};
    use axum::Router;
    // The dev pages (the jobs table, the authenticator, the AI provider's
    // page): for whoever is testing, no sign-in (they are for all accounts).
    let devbox = Router::new()
        .route("/dev/jobs", get(jobs))
        .route("/dev/jobs/fail", post(jobs_fail))
        .route("/dev/jobs/{id}", post(jobs_act))
        .route("/dev/authenticator", get(authenticator).post(authenticator_add))
        .route("/dev/authenticator/rows", get(authenticator_rows))
        .route("/dev/authenticator/{id}", post(authenticator_act))
        // A public OAuth client playing the flow against this server (#169).
        .route("/dev/oauth-client", get(oauth_client::page))
        .route("/dev/oauth-client/callback", get(oauth_client::page))
        .route("/dev/ai", get(super::dev_ai::page))
        .route("/dev/ai/rows", get(super::dev_ai::rows))
        .route("/dev/ai/clear", post(super::dev_ai::clear))
        .route("/dev/ai/hold", post(super::dev_ai::hold))
        .route("/dev/ai/held/{stem}", post(super::dev_ai::answer))
        .route("/dev/ai/profiles/{who}", post(super::dev_ai::profile_act))
        .route("/dev/ai/calls/{id}", get(super::dev_ai::call))
        .route("/dev/ai/fixtures", post(super::dev_ai::scenario_add))
        .route("/dev/ai/fixtures/{name}", get(super::dev_ai::fixture).post(super::dev_ai::fixture_act));
    Router::new()
        .route("/dev/login", get(login))
        // The floating dev device on /login/code: for the holder of the
        // pending sign-in's twofa cookie, no session (see the handler).
        .route("/login/code/device", get(super::auth::login_code_device))
        .route("/dev/reset", get(reset))
        .route("/dev/vocab", post(vocab))
        .merge(devbox)
        // The simulated providers' wire APIs: the app's own server-side
        // clients call them with an API key (checked there), no cookie.
        .route("/dev/ai/anthropic/v1/messages", post(super::dev_ai::anthropic_messages))
        .route("/dev/ai/anthropic/v1/models", get(super::dev_ai::anthropic_models))
        .route("/dev/ai/openai/v1/chat/completions", post(super::dev_ai::openai_chat))
        .route("/dev/ai/openai/v1/models", get(super::dev_ai::openai_models))
        .route("/dev/ai/google/v1beta/openai/chat/completions", post(super::dev_ai::google_chat))
        .route("/dev/ai/google/v1beta/openai/models", get(super::dev_ai::google_models))
        .route("/dev/ai/deepseek/v1/chat/completions", post(super::dev_ai::compatible_chat))
        .route("/dev/ai/mistral/v1/chat/completions", post(super::dev_ai::compatible_chat))
        .route("/dev/ai/xai/v1/chat/completions", post(super::dev_ai::compatible_chat))
        .route("/dev/ai/openrouter/api/v1/chat/completions", post(super::dev_ai::compatible_chat))
        .route("/dev/ai/ollama/v1/chat/completions", post(super::dev_ai::ollama_chat))
        // The boot id, for the dev page to notice a restart (#1036).
        .route("/dev/boot", get(|| async { crate::dev::boot_id() }))
        .route_layer(axum::middleware::from_fn(crate::dev::local_only))
        // Pasted LLM replies and fixtures are whole courses.
        .layer(axum::extract::DefaultBodyLimit::max(super::BIG_BODY_LIMIT))
}

mod jobs_page;
mod oauth_client;
pub use jobs_page::{jobs, jobs_act, jobs_fail};

#[cfg(test)]
mod tests;
