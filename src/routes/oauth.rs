//! The OAuth 2.1 endpoints (#169, `oauth_server.rs` has the rules): the
//! discovery documents, `/oauth/authorize` with its consent page,
//! `/oauth/token`, `/oauth/register` and `/oauth/revoke`. The wire format is
//! in `docs/api.md` "OAuth".
//!
//! Every route answers 404 while the owner's switch is off
//! (`settings::APPS_ON`) or no public address is set
//! (`SKRONGLE_PUBLIC_URL`): an issuer must be a fixed address, and every
//! address handed out here is built from that setting and the base path,
//! never from the request's `Host` or `X-Forwarded-*`.
//!
//! The consent page is a page: the session cookie, the CSRF layer, and on
//! top a same-origin check of its own. The token, registration and
//! revocation endpoints are called by programs: they never read a cookie,
//! the CSRF layer and the proxy's sign-in header leave them alone
//! (`security::csrf`, `proxy_auth::left_alone`), and they answer JSON with
//! `Cache-Control: no-store`.

use askama::Template;
use axum::extract::{FromRequest, Query, Request, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE, COOKIE, LOCATION, ORIGIN, PRAGMA, SET_COOKIE};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, Uri};
use axum::response::{IntoResponse, Redirect, Response};
use axum::Form;
use serde_json::{json, Value};
use std::fmt::Write as _;
use std::sync::Arc;

use super::profile::apps::Choice;
use super::Page;
use crate::auth::api::{Lifetime, Refused, Scope};
use crate::auth::{CurrentUser, MaybeUser};
use crate::db::now;
use crate::error::AppError;
use crate::oauth_server::{self as server, Authorize, AuthorizeError, CodeExchange, Consent, TokenError, Tokens};
use crate::security::{ClientIp, OAUTH_REGISTRATIONS_PER_IP, OAUTH_TOKEN_FAILURES_PER_IP};
use crate::AppState;

/// The cookie that carries a signed-out authorization request through the
/// sign-in (password, then two-factor) and back (`resume`).
pub const NEXT_COOKIE: &str = "oauth_next";
/// Longest authorization query accepted (and so carried in the cookie, hex
/// doubling it, under the 4 KB a cookie holds).
pub const MAX_QUERY: usize = 1800;
/// Largest token or revocation request body.
const FORM_BODY_MAX: usize = 4 * 1024;

// ---------- the issuer and the gate ----------

/// This server's address as an OAuth issuer: `SKRONGLE_PUBLIC_URL` and the
/// base path, or `None` without a public address.
pub fn issuer(state: &AppState) -> Option<String> {
    state.public_url.as_deref().map(|u| format!("{}{}", u.trim_end_matches('/'), crate::base_path()))
}

/// The protected-resource document of `/mcp`, named in its 401's
/// `WWW-Authenticate` header (`resource_metadata`, RFC 9728 §5.1).
pub fn resource_metadata_url(state: &AppState) -> Option<String> {
    issuer(state).map(|i| format!("{i}/.well-known/oauth-protected-resource/mcp"))
}

/// The issuer, or 404: no public address, or the owner's switch is off.
async fn gate(state: &AppState) -> Result<String, AppError> {
    let Some(issuer) = issuer(state) else { return Err(AppError::NotFound) };
    if !state.db.call(|c| crate::settings::apps_on(c)).await? {
        return Err(AppError::NotFound);
    }
    Ok(issuer)
}

/// JSON that no cache keeps.
fn no_store(status: StatusCode, body: &Value) -> Response {
    let mut r = (status, [(CONTENT_TYPE, "application/json")], body.to_string()).into_response();
    r.headers_mut().insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    r.headers_mut().insert(PRAGMA, HeaderValue::from_static("no-cache"));
    r
}

// ---------- discovery ----------

fn scope_names() -> Vec<&'static str> {
    Scope::ALL.iter().map(|s| s.as_str()).collect()
}

