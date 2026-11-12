//! Agent credentials (#167, plan #166): the grants an account gave apps and
//! assistants, their bearer tokens, the two scopes, and the `ApiUser`
//! extractor. What it does for people is in `docs/features.md` "Connected
//! apps".
//!
//! A grant is one connection: the account, the app's name and client id,
//! its scopes, an optional course list, and when it was made, expires and
//! was last used. Every grant expires (30 days, 90 or a year). Its tokens
//! are 256 random bits behind the `skr_` prefix; only their SHA-256 is
//! stored (`auth::hash_token`, as for sessions), so a copy of the database
//! holds no working token.
//!
//! `ApiUser` reads only `Authorization: Bearer …`; `CurrentUser` reads only
//! the session cookie. A cookie authorises no API route and a token opens no
//! page. Everything is checked again on every request, nothing is cached:
//! the owner's switch (`settings::APPS_ON`), the grant and its expiry, the
//! account's Create courses permission and its temporary-password gate.

use axum::extract::FromRequestParts;
use axum::http::header::{AUTHORIZATION, CACHE_CONTROL, WWW_AUTHENTICATE};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use rusqlite::{params, Connection, OptionalExtension};
use std::sync::Arc;

use super::hash_token;
use crate::caps::{MAX_API_GRANTS, MAX_API_TOKENS, MAX_GRANTS_PER_ACCOUNT};
use crate::error::AppError;
use crate::perms::{Perm, Perms};
use crate::security::{ClientIp, API_BAD_TOKENS_PER_IP};
use crate::AppState;

/// Every token starts with this, so a leaked one can be recognised.
pub const TOKEN_PREFIX: &str = "skr_";

/// The `client_id` of a grant made by hand on the profile (Make a token).
pub const PERSONAL_CLIENT_ID: &str = "personal";

/// `last_used_at` is written at most this often per grant, so reads do not
/// become SD-card writes.
pub const LAST_USED_EVERY_SECS: i64 = 60;

/// The longest name a token can be given on the profile (characters).
pub const NAME_MAX: usize = 60;

/// What a grant lets an app do. Fixed: nothing else exists to ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// List the account's own courses, read their files and the course
    /// format, validate.
    Read,
    /// Create a course, replace its files.
    Write,
}

impl Scope {
    pub const ALL: [Scope; 2] = [Scope::Read, Scope::Write];

    pub fn as_str(self) -> &'static str {
        match self {
            Scope::Read => "courses:read",
            Scope::Write => "courses:write",
        }
    }

    #[cfg_attr(not(test), allow(dead_code))] // OAuth parses the scopes asked for (#169)
    pub fn parse(s: &str) -> Option<Scope> {
        Scope::ALL.into_iter().find(|x| x.as_str() == s)
    }

    /// The scopes named in a space-separated list, in `ALL` order; unknown
    /// ones are dropped.
    pub fn parse_list(s: &str) -> Vec<Scope> {
        let named: Vec<&str> = s.split_whitespace().collect();
        Scope::ALL.into_iter().filter(|x| named.contains(&x.as_str())).collect()
    }

    pub fn join(scopes: &[Scope]) -> String {
        scopes.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" ")
    }

    /// How the profile and Manage Users name it.
    pub fn label(self) -> String {
        crate::t(match self {
            Scope::Read => "apps.scope_read",
            Scope::Write => "apps.scope_write",
        })
    }
}

/// How long a grant lasts. There is no "until revoked".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lifetime {
    Days30,
    Days90,
    Year,
}

impl Lifetime {
    pub const ALL: [Lifetime; 3] = [Lifetime::Days30, Lifetime::Days90, Lifetime::Year];
    pub const DEFAULT: Lifetime = Lifetime::Days90;

    /// The form's value.
    pub fn id(self) -> &'static str {
        match self {
            Lifetime::Days30 => "30",
            Lifetime::Days90 => "90",
            Lifetime::Year => "365",
        }
    }

    pub fn parse(s: &str) -> Option<Lifetime> {
        Lifetime::ALL.into_iter().find(|l| l.id() == s)
    }

    pub fn secs(self) -> i64 {
        let days = match self {
            Lifetime::Days30 => 30,
            Lifetime::Days90 => 90,
            Lifetime::Year => 365,
        };
        days * 24 * 60 * 60
    }

    pub fn label(self) -> String {
        crate::t(match self {
            Lifetime::Days30 => "apps.lifetime_30",
            Lifetime::Days90 => "apps.lifetime_90",
            Lifetime::Year => "apps.lifetime_365",
        })
    }
}

