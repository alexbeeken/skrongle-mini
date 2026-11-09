//! The course registry: built-in courses under `data/courses/<id>/`, checked
//! at boot (the seed course kept built, #5) and built on first use (#32),
//! plus courses people build in the app, stored in SQLite as the same files
//! (`courses` + `course_files`) and built on first use too.
//! A build on a request runs on an OS thread of its own (`off_worker`), never
//! on the one async worker.
//!
//! A *project* is a (user, course) pair: opening a course records it in
//! `projects` and makes it the user's active course (`users.active_course`).
//! Every page of the learning UI works on the active course.

use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::content::{ContentError, ContentStore, CourseFile, Manifest, Unit, MANIFEST_FILE};
use crate::db::{now, Db};
use crate::error::AppError;

/// Largest file accepted from the editor / upload (1 MiB).
pub const MAX_FILE_BYTES: usize = 1024 * 1024;
/// Files per course.
pub const MAX_FILES: usize = 400;
/// All files of one course together (paths and contents, in bytes): 8 MiB,
/// twice the Spanish course (3.7 MB), the biggest real one (#366).
pub const MAX_COURSE_BYTES: usize = 8 * 1024 * 1024;
/// All files of all the courses one account owns together: 32 MiB.
pub const MAX_ACCOUNT_BYTES: usize = 32 * 1024 * 1024;
/// Courses one account may own.
pub const MAX_COURSES_PER_USER: usize = 50;
/// How many built courses (built-in and user-made together) the in-memory
/// cache keeps: past it the least recently used one is dropped and rebuilt
/// on its next use (#33). Equal to `caps::MAX_ACCOUNTS`: each account has one
/// active course, so this is the most anyone can be using.
pub const MAX_STORES: usize = crate::caps::MAX_ACCOUNTS;

/// What one account may store in courses. `Default` gives the constants
/// above; the server reads `SKRONGLE_COURSES_PER_USER`, `SKRONGLE_COURSE_BYTES` and
/// `SKRONGLE_COURSE_BYTES_PER_USER` (docs/self-hosting.md). Every write of
/// course files checks `room_for` in the same `Db::call` that writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_courses: usize,
    pub course_bytes: usize,
    pub account_bytes: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits { max_courses: MAX_COURSES_PER_USER, course_bytes: MAX_COURSE_BYTES, account_bytes: MAX_ACCOUNT_BYTES }
    }
}

/// A cached course: the store and when it was last used (a counter), for
/// the cache's LRU eviction.
struct Cached {
    store: Arc<ContentStore>,
    used: u64,
}

/// The built courses, keyed by course id, at most `max_entries` of them,
/// least recently used out first.
struct Cache {
    entries: HashMap<String, Cached>,
    tick: u64,
    max_entries: usize,
}

impl Default for Cache {
    fn default() -> Cache {
        Cache { entries: HashMap::new(), tick: 0, max_entries: MAX_STORES }
    }
}

impl Cache {
    fn get(&mut self, id: &str) -> Option<Arc<ContentStore>> {
        self.tick += 1;
        let tick = self.tick;
        self.entries.get_mut(id).map(|e| {
            e.used = tick;
            Arc::clone(&e.store)
        })
    }

    /// Keep `store`, then drop the least recently used courses until the
    /// cache holds `max_entries` again. The store just put in always stays.
    fn insert(&mut self, id: &str, store: Arc<ContentStore>) {
        self.tick += 1;
        let key = id.to_string();
        self.entries.insert(key.clone(), Cached { store, used: self.tick });
        while self.entries.len() > self.max_entries.max(1) {
            let oldest = self.entries.iter().filter(|(k, _)| **k != key).min_by_key(|(_, e)| e.used).map(|(k, _)| k.clone());
            match oldest {
                Some(k) => self.entries.remove(&k),
                None => break,
            };
        }
    }

    fn remove(&mut self, id: &str) {
        self.entries.remove(id);
    }
}

/// The built-in course boot keeps built (and the dev seed opens), when it ships.
pub const SEED_COURSE: &str = "spanish";

/// Courses for automated tests only, under `<data>/test-courses/<id>/`: loaded
/// as built-in courses when `TEST_COURSE=1` in a dev build, never otherwise. The test course
/// there uses every feature a course file can switch on (see its AUTHORING.md).
pub const TEST_COURSES_DIR: &str = "test-courses";

/// What the registry keeps of a built-in course while it is not built
/// (#32): its manifest and the numbers its card, the landing page and the
/// settings wizard show. The lessons are built on first use (`Courses::get`).
pub struct BuiltinCourse {
    manifest: Manifest,
    /// `ContentStore::totals`: lessons, questions, terms.
    totals: (usize, usize, usize),
    /// The levels a learner may start at (`ContentStore::start_units`), each
    /// with the number of lessons in it.
    levels: Vec<(usize, usize)>,
    /// `placement::offered`.
    placement: bool,
}

impl BuiltinCourse {
    fn of(store: &ContentStore) -> BuiltinCourse {
        BuiltinCourse {
            manifest: store.manifest().clone(),
            totals: store.totals(),
            levels: store.start_units().into_iter().map(|u| (u, store.lessons_in(u).count())).collect(),
            placement: crate::placement::offered(store),
        }
    }

    pub fn id(&self) -> &str {
        &self.manifest.id
    }

    pub fn title(&self) -> &str {
        &self.manifest.title
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    pub fn units(&self) -> &[Unit] {
        &self.manifest.units
    }

    pub fn totals(&self) -> (usize, usize, usize) {
        self.totals
    }

    /// `ContentStore::start_units` with each level's lesson count.
    pub fn levels(&self) -> &[(usize, usize)] {
        &self.levels
    }

    /// `ContentStore::start_index`.
    pub fn start_index(&self, id: &str) -> usize {
        self.manifest.unit_index(id).filter(|u| self.levels.iter().any(|(l, _)| l == u)).unwrap_or(0)
    }

    /// `placement::offered`: the course has a placement test to take.
    pub fn placement_offered(&self) -> bool {
        self.placement
    }
}

/// Called on the build thread before a built-in course is built again
/// (tests: a slow or a failing build).
type BuildHook = Arc<dyn Fn(&str) + Send + Sync>;

pub struct Courses {
    builtin: Vec<BuiltinCourse>,
    /// Where each built-in course was loaded from, to build it on demand
    /// (and for `builtin_files`).
    builtin_dirs: HashMap<String, PathBuf>,
    /// Cache of the courses already built: built-in ones from their folder
    /// and user-made ones from the database, at most `MAX_STORES` together
    /// (least recently used out first, #33). Shared with the build thread,
    /// which puts what it built in itself.
    cache: Arc<Mutex<Cache>>,
    /// Held while a built-in course is built on a request, so two first
    /// requests for it wait for one build instead of making two. An async
    /// lock: a request waiting on it leaves the worker to the others. The
    /// build thread holds it and lets it go when it ends, panic or not.
    building: Arc<tokio::sync::Mutex<()>>,
    /// Built-in courses built after boot (`builtin_store`, `get`).
    rebuilds: Arc<AtomicUsize>,
    hook: Option<BuildHook>,
    /// What one account may store in courses.
    limits: Limits,
}

/// Build a built-in course from its folder, counted in `rebuilds`.
fn rebuild(dir: &Path, id: &str, rebuilds: &AtomicUsize, hook: Option<&BuildHook>) -> Result<ContentStore, ContentError> {
    if let Some(hook) = hook {
        hook(id);
    }
    rebuilds.fetch_add(1, Ordering::Relaxed);
    ContentStore::load_dir(dir)
}

/// Run `f` on an OS thread of its own and wait for it without holding the
/// async worker (a course build is up to 2 s on the Pi; the Tokio blocking
/// pool has two threads and is `Db::call`'s). `None` when the thread could
/// not start or `f` panicked.
async fn off_worker<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let spawned = std::thread::Builder::new().name("course-build".into()).spawn(move || {
        let _ = tx.send(f());
    });
    if let Err(e) = spawned {
        tracing::warn!("no thread to build a course on: {e}");
        return None;
    }
    rx.await.ok()
}

