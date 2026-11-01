//! `/dev/ai/*`: the dev AI provider (`crate::dev_ai`), mounted only in dev mode.
//!
//! Two halves. The wire endpoints, which the app's own model clients call
//! with an API key like any provider (no cookie, so they are not behind the
//! developer check; the key decides): `POST /dev/ai/anthropic/v1/messages`,
//! `POST /dev/ai/openai/v1/chat/completions`,
//! `POST /dev/ai/google/v1beta/openai/chat/completions`, and the matching
//! `GET …/models`. And the dev mailbox's AI section (no sign-in needed):
//! `/dev/ai` (the log, the two key profiles' limits and injected failures,
//! Manual replies with the held requests, the fixtures), `/dev/ai/calls/{id}` (one call's bodies) and
//! `/dev/ai/fixtures/{name}` (edit a fixture file in memory, save it, reset it).

use askama::Template;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::{Form, Json};
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;

use super::Page;
use crate::auth::{CurrentUser, MaybeUser};
use crate::dev_ai::{self, Entry, Fault, Format, Injection, Measure, Plan, Who, WireRequest};
use crate::error::AppError;
use crate::AppState;

// ---------- the wire endpoints ----------

fn header<'a>(headers: &'a HeaderMap, name: &str) -> &'a str {
    headers.get(name).and_then(|v| v.to_str().ok()).unwrap_or("")
}

/// The key a request carries: `x-api-key` (Anthropic) or `Authorization: Bearer` (OpenAI, Gemini).
fn api_key(format: Format, headers: &HeaderMap) -> String {
    match format {
        Format::Anthropic => header(headers, "x-api-key").to_string(),
        _ => header(headers, "authorization").strip_prefix("Bearer ").unwrap_or("").trim().to_string(),
    }
}

fn wire_json(format: Format, status: u16, body: serde_json::Value, id: i64, retry_after: bool) -> Response {
    let mut res = (StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR), Json(body)).into_response();
    let rid = if format == Format::Anthropic { format!("req_dev_{id}") } else { format!("req-dev-{id}") };
    let name = if format == Format::Anthropic { "request-id" } else { "x-request-id" };
    if let Ok(v) = rid.parse() {
        res.headers_mut().insert(name, v);
    }
    if retry_after {
        res.headers_mut().insert("retry-after", axum::http::HeaderValue::from_static("30"));
    }
    res
}

/// An error that comes before the request is understood (no key, a bad body): logged, answered.
fn refuse(state: &AppState, format: Format, headers: &HeaderMap, body: &[u8], status: u16, kind: &str, message: &str) -> Response {
    let key = api_key(format, headers);
    let id = state.dev_ai.begin(Entry {
        id: 0,
        at: crate::db::now(),
        who: dev_ai::who(&key).map(|w| w.id()).unwrap_or("unknown").to_string(),
        key_hint: dev_ai::key_hint(&key),
        format,
        purpose: dev_ai::Meta::parse(header(headers, crate::ai::CALL_HEADER)).purpose,
        model: String::new(),
        fixture: String::new(),
        tokens_in: 0,
        tokens_out: 0,
        dollars: 0.0,
        status,
        outcome: "refused".into(),
        error: message.to_string(),
        request: String::from_utf8_lossy(body).into_owned(),
        reply: String::new(),
    });
    let reply = dev_ai::error_body(format, kind, message, id);
    state.dev_ai.finish(id, |e| e.reply = serde_json::to_string_pretty(&reply).unwrap_or_default());
    wire_json(format, status, reply, id, false)
}