/// What a token is: one made by hand on the profile, or (OAuth, #169) a
/// short access token and its refresh token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Personal,
    #[cfg_attr(not(test), allow(dead_code))] // OAuth mints these (#169)
    Access,
    #[cfg_attr(not(test), allow(dead_code))] // OAuth mints these (#169)
    Refresh,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Personal => "personal",
            Kind::Access => "access",
            Kind::Refresh => "refresh",
        }
    }
}

/// One grant, as the profile and Manage Users list it.
#[derive(Clone, Debug)]
#[allow(dead_code)] // `user_id` and `client_id` are read by OAuth and the owner's review (#169, #170)
pub struct Grant {
    pub id: i64,
    pub user_id: i64,
    /// The account's address (Manage Users names whose it is).
    pub email: String,
    pub client_id: String,
    pub client_name: String,
    pub scopes: Vec<Scope>,
    /// The course ids it may reach; empty for all of the account's own.
    pub courses: Vec<String>,
    pub created_at: i64,
    pub expires_at: i64,
    pub last_used_at: Option<i64>,
}

/// What a new grant is for.
pub struct NewGrant<'a> {
    pub client_id: &'a str,
    pub client_name: &'a str,
    pub scopes: &'a [Scope],
    pub courses: &'a [String],
    pub lifetime: Lifetime,
}

/// Why a grant or a token was not made.
#[derive(Debug, PartialEq, Eq)]
pub enum Refused {
    /// The account already has `MAX_GRANTS_PER_ACCOUNT`: revoke one first.
    TooManyGrants,
    /// The server's table is at its cap (`MAX_API_GRANTS`, `MAX_API_TOKENS`).
    Full,
    /// No scope was asked for.
    NoScope,
}

impl Refused {
    pub fn message(&self) -> String {
        match self {
            Refused::TooManyGrants => crate::t1("apps.too_many", "max", MAX_GRANTS_PER_ACCOUNT),
            Refused::Full => crate::t("apps.server_full"),
            Refused::NoScope => crate::t("apps.no_scope"),
        }
    }
}

fn count(conn: &Connection, sql: &str, p: impl rusqlite::Params) -> rusqlite::Result<usize> {
    conn.query_row(sql, p, |r| r.get::<_, i64>(0)).map(|n| n.max(0) as usize)
}

/// A new grant for `user_id`, refused past the caps. Its course list is
/// kept as given: the caller keeps only the account's own courses.
pub fn create_grant(conn: &Connection, user_id: i64, g: &NewGrant, now: i64) -> rusqlite::Result<Result<i64, Refused>> {
    if g.scopes.is_empty() {
        return Ok(Err(Refused::NoScope));
    }
    if count(conn, "SELECT COUNT(*) FROM api_grants WHERE user_id = ?1", [user_id])? >= MAX_GRANTS_PER_ACCOUNT {
        return Ok(Err(Refused::TooManyGrants));
    }
    if count(conn, "SELECT COUNT(*) FROM api_grants", [])? >= MAX_API_GRANTS {
        return Ok(Err(Refused::Full));
    }
    conn.execute(
        "INSERT INTO api_grants (user_id, client_id, client_name, scopes, courses, created_at, expires_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![user_id, g.client_id, g.client_name, Scope::join(g.scopes), g.courses.join(" "), now, now + g.lifetime.secs()],
    )?;
    Ok(Ok(conn.last_insert_rowid()))
}

/// A fresh token: the prefix and 256 random bits in hex. It exists in
/// plain text only in the answer that shows it.
fn new_token() -> String {
    format!("{TOKEN_PREFIX}{}", super::generate_token())
}