impl Courses {
    /// Check every `data/courses/*/course.json`. An empty or missing directory
    /// is fine (the app then only offers "build your own"). Each course is
    /// built once to validate it. One is kept in the cache, so the first
    /// request for it builds nothing (#5): `SEED_COURSE`, else the first.
    /// The others are dropped, only their `BuiltinCourse` kept: keeping both
    /// shipped courses built adds about 4 MB of RSS, too much on a Pi for
    /// a course nobody may open.
    /// `get` builds one on first use, off the worker (#32).
    pub fn load_builtin(data_dir: &Path) -> Result<Courses, ContentError> {
        Courses::load(data_dir, false)
    }

    /// `load_builtin`, plus the test courses under `TEST_COURSES_DIR` when
    /// `with_test_courses` (`TEST_COURSE=1`, dev builds). They come after the real ones.
    pub fn load(data_dir: &Path, with_test_courses: bool) -> Result<Courses, ContentError> {
        let mut roots = vec![data_dir.join("courses")];
        if with_test_courses {
            roots.push(data_dir.join(TEST_COURSES_DIR));
        }
        let mut builtin = Vec::new();
        let mut builtin_dirs = HashMap::new();
        // The one course boot keeps built: the seed course, else the first.
        // Decided as each is built, so no other one outlives its check.
        let seed_shipped = roots.iter().any(|r| r.join(SEED_COURSE).join(MANIFEST_FILE).is_file());
        let mut kept: Option<ContentStore> = None;
        for root in roots.into_iter().filter(|r| r.is_dir()) {
            let mut dirs: Vec<_> = std::fs::read_dir(&root)
                .map_err(|e| ContentError::Io(root.clone(), e))?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.join(MANIFEST_FILE).is_file())
                .collect();
            dirs.sort();
            for dir in dirs {
                let store = ContentStore::load_dir(&dir)?;
                let expected = dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if store.id() != expected {
                    return Err(ContentError::Invalid(format!(
                        "{}: course id '{}' must match its folder name '{}'",
                        dir.join(MANIFEST_FILE).display(),
                        store.id(),
                        expected
                    )));
                }
                if builtin_dirs.contains_key(store.id()) {
                    return Err(ContentError::Duplicate(format!("course id '{}' ({})", store.id(), dir.display())));
                }
                builtin_dirs.insert(store.id().to_string(), dir);
                builtin.push(BuiltinCourse::of(&store));
                let keep = if seed_shipped { store.id() == SEED_COURSE } else { kept.is_none() };
                if keep {
                    kept = Some(store);
                }
            }
        }
        let courses = Courses::with_builtin(builtin, builtin_dirs);
        if let Some(store) = kept {
            courses.lock().insert(&store.id().to_string(), Arc::new(store));
        }
        Ok(courses)
    }

    fn with_builtin(builtin: Vec<BuiltinCourse>, builtin_dirs: HashMap<String, PathBuf>) -> Courses {
        Courses {
            builtin,
            builtin_dirs,
            cache: Arc::default(),
            building: Arc::default(),
            rebuilds: Arc::default(),
            hook: None,
            limits: Limits::default(),
        }
    }

    #[cfg(test)]
    pub fn empty() -> Courses {
        Courses::with_builtin(Vec::new(), HashMap::new())
    }

    /// The same registry with `hook` run before each rebuild of a built-in.
    #[cfg(test)]
    pub fn with_build_hook(mut self, hook: impl Fn(&str) + Send + Sync + 'static) -> Courses {
        self.hook = Some(Arc::new(hook));
        self
    }

    /// How many times a built-in course was built since boot.
    pub fn rebuilds(&self) -> usize {
        self.rebuilds.load(Ordering::Relaxed)
    }

    /// The same registry with other storage limits (from the environment).
    pub fn with_limits(mut self, limits: Limits) -> Courses {
        self.limits = limits;
        self
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }

    /// The built-in courses, as their summaries; `builtin_store` (or `get`)
    /// for the lessons.
    pub fn builtin(&self) -> &[BuiltinCourse] {
        &self.builtin
    }

    pub fn builtin_by_id(&self, id: &str) -> Option<&BuiltinCourse> {
        self.builtin.iter().find(|c| c.id() == id)
    }

    /// A built-in course built, from the cache or else from its folder, on
    /// the calling thread: for start-up, commands and tests, never on a
    /// request (`get` builds off the worker). `None` for any other id, or
    /// when its files no longer load (they did at boot).
    pub fn builtin_store(&self, id: &str) -> Option<Arc<ContentStore>> {
        let dir = self.builtin_dirs.get(id)?;
        if let Some(c) = self.lock().get(id) {
            return Some(c);
        }
        match rebuild(dir, id, &self.rebuilds, self.hook.as_ref()) {
            Ok(store) => {
                let store = Arc::new(store);
                self.lock().insert(id, Arc::clone(&store));
                Some(store)
            }
            Err(e) => {
                tracing::warn!("built-in course {id} does not load: {e}");
                None
            }
        }
    }

    /// `builtin_store` for a request: a build runs on its own thread under
    /// `building`, so a second request for the same course waits for it
    /// and every other request is served meanwhile. The thread puts the
    /// course in the cache itself, so a request that gave up waiting does
    /// not waste the build.
    async fn builtin_store_off_worker(&self, id: &str) -> Option<Arc<ContentStore>> {
        let dir = self.builtin_dirs.get(id)?.clone();
        if let Some(c) = self.lock().get(id) {
            return Some(c);
        }
        let one = Arc::clone(&self.building).lock_owned().await;
        if let Some(c) = self.lock().get(id) {
            return Some(c);
        }
        let (cache, rebuilds, hook, key) = (Arc::clone(&self.cache), Arc::clone(&self.rebuilds), self.hook.clone(), id.to_string());
        let built = off_worker(move || {
            // `one` moves here: dropped when this ends or unwinds.
            let _one = one;
            let store = Arc::new(rebuild(&dir, &key, &rebuilds, hook.as_ref())?);
            cache.lock().unwrap_or_else(|p| p.into_inner()).insert(&key, Arc::clone(&store));
            Ok::<_, ContentError>(store)
        })
        .await;
        match built {
            Some(Ok(store)) => Some(store),
            Some(Err(e)) => {
                tracing::warn!("built-in course {id} does not load: {e}");
                None
            }
            None => {
                tracing::warn!("built-in course {id} does not load: its build stopped");
                None
            }
        }
    }

    /// The files of a built-in course, read from its folder (`None` for any
    /// other id): what a copy of it starts from (#629).
    pub fn builtin_files(&self, id: &str) -> Option<Result<Vec<CourseFile>, ContentError>> {
        self.builtin_dirs.get(id).map(|dir| crate::content::read_course_dir(dir))
    }

    pub fn is_builtin(&self, id: &str) -> bool {
        self.builtin_dirs.contains_key(id)
    }

    /// Resolve a course by id: cached, or built from its folder (built-in)
    /// or from the database.
    /// `Ok(None)` when it does not exist or does not currently load, which
    /// includes a manifest whose id is not the course's own (#380: the
    /// learning tables key on the manifest id, so such a course would write
    /// under another course's id).
    pub async fn get(&self, db: &Db, id: &str) -> Result<Option<Arc<ContentStore>>, AppError> {
        if self.is_builtin(id) {
            return Ok(self.builtin_store_off_worker(id).await);
        }
        if let Some(c) = self.lock().get(id) {
            return Ok(Some(c));
        }
        let id_owned = id.to_string();
        let files = db.call(move |c| load_ready_files(c, &id_owned)).await?;
        if files.is_empty() {
            return Ok(None);
        }
        let Some(built) = off_worker(move || ContentStore::from_files(&files)).await else {
            tracing::warn!("course {id} does not load: its build stopped");
            return Ok(None);
        };
        match built {
            Ok(store) if store.id() != id => {
                tracing::warn!("course {id} does not load: its manifest says id {:?}", store.id());
                Ok(None)
            }
            Ok(store) => {
                let store = Arc::new(store);
                self.lock().insert(id, Arc::clone(&store));
                Ok(Some(store))
            }
            Err(e) => {
                tracing::warn!("course {id} does not load: {e}");
                Ok(None)
            }
        }
    }

    /// Forget a cached course (after an edit).
    pub fn invalidate(&self, id: &str) {
        self.lock().remove(id);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Cache> {
        self.cache.lock().unwrap_or_else(|p| p.into_inner())
    }
}

// ---------- database: user-made courses ----------

