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
fn migrations_apply_once() {
    let db = Db::open_in_memory().unwrap();
    let v: i64 = db.with(|c| c.query_row("PRAGMA user_version", [], |r| r.get(0))).unwrap();
    assert_eq!(v, MIGRATIONS.len() as i64);
    assert_eq!(v, 1, "the schema starts at v1 (#49)");
    // Re-running is a no-op.
    db.with(|c| migrate(c)).unwrap();
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
