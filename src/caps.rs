//! The limits a Skrongle mini holds itself to (#48). Fixed numbers, except
//! one setting: a known size is what lets every structure that grows with
//! use (sessions, rate-limit keys, connections, tutor calls, and the built
//! courses kept in memory, `courses::MAX_STORES`) be bounded. Sessions,
//! rate-limit keys and the per-learner caches follow [`MAX_ACCOUNTS`] and
//! the student limit (`SKRONGLE_STUDENT_LIMIT`, mini #216), through
//! [`Bounds`]; the costly ones (connections, tutor calls, built courses) are
//! fixed numbers of their own. Students have no API grants or tokens, so
//! those tables follow [`MAX_ACCOUNTS`] alone.

use std::ops::RangeInclusive;
use std::sync::OnceLock;

/// Accounts one Skrongle mini can hold (#29): a class, or a household with
/// room to spare. It was 8 until mini #133. Sessions, rate-limit keys and
/// the per-learner caches are derived from this number.
pub const MAX_ACCOUNTS: usize = 50;

/// Is a server holding `count` normal accounts at its cap? Students are
/// counted apart ([`students_full`]). The dev server is never
/// at it: it registers an account per e2e spec, and the
/// cap is for the mini the family runs (#29).
pub fn accounts_full(count: i64, dev: bool) -> bool {
    !dev && count >= MAX_ACCOUNTS as i64
}

/// Live sessions one account keeps (phone, tablet, laptop, one spare, #30).
/// Signing in a fifth time ends the oldest, so the `sessions` table never
/// holds more than [`Bounds::sessions`] rows (300 at the defaults: 200 for
/// accounts, 100 for students).
pub const MAX_SESSIONS_PER_ACCOUNT: usize = 4;

/// Live sessions one student account keeps (mini #216): a shared classroom
/// computer and one more. Signing in a third time ends the oldest.
pub const MAX_SESSIONS_PER_STUDENT: usize = 2;

/// Keys one rate-limit table holds per learner (#36): 8 per account or
/// student (400 at the defaults), which is ample for the addresses a class
/// or a household signs in from; a full table only ever refuses a
/// stranger's address during a flood. Keeps the in-memory tables to tens of
/// KiB on the Pi. The table's size is [`Bounds::rate_limit_keys`].
pub const RATE_LIMIT_KEYS_PER_LEARNER: usize = 8;

/// Student accounts a box holds unless `SKRONGLE_STUDENT_LIMIT` says
/// otherwise (mini #216). A Pi Zero 2 W is measured at this number.
pub const DEFAULT_STUDENT_LIMIT: usize = 50;

/// The values `SKRONGLE_STUDENT_LIMIT` may take. Above the default the
/// owner's box pays for it; above 1000 nothing on one small box is sized
/// for it.
pub const STUDENT_LIMIT_RANGE: RangeInclusive<usize> = 1..=1000;

/// Read `SKRONGLE_STUDENT_LIMIT`'s value (`None`: not set). A value that is
/// not a whole number in [`STUDENT_LIMIT_RANGE`] is an error naming the
/// setting, which stops the server at startup, as a bad `SKRONGLE_MIN_AGE`
/// does.
pub fn parse_student_limit(value: Option<&str>) -> Result<usize, String> {
    let Some(v) = value else { return Ok(DEFAULT_STUDENT_LIMIT) };
    let (lo, hi) = (STUDENT_LIMIT_RANGE.start(), STUDENT_LIMIT_RANGE.end());
    match v.trim().parse::<usize>() {
        Ok(n) if STUDENT_LIMIT_RANGE.contains(&n) => Ok(n),
        _ => Err(format!("SKRONGLE_STUDENT_LIMIT must be a whole number from {lo} to {hi}, got {v:?}")),
    }
}

/// The sizes that grow with the learners a box holds: [`MAX_ACCOUNTS`]
/// normal accounts plus `students` student accounts. Computed once at
/// startup ([`init_student_limit`]); every table and cache that follows the
/// student limit takes its size from here, so a box at limit N has a known
/// size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    /// Student accounts the box holds (`SKRONGLE_STUDENT_LIMIT`).
    pub students: usize,
    /// Accounts of either kind: one entry each in the per-learner caches
    /// (progress maps, game sessions).
    pub learners: usize,
    /// Rows `sessions` holds at most, and entries in the session cache:
    /// [`MAX_SESSIONS_PER_ACCOUNT`] per account, [`MAX_SESSIONS_PER_STUDENT`]
    /// per student.
    pub sessions: usize,
    /// Keys one rate-limit table holds: [`RATE_LIMIT_KEYS_PER_LEARNER`] per
    /// learner.
    pub rate_limit_keys: usize,
}