async fn serve(state: &AppState, format: Format, headers: &HeaderMap, body: &[u8]) -> Response {
    if format == Format::Anthropic && header(headers, "anthropic-version").is_empty() {
        return refuse(state, format, headers, body, 400, "invalid_request_error", "anthropic-version: header is required");
    }
    let key = api_key(format, headers);
    let Some(who) = dev_ai::who(&key) else {
        let (kind, message) = if format == Format::Anthropic {
            ("authentication_error", "invalid x-api-key")
        } else {
            ("invalid_request_error", "Incorrect API key provided. You can find your API key at https://platform.openai.com/account/api-keys.")
        };
        return refuse(state, format, headers, body, 401, kind, message);
    };
    let (model, system, messages) = match dev_ai::parse_body(format, body) {
        Ok(p) => p,
        Err(e) => return refuse(state, format, headers, body, 400, "invalid_request_error", &e),
    };
    let req = WireRequest {
        format,
        who,
        key: key.clone(),
        model,
        system,
        messages,
        meta: dev_ai::Meta::parse(header(headers, crate::ai::CALL_HEADER)),
        body: serde_json::from_slice::<serde_json::Value>(body).ok().and_then(|v| serde_json::to_string_pretty(&v).ok()).unwrap_or_default(),
    };
    let now = crate::db::now();
    let mut plan = state.dev_ai.plan(&req, &crate::dev::fixtures_dir(), now);
    let id = state.dev_ai.begin(Entry {
        id: 0,
        at: now,
        who: who.id().into(),
        key_hint: dev_ai::key_hint(&key),
        format,
        purpose: req.purpose(),
        model: req.model.clone(),
        fixture: String::new(),
        tokens_in: 0,
        tokens_out: 0,
        dollars: 0.0,
        status: 0,
        outcome: "answering".into(),
        error: String::new(),
        request: req.body.clone(),
        reply: String::new(),
    });
    let mut hung = None;
    if let Plan::Hang { secs, then } = plan {
        state.dev_ai.finish(id, |e| e.outcome = format!("hanging {secs} s"));
        tracing::info!("dev-ai: call #{id} hangs {secs} s");
        tokio::time::sleep(Duration::from_secs(secs)).await;
        hung = Some(secs);
        plan = *then;
    }
    if plan == Plan::Hold {
        plan = held(state, &req, id).await;
    }
    let after_hang = |s: String| match hung {
        Some(h) => format!("hung {h} s, then {s}"),
        None => s,
    };
    match plan {
        Plan::Reply { text, delay, fixture } => {
            tokio::time::sleep(delay).await;
            let (tin, tout) = (req.tokens_in(), dev_ai::tokens(&text));
            let dollars = state.dev_ai.charge(who, &req.model, tin, tout);
            let reply = dev_ai::success_body(format, id, &req.model, &text, tin, tout, crate::db::now());
            let pretty = serde_json::to_string_pretty(&reply).unwrap_or_default();
            state.dev_ai.finish(id, |e| {
                e.status = 200;
                e.outcome = after_hang("reply".into());
                e.fixture = fixture;
                e.tokens_in = tin;
                e.tokens_out = tout;
                e.dollars = dollars;
                e.reply = pretty;
            });
            wire_json(format, 200, reply, id, false)
        }
        Plan::Fail { status, kind, message, delay, why } => {
            tokio::time::sleep(delay).await;
            let reply = dev_ai::error_body(format, &kind, &message, id);
            let pretty = serde_json::to_string_pretty(&reply).unwrap_or_default();
            state.dev_ai.finish(id, |e| {
                e.status = status;
                e.outcome = after_hang(why);
                e.error = message.clone();
                e.reply = pretty;
            });
            wire_json(format, status, reply, id, status == 429 || status == 529 || status == 503)
        }
        Plan::Malformed => {
            let text = format!("{{\"id\": \"msg_dev_{id}\", \"content\": [{{\"type\": \"text\", \"text\": \"half a rep");
            state.dev_ai.finish(id, |e| {
                e.status = 200;
                e.outcome = after_hang("injected: malformed JSON body".into());
                e.reply = text.clone();
            });
            ([(axum::http::header::CONTENT_TYPE, "application/json")], text).into_response()
        }
        Plan::Hang { .. } | Plan::Hold => {
            state.dev_ai.finish(id, |e| e.status = 500);
            wire_json(format, 500, dev_ai::error_body(format, "api_error", "dev simulator: nested hang or hold", id), id, false)
        }
    }
}

/// Manual replies: write the request as `<seq>-<purpose>.request.json` in
/// the capture directory and wait for `<seq>-<purpose>.response.txt` (typed
/// in the page's Manual replies section, or written by a script). A reply starting with `ERROR:`
/// answers with that error, as a fixture's does.
async fn held(state: &AppState, req: &WireRequest, id: i64) -> Plan {
    let dir = crate::dev::capture_dir();
    let fail = |message: String, why: &str| Plan::Fail { status: 500, kind: "api_error".into(), message, delay: Duration::ZERO, why: why.into() };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return fail(format!("dev simulator: cannot create {}: {e}", dir.display()), "hold");
    }
    let n = state.dev_ai.next_hold_seq(&dir);
    let purpose = req.purpose();
    let stem = format!("{n:04}-{}", if purpose.is_empty() { "unknown" } else { purpose.as_str() });
    let response = dir.join(format!("{stem}.response.txt"));
    let request = serde_json::json!({
        "seq": n,
        "purpose": purpose,
        "key": req.meta.key,
        "job_id": req.meta.job,
        "provider": req.format.id(),
        "model": req.model,
        "system": req.system,
        "messages": req.messages.iter().map(|(role, content)| serde_json::json!({ "role": role, "content": content })).collect::<Vec<_>>(),
        "respond_in": response.file_name().map(|f| f.to_string_lossy().into_owned()),
        "created_at": crate::db::now(),
    });
    let path = dir.join(format!("{stem}.request.json"));
    let text = serde_json::to_string_pretty(&request).unwrap_or_default();
    if let Err(e) = crate::dev::write_atomic(&path, &text) {
        return fail(format!("dev simulator: cannot write {}: {e}", path.display()), "hold");
    }
    state.dev_ai.finish(id, |e| e.outcome = format!("held: answer {stem} on /dev/ai"));
    tracing::info!("dev-ai: call #{id} held: waiting for {} (request in {})", response.display(), path.display());
    let timeout = crate::dev::capture_timeout();
    let Some(reply) = crate::dev::wait_for(&response, timeout).await else {
        let message = format!("dev simulator: no reply to {} within {} s", path.display(), timeout.as_secs());
        return Plan::Fail { status: 504, kind: "api_error".into(), message, delay: Duration::ZERO, why: "hold: no reply".into() };
    };
    let why = format!("answered by hand: {stem}");
    match reply.strip_prefix("ERROR:") {
        Some(msg) => dev_ai::fixture_error(req.format, msg, Duration::ZERO, why),
        None => Plan::Reply { text: reply, delay: Duration::ZERO, fixture: why },
    }
}

