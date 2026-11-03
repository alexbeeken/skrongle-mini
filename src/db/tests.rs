use super::*;

/// Test aid (#607-#613): `sql`'s query plan (`EXPLAIN QUERY PLAN`, its
/// parameters left unbound), asserting that `table` is only reached
/// through `index`: never a full scan, and no other index.
pub(crate) fn assert_plan_uses(conn: &Connection, sql: &str, table: &str, index: &str) -> Vec<String> {
    let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
    let mut rows = stmt.raw_query();
    let mut plan = Vec::new();
    while let Some(r) = rows.next().unwrap() {
        plan.push(r.get::<_, String>(3).unwrap());
    }
    let on_table: Vec<&String> = plan.iter().filter(|l| l.split(' ').nth(1) == Some(table)).collect();
    assert!(!on_table.is_empty(), "{table} is not in the plan of {sql}: {plan:?}");
    for line in on_table {
        assert!(line.starts_with("SEARCH ") && line.contains(&format!(" INDEX {index} ")), "{sql}\nreaches {table} without {index}: {line}");
    }
    plan
}

/// Deleting a parent row (a user, a course) runs its `ON DELETE`
/// actions as `DELETE … WHERE <fk> = ?` on every child table: each
/// foreign key needs an index that starts with its columns (or is the
/// child's rowid), or deleting one account reads a whole table (#620).
#[test]
fn every_foreign_key_is_indexed() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let tables: Vec<String> =
            c.prepare("SELECT name FROM sqlite_master WHERE type = 'table'")?.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
        let mut missing = Vec::new();
        for t in &tables {
            let mut fks: BTreeMap<i64, Vec<String>> = BTreeMap::new();
            let mut s = c.prepare(&format!("SELECT id, \"from\" FROM pragma_foreign_key_list('{t}') ORDER BY id, seq"))?;
            for row in s.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))? {
                let (id, col) = row?;
                fks.entry(id).or_default().push(col);
            }
            let rowid: Vec<String> = c
                .prepare(&format!("SELECT name FROM pragma_table_info('{t}') WHERE pk = 1 AND type = 'INTEGER'"))?
                .query_map([], |r| r.get(0))?
                .collect::<Result<_, _>>()?;
            let indexes: Vec<String> =
                c.prepare(&format!("SELECT name FROM pragma_index_list('{t}')"))?.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
            let mut index_cols = Vec::new();
            for i in &indexes {
                // An expression column (`lower(to_addr)`) has no name; it covers no key.
                let cols: Vec<String> = c
                    .prepare(&format!("SELECT name FROM pragma_index_info('{i}') ORDER BY seqno"))?
                    .query_map([], |r| Ok(r.get::<_, Option<String>>(0)?.unwrap_or_default()))?
                    .collect::<Result<_, _>>()?;
                index_cols.push(cols);
            }
            for cols in fks.values() {
                let covered = (cols.len() == 1 && rowid == *cols) || index_cols.iter().any(|ic| ic.len() >= cols.len() && ic[..cols.len()] == cols[..]);
                if !covered {
                    missing.push(format!("{t}({})", cols.join(", ")));
                }
            }
        }
        assert!(missing.is_empty(), "foreign keys without an index: {missing:?}");
        Ok(())
    })
    .unwrap();
}

#[test]
fn the_fallout_theme_is_renamed_for_learners() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    for (i, m) in MIGRATIONS.iter().take(15).enumerate() {
        conn.execute_batch(m).unwrap();
        conn.execute_batch(&format!("PRAGMA user_version = {};", i + 1)).unwrap();
    }
    conn.execute("INSERT INTO users (id, username, password_hash, created_at, theme) VALUES (1, 'ann', 'h', 1, 'fallout')", []).unwrap();
    conn.execute("INSERT INTO quiz_history (user_id, lesson_id, question_id, selected_answer, was_correct, answered_at) VALUES (1, 'a1-x', 'a1-tener-fallout-01', 'a', 1, 1)", []).unwrap();
    migrate(&conn).unwrap();
    let theme: String = conn.query_row("SELECT theme FROM users WHERE id = 1", [], |r| r.get(0)).unwrap();
    let q: String = conn.query_row("SELECT question_id FROM quiz_history", [], |r| r.get(0)).unwrap();
    assert_eq!((theme.as_str(), q.as_str()), ("rustwake", "a1-tener-rustwake-01"));
}

