//! Sign-in through a reverse proxy's identity header (#642).
//!
//! A self-hoster who already signs people in at the proxy (Authelia,
//! Authentik, Tailscale Serve, Cloudflare Access) sets two settings, read as
//! they are (no older spelling):
//!
//! - `SKRONGLE_PROXY_AUTH_HEADER`: the request header naming the person, by
//!   e-mail address (`Remote-Email`, `Tailscale-User-Login`, …);
//! - `SKRONGLE_PROXY_AUTH_FROM`: the proxy's addresses, comma-separated, each
//!   an IP address or a CIDR range.
//!
//! Both or neither: one without the other stops the server at startup.
//!
//! With both set, [`middleware`] runs before every handler. A request whose
//! TCP peer (the socket's own address, never `X-Forwarded-For`, which the
//! proxy may have been lied to about) is in the list and that carries the
//! header is signed in as the account with that address (case-insensitive):
//! a row in `sessions` and the `sid` cookie, as a password sign-in makes,
//! with no two-factor step and no rate limit. A session for a different
//! account is replaced. With no such account the sign-up rules apply
//! (`SKRONGLE_SIGNUP`, `signup.rs`): `open` makes an account with no
//! password, its address confirmed (the proxy vouched for it), and the Terms
//! gate asks for acceptance as for any account; `invite` and `closed` answer
//! with the closed page. The same header from any other peer is ignored and
//! logged (at most once an hour per address).
//!
//! The request continues with the new session in its `Cookie` header, so
//! every extractor downstream (`CurrentUser`, `MaybeUser`, `SessionToken`)
//! sees the account without knowing about the proxy. `/dev/*` (dev mode's
//! `/dev/login` among them), `/static/*` and `/logout` are left alone.

use axum::extract::{ConnectInfo, Request, State};
use axum::http::header::COOKIE;
use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};

use crate::signup::{self, Refusal};
use crate::AppState;

/// The two settings.
pub const HEADER_VAR: &str = "SKRONGLE_PROXY_AUTH_HEADER";
pub const FROM_VAR: &str = "SKRONGLE_PROXY_AUTH_FROM";

/// How often an untrusted address carrying the header is logged.
const WARN_EVERY_SECS: i64 = 3600;
/// Addresses remembered for that; past it the memory starts over.
const WARN_KEYS: usize = 1024;

/// An address range: `10.0.0.0/8`, or one address (`/32`, `/128`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Net {
    addr: IpAddr,
    prefix: u8,
}

impl Net {
    pub fn parse(s: &str) -> Result<Net, String> {
        let s = s.trim();
        let (addr, prefix) = match s.split_once('/') {
            Some((a, p)) => (a, Some(p)),
            None => (s, None),
        };
        let addr: IpAddr = addr.trim().parse().map_err(|_| format!("{FROM_VAR}: {s:?} is not an IP address or CIDR range"))?;
        let addr = canonical(addr);
        let max = if addr.is_ipv4() { 32 } else { 128 };
        let prefix = match prefix {
            None => max,
            Some(p) => match p.trim().parse::<u8>() {
                Ok(p) if p <= max => p,
                _ => return Err(format!("{FROM_VAR}: {s:?} has a prefix length that is not 0-{max}")),
            },
        };
        Ok(Net { addr, prefix })
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        match (self.addr, canonical(ip)) {
            (IpAddr::V4(net), IpAddr::V4(ip)) => {
                let mask = u32::MAX.checked_shl(32 - u32::from(self.prefix)).unwrap_or(0);
                u32::from(net) & mask == u32::from(ip) & mask
            }
            (IpAddr::V6(net), IpAddr::V6(ip)) => {
                let mask = u128::MAX.checked_shl(128 - u32::from(self.prefix)).unwrap_or(0);
                u128::from(net) & mask == u128::from(ip) & mask
            }
            _ => false,
        }
    }
}

impl std::fmt::Display for Net {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let max = if self.addr.is_ipv4() { 32 } else { 128 };
        if self.prefix == max { write!(f, "{}", self.addr) } else { write!(f, "{}/{}", self.addr, self.prefix) }
    }
}

/// An IPv4-mapped IPv6 address (`::ffff:10.0.0.1`, a dual-stack listener)
/// counts as its IPv4 address.
fn canonical(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(ip, IpAddr::V4),
        v4 => v4,
    }
}

/// The settings, when on.
#[derive(Debug)]
pub struct Config {
    pub header: HeaderName,
    pub from: Vec<Net>,
    /// When each untrusted address carrying the header was last logged.
    warned: Mutex<HashMap<IpAddr, i64>>,
}

