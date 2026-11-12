use super::*;
use std::collections::BTreeMap;

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
fn migrations_apply_once() {
    let db = Db::open_in_memory().unwrap();
    let v: i64 = db.with(|c| c.query_row("PRAGMA user_version", [], |r| r.get(0))).unwrap();
    assert_eq!(v, schema_version());
    assert_eq!(v, 5, "v1 is the schema (#49), v2 drops Skrongle Cash, v3 names share links, v4 adds permissions (#138), v5 adds courses.error_kind");
    // Re-running is a no-op.
    db.with(|c| migrate(c)).unwrap();
}

/// What a schema is, apart from how its SQL was typed: every table's
/// columns, indexes and foreign keys.
fn schema_shape(conn: &Connection) -> Vec<String> {
    let rows = |sql: &str| -> Vec<String> {
        let mut stmt = conn.prepare(sql).unwrap();
        let n = stmt.column_count();
        let mut rows = stmt.raw_query();
        let mut out = Vec::new();
        while let Some(r) = rows.next().unwrap() {
            out.push((0..n).map(|i| format!("{:?}", r.get_ref(i).unwrap())).collect::<Vec<_>>().join("|"));
        }
        out
    };
    let mut shape = Vec::new();
    let names: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    for t in names {
        for line in rows(&format!("SELECT name, type, \"notnull\", dflt_value, pk FROM pragma_table_info('{t}') ORDER BY cid")) {
            shape.push(format!("{t} column {line}"));
        }
        let mut indexes = rows(&format!(
            "SELECT l.\"unique\", l.origin, l.partial, (SELECT group_concat(name) FROM pragma_index_info(l.name)) FROM pragma_index_list('{t}') l"
        ));
        indexes.sort();
        shape.extend(indexes.into_iter().map(|line| format!("{t} index {line}")));
        let mut keys = rows(&format!("SELECT \"table\", \"from\", \"to\", on_update, on_delete FROM pragma_foreign_key_list('{t}')"));
        keys.sort();
        shape.extend(keys.into_iter().map(|line| format!("{t} key {line}")));
    }
    shape.extend(rows("SELECT type, name, tbl_name FROM sqlite_master WHERE type IN ('index', 'trigger', 'view') AND name NOT LIKE 'sqlite_%' ORDER BY name"));
    shape
}

/// Every migration after the first can be taken back, and its `down`
/// leaves exactly the schema the version before made.
#[test]
fn every_down_restores_the_schema_before_it() {
    for version in 2..=schema_version() as usize {
        let mut before = Connection::open_in_memory().unwrap();
        before.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        MIGRATIONS.to_version(&mut before, version - 1).unwrap();
        let mut back = Connection::open_in_memory().unwrap();
        back.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        MIGRATIONS.to_version(&mut back, version).unwrap();
        migrate_to(&mut back, version as i64 - 1).unwrap_or_else(|e| panic!("down from v{version}: {e}"));
        let v: i64 = back.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, version as i64 - 1);
        assert_eq!(schema_shape(&back), schema_shape(&before), "the down of v{version} does not restore v{}", version - 1);
    }
}

