//! Skrongle's own small OAuth 2.1 authorization server (#169, plan #166):
//! the part the MCP authorization spec asks of a server that issues its own
//! tokens, so an app that connects from a vendor's cloud can sign in. The
//! HTTP side (discovery, the consent page, the token, registration and
//! revocation endpoints) is `routes/oauth.rs`; grants and tokens are
//! `auth::api`'s. What it does for people is in `docs/features.md`
//! "Connected apps"; the wire format in `docs/api.md` "OAuth".
//!
//! The rules, each checked here and tested in `oauth_server/tests.rs`:
//!
//! - public clients only (`token_endpoint_auth_method: none`), and PKCE
//!   with S256 on every request (`plain` is refused);
//! - redirect addresses match exactly: `https://…`, or `http://` on
//!   `localhost`, `127.0.0.1` or `[::1]` where only the port may differ;
//! - `state` and `resource` (this server's address or its `/mcp`) are
//!   required; the code is bound to the client, the redirect address and
//!   the resource, and the token request repeats all three;
//! - a code works once, for ten minutes; a second use revokes the grant it
//!   made and every token under it;
//! - a refresh token works once, and only for the client the grant was
//!   made for: refreshing marks it used, and a used one presented again
//!   revokes the grant and every token under it (#184);
//! - registration is bounded: five redirect addresses of 200 characters, a
//!   name of 60, `MAX_OAUTH_CLIENTS` rows (the oldest unused one goes
//!   first), and a registration nobody finished a grant for within a day
//!   is purged. Nothing is ever fetched for a client.

use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;

use crate::auth::api::{self, Kind, Lifetime, NewGrant, Refused, Scope};
use crate::auth::{generate_token, hash_token};
use crate::caps::{MAX_OAUTH_CLIENTS, MAX_OAUTH_CODES};
use crate::perms::Perm;

/// A code works for ten minutes, once.
pub const CODE_TTL_SECS: i64 = 10 * 60;
/// An access token from OAuth works for an hour (never past its grant).
pub const ACCESS_TTL_SECS: i64 = 60 * 60;
/// A registration nobody finished a grant for is gone after a day.
pub const UNUSED_CLIENT_TTL_SECS: i64 = 24 * 60 * 60;
/// Most redirect addresses one registration may hold.
pub const MAX_REDIRECT_URIS: usize = 5;
/// Longest redirect address accepted.
pub const MAX_URI_LEN: usize = 200;
/// Longest `state` accepted.
pub const MAX_STATE_LEN: usize = 500;
/// Largest registration body (RFC 7591 JSON).
pub const REGISTRATION_BODY_MAX: usize = 4 * 1024;

// ---------- redirect addresses ----------

/// The parts of a redirect address the checks need.
struct Uri<'a> {
    scheme: &'a str,
    host: &'a str,
    /// Path and query, from the first `/` or `?` on ('' for none).
    rest: &'a str,
}

/// A redirect address as the checks read it: printable ASCII, no space, no
/// fragment, no user name, a host of letters, digits, `-` and `.` (or an
/// IPv6 address in brackets), a numeric port.
fn parse_uri(uri: &str) -> Option<Uri<'_>> {
    if uri.is_empty() || uri.len() > MAX_URI_LEN || uri.contains('#') || !uri.bytes().all(|b| b.is_ascii_graphic()) {
        return None;
    }
    let (scheme, after) = uri.split_once("://")?;
    let end = after.find(['/', '?']).unwrap_or(after.len());
    let (authority, rest) = after.split_at(end);
    if authority.is_empty() || authority.contains('@') || authority.contains('\\') {
        return None;
    }
    let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
        let (h, tail) = v6.split_once(']')?;
        if h.is_empty() || !h.bytes().all(|b| b.is_ascii_hexdigit() || b == b':' || b == b'.') {
            return None;
        }
        match tail {
            "" => (h, None),
            t => (h, Some(t.strip_prefix(':')?)),
        }
    } else {
        let (h, p) = match authority.split_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (authority, None),
        };
        if h.is_empty() || !h.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.') {
            return None;
        }
        (h, p)
    };
    if port.is_some_and(|p| p.is_empty() || p.len() > 5 || !p.bytes().all(|b| b.is_ascii_digit())) {
        return None;
    }
    Some(Uri { scheme, host, rest })
}