#[test]
fn migrations_apply_once() {
    let db = Db::open_in_memory().unwrap();
    let v: i64 = db.with(|c| c.query_row("PRAGMA user_version", [], |r| r.get(0))).unwrap();
    assert_eq!(v, MIGRATIONS.len() as i64);
    // Re-running is a no-op.
    db.with(|c| migrate(c)).unwrap();
}

#[test]
fn v14_renames_the_stardew_theme() {
    let conn = Connection::open_in_memory().unwrap();
    for sql in &MIGRATIONS[..13] {
        conn.execute_batch(sql).unwrap();
    }
    conn.execute_batch("PRAGMA user_version = 13;").unwrap();
    for (name, theme) in [("ann", "stardew"), ("bob", "wow")] {
        conn.execute("INSERT INTO users (username, password_hash, created_at, theme) VALUES (?1, 'h', 1, ?2)", rusqlite::params![name, theme]).unwrap();
    }
    conn.execute("INSERT INTO quiz_history (user_id, lesson_id, question_id, selected_answer, was_correct, answered_at) VALUES (1, 'a1-ir', 'a1-ir-stardew-01', 'a', 1, 1)", []).unwrap();
    migrate(&conn).unwrap();
    let theme = |id: i64| -> String { conn.query_row("SELECT theme FROM users WHERE id = ?1", [id], |r| r.get(0)).unwrap() };
    assert_eq!((theme(1), theme(2)), ("meadowbrook".to_string(), "realm".to_string()));
    let q: String = conn.query_row("SELECT question_id FROM quiz_history", [], |r| r.get(0)).unwrap();
    assert_eq!(q, "a1-ir-meadowbrook-01");
}

