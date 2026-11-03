//! SQLite access: a single connection behind a mutex, used from the Tokio
//! blocking pool. On a Pi Zero 2 W one connection in WAL mode is plenty and
//! keeps the memory footprint flat (one page cache, one statement cache).
//!
//! Every progress table carries a `course_id`: a learner's mastery, history,
//! vocabulary and game records are kept per course.

use rusqlite::trace::{TraceEvent, TraceEventCodes};
use rusqlite::{Connection, StatementStatus};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use crate::error::AppError;
use crate::mastery::cache::ProgressCache;

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
    /// The signed-in users by session (#37), forgotten by this connection's
    /// temporary triggers whenever a table they are read from changes.
    sessions: Arc<crate::auth::SessionCache>,
    /// Learners' progress for their course, in memory (#38).
    progress: Arc<ProgressCache>,
}

/// Pragmas applied to every connection. See the README for the reasoning.
const PRAGMAS: &str = "
    PRAGMA journal_mode = WAL;
    PRAGMA synchronous = NORMAL;
    PRAGMA cache_size = -2000;      -- ~2 MB page cache
    PRAGMA temp_store = MEMORY;
    PRAGMA foreign_keys = ON;
    PRAGMA busy_timeout = 5000;
    PRAGMA mmap_size = 0;           -- keep RSS predictable; the DB is tiny anyway
    PRAGMA wal_autocheckpoint = 500;
";

/// The schema level this binary migrates a database to.
pub fn schema_version() -> i64 {
    MIGRATIONS.len() as i64
}

/// The exit status of a server that refuses a database a newer release
/// upgraded (#1329). `learnmap.service` does not restart on it.
pub const NEWER_DB_EXIT: i32 = 78;

fn newer_db_message(found: i64) -> String {
    format!(
        "this database was upgraded by a newer Skrongle (schema v{found}; this binary knows up to v{}); deploy that version or restore a backup",
        schema_version()
    )
}

/// Refuses a database at `path` whose schema is newer than this binary's,
/// reading it without writing anything. A missing or unreadable file passes:
/// `Db::open` reports those.
pub fn check_not_newer(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let Ok(conn) = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY) else { return Ok(()) };
    match conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0)) {
        Ok(found) if found > schema_version() => Err(newer_db_message(found)),
        _ => Ok(()),
    }
}

mod migrations;
use migrations::{MIGRATIONS, REBUILDS};

impl Db {
    pub fn open(path: &Path) -> rusqlite::Result<Db> {
        let conn = Connection::open(path)?;
        conn.execute_batch(PRAGMAS)?;
        migrate(&conn)?;
        start_trace(&conn);
        Db::new(conn)
    }

    /// Hold a migrated connection, with an empty session cache (and the
    /// triggers that keep it true) and an empty progress cache.
    fn new(conn: Connection) -> rusqlite::Result<Db> {
        let sessions = Arc::new(crate::auth::SessionCache::default());
        let watched = Arc::clone(&sessions);
        conn.create_scalar_function("forget_sessions", 0, rusqlite::functions::FunctionFlags::SQLITE_UTF8, move |_| {
            watched.forget_all();
            Ok(0)
        })?;
        conn.execute_batch(&crate::auth::SessionCache::triggers_sql())?;
        Ok(Db { conn: Arc::new(Mutex::new(conn)), sessions, progress: Arc::default() })
    }

    /// The signed-in users kept in memory (`auth::cached_user`).
    pub fn sessions(&self) -> Arc<crate::auth::SessionCache> {
        Arc::clone(&self.sessions)
    }

