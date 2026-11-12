//! `<base>/mcp` (#168, `docs/api.md`): the MCP server's HTTP gate. One POST,
//! one JSON answer; nothing stays open. Every request, in this order:
//!
//! 1. the owner's switch (`settings::APPS_ON`) is on, else 404 as if there
//!    were no such route;
//! 2. no `Origin` header, whatever its value, else 403: a browser always
//!    sends one, so a web page cannot reach a box on the home network by DNS
//!    rebinding. No `Access-Control-*` header is ever sent;
//! 3. POST only (405 with `Allow: POST`: no stream to open), JSON only (415);
//! 4. a bearer token (`ApiUser`: never a cookie, never in the address);
//! 5. the grant's request limit (`security::MCP_CALLS_PER_GRANT`, 429);
//! 6. the body, capped while it is read at the course size limit plus a
//!    small margin (413), then nesting depth and batch size (`mcp::parse`);
//! 7. each message's scope (403 `insufficient_scope`).
//!
//! With a public address set, a 401 and a 403 name the protected-resource
//! document in `WWW-Authenticate` (`resource_metadata`, RFC 9728 §5.1), so
//! an app can find where to sign in (#169, `routes/oauth.rs`).

use axum::extract::{FromRequestParts, Request, State};
use axum::http::header::{ALLOW, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE, ORIGIN, WWW_AUTHENTICATE};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};
use std::sync::Arc;

use crate::auth::api::{ApiError, ApiUser};
use crate::mcp::{self, tools::Ctx, Item};
use crate::AppState;

/// What a request body may hold beyond the course size limit: the
/// JSON-RPC envelope and the escaping of the files' text. A course near the
/// limit is sent in more than one call (`create_course`, then
/// `put_course_files`).
pub const BODY_MARGIN: usize = 64 * 1024;

/// The body cap for these course limits.
pub fn body_cap(limits: crate::courses::Limits) -> usize {
    limits.course_bytes.saturating_add(BODY_MARGIN)
}

fn json_response(status: StatusCode, body: &Value) -> Response {
    let mut r = (status, [(CONTENT_TYPE, "application/json")], body.to_string()).into_response();
    r.headers_mut().insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    r
}

/// A refusal before any JSON-RPC message was read: `{"error": code}`.
fn refuse(status: StatusCode, code: &str) -> Response {
    json_response(status, &json!({ "error": code }))
}

/// A JSON-RPC error with no id, at an HTTP status.
fn rpc_refusal(status: StatusCode, code: i64, message: &str) -> Response {
    json_response(status, &mcp::error(&Value::Null, code, message))
}

/// `Content-Type: application/json`, with or without parameters.
fn is_json(headers: &HeaderMap) -> bool {
    headers
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .is_some_and(|essence| essence.trim().eq_ignore_ascii_case("application/json"))
}

/// `e`'s answer, its `WWW-Authenticate` challenge naming the
/// protected-resource document when there is a public address (#169).
fn challenge(state: &AppState, e: ApiError) -> Response {
    let mut r = e.into_response();
    let Some(url) = crate::routes::oauth::resource_metadata_url(state) else { return r };
    let Some(v) = r.headers().get(WWW_AUTHENTICATE).and_then(|v| v.to_str().ok()) else { return r };
    let v = if v == "Bearer" { format!("Bearer resource_metadata=\"{url}\"") } else { format!("{v}, resource_metadata=\"{url}\"") };
    if let Ok(v) = HeaderValue::from_str(&v) {
        r.headers_mut().insert(WWW_AUTHENTICATE, v);
    }
    r
}

/// The request's token still works for the same grant: the switch, the
/// grant, its expiry and the account's permission asked again, as
/// `ApiUser` asks them. A failed lookup is not counted as a bad token.
async fn still_works(state: &AppState, headers: &HeaderMap, grant_id: i64) -> bool {
    let token = crate::auth::api::bearer(headers).unwrap_or_default();
    let now = crate::db::now();
    matches!(
        state.db.call(move |c| crate::auth::api::user_for_token(c, &token, now)).await,
        Ok(crate::auth::api::Lookup::Found(u)) if u.grant_id == grant_id
    )
}