fn is_loopback(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

/// A redirect address an app may register: `https://` with a host, or
/// `http://localhost`, `http://127.0.0.1` or `http://[::1]` (a program on
/// the person's own computer). No other scheme (`javascript:`, `data:`, an
/// app's own), no fragment, no user name.
pub fn valid_redirect_uri(uri: &str) -> bool {
    parse_uri(uri).is_some_and(|u| match u.scheme {
        "https" => true,
        "http" => is_loopback(u.host),
        _ => false,
    })
}

/// `asked` is the registered address `registered`: the same string, or for
/// a loopback `http://` address the same but for the port (RFC 8252 §7.3: a
/// program on the person's computer picks a free port each time).
pub fn redirect_matches(registered: &str, asked: &str) -> bool {
    if !valid_redirect_uri(asked) {
        return false;
    }
    if registered == asked {
        return true;
    }
    match (parse_uri(registered), parse_uri(asked)) {
        (Some(r), Some(a)) => r.scheme == "http" && a.scheme == "http" && is_loopback(r.host) && r.host == a.host && r.rest == a.rest,
        _ => false,
    }
}

/// The host the consent page names: where the answer goes.
pub fn redirect_host(uri: &str) -> String {
    parse_uri(uri).map(|u| u.host.to_string()).unwrap_or_default()
}

// ---------- the registry ----------

/// One registered app.
#[derive(Clone, Debug)]
pub struct Client {
    pub client_id: String,
    pub name: String,
    pub redirect_uris: Vec<String>,
}

impl Client {
    /// The registered address `uri` matches, if any.
    pub fn registered_for(&self, uri: &str) -> Option<&str> {
        self.redirect_uris.iter().find(|r| redirect_matches(r, uri)).map(String::as_str)
    }
}

/// A registration still in force: made within the day, finished a grant
/// within the day, or with a grant now.
const LIVE_CLIENT: &str = "(COALESCE(last_grant_at, created_at) > ?2 OR EXISTS (SELECT 1 FROM api_grants g WHERE g.client_id = oauth_clients.client_id))";

/// The app `client_id`, unless nobody finished a grant for it within a day.
pub fn client(conn: &Connection, client_id: &str, now: i64) -> rusqlite::Result<Option<Client>> {
    if client_id.is_empty() || client_id.len() > 64 {
        return Ok(None);
    }
    let sql = format!("SELECT client_id, name, redirect_uris FROM oauth_clients WHERE client_id = ?1 AND {LIVE_CLIENT}");
    conn.query_row(&sql, params![client_id, now - UNUSED_CLIENT_TTL_SECS], |r| {
        Ok(Client { client_id: r.get(0)?, name: r.get(1)?, redirect_uris: r.get::<_, String>(2)?.lines().map(str::to_string).collect() })
    })
    .optional()
}

/// What a registration may say (RFC 7591 §2). Any other field is ignored,
/// and nothing named in one (a logo, a policy page) is ever fetched.
#[derive(Deserialize, Default, Debug)]
pub struct Registration {
    pub client_name: Option<String>,
    #[serde(default)]
    pub redirect_uris: Vec<String>,
    pub token_endpoint_auth_method: Option<String>,
    pub grant_types: Option<Vec<String>>,
    pub response_types: Option<Vec<String>>,
}

/// An app's name as registered: trimmed, at most `api::NAME_MAX`
/// characters, no control, direction or invisible characters
/// (`api::is_hidden_char`), no markup.
pub fn valid_name(name: &str) -> bool {
    name.chars().count() <= api::NAME_MAX && !name.chars().any(|c| api::is_hidden_char(c) || matches!(c, '<' | '>' | '"' | '`'))
}

/// Check a registration: the app's name and redirect addresses, or the
/// RFC 7591 error code.
pub fn check_registration(r: &Registration) -> Result<(String, Vec<String>), &'static str> {
    if r.redirect_uris.is_empty() || r.redirect_uris.len() > MAX_REDIRECT_URIS || !r.redirect_uris.iter().all(|u| valid_redirect_uri(u)) {
        return Err("invalid_redirect_uri");
    }
    if r.token_endpoint_auth_method.as_deref().is_some_and(|m| m != "none") {
        return Err("invalid_client_metadata");
    }
    if r.grant_types.as_ref().is_some_and(|g| !g.iter().all(|g| g == "authorization_code" || g == "refresh_token")) {
        return Err("invalid_client_metadata");
    }
    if r.response_types.as_ref().is_some_and(|t| !t.iter().all(|t| t == "code")) {
        return Err("invalid_client_metadata");
    }
    let name = r.client_name.as_deref().unwrap_or("").trim();
    if !valid_name(name) {
        return Err("invalid_client_metadata");
    }
    // An app with no name is named by where the answer goes.
    let name = if name.is_empty() { r.redirect_uris.first().map(|u| redirect_host(u)).unwrap_or_default() } else { name.to_string() };
    let mut uris: Vec<String> = Vec::new();
    for u in &r.redirect_uris {
        if !uris.contains(u) {
            uris.push(u.clone());
        }
    }
    Ok((name, uris))
}

