//! The line for AI jobs (#874): generations, revisions, added lessons and
//! translations.
//!
//! - Each job is a row of kind [`GENERATE`] on the `jobs` table (`jobs.rs`).
//!   Its payload names the job, the course and how to rebuild the request,
//!   never a key.
//! - [`slots`] jobs run at once (`SKRONGLE_AI_JOBS`, default 2), each on its
//!   own OS thread: never on Tokio's blocking pool, never on the one runner
//!   thread. A job that finds a slot free (and nobody waiting) starts at
//!   once. Past that it waits in line, at most [`line_limit`] deep (10 on the
//!   hosted site, 3 on Skrongle mini); past that it is refused, as before,
//!   with a log line.
//! - The runner (`jobs.rs`) takes the next waiting row when a slot frees and
//!   hands it to a new generator thread ([`handler`], `Step::Handed`). While a
//!   job runs, one heartbeat thread renews the leases of this server's rows
//!   every [`HEARTBEAT`], so a run of up to an hour keeps its row; the
//!   generator thread records the end itself.
//! - After a restart ([`at_boot`], [`adopt`]) a generation goes on by itself
//!   from the lessons already written, when its key can be had again: the
//!   server's own (Skrongle AI) or one saved on the profile. A key pasted into
//!   the form lived only in memory, so that job stops and keeps its *Resume*
//!   button, as before. Revisions, added lessons and translations stop too.

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use super::*;
use crate::jobs::{Claimed, Kind, Step, LEASE};

/// A generation, revision, added lesson or translation: one row per job.
/// The runner never retries one; past `max_attempts` takes (a server that
/// keeps stopping mid-run) the job stops for good.
pub const GENERATE: Kind = Kind { name: "generate_course", max_attempts: 3, delays: crate::mail::RETRY_DELAYS, every: None, at: None };
/// Jobs running at once unless `SKRONGLE_AI_JOBS` says otherwise.
pub const DEFAULT_SLOTS: usize = 2;
/// Jobs that may wait in line.
pub const LINE: usize = 3;
/// How often the heartbeat renews the leases of running jobs.
pub const HEARTBEAT: Duration = Duration::from_secs(120);

/// Jobs running at once (`SKRONGLE_AI_JOBS`, default [`DEFAULT_SLOTS`]).
pub fn slots() -> usize {
    crate::env("AI_JOBS").and_then(|v| v.parse::<usize>().ok()).filter(|n| *n > 0).unwrap_or(DEFAULT_SLOTS)
}

/// Jobs that may wait in line.
pub fn line_limit() -> usize {
    LINE
}

/// `locked_by` on the rows this server's generator threads hold.
fn worker() -> &'static str {
    static W: OnceLock<String> = OnceLock::new();
    W.get_or_init(|| format!("pid{}-ai", std::process::id()))
}

/// The job table: every job by id (finished ones until they expire), the
/// generator threads still alive and the jobs waiting in line. Capacity
/// counts threads, not table rows, so a job dropped from the table (its
/// account deleted) still holds its slot until its thread has exited.
#[derive(Default)]
pub(super) struct JobTable {
    pub(super) map: HashMap<String, Arc<Job>>,
    /// Job id → (user id, `jobs` row), for each generator thread that has not exited.
    pub(super) live: HashMap<String, (i64, i64)>,
    /// Job id → (user id, `jobs` row), for each job waiting in line.
    pub(super) waiting: HashMap<String, (i64, i64)>,
    /// The requests of waiting jobs. A pasted key lives only here; a job
    /// adopted after a restart has none (its request is rebuilt).
    pending: HashMap<String, Request>,
    /// Wakes the runner when a slot frees.
    wake: Option<Sender<()>>,
    heartbeat: bool,
}

#[derive(Default)]
pub struct Jobs {
    inner: Arc<Mutex<JobTable>>,
}

