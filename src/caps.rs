//! The limits a Skrongle mini holds itself to (#48). Fixed numbers, not
//! settings: a small known size is what lets every structure that grows
//! with use (sessions, rate-limit keys, connections, tutor calls) be
//! bounded by a number derived from [`MAX_ACCOUNTS`].

/// Accounts one Skrongle mini can hold (#29): a household plus a couple of
/// guests. The other caps are derived from this number.
pub const MAX_ACCOUNTS: usize = 8;

use std::sync::atomic::{AtomicBool, Ordering};

static ACCOUNTS_EXEMPT: AtomicBool = AtomicBool::new(false);

/// Lift the account cap for this process. Called once at startup for the
/// hosted site (the cap is a mini's) and for the dev server (its seeded
/// crowd and the e2e specs make far more than 8 accounts). Unit tests
/// never call it, so they see the cap.
pub fn exempt_accounts() {
    ACCOUNTS_EXEMPT.store(true, Ordering::Relaxed);
}

/// Is a server holding `count` accounts at its cap?
pub fn accounts_full(count: i64) -> bool {
    !ACCOUNTS_EXEMPT.load(Ordering::Relaxed) && count >= MAX_ACCOUNTS as i64
}