/// A new token of `kind` under `grant_id`, working until `expires_at` (never
/// past the grant's own expiry, which `user_for_token` checks too). Only
/// its hash is stored; the plain token is returned this once.
pub fn mint(conn: &Connection, grant_id: i64, kind: Kind, expires_at: i64) -> rusqlite::Result<Result<String, Refused>> {
    if count(conn, "SELECT COUNT(*) FROM api_tokens", [])? >= MAX_API_TOKENS {
        return Ok(Err(Refused::Full));
    }
    let token = new_token();
    conn.execute(
        "INSERT INTO api_tokens (grant_id, kind, token_hash, expires_at) VALUES (?1, ?2, ?3, ?4)",
        params![grant_id, kind.as_str(), hash_token(&token), expires_at],
    )?;
    Ok(Ok(token))
}

/// Make a token on the profile: a grant named `name` and one personal token
/// that lasts as long as it. Call it inside a transaction, so a refused
/// token leaves no grant behind.
pub fn make_personal(conn: &Connection, user_id: i64, g: &NewGrant, now: i64) -> rusqlite::Result<Result<String, Refused>> {
    let grant = match create_grant(conn, user_id, g, now)? {
        Ok(id) => id,
        Err(e) => return Ok(Err(e)),
    };
    mint(conn, grant, Kind::Personal, now + g.lifetime.secs())
}

const GRANT_COLS: &str = "g.id, g.user_id, COALESCE(u.email, ''), g.client_id, g.client_name, g.scopes, g.courses, g.created_at, g.expires_at, g.last_used_at";

fn grant_row(r: &rusqlite::Row) -> rusqlite::Result<Grant> {
    Ok(Grant {
        id: r.get(0)?,
        user_id: r.get(1)?,
        email: r.get(2)?,
        client_id: r.get(3)?,
        client_name: r.get(4)?,
        scopes: Scope::parse_list(&r.get::<_, String>(5)?),
        courses: r.get::<_, String>(6)?.split_whitespace().map(str::to_string).collect(),
        created_at: r.get(7)?,
        expires_at: r.get(8)?,
        last_used_at: r.get(9)?,
    })
}

/// The account's grants still in force, newest first.
pub fn grants(conn: &Connection, user_id: i64, now: i64) -> rusqlite::Result<Vec<Grant>> {
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT {GRANT_COLS} FROM api_grants g JOIN users u ON u.id = g.user_id WHERE g.user_id = ?1 AND g.expires_at > ?2 ORDER BY g.id DESC"
    ))?;
    let rows = stmt.query_map(params![user_id, now], grant_row)?;
    rows.collect()
}

/// Every account's grants still in force (the owner's list on Manage
/// Users), newest first.
pub fn all_grants(conn: &Connection, now: i64) -> rusqlite::Result<Vec<Grant>> {
    let mut stmt =
        conn.prepare_cached(&format!("SELECT {GRANT_COLS} FROM api_grants g JOIN users u ON u.id = g.user_id WHERE g.expires_at > ?1 ORDER BY g.id DESC"))?;
    let rows = stmt.query_map([now], grant_row)?;
    rows.collect()
}

/// Revoke one of `user_id`'s grants and every token under it. `false` when
/// the account has no such grant.
pub fn revoke(conn: &Connection, user_id: i64, grant_id: i64) -> rusqlite::Result<bool> {
    Ok(conn.execute("DELETE FROM api_grants WHERE id = ?1 AND user_id = ?2", params![grant_id, user_id])? > 0)
}

/// Revoke any account's grant (the owner, on Manage Users). `false` when
/// there is no such grant.
pub fn revoke_any(conn: &Connection, grant_id: i64) -> rusqlite::Result<bool> {
    Ok(conn.execute("DELETE FROM api_grants WHERE id = ?1", [grant_id])? > 0)
}

/// Revoke every grant of the account: a password change, a temporary
/// password, two-factor turned off by the owner. Returns how many.
pub fn revoke_all_for_user(conn: &Connection, user_id: i64) -> rusqlite::Result<usize> {
    conn.execute("DELETE FROM api_grants WHERE user_id = ?1", [user_id])
}

/// Delete expired tokens and grants (with the sessions, in the hourly
/// housekeeping and at startup).
pub fn purge_expired(conn: &Connection, now: i64) -> rusqlite::Result<usize> {
    let tokens = conn.execute("DELETE FROM api_tokens WHERE expires_at <= ?1", [now])?;
    let grants = conn.execute("DELETE FROM api_grants WHERE expires_at <= ?1", [now])?;
    Ok(tokens + grants)
}

