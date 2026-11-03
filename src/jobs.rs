//! Background jobs (#872): one `jobs` table and one runner thread.
//!
//! - A job is a row: a `kind` from [`KINDS`], a JSON payload and a time it
//!   may run (`run_at`). Code queues one with [`enqueue`] (inside a
//!   transaction it holds) and wakes the runner with [`Queue::notify`]. A
//!   kind not in [`KINDS`] is refused, with a log line.
//! - One OS thread (`jobs`) takes due rows one at a time with [`claim`], a
//!   single `UPDATE … RETURNING` statement. On SQLite that one statement
//!   holds the database's write lock, so it is safe across processes too;
//!   a Postgres store adds
//!   `FOR UPDATE SKIP LOCKED` to the inner select so several servers never
//!   take the same row.
//! - Taking a row leases it to this server until `locked_until`
//!   ([`LEASE`]). A `running` row whose lease ran out (its server stopped
//!   mid-job) is taken again, by the same server after a restart or by
//!   another one.
//! - A job that fails is tried again after its kind's delays (the mail
//!   outbox's [`crate::mail::RETRY_DELAYS`] by default); after
//!   `max_attempts` it is `failed`, with `last_error`. Every failure is
//!   logged.
//! - A kind with `every` queues its next run when a run ends (done, or
//!   failed for good): the hourly housekeeping is one such kind, with
//!   every purge as a step ([`housekeeping`]). At boot each of them gets a row due now if it has
//!   none, so a server restarted more often than hourly still cleans up.
//!   A kind with `at` queues it at the time `at` names instead, and its
//!   first row waits for that time too (the nightly content check, #1038).
//!
//! - The mail outbox (`mail_outbox`) keeps its own
//!   table but is drained by this runner too (#873): it is a [`Drain`].
//!   A row is leased the same way, with [`lease_outbox`] (`locked_by`,
//!   `locked_until`; it stays `queued` until it is sent or fails), so a row
//!   being sent is never handed out twice. Each pass sends at most
//!   [`OUTBOX_BATCH`] rows per outbox before the next job, and the outboxes
//!   go first, so a sign-in code never waits behind housekeeping.
//!
//! - AI jobs (#874, `ai/queue.rs`, kind [`crate::ai::GENERATE`]): a generation resumed after a restart is queued
//!   here but never run on this thread: the handler starts the job's own OS
//!   thread and returns [`Step::Handed`], and that thread renews the lease and
//!   records the end. A gate ([`Runner::handle_claimed`]) keeps the runner
//!   from taking one while every slot is busy.
//!
//! `/dev/jobs` lists the rows in dev mode.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use crate::db::Db;
use crate::mail::RETRY_DELAYS;

/// How long a taken job (or outbox row) is this server's. Past it, the row
/// is taken again.
pub const LEASE: i64 = 600;
/// Rows each outbox sends per pass before the runner looks at other work.
pub const OUTBOX_BATCH: usize = 10;
/// The runner looks at the table at least this often (seconds).
const MAX_WAIT: i64 = 60;
/// An hour, the housekeeping period.
const HOUR: i64 = 3600;
/// Finished rows are deleted after this long (the housekeeping); failed
/// ones after [`KEEP_FAILED`].
const KEEP_DONE: i64 = 86_400;
const KEEP_FAILED: i64 = 30 * 86_400;
/// Housekeeping tries a failed purge twice more before the next hour.
const HOUSEKEEPING_DELAYS: &[i64] = &[60, 300];

/// What the runner knows about a kind of job.
#[derive(Clone, Copy, Debug)]
pub struct Kind {
    pub name: &'static str,
    /// Runs, the first included, before the row is `failed`.
    pub max_attempts: i64,
    /// The wait before each retry; the last one repeats if there are more
    /// attempts than delays.
    pub delays: &'static [i64],
    /// Queue the next run this many seconds after a run ends.
    pub every: Option<i64>,
    /// Or at the time this gives after a run ends at `now` (the nightly
    /// content check runs at midnight, #1038). Its first run waits for it too.
    pub at: Option<fn(i64) -> i64>,
}

impl Kind {
    /// Does it queue its next run by itself?
    pub fn periodic(&self) -> bool {
        self.every.is_some() || self.at.is_some()
    }