/// Where a user-made course is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CourseState {
    /// An AI job is writing it; it cannot be opened or edited yet.
    Generating,
    /// An AI job is revising it from feedback: it opens as it was, but its
    /// files cannot change until the job swaps in the new ones (or gives up).
    Revising,
    Ready,
    /// Generation stopped (an error, a cancel, a restart); it can resume.
    Failed,
}

impl CourseState {
    pub fn as_str(self) -> &'static str {
        match self {
            CourseState::Generating => "generating",
            CourseState::Revising => "revising",
            CourseState::Ready => "ready",
            CourseState::Failed => "failed",
        }
    }

    fn parse(s: &str) -> CourseState {
        match s {
            "generating" => CourseState::Generating,
            "revising" => CourseState::Revising,
            "failed" => CourseState::Failed,
            _ => CourseState::Ready,
        }
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // created_at/updated_at are read by the editor page in templates to come
pub struct CourseRow {
    pub id: String,
    pub owner_id: i64,
    pub title: String,
    pub created_at: i64,
    pub updated_at: i64,
    /// Last validation error, if the course does not currently load.
    pub error: Option<String>,
    pub state: CourseState,
    /// 'hand', 'ai', or 'translate' (a copy being or once translated, #630).
    pub source: String,
    /// The generation job writing it (in memory; gone after a restart).
    pub job_id: Option<String>,
    /// What the learner asked the AI for.
    pub brief: String,
    /// The AI's plan (JSON), kept so a failed run can resume.
    pub outline: Option<String>,
}

impl CourseRow {
    pub fn is_generating(&self) -> bool {
        self.state == CourseState::Generating
    }

    /// An AI job holds the course (writing or revising it): no edits now.
    pub fn is_busy(&self) -> bool {
        matches!(self.state, CourseState::Generating | CourseState::Revising)
    }

    /// A course that loads and nothing is running on, however it was made:
    /// it can be revised.
    pub fn can_revise(&self) -> bool {
        self.state == CourseState::Ready && self.error.is_none()
    }
}

fn course_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<CourseRow> {
    Ok(CourseRow {
        id: r.get(0)?,
        owner_id: r.get(1)?,
        title: r.get(2)?,
        created_at: r.get(3)?,
        updated_at: r.get(4)?,
        error: r.get(5)?,
        state: CourseState::parse(&r.get::<_, String>(6)?),
        source: r.get(7)?,
        job_id: r.get(8)?,
        brief: r.get(9)?,
        outline: r.get(10)?,
    })
}

const COURSE_COLS: &str = "id, owner_id, title, created_at, updated_at, error, state, source, job_id, brief, outline";

/// Register a course an AI job is about to write: no files yet, greyed out
/// in the course list until the job finishes. (The app makes a draft with
/// "Create a course" and hands it over with `restart_generation`.)
#[cfg(test)]
pub fn create_generating(conn: &Connection, owner_id: i64, id: &str, title: &str, brief: &str, notes: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO courses (id, owner_id, title, created_at, updated_at, state, source, brief, notes)
         VALUES (?1, ?2, ?3, ?4, ?4, 'generating', 'ai', ?5, ?6)",
        params![id, owner_id, title, now(), brief, notes],
    )?;
    Ok(())
}

/// Replace what a course keeps in `notes` (a translation's plan, #630).
pub fn set_notes(conn: &Connection, id: &str, notes: &str) -> rusqlite::Result<()> {
    conn.execute("UPDATE courses SET notes = ?2 WHERE id = ?1", params![id, notes])?;
    Ok(())
}

/// Hand a copy just made to the translation job `job_id` (#630): it is
/// 'generating' (nobody opens it half translated) and its notes hold the
/// job's plan, for a resume.
pub fn start_translation(conn: &Connection, id: &str, job_id: &str, plan: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE courses SET state = 'generating', source = 'translate', job_id = ?2, notes = ?3, error = NULL, updated_at = ?4 WHERE id = ?1",
        params![id, job_id, plan, now()],
    )?;
    Ok(())
}

/// The material a generated course was built from.
pub fn notes_of(conn: &Connection, id: &str) -> rusqlite::Result<String> {
    conn.query_row("SELECT notes FROM courses WHERE id = ?1", [id], |r| r.get(0))
}

/// Mark a course as being (re)generated by `job_id`.
pub fn set_generating(conn: &Connection, id: &str, job_id: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE courses SET state = 'generating', job_id = ?2, error = NULL, updated_at = ?3 WHERE id = ?1",
        params![id, job_id, now()],
    )?;
    Ok(())
}

/// Mark a course as being revised by `job_id`. False when it is not a ready
/// course any more (another request got there first), so two revisions can't
/// run on one course.
pub fn set_revising(conn: &Connection, id: &str, job_id: &str) -> rusqlite::Result<bool> {
    let n = conn.execute("UPDATE courses SET state = 'revising', job_id = ?2 WHERE id = ?1 AND state = 'ready'", params![id, job_id])?;
    Ok(n == 1)
}

/// The end of a revision job, whatever its outcome: the course is ready
/// again (its files are the new set, or untouched).
pub fn finish_revision(conn: &Connection, id: &str) -> rusqlite::Result<()> {
    conn.execute("UPDATE courses SET state = 'ready', updated_at = ?2 WHERE id = ?1 AND state = 'revising'", params![id, now()])?;
    Ok(())
}

/// How many changes back Undo can go.
pub const UNDO_STEPS: i64 = 10;

// Undo steps are the `course_undo` rows with `seq > 0` (newest = highest).
// Redo steps share the table with `seq < 0` (newest = lowest): Undo files
// the course as it was before the undo there, Redo takes it back. Any new
// change clears them, as in an editor. No schema of their own (#231).

/// Drop the course's Redo steps: a new change was made.
fn clear_redo_of(tx: &rusqlite::Transaction, id: &str) -> rusqlite::Result<()> {
    tx.execute("DELETE FROM course_undo_files WHERE course_id = ?1 AND seq < 0", [id])?;
    tx.execute("DELETE FROM course_undo WHERE course_id = ?1 AND seq < 0", [id])?;
    Ok(())
}

/// The seq a new Undo step gets.
fn next_undo_seq(tx: &rusqlite::Transaction, id: &str) -> rusqlite::Result<i64> {
    tx.query_row("SELECT COALESCE(MAX(seq), 0) + 1 FROM course_undo WHERE course_id = ?1 AND seq > 0", [id], |r| r.get(0))
}

/// File the course's current files, title and outline as step `seq` with `label`.
fn file_current(tx: &rusqlite::Transaction, id: &str, seq: i64, label: &str, session: Option<&str>) -> rusqlite::Result<()> {
    let t = now();
    tx.execute(
        "INSERT INTO course_undo (course_id, seq, title, outline, label, created_at, session, started_at)
         SELECT id, ?2, title, outline, ?3, ?4, ?5, ?4 FROM courses WHERE id = ?1",
        params![id, seq, label, t, session],
    )?;
    tx.execute("INSERT INTO course_undo_files (course_id, seq, path, content) SELECT course_id, ?2, path, content FROM course_files WHERE course_id = ?1", params![id, seq])?;
    Ok(())
}

/// Put step `seq`'s files, title and outline back as the course, and drop the step.
fn restore_step(tx: &rusqlite::Transaction, id: &str, seq: i64, title: &str, outline: Option<&str>) -> rusqlite::Result<()> {
    tx.execute("DELETE FROM course_files WHERE course_id = ?1", [id])?;
    tx.execute(
        "INSERT INTO course_files (course_id, path, content, updated_at) SELECT course_id, path, content, ?3 FROM course_undo_files WHERE course_id = ?1 AND seq = ?2",
        params![id, seq, now()],
    )?;
    tx.execute("UPDATE courses SET title = ?2, outline = ?3, updated_at = ?4 WHERE id = ?1", params![id, title, outline, now()])?;
    tx.execute("DELETE FROM course_undo_files WHERE course_id = ?1 AND seq = ?2", params![id, seq])?;
    tx.execute("DELETE FROM course_undo WHERE course_id = ?1 AND seq = ?2", params![id, seq])?;
    Ok(())
}

/// Drop the Undo steps older than the newest `UNDO_STEPS`.
fn trim_undo(tx: &rusqlite::Transaction, id: &str, newest: i64) -> rusqlite::Result<()> {
    tx.execute("DELETE FROM course_undo_files WHERE course_id = ?1 AND seq > 0 AND seq <= ?2", params![id, newest - UNDO_STEPS])?;
    tx.execute("DELETE FROM course_undo WHERE course_id = ?1 AND seq > 0 AND seq <= ?2", params![id, newest - UNDO_STEPS])?;
    Ok(())
}