/// What a token lookup found.
#[derive(Debug)]
pub enum Lookup {
    /// The owner's switch is off: the API does not exist (404).
    Off,
    /// No working token: unknown, expired, revoked, a refresh token, an
    /// account without Create courses or behind a temporary password (401).
    Refused,
    Found(ApiUser),
}

/// The token query: one indexed lookup by hash, so the time it takes says
/// nothing about how near a wrong token was to a right one.
const TOKEN_SQL: &str = "SELECT g.user_id, g.id, g.client_id, g.client_name, g.scopes, g.courses, g.last_used_at,
        u.owner, u.perms, u.must_change_password
 FROM api_tokens t JOIN api_grants g ON g.id = t.grant_id JOIN users u ON u.id = g.user_id
 WHERE t.token_hash = ?1 AND t.kind IN ('personal', 'access') AND t.expires_at > ?2 AND g.expires_at > ?2";

/// The account and grant behind a bearer token, checked in full: the
/// switch, the token and grant and their expiry, Create courses, the
/// temporary-password gate. Marks the grant used, at most once a minute.
pub fn user_for_token(conn: &Connection, token: &str, now: i64) -> rusqlite::Result<Lookup> {
    if !crate::settings::apps_on(conn)? {
        return Ok(Lookup::Off);
    }
    let found = conn
        .query_row(TOKEN_SQL, params![hash_token(token), now], |r| {
            let user = ApiUser {
                id: r.get(0)?,
                grant_id: r.get(1)?,
                client_id: r.get(2)?,
                client_name: r.get(3)?,
                scopes: Scope::parse_list(&r.get::<_, String>(4)?),
                courses: r.get::<_, String>(5)?.split_whitespace().map(str::to_string).collect(),
            };
            let may = super::allowed(r.get(7)?, Perms::from_bits(r.get(8)?), Perm::CreateCourses);
            let waiting: bool = r.get(9)?;
            Ok((user, r.get::<_, Option<i64>>(6)?, may && !waiting))
        })
        .optional()?;
    let Some((user, last_used, ok)) = found else { return Ok(Lookup::Refused) };
    if !ok {
        return Ok(Lookup::Refused);
    }
    if last_used.is_none_or(|t| now - t >= LAST_USED_EVERY_SECS) {
        conn.execute("UPDATE api_grants SET last_used_at = ?2 WHERE id = ?1", params![user.grant_id, now])?;
    }
    Ok(Lookup::Found(user))
}

/// The bearer token of a request: `Authorization: Bearer <token>`. `None`
/// without the header; `Some("")` for one that holds no token of ours
/// (another scheme, the wrong shape), which still costs a lookup.
pub fn bearer(headers: &HeaderMap) -> Option<String> {
    let v = headers.get(AUTHORIZATION)?.to_str().unwrap_or("").trim();
    let (scheme, token) = v.split_once(' ').unwrap_or((v, ""));
    let token = token.trim();
    let ours = token.len() == TOKEN_PREFIX.len() + 64
        && token.starts_with(TOKEN_PREFIX)
        && token.bytes().skip(TOKEN_PREFIX.len()).all(|b| b.is_ascii_hexdigit());
    Some(if scheme.eq_ignore_ascii_case("bearer") && ours { token.to_ascii_lowercase() } else { String::new() })
}

/// Does the address carry a token (`access_token=`, `token=`, in any case
/// or percent-encoding)? Such a request is refused whatever else it
/// carries: an address ends up in logs, histories and Referer headers. A
/// query that does not parse is looked at as it is.
pub fn token_in_query(uri: &axum::http::Uri) -> bool {
    let Some(raw) = uri.query() else { return false };
    let named = |name: &str| name.eq_ignore_ascii_case("access_token") || name.eq_ignore_ascii_case("token");
    match axum::extract::Query::<Vec<(String, String)>>::try_from_uri(uri) {
        Ok(axum::extract::Query(pairs)) => pairs.iter().any(|(k, _)| named(k.trim())),
        Err(_) => raw.split('&').any(|pair| named(pair.split_once('=').map_or(pair, |(k, _)| k))) || raw.to_ascii_lowercase().contains("token"),
    }
}