impl Config {
    /// The two settings' values (unset or blank is `None`): `Ok(None)` when
    /// both are unset, an error naming the variables when only one is set
    /// or a value does not parse.
    pub fn parse(header: Option<&str>, from: Option<&str>) -> Result<Option<Config>, String> {
        let header = header.map(str::trim).filter(|s| !s.is_empty());
        let from = from.map(str::trim).filter(|s| !s.is_empty());
        let (header, from) = match (header, from) {
            (None, None) => return Ok(None),
            (Some(_), None) => return Err(format!("{HEADER_VAR} is set but {FROM_VAR} is not: set both (the proxy's addresses are required) or neither")),
            (None, Some(_)) => return Err(format!("{FROM_VAR} is set but {HEADER_VAR} is not: set both or neither")),
            (Some(h), Some(f)) => (h, f),
        };
        let header = HeaderName::from_bytes(header.to_ascii_lowercase().as_bytes()).map_err(|_| format!("{HEADER_VAR}: {header:?} is not a header name"))?;
        let from = from.split(',').filter(|s| !s.trim().is_empty()).map(Net::parse).collect::<Result<Vec<_>, _>>()?;
        if from.is_empty() {
            return Err(format!("{FROM_VAR} names no address"));
        }
        Ok(Some(Config { header, from, warned: Mutex::new(HashMap::new()) }))
    }

    /// From the environment.
    pub fn from_env() -> Result<Option<Config>, String> {
        Config::parse(std::env::var(HEADER_VAR).ok().as_deref(), std::env::var(FROM_VAR).ok().as_deref())
    }

    /// The startup log line.
    pub fn describe(&self) -> String {
        let from: Vec<String> = self.from.iter().map(Net::to_string).collect();
        format!(
            "proxy sign-in: requests from {} carrying the {} header are signed in as the account with that e-mail address; /login and /register only say so ({HEADER_VAR}, {FROM_VAR})",
            from.join(", "),
            self.header
        )
    }

    /// Too wide to mean "the proxy": every address, or most of them.
    pub fn too_wide(&self) -> bool {
        self.from.iter().any(|n| n.prefix < 8)
    }

    pub fn trusts(&self, ip: IpAddr) -> bool {
        self.from.iter().any(|n| n.contains(ip))
    }

    /// Log a request carrying the header from `ip`, an address not in the
    /// list, unless it was logged within the hour. `true` when logged.
    fn warn_untrusted(&self, ip: Option<IpAddr>, now: i64) -> bool {
        let key = ip.map_or(IpAddr::from([0u8, 0, 0, 0]), canonical);
        let mut seen = self.warned.lock().unwrap_or_else(|p| p.into_inner());
        if seen.get(&key).is_some_and(|t| now - t < WARN_EVERY_SECS) {
            return false;
        }
        if seen.len() >= WARN_KEYS {
            seen.clear();
        }
        seen.insert(key, now);
        let who = ip.map_or_else(|| "an unknown address".to_string(), |ip| ip.to_string());
        tracing::warn!("ignored the {} header from {who}: not in {FROM_VAR} (logged once an hour per address)", self.header);
        true
    }
}

/// The app path of a request: without the base path prefix, if any.
fn app_path(req: &Request) -> &str {
    let path = req.uri().path();
    path.strip_prefix(crate::base_path()).filter(|p| p.starts_with('/')).unwrap_or(path)
}

/// Paths the proxy's header never signs in on.
fn left_alone(path: &str) -> bool {
    (cfg!(feature = "dev") && path.starts_with("/dev/")) || path.starts_with("/static/") || path == "/logout"
}

/// What the header asks for.
enum Outcome {
    /// The session on the request is already this account's.
    Already,
    /// A new session (its token) for this account.
    SignedIn(i64, String),
    /// A new account was made, with a session.
    Created(i64, String),
    /// No account, and none may be made.
    Refused(Refusal),
    /// The header's value is not an e-mail address an account could have.
    NotAnAddress,
}

