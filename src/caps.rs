//! The limits a Skrongle mini holds itself to (#48). Fixed numbers, not
//! settings: a known size is what lets every structure that grows with use
//! (sessions, rate-limit keys, connections, tutor calls, and the built
//! courses kept in memory, `courses::MAX_STORES`) be bounded. Sessions,
//! rate-limit keys and the per-learner caches follow [`MAX_ACCOUNTS`]; the
//! costly ones (connections, tutor calls, built courses) are fixed numbers
//! of their own.

/// Accounts one Skrongle mini can hold (#29): a class, or a household with
/// room to spare. It was 8 until mini #133. Sessions, rate-limit keys and
/// the per-learner caches are derived from this number.
pub const MAX_ACCOUNTS: usize = 50;

/// Is a server holding `count` accounts at its cap? The dev server is never
/// at it: it registers an account per e2e spec, and the
/// cap is for the mini the family runs (#29).
pub fn accounts_full(count: i64, dev: bool) -> bool {
    !dev && count >= MAX_ACCOUNTS as i64
}

/// Live sessions one account keeps (phone, tablet, laptop, one spare, #30).
/// Signing in a fifth time ends the oldest, so the `sessions` table never
/// holds more than `MAX_ACCOUNTS * MAX_SESSIONS_PER_ACCOUNT` (200) rows.
pub const MAX_SESSIONS_PER_ACCOUNT: usize = 4;

/// Keys one rate-limit table holds (#36): 8 per account (400), which is
/// ample for the addresses a class or a household signs in from; a full
/// table only ever refuses a stranger's address during a flood. Keeps the
/// in-memory tables to tens of KiB on the Pi.
pub const MAX_RATE_LIMIT_KEYS: usize = MAX_ACCOUNTS * 8;

/// Open HTTP connections one Skrongle mini allows (#46). Not derived from
/// [`MAX_ACCOUNTS`]: 128 is about twenty browsers loading a page at the
/// same moment (a browser opens up to 6), and half of the unit's
/// `LimitNOFILE=256`, which also has to cover the database files and the
/// calls out to AI providers. An idle connection is closed after 15
/// seconds (`server::Limits::header_read`), so a class takes turns on them.
/// Past the cap new clients wait in the kernel's backlog, and a runaway
/// client or a port scan cannot hold more sockets than that. It was 32
/// while a mini held 8 accounts (mini #133).
pub const MAX_CONNECTIONS: usize = 128;

/// Tutor model calls in flight at once, for all learners (#47). Each holds an
/// OS thread and a TLS connection for up to two minutes on a 512 MB Pi; a
/// third call is refused with the busy message (`tutor::ALL_BUSY`).
pub const MAX_TUTOR_CALLS: usize = 2;

/// Connected apps one account keeps (#167): grants in `api_grants`, as for
/// sessions. A fifth is refused with a message saying to revoke one.
pub const MAX_GRANTS_PER_ACCOUNT: usize = 4;

/// Rows `api_grants` holds for the whole server: one per grant that can exist.
pub const MAX_API_GRANTS: usize = MAX_ACCOUNTS * MAX_GRANTS_PER_ACCOUNT;

/// Rows `api_tokens` holds for the whole server: a personal token is one
/// row; an app that signs in with OAuth holds an access and a refresh token
/// and, after refreshing, the access token it replaced (working until its
/// hour ends). The refresh token it used is deleted: an earlier one is
/// still known by the family hash on its grant (#192), with no row per
/// refresh. So three rows a grant, however often it refreshes:
/// 50 accounts × 4 grants = 200 grants; 200 × 3 = 600 rows.
pub const MAX_API_TOKENS: usize = MAX_API_GRANTS * 3;

/// Apps registered for OAuth sign-in (#169, `oauth_clients`): a new
/// registration past it pushes out the oldest one with no grant. A
/// registration nobody finished a grant for is purged after a day anyway.
pub const MAX_OAUTH_CLIENTS: usize = 100;

/// OAuth codes waiting to be traded at once (#169, `oauth_codes`): each
/// lives ten minutes. Past it Allow says the server is busy.
pub const MAX_OAUTH_CODES: usize = 200;