/// Registrations nobody finished a grant for within a day, and apps whose
/// grants are all gone for a day: deleted. Returns how many.
fn purge_clients(conn: &Connection, now: i64) -> rusqlite::Result<usize> {
    conn.execute(
        "DELETE FROM oauth_clients WHERE COALESCE(last_grant_at, created_at) <= ?1
         AND NOT EXISTS (SELECT 1 FROM api_grants g WHERE g.client_id = oauth_clients.client_id)",
        [now - UNUSED_CLIENT_TTL_SECS],
    )
}

/// How many apps registered since `since` (the owner's card, #170). A
/// registration pushed out by a newer one at the cap is no longer counted.
pub fn registered_since(conn: &Connection, since: i64) -> rusqlite::Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM oauth_clients WHERE created_at > ?1", [since], |r| r.get(0))
}

/// Register an app; its new client id, or `Refused::Full` when the table
/// holds `MAX_OAUTH_CLIENTS` rows that all have a grant. Stale rows go
/// first, then the oldest row with no grant.
pub fn register(conn: &Connection, name: &str, redirect_uris: &[String], now: i64) -> rusqlite::Result<Result<String, Refused>> {
    purge_clients(conn, now)?;
    let rows: i64 = conn.query_row("SELECT COUNT(*) FROM oauth_clients", [], |r| r.get(0))?;
    if rows.max(0) as usize >= MAX_OAUTH_CLIENTS {
        let evicted = conn.execute(
            "DELETE FROM oauth_clients WHERE id = (SELECT id FROM oauth_clients c
              WHERE NOT EXISTS (SELECT 1 FROM api_grants g WHERE g.client_id = c.client_id) ORDER BY created_at, id LIMIT 1)",
            [],
        )?;
        if evicted == 0 {
            return Ok(Err(Refused::Full));
        }
    }
    let client_id = generate_token();
    conn.execute(
        "INSERT INTO oauth_clients (client_id, name, redirect_uris, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![client_id, name, redirect_uris.join("\n"), now],
    )?;
    Ok(Ok(client_id))
}

// ---------- PKCE ----------

/// Unpadded base64url (RFC 4648 §5), for the S256 challenge.
pub fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let (b0, b1, b2) = (chunk.first().copied().unwrap_or(0), chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0));
        let n = (u32::from(b0) << 16) | (u32::from(b1) << 8) | u32::from(b2);
        for i in 0..=chunk.len() {
            let six = ((n >> (18 - 6 * i)) & 63) as usize;
            out.push(char::from(ALPHABET[six])); // panic-ok: a six-bit value is < 64
        }
    }
    out
}

/// A verifier as RFC 7636 §4.1 allows: 43 to 128 unreserved characters.
pub fn valid_verifier(v: &str) -> bool {
    (43..=128).contains(&v.len()) && v.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b))
}

