//! What the account agreed to, for the profile's Privacy and consent card.
//! Table `consents` (#974) once held one row per time a learner gave or
//! took back the optional AI features consent; since the AI features are
//! the server's to show or hide (`settings::AI_OFF`, #25) nothing writes
//! it, and the export and account deletion still take any rows along
//! (`privacy::USER_TABLES`).
//!
//! The Terms and the age confirmation are not optional (`terms.rs`,
//! `users.age_confirmed`); they stay where they are.

use rusqlite::{Connection, OptionalExtension};

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