/// The authorization server's metadata (RFC 8414 §2).
pub fn metadata(issuer: &str) -> Value {
    json!({
        "issuer": issuer,
        "authorization_endpoint": format!("{issuer}/oauth/authorize"),
        "token_endpoint": format!("{issuer}/oauth/token"),
        "registration_endpoint": format!("{issuer}/oauth/register"),
        "revocation_endpoint": format!("{issuer}/oauth/revoke"),
        "response_types_supported": ["code"],
        "response_modes_supported": ["query"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "code_challenge_methods_supported": ["S256"],
        "token_endpoint_auth_methods_supported": ["none"],
        "revocation_endpoint_auth_methods_supported": ["none"],
        "scopes_supported": scope_names(),
        "authorization_response_iss_parameter_supported": true,
    })
}

/// GET `<base>/.well-known/oauth-authorization-server` (RFC 8414), and the
/// same at `<base>/.well-known/openid-configuration`, the one place the MCP
/// spec's discovery order reaches under the base path (`docs/self-hosting.md`).
pub async fn server_metadata(State(state): State<Arc<AppState>>) -> Result<Response, AppError> {
    let issuer = gate(&state).await?;
    Ok(no_store(StatusCode::OK, &metadata(&issuer)))
}

fn resource_doc(issuer: &str, resource: &str) -> Value {
    json!({
        "resource": resource,
        "authorization_servers": [issuer],
        "scopes_supported": scope_names(),
        "bearer_methods_supported": ["header"],
        "resource_name": "Skrongle",
    })
}

/// GET `<base>/.well-known/oauth-protected-resource` (RFC 9728): the site.
pub async fn resource_site(State(state): State<Arc<AppState>>) -> Result<Response, AppError> {
    let issuer = gate(&state).await?;
    Ok(no_store(StatusCode::OK, &resource_doc(&issuer, &issuer)))
}

/// GET `<base>/.well-known/oauth-protected-resource/mcp`: the MCP server.
pub async fn resource_mcp(State(state): State<Arc<AppState>>) -> Result<Response, AppError> {
    let issuer = gate(&state).await?;
    Ok(no_store(StatusCode::OK, &resource_doc(&issuer, &format!("{issuer}/mcp"))))
}

// ---------- the pages ----------

/// An authorization request that failed a check: the error code and what
/// is wrong. Nothing is sent back to the app.
#[derive(Template)]
#[template(path = "oauth_error.html")]
pub struct ErrorTemplate {
    pub user: Option<CurrentUser>,
    /// The OAuth error code; '' for none.
    pub error: &'static str,
    pub message: String,
}

fn error_page(user: Option<CurrentUser>, e: AuthorizeError) -> Response {
    let message = crate::t(&format!("oauth.error.{}", e.what));
    let page = Page(ErrorTemplate { user, error: e.error, message });
    (StatusCode::BAD_REQUEST, [(CACHE_CONTROL, "no-store")], page).into_response()
}

/// The account may not connect apps: it lacks Create courses.
#[derive(Template)]
#[template(path = "oauth_no_permission.html")]
pub struct NoPermissionTemplate {
    pub user: Option<CurrentUser>,
}

fn no_permission(user: CurrentUser) -> Response {
    (StatusCode::FORBIDDEN, [(CACHE_CONTROL, "no-store")], Page(NoPermissionTemplate { user: Some(user) })).into_response()
}

/// The consent page: who asks, for what, where the answer goes; Allow or
/// Deny. Its form carries the request back, checked again on the POST.
#[derive(Template)]
#[template(path = "oauth_consent.html")]
pub struct ConsentTemplate {
    pub user: Option<CurrentUser>,
    /// The app's name as it registered (escaped by the template).
    pub client_name: String,
    /// The host the answer goes to, from the registered address.
    pub host: String,
    pub scopes: Vec<String>,
    /// The account's own courses, for the optional course list.
    pub courses: Vec<Choice>,
    pub lifetimes: Vec<Choice>,
    /// The request's fields, sent back as hidden fields.
    pub fields: Vec<(&'static str, String)>,
}

/// Check an authorization request's fields; the request, or why not.
async fn check(state: &AppState, pairs: &[(String, String)], issuer: &str) -> Result<Result<Authorize, AuthorizeError>, AppError> {
    let p = match server::AuthorizeParams::from_pairs(pairs) {
        Ok(p) => p,
        Err(e) => return Ok(Err(e)),
    };
    let id = p.client_id.clone().unwrap_or_default();
    let t = now();
    let client = state.db.call(move |c| server::client(c, &id, t)).await?;
    Ok(server::check_authorize(&p, client, issuer))
}

fn next_cookie(value: &str, max_age: i64, secure: bool) -> (HeaderName, HeaderValue) {
    let v = format!("{NEXT_COOKIE}={value}; Path={}; HttpOnly; SameSite=Lax; Max-Age={max_age}{}", crate::cookie_path(), if secure { "; Secure" } else { "" });
    // Every part is ASCII: a fixed name, hex and numbers.
    (SET_COOKIE, HeaderValue::from_str(&v).unwrap_or_else(|_| HeaderValue::from_static("")))
}

/// Clears the cookie (after a sign-in used it, or gave up).
pub fn clear_next_cookie(secure: bool) -> (HeaderName, HeaderValue) {
    next_cookie("", 0, secure)
}

fn hex(s: &str) -> String {
    s.bytes().fold(String::with_capacity(s.len() * 2), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

fn unhex(s: &str) -> Option<String> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    let bytes: Option<Vec<u8>> = (0..s.len()).step_by(2).map(|i| s.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok())).collect();
    String::from_utf8(bytes?).ok()
}

/// The query carried in the cookie, if it is one a request could have had:
/// printable ASCII, no fragment, not too long.
fn carried_query(headers: &HeaderMap) -> Option<String> {
    let value = headers.get_all(COOKIE).iter().find_map(|v| {
        v.to_str().ok()?.split(';').find_map(|pair| {
            let (k, v) = pair.trim().split_once('=')?;
            (k.trim() == NEXT_COOKIE).then(|| v.trim().to_string())
        })
    })?;
    unhex(&value).filter(|q| !q.is_empty() && q.len() <= MAX_QUERY && q.bytes().all(|b| b.is_ascii_graphic() && b != b'#'))
}

/// After a sign-in: where an authorization request that waited for it goes
/// on, `<base>/oauth/authorize?…`. Always that local path, whatever the
/// cookie holds: the sign-in cannot be turned into a redirect elsewhere.
/// The request is checked again there.
pub fn resume(headers: &HeaderMap) -> Option<String> {
    carried_query(headers).map(|q| crate::url(&format!("/oauth/authorize?{q}")))
}

/// GET /oauth/authorize: a request that fails a check gets a page that says
/// which, and is never redirected. Signed out, the normal sign-in first and
/// back here after; an account without Create courses gets a page saying to
/// ask the owner; else the consent page. Nothing is granted on GET.
pub async fn authorize(State(state): State<Arc<AppState>>, MaybeUser(user): MaybeUser, uri: Uri) -> Result<Response, AppError> {
    let issuer = gate(&state).await?;
    let raw = uri.query().unwrap_or("");
    if raw.len() > MAX_QUERY {
        return Ok(error_page(user, server::bad("too_long")));
    }
    let pairs = match Query::<Vec<(String, String)>>::try_from_uri(&uri) {
        Ok(Query(p)) => p,
        Err(_) => return Ok(error_page(user, server::bad("malformed"))),
    };
    let a = match check(&state, &pairs, &issuer).await? {
        Ok(a) => a,
        Err(e) => return Ok(error_page(user, e)),
    };
    let Some(user) = user else {
        let cookie = next_cookie(&hex(raw), server::CODE_TTL_SECS, state.secure_cookies);
        return Ok(([cookie], Redirect::to(&crate::url("/login"))).into_response());
    };
    if user.must_change_password {
        return Err(AppError::PasswordChangeRequired(format!("/oauth/authorize?{raw}")));
    }
    if !user.connects_apps() {
        return Ok(no_permission(user));
    }
    let uid = user.id;
    let owned = state.db.call(move |c| crate::courses::courses_owned_by(c, uid)).await?;
    let page = ConsentTemplate {
        client_name: a.client.name.clone(),
        host: server::redirect_host(&a.registered_uri),
        scopes: a.scopes.iter().map(|s| s.label()).collect(),
        courses: owned.into_iter().map(|c| Choice { id: c.id, label: c.title, checked: false }).collect(),
        lifetimes: Lifetime::ALL.into_iter().map(|l| Choice { id: l.id().into(), label: l.label(), checked: l == Lifetime::DEFAULT }).collect(),
        fields: a.hidden_fields(),
        user: Some(user),
    };
    Ok(([(CACHE_CONTROL, "no-store")], Page(page)).into_response())
}

/// The scheme, host and port of an address: what a browser's `Origin` says.
fn origin_of(url: &str) -> &str {
    let after = url.find("://").map_or(0, |i| i + 3);
    url.get(after..).and_then(|rest| rest.find('/')).map_or(url, |i| url.get(..after + i).unwrap_or(url))
}

/// The consent POST came from this site's own page: `Sec-Fetch-Site:
/// same-origin`, or (a browser without it) an `Origin` that is this
/// server's public address. A request with neither is not a browser's form
/// and is refused, unlike other forms (`security::is_cross_site`).
pub fn same_origin(headers: &HeaderMap, issuer: &str) -> bool {
    let get = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
    match get("sec-fetch-site") {
        Some(site) => site == "same-origin",
        None => get(ORIGIN.as_str()).is_some_and(|o| o == origin_of(issuer)),
    }
}

/// `uri` with `pairs` added to its query.
fn with_query(uri: &str, pairs: &[(&str, &str)]) -> String {
    let mut out = uri.to_string();
    for (i, (k, v)) in pairs.iter().enumerate() {
        out.push(if i == 0 && !uri.contains('?') { '?' } else { '&' });
        let _ = write!(out, "{k}={}", crate::urlenc::urlencode(v));
    }
    out
}

/// A 303 to the app's checked redirect address.
fn send_back(to: &str) -> Response {
    match HeaderValue::from_str(to) {
        Ok(v) => (StatusCode::SEE_OTHER, [(LOCATION, v), (CACHE_CONTROL, HeaderValue::from_static("no-store"))]).into_response(),
        Err(_) => error_page(None, server::bad("redirect_uri")),
    }
}

/// POST /oauth/authorize: the consent page's answer, from a signed-in
/// session (the cookie: the proxy's header alone does not sign in here)
/// and this site's own page. The request is checked again. Deny sends
/// `access_denied` back; Allow makes one grant and sends back a one-time
/// code. Both carry `state` unchanged and `iss`.
pub async fn decide(State(state): State<Arc<AppState>>, user: CurrentUser, headers: HeaderMap, Form(fields): Form<Vec<(String, String)>>) -> Result<Response, AppError> {
    let issuer = gate(&state).await?;
    if !same_origin(&headers, &issuer) {
        tracing::warn!("refused a consent answer that did not come from the consent page");
        return Ok((StatusCode::FORBIDDEN, crate::t("error.cross_site")).into_response());
    }
    if !user.connects_apps() {
        return Ok(no_permission(user));
    }
    let a = match check(&state, &fields, &issuer).await? {
        Ok(a) => a,
        Err(e) => return Ok(error_page(Some(user), e)),
    };
    let field = |name: &str| fields.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str());
    match field("decision") {
        Some("deny") => {
            tracing::info!("user {} denied an OAuth app", user.id);
            Ok(send_back(&with_query(&a.redirect_uri, &[("error", "access_denied"), ("state", &a.state), ("iss", &issuer)])))
        }
        Some("allow") => {
            let lifetime = field("lifetime").and_then(Lifetime::parse).unwrap_or(Lifetime::DEFAULT);
            let asked: Vec<String> = fields.iter().filter(|(k, _)| k == "course").map(|(_, v)| v.clone()).collect();
            let (uid, req, t) = (user.id, a.clone(), now());
            let made = state
                .db
                .call(move |c| {
                    // Only the account's own courses can be on the list.
                    let owned = crate::courses::courses_owned_by(c, uid)?;
                    let courses: Vec<String> = asked.into_iter().filter(|id| owned.iter().any(|o| &o.id == id)).collect();
                    server::allow(c, &req, &Consent { user_id: uid, courses: &courses, lifetime }, t)
                })
                .await?;
            let code = match made {
                Ok(code) => code,
                Err(e) => return Ok(refused_page(user, &e)),
            };
            tracing::info!("user {uid} connected an OAuth app");
            Ok(send_back(&with_query(&a.redirect_uri, &[("code", &code), ("state", &a.state), ("iss", &issuer)])))
        }
        _ => Ok(error_page(Some(user), server::bad("decision"))),
    }
}

/// Allow could not make the grant: the account has its four, or the server
/// is full. Said on a page; the app hears nothing.
fn refused_page(user: CurrentUser, e: &Refused) -> Response {
    let page = Page(ErrorTemplate { user: Some(user), error: "", message: e.message() });
    (StatusCode::CONFLICT, [(CACHE_CONTROL, "no-store")], page).into_response()
}

// ---------- token, register, revoke: for programs ----------

fn token_error(e: TokenError) -> Response {
    let status = match e {
        TokenError::InvalidClient => StatusCode::UNAUTHORIZED,
        TokenError::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        _ => StatusCode::BAD_REQUEST,
    };
    no_store(status, &json!({ "error": e.code() }))
}

fn too_many() -> Response {
    no_store(StatusCode::TOO_MANY_REQUESTS, &json!({ "error": "too_many_requests" }))
}

/// A form body of name-value pairs, each name at most once (RFC 6749 §3.2):
/// `application/x-www-form-urlencoded`, at most `FORM_BODY_MAX` bytes.
async fn read_form(req: Request) -> Result<Vec<(String, String)>, TokenError> {
    let declared = req.headers().get(CONTENT_LENGTH).and_then(|v| v.to_str().ok()?.parse::<usize>().ok());
    if declared.is_some_and(|n| n > FORM_BODY_MAX) {
        return Err(TokenError::InvalidRequest);
    }
    let Ok(Form(pairs)) = Form::<Vec<(String, String)>>::from_request(req, &()).await else { return Err(TokenError::InvalidRequest) };
    for (i, (k, _)) in pairs.iter().enumerate() {
        if pairs.iter().skip(i + 1).any(|(other, _)| other == k) {
            return Err(TokenError::InvalidRequest);
        }
    }
    Ok(pairs)
}

fn get<'a>(pairs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    pairs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str()).filter(|v| !v.is_empty())
}