/// An S256 challenge: the unpadded base64url of a SHA-256, 43 characters.
pub fn valid_challenge(c: &str) -> bool {
    c.len() == 43 && c.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// The S256 challenge of `verifier`.
pub fn s256(verifier: &str) -> String {
    use sha2::{Digest, Sha256};
    base64url(&Sha256::digest(verifier.as_bytes()))
}

/// `verifier` answers `challenge`.
pub fn pkce_ok(challenge: &str, verifier: &str) -> bool {
    valid_verifier(verifier) && s256(verifier) == challenge
}

// ---------- the authorization request ----------

/// The fields of an authorization request, from `GET /oauth/authorize`'s
/// query or the consent form's hidden fields.
#[derive(Default, Clone, Debug)]
pub struct AuthorizeParams {
    pub response_type: Option<String>,
    pub client_id: Option<String>,
    pub redirect_uri: Option<String>,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
    pub state: Option<String>,
    pub resource: Option<String>,
    pub scope: Option<String>,
}

impl AuthorizeParams {
    /// The request's fields from name-value pairs; other names are ignored.
    /// A field given twice is refused (RFC 6749 §3.1).
    pub fn from_pairs(pairs: &[(String, String)]) -> Result<AuthorizeParams, AuthorizeError> {
        let mut p = AuthorizeParams::default();
        for (k, v) in pairs {
            let slot = match k.as_str() {
                "response_type" => &mut p.response_type,
                "client_id" => &mut p.client_id,
                "redirect_uri" => &mut p.redirect_uri,
                "code_challenge" => &mut p.code_challenge,
                "code_challenge_method" => &mut p.code_challenge_method,
                "state" => &mut p.state,
                "resource" => &mut p.resource,
                "scope" => &mut p.scope,
                _ => continue,
            };
            if slot.is_some() {
                return Err(bad("duplicate"));
            }
            *slot = Some(v.clone());
        }
        Ok(p)
    }
}

/// A request that passed every check and may go on to the consent page.
#[derive(Clone, Debug)]
pub struct Authorize {
    pub client: Client,
    pub redirect_uri: String,
    /// The registered address it matched: the consent page names its host.
    pub registered_uri: String,
    pub code_challenge: String,
    pub state: String,
    pub resource: String,
    pub scopes: Vec<Scope>,
}

impl Authorize {
    /// The request's fields, as the consent form sends them back.
    pub fn hidden_fields(&self) -> Vec<(&'static str, String)> {
        vec![
            ("response_type", "code".into()),
            ("client_id", self.client.client_id.clone()),
            ("redirect_uri", self.redirect_uri.clone()),
            ("code_challenge", self.code_challenge.clone()),
            ("code_challenge_method", "S256".into()),
            ("state", self.state.clone()),
            ("resource", self.resource.clone()),
            ("scope", Scope::join(&self.scopes)),
        ]
    }
}

/// Why an authorization request is refused: the OAuth error code and the
/// i18n key (`oauth.error.<what>`) of the sentence the error page shows.
/// Nothing is ever sent back to the app for one of these.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct AuthorizeError {
    pub error: &'static str,
    pub what: &'static str,
}

pub fn bad(what: &'static str) -> AuthorizeError {
    AuthorizeError { error: "invalid_request", what }
}

/// The resources (RFC 8707) this server serves: its address (`issuer`,
/// the public address and the base path) and its `/mcp`.
pub fn resources(issuer: &str) -> [String; 2] {
    [issuer.to_string(), format!("{issuer}/mcp")]
}

/// `resource` is one this server serves, a trailing slash aside: the one
/// it names, as served.
pub fn served_resource(resource: &str, issuer: &str) -> Option<String> {
    let r = resource.strip_suffix('/').unwrap_or(resource);
    resources(issuer).into_iter().find(|s| s == r)
}