/// #363: where two accounts had verified one number, the migration keeps
/// the first verification and undoes the later one (and its text-message
/// second step) before the unique index.
#[test]
fn the_sms_ledger_migration_keeps_one_verification_per_number() {
    let v = MIGRATIONS.iter().position(|m| m.contains("CREATE TABLE sms_sends")).unwrap();
    let conn = Connection::open_in_memory().unwrap();
    for sql in &MIGRATIONS[..v] {
        conn.execute_batch(sql).unwrap();
    }
    conn.execute_batch(&format!("PRAGMA user_version = {v};")).unwrap();
    for (id, at, method) in [(1, 20, "sms"), (2, 10, "email"), (3, 30, "sms")] {
        conn.execute(
            "INSERT INTO users (id, password_hash, created_at, email, phone, phone_verified_at, two_factor) VALUES (?1, 'h', 1, ?2, '+447700900123', ?3, ?4)",
            rusqlite::params![id, format!("u{id}@example.org"), at, method],
        )
        .unwrap();
    }
    migrate(&conn).unwrap();
    let rows: Vec<(Option<i64>, String)> = (1..=3)
        .map(|id| conn.query_row("SELECT phone_verified_at, two_factor FROM users WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).unwrap())
        .collect();
    assert_eq!(rows, vec![(None, String::new()), (Some(10), "email".to_string()), (None, String::new())]);
    assert!(conn.execute("UPDATE users SET phone_verified_at = 1 WHERE id = 1", []).is_err());
}

/// #623, #624: the AI totals migration fills the month and day totals
/// from the usage rows already there.
#[test]
fn the_ai_totals_migration_backfills_months_and_days() {
    let v = MIGRATIONS.iter().position(|m| m.contains("CREATE TABLE ai_usage_months")).unwrap();
    let conn = Connection::open_in_memory().unwrap();
    for sql in &MIGRATIONS[..v] {
        conn.execute_batch(sql).unwrap();
    }
    conn.execute_batch(&format!("PRAGMA user_version = {v};")).unwrap();
    conn.execute("INSERT INTO users (id, password_hash, created_at, email) VALUES (1, 'h', 1, 'a@example.org')", []).unwrap();
    // 2026-09-30 23:59:59, 2026-10-01 00:00:00 and 2026-10-01 12:00 UTC.
    for (at, cost, credit, source) in [(1_790_812_799, 100, 0, "hosted"), (1_790_812_800, 20, 5, "hosted"), (1_790_856_000, 3, 0, "hosted"), (1_790_856_000, 0, 0, "own")] {
        conn.execute(
            "INSERT INTO ai_usage (user_id, purpose, provider, model, cost_micros, credit_micros, key_source, created_at) VALUES (1, 'tutor-turn', 'anthropic', 'm', ?1, ?2, ?3, ?4)",
            rusqlite::params![cost, credit, source, at],
        )
        .unwrap();
    }
    migrate(&conn).unwrap();
    let months: Vec<(i64, i64, i64, i64, i64)> = conn
        .prepare("SELECT month, cost_micros, credit_micros, hosted_calls, own_calls FROM ai_usage_months ORDER BY month")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(months, vec![(1_788_220_800, 100, 0, 1, 0), (1_790_812_800, 23, 5, 2, 1)]);
    let days: Vec<(i64, i64)> =
        conn.prepare("SELECT day, cost_micros FROM ai_spend_days ORDER BY day").unwrap().query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().collect::<Result<_, _>>().unwrap();
    assert_eq!(days, vec![(1_790_726_400, 100), (1_790_812_800, 23)]);
}

#[test]
fn v18_renames_the_wow_theme_and_its_question_ids() {
    let conn = Connection::open_in_memory().unwrap();
    for sql in &MIGRATIONS[..17] {
        conn.execute_batch(sql).unwrap();
    }
    conn.execute_batch("PRAGMA user_version = 17;").unwrap();
    conn.execute("INSERT INTO users (username, password_hash, created_at, theme) VALUES ('ann', 'h', 1, 'wow')", []).unwrap();
    conn.execute("INSERT INTO users (username, password_hash, created_at, theme) VALUES ('bob', 'h', 1, 'rustwake')", []).unwrap();
    conn.execute("INSERT INTO quiz_history (user_id, lesson_id, question_id, selected_answer, was_correct, answered_at) VALUES (1, 'a1-x', 'a1-x-wow-01', 'a', 1, 1)", []).unwrap();
    migrate(&conn).unwrap();
    let themes: Vec<String> = conn.prepare("SELECT theme FROM users ORDER BY id").unwrap().query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
    assert_eq!(themes, ["realm", "rustwake"]);
    let q: String = conn.query_row("SELECT question_id FROM quiz_history", [], |r| r.get(0)).unwrap();
    assert_eq!(q, "a1-x-realm-01");
}

#[test]
fn v13_carries_guide_completions_and_reads_over() {
    let conn = Connection::open_in_memory().unwrap();
    for sql in &MIGRATIONS[..11] {
        conn.execute_batch(sql).unwrap();
    }
    conn.execute_batch("PRAGMA user_version = 11;").unwrap();
    conn.execute("INSERT INTO users (username, password_hash, created_at) VALUES ('ann', 'h', 1)", []).unwrap();
    for (lesson, completed, stars) in [("good", 2, 3), ("weak", 1, 1), ("started", 0, 0)] {
        conn.execute(
            "INSERT INTO journey_progress (user_id, course_id, lesson_id, step, completed, best_stars, xp, run_correct, run_answered, updated_at)
                 VALUES (1, 'spanish', ?1, 0, ?2, ?3, 0, 0, 0, 5)",
            rusqlite::params![lesson, completed, stars],
        )
        .unwrap();
    }
    // Practised before the reading check: v12 marks it read, v13 seen.
    conn.execute(
        "INSERT INTO user_progress (user_id, course_id, lesson_id, mastery_score, times_tested, last_practiced_at) VALUES (1, 'spanish', 'read', 40, 2, 7)",
        [],
    )
    .unwrap();
    migrate(&conn).unwrap();
    let seen: Option<i64> =
        conn.query_row("SELECT seen_at FROM lesson_passes WHERE lesson_id = 'read'", [], |r| r.get(0)).unwrap();
    assert_eq!(seen, Some(7));
    let rows: Vec<(String, Option<i64>, i64)> = conn
        .prepare("SELECT lesson_id, passed_at, best_pct FROM lesson_passes WHERE lesson_id != 'read' ORDER BY lesson_id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(rows, vec![("good".to_string(), Some(5), 90), ("weak".to_string(), None, 0)]);
}

#[test]
fn v21_keeps_the_backup_of_the_last_revision_as_an_undo_step() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    for sql in &MIGRATIONS[..20] {
        conn.execute_batch(sql).unwrap();
    }
    conn.execute_batch("PRAGMA user_version = 20;").unwrap();
    conn.execute("INSERT INTO users (username, password_hash, created_at) VALUES ('ann', 'h', 1)", []).unwrap();
    conn.execute("INSERT INTO courses (id, owner_id, title, created_at, updated_at) VALUES ('c', 1, 'C', 1, 1)", []).unwrap();
    conn.execute("INSERT INTO course_backups (course_id, title, outline, feedback, created_at) VALUES ('c', 'Old', NULL, 'more examples', 9)", []).unwrap();
    conn.execute("INSERT INTO course_backup_files (course_id, path, content) VALUES ('c', 'course.json', 'old')", []).unwrap();
    migrate(&conn).unwrap();
    let mut conn = conn;
    let step = crate::courses::undo_step(&conn, "c").unwrap().unwrap();
    assert_eq!((step.label.as_str(), step.created_at, step.steps), ("Changed with feedback: \u{201c}more examples\u{201d}", 9, 1));
    assert_eq!(crate::courses::undo_revision(&mut conn, "c").unwrap().as_deref(), Some(step.label.as_str()));
    assert_eq!(crate::courses::read_file(&conn, "c", "course.json").unwrap().as_deref(), Some("old"));
}

#[test]
fn v35_gives_each_account_the_colour_its_picture_wore() {
    let conn = Connection::open_in_memory().unwrap();
    for sql in &MIGRATIONS[..34] {
        conn.execute_batch(sql).unwrap();
    }
    conn.execute_batch("PRAGMA user_version = 34;").unwrap();
    // The eight pictures of the time, in palette order: each one's own colour was its slot.
    let avatars = ["noor", "amani", "yuki", "kai", "arjun", "sofia", "mochi", "coco"];
    for (i, a) in avatars.iter().enumerate() {
        conn.execute("INSERT INTO users (password_hash, created_at, avatar) VALUES ('h', ?1, ?2)", rusqlite::params![i as i64, a]).unwrap();
    }
    conn.execute("INSERT INTO users (password_hash, created_at, avatar, avatar_color) VALUES ('h', 100, 'kai', 6)", []).unwrap();
    migrate(&conn).unwrap();
    // Each picture's own colour is its palette slot, so nobody's changed.
    for (i, a) in avatars.iter().enumerate() {
        let slot: u8 = conn.query_row("SELECT avatar_color FROM users WHERE avatar = ?1 AND created_at < 100", [a], |r| r.get(0)).unwrap();
        assert_eq!(slot as usize, i + 1, "{a}");
    }
    // A colour already picked stays.
    assert_eq!(conn.query_row("SELECT avatar_color FROM users WHERE created_at = 100", [], |r| r.get::<_, u8>(0)).unwrap(), 6);
}

#[test]
fn v26_drops_usernames_and_keeps_everything_else() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    for sql in &MIGRATIONS[..25] {
        conn.execute_batch(sql).unwrap();
    }
    conn.execute_batch("PRAGMA user_version = 25;").unwrap();
    conn.execute(
        "INSERT INTO users (id, username, password_hash, created_at, email, first_name, theme, developer, google_sub) VALUES (7, 'ann', 'h', 1, 'ann@example.org', 'Ann', 'realm', 1, 'g-7')",
        [],
    )
    .unwrap();
    conn.execute("INSERT INTO sessions (user_id, session_token, created_at, expires_at) VALUES (7, 't', 1, 2)", []).unwrap();
    conn.execute("INSERT INTO terms_acceptances (user_id, version, accepted_at) VALUES (7, 'v1', 1)", []).unwrap();
    migrate(&conn).unwrap();

    let cols: Vec<String> = conn.prepare("SELECT name FROM pragma_table_info('users')").unwrap().query_map([], |r| r.get(0)).unwrap().collect::<Result<_, _>>().unwrap();
    assert!(!cols.iter().any(|c| c == "username"), "{cols:?}");
    let row: (String, String, String, bool, String) = conn
        .query_row("SELECT email, first_name, theme, developer, google_sub FROM users WHERE id = 7", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
        .unwrap();
    assert_eq!(row, ("ann@example.org".into(), "Ann".into(), "realm".into(), true, "g-7".into()));
    // The acceptance survived the rebuild (the plain-text session did
    // not: v36 ends them all, and marks Terms accepters age-confirmed),
    // and the foreign keys still bite.
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM terms_acceptances", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    assert!(conn.query_row("SELECT age_confirmed FROM users WHERE id = 7", [], |r| r.get::<_, bool>(0)).unwrap());
    assert!(conn.query_row("PRAGMA foreign_keys", [], |r| r.get::<_, bool>(0)).unwrap());
    conn.execute("DELETE FROM users WHERE id = 7", []).unwrap();
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM terms_acceptances", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    assert!(conn.execute("INSERT INTO sessions (user_id, session_token, created_at, expires_at) VALUES (99, 'u', 1, 2)", []).is_err());
    assert!(conn.execute("INSERT INTO users (password_hash, created_at, google_sub) VALUES ('', 1, 'g-8'), ('', 1, 'g-8')", []).is_err());
}

/// The migration that rebuilds `users` for #364 (find it by its SQL, so
/// renumbering it when another migration lands first changes nothing).
fn v_users_autoincrement() -> i64 {
    MIGRATIONS.iter().position(|m| m.contains("id                INTEGER PRIMARY KEY AUTOINCREMENT")).unwrap() as i64 + 1
}

/// A migration that drops a table others point at must run with
/// foreign keys off, or the DROP cascades into every user table.
#[test]
fn every_users_rebuild_runs_with_foreign_keys_off() {
    for (i, sql) in MIGRATIONS.iter().enumerate() {
        if sql.contains("DROP TABLE users") {
            assert!(REBUILDS.contains(&(i as i64 + 1)), "v{} rebuilds users but is not in REBUILDS", i + 1);
        }
    }
}

#[test]
fn users_rebuild_keeps_everything_and_never_reuses_ids() {
    let v = v_users_autoincrement() as usize;
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    for sql in &MIGRATIONS[..v - 1] {
        conn.execute_batch(sql).unwrap();
    }
    conn.execute_batch(&format!("PRAGMA user_version = {};", v - 1)).unwrap();
    conn.execute(
        "INSERT INTO users (id, password_hash, created_at, email, first_name, theme, google_sub, ai_off, email_verified_at, superuser,
                                avatar_color, review_quiet, phone, phone_verified_at, two_factor)
             VALUES (7, 'h', 1, 'ann@example.org', 'Ann', 'realm', 'g-7', 1, 5, 1, 3, 1, '+15550001', 6, 'sms'),
                    (8, 'h', 2, 'bo@example.org', 'Bo', '', NULL, 0, NULL, 0, 0, 0, '', NULL, '')",
        [],
    )
    .unwrap();
    conn.execute("INSERT INTO terms_acceptances (user_id, version, accepted_at) VALUES (7, 'v1', 1)", []).unwrap();
    conn.execute(
        "INSERT INTO subscriptions (user_id, customer_id, subscription_id, status, price_id, cancel_at_period_end, event_at, updated_at)
             VALUES (7, 'cus_7', 'sub_7', 'active', 'p', 0, 1, 1)",
        [],
    )
    .unwrap();
    migrate(&conn).unwrap();

    // Every column survived; the customer moved onto the account; each
    // account got its own billing_ref.
    let row: ((String, String, String, String, bool, i64), (bool, u8, bool, String, i64, String, Option<String>)) = conn
        .query_row(
            "SELECT email, first_name, theme, google_sub, ai_off, email_verified_at, superuser, avatar_color, review_quiet, phone,
                        phone_verified_at, two_factor, stripe_customer FROM users WHERE id = 7",
            [],
            |r| Ok(((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?), (r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?, r.get(11)?, r.get(12)?))),
        )
        .unwrap();
    assert_eq!(
        row,
        (
            ("ann@example.org".into(), "Ann".into(), "realm".into(), "g-7".into(), true, 5),
            (true, 3, true, "+15550001".into(), 6, "sms".into(), Some("cus_7".into()))
        )
    );
    let refs: Vec<String> = conn.prepare("SELECT billing_ref FROM users ORDER BY id").unwrap().query_map([], |r| r.get(0)).unwrap().collect::<Result<_, _>>().unwrap();
    assert!(refs.len() == 2 && refs[0] != refs[1] && refs.iter().all(|r| r.len() == 32), "{refs:?}");
    let indexes: Vec<String> = conn.prepare("SELECT name FROM pragma_index_list('users') ORDER BY name").unwrap().query_map([], |r| r.get(0)).unwrap().collect::<Result<_, _>>().unwrap();
    for name in ["idx_users_billing_ref", "idx_users_email", "idx_users_google_sub", "idx_users_stripe_customer"] {
        assert!(indexes.iter().any(|i| i == name), "{name} missing: {indexes:?}");
    }
    // Nothing cascaded away, and the foreign keys still bite.
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM terms_acceptances", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM subscriptions", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    assert!(conn.query_row("PRAGMA foreign_keys", [], |r| r.get::<_, bool>(0)).unwrap());
    assert!(conn.execute("INSERT INTO users (password_hash, created_at, google_sub) VALUES ('', 1, 'g-7')", []).is_err());

    // Deleting the newest account does not free its id for the next one.
    conn.execute("DELETE FROM users WHERE id = 8", []).unwrap();
    conn.execute("INSERT INTO users (password_hash, created_at) VALUES ('h', 3)", []).unwrap();
    assert_eq!(conn.last_insert_rowid(), 9);
    conn.execute("DELETE FROM users WHERE id = 7", []).unwrap();
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM subscriptions", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
}

#[test]
fn v29_leaves_existing_accounts_unverified() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    for sql in &MIGRATIONS[..28] {
        conn.execute_batch(sql).unwrap();
    }
    conn.execute_batch("PRAGMA user_version = 28;").unwrap();
    conn.execute("INSERT INTO users (id, password_hash, created_at, email) VALUES (3, 'h', 1, 'ann@example.org')", []).unwrap();
    migrate(&conn).unwrap();
    let verified: Option<i64> = conn.query_row("SELECT email_verified_at FROM users WHERE id = 3", [], |r| r.get(0)).unwrap();
    assert_eq!(verified, None);
    conn.execute("INSERT INTO email_verifications (user_id, email, token_hash, created_at, expires_at) VALUES (3, 'ann@example.org', 'h1', 1, 2)", []).unwrap();
    assert!(conn.execute("INSERT INTO email_verifications (user_id, email, token_hash, created_at, expires_at) VALUES (3, 'x', 'h1', 1, 2)", []).is_err(), "hashes are unique");
    conn.execute("DELETE FROM users WHERE id = 3", []).unwrap();
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM email_verifications", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
}