/// The token request's answer, before the rate limit counts a refusal.
async fn token_result(state: &AppState, issuer: &str, req: Request) -> Result<Result<Tokens, TokenError>, AppError> {
    let pairs = match read_form(req).await {
        Ok(p) => p,
        Err(e) => return Ok(Err(e)),
    };
    let Some(client_id) = get(&pairs, "client_id").map(str::to_string) else { return Ok(Err(TokenError::InvalidRequest)) };
    let t = now();
    let id = client_id.clone();
    if state.db.call(move |c| server::client(c, &id, t)).await?.is_none() {
        return Ok(Err(TokenError::InvalidClient));
    }
    match get(&pairs, "grant_type") {
        Some("authorization_code") => {
            let (Some(code), Some(redirect_uri), Some(verifier), Some(resource)) =
                (get(&pairs, "code"), get(&pairs, "redirect_uri"), get(&pairs, "code_verifier"), get(&pairs, "resource"))
            else {
                return Ok(Err(TokenError::InvalidRequest));
            };
            if server::served_resource(resource, issuer).is_none() {
                return Ok(Err(TokenError::InvalidTarget));
            }
            let (code, redirect_uri, verifier, resource) = (code.to_string(), redirect_uri.to_string(), verifier.to_string(), resource.to_string());
            state
                .db
                .call(move |c| {
                    let x = CodeExchange { code: &code, client_id: &client_id, redirect_uri: &redirect_uri, code_verifier: &verifier, resource: &resource };
                    server::exchange_code(c, &x, t)
                })
                .await
        }
        Some("refresh_token") => {
            let Some(refresh) = get(&pairs, "refresh_token").map(str::to_string) else { return Ok(Err(TokenError::InvalidRequest)) };
            if get(&pairs, "resource").is_some_and(|r| server::served_resource(r, issuer).is_none()) {
                return Ok(Err(TokenError::InvalidTarget));
            }
            state.db.call(move |c| server::refresh(c, &refresh, &client_id, t)).await
        }
        Some(_) => Ok(Err(TokenError::UnsupportedGrantType)),
        None => Ok(Err(TokenError::InvalidRequest)),
    }
}

