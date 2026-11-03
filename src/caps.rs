//! The limits a Skrongle mini holds itself to (#48). Fixed numbers, not
//! settings: a small known size is what lets every structure that grows
//! with use (sessions, rate-limit keys, connections, tutor calls) be
//! bounded by a number derived from [`MAX_ACCOUNTS`].

/// Accounts one Skrongle mini can hold (#29): a household plus a couple of
/// guests. The other caps are derived from this number.
pub const MAX_ACCOUNTS: usize = 8;

/// Keys one rate-limit table holds (#36). A mini has a handful of accounts
/// and addresses, so 8 keys per account is ample; a full table only ever
/// refuses a stranger's address during a flood. Keeps the in-memory tables
/// a few KiB on the Pi.
pub const MAX_RATE_LIMIT_KEYS: usize = MAX_ACCOUNTS * 8;

/// Open HTTP connections one Skrongle mini allows (#46): 8 accounts times a
/// browser's 6 connections, with room. Past it new clients wait in the
/// kernel's backlog, and a runaway client or a port scan cannot hold more
/// sockets than that.
pub const MAX_CONNECTIONS: usize = 32;

/// Is a server holding `count` accounts at its cap? The dev server is never
/// at it: it registers an account per e2e spec and seeds a crowd, and the
/// cap is for the mini the family runs (#29).
pub fn accounts_full(count: i64, dev: bool) -> bool {
    !dev && count >= MAX_ACCOUNTS as i64
}

/// Live sessions one account keeps (phone, tablet, laptop, one spare, #30).
/// Signing in a fifth time ends the oldest, so the `sessions` table never
/// holds more than `MAX_ACCOUNTS * MAX_SESSIONS_PER_ACCOUNT` (32) rows.
pub const MAX_SESSIONS_PER_ACCOUNT: usize = 4;

/// Tutor model calls in flight at once, for all learners (#47). Each holds an
/// OS thread and a TLS connection for up to two minutes on a 512 MB Pi; a
/// third call is refused with the busy message (`tutor::ALL_BUSY`).
pub const MAX_TUTOR_CALLS: usize = 2;
