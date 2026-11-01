//! `skrongle content-check` (#1041): the content check in a process of its
//! own, beside the server on the same database. The server
//! (`SKRONGLE_CONTENT_CHECK=external`) queues the checks and its jobs thread
//! skips them; this process takes only the `content_check.*` kinds from the
//! `jobs` table with the usual claim and lease, so a worker stopped
//! mid-check leaves a job that is taken again when its lease runs out. A
//! course it flags is handed back to the server as a [`super::FLAGGED`] job
//! (the server drops it from its cache).
//!
//! It checks in every [`CHECK_IN`] seconds (`content_check_worker`); the
//! superuser's course table shows when, and how many checks wait. The
//! process setup (settings, the database, dev stand-ins) is in
//! `src/check_worker.rs`.
//!
//! On the same computer only for now: the hosted database is SQLite.
//! Running it on another computer waits for the shared database (#581).

use std::sync::Arc;

use rusqlite::{params, Connection, OptionalExtension};

use crate::db::Db;
use crate::jobs::{self, Runner};
use crate::metering::Meter;

/// How often the worker writes that it is alive (seconds).
pub const CHECK_IN: u64 = 60;
/// How often it looks for a job nobody woke it for (seconds).
pub const POLL: i64 = 2;
/// Silent this long, the superuser page shows a red ! (seconds).
pub const SILENT: i64 = 86_400;

/// Write that `name` is alive at `now`.
pub fn check_in(conn: &Connection, name: &str, now: i64) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO content_check_worker (id, name, seen_at) VALUES (1, ?1, ?2)
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, seen_at = excluded.seen_at",
        params![name, now],
    )?;
    Ok(())
}

/// The worker as the server sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    /// When it last checked in; `None`: never.
    pub seen_at: Option<i64>,
    /// Scans and reviews queued or running (not the nightly run, which always waits).
    pub waiting: i64,
}

impl Status {
    /// Never seen, or silent for [`SILENT`].
    pub fn silent(&self, now: i64) -> bool {
        self.seen_at.is_none_or(|t| now - t >= SILENT)
    }
}

pub fn status(conn: &Connection) -> rusqlite::Result<Status> {
    let seen_at = conn.query_row("SELECT seen_at FROM content_check_worker WHERE id = 1", [], |r| r.get(0)).optional()?;
    let waiting = conn.query_row(
        "SELECT COUNT(*) FROM jobs WHERE kind IN (?1, ?2) AND state IN ('queued', 'running')",
        params![super::SCAN.name, super::review::REVIEW.name],
        |r| r.get(0),
    )?;
    Ok(Status { seen_at, waiting })
}

/// The worker's jobs: the checks, each flag handed to the server as a
/// [`super::FLAGGED`] job. Jev is set ([`super::init_worker`]).
pub fn runner(db: &Db, meter: Arc<Meter>) -> Runner {
    let db2 = db.clone();
    let on_flag: super::OnFlag = Arc::new(move |id: &str| {
        let payload = serde_json::json!({ "course": id });
        let now = crate::db::now();
        if let Err(e) = db2.with(|c| jobs::enqueue(c, super::FLAGGED.name, &payload, now, now)) {
            tracing::warn!("content check: could not tell the server course {id} is flagged: {e}");
        }
    });
    super::checks(Runner::new().poll_every(POLL), meter, on_flag)
}

/// Start the jobs thread and check in until the process is stopped.
pub fn run(db: Db, meter: Arc<Meter>) -> Result<(), String> {
    let runner = runner(&db, meter);
    let name = runner.worker().to_string();
    // Nobody wakes it: it polls. The sender stays alive so the thread does too.
    let (_queue, wake) = jobs::Queue::new();
    runner.start(db.clone(), wake).map_err(|e| format!("content check thread: {e}"))?;
    tracing::info!("content check worker {name}: taking checks from the jobs table every {POLL} s");
    loop {
        if let Err(e) = db.with(|c| check_in(c, &name, crate::db::now())) {
            tracing::warn!("content check worker: check-in failed: {e}");
        }
        std::thread::sleep(std::time::Duration::from_secs(CHECK_IN));
    }
}

#[cfg(test)]
mod tests;
