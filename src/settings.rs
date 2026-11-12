//! Server-wide settings the owner changes on the Manage Users page (table
//! `settings`, one row per name, #25): [`AI_OFF`], the switch that hides
//! every AI feature for every account, and [`OLLAMA_MODEL`]. A name with no
//! row has its default.

use rusqlite::{params, Connection, OptionalExtension};

/// "Hide AI features": `1` hides the tutor, course generation, Edit with AI
/// and translation for everyone, and their routes refuse
/// (`CurrentUser::require_ai`). No row, or `0`: shown.
pub const AI_OFF: &str = "ai_off";

/// "Let apps connect" (#167): `1` lets an account with Create courses
/// connect an app or an assistant (`auth::api`). No row, or `0`: off, and
/// every API route answers 404 and every token stops working.
pub const APPS_ON: &str = "apps_on";

/// The model the Ollama-compatible server runs (#24), set with its base URL
/// on Manage Users (`secrets::validate_model`). No row: `ai::OLLAMA_DEFAULT_MODEL`.
pub const OLLAMA_MODEL: &str = "ollama_model";

/// The stored value of a setting; `None` when it was never set.
pub fn get(conn: &Connection, name: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT value FROM settings WHERE name = ?1", [name], |r| r.get(0)).optional()
}

/// Store a setting (or replace it).
pub fn set(conn: &Connection, name: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO settings (name, value) VALUES (?1, ?2) ON CONFLICT(name) DO UPDATE SET value = excluded.value",
        params![name, value],
    )?;
    Ok(())
}

/// Are the AI features hidden on this server?
pub fn ai_off(conn: &Connection) -> rusqlite::Result<bool> {
    Ok(get(conn, AI_OFF)?.as_deref() == Some("1"))
}

/// Hide (`true`) or show the AI features for every account.
pub fn set_ai_off(conn: &Connection, off: bool) -> rusqlite::Result<()> {
    set(conn, AI_OFF, if off { "1" } else { "0" })
}

/// May apps connect to this server?
pub fn apps_on(conn: &Connection) -> rusqlite::Result<bool> {
    Ok(get(conn, APPS_ON)?.as_deref() == Some("1"))
}

/// Let apps connect (`true`) or not.
pub fn set_apps_on(conn: &Connection, on: bool) -> rusqlite::Result<()> {
    set(conn, APPS_ON, if on { "1" } else { "0" })
}

/// [`apps_on`] as SQL, for the token query (`auth::api::user_for_token`)
/// and the session query, so neither pays a query of its own.
pub const APPS_ON_SQL: &str = "COALESCE((SELECT value = '1' FROM settings WHERE name = 'apps_on'), 0)";

/// [`ai_off`] as SQL, for the session query (`auth::session_sql`); the
/// session cache watches `settings`, so flipping the switch reaches every
/// signed-in account at its next request.
pub const AI_OFF_SQL: &str = "COALESCE((SELECT value = '1' FROM settings WHERE name = 'ai_off'), 0)";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai_is_shown_until_hidden() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            assert!(!ai_off(c)?);
            set_ai_off(c, true)?;
            assert!(ai_off(c)?);
            let sql: bool = c.query_row(&format!("SELECT {AI_OFF_SQL}"), [], |r| r.get(0))?;
            assert!(sql);
            set_ai_off(c, false)?;
            assert!(!ai_off(c)?);
            assert_eq!(get(c, AI_OFF)?.as_deref(), Some("0"));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn apps_are_off_until_turned_on() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            assert!(!apps_on(c)?);
            let sql = |c: &Connection| c.query_row(&format!("SELECT {APPS_ON_SQL}"), [], |r| r.get::<_, bool>(0));
            assert!(!sql(c)?);
            set_apps_on(c, true)?;
            assert!(apps_on(c)? && sql(c)?);
            set_apps_on(c, false)?;
            assert!(!apps_on(c)? && !sql(c)?);
            Ok(())
        })
        .unwrap();
    }
}
