//! Axum router, shared response helpers, static assets.

use askama::Template;
use axum::extract::{Path, Query, RawQuery};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::Router;
use rust_embed::RustEmbed;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use crate::error::AppError;
use crate::AppState;

pub mod account_deletion;
pub mod auth;
pub mod courses;
pub mod share;
pub mod dev;
pub mod dev_ai;
pub mod sprint;
pub mod docs;
pub mod games;
pub mod guide;
pub mod listening;
pub mod map;
pub mod pages;
pub mod profile;
pub mod progress;
pub mod quiz;
pub mod password;
pub mod reset;
pub mod shop;
pub mod superuser;
pub mod tutor;
pub mod vocab;

/// Everything under `static/` is compiled into the binary.
#[derive(RustEmbed)]
#[folder = "static/"]
struct Assets;

/// The Markdown documentation under `docs/`, served at `/docs`.
#[derive(RustEmbed)]
#[folder = "docs/"]
pub struct Docs;

/// A permanent redirect to `to` with the request's query string, for a path
/// that moved (the modes used to live at /d, /k and /games). A 308, so a
/// POST stays a POST.
async fn moved(to: &str, RawQuery(query): RawQuery) -> Redirect {
    let url = crate::url(to);
    Redirect::permanent(&match query {
        Some(q) => format!("{url}?{q}"),
        None => url,
    })
}


/// The body limit of every request (#370): forms and JSON from the pages are
/// a few KB. Larger bodies only go to `big` routes.
pub const BODY_LIMIT: usize = 64 * 1024;
/// The body limit of the course editor, the AI forms (pasted material up to
/// `ai::MAX_NOTES` characters, percent-encoded) and the dev tools: axum's
/// own default, what they always had. All of them need a session (the dev
/// tools a local request).
pub const BIG_BODY_LIMIT: usize = 2 * 1024 * 1024;

/// Reads a small request body (up to `BODY_LIMIT`, by its Content-Length)
/// before any handler runs (#1063). A handler that answers without reading
/// the body (a refusal, a 404, a POST with a form it ignores) otherwise
/// leaves hyper with unread bytes: when they arrive after the answer, hyper
/// closes the connection and the client's kernel resets it, which can throw
/// the answer away before the browser reads it. Larger bodies stream as
/// before; their routes read them.
async fn read_small_body(req: axum::extract::Request, next: axum::middleware::Next) -> Response {
    let small = req
        .headers()
        .get(axum::http::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok()?.parse::<usize>().ok())
        .is_some_and(|n| n > 0 && n <= BODY_LIMIT);
    if !small {
        return next.run(req).await;
    }
    let (parts, body) = req.into_parts();
    match axum::body::to_bytes(body, BODY_LIMIT).await {
        Ok(bytes) => next.run(axum::extract::Request::from_parts(parts, axum::body::Body::from(bytes))).await,
        // The client stopped sending: nobody is left to read an answer.
        Err(_) => StatusCode::BAD_REQUEST.into_response(),
    }
}

/// A route that takes a large body (see `BIG_BODY_LIMIT`).
fn big<S: Clone + Send + Sync + 'static>(r: axum::routing::MethodRouter<S>) -> axum::routing::MethodRouter<S> {
    r.layer(axum::extract::DefaultBodyLimit::max(BIG_BODY_LIMIT))
}