/// POST /dev/ai/anthropic/v1/messages: the Anthropic Messages API.
pub async fn anthropic_messages(State(state): State<Arc<AppState>>, headers: HeaderMap, body: Bytes) -> Response {
    serve(&state, Format::Anthropic, &headers, &body).await
}

/// POST /dev/ai/openai/v1/chat/completions: OpenAI chat completions.
pub async fn openai_chat(State(state): State<Arc<AppState>>, headers: HeaderMap, body: Bytes) -> Response {
    serve(&state, Format::OpenAi, &headers, &body).await
}

/// POST /dev/ai/google/v1beta/openai/chat/completions: Gemini's OpenAI-compatible endpoint.
pub async fn google_chat(State(state): State<Arc<AppState>>, headers: HeaderMap, body: Bytes) -> Response {
    serve(&state, Format::Google, &headers, &body).await
}

/// POST /dev/ai/jev/v1/systemone: a stand-in for Jev (TypeSafe System One)
/// for the content check (#1036), answering from its fixture
/// (`content_check::jev::stand_in`). Key: `dev-jev`.
pub async fn jev_systemone(headers: HeaderMap, body: Bytes) -> Response {
    use crate::content_check::jev;
    if header(&headers, "authorization") != format!("Bearer {}", jev::STAND_IN_KEY) {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": {"message": "invalid API key"}}))).into_response();
    }
    let Ok(req) = serde_json::from_slice::<serde_json::Value>(&body) else {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": {"message": "the body is not JSON"}}))).into_response();
    };
    let fixture: serde_json::Value = std::fs::read_to_string(jev::STAND_IN_FIXTURE).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    (StatusCode::OK, Json(jev::stand_in(&fixture, &req))).into_response()
}

async fn models(format: Format, headers: HeaderMap) -> Response {
    if dev_ai::who(&api_key(format, &headers)).is_none() {
        let kind = if format == Format::Anthropic { "authentication_error" } else { "invalid_request_error" };
        return wire_json(format, 401, dev_ai::error_body(format, kind, "invalid x-api-key", 0), 0, false);
    }
    wire_json(format, 200, dev_ai::models_body(format), 0, false)
}

/// GET /dev/ai/anthropic/v1/models
pub async fn anthropic_models(headers: HeaderMap) -> Response {
    models(Format::Anthropic, headers).await
}

/// GET /dev/ai/openai/v1/models
pub async fn openai_models(headers: HeaderMap) -> Response {
    models(Format::OpenAi, headers).await
}

/// GET /dev/ai/google/v1beta/openai/models
pub async fn google_models(headers: HeaderMap) -> Response {
    models(Format::Google, headers).await
}

// ---------- the dev mailbox's AI section ----------

/// One row of the call log (`dev_ai_rows.html`).
pub struct CallRow {
    pub id: i64,
    pub href: String,
    pub ago: String,
    pub who: String,
    pub who_label: String,
    pub key_hint: String,
    pub format: &'static str,
    pub purpose: String,
    pub model: String,
    pub fixture: String,
    pub tokens: String,
    pub status: u16,
    /// `ok`, `pending` or `error`, for the badge's colour.
    pub state: &'static str,
    pub outcome: String,
}

fn call_row(e: &Entry, now: i64) -> CallRow {
    CallRow {
        id: e.id,
        href: crate::url(&format!("/dev/ai/calls/{}", e.id)),
        ago: super::time_ago(e.at, now),
        who: e.who.clone(),
        who_label: Who::parse(&e.who).map(|w| w.label()).unwrap_or("Unknown key").to_string(),
        key_hint: e.key_hint.clone(),
        format: e.format.id(),
        purpose: if e.purpose.is_empty() { "—".into() } else { e.purpose.clone() },
        model: e.model.clone(),
        fixture: e.fixture.clone(),
        tokens: if e.status == 200 { format!("{} / {}", e.tokens_in, e.tokens_out) } else { "—".into() },
        status: e.status,
        state: match e.status {
            0 => "pending",
            200 => "ok",
            _ => "error",
        },
        outcome: if e.error.is_empty() { e.outcome.clone() } else { format!("{}: {}", e.outcome, e.error) },
    }
}

#[derive(Template)]
#[template(path = "dev_ai_rows.html")]
pub struct CallRows {
    pub rows: Vec<CallRow>,
}

/// A held request (Manual replies), as a collapsible row at the bottom of the page.
pub struct HeldView {
    pub stem: String,
    pub purpose: String,
    pub key: String,
    pub model: String,
    pub system: String,
    pub messages: Vec<(String, String)>,
    pub response: Option<String>,
    pub pending: bool,
    /// `POST /dev/ai/held/{stem}`.
    pub action: String,
}

#[derive(Template)]
#[template(path = "dev_ai_held.html")]
pub struct HeldRows {
    pub items: Vec<HeldView>,
}