#[test]
fn v31_renames_the_profile_pictures() {
    let conn = Connection::open_in_memory().unwrap();
    for sql in &MIGRATIONS[..30] {
        conn.execute_batch(sql).unwrap();
    }
    conn.execute_batch("PRAGMA user_version = 30;").unwrap();
    for (id, avatar) in [(1, "owl"), (2, "leaf"), (3, "bolt"), (4, "odd")] {
        conn.execute("INSERT INTO users (id, password_hash, created_at, avatar) VALUES (?1, 'h', 1, ?2)", rusqlite::params![id, avatar]).unwrap();
    }
    migrate(&conn).unwrap();
    let avatar = |id: i64| -> String { conn.query_row("SELECT avatar FROM users WHERE id = ?1", [id], |r| r.get(0)).unwrap() };
    assert_eq!((avatar(1), avatar(2), avatar(3), avatar(4)), ("noor".into(), "sofia".into(), "mochi".into(), "odd".into()));
    // The rewrite maps every old id to the picture that took it.
    let aliases = [("owl", "noor"), ("fox", "kai"), ("cat", "mochi"), ("bear", "coco"), ("rabbit", "sofia"), ("robot", "arjun"), ("rocket", "yuki"), ("star", "amani"), ("leaf", "sofia"), ("bolt", "mochi")];
    for (old, new) in aliases {
        conn.execute("UPDATE users SET avatar = ?1 WHERE id = 4", [old]).unwrap();
        conn.execute_batch(MIGRATIONS[30]).unwrap();
        assert_eq!(avatar(4), new, "{old}");
    }
}

