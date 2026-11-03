//! A learner's data as a whole: account deletion, the retention rules the hourly housekeeping applies, and
//! the age confirmation at sign-up. What is stored where, and why, is in
//! `docs/data-inventory.md`; keep the two in step.

use rusqlite::{params, Connection};

/// Every table that holds a learner's rows, with the condition that selects
/// them (`?1` is the user id). Deleting the `users` row removes all of them
/// through `ON DELETE CASCADE`. A new table
/// with user data must be added here (the tests fail until it is).
#[cfg_attr(not(test), allow(dead_code))] // the tests read it: they check every table is listed and that deletion clears each
pub const USER_TABLES: &[(&str, &str)] = &[
    ("users", "id = ?1"),
    ("sessions", "user_id = ?1"),
    ("user_progress", "user_id = ?1"),
    ("quiz_history", "user_id = ?1"),
    ("journey_progress", "user_id = ?1"),
    ("user_vocab", "user_id = ?1"),
    ("lesson_passes", "user_id = ?1"),
    ("game_scores", "user_id = ?1"),
    ("game_activity", "user_id = ?1"),
    ("activity_log", "user_id = ?1"),
    ("lesson_touches", "user_id = ?1"),
    ("lesson_notes", "user_id = ?1"),
    ("earnings", "user_id = ?1"),
    ("motivator_purchases", "user_id = ?1"),
    ("game_rounds", "user_id = ?1"),
    ("projects", "user_id = ?1"),
    ("user_api_keys", "user_id = ?1"),
    ("subscriptions", "user_id = ?1"),
    ("plan_grants", "user_id = ?1"),
    ("plan_cancellations", "user_id = ?1"),
    ("ai_usage", "user_id = ?1"),
    ("ai_usage_months", "user_id = ?1"),
    ("ai_credit_purchases", "user_id = ?1"),
    ("terms_acceptances", "user_id = ?1"),
    ("terms_announcements", "user_id = ?1"),
    ("breach_notices", "user_id = ?1"),
    ("consents", "user_id = ?1"),
    ("mail_outbox", "user_id = ?1"),
    ("sms_outbox", "user_id = ?1"),
    ("sms_sends", "user_id = ?1"),
    ("email_verifications", "user_id = ?1"),
    ("password_resets", "user_id = ?1"),
    ("totp_keys", "user_id = ?1"),
    ("phone_verifications", "user_id = ?1"),
    ("two_factor_challenges", "user_id = ?1"),
    ("two_factor_confirmations", "user_id = ?1"),
    ("courses", "owner_id = ?1"),
    ("course_files", "course_id IN (SELECT id FROM courses WHERE owner_id = ?1)"),
    ("course_undo", "course_id IN (SELECT id FROM courses WHERE owner_id = ?1)"),
    ("course_undo_files", "course_id IN (SELECT id FROM courses WHERE owner_id = ?1)"),
    ("course_ai_runs", "course_id IN (SELECT id FROM courses WHERE owner_id = ?1)"),
    ("course_ai_run_files", "course_id IN (SELECT id FROM courses WHERE owner_id = ?1)"),
    ("course_shares", "course_id IN (SELECT id FROM courses WHERE owner_id = ?1) OR user_id = ?1"),
    ("course_flags", "course_id IN (SELECT id FROM courses WHERE owner_id = ?1)"),
    ("content_scores", "course_id IN (SELECT id FROM courses WHERE owner_id = ?1)"),
    ("content_category_scores", "course_id IN (SELECT id FROM courses WHERE owner_id = ?1)"),
    ("content_checks", "course_id IN (SELECT id FROM courses WHERE owner_id = ?1)"),
    ("user_themes", "owner_id = ?1"),
    ("user_theme_files", "theme_id IN (SELECT id FROM user_themes WHERE owner_id = ?1)"),
    ("data_exports", "user_id = ?1"),
    ("signup_invites", "created_by = ?1 OR used_by = ?1"),
    ("content_reports", "user_id = ?1"),
    ("privacy_actions", "superuser_id = ?1"),
    ("waitlist", "user_id = ?1"),
];

/// User tables whose rows outlive the account: deleting it sets their
/// `user_id` to NULL instead (`ON DELETE SET NULL`). `sms_sends` is
/// the leftover texts' rate-limit ledger; its table goes with the schema
/// squash. `privacy_actions` is the log of privacy requests
/// answered (#981): a superuser's rows stay, without their id.
#[cfg_attr(not(test), allow(dead_code))]
pub const KEPT_ON_DELETE: &[&str] = &["sms_sends", "privacy_actions"];