/// Swap in a revision's files in one transaction, keeping the current set
/// (with its title and outline) as the newest Undo step. `label` says what
/// the change is ("Removed “Simplifying”"); the oldest steps beyond
/// `UNDO_STEPS` are dropped.
pub fn commit_revision(conn: &mut Connection, id: &str, files: &[CourseFile], title: &str, outline: Option<&str>, label: &str) -> rusqlite::Result<()> {
    commit_revision_in(conn, id, files, title, outline, label, None)
}

/// `commit_revision` for a save of the editor page `session` (autosave): its
/// later saves join this step (`amend_session_step`).
pub fn commit_revision_in(
    conn: &mut Connection,
    id: &str,
    files: &[CourseFile],
    title: &str,
    outline: Option<&str>,
    label: &str,
    session: Option<&str>,
) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    let t = now();
    let seq = next_undo_seq(&tx, id)?;
    clear_redo_of(&tx, id)?;
    file_current(&tx, id, seq, label, session)?;
    trim_undo(&tx, id, seq)?;
    tx.execute("DELETE FROM course_files WHERE course_id = ?1", [id])?;
    for f in files {
        tx.execute("INSERT INTO course_files (course_id, path, content, updated_at) VALUES (?1, ?2, ?3, ?4)", params![id, f.path, f.content, t])?;
    }
    tx.execute("UPDATE courses SET title = ?2, outline = ?3, updated_at = ?4 WHERE id = ?1", params![id, title, outline, t])?;
    tx.commit()
}

/// How long the saves of one editor page keep joining the same Undo step.
pub const SESSION_STEP_SECS: i64 = 600;