/// Every method on `/mcp`.
pub async fn endpoint(State(state): State<Arc<AppState>>, req: Request) -> Response {
    let (mut parts, body) = req.into_parts();
    match state.db.call(|c| crate::settings::apps_on(c)).await {
        Ok(true) => {}
        Ok(false) => return ApiError::Off.into_response(),
        Err(e) => return ApiError::Internal(e).into_response(),
    }
    if parts.headers.contains_key(ORIGIN) {
        return refuse(StatusCode::FORBIDDEN, "origin_not_allowed");
    }
    if parts.method != Method::POST {
        let mut r = refuse(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed");
        r.headers_mut().insert(ALLOW, HeaderValue::from_static("POST"));
        return r;
    }
    if !is_json(&parts.headers) {
        return refuse(StatusCode::UNSUPPORTED_MEDIA_TYPE, "unsupported_media_type");
    }
    let user = match ApiUser::from_request_parts(&mut parts, &state).await {
        Ok(u) => u,
        Err(e) => return challenge(&state, e),
    };
    if let Some(v) = parts.headers.get("mcp-protocol-version").and_then(|v| v.to_str().ok()) {
        if !mcp::PROTOCOL_VERSIONS.contains(&v) {
            return refuse(StatusCode::BAD_REQUEST, "unsupported_protocol_version");
        }
    }
    if !state.limits.hit(crate::security::MCP_CALLS_PER_GRANT, &user.grant_id.to_string(), crate::db::now()) {
        return rpc_refusal(StatusCode::TOO_MANY_REQUESTS, mcp::TOO_MANY_REQUESTS, "Too many requests from this connection. Wait a few minutes.");
    }
    let cap = body_cap(state.courses.limits());
    let too_big = || rpc_refusal(StatusCode::PAYLOAD_TOO_LARGE, mcp::INVALID_REQUEST, "The request is larger than a course may be.");
    let declared = parts.headers.get(CONTENT_LENGTH).and_then(|v| v.to_str().ok()?.parse::<usize>().ok());
    if declared.is_some_and(|n| n > cap) {
        return too_big();
    }
    let Ok(bytes) = axum::body::to_bytes(body, cap).await else { return too_big() };
    let incoming = match mcp::parse(&bytes) {
        Ok(m) => m,
        Err(e) => return json_response(StatusCode::BAD_REQUEST, &e),
    };
    drop(bytes);
    // Every scope first: a batch runs whole or not at all.
    for item in &incoming.items {
        if let Item::Request(m) = item {
            if let Some(scope) = mcp::needed_scope(m) {
                if let Err(e) = user.require(scope) {
                    return challenge(&state, e);
                }
            }
        }
    }
    let ctx = Ctx { state: &state, user: &user };
    let mut answers = Vec::new();
    let mut ran = false;
    for item in incoming.items {
        match item {
            // A batch's later messages ask again (#170): a switch turned off,
            // a grant revoked or a permission taken away while the earlier
            // ones ran stops the rest.
            Item::Request(m) if ran && !still_works(&state, &parts.headers, user.grant_id).await => {
                answers.push(mcp::error(&m.id, mcp::CONNECTION_ENDED, "This connection stopped working while the batch ran."));
            }
            Item::Request(m) => {
                ran = true;
                answers.push(mcp::answer(&ctx, m).await);
            }
            Item::Invalid(e) => answers.push(e),
            Item::Quiet => {}
        }
    }
    match (incoming.batch, answers.len()) {
        (_, 0) => {
            let mut r = StatusCode::ACCEPTED.into_response();
            r.headers_mut().insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
            r
        }
        (true, _) => json_response(StatusCode::OK, &Value::Array(answers)),
        (false, _) => {
            let one = answers.pop().unwrap_or(Value::Null);
            let status = if one.get("error").is_some_and(|e| e.get("code").and_then(Value::as_i64) == Some(mcp::INVALID_REQUEST)) {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::OK
            };
            json_response(status, &one)
        }
    }
}