impl HeldRows {
    pub fn pending(&self) -> usize {
        self.items.iter().filter(|i| i.pending).count()
    }
}

/// The held requests on disk, newest first.
fn held_rows() -> HeldRows {
    let s = |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or_default().to_string();
    let items = crate::dev::list_requests(&crate::dev::capture_dir())
        .into_iter()
        .rev()
        .map(|c| HeldView {
            purpose: s(&c.request, "purpose"),
            key: s(&c.request, "key"),
            model: s(&c.request, "model"),
            system: s(&c.request, "system"),
            messages: c
                .request
                .get("messages")
                .and_then(|m| m.as_array())
                .map(|a| a.iter().map(|m| (s(m, "role"), s(m, "content"))).collect())
                .unwrap_or_default(),
            action: crate::url(&format!("/dev/ai/held/{}", c.stem)),
            pending: c.response.is_none(),
            stem: c.stem,
            response: c.response,
        })
        .collect();
    HeldRows { items }
}

/// The held requests for scripts (`/dev/ai?format=json`), newest first.
fn held_json() -> Vec<serde_json::Value> {
    crate::dev::list_requests(&crate::dev::capture_dir())
        .iter()
        .rev()
        .map(|c| serde_json::json!({ "stem": c.stem, "pending": c.response.is_none(), "request": c.request, "response": c.response }))
        .collect()
}

/// One key profile's card.
pub struct ProfileView {
    pub id: &'static str,
    pub label: &'static str,
    pub action: String,
    pub keys: Vec<KeyView>,
    pub usage: String,
    pub detail: String,
    /// The limit as the form shows it; empty: none.
    pub limit: String,
    pub measure: &'static str,
    pub over: bool,
    pub exhausted: bool,
    pub inject: Option<String>,
}

/// A key on a profile's card: a copy button in its provider's colours.
pub struct KeyView {
    pub key: &'static str,
    /// `anthropic`, `openai` or `google` (the button's colours).
    pub provider: &'static str,
    pub label: String,
}

fn key_views(who: Who) -> Vec<KeyView> {
    who.keys()
        .into_iter()
        .map(|key| {
            let p = crate::dev_ai::key_provider(key);
            KeyView { key, provider: p.id(), label: p.label().to_string() }
        })
        .collect()
}

fn profile_view(state: &AppState, who: Who) -> ProfileView {
    let p = state.dev_ai.profile(who);
    let u = &p.usage;
    ProfileView {
        id: who.id(),
        label: who.label(),
        action: crate::url(&format!("/dev/ai/profiles/{}", who.id())),
        keys: key_views(who),
        usage: p.usage_line(),
        detail: format!("{} · {} tokens in · {} out · ${:.4}", Measure::Requests.show(u.requests as f64), crate::ai::grouped(u.tokens_in as usize), crate::ai::grouped(u.tokens_out as usize), u.dollars),
        limit: p.limit.map(|l| if l.fract() == 0.0 { format!("{}", l as u64) } else { format!("{l}") }).unwrap_or_default(),
        measure: p.measure.id(),
        over: p.over(),
        exhausted: p.exhausted,
        inject: p.inject.map(|i| i.describe()),
    }
}

/// One fixture file on the page.
pub struct FixtureView {
    pub name: String,
    pub href: String,
    pub keys: Vec<String>,
    pub edited: bool,
    pub on_disk: bool,
    pub error: String,
}

fn fixture_path(name: &str) -> std::path::PathBuf {
    crate::dev::fixtures_dir().join(format!("{name}.json"))
}

/// The reply keys of a fixture file's text, or why it does not parse.
fn reply_keys(text: &str) -> Result<Vec<String>, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("not JSON: {e}"))?;
    let replies = v.get("replies").and_then(|r| r.as_object()).ok_or("no \"replies\" object")?;
    Ok(replies.keys().cloned().collect())
}

fn fixtures() -> Vec<FixtureView> {
    let dir = crate::dev::fixtures_dir();
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .map(|rd| rd.filter_map(|e| e.ok()).filter_map(|e| e.file_name().to_string_lossy().strip_suffix(".json").map(str::to_string)).collect())
        .unwrap_or_default();
    for p in crate::dev::fixture_edit_paths() {
        if p.parent() == Some(dir.as_path()) {
            if let Some(n) = p.file_name().and_then(|f| f.to_str()).and_then(|f| f.strip_suffix(".json")) {
                names.push(n.to_string());
            }
        }
    }
    names.sort();
    names.dedup();
    names
        .into_iter()
        .filter(|n| crate::dev::valid_stem(n))
        .map(|name| {
            let path = fixture_path(&name);
            let (keys, error) = match crate::dev::fixture_text(&path).map_err(|e| e.to_string()).and_then(|t| reply_keys(&t)) {
                Ok(k) => (k, String::new()),
                Err(e) => (Vec::new(), e),
            };
            FixtureView { href: crate::url(&format!("/dev/ai/fixtures/{name}")), keys, edited: crate::dev::fixture_edited(&path), on_disk: path.is_file(), error, name }
        })
        .collect()
}