/// Down to v1 and up again with rows in the tables the migrations touch:
/// accounts, courses and every share link (with its token) are still there.
/// Links v2 could not hold as they were got their id in the name.
#[test]
fn down_and_up_again_keeps_the_rows() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let ann = crate::auth::create_user(c, "ann@example.org", "h")?;
        c.execute("INSERT INTO courses (id, owner_id, title, created_at, updated_at) VALUES ('es', ?1, 'Spanish', 1, 1), ('fr', ?1, 'French', 1, 1)", [ann])?;
        c.execute_batch(
            "INSERT INTO course_shares (id, course_id, label, token_hash, created_at) VALUES
                 (1, 'es', '', 't1', 1), (2, 'es', '', 't2', 1), (3, 'es', 'Bea', 't3', 1), (4, 'es', 'Bea', 't4', 1), (5, 'fr', 'Bea', 't5', 1);",
        )?;
        migrate_to(c, 1)?;
        assert_eq!(c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?, 1);
        let emails: Vec<String> = c.prepare("SELECT email FROM course_shares ORDER BY id")?.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
        assert_eq!(emails, ["link 1", "link 2", "Bea", "Bea (4)", "Bea"]);
        migrate(c)?;
        assert_eq!(c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?, schema_version());
        let links: Vec<(String, String)> = c.prepare("SELECT label, token_hash FROM course_shares ORDER BY id")?.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
        assert_eq!(links, [("link 1", "t1"), ("link 2", "t2"), ("Bea", "t3"), ("Bea (4)", "t4"), ("Bea", "t5")].map(|(a, b)| (a.to_string(), b.to_string())));
        let (id, perms): (i64, i64) = c.query_row("SELECT id, perms FROM users WHERE email = 'ann@example.org'", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
        assert_eq!((id, perms), (ann, 63), "the account is kept; going up gives it the Teacher set again");
        assert_eq!(c.query_row("SELECT count(*) FROM courses", [], |r| r.get::<_, i64>(0))?, 2);
        Ok(())
    })
    .unwrap();
}

/// v1 has no `down` (there is no schema before it), and a version this
/// binary does not know is refused; neither changes the database.
#[test]
fn a_schema_that_does_not_exist_is_refused() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        for target in [0, -1, schema_version() + 1] {
            let e = migrate_to(c, target).unwrap_err().to_string();
            assert!(e.contains(&format!("there is no schema v{target}")), "{e}");
        }
        assert_eq!(c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?, schema_version());
        Ok(())
    })
    .unwrap();
}

/// Account ids are never reused (`AUTOINCREMENT`): they end up in caches,
/// sealed secrets' associated data and shared links.
#[test]
fn user_ids_are_never_reused() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let ann = crate::auth::create_user(c, "ann@example.org", "h")?;
        c.execute("DELETE FROM users WHERE id = ?1", [ann])?;
        let bob = crate::auth::create_user(c, "bob@example.org", "h")?;
        assert!(bob > ann, "{bob} reuses {ann}");
        Ok(())
    })
    .unwrap();
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