/// A save of the editor page `session` that joins the newest Undo step,
/// when that step is this session's and began less than
/// `SESSION_STEP_SECS` ago: the files are swapped in without a new step, so
/// one Undo takes back the whole stretch of editing. `label` gets the files
/// from before the step and names the change as a whole. False (nothing
/// written) when the newest step is not this session's.
pub fn amend_session_step(
    conn: &mut Connection,
    id: &str,
    session: &str,
    files: &[CourseFile],
    outline: Option<&str>,
    label: impl FnOnce(&[CourseFile]) -> String,
) -> rusqlite::Result<bool> {
    let tx = conn.transaction()?;
    let t = now();
    let newest: Option<(i64, Option<String>, Option<i64>)> = tx
        .query_row("SELECT seq, session, started_at FROM course_undo WHERE course_id = ?1 AND seq > 0 ORDER BY seq DESC LIMIT 1", [id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .optional()?;
    let Some((seq, Some(s), Some(started))) = newest else { return Ok(false) };
    if s != session || t - started >= SESSION_STEP_SECS {
        return Ok(false);
    }
    let before: Vec<CourseFile> = {
        let mut stmt = tx.prepare("SELECT path, content FROM course_undo_files WHERE course_id = ?1 AND seq = ?2 ORDER BY path")?;
        let rows = stmt.query_map(params![id, seq], |r| Ok(CourseFile { path: r.get(0)?, content: r.get(1)? }))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    tx.execute("UPDATE course_undo SET label = ?3, created_at = ?4 WHERE course_id = ?1 AND seq = ?2", params![id, seq, label(&before), t])?;
    clear_redo_of(&tx, id)?;
    tx.execute("DELETE FROM course_files WHERE course_id = ?1", [id])?;
    for f in files {
        tx.execute("INSERT INTO course_files (course_id, path, content, updated_at) VALUES (?1, ?2, ?3, ?4)", params![id, f.path, f.content, t])?;
    }
    tx.execute("UPDATE courses SET outline = ?2, updated_at = ?3 WHERE id = ?1", params![id, outline, t])?;
    tx.commit()?;
    Ok(true)
}

/// A short fingerprint of a course's files (FNV-1a over paths and contents),
/// so the editor can tell whether the course changed since it loaded it (in
/// another tab, or by an AI job).
pub fn version_of(files: &[CourseFile]) -> String {
    let mut sorted: Vec<&CourseFile> = files.iter().collect();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for f in sorted {
        for b in f.path.bytes().chain([0u8]).chain(f.content.bytes()).chain([0u8]) {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    format!("{h:016x}")
}

/// The newest change that can be undone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoStep {
    pub label: String,
    pub created_at: i64,
    /// Steps in all, this one included.
    pub steps: usize,
}

pub fn undo_step(conn: &Connection, id: &str) -> rusqlite::Result<Option<UndoStep>> {
    conn.query_row(
        "SELECT label, created_at, (SELECT COUNT(*) FROM course_undo WHERE course_id = ?1 AND seq > 0) FROM course_undo WHERE course_id = ?1 AND seq > 0 ORDER BY seq DESC LIMIT 1",
        [id],
        |r| Ok(UndoStep { label: r.get(0)?, created_at: r.get(1)?, steps: r.get::<_, i64>(2)? as usize }),
    )
    .optional()
}

/// The newest undone change that Redo can put back.
pub fn redo_step(conn: &Connection, id: &str) -> rusqlite::Result<Option<UndoStep>> {
    conn.query_row(
        "SELECT label, created_at, (SELECT COUNT(*) FROM course_undo WHERE course_id = ?1 AND seq < 0) FROM course_undo WHERE course_id = ?1 AND seq < 0 ORDER BY seq ASC LIMIT 1",
        [id],
        |r| Ok(UndoStep { label: r.get(0)?, created_at: r.get(1)?, steps: r.get::<_, i64>(2)? as usize }),
    )
    .optional()
}

/// One row of `course_undo`: the step's sequence number, its files' title
/// and outline, and its label.
struct StepRow {
    seq: i64,
    title: String,
    outline: Option<String>,
    label: String,
}

/// A step's files, title, outline and label: the newest Undo step
/// (`redo` false) or Redo step (`redo` true).
fn top_step(tx: &rusqlite::Transaction, id: &str, redo: bool) -> rusqlite::Result<Option<StepRow>> {
    let sql = if redo {
        "SELECT seq, title, outline, label FROM course_undo WHERE course_id = ?1 AND seq < 0 ORDER BY seq ASC LIMIT 1"
    } else {
        "SELECT seq, title, outline, label FROM course_undo WHERE course_id = ?1 AND seq > 0 ORDER BY seq DESC LIMIT 1"
    };
    tx.query_row(sql, [id], |r| Ok(StepRow { seq: r.get(0)?, title: r.get(1)?, outline: r.get(2)?, label: r.get(3)? })).optional()
}

/// Put back the files from before the newest change. The course as it was
/// becomes a Redo step, so Redo can take the undo back. Returns what was
/// undone; `None` when there is nothing.
pub fn undo_revision(conn: &mut Connection, id: &str) -> rusqlite::Result<Option<String>> {
    let tx = conn.transaction()?;
    let Some(step) = top_step(&tx, id, false)? else { return Ok(None) };
    let redo: i64 = tx.query_row("SELECT COALESCE(MIN(seq), 0) - 1 FROM course_undo WHERE course_id = ?1 AND seq < 0", [id], |r| r.get(0))?;
    file_current(&tx, id, redo, &step.label, None)?;
    restore_step(&tx, id, step.seq, &step.title, step.outline.as_deref())?;
    tx.commit()?;
    Ok(Some(step.label))
}

/// Put back the newest change Undo took away. The course as it was becomes
/// an Undo step again (one no editor session joins). Returns what was
/// redone; `None` when there is nothing.
pub fn redo_revision(conn: &mut Connection, id: &str) -> rusqlite::Result<Option<String>> {
    let tx = conn.transaction()?;
    let Some(step) = top_step(&tx, id, true)? else { return Ok(None) };
    let undo = next_undo_seq(&tx, id)?;
    file_current(&tx, id, undo, &step.label, None)?;
    trim_undo(&tx, id, undo)?;
    restore_step(&tx, id, step.seq, &step.title, step.outline.as_deref())?;
    tx.commit()?;
    Ok(Some(step.label))
}

/// The latest AI run on a course, for "Review AI changes": the files it
/// changed, as they were before and as the AI wrote them.
#[derive(Debug, Clone)]
pub struct AiRun {
    pub label: String,
    pub created_at: i64,
    /// Sorted by path; `None` on a side: the file was added (or removed).
    pub files: Vec<AiRunFile>,
    /// What the learner left out of it in the review, as JSON
    /// (`review::Exclusions`); empty for nothing.
    pub excluded: String,
    /// The learner's edits of its lines, as JSON (`review::Edits`); empty for none.
    pub edits: String,
    /// What the AI said about each lesson it fixed or checked, as JSON
    /// (`ai::LessonNote`, #1238); empty for nothing.
    pub why: String,
}

#[derive(Debug, Clone)]
pub struct AiRunFile {
    pub path: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

/// Keep what an AI run changed, comparing `before` (the files when it
/// started) with the course's files now, with the run's `why` lines (JSON,
/// may be empty). A run that changed nothing leaves the previous one in
/// place. Returns whether a run was kept.
pub fn record_ai_run(conn: &mut Connection, id: &str, before: &[CourseFile], label: &str, why: &str) -> rusqlite::Result<bool> {
    let after = load_files(conn, id)?;
    let old: HashMap<&str, &str> = before.iter().map(|f| (f.path.as_str(), f.content.as_str())).collect();
    let new: HashMap<&str, &str> = after.iter().map(|f| (f.path.as_str(), f.content.as_str())).collect();
    let removed: Vec<&CourseFile> = before.iter().filter(|f| !new.contains_key(f.path.as_str())).collect();
    let changed: Vec<&CourseFile> = after.iter().filter(|f| old.get(f.path.as_str()) != Some(&f.content.as_str())).collect();
    if removed.is_empty() && changed.is_empty() {
        return Ok(false);
    }
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM course_ai_runs WHERE course_id = ?1", [id])?;
    tx.execute("INSERT INTO course_ai_runs (course_id, label, created_at, why) VALUES (?1, ?2, ?3, ?4)", params![id, label, now(), why])?;
    let put = |side: &str, path: &str, content: &str| {
        tx.execute("INSERT INTO course_ai_run_files (course_id, side, path, content) VALUES (?1, ?2, ?3, ?4)", params![id, side, path, content])
    };
    for f in &removed {
        put("before", &f.path, &f.content)?;
    }
    for f in &changed {
        if let Some(c) = old.get(f.path.as_str()) {
            put("before", &f.path, c)?;
        }
        put("after", &f.path, &f.content)?;
    }
    tx.commit()?;
    Ok(true)
}

/// When the latest AI run on a course was, if it had one.
pub fn ai_run_at(conn: &Connection, id: &str) -> rusqlite::Result<Option<i64>> {
    conn.query_row("SELECT created_at FROM course_ai_runs WHERE course_id = ?1", [id], |r| r.get(0)).optional()
}

pub fn ai_run(conn: &Connection, id: &str) -> rusqlite::Result<Option<AiRun>> {
    let head: Option<(String, i64, String, String, String)> = conn
        .query_row("SELECT label, created_at, excluded, edits, why FROM course_ai_runs WHERE course_id = ?1", [id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })
        .optional()?;
    let Some((label, created_at, excluded, edits, why)) = head else { return Ok(None) };
    let mut stmt = conn.prepare("SELECT side, path, content FROM course_ai_run_files WHERE course_id = ?1 ORDER BY path, side")?;
    let mut files: Vec<AiRunFile> = Vec::new();
    for row in stmt.query_map([id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))? {
        let (side, path, content) = row?;
        if files.last().map_or(true, |f| f.path != path) {
            files.push(AiRunFile { path, before: None, after: None });
        }
        let f = files.last_mut().expect("just pushed");
        if side == "before" {
            f.before = Some(content);
        } else {
            f.after = Some(content);
        }
    }
    Ok(Some(AiRun { label, created_at, files, excluded, edits, why }))
}

/// Keep what the learner left out of the latest AI run and how they edited
/// it (`review::Exclusions`, `review::Edits` as JSON).
pub fn set_ai_run_excluded(conn: &Connection, id: &str, excluded: &str, edits: &str) -> rusqlite::Result<()> {
    conn.execute("UPDATE course_ai_runs SET excluded = ?2, edits = ?3 WHERE course_id = ?1", params![id, excluded, edits])?;
    Ok(())
}

/// A short label quoting the learner's words.
pub fn quoted(prefix: &str, words: &str) -> String {
    let words = words.split_whitespace().collect::<Vec<_>>().join(" ");
    let short: String = words.chars().take(120).collect();
    let short = if words.chars().count() > 120 { short.trim_end().to_string() + "…" } else { short };
    format!("{prefix} \u{201c}{short}\u{201d}")
}

pub fn set_outline(conn: &Connection, id: &str, title: &str, outline: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE courses SET title = ?2, outline = ?3, updated_at = ?4 WHERE id = ?1",
        params![id, title, outline, now()],
    )?;
    Ok(())
}

/// The end of a generation job: ready, or failed with the reason.
pub fn finish_generation(conn: &Connection, id: &str, error: Option<&str>) -> rusqlite::Result<()> {
    let state = if error.is_some() { CourseState::Failed } else { CourseState::Ready };
    conn.execute(
        "UPDATE courses SET state = ?2, error = COALESCE(?3, error), updated_at = ?4 WHERE id = ?1",
        params![id, state.as_str(), error, now()],
    )?;
    Ok(())
}

/// At boot, after `ai::at_boot`: a course still 'generating' whose job does
/// not go on after the restart (#874) was interrupted by the restart. It
/// keeps what was written and can resume. A revision writes nothing until
/// it has finished, so an interrupted one leaves the course as it was.
pub fn fail_interrupted(conn: &Connection) -> rusqlite::Result<usize> {
    conn.execute("UPDATE courses SET state = 'ready' WHERE state = 'revising'", [])?;
    conn.execute(
        // `updated_at` starts the failed-course retention clock (jobs/retention.rs).
        "UPDATE courses SET state = 'failed', updated_at = ?1, error = ?2
         WHERE state = 'generating'
           AND id NOT IN (SELECT json_extract(payload_json, '$.course') FROM jobs WHERE kind = ?3 AND state IN ('queued', 'running'))",
        params![now(), crate::ai::INTERRUPTED, crate::ai::GENERATE.name],
    )
}

pub fn course(conn: &Connection, id: &str) -> rusqlite::Result<Option<CourseRow>> {
    conn.query_row(&format!("SELECT {COURSE_COLS} FROM courses WHERE id = ?1"), [id], course_row).optional()
}

pub fn courses_owned_by(conn: &Connection, user_id: i64) -> rusqlite::Result<Vec<CourseRow>> {
    let mut stmt = conn.prepare_cached(&format!("SELECT {COURSE_COLS} FROM courses WHERE owner_id = ?1 ORDER BY updated_at DESC"))?;
    let rows = stmt.query_map([user_id], course_row)?;
    rows.collect()
}

#[cfg(test)]
pub fn course_exists(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    conn.query_row("SELECT EXISTS(SELECT 1 FROM courses WHERE id = ?1)", [id], |r| r.get(0))
}

/// A short, URL-safe id from a title, with a random suffix so two people can
/// both build "Music theory".
pub fn new_course_id(title: &str) -> String {
    let mut slug: String = title
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .take(4)
        .collect::<Vec<_>>()
        .join("-");
    slug.truncate(32);
    if slug.is_empty() {
        slug = "course".into();
    }
    let token = crate::auth::generate_token();
    format!("{slug}-{}", &token[..6])
}

/// Create a course with its files. The manifest's id is rewritten to `id`.
pub fn create_course(conn: &mut Connection, owner_id: i64, id: &str, title: &str, files: &[CourseFile]) -> rusqlite::Result<()> {
    let t = now();
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO courses (id, owner_id, title, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)",
        params![id, owner_id, title, t],
    )?;
    for f in files {
        tx.execute(
            "INSERT INTO course_files (course_id, path, content, updated_at) VALUES (?1, ?2, ?3, ?4)",
            params![id, f.path, f.content, t],
        )?;
    }
    tx.commit()
}

pub fn delete_course(conn: &Connection, id: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM courses WHERE id = ?1", [id])?;
    conn.execute("DELETE FROM projects WHERE course_id = ?1", [id])?;
    conn.execute("UPDATE users SET active_course = NULL WHERE active_course = ?1", [id])?;
    // Progress rows stay (a course re-added under its id finds them), but
    // nobody is on this course any more (#38).
    crate::mastery::cache::forget_course(id);
    Ok(())
}

/// Record the outcome of the last validation and refresh the title from the manifest.
pub fn set_status(conn: &Connection, id: &str, title: Option<&str>, error: Option<&str>) -> rusqlite::Result<()> {
    match title {
        Some(t) => conn.execute(
            "UPDATE courses SET title = ?2, error = ?3, updated_at = ?4 WHERE id = ?1",
            params![id, t, error, now()],
        )?,
        None => conn.execute("UPDATE courses SET error = ?2, updated_at = ?3 WHERE id = ?1", params![id, error, now()])?,
    };
    Ok(())
}

pub fn load_files(conn: &Connection, course_id: &str) -> rusqlite::Result<Vec<CourseFile>> {
    let mut stmt = conn.prepare_cached("SELECT path, content FROM course_files WHERE course_id = ?1 ORDER BY path")?;
    let rows = stmt.query_map([course_id], |r| Ok(CourseFile { path: r.get(0)?, content: r.get(1)? }))?;
    rows.collect()
}

/// The files of a course that can be opened: not while it is being generated
/// (half-written), so the cache never holds a partial course.
pub fn load_ready_files(conn: &Connection, course_id: &str) -> rusqlite::Result<Vec<CourseFile>> {
    let state: Option<String> = conn.query_row("SELECT state FROM courses WHERE id = ?1", [course_id], |r| r.get(0)).optional()?;
    if state.as_deref() == Some("generating") {
        return Ok(Vec::new());
    }
    load_files(conn, course_id)
}

/// (path, bytes, updated_at) for the editor's file list.
pub fn list_files(conn: &Connection, course_id: &str) -> rusqlite::Result<Vec<(String, usize, i64)>> {
    let mut stmt = conn.prepare_cached("SELECT path, LENGTH(content), updated_at FROM course_files WHERE course_id = ?1 ORDER BY path")?;
    let rows = stmt.query_map([course_id], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? as usize, r.get(2)?)))?;
    rows.collect()
}

pub fn read_file(conn: &Connection, course_id: &str, path: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT content FROM course_files WHERE course_id = ?1 AND path = ?2", params![course_id, path], |r| r.get(0))
        .optional()
}

pub fn write_file(conn: &Connection, course_id: &str, path: &str, content: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO course_files (course_id, path, content, updated_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(course_id, path) DO UPDATE SET content = excluded.content, updated_at = excluded.updated_at",
        params![course_id, path, content, now()],
    )?;
    conn.execute("UPDATE courses SET updated_at = ?2 WHERE id = ?1", params![course_id, now()])?;
    Ok(())
}

pub fn delete_file(conn: &Connection, course_id: &str, path: &str) -> rusqlite::Result<bool> {
    let n = conn.execute("DELETE FROM course_files WHERE course_id = ?1 AND path = ?2", params![course_id, path])?;
    if n > 0 {
        // An edit: the nightly content check looks at it again (#1038).
        conn.execute("UPDATE courses SET updated_at = ?2 WHERE id = ?1", params![course_id, now()])?;
    }
    Ok(n > 0)
}

pub fn file_count(conn: &Connection, course_id: &str) -> rusqlite::Result<usize> {
    conn.query_row("SELECT COUNT(*) FROM course_files WHERE course_id = ?1", [course_id], |r| r.get::<_, i64>(0)).map(|n| n as usize)
}

/// The bytes a set of course files takes: paths and contents, as stored.
pub fn size_of(files: &[CourseFile]) -> usize {
    files.iter().map(|f| f.path.len() + f.content.len()).sum()
}

/// The bytes the course would take with `writes` stored (each replacing the
/// file at its path, or added).
pub fn size_after(conn: &Connection, course_id: &str, writes: &[CourseFile]) -> rusqlite::Result<usize> {
    let mut stmt = conn.prepare_cached("SELECT path, LENGTH(CAST(path AS BLOB)) + LENGTH(CAST(content AS BLOB)) FROM course_files WHERE course_id = ?1")?;
    let rows = stmt.query_map([course_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize)))?;
    let mut total = 0;
    for row in rows {
        let (path, bytes) = row?;
        if !writes.iter().any(|w| w.path == path) {
            total += bytes;
        }
    }
    Ok(total + size_of(writes))
}