/// Check an authorization request against its app (`None`: unknown) and
/// this server's address. The app and the redirect address first: until
/// both are known good, nothing about the request is trusted.
pub fn check_authorize(p: &AuthorizeParams, client: Option<Client>, issuer: &str) -> Result<Authorize, AuthorizeError> {
    let some = |v: &Option<String>| v.as_deref().filter(|s| !s.is_empty()).map(str::to_string);
    let Some(client) = client else {
        return Err(AuthorizeError { error: if some(&p.client_id).is_some() { "unauthorized_client" } else { "invalid_request" }, what: "client" });
    };
    let redirect_uri = some(&p.redirect_uri).ok_or(bad("redirect_uri"))?;
    let registered_uri = client.registered_for(&redirect_uri).ok_or(bad("redirect_uri"))?.to_string();
    if p.response_type.as_deref() != Some("code") {
        return Err(AuthorizeError { error: "unsupported_response_type", what: "response_type" });
    }
    let code_challenge = some(&p.code_challenge).filter(|c| valid_challenge(c)).ok_or(bad("code_challenge"))?;
    if p.code_challenge_method.as_deref() != Some("S256") {
        return Err(bad("code_challenge_method"));
    }
    let state = some(&p.state).filter(|s| s.len() <= MAX_STATE_LEN).ok_or(bad("state"))?;
    let resource = some(&p.resource).and_then(|r| served_resource(&r, issuer)).ok_or(AuthorizeError { error: "invalid_target", what: "resource" })?;
    let scopes = match some(&p.scope) {
        None => Scope::ALL.to_vec(),
        Some(s) => {
            let named: Vec<&str> = s.split_whitespace().collect();
            if named.is_empty() || !named.iter().all(|n| Scope::parse(n).is_some()) {
                return Err(AuthorizeError { error: "invalid_scope", what: "scope" });
            }
            Scope::parse_list(&s)
        }
    };
    Ok(Authorize { client, redirect_uri, registered_uri, code_challenge, state, resource, scopes })
}

// ---------- Allow: the grant and its code ----------

/// What the person chose on the consent page.
pub struct Consent<'a> {
    pub user_id: i64,
    /// The account's own courses it picked (the caller keeps only those).
    pub courses: &'a [String],
    pub lifetime: Lifetime,
}

/// Allow: one grant (counted against the account's cap) and a one-time
/// code that points at it, in one transaction. The code itself is returned
/// this once; only its hash is stored. `Refused::Full` also when the server
/// holds `MAX_OAUTH_CODES` live codes.
pub fn allow(conn: &mut Connection, a: &Authorize, c: &Consent, now: i64) -> rusqlite::Result<Result<String, Refused>> {
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM oauth_codes WHERE expires_at <= ?1", [now])?;
    let live: i64 = tx.query_row("SELECT COUNT(*) FROM oauth_codes", [], |r| r.get(0))?;
    if live.max(0) as usize >= MAX_OAUTH_CODES {
        return Ok(Err(Refused::Full));
    }
    let g = NewGrant { client_id: &a.client.client_id, client_name: &a.client.name, scopes: &a.scopes, courses: c.courses, lifetime: c.lifetime };
    let grant = match api::create_grant(&tx, c.user_id, &g, now)? {
        Ok(id) => id,
        Err(e) => return Ok(Err(e)),
    };
    let code = generate_token();
    tx.execute(
        "INSERT INTO oauth_codes (code_hash, client_id, user_id, grant_id, redirect_uri, resource, code_challenge, expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![hash_token(&code), a.client.client_id, c.user_id, grant, a.redirect_uri, a.resource, a.code_challenge, now + CODE_TTL_SECS],
    )?;
    tx.execute("UPDATE oauth_clients SET last_grant_at = ?2 WHERE client_id = ?1", params![a.client.client_id, now])?;
    tx.commit()?;
    Ok(Ok(code))
}

// ---------- the token endpoint ----------

/// Why the token endpoint says no (RFC 6749 §5.2). The answer carries the
/// code only.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum TokenError {
    InvalidRequest,
    InvalidClient,
    InvalidGrant,
    UnsupportedGrantType,
    InvalidTarget,
    /// The token table is full (`MAX_API_TOKENS`).
    Unavailable,
}

impl TokenError {
    pub fn code(self) -> &'static str {
        match self {
            TokenError::InvalidRequest => "invalid_request",
            TokenError::InvalidClient => "invalid_client",
            TokenError::InvalidGrant => "invalid_grant",
            TokenError::UnsupportedGrantType => "unsupported_grant_type",
            TokenError::InvalidTarget => "invalid_target",
            TokenError::Unavailable => "temporarily_unavailable",
        }
    }
}

/// A new access and refresh token, and the scopes they carry.
#[derive(Debug)]
pub struct Tokens {
    pub access: String,
    pub refresh: String,
    pub expires_in: i64,
    pub scopes: Vec<Scope>,
}

