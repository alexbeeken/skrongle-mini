//! Which tables hold a learner's rows. What is stored where, and why, is in
//! `docs/data-inventory.md`; keep the two in step.


/// Every table that holds a learner's rows, with the condition that selects
/// them (`?1` is the user id); every one cascades from `users(id)`. A new table
/// with user data must be added here (the tests fail until it is).
#[cfg_attr(not(test), allow(dead_code))] // the tests read it: they check every table is listed
pub(crate) const USER_TABLES: &[(&str, &str)] = &[
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
    ("game_rounds", "user_id = ?1"),
    ("projects", "user_id = ?1"),
    ("totp_keys", "user_id = ?1"),
    ("two_factor_challenges", "user_id = ?1"),
    ("courses", "owner_id = ?1"),
    ("course_files", "course_id IN (SELECT id FROM courses WHERE owner_id = ?1)"),
    ("course_undo", "course_id IN (SELECT id FROM courses WHERE owner_id = ?1)"),
    ("course_undo_files", "course_id IN (SELECT id FROM courses WHERE owner_id = ?1)"),
    ("course_ai_runs", "course_id IN (SELECT id FROM courses WHERE owner_id = ?1)"),
    ("course_ai_run_files", "course_id IN (SELECT id FROM courses WHERE owner_id = ?1)"),
    ("course_shares", "course_id IN (SELECT id FROM courses WHERE owner_id = ?1) OR user_id = ?1"),
    ("signup_invites", "created_by = ?1 OR used_by = ?1"),
];

/// Tables that hold no learner's rows, only the server's:
/// `jobs` is the background job queue (jobs.rs, #872): housekeeping rows, no account.
/// `server_api_keys` and `settings` are the server's AI API keys and settings (#25), the owner's, no account.
/// `permission_presets` are the named permission sets accounts are given (#138, perms.rs): no account's rows.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) const SHARED_TABLES: &[&str] = &["jobs", "server_api_keys", "settings", "permission_presets"];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    /// Every table of the schema is a user table listed in `USER_TABLES`
    /// (or a server-wide one in `SHARED_TABLES`).
    #[test]
    fn every_table_is_covered() {
        let db = Db::open_in_memory().unwrap();
        let listed: Vec<&str> = USER_TABLES.iter().map(|(t, _)| *t).collect();
        db.with(|c| {
            let mut stmt = c.prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")?;
            let names: Vec<String> = stmt.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
            for t in names.into_iter().filter(|t| !SHARED_TABLES.contains(&t.as_str())) {
                assert!(listed.contains(&t.as_str()), "table {t} is not in db::tables::USER_TABLES (deletion, docs/data-inventory.md)");
            }
            Ok(())
        })
        .unwrap();
    }
}