#[derive(Template)]
#[template(path = "dev_ai.html")]
pub struct AiTemplate {
    pub user: Option<CurrentUser>,
    pub mode: String,
    /// (format, base URL) a custom base URL can name.
    pub bases: Vec<(&'static str, String)>,
    pub timeout: String,
    pub profiles: Vec<ProfileView>,
    pub measures: Vec<(&'static str, &'static str)>,
    pub faults: Vec<(&'static str, &'static str)>,
    pub log: CallRows,
    pub fixtures: Vec<FixtureView>,
    /// Manual replies is on: requests wait in the section at the bottom.
    pub holding: bool,
    /// The held requests, newest first.
    pub held: HeldRows,
    /// How many of them still wait for a reply.
    pub pending: usize,
    /// Where they are written (`SKRONGLE_LLM_DIR`).
    pub capture_dir: String,
    pub default_hang: u64,
    /// A message after an action (`?notice=`).
    pub notice: String,
}

impl AiTemplate {
    /// The section, for `dev_box_kinds.html`.
    pub fn section(&self) -> &'static str {
        "ai"
    }
}

fn mode() -> String {
    if crate::dev::simulating() {
        return "dev mode: the app's model calls come here by default".into();
    }
    "SKRONGLE_LLM_MODE=live: the app's calls go to the real providers unless a base URL points here".into()
}

/// `http://<host>/dev/ai`, as the request reached this server.
fn root(headers: &HeaderMap) -> String {
    let host = header(headers, "host");
    format!("http://{}{}", if host.is_empty() { "127.0.0.1" } else { host }, crate::url("/dev/ai"))
}

fn bases(headers: &HeaderMap) -> Vec<(&'static str, String)> {
    use crate::ai::ProviderKind;
    let root = root(headers);
    [ProviderKind::Anthropic, ProviderKind::OpenAi, ProviderKind::Google].into_iter().map(|k| (k.id(), crate::dev::simulator_base_url(&root, k))).collect()
}

fn client_timeout() -> Option<u64> {
    crate::env("AI_TIMEOUT_SECS").and_then(|v| v.parse().ok())
}

fn entry_json(e: &Entry) -> serde_json::Value {
    serde_json::json!({
        "id": e.id, "at": e.at, "who": e.who, "key": e.key_hint, "format": e.format.id(), "purpose": e.purpose,
        "model": e.model, "fixture": e.fixture, "tokens_in": e.tokens_in, "tokens_out": e.tokens_out,
        "dollars": e.dollars, "status": e.status, "outcome": e.outcome, "error": e.error,
    })
}

fn profile_json(state: &AppState, who: Who) -> serde_json::Value {
    let p = state.dev_ai.profile(who);
    serde_json::json!({
        "limit": p.limit, "measure": p.measure.id(), "used": p.used(), "over": p.over(), "exhausted": p.exhausted,
        "usage": { "requests": p.usage.requests, "tokens_in": p.usage.tokens_in, "tokens_out": p.usage.tokens_out, "dollars": p.usage.dollars },
        "inject": p.inject.map(|i| i.describe()), "keys": who.keys(),
    })
}

#[derive(Deserialize)]
pub struct AiParams {
    pub format: Option<String>,
    #[serde(default)]
    pub notice: String,
}

fn wants_html(headers: &HeaderMap, format: Option<&str>) -> bool {
    match format {
        Some("json") => false,
        Some("html") => true,
        _ => header(headers, "accept").contains("text/html"),
    }
}

/// GET /dev/ai: the AI section. JSON for scripts (`?format=json`): the
/// mode, the base URLs, both profiles, the log, the fixtures and the held
/// requests.
pub async fn page(State(state): State<Arc<AppState>>, MaybeUser(user): MaybeUser, headers: HeaderMap, Query(q): Query<AiParams>) -> Response {
    let now = crate::db::now();
    if !wants_html(&headers, q.format.as_deref()) {
        return Json(serde_json::json!({
            "mode": mode(),
            "simulating": crate::dev::simulating(),
            "holding": state.dev_ai.holding(),
            "base_urls": bases(&headers).into_iter().collect::<std::collections::BTreeMap<_, _>>(),
            "client_timeout_secs": client_timeout(),
            "profiles": { "platform": profile_json(&state, Who::Platform), "learner": profile_json(&state, Who::Learner) },
            "calls": state.dev_ai.log().iter().map(entry_json).collect::<Vec<_>>(),
            "fixtures": fixtures().iter().map(|f| serde_json::json!({ "name": f.name, "keys": f.keys, "edited": f.edited, "on_disk": f.on_disk, "error": f.error })).collect::<Vec<_>>(),
            "held": held_json(),
        }))
        .into_response();
    }
    let held = held_rows();
    Page(AiTemplate {
        user,
        mode: mode(),
        bases: bases(&headers),
        timeout: match client_timeout() {
            Some(s) => format!("{s} s (SKRONGLE_AI_TIMEOUT_SECS)"),
            None => "15 min for generation, 2 min for chat (set SKRONGLE_AI_TIMEOUT_SECS to change)".into(),
        },
        profiles: Who::ALL.into_iter().map(|w| profile_view(&state, w)).collect(),
        measures: Measure::ALL.into_iter().map(|m| (m.id(), m.label())).collect(),
        faults: Fault::ALL.into_iter().map(|f| (f.id(), f.label())).collect(),
        log: CallRows { rows: state.dev_ai.log().iter().map(|e| call_row(e, now)).collect() },
        fixtures: fixtures(),
        holding: state.dev_ai.holding(),
        pending: held.pending(),
        capture_dir: crate::dev::capture_dir().display().to_string(),
        held,
        default_hang: dev_ai::DEFAULT_HANG_SECS,
        notice: q.notice,
    })
    .into_response()
}