/// Held by a generator thread: frees its slot when the thread ends, however
/// it ends, and wakes the runner for whoever waits next.
struct LiveSlot {
    table: Arc<Mutex<JobTable>>,
    id: String,
}

impl Drop for LiveSlot {
    fn drop(&mut self) {
        let mut t = self.table.lock().unwrap_or_else(|p| p.into_inner());
        t.live.remove(&self.id);
        if let Some(w) = &t.wake {
            let _ = w.send(());
        }
    }
}

impl JobTable {
    /// Whether `user_id` may have a job now (one each): `true` to start at
    /// once, `false` to wait in line. Refused when the line is full.
    fn capacity(&self, user_id: i64) -> Result<bool, String> {
        if self.live.values().chain(self.waiting.values()).any(|(u, _)| *u == user_id) {
            return Err("You already have a course being generated; wait for it to finish (or cancel it).".into());
        }
        if self.live.len() < slots() && self.waiting.is_empty() {
            return Ok(true);
        }
        if self.waiting.len() >= line_limit() {
            tracing::warn!("generate: refused user {user_id}: {} job(s) running and {} waiting, the line is full", self.live.len(), self.waiting.len());
            return Err("Too many courses are being generated right now; try again in a few minutes.".into());
        }
        Ok(false)
    }

    /// Keep at most `MAX_FINISHED_PER_USER` finished jobs of `user_id`, the newest.
    pub(super) fn trim_finished(&mut self, user_id: i64) {
        let mut finished: Vec<(i64, String)> = self
            .map
            .iter()
            .filter(|(_, j)| j.user_id == user_id)
            .filter_map(|(id, j)| j.snapshot().finished_at.map(|t| (t, id.clone())))
            .collect();
        if finished.len() <= MAX_FINISHED_PER_USER {
            return;
        }
        finished.sort();
        let extra = finished.len() - MAX_FINISHED_PER_USER;
        for (_, id) in finished.into_iter().take(extra) {
            self.map.remove(&id);
        }
    }
}

impl Jobs {
    pub fn get(&self, id: &str) -> Option<Arc<Job>> {
        self.lock().map.get(id).cloned()
    }

    /// Wake the runner through `wake` when a slot frees (`main.rs`).
    pub fn wake_with(&self, wake: Sender<()>) {
        self.lock().wake = Some(wake);
    }

    /// The job's progress, with its place when it waits in line.
    pub fn status(&self, job: &Job) -> Status {
        let mut s = job.snapshot();
        let table = self.lock();
        if let Some((_, row)) = table.waiting.get(&s.id) {
            let n = table.waiting.values().filter(|(_, r)| r <= row).count();
            s.place = Some(n);
            s.phase = crate::t1("courses.generate.in_line", "n", n);
        }
        s
    }

    /// A job that has already finished (a generation, no error), for tests
    /// of the pages that follow a job: what the editor shows when the job
    /// is done but the course row has not caught up.
    #[cfg(test)]
    pub fn insert_finished_for_test(&self, id: &str, user_id: i64, course_id: &str) {
        let job = Job {
            user_id,
            course_id: course_id.into(),
            status: Mutex::new(Status {
                id: id.into(),
                phase: "Done".into(),
                log: vec![],
                lessons_done: 3,
                lessons_total: 3,
                done: true,
                error: None,
                course_id: Some(course_id.into()),
                course_title: None,
                revision: false,
                adding: false,
                editing: false,
                lesson_title: None,
                translating: false,
                place: None,
                finished_at: Some(crate::db::now()),
                notes: vec![],
            }),
            cancel: AtomicBool::new(false),
            deadline: Mutex::new(std::time::Instant::now()),
        };
        self.lock().map.insert(id.into(), Arc::new(job));
    }