/// The bytes the files of `owner`'s courses take, all but `except`'s.
fn account_bytes(conn: &Connection, owner: i64, except: &str) -> rusqlite::Result<usize> {
    conn.query_row(
        "SELECT COALESCE(SUM(LENGTH(CAST(f.path AS BLOB)) + LENGTH(CAST(f.content AS BLOB))), 0)
         FROM course_files f JOIN courses c ON c.id = f.course_id WHERE c.owner_id = ?1 AND c.id != ?2",
        params![owner, except],
        |r| r.get::<_, i64>(0),
    )
    .map(|n| n as usize)
}

/// Bytes as the refusals say them: "8 MB", "3.6 MB" (MiB, rounded up).
pub fn megabytes(bytes: usize) -> String {
    let tenths = (bytes * 10).div_ceil(1024 * 1024);
    if tenths % 10 == 0 {
        format!("{} MB", tenths / 10)
    } else {
        format!("{}.{} MB", tenths / 10, tenths % 10)
    }
}

/// Whether course `course_id` may take `new_size` bytes (`size_of` /
/// `size_after` of what is about to be stored): within the course limit and,
/// with its owner's other courses, the account limit. Call it in the same
/// `Db::call` as the write; `Some(problem)` names the limit and how to get
/// under it, and nothing may be written then.
pub fn room_for(conn: &Connection, course_id: &str, new_size: usize, limits: Limits) -> rusqlite::Result<Option<crate::problems::Problem>> {
    let owner: Option<i64> = conn.query_row("SELECT owner_id FROM courses WHERE id = ?1", [course_id], |r| r.get(0)).optional()?;
    room_in(conn, owner, course_id, new_size, limits)
}

/// `room_for` with the owner known (`None`: no account to check), their
/// course `except` left out of the account's bytes.
fn room_in(conn: &Connection, owner: Option<i64>, except: &str, new_size: usize, limits: Limits) -> rusqlite::Result<Option<crate::problems::Problem>> {
    use crate::problems::Problem;
    if new_size > limits.course_bytes {
        return Ok(Some(
            Problem::refused(format!("The course would take {}; a course may take at most {}", megabytes(new_size), megabytes(limits.course_bytes)))
                .hint("remove lessons or pictures, or split the course in two"),
        ));
    }
    let Some(owner) = owner else { return Ok(None) };
    let total = account_bytes(conn, owner, except)? + new_size;
    if total > limits.account_bytes {
        return Ok(Some(
            Problem::refused(format!(
                "Your courses would take {}; all of an account's courses together may take at most {}",
                megabytes(total),
                megabytes(limits.account_bytes)
            ))
            .hint("delete a course you no longer need"),
        ));
    }
    Ok(None)
}

/// Whether `owner` may store one more course of `new_size` bytes, a copy
/// (#629): within the course limit and, with all their courses, the account
/// limit. Call it in the same `Db::call` as `create_course`, with
/// `room_for_course`.
pub fn room_for_copy(conn: &Connection, owner: i64, new_size: usize, limits: Limits) -> rusqlite::Result<Option<crate::problems::Problem>> {
    room_in(conn, Some(owner), "", new_size, limits)
}

/// The files of a course copied as course `id` called `title` (#629): every
/// file as it is, the manifest's id and title rewritten (its subject stays:
/// the copy teaches the same thing).
pub fn copied_files(files: &[CourseFile], id: &str, title: &str) -> Result<Vec<CourseFile>, String> {
    let mut out = Vec::with_capacity(files.len());
    let mut manifest = false;
    for f in files {
        if f.path != MANIFEST_FILE {
            out.push(f.clone());
            continue;
        }
        let mut v: serde_json::Value = serde_json::from_str(&f.content).map_err(|e| format!("{MANIFEST_FILE}: {e}"))?;
        let obj = v.as_object_mut().ok_or_else(|| format!("{MANIFEST_FILE}: not a JSON object"))?;
        obj.insert("id".into(), serde_json::json!(id));
        obj.insert("title".into(), serde_json::json!(title));
        let content = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())? + "\n";
        out.push(CourseFile { path: MANIFEST_FILE.into(), content });
        manifest = true;
    }
    if !manifest {
        return Err(format!("the course has no {MANIFEST_FILE}"));
    }
    Ok(out)
}

