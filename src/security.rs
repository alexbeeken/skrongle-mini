//! Request-level protections that apply to the whole app: security headers
//! on every response, a cross-site check on every state-changing request
//! (CSRF), and small in-memory rate limits for the account forms.

use axum::extract::{ConnectInfo, Request};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Mutex;

// ---------- headers ----------

/// The page policy. Scripts and styles come from this origin; the inline
/// `<script>` blocks and `on…=` attributes in the templates need
/// `'unsafe-inline'` (moving them into files is listed in
/// docs/security-review.md). Images may come from https URLs because a theme
/// may name its art by URL. Nothing may frame the app (the dev device page
/// `/login/code/device`, dev mode only, lets `/login/code` frame it).
pub const CSP: &str = "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; \
img-src 'self' data: https:; font-src 'self'; connect-src 'self'; media-src 'self'; object-src 'none'; \
base-uri 'self'; frame-ancestors 'none'";

/// Adds the security headers a handler did not set itself (the picture
/// routes set a stricter CSP of their own). `Strict-Transport-Security` only
/// when the site is served over HTTPS (`SKRONGLE_SECURE_COOKIES=1`).
pub async fn headers(secure: bool, req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    let mut set = |name: header::HeaderName, value: &'static str| {
        if !h.contains_key(&name) {
            h.insert(name, HeaderValue::from_static(value));
        }
    };
    set(header::CONTENT_SECURITY_POLICY, CSP);
    set(header::X_FRAME_OPTIONS, "DENY");
    set(header::X_CONTENT_TYPE_OPTIONS, "nosniff");
    set(header::REFERRER_POLICY, "same-origin");
    if secure {
        set(header::STRICT_TRANSPORT_SECURITY, "max-age=31536000");
    }
    res
}

/// What to warn a self-hoster about at startup: cookies and passwords sent
/// without TLS.
pub fn https_warnings(bind: SocketAddr, secure_cookies: bool, public_url: Option<&str>) -> Vec<String> {
    let mut out = Vec::new();
    if secure_cookies {
        return out;
    }
    if public_url.is_some_and(|u| u.starts_with("https://")) {
        out.push("SKRONGLE_PUBLIC_URL is https:// but SKRONGLE_SECURE_COOKIES is not 1: set it so session cookies are only sent over HTTPS".into());
    } else if !bind.ip().is_loopback() {
        out.push(format!(
            "serving plain HTTP on {bind}: passwords and session cookies cross the network unencrypted. Fine on a home network you trust; \
             anywhere else put a TLS proxy (Caddy, nginx) in front and set SKRONGLE_SECURE_COOKIES=1"
        ));
    }
    out
}

// ---------- CSRF ----------

/// Did a browser send this request from another site? Browsers say so in
/// `Sec-Fetch-Site`; older ones only send `Origin`, compared with the host
/// the request was made to. A request with neither is not from a browser
/// form or script (curl, an agent with the session cookie) and passes.
pub fn is_cross_site(headers: &HeaderMap) -> bool {
    let get = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
    if let Some(site) = get("sec-fetch-site") {
        return !matches!(site, "same-origin" | "none");
    }
    let Some(origin) = get("origin") else { return false };
    let Some(authority) = origin.split_once("://").map(|(_, rest)| rest) else {
        return true; // "null" (sandboxed frames, some redirects) or garbage
    };
    let hosts = [get("x-forwarded-host"), get("host")];
    !hosts.iter().flatten().any(|h| h.eq_ignore_ascii_case(authority))
}

/// Refuses state-changing requests that come from another site, so a page
/// elsewhere cannot submit the app's forms with the learner's cookie (or
/// log them into someone else's account). `SameSite=Lax` on the session
/// cookie already stops most of this; the check also covers login forms,
/// sibling subdomains and browsers without SameSite.
pub async fn csrf(req: Request, next: Next) -> Response {
    let safe = matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS);
    if !safe && is_cross_site(req.headers()) {
        tracing::warn!("refused a cross-site {} {}", req.method(), req.uri().path());
        return (StatusCode::FORBIDDEN, crate::t("error.cross_site")).into_response();
    }
    next.run(req).await
}

