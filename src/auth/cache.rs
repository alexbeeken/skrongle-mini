//! The signed-in users, kept in memory by session (#37), so a request after
//! the first finds its user without the session query (`user_for_session`:
//! a join of `sessions`, `users`, `courses` and `projects` plus the
//! terms subquery). The working set is a few dozen `CurrentUser`s: at most
//! `MAX_ACCOUNTS * MAX_SESSIONS_PER_ACCOUNT` sessions exist.
//!
//! Kept true by the connection, not by each write path: `Db` creates
//! temporary triggers ([`SessionCache::triggers_sql`]) that forget every
//! entry when a row of a table the query reads ([`WATCHED`]) is inserted,
//! updated or deleted, whichever code does it (sign-in and sign-out, profile
//! and course changes, terms, account deletion, the hourly purge, and any
//! write added later). An entry is also dropped when its session expires and
//! after [`MAX_AGE_SECS`], for what changes with time alone (the terms notice
//! by date) or from another process (`skrongle reset-password` run beside
//! the server).

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use super::CurrentUser;
use crate::caps::{MAX_ACCOUNTS, MAX_SESSIONS_PER_ACCOUNT};

/// The tables `user_for_session` reads. A write to any of them forgets the
/// whole cache (it is tiny and refills on the next request).
pub const WATCHED: &[&str] = &["sessions", "users", "courses", "projects", "settings"];

/// Entries the cache holds: one per session that can exist.
pub const CAPACITY: usize = MAX_ACCOUNTS * MAX_SESSIONS_PER_ACCOUNT;

/// The longest an entry is used before the database is asked again.
pub const MAX_AGE_SECS: i64 = 60;

struct Entry {
    user: CurrentUser,
    /// Used until then: the session's `expires_at`, or `MAX_AGE_SECS` after
    /// it was read, whichever comes first.
    until: i64,
}

/// Users by the session token's hash (`hash_token`, what `sessions` stores).
#[derive(Default)]
pub struct SessionCache {
    entries: Mutex<HashMap<String, Entry>>,
    /// Sessions read from the database (tests count them).
    reads: AtomicUsize,
}

impl SessionCache {
    fn entries(&self) -> std::sync::MutexGuard<'_, HashMap<String, Entry>> {
        self.entries.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The user of session `hash` at time `now`, if it is cached and still
    /// good; an entry past its time is dropped.
    pub fn get(&self, hash: &str, now: i64) -> Option<CurrentUser> {
        let mut entries = self.entries();
        match entries.get(hash) {
            Some(e) if e.until > now => Some(e.user.clone()),
            Some(_) => {
                entries.remove(hash);
                None
            }
            None => None,
        }
    }

    /// Remember the user read for session `hash` at `now`, whose session
    /// expires at `expires_at`. Call it with the database's lock held, in the
    /// same `Db::call` as the read, so no write can land between the two.
    pub fn put(&self, hash: String, user: CurrentUser, now: i64, expires_at: i64) {
        self.reads.fetch_add(1, Ordering::Relaxed);
        let mut entries = self.entries();
        if entries.len() >= CAPACITY && !entries.contains_key(&hash) {
            entries.clear();
        }
        entries.insert(hash, Entry { user, until: expires_at.min(now + MAX_AGE_SECS) });
    }

    /// The connection's temporary triggers: after every insert, update or
    /// delete of a row of a [`WATCHED`] table, the `forget_sessions()` SQL
    /// function `Db` registers runs `forget_all`. Triggers and not an update
    /// hook: SQLite calls no update hook for a `WITHOUT ROWID` table, which
    /// `courses` and `projects` are, nor for a `DELETE` without `WHERE`.
    pub fn triggers_sql() -> String {
        let mut sql = String::new();
        for table in WATCHED {
            for op in ["INSERT", "UPDATE", "DELETE"] {
                let name = format!("forget_sessions_{}_{}", table, op.to_lowercase());
                sql.push_str(&format!(
                    "CREATE TEMP TRIGGER IF NOT EXISTS {name} AFTER {op} ON {table} BEGIN SELECT forget_sessions(); END;\n"
                ));
            }
        }
        sql
    }