/// POST /oauth/token: a code with its PKCE verifier, the client, the
/// redirect address and the resource gives an access token (an hour) and a
/// refresh token; a refresh token gives a new pair and stops working.
/// Errors per RFC 6749 §5.2, the code only. Each refusal counts against
/// `OAUTH_TOKEN_FAILURES_PER_IP`; past it, 429.
pub async fn token(State(state): State<Arc<AppState>>, ClientIp(ip): ClientIp, req: Request) -> Result<Response, AppError> {
    let issuer = gate(&state).await?;
    if state.limits.blocked(OAUTH_TOKEN_FAILURES_PER_IP, &ip, now()) {
        return Ok(too_many());
    }
    Ok(match token_result(&state, &issuer, req).await? {
        Ok(t) => no_store(
            StatusCode::OK,
            &json!({
                "access_token": t.access,
                "token_type": "Bearer",
                "expires_in": t.expires_in,
                "refresh_token": t.refresh,
                "scope": Scope::join(&t.scopes),
            }),
        ),
        Err(e) => {
            state.limits.hit(OAUTH_TOKEN_FAILURES_PER_IP, &ip, now());
            token_error(e)
        }
    })
}

/// POST /oauth/register (RFC 7591): an app registers its name and redirect
/// addresses and gets a client id. Public clients only; at most
/// `OAUTH_REGISTRATIONS_PER_IP` an hour from one address and
/// `REGISTRATION_BODY_MAX` bytes. Nothing named in it is ever fetched.
pub async fn register(State(state): State<Arc<AppState>>, ClientIp(ip): ClientIp, req: Request) -> Result<Response, AppError> {
    gate(&state).await?;
    let t = now();
    if !state.limits.hit(OAUTH_REGISTRATIONS_PER_IP, &ip, t) {
        return Ok(too_many());
    }
    let too_big = || no_store(StatusCode::PAYLOAD_TOO_LARGE, &json!({ "error": "invalid_client_metadata" }));
    let declared = req.headers().get(CONTENT_LENGTH).and_then(|v| v.to_str().ok()?.parse::<usize>().ok());
    if declared.is_some_and(|n| n > server::REGISTRATION_BODY_MAX) {
        return Ok(too_big());
    }
    let Ok(body) = axum::body::to_bytes(req.into_body(), server::REGISTRATION_BODY_MAX).await else { return Ok(too_big()) };
    let Ok(reg) = serde_json::from_slice::<server::Registration>(&body) else {
        return Ok(no_store(StatusCode::BAD_REQUEST, &json!({ "error": "invalid_client_metadata" })));
    };
    let (name, uris) = match server::check_registration(&reg) {
        Ok(ok) => ok,
        Err(code) => return Ok(no_store(StatusCode::BAD_REQUEST, &json!({ "error": code }))),
    };
    let (n, u) = (name.clone(), uris.clone());
    let client_id = match state.db.call(move |c| server::register(c, &n, &u, t)).await? {
        Ok(id) => id,
        Err(_) => return Ok(no_store(StatusCode::SERVICE_UNAVAILABLE, &json!({ "error": "temporarily_unavailable" }))),
    };
    tracing::info!("an OAuth app registered");
    Ok(no_store(
        StatusCode::CREATED,
        &json!({
            "client_id": client_id,
            "client_id_issued_at": t,
            "client_name": name,
            "redirect_uris": uris,
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "token_endpoint_auth_method": "none",
        }),
    ))
}