#[test]
fn v22_leaves_existing_users_plain_learners() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    for sql in &MIGRATIONS[..21] {
        conn.execute_batch(sql).unwrap();
    }
    conn.execute_batch("PRAGMA user_version = 21;").unwrap();
    conn.execute("INSERT INTO users (id, username, password_hash, created_at) VALUES (1, 'ann', 'h', 1)", []).unwrap();
    migrate(&conn).unwrap();
    let developer: bool = conn.query_row("SELECT developer FROM users WHERE id = 1", [], |r| r.get(0)).unwrap();
    assert!(!developer);
}

#[test]
fn v50_gives_each_course_the_accounts_picture() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    for sql in &MIGRATIONS[..49] {
        conn.execute_batch(sql).unwrap();
    }
    conn.execute_batch("PRAGMA user_version = 49;").unwrap();
    conn.execute("INSERT INTO users (id, password_hash, created_at, email, avatar, avatar_color) VALUES (1, 'h', 1, 'a@example.org', 'kai', 4)", []).unwrap();
    for course in ["spanish", "french"] {
        conn.execute("INSERT INTO projects (user_id, course_id, created_at, last_opened_at) VALUES (1, ?1, 1, 1)", [course]).unwrap();
    }
    migrate(&conn).unwrap();
    let pictures: Vec<(String, i64)> =
        conn.prepare("SELECT avatar, avatar_color FROM projects ORDER BY course_id").unwrap().query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(Result::unwrap).collect();
    assert_eq!(pictures, [("kai".to_string(), 4), ("kai".to_string(), 4)]);
}

