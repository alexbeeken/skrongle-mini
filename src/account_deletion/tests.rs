use super::*;

const DAY: i64 = 86_400;

/// Asking signs the account out everywhere and sets the date 14 days out;
/// a cancelled plan whose paid time runs longer pushes it to that end;
/// cancelling clears it; only due accounts are listed.
#[test]
fn scheduling_signs_out_and_waits_fourteen_days() {
    let db = crate::db::Db::open_in_memory().unwrap();
    let now = 1_800_000_000;
    let (ann, bea) = db
        .with(|c| {
            let ann = crate::auth::create_user(c, "ann@example.org", "x")?;
            let bea = crate::auth::create_user(c, "bea@example.org", "x")?;
            crate::auth::create_session(c, ann)?;
            crate::auth::create_session(c, ann)?;
            crate::auth::create_session(c, bea)?;
            Ok((ann, bea))
        })
        .unwrap();
    assert_eq!(db.with(|c| schedule(c, ann, now)).unwrap(), Some(now + 14 * DAY));
    let sessions = |u: i64| db.with(|c| c.query_row("SELECT COUNT(*) FROM sessions WHERE user_id = ?1", [u], |r| r.get::<_, i64>(0))).unwrap();
    assert_eq!((sessions(ann), sessions(bea)), (0, 1), "only that account is signed out");
    assert_eq!(db.with(|c| schedule(c, 999, now)).unwrap(), None);

    assert_eq!(db.with(|c| schedule(c, bea, now)).unwrap(), Some(now + 14 * DAY));

    assert!(db.with(|c| due(c, now + 14 * DAY - 1)).unwrap().is_empty());
    assert_eq!(db.with(|c| due(c, now + 14 * DAY)).unwrap(), vec![ann, bea]);
    assert!(db.with(|c| cancel(c, ann)).unwrap());
    assert!(!db.with(|c| cancel(c, ann)).unwrap(), "nothing left to cancel");
    assert_eq!(db.with(|c| due(c, now + 14 * DAY)).unwrap(), vec![bea]);
}

/// The due-accounts query reads the partial index, not the whole table.
#[test]
fn due_uses_its_index() {
    let db = crate::db::Db::open_in_memory().unwrap();
    let plan: String = db
        .with(|c| {
            let mut stmt = c.prepare("EXPLAIN QUERY PLAN SELECT id FROM users WHERE delete_at IS NOT NULL AND delete_at <= ?1 ORDER BY delete_at LIMIT ?2")?;
            let rows = stmt.query_map(params![0, BATCH], |r| r.get::<_, String>(3))?.collect::<Result<Vec<_>, _>>()?;
            Ok(rows.join("; "))
        })
        .unwrap();
    assert!(plan.contains("idx_users_delete_at"), "{plan}");
}