pub fn router(state: Arc<AppState>) -> Router {
    let mut app = Router::new()
        // pages (all inside the active course)
        .route("/", get(pages::root))
        // Study mode (short study lessons with quizzes): overview, lessons, practice, map, vocabulary, history
        // Study mode opens at its lessons; the overview it had is gone.
        .route("/study", get(|q: RawQuery| moved("/lessons", q)))
        .route("/lessons", get(pages::lessons))
        .route("/map", get(map::map))
        .route("/lessons/{slug}", get(pages::lesson))
        .route("/lessons/{slug}/read", post(progress::read))
        .route("/api/lessons/{slug}/note", post(pages::save_note))
        .route("/api/practice/finish", post(progress::practice_finish))
        .route("/api/progress", get(progress::overview))
        .route("/api/progress/lessons/{lesson}", get(progress::lesson))
        .route("/api/progress/seen", post(progress::seen))
        .route("/api/progress/answers", post(progress::answers))
        .route("/api/progress/pass", post(progress::pass))
        .route("/history", get(pages::history))
        .route("/mistakes", get(pages::mistakes))
        .route("/practice", get(quiz::practice))
        .route("/listening", get(listening::index))
        .route("/listening/play", get(listening::play))
        .route("/api/listening/check", post(listening::check))
        .route("/api/listening/finish", post(listening::finish))
        // courses: picking one, building one, generating one
        .route("/courses", get(courses::picker))
        .route("/courses/open/{id}", post(courses::open))
        .route("/courses/new", get(courses::new_form).post(courses::create))
        .route("/courses/generate", get(courses::generate_form))
        .route("/courses/generate/{job}", get(courses::generate_status))
        .route("/api/generate/{job}", get(courses::generate_api))
        .route("/courses/{id}", get(courses::editor))
        .route("/courses/{id}/file", get(courses::file_form).merge(big(post(courses::file_save))))
        .route("/courses/{id}/file/delete", post(courses::file_delete))
        .route("/courses/{id}/publish", post(courses::publish))
        .route("/courses/{id}/delete", post(courses::delete))
        .route("/courses/{id}/copy", post(courses::copy))
        .route("/courses/{id}/translate", post(courses::translate))
        .route("/courses/{id}/cancel", post(courses::generate_cancel))
        .route("/courses/{id}/ai", big(post(courses::ai_start)))
        .route("/courses/{id}/generate", big(post(courses::generate_in)))
        .route("/courses/{id}/undo", post(courses::undo))
        .route("/courses/{id}/redo", post(courses::redo))
        .route("/courses/{id}/review", big(post(courses::review_accept)))
        .route("/api/review-quiet", post(courses::review_quiet))
        .route("/courses/{id}/lessons", big(post(courses::save_lessons)))
        .route("/courses/{id}/export.json", get(courses::export))
        .route("/courses/{id}/settings", post(courses::set_settings))
        .route("/courses/{id}/stat/{metric}", get(courses::stat_detail))
        .route("/courses/{id}/placement", get(courses::placement).post(courses::mark_placement))
        // sharing a course with other learners (#451)
        .route("/courses/shared/{token}", get(share::open_link))
        .route("/courses/{id}/share", get(share::to_course).post(share::invite))
        .route("/courses/{id}/share/remove", post(share::remove))
        .route("/api/courses/{id}/files", get(courses::files_api).merge(big(post(courses::upload))))
        .route("/api/courses/{id}/file", get(courses::file_api))
        .route("/api/courses/{id}/preview-answer", post(courses::preview_answer))
        .route("/api/courses/{id}/answer-key", post(courses::answer_key))
        .route("/api/courses/{id}/lessons", big(post(courses::save_lessons)))
        // big(): a paste near the 64 KB cap is past the default body limit once JSON-encoded; the handler gives the friendly "at most 64 KB" refusal (#453)
        .route("/api/courses/{id}/words", big(post(courses::paste_words)))
        // the language picker on the login and register pages (and anywhere logged out)
        .route("/lang", post(auth::set_lang))
        // documentation (public)
        .route("/docs", get(docs::index))
        .route("/llms.txt", get(docs::llms_txt))
        .route("/docs/{page}", get(docs::page))
        .route("/self-host", get(docs::self_host))
        .route("/privacy", get(docs::privacy))
        .route("/cookies", get(docs::cookies))
        .route("/password/change", get(password::change_form).post(password::change))
        // the modes' old paths (/d, /k, /g, /games), for bookmarks and open tabs
        .route("/d", get(|q: RawQuery| moved("/sprint", q)))
        .route("/d/{*rest}", get(|Path(rest): Path<String>, q: RawQuery| async move { moved(&format!("/sprint/{rest}"), q).await }))
        .route("/api/d/{*rest}", post(|Path(rest): Path<String>, q: RawQuery| async move { moved(&format!("/api/sprint/{rest}"), q).await }))
        .route("/k", get(|q: RawQuery| moved("/lessons", q)))
        .route("/g", get(|q: RawQuery| moved("/play", q)))
        .route("/games", get(|q: RawQuery| moved("/play", q)))
        .route("/games/{*rest}", get(|Path(rest): Path<String>, q: RawQuery| async move { moved(&format!("/play/{rest}"), q).await }))
        // Sprint mode (phone-first sessions)
        .route("/sprint", get(sprint::home))
        .route("/sprint/play", get(sprint::play))
        .route("/api/sprint/check", post(sprint::check))
        .route("/api/sprint/finish", post(sprint::finish))
        // AI tutoring (chat; the learner's own API key)
        .route("/tutor", get(tutor::page))
        .route("/api/tutor/session", get(tutor::session))
        .route("/api/tutor/start", post(tutor::start))
        .route("/api/tutor/turn", post(tutor::turn))
        .route("/api/tutor/cancel", post(tutor::cancel))
        // guide
        .route("/guide", get(guide::map))
        .route("/guide/{slug}", get(guide::stage))
        .route("/api/guide/step", post(guide::complete_step))
        .route("/api/guide/restart", post(guide::restart))
        // Play mode (games and one-off exercises)
        .route("/play", get(games::hub))
        .route("/play/matching", get(games::pick))
        .route("/play/matching/play", get(games::play))
        .route("/api/games/matching/start", post(games::start))
        .route("/api/games/matching/finish", post(games::finish))
        .route("/play/conjugation", get(games::conjugation_setup))
        .route("/play/conjugation/play", get(games::conjugation_play))
        .route("/api/games/conjugation/deal", post(games::conjugation_deal))
        .route("/api/games/conjugation/check", post(games::conjugation_check))
        .route("/api/games/conjugation/finish", post(games::conjugation_finish))
        .route("/play/numbers", get(games::numbers))
        .route("/api/games/numbers/finish", post(games::numbers_finish))
        .route("/play/dates", get(games::dates))
        .route("/api/games/dates/finish", post(games::dates_finish))
        // the active course's picture files (SVG)
        .route("/pictures/{*path}", get(course_picture))
        .route("/shop", get(shop::page))
        .route("/api/shop/buy", post(shop::buy))
        .route("/badges/{file}", get(shop::art))
        // vocabulary
        .route("/vocabulary", get(vocab::index))
        .route("/lookup", get(vocab::lookup))
        .route("/vocabulary/study", get(vocab::study))
        .route("/api/vocabulary/review", post(vocab::review))
        .route("/api/vocabulary/due", get(vocab::due))
        // auth
        .route("/login", get(auth::login_form).post(auth::login))
        .route("/login/code", get(auth::login_code_form).post(auth::login_code))
        .route("/login/code/resend", post(auth::login_code_resend))
        .route("/login/code/cancel", post(auth::login_code_cancel))
        .route("/register", get(auth::register_form).post(auth::register))
        .route("/logout", post(auth::logout))
        .route("/account/deletion", get(account_deletion::page))
        .route("/account/deletion/cancel", post(account_deletion::cancel))
        .route("/verify-email", get(auth::verify_email))
        .route("/reset", get(reset::form).post(reset::submit))
        // profile (works without an active course)
        .route("/profile", get(profile::show))
        .route("/api/profile", post(profile::save))
        .route("/profile/password", post(profile::change_password))
        .route("/profile/sessions/end-others", post(profile::end_other_sessions))
        .route("/profile/api-keys/remove", post(profile::remove_api_key))
        .route("/profile/ai", post(profile::set_ai))
        .route("/profile/export", post(profile::export))
        .route("/profile/export/download", get(profile::export_download).post(profile::export_download_now))
        .route("/profile/delete", post(profile::delete_account))
        .route("/profile/verify-email/resend", post(profile::resend_verification))
        .route("/profile/two-factor", post(profile::set_two_factor))
        .route("/profile/two-factor/totp/confirm", post(profile::confirm_totp))
        .route("/profile/two-factor/totp/remove", post(profile::remove_totp))
        .route("/profile/two-factor/code", post(profile::send_confirm_code))
        // superusers (404 for anyone else)
        .route("/superuser", get(superuser::show))
        .route("/superuser/reset-email-verification", post(superuser::reset_email_verification))
        .route("/superuser/invites", post(superuser::make_invite))
        .route("/superuser/invites/revoke", post(superuser::revoke_invite))
        .route("/superuser/temp-password", post(superuser::set_temp_password))
        .route("/superuser/two-factor-off", post(superuser::turn_off_two_factor))
        // api
        .route("/api/answer", post(quiz::answer))
        .route("/api/lesson-check", post(quiz::lesson_check))
        // misc
        .route("/static/{*path}", get(static_asset))
        .route("/manifest.webmanifest", get(web_manifest))
        .route("/healthz", get(healthz));
    // Local testing only (`SKRONGLE_DEV=1` on a loopback bind): without it these paths 404.
    // And even then only for requests made on this machine, not through a proxy.
    if state.dev {
        // The dev mailbox (e-mail, the authenticator, the AI provider's
        // page): for whoever is testing, no sign-in (it is for all accounts).
        let devbox = Router::new()
            .route("/dev/mail", get(dev::mail))
            .route("/dev/jobs", get(dev::jobs))
            .route("/dev/jobs/fail", post(dev::jobs_fail))
            .route("/dev/jobs/{id}", post(dev::jobs_act))
            .route("/dev/mail/send", post(dev::mail_send))
            .route("/dev/mail/{id}", get(dev::mail_message).post(dev::mail_act))
            .route("/dev/inbox", get(dev::inbox))
            .route("/dev/inbox/rows", get(dev::box_rows))
            .route("/dev/inbox/latest", get(dev::inbox_latest))
            .route("/dev/inbox/clear", post(dev::inbox_clear))
            .route("/dev/inbox/{id}", get(dev::inbox_message).post(dev::inbox_act))
            .route("/dev/inbox/{id}/raw", get(dev::inbox_raw))
            .route("/dev/authenticator", get(dev::authenticator).post(dev::authenticator_add))
            .route("/dev/authenticator/rows", get(dev::authenticator_rows))
            .route("/dev/authenticator/{id}", post(dev::authenticator_act))
            .route("/dev/ai", get(dev_ai::page))
            .route("/dev/ai/rows", get(dev_ai::rows))
            .route("/dev/ai/clear", post(dev_ai::clear))
            .route("/dev/ai/hold", post(dev_ai::hold))
            .route("/dev/ai/held/{stem}", post(dev_ai::answer))
            .route("/dev/ai/profiles/{who}", post(dev_ai::profile_act))
            .route("/dev/ai/calls/{id}", get(dev_ai::call))
            .route("/dev/ai/fixtures", post(dev_ai::scenario_add))
            .route("/dev/ai/fixtures/{name}", get(dev_ai::fixture).post(dev_ai::fixture_act));
        app = app.merge(
            Router::new()
                .route("/dev/login", get(dev::login))
                // The floating dev device on /login/code: for the holder of the
                // pending sign-in's twofa cookie, no session (see the handler).
                .route("/login/code/device", get(auth::login_code_device))
                .route("/dev/reset", get(dev::reset))
                .route("/dev/vocab", post(dev::vocab))
                .merge(devbox)
                // The simulated providers' wire APIs: the app's own server-side
                // clients call them with an API key (checked there), no cookie.
                .route("/dev/ai/anthropic/v1/messages", post(dev_ai::anthropic_messages))
                .route("/dev/ai/anthropic/v1/models", get(dev_ai::anthropic_models))
                .route("/dev/ai/openai/v1/chat/completions", post(dev_ai::openai_chat))
                .route("/dev/ai/openai/v1/models", get(dev_ai::openai_models))
                .route("/dev/ai/google/v1beta/openai/chat/completions", post(dev_ai::google_chat))
                .route("/dev/ai/google/v1beta/openai/models", get(dev_ai::google_models))
                // The content check's Jev stand-in (#1036).
                .route("/dev/boot", get(|| async { crate::dev::boot_id() }))
                .route_layer(axum::middleware::from_fn(crate::dev::local_only))
                // Pasted LLM replies and fixtures are whole courses.
                .layer(axum::extract::DefaultBodyLimit::max(BIG_BODY_LIMIT)),
        );
    }
    let secure = state.secure_cookies;
    let dev = state.dev;
    let reload = dev && crate::dev::reload_wanted();
    let app = app
        .fallback(|| async { AppError::NotFound })
        .layer(axum::middleware::from_fn_with_state(state.clone(), crate::error::in_layout));
    // A reverse proxy's identity header signs people in (#642), before any
    // handler or the error layout looks for a session.
    let app = if state.proxy_auth.is_some() {
        app.layer(axum::middleware::from_fn_with_state(state.clone(), crate::proxy_auth::middleware))
    } else {
        app
    };
    // A blocked address (#967) is refused before any session lookup.
    let app = app.with_state(state);

    let base = crate::base_path();
    let app = if base.is_empty() {
        app
    } else {
        // Mounted under a prefix (e.g. behind nginx at /spanish). `nest` maps the bare
        // prefix to the home menu; the trailing-slash form is redirected to it so there
        // is a single canonical root URL. All generated links are absolute.
        let root = crate::url("/");
        Router::new()
            .route(&format!("{base}/"), get(move || async move { axum::response::Redirect::permanent(&root) }))
            .nest(base, app)
            .fallback(|| async { AppError::NotFound })
    };
    // `./.dev-start --watch`: pages reload themselves after a restart.
    let app = if reload { app.layer(axum::middleware::from_fn(crate::dev::inject_reload)) } else { app };
    // Dev mode: every route answers only requests made to this machine (#388).
    let app = if dev { app.layer(axum::middleware::from_fn(crate::dev::local_host_only)) } else { app };
    // Outermost: every response (404s included) gets the security headers;
    // cross-site POSTs are refused before any handler runs.
    app.layer(axum::middleware::from_fn(crate::security::csrf))
        .layer(axum::middleware::from_fn(read_small_body))
        .layer(axum::middleware::from_fn(crate::i18n::middleware))
        .layer(axum::middleware::from_fn(move |req, next| crate::security::headers(secure, req, next)))
        // Applied first, so a route's own `big` limit (set later, inside) wins.
        .layer(axum::extract::DefaultBodyLimit::max(BODY_LIMIT))
}