/// The longest course title the editor takes.
pub const MAX_TITLE_CHARS: usize = 80;

/// `title` cut to at most `MAX_TITLE_CHARS` characters, with "…" when cut.
pub fn short_title(title: &str) -> String {
    let title = title.trim();
    if title.chars().count() <= MAX_TITLE_CHARS {
        return title.to_string();
    }
    let cut: String = title.chars().take(MAX_TITLE_CHARS - 1).collect();
    cut.trim_end().to_string() + "…"
}

/// Whether `owner` may make one more course (`Limits::max_courses`). Call it
/// in the same `Db::call` as `create_course`.
pub fn room_for_course(conn: &Connection, owner: i64, limits: Limits) -> rusqlite::Result<Option<crate::problems::Problem>> {
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM courses WHERE owner_id = ?1", [owner], |r| r.get(0))?;
    if n as usize >= limits.max_courses {
        return Ok(Some(
            crate::problems::Problem::refused(format!("You have {n} courses; an account may have at most {}", limits.max_courses))
                .hint("delete a course you no longer need to make another"),
        ));
    }
    Ok(None)
}

/// Validate a path the editor may write: `course.json`, `placement.json` or
/// `lessons/<name>.json` (sub-folders allowed, no `..`, no leading slash).
pub fn valid_path(path: &str) -> bool {
    if path == MANIFEST_FILE || path == crate::placement::FILE {
        return true;
    }
    let (rest, ext) = if let Some(rest) = path.strip_prefix("lessons/") {
        (rest, ".json")
    } else if let Some(rest) = path.strip_prefix(crate::content::PICTURES_DIR) {
        (rest, crate::content::PICTURE_EXT)
    } else {
        return false;
    };
    rest.ends_with(ext) && crate::content::is_safe_rel_path(rest)
}

/// Rebuild a course from the database, record the outcome and refresh the
/// in-memory cache. Returns the validation error, if any.
pub fn revalidate(conn: &Connection, registry: &Courses, course_id: &str) -> rusqlite::Result<Option<String>> {
    let files = load_files(conn, course_id)?;
    registry.invalidate(course_id);
    // A stopped generation with its outline saved stays stopped, whatever is
    // saved into it: its card keeps Resume (which writes the lessons still
    // missing and keeps the files that load) and the reason it stopped (#81).
    let resumable = conn
        .query_row("SELECT 1 FROM courses WHERE id = ?1 AND state = 'failed' AND outline IS NOT NULL", [course_id], |_| Ok(()))
        .optional()?
        .is_some();
    match ContentStore::from_files(&files) {
        Ok(store) => {
            if store.id() != course_id {
                let msg = crate::problems::Problem::rules(format!("The course's \"id\" is “{}”, but it must be “{course_id}”", store.id()))
                    .file(MANIFEST_FILE)
                    .field("id")
                    .hint(format!("change it back to \"{course_id}\""))
                    .text;
                if !resumable {
                    set_status(conn, course_id, None, Some(&msg))?;
                }
                return Ok(Some(msg));
            }
            if resumable {
                conn.execute("UPDATE courses SET title = ?2, updated_at = ?3 WHERE id = ?1", params![course_id, store.title(), now()])?;
            } else {
                set_status(conn, course_id, Some(store.title()), None)?;
                // A generation that stopped before its outline, fixed by hand, is a course now.
                conn.execute("UPDATE courses SET state = 'ready' WHERE id = ?1 AND state = 'failed'", [course_id])?;
            }
            registry.lock().insert(course_id, Arc::new(store));
            Ok(None)
        }
        Err(e) => {
            // Worded as the editor words it (problems.rs), wherever it is shown.
            let msg = crate::problems::Course::new(&files).explain(&e).text;
            if !resumable {
                set_status(conn, course_id, None, Some(&msg))?;
            }
            Ok(Some(msg))
        }
    }
}

// ---------- projects ----------

#[derive(Debug, Clone)]
pub struct ProjectRow {
    pub course_id: String,
    pub last_opened_at: i64,
    /// The motivator worn in this course (#574), '' for the default.
    pub motivator: String,
    /// False until the learner leaves the settings wizard of the course's
    /// first visit (#574).
    pub set_up: bool,
    /// The level the learner chose (`projects.start_unit`), '' for the beginning.
    pub start_unit: String,
}

/// Open a course for the user: upsert the project row and make it active.
/// True when this is the first time they open it.
pub fn open_project(conn: &Connection, user_id: i64, course_id: &str) -> rusqlite::Result<bool> {
    let t = now();
    let new = conn
        .query_row("SELECT 1 FROM projects WHERE user_id = ?1 AND course_id = ?2", params![user_id, course_id], |_| Ok(()))
        .optional()?
        .is_none();
    // A course opened for the first time starts with the default motivator
    // ('', which wears `motivators::DEFAULT`).
    conn.execute(
        "INSERT INTO projects (user_id, course_id, created_at, last_opened_at)
         SELECT ?1, ?2, ?3, ?3 FROM users WHERE id = ?1
         ON CONFLICT(user_id, course_id) DO UPDATE SET last_opened_at = excluded.last_opened_at",
        params![user_id, course_id, t],
    )?;
    conn.execute("UPDATE users SET active_course = ?2 WHERE id = ?1", params![user_id, course_id])?;
    Ok(new)
}

/// What a course the user has not opened yet would start with (#587): the
/// default motivator, the beginning. The settings wizard behind its Start
/// button shows these.
pub fn fresh_project(conn: &Connection, user_id: i64, course_id: &str) -> rusqlite::Result<ProjectRow> {
    conn.query_row("SELECT 1 FROM users WHERE id = ?1", [user_id], |_| {
        Ok(ProjectRow {
            course_id: course_id.to_string(),
            last_opened_at: 0,
            motivator: String::new(),
            set_up: false,
            start_unit: String::new(),
        })
    })
}

/// Changes to a course's settings (#565, #574); a field left `None` keeps
/// its stored value.
#[derive(Debug, Default)]
pub struct SettingsChange {
    /// The motivator worn (a known id).
    pub motivator: Option<String>,
    /// The level started at (#587): a unit id, '' for the beginning
    /// (`ContentStore::start_index` turns it into a unit index).
    pub start_unit: Option<String>,
}

/// Save a course's settings, for a course the user opened. Any save, even
/// of nothing, counts the course as set up: its first-visit wizard is done.
/// False when they never opened it.
pub fn set_project_settings(conn: &Connection, user_id: i64, course_id: &str, change: &SettingsChange) -> rusqlite::Result<bool> {
    let n = conn.execute(
        "UPDATE projects SET set_up = 1,
                motivator = COALESCE(?3, motivator),
                start_unit = COALESCE(?4, start_unit)
         WHERE user_id = ?1 AND course_id = ?2",
        params![user_id, course_id, change.motivator, change.start_unit],
    )?;
    Ok(n > 0)
}

pub fn clear_active(conn: &Connection, user_id: i64) -> rusqlite::Result<()> {
    conn.execute("UPDATE users SET active_course = NULL WHERE id = ?1", [user_id])?;
    Ok(())
}

pub fn projects_of(conn: &Connection, user_id: i64) -> rusqlite::Result<Vec<ProjectRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT course_id, last_opened_at, motivator, set_up, start_unit FROM projects WHERE user_id = ?1 ORDER BY last_opened_at DESC",
    )?;
    let rows = stmt.query_map([user_id], project_row)?;
    rows.collect()
}

/// Whether the user ever opened the course: a primary-key lookup.
pub fn has_project(conn: &Connection, user_id: i64, course_id: &str) -> rusqlite::Result<bool> {
    let mut stmt = conn.prepare_cached(HAS_PROJECT)?;
    stmt.exists(params![user_id, course_id])
}

const HAS_PROJECT: &str = "SELECT 1 FROM projects WHERE user_id = ?1 AND course_id = ?2";

/// The user's project in one course, if they opened it.
pub fn project(conn: &Connection, user_id: i64, course_id: &str) -> rusqlite::Result<Option<ProjectRow>> {
    conn.query_row(
        "SELECT course_id, last_opened_at, motivator, set_up, start_unit FROM projects WHERE user_id = ?1 AND course_id = ?2",
        params![user_id, course_id],
        project_row,
    )
    .optional()
}

