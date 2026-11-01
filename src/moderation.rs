//! What a superuser does to keep the Terms (#967), from `/superuser`: take a
//! made course down (it opens for nobody, its owner sees why) and put it
//! back, suspend an account (signed out, sign-in refused, its courses not
//! shared out) and lift it, and block an address or range (every request a
//! plain 403). Deleting an account is the learner's own deletion
//! (`privacy::delete_account`). Hosted only: Skrongle mini's owner has the
//! database.
//!
//! The blocked ranges live in memory too ([`Blocklist`]), so the check on
//! every request ([`block`]) costs no query; the list is read from
//! `blocked_ips` at startup and replaced whenever the page changes it.

use axum::extract::{ConnectInfo, Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use rusqlite::{params, Connection};
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, RwLock};

// ---------- courses ----------

/// Take a made course down: nobody opens it (`courses::load_ready_files`
/// gives nothing) until [`put_back`]. Its shares stay, so putting it back
/// restores them. `false` when there is no such made course.
pub fn take_down(conn: &Connection, course_id: &str, now: i64) -> rusqlite::Result<bool> {
    Ok(conn.execute(
        "UPDATE courses SET taken_down_at = ?2 WHERE id = ?1 AND taken_down_at IS NULL",
        params![course_id, now],
    )? > 0)
}

/// Put a taken-down course back. `false` when it was not down.
pub fn put_back(conn: &Connection, course_id: &str) -> rusqlite::Result<bool> {
    Ok(conn.execute(
        "UPDATE courses SET taken_down_at = NULL WHERE id = ?1 AND taken_down_at IS NOT NULL",
        [course_id],
    )? > 0)
}

/// What the owner of a course that is down reads, with the contact address.
pub fn removed_text() -> String {
    crate::t1(
        "courses.picker.removed_info",
        "email",
        crate::terms::legal_email(),
    )
}

// ---------- accounts ----------

/// Suspend an account: its sessions and sign-ins waiting for a code end,
/// and until [`lift`] it cannot sign in (`routes::auth::sign_in`) and no
/// session of it resolves (`auth::user_for_session`). Its courses stop being
/// shared out (`share::can_open`, `share::shared_with`). Returns the number
/// of sessions ended, `None` when it was suspended already.
pub fn suspend(conn: &Connection, user_id: i64, now: i64) -> rusqlite::Result<Option<usize>> {
    if conn.execute(
        "UPDATE users SET suspended_at = ?2 WHERE id = ?1 AND suspended_at IS NULL",
        params![user_id, now],
    )? == 0
    {
        return Ok(None);
    }
    conn.execute(
        "DELETE FROM two_factor_challenges WHERE user_id = ?1",
        [user_id],
    )?;
    Ok(Some(conn.execute(
        "DELETE FROM sessions WHERE user_id = ?1",
        [user_id],
    )?))
}

/// Lift a suspension. `false` when the account was not suspended.
pub fn lift(conn: &Connection, user_id: i64) -> rusqlite::Result<bool> {
    Ok(conn.execute(
        "UPDATE users SET suspended_at = NULL WHERE id = ?1 AND suspended_at IS NOT NULL",
        [user_id],
    )? > 0)
}

pub fn is_suspended(conn: &Connection, user_id: i64) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM users WHERE id = ?1 AND suspended_at IS NOT NULL)",
        [user_id],
        |r| r.get(0),
    )
}

/// What a suspended account reads when it tries to sign in.
pub fn suspended_text() -> String {
    crate::t1("auth.login.suspended", "email", crate::terms::legal_email())
}

// ---------- addresses ----------

/// One blocked address or range: `addr` with its first `prefix` bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Net {
    addr: IpAddr,
    prefix: u8,
}