// ---------- client address ----------

/// The client's IP address: the socket peer, or, when the peer is a proxy
/// on this machine (nginx, Caddy), the address it appended last to
/// `X-Forwarded-For`. Earlier entries are whatever the client sent and are
/// not trusted.
pub fn client_ip(headers: &HeaderMap, peer: Option<SocketAddr>) -> Option<IpAddr> {
    let peer = peer?.ip();
    if peer.is_loopback() {
        let forwarded = headers
            .get_all("x-forwarded-for")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(','))
            .filter_map(|s| s.trim().parse::<IpAddr>().ok())
            .last();
        if let Some(ip) = forwarded {
            return Some(ip);
        }
    }
    Some(peer)
}

/// The rate-limit key for an address: an IPv4 address as it is, an IPv6
/// address by its /64 (one home or server gets a whole /64, so keying by the
/// exact address would hand an attacker endless fresh keys, #371). An
/// IPv4-mapped IPv6 address counts as its IPv4 address.
pub fn rate_key(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => v4.to_string(),
            None => {
                let s = v6.segments();
                format!("{:x}:{:x}:{:x}:{:x}::/64", s[0], s[1], s[2], s[3])
            }
        },
    }
}

/// The client of a request, for the rate limits: its [`rate_key`] ("?"
/// when the server was started without connection info, as in tests).
pub struct ClientIp(pub String);

impl<S: Send + Sync> axum::extract::FromRequestParts<S> for ClientIp {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut axum::http::request::Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let peer = parts.extensions.get::<ConnectInfo<SocketAddr>>().map(|c| c.0);
        Ok(ClientIp(client_ip(&parts.headers, peer).map(rate_key).unwrap_or_else(|| "?".into())))
    }
}

// ---------- rate limits ----------

/// A limit: at most `max` hits per `window` seconds for one key.
#[derive(Clone, Copy, Debug)]
pub struct Limit {
    pub name: &'static str,
    pub max: u32,
    pub window: i64,
}

/// Sign-in attempts from one address.
pub const LOGIN_PER_IP: Limit = Limit { name: "login-ip", max: 30, window: 600 };
/// Wrong passwords for one account (e-mail address), from anywhere.
pub const LOGIN_FAILURES_PER_USER: Limit = Limit { name: "login-user", max: 10, window: 900 };
/// Registration attempts from one address.
pub const REGISTER_PER_IP: Limit = Limit { name: "register-ip", max: 20, window: 3600 };
/// Password changes tried by one account.
pub const PASSWORD_PER_USER: Limit = Limit { name: "password-user", max: 10, window: 900 };
/// "Send the e-mail again" (address confirmation) by one account.
pub const VERIFY_RESEND_PER_USER: Limit = Limit { name: "verify-resend-user", max: 5, window: 3600 };
/// E-mailed codes sent again by one account (a sign-in's "Send a new code").
pub const TWO_FACTOR_SEND_PER_USER: Limit = Limit { name: "two-factor-send-user", max: 5, window: 900 };
/// Password reset links asked for from one address (#379).
pub const RESET_PER_IP: Limit = Limit { name: "reset-ip", max: 10, window: 3600 };
/// Password reset links sent to one account's address (#379); past it the
/// page says the same and nothing is sent.
pub const RESET_PER_ADDRESS: Limit = Limit { name: "reset-address", max: 3, window: 3600 };
/// New passwords tried with reset links from one address (#379).
pub const RESET_SET_PER_IP: Limit = Limit { name: "reset-set-ip", max: 10, window: 600 };
/// "Someone tried to register with your address" messages to one address (#379).
pub const REGISTER_TAKEN_PER_ADDRESS: Limit = Limit { name: "register-taken-address", max: 3, window: 3600 };
/// Learning writes by one account (an answer, a finished round, a Guide
/// step, a word reviewed; `routes::limit_learning_write`, #381): a burst of
/// 60 in 10 seconds, far above a fast learner (two writes per Guide question).
pub const LEARNING_WRITES_PER_USER: Limit = Limit { name: "learning-writes-user", max: 60, window: 10 };
/// The same over an hour: one write a second, every second, for the hour.
pub const LEARNING_WRITES_PER_USER_HOUR: Limit = Limit { name: "learning-writes-user-hour", max: 3600, window: 3600 };
/// AI jobs (Generate, Only add, Allow overall changes) started by one
/// account in an hour, and in a day.
pub const AI_JOBS_PER_USER_HOUR: Limit = Limit { name: "ai-jobs-user-hour", max: 10, window: 3600 };
pub const AI_JOBS_PER_USER_DAY: Limit = Limit { name: "ai-jobs-user-day", max: 40, window: 86_400 };
/// Writes to course files by one account (create, upload, file save and
/// delete, the lesson table's saves): each rebuilds the whole course, so a
/// script cannot keep the server busy (#366). The editor's autosave saves at
/// most about once a second while someone works, well under this.
pub const COURSE_WRITES_PER_USER: Limit = Limit { name: "course-writes-user", max: 120, window: 300 };