#[test]
fn v51_gives_each_course_the_accounts_motivator_and_counts_it_started() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    for sql in &MIGRATIONS[..50] {
        conn.execute_batch(sql).unwrap();
    }
    conn.execute_batch("PRAGMA user_version = 50;").unwrap();
    conn.execute("INSERT INTO users (id, password_hash, created_at, email, motivator) VALUES (1, 'h', 1, 'a@example.org', 'skc')", []).unwrap();
    conn.execute("INSERT INTO projects (user_id, course_id, created_at, last_opened_at) VALUES (1, 'spanish', 1, 1)", []).unwrap();
    migrate(&conn).unwrap();
    let row: (String, bool) = conn.query_row("SELECT motivator, set_up FROM projects", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!(row, ("skc".to_string(), true));
}

#[test]
fn v52_puts_courses_on_the_profiles_theme_on_default() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    for sql in &MIGRATIONS[..51] {
        conn.execute_batch(sql).unwrap();
    }
    conn.execute_batch("PRAGMA user_version = 51;").unwrap();
    conn.execute("INSERT INTO users (id, password_hash, created_at, email, theme) VALUES (1, 'h', 1, 'a@example.org', 'realm')", []).unwrap();
    conn.execute("INSERT INTO projects (user_id, course_id, created_at, last_opened_at, theme) VALUES (1, 'spanish', 1, 1, NULL), (1, 'french', 1, 1, 'rustwake')", []).unwrap();
    migrate(&conn).unwrap();
    let themes: Vec<String> = conn.prepare("SELECT theme FROM projects ORDER BY course_id").unwrap().query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
    assert_eq!(themes, ["rustwake", ""]);
}