/// Course facts templates need on nearly every page: labels, languages,
/// helper characters, whether paradigm and listening features exist.
#[derive(Clone, Debug)]
pub struct CourseView {
    pub title: String,
    pub icon: String,
    pub term_label: String,
    pub term_lang: String,
    pub meaning_label: String,
    pub meaning_lang: String,
    pub chars: Vec<String>,
    pub has_paradigm: bool,
    /// Listening games (Numbers by ear) exist for language courses only.
    pub has_numbers: bool,
    /// The date game needs weekday and month names (`calendar` in the manifest).
    pub has_dates: bool,
    /// Listening practice: a language course with `listening` sentences in some lesson.
    pub has_listening: bool,
    /// The language words are read aloud in when tapped (`terms.lang` of a
    /// language course; empty otherwise, which turns tap-to-hear off).
    pub say_lang: String,
    pub paradigm_label: String,
    pub item_label: String,
    pub praise: String,
    /// The app's icons (streak, XP, the modes…).
    pub icons: crate::content::Icons,
}

impl CourseView {
    pub fn of(c: &crate::content::ContentStore) -> CourseView {
        let m = c.manifest();
        CourseView {
            title: m.title.clone(),
            icon: m.icon().to_string(),
            term_label: m.terms.label.clone(),
            term_lang: m.terms.lang.clone(),
            meaning_label: m.meanings.label.clone(),
            meaning_lang: m.meanings.lang.clone(),
            chars: m.chars.clone(),
            has_paradigm: c.has_conjugations(),
            has_numbers: m.is_language(),
            has_dates: m.calendar.is_some(),
            has_listening: c.has_listening(),
            say_lang: if m.is_language() { m.terms.lang.clone() } else { String::new() },
            paradigm_label: m.paradigm.as_ref().map(|p| p.label.clone()).unwrap_or_default(),
            item_label: m.paradigm.as_ref().map(|p| p.item_label.clone()).unwrap_or_default(),
            praise: c.praise().into_iter().next().unwrap_or_else(|| crate::t("study.correct")),
            icons: crate::content::Icons::default(),
        }
    }
}