    /// When the next run of a periodic kind is due, after a run ending at `now`.
    pub fn next_run(&self, now: i64) -> Option<i64> {
        self.at.map(|f| f(now)).or(self.every.map(|e| now + e))
    }
}

const fn hourly(name: &'static str) -> Kind {
    Kind { name, max_attempts: HOUSEKEEPING_DELAYS.len() as i64 + 1, delays: HOUSEKEEPING_DELAYS, every: Some(HOUR), at: None }
}

/// The hourly housekeeping: every purge in one job ([`housekeeping`]).
pub const HOUSEKEEPING: Kind = hourly("housekeeping");
/// "Download my data" (exports.rs), the whole file in one run. A failed run
/// fails the export (the learner can ask again at once, so one attempt); a
/// server that stopped mid-run takes the row again when its lease runs out.
pub const EXPORT: Kind = Kind { name: "export", max_attempts: 1, delays: RETRY_DELAYS, every: None, at: None };
/// Dev mode only (`/dev/jobs`): a job that always fails, to watch retries.
pub const DEV_FAIL: Kind = Kind { name: "dev.fail", max_attempts: RETRY_DELAYS.len() as i64 + 1, delays: RETRY_DELAYS, every: None, at: None };

/// Every kind there is. A row of another kind is refused by [`enqueue`].
pub const KINDS: &[Kind] = &[HOUSEKEEPING, EXPORT, DEV_FAIL, crate::ai::GENERATE];

pub fn kind(name: &str) -> Option<Kind> {
    KINDS.iter().find(|k| k.name == name).copied()
}

// ---------- the table ----------

/// Queue a job of `kind` due at `run_at`: its id, or `None` when the kind
/// is unknown (logged, nothing queued).
pub fn enqueue(conn: &Connection, kind_name: &str, payload: &serde_json::Value, run_at: i64, now: i64) -> rusqlite::Result<Option<i64>> {
    let Some(k) = kind(kind_name) else {
        tracing::warn!("jobs: refused a job of unknown kind {kind_name:?}");
        return Ok(None);
    };
    conn.execute(
        "INSERT INTO jobs (kind, payload_json, run_at, max_attempts, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![k.name, payload.to_string(), run_at, k.max_attempts, now],
    )?;
    Ok(Some(conn.last_insert_rowid()))
}

/// Queue `kind` unless a row of it is already waiting or running: due now,
/// or at its `at` time. The row id when one was added.
pub fn ensure(conn: &Connection, k: &Kind, now: i64) -> rusqlite::Result<Option<i64>> {
    let run_at = k.at.map_or(now, |f| f(now));
    let n = conn.execute(
        "INSERT INTO jobs (kind, payload_json, run_at, max_attempts, created_at)
         SELECT ?1, '{}', ?2, ?3, ?4 WHERE NOT EXISTS (SELECT 1 FROM jobs WHERE kind = ?1 AND state IN ('queued', 'running'))",
        params![k.name, run_at, k.max_attempts, now],
    )?;
    Ok((n > 0).then(|| conn.last_insert_rowid()))
}

/// A job the runner took.
#[derive(Debug, Clone, PartialEq)]
pub struct Claimed {
    pub id: i64,
    pub kind: String,
    pub payload: String,
    /// This run included.
    pub attempts: i64,
    pub max_attempts: i64,
}

/// Take the next due job of one of `kinds` for `worker`: a queued row whose
/// `run_at` has come, or a running one whose lease ran out. One statement;
/// on Postgres the inner select gets `FOR UPDATE SKIP LOCKED`.
pub fn claim(conn: &Connection, kinds: &[&str], worker: &str, now: i64) -> rusqlite::Result<Option<Claimed>> {
    if kinds.is_empty() {
        return Ok(None);
    }
    let marks = vec!["?"; kinds.len()].join(", ");
    let sql = format!(
        "UPDATE jobs SET state = 'running', locked_by = ?1, locked_until = ?2, attempts = attempts + 1
         WHERE id = (SELECT id FROM jobs
                     WHERE ((state = 'queued' AND run_at <= ?3) OR (state = 'running' AND locked_until <= ?3))
                       AND kind IN ({marks})
                     ORDER BY priority, run_at, id LIMIT 1)
         RETURNING id, kind, payload_json, attempts, max_attempts"
    );
    let mut values: Vec<rusqlite::types::Value> = vec![worker.to_string().into(), (now + LEASE).into(), now.into()];
    values.extend(kinds.iter().map(|k| rusqlite::types::Value::from(k.to_string())));
    conn.query_row(&sql, rusqlite::params_from_iter(values), |r| {
        Ok(Claimed { id: r.get(0)?, kind: r.get(1)?, payload: r.get(2)?, attempts: r.get(3)?, max_attempts: r.get(4)? })
    })
    .optional()
}