#[test]
fn v40_drops_mock_ai_and_keeps_the_developers() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    for sql in &MIGRATIONS[..39] {
        conn.execute_batch(sql).unwrap();
    }
    conn.execute_batch("PRAGMA user_version = 39;").unwrap();
    conn.execute("INSERT INTO users (id, password_hash, created_at, email, developer, mock_ai) VALUES (1, 'h', 1, 'd@example.org', 1, 1)", []).unwrap();
    migrate(&conn).unwrap();
    let developer: bool = conn.query_row("SELECT developer FROM users WHERE id = 1", [], |r| r.get(0)).unwrap();
    assert!(developer);
    assert!(conn.prepare("SELECT mock_ai FROM users").is_err(), "the column is gone");
}

#[test]
fn v8_scopes_old_progress_to_spanish() {
    let conn = Connection::open_in_memory().unwrap();
    // Replay migrations 1..7, insert legacy rows, then apply v8.
    for sql in &MIGRATIONS[..7] {
        conn.execute_batch(sql).unwrap();
    }
    conn.execute_batch("PRAGMA user_version = 7;").unwrap();
    conn.execute("INSERT INTO users (username, password_hash, created_at) VALUES ('ann', 'h', 1)", []).unwrap();
    conn.execute("INSERT INTO user_progress (user_id, lesson_id, mastery_score) VALUES (1, 'a1-x', 50)", []).unwrap();
    conn.execute("INSERT INTO quiz_history (user_id, lesson_id, question_id, selected_answer, was_correct, answered_at) VALUES (1, 'a1-x', 'q', 'a', 1, 1)", []).unwrap();
    migrate(&conn).unwrap();
    let (course, score): (String, f64) =
        conn.query_row("SELECT course_id, mastery_score FROM user_progress WHERE user_id = 1", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!((course.as_str(), score), ("spanish", 50.0));
    let hist: String = conn.query_row("SELECT course_id FROM quiz_history", [], |r| r.get(0)).unwrap();
    assert_eq!(hist, "spanish");
    let active: String = conn.query_row("SELECT active_course FROM users WHERE id = 1", [], |r| r.get(0)).unwrap();
    assert_eq!(active, "spanish");
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 1);
}