/// Wraps an askama template so handlers can return it directly.
pub struct Page<T>(pub T);

impl<T: Template> IntoResponse for Page<T> {
    fn into_response(self) -> Response {
        match self.0.render() {
            Ok(html) => Html(html).into_response(),
            Err(e) => AppError::from(e).into_response(),
        }
    }
}

/// FNV-1a over bytes; plenty for cache validation.
fn fnv1a(parts: impl IntoIterator<Item = u8>) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in parts {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

static ASSET_VERSION: OnceLock<String> = OnceLock::new();

/// A short fingerprint of every embedded static file, computed once per
/// process. It goes into asset URLs (`?v=…`) so a new build is never served
/// from a browser's cache of the old one.
pub fn asset_version() -> &'static str {
    ASSET_VERSION.get_or_init(|| {
        let mut names: Vec<_> = Assets::iter().collect();
        names.sort();
        let mut h: u64 = 0;
        for name in names {
            if let Some(f) = Assets::get(&name) {
                h ^= fnv1a(name.bytes().chain(f.data.iter().copied()));
                h = h.rotate_left(17);
            }
        }
        format!("{h:016x}")[..10].to_string()
    })
}

/// Versioned URL for an embedded asset, e.g. `/spanish/static/style.css?v=3f9a…`.
pub fn asset_url(name: &str) -> String {
    format!("{}/static/{}?v={}", crate::base_path(), name, asset_version())
}

