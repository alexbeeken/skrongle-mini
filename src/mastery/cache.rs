//! Each learner's `user_progress` rows for the course they are on, kept in
//! memory (#38). Quiz picking, the lesson pages and the home page read a
//! learner's whole map many times a day; with at most
//! [`caps::MAX_ACCOUNTS`](crate::caps::MAX_ACCOUNTS) accounts the maps are
//! small, so they stay here instead of being read again each time.
//!
//! - One entry per user: the course last read. Reading another course
//!   replaces it, and when the cache holds `MAX_ACCOUNTS` users a new one
//!   drops any other.
//! - [`super::save`], the one function that writes a `user_progress` row,
//!   updates the entry after the write. Inside a transaction, which may
//!   still roll back, it drops the learner's entry instead;
//!   [`super::record_answer`] (its own transaction, the hot path) takes the
//!   entry out first and puts it back with the new row after the commit.
//! - Every other change to the table drops entries: course deletion
//!   ([`forget_course`]), the dev reset ([`forget_all`]).
//!   The test `every_user_progress_write_is_known` lists the files that
//!   write the table, so a new one is noticed.
//!
//! The cache belongs to a `Db`; `Db::call` and `Db::with` make it the
//! thread's current one while the closure runs, which is how the mastery
//! functions, given only a `&Connection`, find it. A bare `Connection`
//! (some tests) has none and reads the table every time.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use super::Progress;

/// A learner's progress rows for one course, by lesson id.
pub type ProgressMap = HashMap<String, Progress>;

struct Entry {
    course_id: String,
    map: Arc<ProgressMap>,
}

#[derive(Default)]
pub struct ProgressCache {
    entries: Mutex<HashMap<i64, Entry>>,
    /// `load_progress` reads of the table, for the tests.
    #[cfg(test)]
    pub(crate) reads: std::sync::atomic::AtomicUsize,
}

impl ProgressCache {
    fn entries(&self) -> MutexGuard<'_, HashMap<i64, Entry>> {
        self.entries.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The learner's map for `course_id`, if it is the one held.
    pub fn get(&self, user_id: i64, course_id: &str) -> Option<Arc<ProgressMap>> {
        self.entries().get(&user_id).filter(|e| e.course_id == course_id).map(|e| Arc::clone(&e.map))
    }

    /// One lesson's row from a held map: `Some(None)` when the map is held
    /// and has no row for it, `None` when the map is not held.
    pub fn get_one(&self, user_id: i64, course_id: &str, lesson_id: &str) -> Option<Option<Progress>> {
        self.entries().get(&user_id).filter(|e| e.course_id == course_id).map(|e| e.map.get(lesson_id).cloned())
    }

    /// Hold a map just read from the table, in place of the learner's
    /// other course; when full, a new learner drops any other one.
    pub fn fill(&self, user_id: i64, course_id: &str, map: Arc<ProgressMap>) {
        let mut entries = self.entries();
        if !entries.contains_key(&user_id) && entries.len() >= crate::caps::MAX_ACCOUNTS {
            if let Some(&any) = entries.keys().next() {
                entries.remove(&any);
            }
        }
        entries.insert(user_id, Entry { course_id: course_id.to_string(), map });
    }

    /// Take the learner's map for `course_id` out, if it is the one held.
    pub fn take(&self, user_id: i64, course_id: &str) -> Option<Arc<ProgressMap>> {
        let mut entries = self.entries();
        if entries.get(&user_id).is_some_and(|e| e.course_id == course_id) {
            entries.remove(&user_id).map(|e| e.map)
        } else {
            None
        }
    }

    /// A row was written: update the held map, if it is that course's.
    pub fn put(&self, user_id: i64, course_id: &str, p: &Progress) {
        if let Some(e) = self.entries().get_mut(&user_id).filter(|e| e.course_id == course_id) {
            Arc::make_mut(&mut e.map).insert(p.lesson_id.clone(), p.clone());
        }
    }

    pub fn forget_user(&self, user_id: i64) {
        self.entries().remove(&user_id);
    }

    pub fn forget_course(&self, course_id: &str) {
        self.entries().retain(|_, e| e.course_id != course_id);
    }

    #[cfg(any(feature = "dev", test))]
    pub fn clear(&self) {
        self.entries().clear();
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries().len()
    }
}

thread_local! {
    static CURRENT: RefCell<Option<Arc<ProgressCache>>> = const { RefCell::new(None) };
}

/// Run `f` with `cache` as this thread's current cache (`Db::call`, `Db::with`).
pub fn in_scope<T>(cache: &Arc<ProgressCache>, f: impl FnOnce() -> T) -> T {
    struct Restore(Option<Arc<ProgressCache>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let prev = self.0.take();
            CURRENT.with(|c| *c.borrow_mut() = prev);
        }
    }
    let prev = CURRENT.with(|c| c.borrow_mut().replace(Arc::clone(cache)));
    let _restore = Restore(prev);
    f()
}

/// This thread's current cache, if a `Db` call is running on it.
pub fn current() -> Option<Arc<ProgressCache>> {
    CURRENT.with(|c| c.borrow().clone())
}

/// Drop every entry: the dev reset (`/dev/reset`).
#[cfg(feature = "dev")]
pub fn forget_all() {
    if let Some(c) = current() {
        c.clear();
    }
}

/// Drop every entry for a course that is going away.
pub fn forget_course(course_id: &str) {
    if let Some(c) = current() {
        c.forget_course(course_id);
    }
}

#[cfg(test)]
mod tests {
    use super::super::{load_one, load_progress, mark_read, read_progress, record_answer, save, Progress};
    use crate::db::Db;
    use std::sync::atomic::Ordering;