    /// Forget every user: something they were read from changed.
    pub fn forget_all(&self) {
        let mut entries = self.entries();
        if !entries.is_empty() {
            entries.clear();
        }
    }

    /// Sessions read from the database (and then cached) so far.
    #[cfg(test)]
    pub fn reads(&self) -> usize {
        self.reads.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expired_entries_are_dropped_and_a_full_cache_starts_over() {
        let cache = SessionCache::default();
        cache.put("a".into(), CurrentUser::test(1), 100, 130);
        assert_eq!(cache.get("a", 129).map(|u| u.id), Some(1));
        assert!(cache.get("a", 130).is_none(), "the session expired");
        assert_eq!(cache.len(), 0);
        cache.put("b".into(), CurrentUser::test(2), 100, i64::MAX);
        assert!(cache.get("b", 100 + MAX_AGE_SECS - 1).is_some());
        assert!(cache.get("b", 100 + MAX_AGE_SECS).is_none(), "read again after a while");
        for i in 0..CAPACITY + 1 {
            cache.put(format!("s{i}"), CurrentUser::test(3), 0, i64::MAX);
            assert!(cache.len() <= CAPACITY);
        }
    }

    /// Every watched table has its three triggers on a real connection, and
    /// a write empties the cache whatever the kind of table (`courses` is
    /// WITHOUT ROWID) or statement (a DELETE without WHERE).
    #[test]
    fn a_write_to_any_watched_table_forgets_the_cache() {
        let db = crate::db::Db::open_in_memory().unwrap();
        let cache = db.sessions();
        db.with(|c| {
            for table in WATCHED {
                let n: i64 = c.query_row("SELECT COUNT(*) FROM sqlite_temp_master WHERE type = 'trigger' AND tbl_name = ?1", [table], |r| r.get(0))?;
                assert_eq!(n, 3, "{table}");
                let sql: String = c.query_row("SELECT group_concat(sql) FROM sqlite_temp_master WHERE tbl_name = ?1", [table], |r| r.get(0))?;
                assert!(sql.contains("forget_sessions()"), "{table}");
            }
            cache.put("x".into(), CurrentUser::test(1), 0, i64::MAX);
            c.execute("INSERT INTO users (password_hash, created_at, email) VALUES ('', 0, 'a@example.org')", [])?;
            assert_eq!(cache.len(), 0, "users");
            cache.put("x".into(), CurrentUser::test(1), 0, i64::MAX);
            c.execute("INSERT INTO courses (id, owner_id, title, created_at, updated_at) VALUES ('c', 1, 'C', 1, 1)", [])?;
            assert_eq!(cache.len(), 0, "courses (WITHOUT ROWID)");
            cache.put("x".into(), CurrentUser::test(1), 0, i64::MAX);
            c.execute("DELETE FROM courses", [])?;
            assert_eq!(cache.len(), 0, "a DELETE without WHERE");
            Ok(())
        })
        .unwrap();
    }

    /// Every table the session query reads is watched: a new join or
    /// subquery in `user_for_session` fails here until its table is added
    /// to `WATCHED`, so writes to it forget the cache too.
    #[test]
    fn pragma_optimize_does_not_forget_the_cache() {
        let db = crate::db::Db::open_in_memory().unwrap();
        let cache = db.sessions();
        cache.put("x".into(), CurrentUser::test(1), 0, i64::MAX);
        db.with(|c| c.execute_batch("PRAGMA optimize;")).unwrap();
        assert!(cache.get("x", 1).is_some(), "optimize wrote to a watched table");
    }

    #[test]
    fn every_table_the_session_query_reads_is_watched() {
        let sql = super::super::session_sql();
        let words: Vec<&str> = sql.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).filter(|w| !w.is_empty()).collect();
        let tables: Vec<&str> = words
            .windows(2)
            .filter(|w| w[0].eq_ignore_ascii_case("FROM") || w[0].eq_ignore_ascii_case("JOIN"))
            .map(|w| w[1])
            .collect();
        assert!(tables.len() >= 4, "found {tables:?}");
        for t in &tables {
            assert!(WATCHED.contains(t), "user_for_session reads {t}, which WATCHED does not list");
        }
    }
}
