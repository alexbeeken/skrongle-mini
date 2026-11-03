//! The limits a Skrongle mini holds itself to (#48). Fixed numbers, not
//! settings: a small known size is what lets every structure that grows
//! with use (sessions, rate-limit keys, connections, tutor calls) be
//! bounded by a number derived from [`MAX_ACCOUNTS`].

/// Accounts one Skrongle mini can hold (#29): a household plus a couple of
/// guests. The other caps are derived from this number.
pub const MAX_ACCOUNTS: usize = 8;

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