    fn reads(db: &Db) -> usize {
        db.progress_cache().reads.load(Ordering::Relaxed)
    }

    fn ann(db: &Db) -> i64 {
        db.with(|c| crate::auth::create_user(c, "ann@example.org", "h")).unwrap()
    }

    #[test]
    fn a_warm_load_reads_nothing_and_sees_every_save() {
        let db = Db::open_in_memory().unwrap();
        let uid = ann(&db);
        db.with(|c| {
            assert!(load_progress(c, uid, "es")?.is_empty());
            assert!(load_progress(c, uid, "es")?.is_empty());
            assert_eq!(reads(&db), 1, "two loads, one read");

            save(c, uid, "es", &Progress { mastery_score: 42.0, ..Progress::new("l1") })?;
            assert_eq!(load_progress(c, uid, "es")?["l1"].mastery_score, 42.0);
            mark_read(c, uid, "es", "l2", 7)?;
            assert!(load_progress(c, uid, "es")?["l2"].is_read());
            record_answer(c, uid, "es", "l1", "q1", "a", true)?;
            let p = load_progress(c, uid, "es")?;
            assert_eq!(p["l1"].times_tested, 1);
            assert!(p["l1"].mastery_score > 42.0);
            assert_eq!(load_one(c, uid, "es", "l1")?.unwrap().times_tested, 1);
            assert_eq!(reads(&db), 1, "saves update the held map");
            Ok(())
        })
        .unwrap();
        // What the cache serves is what the table holds.
        let fresh = db.with(|c| read_progress(c, uid, "es")).unwrap();
        let held = db.with(|c| load_progress(c, uid, "es")).unwrap();
        assert_eq!(fresh.len(), held.len());
        for (id, p) in &fresh {
            let h = &held[id];
            assert_eq!((p.mastery_score, p.times_tested, p.streak, p.read_at), (h.mastery_score, h.times_tested, h.streak, h.read_at));
        }
    }

    #[test]
    fn a_rolled_back_save_is_never_served() {
        let db = Db::open_in_memory().unwrap();
        let uid = ann(&db);
        db.with(|c| {
            load_progress(c, uid, "es")?;
            let tx = c.transaction()?;
            save(&tx, uid, "es", &Progress { mastery_score: 99.0, ..Progress::new("l1") })?;
            drop(tx);
            assert!(load_progress(c, uid, "es")?.is_empty());
            Ok(())
        })
        .unwrap();
        assert_eq!(reads(&db), 2, "the save in a transaction dropped the map");
    }

    #[test]
    fn one_course_per_learner_and_at_most_the_account_cap() {
        let db = Db::open_in_memory().unwrap();
        db.with(|c| {
            let uid = crate::auth::create_user(c, "ann@example.org", "h")?;
            load_progress(c, uid, "es")?;
            load_progress(c, uid, "fr")?;
            load_progress(c, uid, "es")?;
            assert_eq!(db.progress_cache().len(), 1);
            assert_eq!(reads(&db), 3, "switching course drops the old one");
            for i in 0..crate::caps::MAX_ACCOUNTS + 2 {
                let other = crate::auth::create_user(c, &format!("u{i}@example.org"), "h")?;
                load_progress(c, other, "es")?;
            }
            assert_eq!(db.progress_cache().len(), crate::caps::MAX_ACCOUNTS);
            Ok(())
        })
        .unwrap();
    }

    #[cfg(feature = "dev")]
    #[test]
    fn deleting_a_course_or_resetting_drops_entries() {
        let db = Db::open_in_memory().unwrap();
        db.with(|c| {
            let uid = crate::auth::create_user(c, "ann@example.org", "h")?;
            let bob = crate::auth::create_user(c, "bob@example.org", "h")?;
            save(c, uid, "es", &Progress::new("l1"))?;
            load_progress(c, uid, "es")?;
            load_progress(c, bob, "fr")?;
            crate::courses::delete_course(c, "fr")?;
            assert_eq!(db.progress_cache().len(), 1, "course deletion");
            load_progress(c, bob, "es")?;
            crate::dev::reset_user(c, bob)?;
            assert_eq!(db.progress_cache().len(), 0, "dev reset");
            Ok(())
        })
        .unwrap();
    }

    /// Every file whose code names `user_progress`, and how the cache keeps
    /// up with it. A new one fails here: route its write through
    /// `mastery::save`, or drop the entries it changes (`cache::forget_*`).
    #[test]
    fn every_user_progress_write_is_known() {
        const KNOWN: &[(&str, &str)] = &[
            ("src/mastery.rs", "reads, and the one write, `save`, which updates the cache"),
            ("src/db/tables.rs", "lists it in USER_TABLES; it writes nothing"),
            ("src/dev.rs", "the dev reset deletes the rows and calls forget_all"),
        ];
        let mut found = Vec::new();
        let mut stack = vec![std::path::PathBuf::from("src")];
        while let Some(dir) = stack.pop() {
            for e in std::fs::read_dir(&dir).unwrap() {
                let path = e.unwrap().path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                let name = path.to_string_lossy().replace('\\', "/");
                if !name.ends_with(".rs") || name.ends_with("tests.rs") || name.contains("/tests/") {
                    continue;
                }
                let text = std::fs::read_to_string(&path).unwrap();
                let code = text.split("#[cfg(test)]\nmod tests").next().unwrap_or("");
                if code.lines().any(|l| l.contains("user_progress") && !l.trim_start().starts_with("//")) {
                    found.push(name);
                }
            }
        }
        found.sort();
        let mut known: Vec<String> = KNOWN.iter().map(|(f, _)| f.to_string()).collect();
        known.sort();
        assert_eq!(found, known, "a file names user_progress: keep the progress cache (src/mastery/cache.rs) in step");
    }
}
