//! Optional consents (table `consents`, #974): one row per time a learner
//! gave one, with `withdrawn_at` set when they took it back, so "you can
//! withdraw at any time" has a record behind it. The profile's Privacy and
//! consent card shows them; the export and account deletion take the rows
//! along with the rest (`privacy::USER_TABLES`).
//!
//! The Terms and the age confirmation are not optional (`terms.rs`,
//! `users.age_confirmed`); they stay where they are.

use rusqlite::{params, Connection, OptionalExtension};

use crate::db::now;

/// AI features: text the learner writes may go to an AI provider. On unless
/// hidden at sign-up or on the profile (`users.ai_off`).
pub const AI: &str = "ai";

/// The latest row for a purpose: given at `granted_at`, still in force while
/// `withdrawn_at` is `None`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
    pub granted_at: i64,
    pub withdrawn_at: Option<i64>,
}

/// Give (`true`) or withdraw (`false`) a consent now. Giving one already in
/// force, or withdrawing one that is not, changes nothing.
pub fn record(conn: &Connection, user_id: i64, purpose: &str, granted: bool) -> rusqlite::Result<()> {
    if granted {
        conn.execute(
            "INSERT INTO consents (user_id, purpose, granted_at)
             SELECT ?1, ?2, ?3 WHERE NOT EXISTS
                 (SELECT 1 FROM consents WHERE user_id = ?1 AND purpose = ?2 AND withdrawn_at IS NULL)",
            params![user_id, purpose, now()],
        )?;
    } else {
        conn.execute(
            "UPDATE consents SET withdrawn_at = ?3 WHERE user_id = ?1 AND purpose = ?2 AND withdrawn_at IS NULL",
            params![user_id, purpose, now()],
        )?;
    }
    Ok(())
}

/// The most recent row for a purpose, if it was ever given.
pub fn latest(conn: &Connection, user_id: i64, purpose: &str) -> rusqlite::Result<Option<Record>> {
    conn.query_row(
        "SELECT granted_at, withdrawn_at FROM consents WHERE user_id = ?1 AND purpose = ?2 ORDER BY granted_at DESC, id DESC LIMIT 1",
        params![user_id, purpose],
        |r| Ok(Record { granted_at: r.get(0)?, withdrawn_at: r.get(1)? }),
    )
    .optional()
}

/// When the account confirmed its age: at sign-up, or on the Terms page for
/// an account from before the box, both alongside a Terms acceptance. `None`
/// when it never did.
pub fn age_confirmed_at(conn: &Connection, user_id: i64) -> rusqlite::Result<Option<i64>> {
    conn.query_row(
        "SELECT (SELECT MIN(accepted_at) FROM terms_acceptances WHERE user_id = u.id) FROM users u WHERE u.id = ?1 AND u.age_confirmed",
        [user_id],
        |r| r.get(0),
    )
    .optional()
    .map(Option::flatten)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn giving_and_withdrawing_keeps_each_period() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let ann = crate::auth::create_user(c, "ann@example.org", "h")?;
            assert_eq!(latest(c, ann, AI)?, None);
            record(c, ann, AI, false)?;
            assert_eq!(latest(c, ann, AI)?, None, "withdrawing what was never given records nothing");
            record(c, ann, AI, true)?;
            record(c, ann, AI, true)?;
            let first = latest(c, ann, AI)?.unwrap();
            assert_eq!(first.withdrawn_at, None);
            record(c, ann, AI, false)?;
            assert!(latest(c, ann, AI)?.unwrap().withdrawn_at.is_some());
            c.execute("UPDATE consents SET granted_at = granted_at - 10, withdrawn_at = withdrawn_at - 5", [])?;
            record(c, ann, AI, true)?;
            assert_eq!(latest(c, ann, AI)?.unwrap().withdrawn_at, None);
            let rows: i64 = c.query_row("SELECT COUNT(*) FROM consents WHERE user_id = ?1", [ann], |r| r.get(0))?;
            assert_eq!(rows, 2);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn age_date_is_the_first_terms_acceptance() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let ann = crate::auth::create_user(c, "ann@example.org", "h")?;
            assert_eq!(age_confirmed_at(c, ann)?, None);
            c.execute("INSERT INTO terms_acceptances (user_id, version, accepted_at) VALUES (?1, 'a', 50), (?1, 'b', 90)", [ann])?;
            assert_eq!(age_confirmed_at(c, ann)?, None, "terms accepted but no age box ticked");
            crate::privacy::record_age_confirmed(c, ann, true)?;
            assert_eq!(age_confirmed_at(c, ann)?, Some(50));
            Ok(())
        })
        .unwrap();
    }
}