/// How a run ended, as [`finish`] recorded it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Done,
    /// Failed; tried again at this time.
    Retry(i64),
    /// Failed for the last time.
    Failed,
    /// The handler took the row over ([`Step::Handed`]).
    Handed,
}

/// Record how `job` ended (`Err`: the error text) and, for a kind with
/// `every` whose run is over for good, queue its next run. One transaction.
pub fn finish(conn: &mut Connection, job: &Claimed, result: Result<(), &str>, now: i64) -> rusqlite::Result<Outcome> {
    let k = kind(&job.kind);
    let tx = conn.transaction()?;
    let outcome = match result {
        Ok(()) => {
            tx.execute("UPDATE jobs SET state = 'done', locked_by = NULL, locked_until = NULL, finished_at = ?2 WHERE id = ?1", params![job.id, now])?;
            Outcome::Done
        }
        Err(e) if job.attempts < job.max_attempts => {
            let delays = k.map_or(RETRY_DELAYS, |k| k.delays);
            let wait = delays.get((job.attempts as usize).saturating_sub(1)).or(delays.last()).copied().unwrap_or(MAX_WAIT);
            let at = now + wait;
            tx.execute(
                "UPDATE jobs SET state = 'queued', locked_by = NULL, locked_until = NULL, run_at = ?2, last_error = ?3 WHERE id = ?1",
                params![job.id, at, e],
            )?;
            Outcome::Retry(at)
        }
        Err(e) => {
            tx.execute(
                "UPDATE jobs SET state = 'failed', locked_by = NULL, locked_until = NULL, last_error = ?2, finished_at = ?3 WHERE id = ?1",
                params![job.id, e, now],
            )?;
            Outcome::Failed
        }
    };
    if let (Some(next), Outcome::Done | Outcome::Failed) = (k.and_then(|k| k.next_run(now)), outcome) {
        tx.execute(
            "INSERT INTO jobs (kind, payload_json, run_at, max_attempts, created_at) SELECT kind, payload_json, ?2, max_attempts, ?3 FROM jobs WHERE id = ?1",
            params![job.id, next, now],
        )?;
    }
    tx.commit()?;
    Ok(outcome)
}

/// When the next job of `kinds` is due (a run time, or a lease running out).
pub fn next_due(conn: &Connection, kinds: &[&str]) -> rusqlite::Result<Option<i64>> {
    if kinds.is_empty() {
        return Ok(None);
    }
    let marks = vec!["?"; kinds.len()].join(", ");
    let sql = format!(
        "SELECT MIN(CASE state WHEN 'queued' THEN run_at ELSE locked_until END) FROM jobs
         WHERE state IN ('queued', 'running') AND kind IN ({marks})"
    );
    conn.query_row(&sql, rusqlite::params_from_iter(kinds.iter()), |r| r.get(0))
}

// ---------- the outbox tables ----------

/// Lease the next due row of an outbox table (`mail_outbox`)
/// to `worker`: a `queued` row whose `next_attempt_at` has come and that no
/// live lease holds (none, or one that ran out because its server stopped
/// mid-send). One statement, like [`claim`]; the row stays `queued` until the
/// sender records how it went and clears the lease. `columns` are read from
/// the leased row with `map`. `table` and `columns` are constants from code.
pub fn lease_outbox<T>(
    conn: &Connection,
    table: &str,
    columns: &str,
    worker: &str,
    now: i64,
    map: impl FnOnce(&rusqlite::Row) -> rusqlite::Result<T>,
) -> rusqlite::Result<Option<T>> {
    let sql = format!(
        "UPDATE {table} SET locked_by = ?1, locked_until = ?2
         WHERE id = (SELECT id FROM {table}
                     WHERE state = 'queued' AND next_attempt_at <= ?3 AND (locked_until IS NULL OR locked_until <= ?3)
                     ORDER BY next_attempt_at, id LIMIT 1)
         RETURNING {columns}"
    );
    conn.query_row(&sql, params![worker, now + LEASE, now], map).optional()
}