/// The account an API request acts for, from its bearer token only.
#[derive(Clone, Debug)]
pub struct ApiUser {
    /// The account's id.
    pub id: i64,
    pub grant_id: i64,
    pub client_id: String,
    pub client_name: String,
    pub scopes: Vec<Scope>,
    /// The courses the grant may reach; empty for all of the account's own.
    pub courses: Vec<String>,
}

impl ApiUser {
    /// `Err(InsufficientScope)` (the one 403) when the grant lacks `scope`.
    pub fn require(&self, scope: Scope) -> Result<(), ApiError> {
        if self.scopes.contains(&scope) { Ok(()) } else { Err(ApiError::InsufficientScope(scope)) }
    }

    /// The grant's course list lets it reach `course_id`. The route checks
    /// that the course is the account's own as well: anything else is 404.
    pub fn allows_course(&self, course_id: &str) -> bool {
        self.courses.is_empty() || self.courses.iter().any(|c| c == course_id)
    }
}

impl FromRequestParts<Arc<AppState>> for ApiUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &Arc<AppState>) -> Result<Self, Self::Rejection> {
        if token_in_query(&parts.uri) {
            return Err(ApiError::TokenInAddress);
        }
        let Ok(ClientIp(ip)) = ClientIp::from_request_parts(parts, state).await;
        let now = crate::db::now();
        if state.limits.blocked(API_BAD_TOKENS_PER_IP, &ip, now) {
            return Err(ApiError::TooManyRequests);
        }
        // No header: still one lookup (of a hash nothing has), so the switch
        // is asked and the answer takes the time a wrong token takes.
        let token = bearer(&parts.headers).unwrap_or_default();
        match state.db.call(move |c| user_for_token(c, &token, now)).await? {
            Lookup::Off => Err(ApiError::Off),
            Lookup::Found(u) => Ok(u),
            Lookup::Refused => {
                state.limits.hit(API_BAD_TOKENS_PER_IP, &ip, now);
                Err(ApiError::Unauthorized)
            }
        }
    }
}

/// How an API route says no. No body names an account, a grant or a token.
#[derive(Debug)]
pub enum ApiError {
    /// No working bearer token: 401, empty, `WWW-Authenticate: Bearer`.
    Unauthorized,
    /// `?access_token=` or `?token=` in the address: 400.
    TokenInAddress,
    /// The owner's switch is off: 404, empty, as if there were no API.
    Off,
    /// Too many bad tokens from this address: 429.
    TooManyRequests,
    /// The grant lacks this scope: 403 `insufficient_scope`.
    InsufficientScope(Scope),
    /// Not there, or not the account's own: 404.
    #[allow(dead_code)] // the MCP tools answer a missing course inside a tool result (#168), not with a status
    NotFound,
    Internal(AppError),
}

impl From<AppError> for ApiError {
    fn from(e: AppError) -> Self {
        ApiError::Internal(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let json = |status: StatusCode, body: serde_json::Value| {
            (status, [(axum::http::header::CONTENT_TYPE, "application/json")], body.to_string()).into_response()
        };
        let mut r = match self {
            ApiError::Unauthorized => {
                let mut r = StatusCode::UNAUTHORIZED.into_response();
                r.headers_mut().insert(WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
                r
            }
            ApiError::TokenInAddress => json(StatusCode::BAD_REQUEST, serde_json::json!({ "error": "invalid_request" })),
            ApiError::Off | ApiError::NotFound => StatusCode::NOT_FOUND.into_response(),
            ApiError::TooManyRequests => json(StatusCode::TOO_MANY_REQUESTS, serde_json::json!({ "error": "too_many_requests" })),
            ApiError::InsufficientScope(scope) => {
                let mut r = json(StatusCode::FORBIDDEN, serde_json::json!({ "error": "insufficient_scope", "scope": scope.as_str() }));
                let v = format!("Bearer error=\"insufficient_scope\", scope=\"{}\"", scope.as_str());
                if let Ok(v) = HeaderValue::from_str(&v) {
                    r.headers_mut().insert(WWW_AUTHENTICATE, v);
                }
                r
            }
            ApiError::Internal(e) => {
                tracing::error!("api: {e}");
                json(StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({ "error": "server_error" }))
            }
        };
        r.headers_mut().insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
        r
    }
}

#[cfg(test)]
mod tests;
