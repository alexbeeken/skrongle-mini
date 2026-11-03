//! How long data is kept: the retention rules the hourly housekeeping applies
//! (`jobs::housekeeping`). What is stored where, and why, is in `docs/data-inventory.md`.

use rusqlite::Connection;

/// `SKRONGLE_FAILED_COURSE_RETENTION_DAYS` when unset.
pub const DEFAULT_FAILED_COURSE_DAYS: i64 = 30;

/// `SKRONGLE_RETAIN_HISTORY_DAYS` when unset (#381): two years of answers and
/// finished rounds, so a script answering for an account cannot keep its
/// rows forever, while a learner's streak and history stay whole.
pub const DEFAULT_HISTORY_DAYS: i64 = 730;

/// How long data is kept, from the environment. The periods are
/// placeholders until the owner decides them (TODO.md, "Decide the privacy
/// facts").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Retention {
    /// `SKRONGLE_RETAIN_HISTORY_DAYS` (default [`DEFAULT_HISTORY_DAYS`]):
    /// answers in `quiz_history` and finished rounds in `game_activity`
    /// older than this are deleted. `None` (0): keep them.
    pub history_days: Option<i64>,
    /// `SKRONGLE_KEEP_NOTES=1`: keep a finished AI course's notes (so revisions
    /// see the original material). Default: delete them once it is ready.
    pub keep_notes: bool,
    /// `SKRONGLE_FAILED_COURSE_RETENTION_DAYS` (default 30): an AI course whose
    /// generation failed, untouched for this long, is deleted with its notes
    /// and files. `None` (0): keep them.
    pub failed_course_days: Option<i64>,
}

impl Retention {
    pub fn from_env() -> Retention {
        let days = |name: &str| crate::env(name).and_then(|v| v.trim().parse::<i64>().ok());
        Retention {
            history_days: Some(days("RETAIN_HISTORY_DAYS").unwrap_or(DEFAULT_HISTORY_DAYS)).filter(|d| *d > 0),
            keep_notes: crate::env("KEEP_NOTES").is_some_and(|v| v == "1" || v == "true"),
            failed_course_days: Some(days("FAILED_COURSE_RETENTION_DAYS").unwrap_or(DEFAULT_FAILED_COURSE_DAYS)).filter(|d| *d > 0),
        }
    }
}

/// What one sweep removed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Swept {
    /// Answers deleted from `quiz_history`.
    pub answers: usize,
    /// Finished rounds deleted from `game_activity`.
    pub rounds: usize,
    /// Finished AI courses whose notes were emptied.
    pub notes: usize,
    /// Failed AI courses deleted (their cached `ContentStore`s must go).
    pub failed_courses: Vec<String>,
}

/// Apply the retention rules: old answers and finished rounds go, the notes of AI courses whose
/// generation finished successfully are emptied (a failed one keeps them so
/// it can resume), and a failed course nobody resumed or edited within
/// `failed_course_days` is deleted, notes and all.
pub fn apply_retention(conn: &Connection, r: &Retention, now: i64) -> rusqlite::Result<Swept> {
    let (answers, rounds) = match r.history_days {
        Some(days) => {
            // The daily goal's log goes with them: it is read for today, a
            // run or the last week; the running totals are in `earnings`.
            conn.execute("DELETE FROM activity_log WHERE at < ?1", [now - days * 86_400])?;
            (
                conn.execute("DELETE FROM quiz_history WHERE answered_at < ?1", [now - days * 86_400])?,
                conn.execute("DELETE FROM game_activity WHERE played_at < ?1", [now - days * 86_400])?,
            )
        }
        None => (0, 0),
    };
    let notes = if r.keep_notes { 0 } else { conn.execute("UPDATE courses SET notes = '' WHERE state = 'ready' AND notes <> ''", [])? };
    let mut failed_courses = Vec::new();
    if let Some(days) = r.failed_course_days {
        let mut stmt = conn.prepare("SELECT id FROM courses WHERE state = 'failed' AND updated_at < ?1 ORDER BY id")?;
        failed_courses = stmt.query_map([now - days * 86_400], |r| r.get::<_, String>(0))?.collect::<Result<_, _>>()?;
        for id in &failed_courses {
            crate::courses::delete_course(conn, id)?;
        }
    }
    Ok(Swept { answers, rounds, notes, failed_courses })
}

#[cfg(test)]
mod tests;