/// When an outbox table next has a row to send: its `next_attempt_at`, or
/// the end of the lease holding it, whichever is later.
pub fn outbox_next_due(conn: &Connection, table: &str) -> rusqlite::Result<Option<i64>> {
    conn.query_row(&format!("SELECT MIN(MAX(next_attempt_at, COALESCE(locked_until, 0))) FROM {table} WHERE state = 'queued'"), [], |r| r.get(0))
}

/// Delete finished rows older than a day and failed ones older than 30 days.
pub fn purge(conn: &Connection, now: i64) -> rusqlite::Result<usize> {
    conn.execute(
        "DELETE FROM jobs WHERE (state = 'done' AND finished_at < ?1) OR (state = 'failed' AND finished_at < ?2)",
        params![now - KEEP_DONE, now - KEEP_FAILED],
    )
}

/// A row as `/dev/jobs` shows it.
#[derive(Debug, Clone, Serialize)]
pub struct Row {
    pub id: i64,
    pub kind: String,
    pub payload: String,
    pub state: String,
    pub priority: i64,
    pub run_at: i64,
    pub attempts: i64,
    pub max_attempts: i64,
    pub locked_by: Option<String>,
    pub locked_until: Option<i64>,
    pub last_error: Option<String>,
    pub created_at: i64,
    pub finished_at: Option<i64>,
}

/// The newest `limit` rows, newest first.
pub fn recent(conn: &Connection, limit: usize) -> rusqlite::Result<Vec<Row>> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, payload_json, state, priority, run_at, attempts, max_attempts, locked_by, locked_until, last_error, created_at, finished_at
         FROM jobs ORDER BY id DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit as i64], |r| {
        Ok(Row {
            id: r.get(0)?,
            kind: r.get(1)?,
            payload: r.get(2)?,
            state: r.get(3)?,
            priority: r.get(4)?,
            run_at: r.get(5)?,
            attempts: r.get(6)?,
            max_attempts: r.get(7)?,
            locked_by: r.get(8)?,
            locked_until: r.get(9)?,
            last_error: r.get(10)?,
            created_at: r.get(11)?,
            finished_at: r.get(12)?,
        })
    })?;
    rows.collect()
}

/// Dev: make a queued row due now, or a running row's lease run out. The
/// number of rows changed (0: no such row in either state).
pub fn dev_run_now(conn: &Connection, id: i64, now: i64) -> rusqlite::Result<usize> {
    conn.execute(
        "UPDATE jobs SET run_at = CASE state WHEN 'queued' THEN ?2 ELSE run_at END,
                         locked_until = CASE state WHEN 'running' THEN ?2 ELSE locked_until END
         WHERE id = ?1 AND state IN ('queued', 'running')",
        params![id, now],
    )
}

// ---------- the runner ----------

/// Wakes the runner; kept in `AppState`.
pub struct Queue {
    wake: Mutex<Sender<()>>,
}

impl Queue {
    /// The handle and the receiver [`Runner::start`] takes.
    pub fn new() -> (Queue, Receiver<()>) {
        let (tx, rx) = mpsc::channel();
        (Queue { wake: Mutex::new(tx) }, rx)
    }

    /// A queue nobody runs (router tests).
    #[cfg(test)]
    pub fn idle() -> Queue {
        Queue::new().0
    }

    /// Wake the runner: a job was queued or made due.
    pub fn notify(&self) {
        let _ = self.wake.lock().unwrap_or_else(|p| p.into_inner()).send(());
    }