    /// Stop and forget every job of `user_id` (their account is being
    /// deleted): a running one stops before its next request, and its writes
    /// fail once the course row is gone; a waiting one leaves the line (its
    /// row stops when the runner reaches it). A thread keeps its slot until
    /// it has exited.
    pub fn drop_user(&self, user_id: i64) {
        let mut table = self.lock();
        for j in table.map.values().filter(|j| j.user_id == user_id) {
            j.cancel.store(true, Ordering::SeqCst);
        }
        table.map.retain(|_, j| j.user_id != user_id);
        table.waiting.retain(|_, (u, _)| *u != user_id);
        table.pending.retain(|_, r| r.user_id != user_id);
    }

    pub fn purge_expired(&self) -> usize {
        let cutoff = crate::db::now() - JOB_TTL_SECS;
        let mut table = self.lock();
        let before = table.map.len();
        table.map.retain(|_, j| j.snapshot().finished_at.map(|t| t > cutoff).unwrap_or(true));
        before - table.map.len()
    }

    pub(super) fn lock(&self) -> std::sync::MutexGuard<'_, JobTable> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// Whether this user may start a job now, or wait in line (one each).
/// `start` checks again, under the same lock as it takes the slot.
pub fn check_capacity(jobs: &Jobs, user_id: i64) -> Result<(), String> {
    jobs.lock().capacity(user_id).map(drop)
}

pub fn new_job_id() -> String {
    crate::auth::generate_token().chars().take(16).collect()
}

/// How [`start`] took a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Started {
    /// Its thread is running.
    Now,
    /// It waits in line; the runner starts it when a slot frees.
    Waiting,
}

fn new_job(req: &Request, id: &str) -> Job {
    Job {
        user_id: req.user_id,
        course_id: req.course_id.clone(),
        status: Mutex::new(Status {
            id: id.to_string(),
            phase: "Starting".into(),
            log: Vec::new(),
            lessons_done: 0,
            lessons_total: req.lesson_count,
            done: false,
            error: None,
            course_id: Some(req.course_id.clone()),
            course_title: None,
            revision: req.changes_a_course(),
            adding: req.add_lesson.is_some(),
            editing: req.edit.is_some(),
            lesson_title: req.edit.as_ref().and_then(|e| e.lesson.as_ref()).map(|(_, title)| title.clone()),
            translating: req.translate.is_some(),
            place: None,
            finished_at: None,
            notes: vec![],
        }),
        cancel: AtomicBool::new(false),
        deadline: Mutex::new(std::time::Instant::now() + job_budget()),
    }
}

/// Take a job writing into `req.course_id` (already registered as
/// 'generating' or 'revising' with this job id), held by `jobs` row `row`:
/// start its thread now, or put it in line. When it ends the course is
/// marked ready or failed, and the row done or failed.
pub fn start(jobs: &Jobs, db: Db, registry: Arc<crate::courses::Courses>, req: Request, id: String, row: i64) -> Result<Started, String> {
    let job = Arc::new(new_job(&req, &id));
    // Check and take the slot (or the place in line) under one lock, so two
    // requests at once cannot both pass the check.
    let mut table = jobs.lock();
    let now = table.capacity(req.user_id)?;
    table.map.insert(id.clone(), Arc::clone(&job));
    table.trim_finished(req.user_id);
    if !now {
        job.update(|s| s.phase = "Waiting".into());
        table.waiting.insert(id.clone(), (req.user_id, row));
        table.pending.insert(id, req);
        return Ok(Started::Waiting);
    }
    table.live.insert(id.clone(), (req.user_id, row));
    drop(table);
    launch(jobs, job, db, registry, req, id, row).map(|()| Started::Now)
}