impl Bounds {
    /// The bounds for a box holding `students` student accounts.
    pub const fn for_students(students: usize) -> Bounds {
        let learners = MAX_ACCOUNTS + students;
        Bounds {
            students,
            learners,
            sessions: MAX_ACCOUNTS * MAX_SESSIONS_PER_ACCOUNT + students * MAX_SESSIONS_PER_STUDENT,
            rate_limit_keys: learners * RATE_LIMIT_KEYS_PER_LEARNER,
        }
    }
}

static BOUNDS: OnceLock<Bounds> = OnceLock::new();

/// Set once at startup from `SKRONGLE_STUDENT_LIMIT` ([`parse_student_limit`]).
/// A later call changes nothing. Logs one line above the default.
pub fn init_student_limit(students: usize) {
    if BOUNDS.set(Bounds::for_students(students)).is_ok() && students > DEFAULT_STUDENT_LIMIT {
        tracing::warn!("{}", above_default_line(students));
    }
}

/// The startup line for a limit above the default.
fn above_default_line(students: usize) -> String {
    format!("student limit: {students} (SKRONGLE_STUDENT_LIMIT); a Pi Zero 2 W is measured at {DEFAULT_STUDENT_LIMIT}, so more students use more memory than the measured budget")
}

/// The bounds of this server: from `SKRONGLE_STUDENT_LIMIT`, or the default
/// before startup set it (tests, commands that do not start the server).
pub fn bounds() -> Bounds {
    BOUNDS.get().copied().unwrap_or(Bounds::for_students(DEFAULT_STUDENT_LIMIT))
}

/// Student accounts the box holds (`SKRONGLE_STUDENT_LIMIT`, default 50).
#[cfg_attr(not(test), allow(dead_code))] // counted against from #217 on
pub fn student_limit() -> usize {
    bounds().students
}

/// Is a server holding `count` student accounts at its student limit?
/// Normal accounts are counted apart ([`accounts_full`]). A lowered limit
/// deletes nobody: it only refuses new students until the count is under
/// it. The dev server is never at it, as for [`accounts_full`].
#[cfg_attr(not(test), allow(dead_code))] // counted against from #217 on
pub fn students_full(count: i64, dev: bool) -> bool {
    !dev && count >= student_limit() as i64
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_student_limit_defaults_to_50_and_takes_1_to_1000() {
        assert_eq!(DEFAULT_STUDENT_LIMIT, 50);
        assert_eq!(parse_student_limit(None), Ok(50));
        assert_eq!(parse_student_limit(Some("1")), Ok(1));
        assert_eq!(parse_student_limit(Some(" 120 ")), Ok(120));
        assert_eq!(parse_student_limit(Some("1000")), Ok(1000));
        for bad in ["0", "1001", "-5", "abc", "50.5", ""] {
            let e = parse_student_limit(Some(bad)).unwrap_err();
            assert!(e.contains("SKRONGLE_STUDENT_LIMIT") && e.contains("1 to 1000"), "{bad:?}: {e}");
        }
    }

    #[test]
    fn the_bounds_at_the_default() {
        let b = Bounds::for_students(DEFAULT_STUDENT_LIMIT);
        assert_eq!(b, Bounds { students: 50, learners: 100, sessions: 200 + 100, rate_limit_keys: 800 });
        // Before startup sets it (tests, commands), the default holds.
        assert_eq!(bounds(), b);
        assert_eq!(student_limit(), 50);
    }

    #[test]
    fn the_bounds_follow_a_raised_or_lowered_limit() {
        assert_eq!(Bounds::for_students(200), Bounds { students: 200, learners: 250, sessions: 200 + 400, rate_limit_keys: 2000 });
        assert_eq!(Bounds::for_students(1), Bounds { students: 1, learners: 51, sessions: 202, rate_limit_keys: 408 });
        let top = Bounds::for_students(*STUDENT_LIMIT_RANGE.end());
        assert_eq!(top.sessions, 2200, "the largest box still has a known size");
    }

    #[test]
    fn students_and_accounts_are_counted_apart() {
        assert!(!students_full(49, false));
        assert!(students_full(50, false));
        assert!(!students_full(50, true), "the dev server is not capped");
        // A full set of normal accounts says nothing about students.
        assert!(accounts_full(MAX_ACCOUNTS as i64, false) && !students_full(0, false));
    }

    #[test]
    fn the_startup_line_names_the_limit_and_the_pi() {
        let line = above_default_line(120);
        assert!(line.contains("120") && line.contains("Pi Zero 2 W") && line.contains("50"), "{line}");
    }
}