fn project_row(r: &rusqlite::Row) -> rusqlite::Result<ProjectRow> {
    Ok(ProjectRow {
        course_id: r.get(0)?,
        last_opened_at: r.get(1)?,
        motivator: r.get(2)?,
        set_up: r.get(3)?,
        start_unit: r.get(4)?,
    })
}

// ---------- starter files for "build my own" ----------

/// A minimal valid course a person can edit into shape. Its description,
/// unit title and lesson title are in the request's app language (#1255).
pub fn starter_files(id: &str, title: &str) -> Vec<CourseFile> {
    let manifest = serde_json::json!({
        "id": id,
        "title": title,
        "subject": title,
        "description": crate::t("courses.starter.description"),
        "icon": "📚",
        "terms": { "label": "Term", "lang": "" },
        "meanings": { "label": "Meaning", "lang": "" },
        "units": [
            { "id": "1", "title": crate::t("courses.starter.unit"), "description": "The basics" }
        ],
        "groups": [
            { "id": "basics", "title": "Basics" }
        ],
        "chars": []
    });
    let lesson = serde_json::json!({
        "id": "u1-first-lesson",
        "slug": "first-lesson",
        "title": crate::t("courses.starter.lesson"),
        "unit": "1",
        "group": "basics",
        "order": 1,
        "summary": "A one-line summary shown in lists.",
        "focus": "The single idea this lesson teaches",
        "content": "## The idea\n\nExplain the idea in a short paragraph. Each `## ` heading becomes one card in the Guide and Sprint mode.\n\n## An example\n\n- **term** — what it means\n- Use lists, **bold**, tables and code blocks freely.",
        "vocabulary": [
            { "term": "term", "meaning": "a word or phrase worth remembering", "example": "An example sentence using the term." },
            { "term": "meaning", "meaning": "what the term stands for" },
            { "term": "example", "meaning": "a sentence that shows the term in use" }
        ],
        "quiz": [
            {
                "question_id": "u1-first-01",
                "prompt": "Which field holds what a term stands for?",
                "type": "multiple-choice",
                "options": ["meaning", "example", "slug"],
                "correct_answer": "meaning",
                "explanation": "Every vocabulary entry has a term and its meaning.",
                "hints": ["It is the other half of a flashcard."],
                "teaches": ["meaning"]
            },
            {
                "question_id": "u1-first-02",
                "prompt": "Type the word for a sentence that shows a term in use.",
                "type": "fill-in-blank",
                "correct_answer": "example",
                "accepted_answers": ["an example"],
                "explanation": "Examples are optional but make flashcards much better."
            },
            {
                "question_id": "u1-first-03",
                "prompt": "How many options does a true-false question offer?",
                "type": "numeric",
                "correct_answer": "2",
                "explanation": "Numeric answers are checked by value: 2, 2.0 and 4/2 all count. Use \"expression\" for formulas."
            },
            {
                "question_id": "u1-first-04",
                "prompt": "A numeric question can accept answers within a tolerance.",
                "type": "true-false",
                "correct_answer": "true",
                "explanation": "Add \"tolerance\": 0.01 to accept answers rounded to two decimals."
            }
        ],
        "sprint": {
            "icon": "🌱",
            "bites": [
                { "title": "One idea per lesson", "emoji": "🎯", "rule": "Keep each lesson to one idea the learner can hold in their head.", "example": "term → meaning", "example_meaning": "a flashcard" }
            ],
            "sorts": [
                { "title": "Which is which?", "buckets": ["term", "meaning"], "items": [
                    { "text": "a word to learn", "bucket": 0 }, { "text": "what it stands for", "bucket": 1 }, { "text": "the front of a flashcard", "bucket": 0 }
                ], "why": "Terms go on the front of the card, meanings on the back." }
            ]
        }
    });
    vec![
        CourseFile { path: MANIFEST_FILE.into(), content: serde_json::to_string_pretty(&manifest).expect("json") + "\n" },
        CourseFile { path: "lessons/first-lesson.json".into(), content: serde_json::to_string_pretty(&lesson).expect("json") + "\n" },
    ]
}

/// The title of a course made with "Create a course" until it is renamed,
/// in the request's app language (#1255).
pub fn untitled() -> String {
    crate::t("courses.starter.title")
}

/// `key` (a `courses.starter` text) in every app language: a draft made in
/// German still counts as untouched when a draft check runs in English.
fn in_every_language(key: &str) -> Vec<String> {
    crate::i18n::languages().iter().map(|l| l.get(key).to_string()).collect()
}

/// What a learner put into a fresh course before generating it: names only.
/// The AI is given them as context.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Draft {
    /// The course title, unless it is still `untitled()` (in any app language).
    pub title: Option<String>,
    /// The lesson titles in course order (the starter lesson only once renamed).
    pub lessons: Vec<String>,
}

/// `files` as a draft: the starter course where only names changed (the
/// course title, lesson titles, lessons added by name alone). `None` once
/// anything else has, so "Generate the course" goes away.
pub fn draft_of(files: &[CourseFile]) -> Option<Draft> {
    fn strip(content: &str, keys: &[&str]) -> Option<serde_json::Value> {
        let mut v: serde_json::Value = serde_json::from_str(content).ok()?;
        let o = v.as_object_mut()?;
        for k in keys {
            o.remove(*k);
        }
        // A lesson added by name keeps the table's placeholder text, whatever its heading says.
        if o.get("content").and_then(|c| c.as_str()).is_some_and(|c| {
            c.strip_prefix("## ").and_then(|c| c.split_once("\n\n")).is_some_and(|(h, rest)| !h.contains('\n') && rest == BLANK_LESSON_TEXT)
        }) {
            o.insert("content".into(), serde_json::Value::Null);
        }
        Some(v)
    }
    const NAMES: &[&str] = &["id", "title", "subject"];
    const PLACE: &[&str] = &["id", "slug", "title", "unit", "group", "order"];
    // The starter as "Create a course" wrote it in the author's app language, whichever that was.
    let manifest = strip(&files.iter().find(|f| f.path == MANIFEST_FILE)?.content, NAMES)?;
    let starter = crate::i18n::languages()
        .iter()
        .map(|l| crate::i18n::in_lang(&l.code, || starter_files("draft", "")))
        .find(|s| strip(&s[0].content, NAMES).is_some_and(|m| m == manifest))?;
    let (store, lessons) = crate::arrange::lessons_of(files).ok()?;
    if files.len() != lessons.len() + 1 {
        return None;
    }
    let example = strip(&starter[1].content, PLACE)?;
    let named = serde_json::json!({ "summary": "", "content": null });
    let mut titles = Vec::new();
    for l in &lessons {
        let file = files.iter().find(|f| f.path == l.path)?;
        let body = strip(&file.content, PLACE)?;
        let title = l.value.get("title").and_then(|t| t.as_str()).unwrap_or_default().trim().to_string();
        if body == example {
            if !in_every_language("courses.starter.lesson").contains(&title) {
                titles.push(title);
            }
        } else if body == named {
            titles.push(title);
        } else {
            return None;
        }
    }
    let title = store.title().trim();
    let untitled = in_every_language("courses.starter.title").iter().any(|u| u == title);
    Some(Draft { title: (!untitled && !title.is_empty()).then(|| title.to_string()), lessons: titles })
}

/// The text after the heading of a lesson added by name (`arrange::blank_lesson`).
pub const BLANK_LESSON_TEXT: &str = "Write the lesson here. Each `## ` heading becomes one card.";

/// Hand a draft (or a generation that stopped before its outline) to an AI
/// job that writes the whole course into it: its files and Undo steps go,
/// and it becomes an AI course with this brief.
pub fn restart_generation(conn: &mut Connection, id: &str, title: &str, brief: &str, notes: &str) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM course_files WHERE course_id = ?1", [id])?;
    tx.execute("DELETE FROM course_undo_files WHERE course_id = ?1", [id])?;
    tx.execute("DELETE FROM course_undo WHERE course_id = ?1", [id])?;
    tx.execute(
        "UPDATE courses SET title = ?2, source = 'ai', brief = ?3, notes = ?4, outline = NULL, error = NULL, updated_at = ?5 WHERE id = ?1",
        params![id, title, brief, notes, now()],
    )?;
    tx.commit()
}

#[cfg(test)]
mod tests;