/// GET /pictures/{name}.svg -> a picture file of the active course. Served
/// with a CSP that forbids scripts, so an SVG opened directly cannot run
/// anything even in a course someone else wrote.
async fn course_picture(p: crate::auth::Project, Path(path): Path<String>, headers: HeaderMap) -> Response {
    let full = format!("{}{path}", crate::content::PICTURES_DIR);
    let Some(body) = p.course.asset(&full) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let etag = format!("\"{:016x}\"", fnv1a(body.bytes()));
    if headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) == Some(etag.as_str()) {
        return StatusCode::NOT_MODIFIED.into_response();
    }
    (
        [
            (header::CONTENT_TYPE, HeaderValue::from_static("image/svg+xml")),
            (header::CACHE_CONTROL, HeaderValue::from_static("private, no-cache")),
            (header::ETAG, HeaderValue::from_str(&etag).expect("hex etag")),
            (header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("default-src 'none'; style-src 'unsafe-inline'; img-src data:")),
            (header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")),
        ],
        body.to_string(),
    )
        .into_response()
}

/// An SVG file of the app's data (a shop badge), with an
/// ETag and a policy that lets it run nothing.
pub(crate) fn svg_response(headers: &HeaderMap, body: &str, cache: &'static str) -> Response {
    let etag = format!("\"{:016x}\"", fnv1a(body.bytes()));
    if headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) == Some(etag.as_str()) {
        return StatusCode::NOT_MODIFIED.into_response();
    }
    (
        [
            (header::CONTENT_TYPE, HeaderValue::from_static("image/svg+xml")),
            (header::CACHE_CONTROL, HeaderValue::from_static(cache)),
            (header::ETAG, HeaderValue::from_str(&etag).expect("hex etag")),
            (header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("default-src 'none'; style-src 'unsafe-inline'; img-src data:")),
            (header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")),
        ],
        body.to_string(),
    )
        .into_response()
}