#[test]
fn v12_grandfathers_practised_lessons_as_read() {
    let conn = Connection::open_in_memory().unwrap();
    for sql in &MIGRATIONS[..11] {
        conn.execute_batch(sql).unwrap();
    }
    conn.execute_batch("PRAGMA user_version = 11;").unwrap();
    conn.execute("INSERT INTO users (username, password_hash, created_at) VALUES ('ann', 'h', 1)", []).unwrap();
    conn.execute(
        "INSERT INTO user_progress (user_id, course_id, lesson_id, mastery_score, times_tested, last_practiced_at)
             VALUES (1, 'spanish', 'practised', 40, 3, 1234), (1, 'spanish', 'untouched', 0, 0, NULL)",
        [],
    )
    .unwrap();
    migrate(&conn).unwrap();
    let read: Option<i64> =
        conn.query_row("SELECT read_at FROM user_progress WHERE lesson_id = 'practised'", [], |r| r.get(0)).unwrap();
    assert_eq!(read, Some(1234));
    let unread: Option<i64> =
        conn.query_row("SELECT read_at FROM user_progress WHERE lesson_id = 'untouched'", [], |r| r.get(0)).unwrap();
    assert_eq!(unread, None);
}

#[test]
fn a_database_from_a_newer_release_is_refused_and_left_alone() {
    let dir = std::env::temp_dir().join(format!("skrongle-newer-db-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("learn.db");
    let newer = schema_version() + 1;
    Connection::open(&path).unwrap().execute_batch(&format!("PRAGMA user_version = {newer};")).unwrap();
    let msg = check_not_newer(&path).unwrap_err();
    assert!(msg.contains("upgraded by a newer Skrongle") && msg.contains(&format!("v{newer}")), "{msg}");
    assert!(Db::open(&path).is_err());
    let v: i64 = Connection::open(&path).unwrap().query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(v, newer);
    assert!(check_not_newer(&dir.join("missing.db")).is_ok());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// #1360: words collected before the schedule get a rung from how often they
/// were known, due that long after their last review; overdue ones land on
/// today or one of the six days after it.
#[test]
fn vocab_schedule_backfill() {
    let v = MIGRATIONS.iter().position(|m| m.contains("idx_user_vocab_due")).unwrap() + 1;
    let conn = Connection::open_in_memory().unwrap();
    for sql in &MIGRATIONS[..v - 1] {
        conn.execute_batch(sql).unwrap();
    }
    conn.execute_batch(&format!("PRAGMA user_version = {};", v - 1)).unwrap();
    conn.execute("INSERT INTO users (id, password_hash, created_at, email) VALUES (1, 'h', 1, 'ann@example.org')", []).unwrap();
    let day = 86_400;
    let today = now() / day * day;
    // (word, learned_at, times_known, last_reviewed_at)
    let rows: Vec<(String, i64, i64, Option<i64>)> = vec![
        ("fresh".into(), today - 3600, 0, None),
        ("once".into(), today - 5 * day, 1, Some(today - day)),
        ("twice".into(), today - 9 * day, 2, Some(today - 2 * day)),
        ("often".into(), today - 30 * day, 9, Some(today - 3 * day)),
    ];
    let old: Vec<String> = (0..40).map(|i| format!("old{i}")).collect();
    for (w, learned, known, last) in rows.iter().cloned().chain(old.iter().enumerate().map(|(i, w)| (w.clone(), today - 400 * day + i as i64 * 13, 0, None))) {
        conn.execute(
            "INSERT INTO user_vocab (user_id, course_id, lesson_id, word, learned_at, times_known, last_reviewed_at) VALUES (1, 'es', 'l1', ?1, ?2, ?3, ?4)",
            rusqlite::params![w, learned, known, last],
        )
        .unwrap();
    }
    migrate(&conn).unwrap();
    let got = |w: &str| -> (i64, i64) {
        conn.query_row("SELECT gap_days, due_at FROM user_vocab WHERE word = ?1", [w], |r| Ok((r.get(0)?, r.get(1)?))).unwrap()
    };
    // Collected an hour before today began: due today.
    assert_eq!(got("fresh"), (1, today));
    assert_eq!(got("once"), (3, today + 2 * day));
    assert_eq!(got("twice"), (7, today + 5 * day));
    assert_eq!(got("often"), (16, today + 13 * day));
    // Overdue: spread over today and the next six days, no day left empty.
    let mut days = std::collections::BTreeSet::new();
    for w in &old {
        let (gap, due) = got(w);
        assert_eq!(gap, 1);
        assert!((today..today + 7 * day).contains(&due) && due % day == 0, "{w}: {due}");
        days.insert(due);
    }
    assert_eq!(days.len(), 7, "{days:?}");
}