/// The account named `value`, signing in or making one per `mode`.
/// `session` is the request's session token, if any; a session of another
/// account is ended.
fn resolve(conn: &mut Connection, value: &str, session: Option<&str>, mode: signup::Mode, now: i64, dev: bool) -> rusqlite::Result<Outcome> {
    let current: Option<i64> = match session {
        Some(t) => conn
            .query_row(
                "SELECT user_id FROM sessions WHERE session_token = ?1 AND expires_at > ?2",
                params![crate::auth::hash_token(t), now],
                |r| r.get(0),
            )
            .optional()?,
        None => None,
    };
    let account: Option<i64> = conn
        .query_row("SELECT id FROM users WHERE lower(email) = lower(?1) AND email <> '' ORDER BY id LIMIT 1", [value], |r| r.get(0))
        .optional()?;
    if account.is_some() && account == current {
        return Ok(Outcome::Already);
    }
    let tx = conn.transaction()?;
    if let (Some(t), Some(_)) = (session, current) {
        crate::auth::delete_session(&tx, t)?;
    }
    let outcome = match account {
        Some(id) => Outcome::SignedIn(id, crate::auth::create_session(&tx, id)?),
        None => {
            let Ok(email) = crate::profile::validate_email(value) else {
                tx.commit()?;
                return Ok(Outcome::NotAnAddress);
            };
            let admitted = match signup::admit(&tx, mode, "", Some(&email), now, dev)? {
                Ok(a) => a,
                Err(r) => {
                    tx.commit()?;
                    return Ok(Outcome::Refused(r));
                }
            };
            // No password; the proxy vouched for the address.
            tx.execute(
                "INSERT INTO users (password_hash, created_at, email, email_verified_at, perms, preset_id) VALUES ('', ?1, ?2, ?1, ?3, ?4)",
                params![now, email, crate::perms::Perms::TEACHER.bits(), crate::perms::TEACHER_PRESET],
            )?;
            let id = tx.last_insert_rowid();
            if !signup::finish(&tx, admitted, id, now)? {
                return Ok(Outcome::Refused(Refusal::Used));
            }
            Outcome::Created(id, crate::auth::create_session(&tx, id)?)
        }
    };
    tx.commit()?;
    Ok(outcome)
}

