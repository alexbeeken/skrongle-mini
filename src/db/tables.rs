//! Which tables hold a learner's rows. What is stored where, and why, is in
//! `docs/data-inventory.md`; keep the two in step.

/// Every table that holds a learner's rows; every one cascades from
/// `users(id)`. A new table with user data must be added here (the test
/// fails until it is).
const USER_TABLES: &[&str] = &[
    "users",
    "sessions",
    "user_progress",
    "quiz_history",
    "journey_progress",
    "user_vocab",
    "lesson_passes",
    "game_scores",
    "game_activity",
    "activity_log",
    "lesson_touches",
    "lesson_notes",
    "earnings",
    "game_rounds",
    "projects",
    "totp_keys",
    "two_factor_challenges",
    "courses",
    "course_files",
    "course_undo",
    "course_undo_files",
    "course_ai_runs",
    "course_ai_run_files",
    "course_shares",
    "signup_invites",
    "api_grants",
    "api_tokens",
    "oauth_codes",
];

/// Tables that hold no learner's rows, only the server's:
/// `jobs` is the background job queue (jobs.rs, #872): housekeeping rows, no account.
/// `server_api_keys` and `settings` are the server's AI API keys and settings (#25), the owner's, no account.
/// `permission_presets` are the named permission sets accounts are given (#138, perms.rs): no account's rows.
/// `oauth_clients` are the apps that registered for OAuth sign-in (#169, oauth_server.rs): an app's name and addresses, no account.
const SHARED_TABLES: &[&str] = &["jobs", "server_api_keys", "settings", "permission_presets", "oauth_clients"];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    /// Every table of the schema is a user table listed in `USER_TABLES`
    /// (or a server-wide one in `SHARED_TABLES`).
    #[test]
    fn every_table_is_covered() {
        let db = Db::open_in_memory().unwrap();
        db.with(|c| {
            let mut stmt = c.prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")?;
            let names: Vec<String> = stmt.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
            for t in names.into_iter().filter(|t| !SHARED_TABLES.contains(&t.as_str())) {
                assert!(USER_TABLES.contains(&t.as_str()), "table {t} is not in db::tables::USER_TABLES (deletion, docs/data-inventory.md)");
            }
            Ok(())
        })
        .unwrap();
    }
}