impl Net {
    /// "203.0.113.7", "203.0.113.0/24", "2001:db8::/32". `None` for anything
    /// else, or a range wider than a /8 (IPv4) or /16 (IPv6), which would
    /// block a good part of the internet.
    pub fn parse(text: &str) -> Option<Net> {
        let text = text.trim();
        let (addr, prefix) = match text.split_once('/') {
            Some((a, p)) => (
                a.trim().parse::<IpAddr>().ok()?,
                Some(p.trim().parse::<u8>().ok()?),
            ),
            None => (text.parse::<IpAddr>().ok()?, None),
        };
        let addr = plain(addr);
        let (max, min) = if addr.is_ipv4() { (32, 8) } else { (128, 16) };
        let prefix = prefix.unwrap_or(max);
        if prefix > max || prefix < min {
            return None;
        }
        Some(Net {
            addr: mask(addr, prefix),
            prefix,
        })
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        let ip = plain(ip);
        ip.is_ipv4() == self.addr.is_ipv4() && mask(ip, self.prefix) == self.addr
    }
}

impl std::fmt::Display for Net {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let max = if self.addr.is_ipv4() { 32 } else { 128 };
        if self.prefix == max {
            write!(f, "{}", self.addr)
        } else {
            write!(f, "{}/{}", self.addr, self.prefix)
        }
    }
}

/// An IPv4-mapped IPv6 address as its IPv4 address.
fn plain(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(ip, IpAddr::V4),
        v4 => v4,
    }
}

/// The address with every bit past `prefix` cleared.
fn mask(ip: IpAddr, prefix: u8) -> IpAddr {
    match ip {
        IpAddr::V4(v4) => {
            let bits = u32::from(v4);
            let m = if prefix == 0 {
                0
            } else {
                u32::MAX << (32 - u32::from(prefix))
            };
            IpAddr::V4((bits & m).into())
        }
        IpAddr::V6(v6) => {
            let bits = u128::from(v6);
            let m = if prefix == 0 {
                0
            } else {
                u128::MAX << (128 - u32::from(prefix))
            };
            IpAddr::V6((bits & m).into())
        }
    }
}

/// The blocked ranges, in memory: read on every request.
#[derive(Default)]
pub struct Blocklist(RwLock<Arc<Vec<Net>>>);

impl Blocklist {
    /// Read `blocked_ips` again (at startup, and after the page changes it).
    pub fn reload(&self, conn: &Connection) -> rusqlite::Result<()> {
        let nets = blocked(conn)?
            .into_iter()
            .filter_map(|(_, cidr)| Net::parse(&cidr))
            .collect();
        *self.0.write().unwrap_or_else(|p| p.into_inner()) = Arc::new(nets);
        Ok(())
    }

    pub fn blocks(&self, ip: IpAddr) -> bool {
        self.0
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .any(|n| n.contains(ip))
    }
}

/// Block an address or range. `false` when it was blocked already.
pub fn block_net(conn: &Connection, net: Net, now: i64) -> rusqlite::Result<bool> {
    Ok(conn.execute(
        "INSERT OR IGNORE INTO blocked_ips (cidr, created_at) VALUES (?1, ?2)",
        params![net.to_string(), now],
    )? > 0)
}

/// Remove a block by its id; what it blocked, if there was one.
pub fn unblock(conn: &Connection, id: i64) -> rusqlite::Result<Option<String>> {
    use rusqlite::OptionalExtension;
    conn.query_row(
        "DELETE FROM blocked_ips WHERE id = ?1 RETURNING cidr",
        [id],
        |r| r.get(0),
    )
    .optional()
}

/// Every block: id and range, newest first.
pub fn blocked(conn: &Connection) -> rusqlite::Result<Vec<(i64, String)>> {
    let mut stmt = conn.prepare_cached("SELECT id, cidr FROM blocked_ips ORDER BY id DESC")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    rows.collect()
}

/// Middleware: a request from a blocked address gets a plain 403, before
/// any session lookup or handler.
pub async fn block(
    State(state): State<Arc<crate::AppState>>,
    req: Request,
    next: Next,
) -> Response {
    let peer = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0);
    if let Some(ip) =
        crate::security::client_ip(req.headers(), peer).filter(|ip| state.blocked.blocks(*ip))
    {
        tracing::debug!("refused a request from blocked address {ip}");
        return (StatusCode::FORBIDDEN, crate::t("error.blocked")).into_response();
    }
    next.run(req).await
}

#[cfg(test)]
mod tests;