    /// Open an in-memory database (tests).
    #[cfg(test)]
    pub fn open_in_memory() -> rusqlite::Result<Db> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        migrate(&conn)?;
        Db::new(conn)
    }

    /// The progress cache (#38), for the tests.
    #[cfg(test)]
    pub fn progress_cache(&self) -> &ProgressCache {
        &self.progress
    }

    /// Run a closure against the connection on the blocking thread pool.
    pub async fn call<F, T>(&self, f: F) -> Result<T, AppError>
    where
        F: FnOnce(&mut Connection) -> rusqlite::Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let conn = Arc::clone(&self.conn);
        let progress = Arc::clone(&self.progress);
        let lang = crate::i18n::snapshot();
        tokio::task::spawn_blocking(move || {
            let mut guard = conn.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            crate::mastery::cache::in_scope(&progress, || crate::i18n::in_scope(lang, || f(&mut guard)))
        })
        .await
        .map_err(|e| AppError::Internal(format!("db task failed: {e}")))?
        .map_err(AppError::from)
    }

    /// Synchronous variant for startup/maintenance code and tests.
    pub fn with<F, T>(&self, f: F) -> rusqlite::Result<T>
    where
        F: FnOnce(&mut Connection) -> rusqlite::Result<T>,
    {
        let mut guard = self.conn.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        crate::mastery::cache::in_scope(&self.progress, || f(&mut guard))
    }
}

/// `SKRONGLE_SQL_TRACE=<file>`: append one JSON line per statement the
/// connection runs, with when it finished (`t`, ms since the epoch), how
/// long it took (`us`), SQLite's own counters for that run (`vm`
/// virtual-machine steps, `scan` rows read by a full table scan, `sort`
/// sorts) and the SQL with its values filled in. A dev aid for checking that
/// a page's queries stay bounded as the tables grow (docs/testing.md, "Query
/// trace"); nothing runs when the variable is unset.
fn start_trace(conn: &Connection) {
    let Some(path) = crate::env("SQL_TRACE") else { return };
    match std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        Ok(file) => {
            if TRACE_FILE.set(Mutex::new(file)).is_ok() {
                conn.trace_v2(TraceEventCodes::SQLITE_TRACE_STMT | TraceEventCodes::SQLITE_TRACE_PROFILE, Some(trace_statement));
            }
        }
        Err(e) => eprintln!("SKRONGLE_SQL_TRACE={path}: {e}"),
    }
}

static TRACE_FILE: OnceLock<Mutex<std::fs::File>> = OnceLock::new();
/// Each statement's counters when its run began, by statement text: SQLite
/// adds them up over a cached statement's runs, and the trace wants each
/// run's own share. The start event and the finish event come from the
/// same statement object, so the difference is exact.
static TRACE_START: Mutex<BTreeMap<String, (i32, i32, i32)>> = Mutex::new(BTreeMap::new());

fn counters(stmt: &rusqlite::trace::StmtRef<'_>) -> (i32, i32, i32) {
    (
        stmt.get_status(StatementStatus::VmStep),
        stmt.get_status(StatementStatus::FullscanStep),
        stmt.get_status(StatementStatus::Sort),
    )
}

fn trace_statement(event: TraceEvent<'_>) {
    let (stmt, took) = match event {
        TraceEvent::Stmt(stmt, _) => {
            let now = counters(&stmt);
            TRACE_START.lock().unwrap_or_else(|p| p.into_inner()).insert(stmt.sql().into_owned(), now);
            return;
        }
        TraceEvent::Profile(stmt, took) => (stmt, took),
        _ => return,
    };
    let now = counters(&stmt);
    let start = TRACE_START.lock().unwrap_or_else(|p| p.into_inner()).remove(stmt.sql().as_ref()).unwrap_or((0, 0, 0));
    let share = |cur: i32, prev: i32| if cur >= prev { cur - prev } else { cur };
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    let line = serde_json::json!({
        "t": t,
        "us": took.as_micros() as u64,
        "vm": share(now.0, start.0),
        "scan": share(now.1, start.1),
        "sort": share(now.2, start.2),
        "sql": stmt.expanded_sql().unwrap_or_default(),
    });
    if let Some(file) = TRACE_FILE.get() {
        let mut file = file.lock().unwrap_or_else(|p| p.into_inner());
        let _ = writeln!(file, "{line}");
    }
}

fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if current > schema_version() {
        return Err(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ERROR),
            Some(newer_db_message(current)),
        ));
    }
    for (i, sql) in MIGRATIONS.iter().enumerate() {
        let version = (i + 1) as i64;
        if version <= current {
            continue;
        }
        tracing::info!("applying schema migration v{version}");
        // `foreign_keys` cannot change inside a transaction.
        let rebuild = REBUILDS.contains(&version);
        let fk_on: bool = conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0))?;
        if rebuild {
            conn.execute_batch("PRAGMA foreign_keys = OFF;")?;
        }
        conn.execute_batch("BEGIN;")?;
        let applied = conn.execute_batch(sql).and_then(|()| {
            if rebuild && conn.prepare("PRAGMA foreign_key_check")?.query([])?.next()?.is_some() {
                return Err(rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT),
                    Some(format!("migration v{version} broke a foreign key")),
                ));
            }
            Ok(())
        });
        let done = applied.and_then(|()| conn.execute_batch(&format!("PRAGMA user_version = {version}; COMMIT;")));
        if done.is_err() {
            let _ = conn.execute_batch("ROLLBACK;");
        }
        if rebuild && fk_on {
            conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        }
        done?;
    }
    Ok(())
}

/// The plan SQLite picks for `sql` (parameters unbound), one line per step,
/// for tests that a query reads through an index (`SEARCH`) and does not
/// `SCAN` a table that grows.
#[cfg(test)]
pub fn query_plan(conn: &Connection, sql: &str) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?;
    // Unbound parameters are NULL; the plan does not depend on them.
    let mut rows = stmt.raw_query();
    let mut out = Vec::new();
    while let Some(r) = rows.next()? {
        out.push(r.get::<_, String>(3)?);
    }
    Ok(out)
}

/// The tables `sql` reads by a full `SCAN` (its plan's `SCAN` steps, less
/// constant rows and its own subqueries); a query on a table that grows
/// should have none.
#[cfg(test)]
pub fn table_scans(conn: &Connection, sql: &str) -> rusqlite::Result<Vec<String>> {
    let plan = query_plan(conn, sql)?;
    let own: Vec<&str> = plan.iter().filter_map(|l| l.strip_prefix("CO-ROUTINE ").or_else(|| l.strip_prefix("MATERIALIZE "))).collect();
    Ok(plan
        .iter()
        .filter_map(|l| l.strip_prefix("SCAN "))
        .filter(|t| *t != "CONSTANT ROW" && !own.contains(t))
        .map(str::to_string)
        .collect())
}

/// Unix time in seconds.
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Query-plan checks for tests: a statement that must not slow down as a
/// table grows asserts here that it finds its rows through an index.
#[cfg(test)]
pub mod plan {
    use rusqlite::Connection;

    /// The `EXPLAIN QUERY PLAN` lines of `sql` (tests): parameters stay
    /// unbound, so any statement can be planned without values.
    pub fn query_plan(conn: &Connection, sql: &str) -> Vec<String> {
        let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
        let mut rows = stmt.raw_query();
        let mut plan = Vec::new();
        while let Some(row) = rows.next().unwrap() {
            plan.push(row.get::<_, String>(3).unwrap());
        }
        plan
    }

    /// Fail unless `sql` reads `table` through an index (tests): no `SCAN` of
    /// it in the plan, and the plan names `index` when one is given.
    pub fn assert_indexed(conn: &Connection, sql: &str, table: &str, index: Option<&str>) {
        let plan = query_plan(conn, sql);
        assert!(
            !plan.iter().any(|l| l == &format!("SCAN {table}") || l.starts_with(&format!("SCAN {table} "))),
            "{sql}\nscans {table}: {plan:?}"
        );
        assert!(plan.iter().any(|l| l.starts_with(&format!("SEARCH {table} "))), "{sql}\ndoes not search {table}: {plan:?}");
        if let Some(index) = index {
            assert!(plan.iter().any(|l| l.contains(index)), "{sql}\ndoes not use {index}: {plan:?}");
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