    /// A handle that wakes the runner, for the mail and text senders.
    pub fn waker(&self) -> Sender<()> {
        self.wake.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
}

/// An outbox the runner drains besides the `jobs` table: the mail and text
/// outboxes (#873).
pub struct Drain {
    name: &'static str,
    /// Lease one due row, send it and record the outcome: whether there was one.
    send_one: Box<dyn Fn(&Db, &str, i64) -> rusqlite::Result<bool> + Send>,
    /// When a row is next due ([`outbox_next_due`]).
    next_due: Box<dyn Fn(&Db) -> rusqlite::Result<Option<i64>> + Send>,
}

impl Drain {
    pub fn new(
        name: &'static str,
        send_one: impl Fn(&Db, &str, i64) -> rusqlite::Result<bool> + Send + 'static,
        next_due: impl Fn(&Db) -> rusqlite::Result<Option<i64>> + Send + 'static,
    ) -> Drain {
        Drain { name, send_one: Box::new(send_one), next_due: Box::new(next_due) }
    }
}

/// What a run of a job did, when it did not fail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Finished, with a short note for the log (empty: nothing to say).
    Done(String),
    /// The handler took the row over (a thread of its own renews its lease
    /// and records the end): the runner leaves it as it is.
    Handed(String),
}

/// What a job does with the row it was given: a [`Step`], or the error text.
pub type Handler = Box<dyn Fn(&Db, &Claimed) -> Result<Step, String> + Send>;

/// The kinds this server runs and how.
pub struct Runner {
    handlers: HashMap<&'static str, Handler>,
    /// Kinds the runner takes only while their gate says so.
    gates: HashMap<&'static str, Box<dyn Fn() -> bool + Send>>,
    /// The outboxes, drained before the jobs on every pass.
    drains: Vec<Drain>,
    /// `locked_by` on the rows this server takes.
    worker: String,
    /// Look at the table at least this often (seconds), even unwoken.
    poll: i64,
}

impl Runner {
    pub fn new() -> Runner {
        Runner { handlers: HashMap::new(), gates: HashMap::new(), drains: Vec::new(), worker: format!("pid{}@{}", std::process::id(), crate::db::now()), poll: MAX_WAIT }
    }

    /// Run kind `k` with `f`: `Ok` with a note for the log, or the error.
    pub fn handle(self, k: Kind, f: impl Fn(&Db, &str) -> Result<String, String> + Send + 'static) -> Runner {
        self.handle_steps(k, move |db, payload| f(db, payload).map(Step::Done))
    }

    /// Run kind `k` with `f`, which returns a [`Step`] (it may hand the row on).
    pub fn handle_steps(mut self, k: Kind, f: impl Fn(&Db, &str) -> Result<Step, String> + Send + 'static) -> Runner {
        self.handlers.insert(k.name, Box::new(move |db: &Db, job: &Claimed| f(db, &job.payload)));
        self
    }

    /// Run kind `k` with `f`, given the whole row, and take a row of it only
    /// while `gate` says yes (a free slot for an AI job).
    pub fn handle_claimed(
        mut self,
        k: Kind,
        gate: impl Fn() -> bool + Send + 'static,
        f: impl Fn(&Db, &Claimed) -> Result<Step, String> + Send + 'static,
    ) -> Runner {
        self.handlers.insert(k.name, Box::new(f));
        self.gates.insert(k.name, Box::new(gate));
        self
    }

