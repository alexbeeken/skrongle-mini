//! The limits a Skrongle mini holds itself to (#48). Fixed numbers, not
//! settings: a small known size is what lets every structure that grows
//! with use (sessions, rate-limit keys, connections, tutor calls) be
//! bounded by a number derived from [`MAX_ACCOUNTS`].

/// Accounts one Skrongle mini can hold (#29): a household plus a couple of
/// guests. The other caps are derived from this number.
pub const MAX_ACCOUNTS: usize = 8;