/// GET /dev/ai/rows: the log as HTML, each profile's usage, and whether
/// Manual replies is on with its held requests as HTML, for the page's poll.
pub async fn rows(State(state): State<Arc<AppState>>) -> Result<Response, AppError> {
    let now = crate::db::now();
    let log = CallRows { rows: state.dev_ai.log().iter().map(|e| call_row(e, now)).collect() };
    let held = held_rows();
    let mut profiles = serde_json::Map::new();
    for w in Who::ALL {
        let v = profile_view(&state, w);
        profiles.insert(w.id().into(), serde_json::json!({ "usage": v.usage, "detail": v.detail, "over": v.over, "inject": v.inject }));
    }
    Ok(Json(serde_json::json!({
        "log": log.render()?,
        "count": log.rows.len(),
        "profiles": profiles,
        "holding": state.dev_ai.holding(),
        "held": { "html": held.render()?, "pending": held.pending() },
    }))
    .into_response())
}

/// Back to the page (204 for the page's own `data-live` posts).
fn back(headers: &HeaderMap, to: &str, notice: &str) -> Response {
    if headers.contains_key("x-dev-box") {
        return StatusCode::NO_CONTENT.into_response();
    }
    let url = if notice.is_empty() { crate::url(to) } else { format!("{}?notice={}", crate::url(to), crate::urlenc::urlencode(notice)) };
    Redirect::to(&url).into_response()
}

#[derive(Deserialize)]
pub struct ProfileForm {
    #[serde(rename = "do")]
    pub action: String,
    #[serde(default)]
    pub limit: String,
    #[serde(default)]
    pub measure: String,
    #[serde(default)]
    pub fault: String,
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub hang: String,
}

/// POST /dev/ai/profiles/{platform|learner} (`do=settings|reset|exhaust|lift|inject|clear-inject`).
pub async fn profile_act(State(state): State<Arc<AppState>>, headers: HeaderMap, Path(who): Path<String>, Form(f): Form<ProfileForm>) -> Result<Response, AppError> {
    let who = Who::parse(&who).ok_or(AppError::NotFound)?;
    let notice = match f.action.as_str() {
        "settings" => {
            let limit = match f.limit.trim() {
                "" => None,
                l => Some(l.parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0).ok_or_else(|| AppError::BadRequest(format!("limit {l:?}: a number of 0 or more, or empty for none")))?),
            };
            let measure = Measure::parse(&f.measure).ok_or_else(|| AppError::BadRequest(format!("measure {:?}: requests, tokens or dollars", f.measure)))?;
            state.dev_ai.update(who, |p| {
                p.limit = limit;
                p.measure = measure;
            });
            format!("{}: limit {}.", who.label(), limit.map(|l| measure.show(l)).unwrap_or_else(|| "none".into()))
        }
        "reset" => {
            state.dev_ai.update(who, |p| {
                p.usage = Default::default();
                p.exhausted = false;
            });
            format!("{}: usage reset.", who.label())
        }
        "exhaust" => {
            state.dev_ai.update(who, |p| p.exhausted = true);
            format!("{}: over the limit from now on.", who.label())
        }
        "lift" => {
            state.dev_ai.update(who, |p| p.exhausted = false);
            format!("{}: back under the limit (if the counters are).", who.label())
        }
        "inject" => {
            let fault = Fault::parse(&f.fault).ok_or_else(|| AppError::BadRequest(format!("fault {:?}: one of {}", f.fault, Fault::ALL.map(|x| x.id()).join(", "))))?;
            let hang_secs = f.hang.trim().parse::<u64>().ok().filter(|s| *s > 0).unwrap_or(dev_ai::DEFAULT_HANG_SECS).min(dev_ai::HANG_FOREVER_SECS);
            let inj = Injection { fault, sticky: f.mode == "sticky", hang_secs };
            state.dev_ai.update(who, |p| p.inject = Some(inj));
            format!("{}: {}.", who.label(), inj.describe())
        }
        "clear-inject" => {
            state.dev_ai.update(who, |p| p.inject = None);
            format!("{}: no failure injected.", who.label())
        }
        other => return Err(AppError::BadRequest(format!("do {other:?}: settings, reset, exhaust, lift, inject or clear-inject"))),
    };
    tracing::info!("dev-ai: {notice}");
    Ok(back(&headers, "/dev/ai", &notice))
}

/// POST /dev/ai/clear: empty the call log.
pub async fn clear(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    state.dev_ai.clear_log();
    back(&headers, "/dev/ai", "")
}