/// The request's `Cookie` header with `sid` set to `token` (any other
/// `sid` dropped, the other cookies kept).
fn with_session(req: &mut Request, token: &str) {
    let mut pairs: Vec<String> = req
        .headers()
        .get_all(COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .map(str::trim)
        .filter(|p| !p.is_empty() && p.split_once('=').is_none_or(|(k, _)| k.trim() != crate::auth::SESSION_COOKIE))
        .map(str::to_string)
        .collect();
    pairs.push(format!("{}={token}", crate::auth::SESSION_COOKIE));
    let value = HeaderValue::from_str(&pairs.join("; "))
        .or_else(|_| HeaderValue::from_str(&format!("{}={token}", crate::auth::SESSION_COOKIE)));
    req.headers_mut().remove(COOKIE);
    if let Ok(v) = value {
        req.headers_mut().insert(COOKIE, v);
    }
}

/// The middleware (see the module comment). A no-op without the settings.
pub async fn middleware(State(state): State<Arc<AppState>>, mut req: Request, next: Next) -> Response {
    let Some(cfg) = state.proxy_auth.as_ref() else { return next.run(req).await };
    if left_alone(app_path(&req)) {
        return next.run(req).await;
    }
    let Some(value) = req.headers().get(&cfg.header) else { return next.run(req).await };
    let peer = req.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.0.ip());
    if !peer.is_some_and(|ip| cfg.trusts(ip)) {
        cfg.warn_untrusted(peer, crate::db::now());
        return next.run(req).await;
    }
    let value = value.to_str().unwrap_or_default().trim().to_string();
    if value.is_empty() {
        return next.run(req).await;
    }
    let session = crate::auth::session_cookie_from_headers(req.headers());
    let (mode, dev) = (state.signup, state.dev_mode());
    let outcome = state.db.call(move |c| resolve(c, &value, session.as_deref(), mode, crate::db::now(), dev)).await;
    let token = match outcome {
        Err(e) => return e.into_response(),
        Ok(Outcome::Already) => return next.run(req).await,
        Ok(Outcome::NotAnAddress) => {
            tracing::warn!("the {} header from the proxy is not an e-mail address; nobody was signed in", cfg.header);
            return next.run(req).await;
        }
        Ok(Outcome::Refused(r)) => {
            tracing::info!("proxy sign-in: no account for the address the proxy sent, and sign-ups are {} ({})", state.signup.as_str(), signup::VAR);
            return crate::routes::auth::signup_refused(r, StatusCode::FORBIDDEN);
        }
        Ok(Outcome::SignedIn(id, token)) => {
            tracing::info!("user {id} signed in through the proxy");
            token
        }
        Ok(Outcome::Created(id, token)) => {
            tracing::info!("a new user (id {id}) was made through the proxy");
            token
        }
    };
    with_session(&mut req, &token);
    let mut res = next.run(req).await;
    let (name, value) = crate::auth::set_session_cookie(&token, state.secure_cookies);
    res.headers_mut().append(name, value);
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_set_settings_refuse_to_start() {
        assert!(Config::parse(None, None).unwrap().is_none());
        assert!(Config::parse(Some(" "), Some("")).unwrap().is_none(), "blank is unset");
        let e = Config::parse(Some("Remote-Email"), None).unwrap_err();
        assert!(e.contains(HEADER_VAR) && e.contains(FROM_VAR), "{e}");
        let e = Config::parse(None, Some("127.0.0.1")).unwrap_err();
        assert!(e.contains(HEADER_VAR) && e.contains(FROM_VAR), "{e}");
        assert!(Config::parse(Some("Remote-Email"), Some("not-an-ip")).unwrap_err().contains(FROM_VAR));
        assert!(Config::parse(Some("Remote-Email"), Some("10.0.0.0/33")).is_err());
        assert!(Config::parse(Some("Remote-Email"), Some(" , ")).is_err());
        assert!(Config::parse(Some("bad header"), Some("127.0.0.1")).unwrap_err().contains(HEADER_VAR));
        let c = Config::parse(Some("Remote-Email"), Some("127.0.0.1, 10.0.0.0/8, fd7a:115c:a1e0::/48")).unwrap().unwrap();
        assert_eq!(c.header.as_str(), "remote-email");
        assert_eq!(c.from.len(), 3);
        assert!(c.describe().contains("127.0.0.1, 10.0.0.0/8, fd7a:115c:a1e0::/48"), "{}", c.describe());
        assert!(!c.too_wide());
        assert!(Config::parse(Some("X"), Some("0.0.0.0/0")).unwrap().unwrap().too_wide());
    }

    /// The proxy makes accounts up to the cap and refuses the next one;
    /// someone who already has an account still signs in.
    #[test]
    fn the_proxy_makes_no_ninth_account() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let now = 100;
            for n in 0..crate::caps::MAX_ACCOUNTS {
                let o = resolve(c, &format!("u{n}@example.org"), None, signup::Mode::Open, now, false)?;
                assert!(matches!(o, Outcome::Created(..)), "account {n}");
            }
            let o = resolve(c, "late@example.org", None, signup::Mode::Open, now, false)?;
            assert!(matches!(o, Outcome::Refused(Refusal::Full)));
            let count: i64 = c.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))?;
            assert_eq!(count, crate::caps::MAX_ACCOUNTS as i64);
            let o = resolve(c, "u0@example.org", None, signup::Mode::Open, now, false)?;
            assert!(matches!(o, Outcome::SignedIn(..)), "an existing account still signs in");
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn address_ranges() {
        let ip = |s: &str| s.parse::<IpAddr>().unwrap();
        let net = Net::parse("10.1.0.0/16").unwrap();
        assert!(net.contains(ip("10.1.2.3")) && !net.contains(ip("10.2.0.1")));
        assert!(net.contains(ip("::ffff:10.1.9.9")), "IPv4-mapped counts as IPv4");
        let one = Net::parse("127.0.0.1").unwrap();
        assert!(one.contains(ip("127.0.0.1")) && !one.contains(ip("127.0.0.2")));
        assert!(Net::parse("0.0.0.0/0").unwrap().contains(ip("8.8.8.8")));
        let v6 = Net::parse("fd7a:115c:a1e0::/48").unwrap();
        assert!(v6.contains(ip("fd7a:115c:a1e0:ab12::1")) && !v6.contains(ip("fd7a:115c:a1e1::1")));
        assert!(!v6.contains(ip("10.0.0.1")));
        assert!(Net::parse("::1").unwrap().contains(ip("::1")));
    }

    #[test]
    fn untrusted_addresses_are_logged_once_an_hour() {
        let c = Config::parse(Some("Remote-Email"), Some("10.0.0.1")).unwrap().unwrap();
        let a = Some("192.0.2.1".parse().unwrap());
        assert!(c.warn_untrusted(a, 1000));
        assert!(!c.warn_untrusted(a, 1000 + WARN_EVERY_SECS - 1));
        assert!(c.warn_untrusted(Some("192.0.2.2".parse().unwrap()), 1000));
        assert!(c.warn_untrusted(a, 1000 + WARN_EVERY_SECS));
    }
}