/// Start `job`'s generator thread; its slot in `live` is already taken.
fn launch(jobs: &Jobs, job: Arc<Job>, db: Db, registry: Arc<crate::courses::Courses>, req: Request, id: String, row: i64) -> Result<(), String> {
    *job.deadline.lock().unwrap_or_else(|p| p.into_inner()) = std::time::Instant::now() + job_budget();
    job.update(|s| s.phase = "Starting".into());
    heartbeat(jobs, &db);
    let slot = LiveSlot { table: Arc::clone(&jobs.inner), id: id.clone() };
    let spawned = std::thread::Builder::new().name("course-generator".into()).spawn(move || {
        // Freed when this thread ends, not when the job leaves the table.
        let _slot = slot;
        let cid = req.course_id.clone();
        // The files as they were, for "Review AI changes".
        let before = db.with(|c| crate::courses::load_files(c, &cid));
        let outcome = match (&req.edit, &req.revise, &req.add_lesson, &req.translate) {
            (Some(EditRun { feedback, lesson: Some((path, _)) }), ..) => edit_lesson::edit_lesson(&job, &db, &registry, &req, path, feedback),
            (Some(EditRun { feedback, lesson: None }), ..) => edit::edit(&job, &db, &registry, &req, feedback),
            (None, Some(feedback), _, _) => revise(&job, &db, &registry, &req, feedback),
            (None, None, Some(brief), _) => add_lesson(&job, &db, &registry, &req, brief),
            // Its progress is worded in the language of the request that started it.
            (None, None, None, Some(t)) => crate::i18n::in_scope(t.i18n, || translate(&job, &db, &registry, &req, &t.plan)),
            (None, None, None, None) => run(&job, &db, &registry, &req),
        };
        if let Some(m) = &req.meter {
            // Whatever the outcome, what the job did not spend goes back.
            m.release(&job.snapshot().id);
        }
        let err = outcome.as_ref().err().cloned();
        // Still 'generating' or 'revising', so the files are what the AI
        // wrote. A stopped generation keeps the lessons it wrote, so they
        // are kept for review too; a failed revision changed nothing. A
        // translation has nothing to review: every text of it changed.
        if let (Ok(before), None) = (before, &req.translate) {
            let recorded = db.with(|c| {
                let label = if req.changes_a_course() {
                    crate::courses::undo_step(c, &cid)?.map(|s| s.label).unwrap_or_default()
                } else {
                    crate::courses::quoted("Generated with AI:", &req.wish)
                };
                crate::courses::record_ai_run(c, &cid, &before, &label, &notes_to_json(&job.snapshot().notes))
            });
            if let Err(e) = recorded {
                tracing::warn!("generate: could not keep the changes of {cid} for review: {e}");
            }
        }
        let finished = if req.changes_a_course() {
            // A revision that fails has changed nothing: the course is as it was.
            db.with(|c| crate::courses::finish_revision(c, &cid))
        } else {
            db.with(|c| crate::courses::finish_generation(c, &cid, err.as_deref()))
        };
        if let Err(e) = finished.and_then(|()| db.with(|c| settle(c, row, err.as_deref(), crate::db::now()))) {
            tracing::warn!("generate: could not record the end of {cid}: {e}");
        }
        job.update(|s| {
            s.done = true;
            s.finished_at = Some(crate::db::now());
            match outcome {
                Ok(()) => s.phase = "Done".into(),
                Err(e) => {
                    s.phase = "Stopped".into();
                    s.error = Some(e);
                }
            }
        });
    });
    if let Err(e) = spawned {
        // The closure, and the slot it held, went with the error.
        jobs.lock().map.remove(&id);
        return Err(format!("could not start the generator: {e}"));
    }
    Ok(())
}

