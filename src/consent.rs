//! What the account agreed to, for the profile's Privacy and consent card:
//! the age confirmation at sign-up (`users.age_confirmed`). There is no
//! optional consent to record: the AI features are the server's to show or
//! hide (`settings::AI_OFF`, #25).

use rusqlite::{Connection, OptionalExtension};

/// When the account confirmed its age: at sign-up, so the day it was made.
/// `None` when it never did.
pub fn age_confirmed_at(conn: &Connection, user_id: i64) -> rusqlite::Result<Option<i64>> {
    conn.query_row(
        "SELECT created_at FROM users WHERE id = ?1 AND age_confirmed",
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
    fn age_date_is_when_the_account_was_made() {
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let ann = crate::auth::create_user(c, "ann@example.org", "h")?;
            assert_eq!(age_confirmed_at(c, ann)?, None);
            c.execute("UPDATE users SET created_at = 50 WHERE id = ?1", [ann])?;
            assert_eq!(age_confirmed_at(c, ann)?, None, "no age box ticked");
            crate::privacy::record_age_confirmed(c, ann, true)?;
            assert_eq!(age_confirmed_at(c, ann)?, Some(50));
            Ok(())
        })
        .unwrap();
    }
}