#[derive(Template)]
#[template(path = "dev_ai_call.html")]
pub struct CallTemplate {
    pub user: Option<CurrentUser>,
    pub row: CallRow,
    pub dollars: String,
    pub request: String,
    pub reply: String,
}

/// GET /dev/ai/calls/{id}: one call: who, what, the outcome, the request and reply bodies.
pub async fn call(State(state): State<Arc<AppState>>, MaybeUser(user): MaybeUser, headers: HeaderMap, Path(id): Path<i64>, Query(q): Query<AiParams>) -> Result<Response, AppError> {
    let e = state.dev_ai.entry(id).ok_or(AppError::NotFound)?;
    if !wants_html(&headers, q.format.as_deref()) {
        let mut v = entry_json(&e);
        v["request"] = serde_json::Value::String(e.request.clone());
        v["reply"] = serde_json::Value::String(e.reply.clone());
        return Ok(Json(v).into_response());
    }
    Ok(Page(CallTemplate { user, row: call_row(&e, crate::db::now()), dollars: format!("${:.6}", e.dollars), request: e.request.clone(), reply: e.reply.clone() }).into_response())
}

#[derive(Template)]
#[template(path = "dev_ai_fixture.html")]
pub struct FixtureTemplate {
    pub user: Option<CurrentUser>,
    pub name: String,
    pub action: String,
    pub path: String,
    pub text: String,
    pub edited: bool,
    pub on_disk: bool,
    pub keys: Vec<String>,
    pub error: String,
    pub notice: String,
}

fn fixture_name(name: &str) -> Result<String, AppError> {
    let name = name.strip_suffix(".json").unwrap_or(name);
    if crate::dev::valid_stem(name) { Ok(name.to_string()) } else { Err(AppError::BadRequest(format!("{name:?}: a fixture name is lower-case letters, digits and dashes"))) }
}

/// GET /dev/ai/fixtures/{name}: a fixture file to edit (`?format=json`: its text and state).
pub async fn fixture(MaybeUser(user): MaybeUser, headers: HeaderMap, Path(name): Path<String>, Query(q): Query<AiParams>) -> Result<Response, AppError> {
    let name = fixture_name(&name)?;
    let path = fixture_path(&name);
    let text = crate::dev::fixture_text(&path).map_err(|_| AppError::NotFound)?;
    let (keys, error) = match reply_keys(&text) {
        Ok(k) => (k, String::new()),
        Err(e) => (Vec::new(), e),
    };
    let (edited, on_disk) = (crate::dev::fixture_edited(&path), path.is_file());
    if !wants_html(&headers, q.format.as_deref()) {
        return Ok(Json(serde_json::json!({ "name": name, "text": text, "keys": keys, "edited": edited, "on_disk": on_disk, "error": error })).into_response());
    }
    Ok(Page(FixtureTemplate { user, action: crate::url(&format!("/dev/ai/fixtures/{name}")), path: path.display().to_string(), name, text, edited, on_disk, keys, error, notice: q.notice }).into_response())
}

#[derive(Deserialize)]
pub struct FixtureForm {
    #[serde(rename = "do")]
    pub action: String,
    #[serde(default)]
    pub text: String,
}

/// A fixture file's text, checked: JSON with a `replies` object. Pretty-printed.
fn checked_fixture(text: &str) -> Result<String, AppError> {
    let v: serde_json::Value = serde_json::from_str(text.trim()).map_err(|e| AppError::BadRequest(format!("The fixture is not JSON: {e}")))?;
    if !v.get("replies").is_some_and(|r| r.is_object()) {
        return Err(AppError::BadRequest("A fixture file is {\"replies\": {\"<key>\": {\"reply\": …}, \"*\": …}}: it needs a \"replies\" object.".into()));
    }
    Ok(serde_json::to_string_pretty(&v).unwrap_or_default() + "\n")
}

/// POST /dev/ai/fixtures/{name} (`do=apply` with `text`: use it for this
/// server run; `do=save`: write the current text (`text`, if sent) to the
/// file; `do=reset`: forget the edit, back to the file).
pub async fn fixture_act(headers: HeaderMap, Path(name): Path<String>, Form(f): Form<FixtureForm>) -> Result<Response, AppError> {
    let name = fixture_name(&name)?;
    let path = fixture_path(&name);
    let to = format!("/dev/ai/fixtures/{name}");
    let notice = match f.action.as_str() {
        "apply" => {
            let text = checked_fixture(&f.text)?;
            crate::dev::set_fixture_edit(&path, Some(text));
            format!("{name}.json: edited for this server run (not saved to the file).")
        }
        "save" => {
            let text = if f.text.trim().is_empty() { crate::dev::fixture_text(&path).map_err(|_| AppError::NotFound)? } else { f.text.clone() };
            let text = checked_fixture(&text)?;
            crate::dev::write_atomic(&path, &text).map_err(|e| AppError::Internal(format!("cannot write {}: {e}", path.display())))?;
            crate::dev::set_fixture_edit(&path, None);
            tracing::info!("dev-ai: saved {}", path.display());
            format!("Saved to {}.", path.display())
        }
        "reset" => {
            crate::dev::set_fixture_edit(&path, None);
            if path.is_file() { format!("{name}.json: back to the file.") } else { format!("{name}.json: dropped (it was never saved).") }
        }
        other => return Err(AppError::BadRequest(format!("do {other:?}: apply, save or reset"))),
    };
    let to = if f.action == "reset" && !path.is_file() { "/dev/ai".to_string() } else { to };
    Ok(back(&headers, &to, &notice))
}