/// Start the one heartbeat thread, the first time a job runs: every
/// [`HEARTBEAT`] it renews the lease of each running job's row, so a run
/// longer than `jobs::LEASE` keeps its row. A server that stops lets them run out.
fn heartbeat(jobs: &Jobs, db: &Db) {
    let mut t = jobs.lock();
    if t.heartbeat {
        return;
    }
    let (table, db) = (Arc::clone(&jobs.inner), db.clone());
    let spawned = std::thread::Builder::new().name("ai-lease".into()).spawn(move || loop {
        std::thread::sleep(HEARTBEAT);
        let rows: Vec<i64> = table.lock().unwrap_or_else(|p| p.into_inner()).live.values().map(|(_, r)| *r).collect();
        if let Err(e) = db.with(|c| renew(c, &rows, crate::db::now())) {
            tracing::warn!("generate: could not renew the job leases: {e}");
        }
    });
    match spawned {
        Ok(_) => t.heartbeat = true,
        Err(e) => tracing::warn!("generate: no heartbeat thread: {e}"),
    }
}

// ---------- the rows ----------

/// What a job's row holds: enough to rebuild a generation's request after
/// a restart, never a key.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq)]
#[serde(default)]
pub struct Payload {
    pub job: String,
    pub course: String,
    pub user: i64,
    /// "generate", "revise" or "translate".
    pub mode: String,
    /// A generation whose key can be had again (Skrongle AI or a saved key):
    /// it goes on by itself after a restart.
    pub resume: bool,
    /// "hosted" or "own".
    pub key: String,
    pub provider: String,
    pub model: String,
    /// The effort a generation runs at (`Effort::id`), '' for none.
    pub effort: String,
    pub lessons: usize,
    /// "full", "preview" or "outline" (#1119); rows from before it are full.
    #[serde(default)]
    pub depth: String,
    pub subject: String,
    pub instructions: String,
    pub title: Option<String>,
    pub named: Vec<String>,
}

/// The row for `req`. `saved_key`: its own key is the one saved on the profile.
pub fn payload(req: &Request, id: &str, saved_key: bool) -> Payload {
    let mode = match (req.changes_a_course(), &req.translate) {
        (true, _) => "revise",
        (false, None) => "generate",
        (false, Some(_)) => "translate",
    };
    let hosted = req.provider.source == KeySource::Hosted;
    Payload {
        job: id.to_string(),
        course: req.course_id.clone(),
        user: req.user_id,
        mode: mode.into(),
        resume: mode == "generate" && (hosted || saved_key),
        key: req.provider.source.as_str().into(),
        provider: req.provider.kind.id().into(),
        model: req.provider.model.clone(),
        effort: req.provider.effort.map(|e| e.id().to_string()).unwrap_or_default(),
        lessons: req.lesson_count,
        depth: req.depth.as_str().into(),
        subject: req.kind.as_str().into(),
        instructions: req.instructions.clone(),
        title: req.named.title.clone(),
        named: req.named.lessons.clone(),
    }
}

/// Add the row of a job this server starts or puts in line: held by this
/// server (running, leased) until [`release`] puts it in line.
pub fn hold(conn: &Connection, p: &Payload, now: i64) -> rusqlite::Result<i64> {
    let value = serde_json::to_value(p).unwrap_or_default();
    let id = crate::jobs::enqueue(conn, GENERATE.name, &value, now, now)?.ok_or_else(|| rusqlite::Error::InvalidParameterName(GENERATE.name.into()))?;
    conn.execute("UPDATE jobs SET state = 'running', locked_by = ?2, locked_until = ?3, attempts = 1 WHERE id = ?1", params![id, worker(), now + LEASE])?;
    Ok(id)
}

/// Put a held row in line for the runner (unless it ended already: cancelled).
pub fn release(conn: &Connection, row: i64, now: i64) -> rusqlite::Result<usize> {
    conn.execute(
        "UPDATE jobs SET state = 'queued', locked_by = NULL, locked_until = NULL, run_at = ?3, attempts = 0 WHERE id = ?1 AND state = 'running' AND locked_by = ?2",
        params![row, worker(), now],
    )
}

/// Record the end of a job's row: done, or failed with `error`.
pub fn settle(conn: &Connection, row: i64, error: Option<&str>, now: i64) -> rusqlite::Result<usize> {
    conn.execute(
        "UPDATE jobs SET state = CASE WHEN ?2 IS NULL THEN 'done' ELSE 'failed' END, last_error = ?2, locked_by = NULL, locked_until = NULL, finished_at = ?3
         WHERE id = ?1 AND state IN ('queued', 'running')",
        params![row, error, now],
    )
}