/// Most keys kept per limit, so memory stays bounded on the Pi. Each limit
/// has its own table: a flood of one kind of key (fresh addresses) cannot
/// crowd out another (the per-account counters).
const MAX_KEYS: usize = 10_000;

/// One limit's counters: its window length and `key -> (window start, hits)`.
type Table = (i64, HashMap<String, (i64, u32)>);

/// Fixed-window counters in memory (they reset on restart). Off in dev mode,
/// where browser tests register many learners from one address.
///
/// A full table first drops its ended windows; if every window is still
/// running, a new key is refused (counted as over the limit, and
/// [`blocked`](RateLimits::blocked)) rather than evicting a running window:
/// evicting would let a flood of fresh keys reset someone's wrong-password
/// count (#371).
#[derive(Default)]
pub struct RateLimits {
    windows: Mutex<HashMap<&'static str, Table>>,
    disabled: bool,
}

/// Is there room in `table` for one more key? Drops ended windows first.
fn has_room(table: &mut Table, now: i64) -> bool {
    let (window, map) = table;
    if map.len() >= MAX_KEYS {
        map.retain(|_, (start, _)| now - *start < *window);
    }
    map.len() < MAX_KEYS
}

impl RateLimits {
    pub fn new(disabled: bool) -> RateLimits {
        RateLimits { windows: Mutex::default(), disabled }
    }

    /// Is `key` over `limit` already? Does not count anything. A key the
    /// full table has no room for counts as over.
    pub fn blocked(&self, limit: Limit, key: &str, now: i64) -> bool {
        if self.disabled {
            return false;
        }
        let mut tables = self.windows.lock().unwrap_or_else(|e| e.into_inner());
        let table = tables.entry(limit.name).or_insert_with(|| (limit.window, HashMap::new()));
        match table.1.get(&key.to_ascii_lowercase()) {
            Some(&(start, n)) => now - start < limit.window && n >= limit.max,
            None => !has_room(table, now),
        }
    }

    /// Count one hit for `key`; `false` when that goes over the limit (or
    /// the table is full of running windows and `key` is new).
    pub fn hit(&self, limit: Limit, key: &str, now: i64) -> bool {
        if self.disabled {
            return true;
        }
        self.hit_always(limit, key, now)
    }

    /// [`RateLimits::hit`], counted in dev mode too: for a limit small
    /// enough to try by hand.
    pub fn hit_always(&self, limit: Limit, key: &str, now: i64) -> bool {
        let mut tables = self.windows.lock().unwrap_or_else(|e| e.into_inner());
        let table = tables.entry(limit.name).or_insert_with(|| (limit.window, HashMap::new()));
        let key = key.to_ascii_lowercase();
        if !table.1.contains_key(&key) && !has_room(table, now) {
            tracing::warn!("rate limit {} is full; refusing new keys until windows end", limit.name);
            return false;
        }
        let entry = table.1.entry(key).or_insert((now, 0));
        if now - entry.0 >= limit.window {
            *entry = (now, 0);
        }
        entry.1 = entry.1.saturating_add(1);
        entry.1 <= limit.max
    }