/// `/healthz`: "ok", then what the server is (`ok skrongle mini 0.1.0`,
/// #641). Health checks look at the status or the leading "ok" only.
async fn healthz() -> String {
    format!("ok {}", crate::edition(crate::is_mini()))
}

/// The web app manifest (#345): lets a phone install the site as an app
/// that opens full-screen at the home page. Built per request so the paths
/// carry the base path and the asset version. There is no service worker:
/// nothing is cached for offline use, every page still comes from the server.
fn web_manifest_json() -> serde_json::Value {
    let base = crate::base_path();
    let icon = |name: &str, size: &str| serde_json::json!({ "src": asset_url(name), "sizes": size, "type": "image/png" });
    serde_json::json!({
        "name": crate::APP_NAME,
        "short_name": crate::APP_NAME,
        "id": format!("{base}/"),
        "start_url": format!("{base}/"),
        "scope": format!("{base}/"),
        "display": "standalone",
        "background_color": "#fbfaf7",
        "theme_color": "#c0392b",
        "icons": [icon("icon-192.png", "192x192"), icon("icon-512.png", "512x512")],
    })
}

async fn web_manifest() -> Response {
    (
        [
            (header::CONTENT_TYPE, HeaderValue::from_static("application/manifest+json")),
            (header::CACHE_CONTROL, HeaderValue::from_static("no-cache")),
        ],
        web_manifest_json().to_string(),
    )
        .into_response()
}