/// Renew the leases of running rows.
pub fn renew(conn: &Connection, rows: &[i64], now: i64) -> rusqlite::Result<usize> {
    let mut n = 0;
    for r in rows {
        n += conn.execute("UPDATE jobs SET locked_until = ?2 WHERE id = ?1 AND state = 'running'", params![r, now + LEASE])?;
    }
    Ok(n)
}

/// Unfinished rows: (row id, payload).
fn unfinished(conn: &Connection, states: &str) -> rusqlite::Result<Vec<(i64, Payload)>> {
    let mut stmt = conn.prepare(&format!("SELECT id, payload_json FROM jobs WHERE kind = ?1 AND state IN ({states}) ORDER BY id"))?;
    let rows = stmt.query_map([GENERATE.name], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    rows.map(|r| r.map(|(id, p)| (id, serde_json::from_str(&p).unwrap_or_default()))).collect()
}

/// Why a job did not go on after a restart.
pub const INTERRUPTED: &str = "Generation was interrupted when the server restarted. Resume it to write the remaining lessons.";

/// At boot, before `courses::fail_interrupted`: a generation that can go on
/// by itself waits in line again (one server per SQLite file, so a running
/// row at boot is from the server that stopped); any other unfinished job
/// stops. How many wait.
pub fn at_boot(conn: &Connection, now: i64) -> rusqlite::Result<usize> {
    let mut kept = 0;
    for (row, p) in unfinished(conn, "'queued', 'running'")? {
        if p.resume {
            conn.execute("UPDATE jobs SET state = 'queued', locked_by = NULL, locked_until = NULL, run_at = ?2 WHERE id = ?1", params![row, now])?;
            kept += 1;
        } else {
            settle(conn, row, Some(INTERRUPTED), now)?;
        }
    }
    Ok(kept)
}

/// After [`at_boot`]: the generations waiting again, back in the job table
/// so their learners' pages follow them. How many.
pub fn adopt(jobs: &Jobs, db: &Db) -> rusqlite::Result<usize> {
    let rows = db.with(|c| unfinished(c, "'queued'"))?;
    let mut table = jobs.lock();
    for (row, p) in &rows {
        let job = Job {
            user_id: p.user,
            course_id: p.course.clone(),
            status: Mutex::new(Status {
                id: p.job.clone(),
                phase: "Waiting".into(),
                log: Vec::new(),
                lessons_done: 0,
                lessons_total: p.lessons,
                done: false,
                error: None,
                course_id: Some(p.course.clone()),
                course_title: None,
                revision: false,
                adding: false,
                editing: false,
                lesson_title: None,
                translating: false,
                place: None,
                finished_at: None,
                notes: vec![],
            }),
            cancel: AtomicBool::new(false),
            deadline: Mutex::new(std::time::Instant::now()),
        };
        table.map.insert(p.job.clone(), Arc::new(job));
        table.waiting.insert(p.job.clone(), (p.user, *row));
    }
    Ok(rows.len())
}

// ---------- the runner's side ----------

/// May the runner take a waiting row now: is a slot free?
pub fn slot_free(jobs: &Jobs) -> bool {
    jobs.lock().live.len() < slots()
}

/// The runner's handler for [`GENERATE`]: start the claimed job's thread
/// (its request from memory, or rebuilt after a restart), or stop it.
pub fn handler(state: Arc<crate::AppState>) -> impl Fn(&Db, &Claimed) -> Result<Step, String> + Send + 'static {
    move |db, claimed| {
        let p: Payload = serde_json::from_str(&claimed.payload).map_err(|e| format!("payload: {e}"))?;
        let now = crate::db::now();
        let mut table = state.jobs.lock();
        if table.live.contains_key(&p.job) {
            // Still running here (a lease renewed late): keep it.
            drop(table);
            db.with(|c| renew(c, &[claimed.id], now)).map_err(|e| e.to_string())?;
            return Ok(Step::Handed(String::new()));
        }
        if table.live.len() >= slots() {
            // Taken since the runner looked: back in line.
            drop(table);
            db.with(|c| c.execute("UPDATE jobs SET state = 'queued', locked_by = NULL, locked_until = NULL, run_at = ?2 WHERE id = ?1", params![claimed.id, now + 5]))
                .map_err(|e| e.to_string())?;
            return Ok(Step::Handed(String::new()));
        }
        let job = table.map.get(&p.job).cloned();
        let cancelled = job.as_ref().is_some_and(|j| j.cancel.load(Ordering::SeqCst) || j.snapshot().done);
        table.waiting.remove(&p.job);
        let pending = table.pending.remove(&p.job);
        if cancelled {
            drop(table);
            db.with(|c| settle(c, claimed.id, Some("cancelled"), now)).map_err(|e| e.to_string())?;
            return Ok(Step::Handed(String::new()));
        }
        drop(table);
        let req = match pending {
            Some(r) => Ok(r),
            None if claimed.attempts > claimed.max_attempts => Err(INTERRUPTED.to_string()),
            None => rebuild(&state, db, &p),
        };
        let req = match req {
            Ok(r) => r,
            Err(e) => return give_up(&state, db, claimed.id, &p, job.as_ref(), &e),
        };
        let job = job.unwrap_or_else(|| Arc::new(new_job(&req, &p.job)));
        {
            let mut table = state.jobs.lock();
            table.map.insert(p.job.clone(), Arc::clone(&job));
            table.live.insert(p.job.clone(), (p.user, claimed.id));
        }
        tracing::info!("generate: job {} for {} left the line", p.job, p.course);
        launch(&state.jobs, job, db.clone(), Arc::clone(&state.courses), req, p.job.clone(), claimed.id)?;
        Ok(Step::Handed(format!("job {} started", p.job)))
    }
}