#[derive(Deserialize)]
pub struct ScenarioForm {
    /// The fixture file (`generate-outline`): a call site's purpose.
    pub purpose: String,
    /// The reply's key: what a `[fixture:NAME]` token (or a lesson slug) picks.
    pub scenario: String,
    /// The reply: JSON (an object or array is stored as JSON) or text.
    #[serde(default)]
    pub reply: String,
    #[serde(default)]
    pub delay_ms: String,
    /// Instead of a reply: the error to play (`the API answered HTTP 429. …`).
    #[serde(default)]
    pub error: String,
}

/// POST /dev/ai/fixtures: add a named scenario (a reply under a new key) to
/// a fixture file, in memory; a purpose with no file yet gets one.
pub async fn scenario_add(headers: HeaderMap, Form(f): Form<ScenarioForm>) -> Result<Response, AppError> {
    let name = fixture_name(f.purpose.trim())?;
    let key = f.scenario.trim();
    if key.is_empty() || key.len() > 64 || key.contains(']') {
        return Err(AppError::BadRequest("Name the scenario (up to 64 characters, no ']'): a wish with [fixture:NAME] picks it.".into()));
    }
    let path = fixture_path(&name);
    let mut file: serde_json::Value = match crate::dev::fixture_text(&path) {
        Ok(t) => serde_json::from_str(&t).map_err(|e| AppError::BadRequest(format!("{name}.json is not JSON ({e}); fix it first")))?,
        Err(_) => serde_json::json!({ "replies": {} }),
    };
    let mut entry = serde_json::Map::new();
    if !f.error.trim().is_empty() {
        entry.insert("error".into(), f.error.trim().into());
    } else {
        let reply = match serde_json::from_str::<serde_json::Value>(f.reply.trim()) {
            Ok(v @ (serde_json::Value::Object(_) | serde_json::Value::Array(_))) => v,
            _ => serde_json::Value::String(f.reply.replace("\r\n", "\n")),
        };
        entry.insert("reply".into(), reply);
    }
    if let Ok(ms) = f.delay_ms.trim().parse::<u64>() {
        entry.insert("delay_ms".into(), ms.into());
    }
    let replies = file.get_mut("replies").and_then(|r| r.as_object_mut()).ok_or_else(|| AppError::BadRequest(format!("{name}.json has no \"replies\" object")))?;
    replies.insert(key.to_string(), serde_json::Value::Object(entry));
    crate::dev::set_fixture_edit(&path, Some(serde_json::to_string_pretty(&file).unwrap_or_default() + "\n"));
    Ok(back(&headers, &format!("/dev/ai/fixtures/{name}"), &format!("Added the scenario '{key}' to {name}.json (in memory): a wish with [fixture:{key}] picks it.")))
}

#[derive(Deserialize)]
pub struct HoldForm {
    /// `on` or `off`.
    #[serde(rename = "do")]
    pub act: String,
}

/// POST /dev/ai/hold (`do=on|off`): Manual replies. On, every request the
/// fixtures would answer waits for a reply typed in the page's Manual
/// replies section instead.
pub async fn hold(State(state): State<Arc<AppState>>, headers: HeaderMap, Form(f): Form<HoldForm>) -> Result<Response, AppError> {
    let on = match f.act.as_str() {
        "on" => true,
        "off" => false,
        other => return Err(AppError::BadRequest(format!("do={other}: on or off"))),
    };
    state.dev_ai.set_hold(on);
    let notice = if on { "Manual replies on: each request waits for your answer at the bottom of this page." } else { "Replies come from the fixtures again." };
    Ok(back(&headers, "/dev/ai", notice))
}

#[derive(Deserialize)]
pub struct AnswerForm {
    pub reply: String,
}

/// POST /dev/ai/held/{stem} (`reply=`): write the reply file a held request
/// is polling for. A reply starting with `ERROR:` becomes the provider's error.
pub async fn answer(Path(stem): Path<String>, headers: HeaderMap, Form(form): Form<AnswerForm>) -> Result<Response, AppError> {
    let dir = crate::dev::capture_dir();
    if !crate::dev::valid_stem(&stem) || !dir.join(format!("{stem}.request.json")).exists() {
        return Err(AppError::NotFound);
    }
    let path = dir.join(format!("{stem}.response.txt"));
    if path.exists() {
        return Err(AppError::BadRequest(format!("{stem} is already answered")));
    }
    let reply = form.reply.replace("\r\n", "\n");
    if reply.trim().is_empty() {
        return Err(AppError::BadRequest("The reply is empty.".into()));
    }
    crate::dev::write_atomic(&path, &reply).map_err(|e| AppError::Internal(e.to_string()))?;
    Ok(back(&headers, "/dev/ai", &format!("Answered {stem}.")))
}