async fn static_asset(
    Path(path): Path<String>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    let Some(file) = Assets::get(&path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mime = match path.rsplit('.').next() {
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "application/javascript; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    };
    // ETag from the content itself, so a changed file always revalidates.
    let etag = format!("\"{:016x}\"", fnv1a(file.data.iter().copied()));
    let etag_value = HeaderValue::from_str(&etag).expect("etag header");
    if headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) == Some(etag.as_str()) {
        return (StatusCode::NOT_MODIFIED, [(header::ETAG, etag_value)]).into_response();
    }
    // Long-lived caching only for URLs carrying the current version (they change
    // with every build); everything else revalidates. Debug builds serve files
    // straight from disk, so they always revalidate too.
    let versioned = query.get("v").map(String::as_str) == Some(asset_version());
    let cache = if versioned && !cfg!(debug_assertions) {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    (
        [
            (header::CONTENT_TYPE, HeaderValue::from_static(mime)),
            (header::CACHE_CONTROL, HeaderValue::from_static(cache)),
            (header::ETAG, etag_value),
        ],
        file.data.into_owned(),
    )
        .into_response()
}

/// Human-friendly relative time ("3 hours ago").
pub fn time_ago(then: i64, now: i64) -> String {
    let secs = (now - then).max(0);
    let (n, unit) = if secs < 60 {
        return crate::t("common.time_ago.just_now");
    } else if secs < 3600 {
        (secs / 60, "common.time_ago.minutes")
    } else if secs < 86_400 {
        (secs / 3600, "common.time_ago.hours")
    } else if secs < 30 * 86_400 {
        (secs / 86_400, "common.time_ago.days")
    } else if secs < 365 * 86_400 {
        (secs / (30 * 86_400), "common.time_ago.months")
    } else {
        (secs / (365 * 86_400), "common.time_ago.years")
    };
    crate::tn(unit, n)
}

/// JSON for an inline `<script type="application/json">` element: `<`, `>`,
/// `&`, U+2028 and U+2029 become `\u003c` etc., so no course text can close
/// the element, open an HTML comment or a nested script tag (#384). JSON.parse
/// reads the escapes back as the same characters.
pub fn script_json<T: serde::Serialize + ?Sized>(value: &T) -> Result<String, crate::error::AppError> {
    let json = serde_json::to_string(value).map_err(|e| crate::error::AppError::Internal(format!("inline json: {e}")))?;
    let mut out = String::with_capacity(json.len());
    for c in json.chars() {
        match c {
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '&' => out.push_str("\\u0026"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c => out.push(c),
        }
    }
    Ok(out)
}

/// Count one learning write (an answer, a finished round, a Guide step, a
/// reviewed word, an agent's report) for this learner against the per-user
/// limits (#381), so a script cannot append rows as fast as the database
/// takes them. Off in dev mode like every rate limit. Past them: a 429 JSON
/// error, which the pages treat like any failed save.
pub(crate) fn limit_learning_write(state: &AppState, user_id: i64) -> Result<(), AppError> {
    let (key, now) = (user_id.to_string(), crate::db::now());
    let burst = state.limits.hit(crate::security::LEARNING_WRITES_PER_USER, &key, now);
    let hour = state.limits.hit(crate::security::LEARNING_WRITES_PER_USER_HOUR, &key, now);
    if burst && hour {
        Ok(())
    } else {
        tracing::warn!("user {user_id}: over the learning write limit");
        Err(AppError::ApiTooManyRequests("too many answers in a short time; wait a moment and try again".into()))
    }
}

/// The worn motivator's line for a run that began at `since` (#568), as
/// HTML for an end screen; None if the run earned nothing. For a handler
/// whose own `Db::call` is done: one inside it calls `motivators::reward`.
/// `tz` is the learner's offset from UTC in seconds (`sprint::tz_offset_secs`).
pub(crate) async fn reward_line(
    state: &AppState,
    p: &crate::auth::Project,
    tz: i64,
    since: i64,
    lesson_id: Option<String>,
) -> Result<Option<String>, AppError> {
    let (uid, course) = (p.user.id, Arc::clone(&p.course));
    let line = state.db.call(move |c| crate::motivators::reward(c, uid, &course, tz, since, lesson_id.as_deref())).await?;
    Ok(crate::motivators::reward_html(line))
}

/// `test_state` on a server without a mail provider (#633).
#[cfg(test)]
pub(crate) fn test_state_without(mail: bool) -> Arc<AppState> {
    let state = test_state(false);
    let Ok(mut s) = Arc::try_unwrap(state) else { unreachable!("a new state has one owner") }; // panic-ok: tests only, a new state has one owner
    if !mail {
        s.mail = crate::mail::Mailer::idle_unconfigured();
    }
    Arc::new(s)
}

/// An app state for router tests: in-memory database, the built-in courses.
#[cfg(test)]
pub(crate) fn test_state(dev: bool) -> Arc<AppState> {
    let (vault, _) = crate::secrets::Vault::load(Some("ab".repeat(32)), std::path::Path::new("unused.db")).unwrap();
    Arc::new(AppState {
        db: crate::db::Db::open_in_memory().unwrap(),
        courses: Arc::new(crate::courses::Courses::load_builtin(std::path::Path::new("data")).unwrap()),
        secure_cookies: false,
        dummy_hash: "x".into(),
        games: Default::default(),
        jobs: Default::default(),
        vault,
        tutors: Default::default(),
        prices: crate::prices::PriceTable::parse(crate::prices::DEFAULT_PRICES).unwrap(), // panic-ok: tests only, the built-in defaults parse
        dev,
        limits: crate::security::RateLimits::new(dev),
        mail: crate::mail::Mailer::idle(),
        exports: crate::exports::Exports { dir: std::env::temp_dir().join(format!("skrongle-test-exports-{}", std::process::id())) },
        queue: crate::jobs::Queue::idle(),
        dev_inbox: None,
        dev_marks: Default::default(),
        dev_ai: Default::default(),
        dev_authenticator: Default::default(),
        public_url: None,
        signup: Default::default(),
        proxy_auth: None,
    })
}

/// Point every default base URL of this test process at the test
/// simulator (`simulated_provider`), as dev mode does: no unit test can reach
/// a real provider through a saved key.
#[cfg(test)]
pub(crate) fn point_defaults_at_the_simulator() {
    let root = simulator_root();
    crate::dev::simulate_at(root);
}

#[cfg(test)]
fn simulator_root() -> &'static str {
    static ROOT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    ROOT.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
            rt.block_on(async move {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                tx.send(format!("http://{}{}", listener.local_addr().unwrap(), crate::url("/dev/ai"))).unwrap();
                axum::serve(listener, router(test_state(true))).await.unwrap();
            });
        });
        rx.recv().unwrap()
    })
}

/// The dev simulator for unit tests: this app's router on a free loopback
/// port, in a thread of its own, started once per test process. The
/// provider for `kind` points at it with the dev seed's key for `kind`, as
/// dev mode's default base URLs and seeded keys do.
#[cfg(test)]
pub(crate) fn simulated_provider(kind: crate::ai::ProviderKind) -> crate::ai::Provider {
    let root = simulator_root();
    let key = crate::dev_ai::LEARNER_KEYS.iter().find(|(p, _)| *p == kind.secret()).map(|(_, k)| k.to_string()).unwrap_or_default();
    crate::ai::Provider { base_url: crate::dev::simulator_base_url(root, kind), ..kind.provider(key) }
}

#[cfg(test)]
mod tests;