/// #49: a database from before the 1.0 schema (its `users` table still
/// has a column of that history) is refused with one line naming the file,
/// whatever version it recorded, and nothing is written to it.
#[test]
fn a_database_from_before_the_schema_squash_is_refused_and_left_alone() {
    let dir = std::env::temp_dir().join(format!("skrongle-old-db-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (name, column, version) in [("v99.db", "google_sub", 99), ("v1.db", "username", 1)] {
        let path = dir.join(name);
        Connection::open(&path)
            .unwrap()
            .execute_batch(&format!("CREATE TABLE users (id INTEGER PRIMARY KEY, {column} TEXT, password_hash TEXT NOT NULL, created_at INTEGER NOT NULL); PRAGMA user_version = {version};"))
            .unwrap();
        let msg = check_not_newer(&path).unwrap_err();
        assert_eq!(msg, format!("{}: this database predates the 1.0 schema; start from a fresh one", path.display()));
        assert!(!msg.contains('\n'));
        let err = Db::open(&path).err().unwrap().to_string();
        assert!(err.contains("predates the 1.0 schema"), "{err}");
        let conn = Connection::open(&path).unwrap();
        let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        let tables: i64 = conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table'", [], |r| r.get(0)).unwrap();
        assert_eq!((v, tables), (version, 1), "{name} was written to");
    }
    // A database of this schema opens.
    let path = dir.join("fresh.db");
    drop(Db::open(&path).unwrap());
    assert!(check_not_newer(&path).is_ok());
    assert!(Db::open(&path).is_ok());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// v2: a database still on v1 loses the shop's table, and a course that wore
/// Skrongle Cash wears XP.
#[test]
fn v2_drops_the_shop_and_turns_skc_into_xp() {
    let dir = std::env::temp_dir().join(format!("skrongle-v1-db-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("learn.db");
    {
        let mut conn = Connection::open(&path).unwrap();
        MIGRATIONS.to_version(&mut conn, 1).unwrap();
        conn.execute_batch(
            "INSERT INTO users (id, email, password_hash, created_at) VALUES (1, 'ann@example.org', 'h', 1);
             INSERT INTO projects (user_id, course_id, created_at, last_opened_at, motivator) VALUES (1, 'es', 1, 1, 'skc'), (1, 'fr', 1, 1, 'xp'), (1, 'de', 1, 1, 'stats');
             INSERT INTO motivator_purchases (user_id, course_id, item, price, created_at) VALUES (1, 'es', 'sprout', 50, 1);",
        )
        .unwrap();
    }
    let db = Db::open(&path).unwrap();
    db.with(|c| {
        let v: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        assert_eq!(v, schema_version());
        let tables: i64 = c.query_row("SELECT count(*) FROM sqlite_master WHERE name = 'motivator_purchases'", [], |r| r.get(0))?;
        assert_eq!(tables, 0);
        let mut got: Vec<(String, String)> = c.prepare("SELECT course_id, motivator FROM projects ORDER BY course_id")?.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
        got.sort();
        assert_eq!(got, [("de", "stats"), ("es", "xp"), ("fr", "xp")].map(|(a, b)| (a.to_string(), b.to_string())));
        Ok(())
    })
    .unwrap();
    drop(db);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// v4 (#138): the accounts of a database still on v3 get the Teacher set,
/// which is what they could do before, and the two built-in presets exist.
#[test]
fn v4_gives_every_account_the_teacher_permissions() {
    let dir = std::env::temp_dir().join(format!("skrongle-v3-db-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("learn.db");
    {
        let mut conn = Connection::open(&path).unwrap();
        MIGRATIONS.to_version(&mut conn, 3).unwrap();
        conn.execute_batch(
            "INSERT INTO users (id, email, password_hash, created_at) VALUES (1, 'ann@example.org', 'h', 1);
             INSERT INTO users (id, email, password_hash, created_at, owner) VALUES (2, 'bea@example.org', 'h', 1, 1);",
        )
        .unwrap();
    }
    let db = Db::open(&path).unwrap();
    db.with(|c| {
        let v: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        assert_eq!(v, schema_version());
        let users: Vec<(i64, i64, Option<i64>)> =
            c.prepare("SELECT id, perms, preset_id FROM users ORDER BY id")?.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
        assert_eq!(users, [(1, 63, Some(2)), (2, 63, Some(2))]);
        let presets: Vec<(i64, String, bool, i64)> = c
            .prepare("SELECT id, name, builtin, perms FROM permission_presets ORDER BY id")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<Result<_, _>>()?;
        assert_eq!(presets, [(1, "student".to_string(), true, 0), (2, "teacher".to_string(), true, 63)]);
        Ok(())
    })
    .unwrap();
    drop(db);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Pins #94: the bundled SQLite is built without the full-text, R-tree,
/// dbstat and soundex modules (`LIBSQLITE3_FLAGS` in `.cargo/config.toml`),
/// since nothing in `src/` uses them. If you need full-text search, drop the
/// `-USQLITE_ENABLE_FTS5` (and FTS3) flags there; this test then goes too.
#[test]
fn bundled_sqlite_has_no_fts_rtree_dbstat_or_soundex() {
    let conn = Connection::open_in_memory().unwrap();
    assert!(conn.execute_batch("CREATE VIRTUAL TABLE t USING fts5(x)").is_err(), "fts5 is compiled in");
    assert!(conn.execute_batch("CREATE VIRTUAL TABLE t3 USING fts3(x)").is_err(), "fts3 is compiled in");
    assert!(conn.execute_batch("CREATE VIRTUAL TABLE r USING rtree(id, a, b)").is_err(), "rtree is compiled in");
    assert!(conn.prepare("SELECT * FROM dbstat").is_err(), "dbstat is compiled in");
    assert!(conn.prepare("SELECT soundex('a')").is_err(), "soundex is compiled in");
}