/// What a code is traded with: the token request repeats the client, the
/// redirect address and the resource, and brings the PKCE verifier.
pub struct CodeExchange<'a> {
    pub code: &'a str,
    pub client_id: &'a str,
    pub redirect_uri: &'a str,
    pub code_verifier: &'a str,
    pub resource: &'a str,
}

/// The grant still works for its account: not expired, and the account has
/// Create courses and no temporary password waiting.
const GRANT_LIVE: &str = "g.expires_at > ?2 AND (u.owner = 1 OR (u.perms & ?3) != 0) AND u.must_change_password = 0";

fn create_courses_bit() -> i64 {
    Perm::CreateCourses.bit()
}

/// An access token (an hour, never past the grant) and a refresh token (as
/// long as the grant) for `grant_id`.
fn issue(conn: &Connection, grant_id: i64, grant_expires: i64, scopes: Vec<Scope>, now: i64) -> rusqlite::Result<Result<Tokens, TokenError>> {
    let access_until = (now + ACCESS_TTL_SECS).min(grant_expires);
    let Ok(access) = api::mint(conn, grant_id, Kind::Access, access_until)? else { return Ok(Err(TokenError::Unavailable)) };
    let Ok(refresh) = api::mint(conn, grant_id, Kind::Refresh, grant_expires)? else { return Ok(Err(TokenError::Unavailable)) };
    Ok(Ok(Tokens { access, refresh, expires_in: access_until - now, scopes }))
}

/// Trade a code for its grant's first tokens. A code works once: a second
/// use deletes the grant it made, and with it every token under it (the
/// code may have been stolen, OAuth 2.1 §4.1.3).
pub fn exchange_code(conn: &mut Connection, x: &CodeExchange, now: i64) -> rusqlite::Result<Result<Tokens, TokenError>> {
    let tx = conn.transaction()?;
    let sql = format!(
        "SELECT c.grant_id, c.client_id, c.redirect_uri, c.code_challenge, c.resource, c.expires_at, c.used_at, g.scopes, g.expires_at, {GRANT_LIVE}
         FROM oauth_codes c JOIN api_grants g ON g.id = c.grant_id JOIN users u ON u.id = g.user_id WHERE c.code_hash = ?1"
    );
    let hash = hash_token(x.code);
    let row = tx
        .query_row(&sql, params![hash, now, create_courses_bit()], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, Option<i64>>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, i64>(8)?,
                r.get::<_, bool>(9)?,
            ))
        })
        .optional()?;
    let Some((grant_id, client_id, redirect_uri, challenge, resource, expires_at, used_at, scopes, grant_expires, live)) = row else {
        return Ok(Err(TokenError::InvalidGrant));
    };
    if used_at.is_some() {
        tx.execute("DELETE FROM api_grants WHERE id = ?1", [grant_id])?;
        tx.commit()?;
        tracing::warn!("an OAuth code was used twice: grant {grant_id} and its tokens were revoked");
        return Ok(Err(TokenError::InvalidGrant));
    }
    if expires_at <= now || client_id != x.client_id || redirect_uri != x.redirect_uri || !pkce_ok(&challenge, x.code_verifier) || !live {
        return Ok(Err(TokenError::InvalidGrant));
    }
    if x.resource.strip_suffix('/').unwrap_or(x.resource) != resource {
        return Ok(Err(TokenError::InvalidTarget));
    }
    tx.execute("UPDATE oauth_codes SET used_at = ?2 WHERE code_hash = ?1", params![hash, now])?;
    let tokens = match issue(&tx, grant_id, grant_expires, Scope::parse_list(&scopes), now)? {
        Ok(t) => t,
        Err(e) => return Ok(Err(e)),
    };
    tx.commit()?;
    Ok(Ok(tokens))
}