    /// Drop the windows that have ended (hourly housekeeping).
    pub fn purge_expired(&self, now: i64) -> usize {
        let mut tables = self.windows.lock().unwrap_or_else(|e| e.into_inner());
        let mut dropped = 0;
        for (window, map) in tables.values_mut() {
            let before = map.len();
            map.retain(|_, (start, _)| now - *start < *window);
            dropped += before - map.len();
        }
        dropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.append(*k, HeaderValue::from_str(v).unwrap());
        }
        h
    }

    #[test]
    fn cross_site_requests_are_recognised() {
        assert!(!is_cross_site(&headers(&[])));
        assert!(!is_cross_site(&headers(&[("sec-fetch-site", "same-origin")])));
        assert!(!is_cross_site(&headers(&[("sec-fetch-site", "none")])));
        assert!(is_cross_site(&headers(&[("sec-fetch-site", "cross-site")])));
        assert!(is_cross_site(&headers(&[("sec-fetch-site", "same-site")])));
        assert!(!is_cross_site(&headers(&[("host", "learn.example.org"), ("origin", "https://learn.example.org")])));
        assert!(!is_cross_site(&headers(&[("host", "127.0.0.1:9000"), ("origin", "http://127.0.0.1:9000")])));
        assert!(is_cross_site(&headers(&[("host", "learn.example.org"), ("origin", "https://evil.example")])));
        assert!(is_cross_site(&headers(&[("host", "learn.example.org"), ("origin", "null")])));
        // Behind a proxy that rewrites Host, the forwarded host counts.
        assert!(!is_cross_site(&headers(&[("host", "127.0.0.1:8080"), ("x-forwarded-host", "learn.example.org"), ("origin", "https://learn.example.org")])));
        // Sec-Fetch-Site wins over a matching Origin.
        assert!(is_cross_site(&headers(&[("host", "a.org"), ("origin", "https://a.org"), ("sec-fetch-site", "cross-site")])));
    }

    #[test]
    fn warns_about_plain_http() {
        let lan: SocketAddr = "0.0.0.0:8080".parse().unwrap();
        let local: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        assert!(https_warnings(lan, false, None)[0].contains("plain HTTP"));
        assert!(https_warnings(lan, true, None).is_empty());
        assert!(https_warnings(local, false, None).is_empty());
        assert!(https_warnings(local, false, Some("https://learn.example.org"))[0].contains("SKRONGLE_SECURE_COOKIES"));
        assert!(https_warnings(local, true, Some("https://learn.example.org")).is_empty());
    }

    #[test]
    fn client_ip_trusts_only_a_local_proxy() {
        let local: SocketAddr = "127.0.0.1:5000".parse().unwrap();
        let remote: SocketAddr = "203.0.113.9:5000".parse().unwrap();
        let xff = headers(&[("x-forwarded-for", "1.2.3.4, 198.51.100.7")]);
        assert_eq!(client_ip(&xff, Some(local)), Some("198.51.100.7".parse().unwrap()));
        assert_eq!(client_ip(&xff, Some(remote)), Some("203.0.113.9".parse().unwrap()));
        assert_eq!(client_ip(&HeaderMap::new(), Some(local)), Some("127.0.0.1".parse().unwrap()));
        assert_eq!(client_ip(&HeaderMap::new(), None), None);
    }

    #[test]
    fn rate_limits_count_per_key_and_window() {
        let l = RateLimits::new(false);
        let lim = Limit { name: "login-ip", max: 3, window: 600 };
        for _ in 0..3 {
            assert!(l.hit(lim, "1.2.3.4", 1000));
        }
        assert!(!l.hit(lim, "1.2.3.4", 1001));
        assert!(l.blocked(lim, "1.2.3.4", 1001));
        assert!(!l.blocked(lim, "5.6.7.8", 1001));
        assert!(l.hit(lim, "5.6.7.8", 1001));
        // Keys are case-insensitive (addresses are).
        let user = Limit { name: "login-user", max: 1, window: 600 };
        assert!(l.hit(user, "Ana", 1000));
        assert!(l.blocked(user, "ana", 1000));
        // A new window starts afresh.
        assert!(!l.blocked(lim, "1.2.3.4", 1600));
        assert!(l.hit(lim, "1.2.3.4", 1600));
        assert_eq!(l.purge_expired(5000), 3);
        // Dev mode: never limited.
        let off = RateLimits::new(true);
        for _ in 0..10 {
            assert!(off.hit(lim, "x", 1000));
        }
        assert!(!off.blocked(lim, "x", 1000));
    }

    #[test]
    fn rate_limit_memory_is_bounded() {
        let l = RateLimits::new(false);
        let lim = Limit { name: "login-ip", max: 3, window: 600 };
        for i in 0..(MAX_KEYS + 10) {
            let _ = l.hit(lim, &format!("k{i}"), i as i64 / 100);
        }
        assert!(l.windows.lock().unwrap()["login-ip"].1.len() <= MAX_KEYS);
        // Once the first windows have ended, new keys fit again.
        assert!(l.hit(lim, "late", 10_000));
    }

    #[test]
    fn a_flood_of_new_keys_cannot_reset_an_account_counter() {
        let l = RateLimits::new(false);
        let ip = Limit { name: "login-ip", max: 30, window: 600 };
        let user = Limit { name: "login-user", max: 3, window: 900 };
        for _ in 0..3 {
            l.hit(user, "ana@example.org", 1000);
        }
        assert!(l.blocked(user, "ana@example.org", 1000));
        // Fresh addresses fill their own table, not the accounts'.
        for i in 0..(MAX_KEYS + 50) {
            let _ = l.hit(ip, &format!("10.{}.{}.{}", i >> 16, (i >> 8) & 255, i & 255), 1001);
        }
        assert!(l.blocked(user, "ana@example.org", 1002), "the account is still blocked");
        // A full table of running windows refuses a new key instead of
        // evicting one.
        assert!(!l.hit(ip, "192.0.2.1", 1002));
        assert!(l.blocked(ip, "192.0.2.1", 1002));
        assert!(l.hit(ip, "10.0.0.1", 1002), "keys already in the table still count normally");
        // Fill the accounts' table too: the blocked account stays blocked.
        for i in 0..MAX_KEYS {
            let _ = l.hit(user, &format!("u{i}@example.org"), 1003);
        }
        assert!(l.blocked(user, "ana@example.org", 1004));
        assert!(l.blocked(user, "new@example.org", 1004), "no room: a new account counts as over");
        // When the windows end, everything frees up.
        assert!(!l.blocked(user, "ana@example.org", 1000 + 900));
        assert!(l.hit(ip, "192.0.2.1", 1001 + 600));
    }

    #[test]
    fn ipv6_clients_are_keyed_by_their_64() {
        let a: IpAddr = "2001:db8:1:2:aaaa::1".parse().unwrap();
        let b: IpAddr = "2001:db8:1:2:ffff:1:2:3".parse().unwrap();
        let other: IpAddr = "2001:db8:1:3::1".parse().unwrap();
        assert_eq!(rate_key(a), "2001:db8:1:2::/64");
        assert_eq!(rate_key(a), rate_key(b));
        assert_ne!(rate_key(a), rate_key(other));
        assert_eq!(rate_key("203.0.113.9".parse().unwrap()), "203.0.113.9");
        assert_eq!(rate_key("::ffff:203.0.113.9".parse().unwrap()), "203.0.113.9");
    }
}