    /// The kinds it may take now: every gate open.
    fn open_kinds(&self) -> Vec<&'static str> {
        self.kinds().into_iter().filter(|k| self.gates.get(k).is_none_or(|g| g())).collect()
    }

    /// Drain `d` on every pass, before the jobs.
    pub fn drain(mut self, d: Drain) -> Runner {
        self.drains.push(d);
        self
    }

    /// Send up to [`OUTBOX_BATCH`] due rows from each outbox: how many were
    /// handled. An error is logged and ends that outbox's turn.
    pub fn run_drains(&self, db: &Db, now: i64) -> usize {
        let mut handled = 0;
        for d in &self.drains {
            for _ in 0..OUTBOX_BATCH {
                match (d.send_one)(db, &self.worker, now) {
                    Ok(true) => handled += 1,
                    Ok(false) => break,
                    Err(e) => {
                        tracing::warn!("jobs: {} outbox: {e}", d.name);
                        break;
                    }
                }
            }
        }
        handled
    }

    /// When the next job or outbox row is due.
    fn next_due(&self, db: &Db, kinds: &[&str]) -> Option<i64> {
        let jobs = db.with(|c| next_due(c, kinds)).ok().flatten();
        self.drains.iter().filter_map(|d| (d.next_due)(db).ok().flatten()).chain(jobs).min()
    }

    fn kinds(&self) -> Vec<&'static str> {
        let mut v: Vec<&'static str> = self.handlers.keys().copied().collect();
        v.sort_unstable();
        v
    }

    /// Give every periodic kind this server runs a row due now if it has
    /// none (a fresh database, or one whose rows were deleted).
    pub fn seed(&self, db: &Db, now: i64) -> rusqlite::Result<usize> {
        let mut added = 0;
        for name in self.kinds() {
            if let Some(k) = kind(name).filter(Kind::periodic) {
                added += db.with(|c| ensure(c, &k, now))?.map_or(0, |_| 1);
            }
        }
        Ok(added)
    }

    /// Take one due job, run it and record the outcome. `None`: nothing was due.
    pub fn run_one(&self, db: &Db, now: i64) -> rusqlite::Result<Option<(Claimed, Outcome)>> {
        let kinds = self.open_kinds();
        let Some(job) = db.with(|c| claim(c, &kinds, &self.worker, now))? else { return Ok(None) };
        let result = match self.handlers.get(job.kind.as_str()) {
            Some(f) => f(db, &job),
            None => Err(format!("no handler for {}", job.kind)),
        };
        let done_at = crate::db::now().max(now);
        let outcome = match &result {
            Ok(Step::Handed(_)) => Outcome::Handed,
            Ok(Step::Done(_)) => db.with(|c| finish(c, &job, Ok(()), done_at))?,
            Err(e) => db.with(|c| finish(c, &job, Err(e.as_str()), done_at))?,
        };
        match (&result, outcome) {
            (Ok(Step::Handed(note)), _) => tracing::debug!("job #{} {} handed on ({note})", job.id, job.kind),
            (Ok(Step::Done(note)), _) if note.is_empty() => tracing::debug!("job #{} {} done", job.id, job.kind),
            (Ok(Step::Done(note)), _) => tracing::info!("job #{} {}: {note}", job.id, job.kind),
            (Err(e), Outcome::Retry(at)) => tracing::warn!(
                "job #{} {} failed (attempt {} of {}): {e}; trying again in {}s",
                job.id,
                job.kind,
                job.attempts,
                job.max_attempts,
                at - done_at
            ),
            (Err(e), _) => tracing::warn!("job #{} {} failed for good after {} attempts: {e}", job.id, job.kind, job.attempts),
        }
        Ok(Some((job, outcome)))
    }

    /// One pass: a batch from each outbox, then one due job. Whether
    /// anything was done.
    pub fn pass(&self, db: &Db, now: i64) -> bool {
        let sent = self.run_drains(db, now);
        let ran = match self.run_one(db, now) {
            Ok(r) => r.is_some(),
            Err(e) => {
                tracing::warn!("jobs: {e}");
                false
            }
        };
        sent > 0 || ran
    }

    /// Start the `jobs` thread: seed the periodic kinds, then send due
    /// outbox rows and run due jobs, waking on `wake` or when the next one
    /// is due (at least once a minute).
    pub fn start(self, db: Db, wake: Receiver<()>) -> std::io::Result<()> {
        std::thread::Builder::new().name("jobs".into()).spawn(move || {
            if let Err(e) = self.seed(&db, crate::db::now()) {
                tracing::warn!("jobs: {e}");
            }
            loop {
                while self.pass(&db, crate::db::now()) {}
                // A gated kind wakes the runner itself when its gate opens.
                let next = self.next_due(&db, &self.open_kinds());
                let wait = next.map(|t| (t - crate::db::now()).clamp(1, self.poll)).unwrap_or(self.poll) as u64;
                match wake.recv_timeout(Duration::from_secs(wait)) {
                    Ok(()) | Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
        })?;
        Ok(())
    }
}

// ---------- housekeeping ----------

/// The hourly housekeeping, one job that runs every purge in turn: expired
/// sessions, abandoned games, old generation jobs, idle tutor chats,
/// rate-limit windows, accounts due for deletion (#970), the retention rules
/// (privacy.rs), old outbox rows (mail.rs), old verification, reset,
/// two-factor and invitation links, expired data exports with their files,
/// old job rows, and last a WAL checkpoint so the -wal file doesn't grow on
/// the SD card. Each step logs its count; a step that fails is logged and the
/// next still runs, and the job fails only when every step did (#45).
pub fn housekeeping(runner: Runner, state: &Arc<crate::AppState>, retention: crate::privacy::Retention) -> Runner {
    let state = Arc::clone(state);
    runner.handle(HOUSEKEEPING, move |db, _| {
        let s = &state;
        let now = crate::db::now;
        type Step<'a> = (&'static str, Box<dyn Fn() -> Result<String, String> + 'a>);
        let steps: Vec<Step> = vec![
            ("sessions", Box::new(|| sql(db, |c, _| crate::auth::purge_expired_sessions(c).map(|n| note(n, "expired sessions purged"))))),
            ("games", Box::new(|| Ok(note(s.games.purge_expired(), "abandoned games dropped")))),
            ("ai_jobs", Box::new(|| Ok(note(s.jobs.purge_expired(), "old generation jobs dropped")))),
            ("tutor_chats", Box::new(|| Ok(note(s.tutors.purge_expired(), "idle tutor chats dropped")))),
            ("rate_limits", Box::new(|| Ok(note(s.limits.purge_expired(now()), "rate-limit windows dropped")))),
            ("deletions", Box::new(|| crate::account_deletion::sweep(s, db))),
            (
                "retention",
                Box::new(|| {
                    let swept = db.with(|c| crate::privacy::apply_retention(c, &retention, now())).map_err(|e| e.to_string())?;
                    for id in &swept.failed_courses {
                        s.courses.invalidate(id);
                    }
                    Ok(match swept.failed_courses.len() {
                        0 => String::new(),
                        n => format!("deleted {n} failed AI course(s) past SKRONGLE_FAILED_COURSE_RETENTION_DAYS"),
                    })
                }),
            ),
            ("outbox", Box::new(|| sql(db, |c, now| Ok(note(crate::mail::purge(c, now)?, "old outbox rows deleted"))))),
            (
                "links",
                Box::new(|| {
                    sql(db, |c, now| {
                        let n = crate::verify::purge(c, now)? + crate::reset::purge(c, now)? + crate::two_factor::purge(c, now)? + crate::signup::purge(c, now)?;
                        Ok(note(n, "old links deleted"))
                    })
                }),
            ),
            (
                "exports",
                Box::new(|| {
                    let files = db.with(|c| crate::exports::purge(c, now())).map_err(|e| e.to_string())?;
                    s.exports.remove(&files);
                    let strays = crate::exports::remove_strays(db, &s.exports.dir);
                    Ok(note(files.len() + strays, "expired or stray export files removed"))
                }),
            ),
            ("jobs", Box::new(|| sql(db, |c, now| purge(c, now).map(|n| note(n, "old job rows deleted"))))),
            ("wal_checkpoint", Box::new(|| sql(db, |c, _| c.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);").map(|()| String::new())))),
        ];
        let (mut notes, mut failed, total) = (Vec::new(), Vec::new(), steps.len());
        for (name, run) in &steps {
            match run() {
                Ok(n) if n.is_empty() => {}
                Ok(n) => {
                    tracing::info!("housekeeping.{name}: {n}");
                    notes.push(n);
                }
                Err(e) => {
                    tracing::warn!("housekeeping.{name} failed: {e}");
                    failed.push(format!("{name}: {e}"));
                }
            }
        }
        // One broken purge must not stop the rest (the WAL checkpoint above
        // all); the job fails only when every step did.
        if failed.len() == total {
            return Err(failed.join("; "));
        }
        Ok(notes.join("; "))
    })
}

/// Dev mode: the always-failing kind `/dev/jobs` queues.
pub fn dev(runner: Runner) -> Runner {
    runner.handle(DEV_FAIL, |_, _| Err("dev.fail always fails (queued from /dev/jobs)".into()))
}

/// Run `f` on the connection with the current time; errors as text.
fn sql(db: &Db, f: impl FnOnce(&mut Connection, i64) -> rusqlite::Result<String>) -> Result<String, String> {
    db.with(|c| f(c, crate::db::now())).map_err(|e| e.to_string())
}

/// "3 things done", or nothing (a debug line) when none were.
fn note(n: usize, what: &str) -> String {
    if n == 0 {
        String::new()
    } else {
        format!("{n} {what}")
    }
}

#[cfg(test)]
mod tests;