/// POST /oauth/revoke (RFC 7009): an app disconnects itself. The grant
/// behind its access or refresh token goes, with every token under it. An
/// unknown token is not an error (200 either way); an unknown client is
/// `invalid_client` and counts as a refusal.
pub async fn revoke(State(state): State<Arc<AppState>>, ClientIp(ip): ClientIp, req: Request) -> Result<Response, AppError> {
    gate(&state).await?;
    if state.limits.blocked(OAUTH_TOKEN_FAILURES_PER_IP, &ip, now()) {
        return Ok(too_many());
    }
    let refuse = |e: TokenError| {
        state.limits.hit(OAUTH_TOKEN_FAILURES_PER_IP, &ip, now());
        token_error(e)
    };
    let pairs = match read_form(req).await {
        Ok(p) => p,
        Err(e) => return Ok(refuse(e)),
    };
    let (Some(token), Some(client_id)) = (get(&pairs, "token").map(str::to_string), get(&pairs, "client_id").map(str::to_string)) else {
        return Ok(refuse(TokenError::InvalidRequest));
    };
    let t = now();
    let known = state
        .db
        .call(move |c| {
            if server::client(c, &client_id, t)?.is_none() {
                return Ok(false);
            }
            if server::revoke(c, &token, &client_id)? {
                tracing::info!("an OAuth app disconnected itself");
            }
            Ok(true)
        })
        .await?;
    if !known {
        return Ok(refuse(TokenError::InvalidClient));
    }
    Ok(no_store(StatusCode::OK, &json!({})))
}

/// The OAuth routes. Their paths are what `security::csrf` and
/// `proxy_auth::left_alone` name too.
pub fn routes() -> axum::Router<Arc<AppState>> {
    use axum::routing::{get, post};
    axum::Router::new()
        .route("/.well-known/oauth-authorization-server", get(server_metadata))
        .route("/.well-known/openid-configuration", get(server_metadata))
        .route("/.well-known/oauth-protected-resource", get(resource_site))
        .route("/.well-known/oauth-protected-resource/mcp", get(resource_mcp))
        .route("/oauth/authorize", get(authorize).post(decide))
        .route("/oauth/token", post(token))
        .route("/oauth/register", post(register))
        .route("/oauth/revoke", post(revoke))
}

/// The program-facing paths: no cookie, no CSRF check, no proxy sign-in.
pub const PROGRAM_PATHS: [&str; 3] = ["/oauth/token", "/oauth/register", "/oauth/revoke"];

#[cfg(test)]
mod tests;