/// Trade a refresh token for a new pair (rotation with reuse detection,
/// RFC 9700 §4.14.2, #184). Only the client the grant was made for may use
/// it: another client's attempt is `invalid_grant` and changes nothing,
/// whether the token is used or not. The token is then marked used, not
/// deleted, so it is still recognised: presented again, it revokes the
/// grant and every token under it (one of the two holders stole it), as a
/// replayed code does. A grant keeps only its newest used refresh token,
/// and of its access tokens only the newest, still working until its hour
/// ends, so a client that refreshes in a loop cannot fill the token table:
/// a grant holds at most four token rows.
pub fn refresh(conn: &mut Connection, token: &str, client_id: &str, now: i64) -> rusqlite::Result<Result<Tokens, TokenError>> {
    let tx = conn.transaction()?;
    let sql = format!(
        "SELECT t.grant_id, g.client_id, g.scopes, g.expires_at, t.used_at, g.user_id, {GRANT_LIVE}
         FROM api_tokens t JOIN api_grants g ON g.id = t.grant_id JOIN users u ON u.id = g.user_id
         WHERE t.token_hash = ?1 AND t.kind = 'refresh' AND t.expires_at > ?2"
    );
    let hash = hash_token(token);
    let found = tx
        .query_row(&sql, params![hash, now, create_courses_bit()], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, Option<i64>>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, bool>(6)?,
            ))
        })
        .optional()?;
    let Some((grant_id, owner, scopes, grant_expires, used_at, user_id, live)) = found else { return Ok(Err(TokenError::InvalidGrant)) };
    if owner != client_id {
        return Ok(Err(TokenError::InvalidGrant));
    }
    if used_at.is_some() {
        tx.execute("DELETE FROM api_grants WHERE id = ?1", [grant_id])?;
        tx.commit()?;
        tracing::warn!("an OAuth refresh token was used twice: grant {grant_id} of account {user_id} and its tokens were revoked");
        return Ok(Err(TokenError::InvalidGrant));
    }
    if !live {
        return Ok(Err(TokenError::InvalidGrant));
    }
    tx.execute("DELETE FROM api_tokens WHERE grant_id = ?1 AND kind = 'refresh' AND used_at IS NOT NULL", [grant_id])?;
    tx.execute("UPDATE api_tokens SET used_at = ?2 WHERE token_hash = ?1", params![hash, now])?;
    tx.execute(
        "DELETE FROM api_tokens WHERE grant_id = ?1 AND kind = 'access'
         AND id NOT IN (SELECT id FROM api_tokens WHERE grant_id = ?1 AND kind = 'access' ORDER BY id DESC LIMIT 1)",
        [grant_id],
    )?;
    let tokens = match issue(&tx, grant_id, grant_expires, Scope::parse_list(&scopes), now)? {
        Ok(t) => t,
        Err(e) => return Ok(Err(e)),
    };
    tx.commit()?;
    Ok(Ok(tokens))
}

/// Revoke (RFC 7009): the app disconnects itself. The grant behind an
/// access or refresh token goes, with every token under it. A token another
/// client holds, a personal token and an unknown one are left alone.
/// `true` when a grant was revoked.
pub fn revoke(conn: &Connection, token: &str, client_id: &str) -> rusqlite::Result<bool> {
    let grant: Option<i64> = conn
        .query_row(
            "SELECT g.id FROM api_tokens t JOIN api_grants g ON g.id = t.grant_id
             WHERE t.token_hash = ?1 AND t.kind IN ('access', 'refresh') AND g.client_id = ?2",
            params![hash_token(token), client_id],
            |r| r.get(0),
        )
        .optional()?;
    match grant {
        Some(id) => Ok(conn.execute("DELETE FROM api_grants WHERE id = ?1", [id])? > 0),
        None => Ok(false),
    }
}

/// Housekeeping (with `api::purge_expired`, hourly and at startup): expired
/// codes, grants an Allow made whose code was never traded (no token and no
/// code left), and registrations nobody finished a grant for within a day.
pub fn purge(conn: &Connection, now: i64) -> rusqlite::Result<usize> {
    let codes = conn.execute("DELETE FROM oauth_codes WHERE expires_at <= ?1", [now])?;
    let grants = conn.execute(
        "DELETE FROM api_grants WHERE client_id IN (SELECT client_id FROM oauth_clients)
         AND NOT EXISTS (SELECT 1 FROM api_tokens t WHERE t.grant_id = api_grants.id)
         AND NOT EXISTS (SELECT 1 FROM oauth_codes c WHERE c.grant_id = api_grants.id)",
        [],
    )?;
    Ok(codes + grants + purge_clients(conn, now)?)
}

#[cfg(test)]
mod tests;