/// Tables that hold no learner's rows, only server-wide figures:
/// `ai_spend_days` is everyone's hosted AI spend per day (the daily cap,
/// #623). An account's deletion only lowers it.
/// `blocked_ips` and `content_check_worker` are unused tables (#9), tied to no account; they go with the schema squash (#2).
/// `jobs` is the background job queue (jobs.rs, #872): housekeeping rows, no account.
/// `release_settings` is the waitlist switch (#934).
#[cfg_attr(not(test), allow(dead_code))]
pub const SHARED_TABLES: &[&str] = &["ai_spend_days", "blocked_ips", "jobs", "content_check_worker", "release_settings"];

/// What deleting an account removed that lives in caches too.
#[derive(Debug, Default, PartialEq)]
pub struct Deleted {
    /// The user's courses (their cached `ContentStore`s must go).
    pub courses: Vec<String>,
}

/// Delete the account and everything it owns, in one transaction. Every
/// user table cascades from `users(id)` (the courses' files through their
/// parent rows); the few rows that only name a course by id (`projects`,
/// `users.active_course`) are cleared here. `None` when there was no such user.
pub fn delete_account(conn: &mut Connection, user_id: i64) -> rusqlite::Result<Option<Deleted>> {
    let tx = conn.transaction()?;
    let courses: Vec<String> = {
        let mut stmt = tx.prepare("SELECT id FROM courses WHERE owner_id = ?1 ORDER BY id")?;
        let v = stmt.query_map([user_id], |r| r.get(0))?.collect::<Result<_, _>>()?;
        v
    };
    for c in &courses {
        tx.execute("DELETE FROM projects WHERE course_id = ?1", [c])?;
        tx.execute("UPDATE users SET active_course = NULL WHERE active_course = ?1", [c])?;
    }
    // Recent verification mail outlives the account for its day (#395).
    crate::mail::keep_for_caps(&tx, user_id, crate::db::now())?;
    let n = tx.execute("DELETE FROM users WHERE id = ?1", [user_id])?;
    tx.commit()?;
    // Its `user_progress` rows went with the cascade (#38).
    crate::mastery::cache::forget_user(user_id);
    Ok((n > 0).then_some(Deleted { courses }))
}

// ---------- retention ----------

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

// ---------- age at sign-up ----------

/// The "I am at least N" confirmation at sign-up. It is part of the Terms
/// checkbox ("I am at least N and agree to the Terms and Privacy Policy",
/// #72), which registration requires, so
/// the gate is always on; N is `SKRONGLE_MIN_AGE` (`terms::min_age`, 14 by
/// default, #976). `check` refuses a sign-up without the
/// box, and `record_age_confirmed` stores it on the new account.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AgeGate {
    pub min_age: u32,
}

impl AgeGate {
    /// The gate as this server is configured.
    pub fn current() -> AgeGate {
        AgeGate { min_age: crate::terms::min_age() }
    }

    /// The sign-up forms' two boxes: "I am at least N years old" (`age`)
    /// and "I agree to the Terms" (`accept`). `Err` is the sentence for the
    /// form, naming the missing box; `to` ends it ("register", "continue").
    pub fn check(&self, age: bool, accept: bool, to: &str) -> Result<(), String> {
        let min_age = self.min_age;
        let to = match to {
            "register" => crate::t("auth.age_gate.to_register"),
            "continue" => crate::t("auth.age_gate.to_continue"),
            other => other.to_string(),
        };
        match (age, accept) {
            (true, true) => Ok(()),
            (false, false) => Err(crate::t2("auth.age_gate.both", "min_age", min_age, "to", to)),
            (false, true) => Err(crate::t2("auth.age_gate.age", "min_age", min_age, "to", to)),
            (true, false) => Err(crate::t1("auth.age_gate.terms", "to", to)),
        }
    }
}

/// Store the confirmation on the account (`users.age_confirmed`).
pub fn record_age_confirmed(conn: &Connection, user_id: i64, confirmed: bool) -> rusqlite::Result<()> {
    conn.execute("UPDATE users SET age_confirmed = ?2 WHERE id = ?1", params![user_id, confirmed])?;
    Ok(())
}

#[cfg(test)]
mod tests;