/// Stop a job that cannot go on: its course stopped (a generation keeps what
/// it wrote and offers *Resume*), its row failed.
fn give_up(state: &crate::AppState, db: &Db, row: i64, p: &Payload, job: Option<&Arc<Job>>, why: &str) -> Result<Step, String> {
    let current = db.with(|c| crate::courses::course(c, &p.course)).map_err(|e| e.to_string())?;
    let ours = current.as_ref().is_some_and(|c| c.job_id.as_deref() == Some(p.job.as_str()) && c.is_busy());
    db.with(|c| {
        if ours && p.mode == "revise" {
            crate::courses::finish_revision(c, &p.course)?;
        } else if ours {
            crate::courses::finish_generation(c, &p.course, Some(why))?;
        }
        settle(c, row, Some(why), crate::db::now()).map(drop)
    })
    .map_err(|e| e.to_string())?;
    state.meter.release(&p.job);
    if let Some(j) = job {
        j.update(|s| {
            s.done = true;
            s.finished_at = Some(crate::db::now());
            s.phase = "Stopped".into();
            s.error = Some(why.to_string());
        });
    }
    Ok(Step::Handed(format!("job {} stopped: {why}", p.job)))
}

/// A generation's request after a restart: the course row's wish, notes and
/// outline (so it resumes from the lessons already written), and the key
/// again, the server's or the one saved on the profile.
fn rebuild(state: &crate::AppState, db: &Db, p: &Payload) -> Result<Request, String> {
    if !p.resume {
        return Err(INTERRUPTED.into());
    }
    let row = db.with(|c| crate::courses::course(c, &p.course)).map_err(|e| e.to_string())?.ok_or("the course was deleted")?;
    if row.job_id.as_deref() != Some(p.job.as_str()) || !row.is_generating() {
        return Err("the course is no longer being generated".into());
    }
    let notes = db.with(|c| crate::courses::notes_of(c, &p.course)).map_err(|e| e.to_string())?;
    let kind = ProviderKind::parse(&p.provider).ok_or(INTERRUPTED)?;
    let mut provider = if p.key == KeySource::Hosted.as_str() {
        state.meter.provider("generate-lesson").ok_or(INTERRUPTED)?
    } else {
        let key = db.with(|c| crate::secrets::get(c, &state.vault, p.user, kind.secret())).map_err(|e| e.to_string())?.ok_or(INTERRUPTED)?;
        Provider { kind, base_url: kind.default_base_url(), api_key: key, model: p.model.clone(), source: KeySource::Own, effort: None }
    };
    provider.effort = super::Effort::parse(&p.effort);
    Ok(Request {
        user_id: p.user,
        provider,
        course_id: p.course.clone(),
        wish: row.brief.clone(),
        lesson_count: p.lessons,
        depth: Depth::parse(&p.depth),
        notes,
        kind: SubjectKind::parse(&p.subject),
        instructions: p.instructions.clone(),
        resume_outline: row.outline.clone(),
        revise: None,
        add_lesson: None,
        edit: None,
        meter: Some(Arc::clone(&state.meter)),
        named: crate::courses::Draft { title: p.title.clone(), lessons: p.named.clone() },
        translate: None,
    })
}

// ---------- the routes' side ----------

/// Take a job for a route: its row, then [`start`]. `saved_key`: the
/// learner's own key is the one saved on the profile (not one pasted into
/// the form), so a generation can go on by itself after a restart.
pub async fn submit(state: &crate::AppState, req: Request, id: String, saved_key: bool) -> Result<String, String> {
    let p = payload(&req, &id, saved_key);
    let now = crate::db::now();
    let row = state.db.call(move |c| hold(c, &p, now)).await.map_err(|e| e.to_string())?;
    match start(&state.jobs, state.db.clone(), Arc::clone(&state.courses), req, id.clone(), row) {
        Ok(Started::Now) => {}
        Ok(Started::Waiting) => {
            state.db.call(move |c| release(c, row, now)).await.map_err(|e| e.to_string())?;
            state.queue.notify();
            tracing::info!("generate: job {id} waits in line");
        }
        Err(e) => {
            let why = e.clone();
            state.db.call(move |c| settle(c, row, Some(&why), now)).await.map_err(|e| e.to_string())?;
            return Err(e);
        }
    }
    Ok(id)
}

/// Cancel: a waiting job leaves the line and stops at once; a running one
/// stops after its current request.
pub async fn cancel(state: &crate::AppState, job: &Arc<Job>) -> Result<(), crate::error::AppError> {
    let id = job.snapshot().id;
    let waited = {
        let mut t = state.jobs.lock();
        let w = t.waiting.remove(&id);
        if w.is_some() {
            t.pending.remove(&id);
            job.cancel.store(true, Ordering::SeqCst);
        }
        w
    };
    let Some((_, row)) = waited else {
        // Running, or starting: it stops before its next request.
        job.cancel();
        return Ok(());
    };
    let msg = job.check_cancelled().err().unwrap_or_default();
    state.meter.release(&id);
    let (cid, revision, why) = (job.course_id.clone(), job.snapshot().revision, msg.clone());
    state
        .db
        .call(move |c| {
            if revision {
                crate::courses::finish_revision(c, &cid)?;
            } else {
                crate::courses::finish_generation(c, &cid, Some(&why))?;
            }
            settle(c, row, Some(&why), crate::db::now()).map(drop)
        })
        .await?;
    job.update(|s| {
        s.done = true;
        s.finished_at = Some(crate::db::now());
        s.phase = "Stopped".into();
        s.error = Some(msg);
    });
    Ok(())
}
