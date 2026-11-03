//! "Download my data" as a background job (#430, #365).
//!
//! - The profile's button queues a row in `data_exports` ([`request`]): one
//!   export in flight per account, and one a day ([`COOLDOWN_SECS`]).
//! - The request also queues a `jobs` row of kind `export` (#875), and the
//!   shared runner (`jobs.rs`, one OS thread) builds the whole file in one
//!   run: the export's header, then each table of `privacy::USER_TABLES` a
//!   page at a time ([`privacy::page`]), each page one short call on the
//!   connection. A family's data is a few MB, written in well under a
//!   second. Rows go straight to a file in the exports folder (next to the
//!   database, `<db>.exports/`); the account is never held in memory.
//! - A failed run deletes its files and marks the export failed (the job has
//!   one attempt; the learner can ask again at once). A server that stops
//!   mid-run takes the row again when its lease runs out (after a restart)
//!   and starts over; the earlier part file is no row's file any more and
//!   the hourly sweep ([`remove_strays`]) deletes it.
//! - When the file is complete the profile's Data section shows a Download
//!   button (`POST /profile/export/download`, [`take_ready`]), and, when the
//!   server can send mail, the learner is e-mailed a one-time link
//!   (`/profile/export/download?token=…`): 32 random bytes, stored only as
//!   their SHA-256. Either works for [`LINK_TTL_SECS`] and only for the
//!   account signed in. Downloading deletes the file; so does expiry (hourly
//!   housekeeping, [`purge`]) and deleting the account.
//! - The finished file stays in the exports folder next to the database,
//!   and the download streams it from there.
//! - The file has the same content the synchronous export had: the columns
//!   `privacy::EXPORTED` lists. Pages are read at different moments, so an
//!   edit made while it is built may show in one table and not another.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::db::Db;
use crate::{jobs, privacy};

/// The outbox `purpose` of the "your data is ready" mail.
pub const PURPOSE: &str = "data-export";
/// How long the mailed link works.
pub const LINK_TTL_SECS: i64 = 24 * 60 * 60;
/// One export a day per account (failed ones do not count).
pub const COOLDOWN_SECS: i64 = 24 * 60 * 60;
/// Rows are deleted this long after they were made (hourly housekeeping).
pub const KEEP_DAYS: i64 = 30;
/// A page: at most this many rows, and it ends after the row that takes it
/// past this many bytes of text.
const PAGE_ROWS: usize = 200;
const PAGE_BYTES: usize = 256 * 1024;
/// What [`request`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Requested {
    /// A new export is queued (its row id).
    Queued(i64),
    /// One is already queued or being built.
    InFlight,
    /// One was made within the day: the next can be asked for at this time.
    TooSoon(i64),
}

/// Queue an export of `user_id`'s data, unless one is in flight or one was
/// made in the last day. `link_base` is where the mailed link will point.
/// The row and its `export` job are added in one transaction; the caller
/// wakes the runner (`Queue::notify`).
pub fn request(conn: &mut Connection, user_id: i64, link_base: &str, now: i64) -> rusqlite::Result<Requested> {
    let tx = conn.transaction()?;
    let r = request_in(&tx, user_id, link_base, now)?;
    tx.commit()?;
    Ok(r)
}

fn request_in(conn: &Connection, user_id: i64, link_base: &str, now: i64) -> rusqlite::Result<Requested> {
    let in_flight: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM data_exports WHERE user_id = ?1 AND state IN ('queued', 'building'))", [user_id], |r| r.get(0))?;
    if in_flight {
        return Ok(Requested::InFlight);
    }
    if let too_soon @ Requested::TooSoon(_) = request_check(conn, user_id, now)? {
        return Ok(too_soon);
    }
    conn.execute("INSERT INTO data_exports (user_id, link_base, created_at) VALUES (?1, ?2, ?3)", params![user_id, link_base, now])?;
    let id = conn.last_insert_rowid();
    jobs::enqueue(conn, jobs::EXPORT.name, &serde_json::json!({ "export": id }), now, now)?;
    Ok(Requested::Queued(id))
}

/// The newest export of an account, for the profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// `queued`, `building`, `ready`, `downloaded`, `expired` or `failed`.
    pub state: String,
    pub created_at: i64,
    pub expires_at: Option<i64>,
    pub downloaded_at: Option<i64>,
    /// When another export can be asked for (`None`: now).
    pub next_at: Option<i64>,
}

impl Status {
    /// Queued or being built.
    pub fn in_flight(&self) -> bool {
        matches!(self.state.as_str(), "queued" | "building")
    }
}

pub fn status(conn: &Connection, user_id: i64, now: i64) -> rusqlite::Result<Option<Status>> {
    let row = conn
        .query_row(
            "SELECT state, created_at, expires_at, downloaded_at FROM data_exports WHERE user_id = ?1 ORDER BY id DESC LIMIT 1",
            [user_id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    let Some((state, created_at, expires_at, downloaded_at)) = row else { return Ok(None) };
    let next_at = match request_check(conn, user_id, now)? {
        Requested::TooSoon(t) => Some(t),
        _ => None,
    };
    Ok(Some(Status { state, created_at, expires_at, downloaded_at, next_at }))
}

/// [`request`]'s checks without the insert.
fn request_check(conn: &Connection, user_id: i64, now: i64) -> rusqlite::Result<Requested> {
    let last: Option<i64> = conn.query_row(
        "SELECT MAX(created_at) FROM data_exports WHERE user_id = ?1 AND state <> 'failed' AND created_at > ?2",
        params![user_id, now - COOLDOWN_SECS],
        |r| r.get(0),
    )?;
    Ok(last.map_or(Requested::Queued(0), |t| Requested::TooSoon(t + COOLDOWN_SECS)))
}

/// The stored form of a link token: SHA-256, hex.
pub fn hash_token(token: &str) -> String {
    crate::auth::hash_token(token)
}

/// Use a download link: when `token` is the live link of one of `user_id`'s
/// exports, mark it downloaded (the link stops working) and return the
/// file's name in the exports folder, for the caller to send and delete.
pub fn take(conn: &Connection, user_id: i64, token: &str, now: i64) -> rusqlite::Result<Option<String>> {
    if token.is_empty() {
        return Ok(None);
    }
    let found: Option<(i64, String)> = conn
        .query_row(
            "SELECT id, file FROM data_exports WHERE user_id = ?1 AND token_hash = ?2 AND state = 'ready' AND expires_at > ?3",
            params![user_id, hash_token(token), now],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((id, file)) = found else { return Ok(None) };
    conn.execute("UPDATE data_exports SET state = 'downloaded', downloaded_at = ?2, token_hash = '', file = '' WHERE id = ?1", params![id, now])?;
    Ok(Some(file))
}

/// The profile's Download button: like [`take`], for the account's ready
/// export, without the mailed token (the session is the account's).
pub fn take_ready(conn: &Connection, user_id: i64, now: i64) -> rusqlite::Result<Option<String>> {
    let found: Option<(i64, String)> = conn
        .query_row(
            "SELECT id, file FROM data_exports WHERE user_id = ?1 AND state = 'ready' AND expires_at > ?2 ORDER BY id DESC LIMIT 1",
            params![user_id, now],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((id, file)) = found else { return Ok(None) };
    conn.execute("UPDATE data_exports SET state = 'downloaded', downloaded_at = ?2, token_hash = '', file = '' WHERE id = ?1", params![id, now])?;
    Ok(Some(file))
}

/// The housekeeping's reads, each served by an index (v59, #626): the files
/// [`purge`] deletes, the links it expires, the rows it drops, and the
/// files [`remove_strays`] keeps.
const PURGE_FILES: &str = "SELECT file FROM data_exports WHERE file <> '' AND ((state = 'ready' AND expires_at <= ?1) OR (created_at < ?2 AND state NOT IN ('queued', 'building')))";
const PURGE_EXPIRED: &str = "UPDATE data_exports SET state = 'expired', token_hash = '', file = '' WHERE state = 'ready' AND expires_at <= ?1";
const PURGE_OLD: &str = "DELETE FROM data_exports WHERE created_at < ?1 AND state NOT IN ('queued', 'building')";
const KNOWN_FILES: &str = "SELECT file FROM data_exports WHERE file <> ''";

/// Retention: a ready export whose link has expired loses its link and its
/// file; rows older than [`KEEP_DAYS`] go. The file names to delete.
pub fn purge(conn: &Connection, now: i64) -> rusqlite::Result<Vec<String>> {
    let mut files: Vec<String> = {
        let mut stmt = conn.prepare(PURGE_FILES)?;
        let v = stmt.query_map(params![now, now - KEEP_DAYS * 86_400], |r| r.get(0))?.collect::<Result<_, _>>()?;
        v
    };
    conn.execute(PURGE_EXPIRED, [now])?;
    conn.execute(PURGE_OLD, [now - KEEP_DAYS * 86_400])?;
    files.dedup();
    Ok(files)
}

/// Delete files from the exports folder (names from [`take`], [`purge`] or
/// account deletion). A missing file is fine.
pub fn remove_files(dir: &Path, files: &[String]) {
    for f in files {
        if let Some(p) = file_path(dir, f) {
            if let Err(e) = std::fs::remove_file(&p) {
                if e.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!("exports: could not delete {}: {e}", p.display());
                }
            }
        }
    }
}

/// A stored name as a path in `dir`; `None` for anything but a plain name.
pub fn file_path(dir: &Path, file: &str) -> Option<PathBuf> {
    let plain = !file.is_empty() && file.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.') && !file.starts_with('.');
    plain.then(|| dir.join(file))
}

// ---------- building, the whole file in one job run ----------

/// An `export` job's payload: the `data_exports` row.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct Payload {
    export: i64,
}

/// Is the export still wanted (the row, and so the account, still there)?
fn wanted(db: &Db, id: i64) -> bool {
    db.with(|c| {
        c.query_row(
            "SELECT EXISTS(SELECT 1 FROM data_exports WHERE id = ?1)",
            [id],
            |r| r.get(0),
        )
    })
    .unwrap_or(false)
}

/// Why a build stopped.
#[derive(Debug)]
enum Stop {
    /// The account (or the row) went while it was built.
    Gone,
    Failed(String),
}

impl From<rusqlite::Error> for Stop {
    fn from(e: rusqlite::Error) -> Stop {
        Stop::Failed(e.to_string())
    }
}

impl From<std::io::Error> for Stop {
    fn from(e: std::io::Error) -> Stop {
        Stop::Failed(e.to_string())
    }
}

/// The `export` job's handler: builds a file in one run.
pub struct Builder {
    /// The exports folder.
    pub dir: PathBuf,
    /// Wakes the mail sender. `None` on a server that cannot send mail: no
    /// mail, the profile's Download button is the way to the file.
    pub wake_mail: Option<Box<dyn Fn() + Send>>,
}

impl Builder {
    pub fn new(dir: PathBuf, wake_mail: Option<Box<dyn Fn() + Send>>) -> Builder {
        Builder { dir, wake_mail }
    }

    /// One run of the `export` job: the whole export its payload names. A
    /// failure marks the export failed and deletes its files; the learner
    /// asks again.
    pub fn run(&self, db: &Db, payload: &str) -> Result<String, String> {
        let id = serde_json::from_str::<Payload>(payload)
            .map_err(|e| format!("bad payload: {e}"))?
            .export;
        let row: Option<(i64, String)> = db
            .with(|c| {
                c.query_row(
                    "SELECT user_id, state FROM data_exports WHERE id = ?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
            })
            .map_err(|e| e.to_string())?;
        let Some((user_id, state)) = row else {
            return Ok(format!("export #{id}: the account went; nothing built"));
        };
        if !matches!(state.as_str(), "queued" | "building") {
            // The server stopped after the export was finished but before the job was.
            return Ok(String::new());
        }
        // The name goes on the row first, so the stray sweep and account
        // deletion know the file; a stopped server's earlier part file is
        // left to the sweep.
        let file = format!("{id}-{}.json", &crate::auth::generate_token()[..16]); // panic-ok: the token is 64 hex digits
        match self.build(db, id, user_id, &file) {
            Ok(bytes) => {
                let how = match &self.wake_mail {
                    Some(wake) => {
                        wake();
                        "link mailed"
                    }
                    None => "on the profile (no mail)",
                };
                Ok(format!(
                    "export #{id} for user {user_id} ready ({bytes} bytes); {how}"
                ))
            }
            Err(Stop::Gone) => {
                self.remove(&file);
                Ok(format!(
                    "export #{id}: the account went while it was built; file deleted"
                ))
            }
            Err(Stop::Failed(why)) => {
                self.remove(&file);
                let _ = db.with(|c| c.execute("UPDATE data_exports SET state = 'failed', error = ?2, file = '' WHERE id = ?1", params![id, why]));
                Err(format!("export #{id} for user {user_id} failed: {why}"))
            }
        }
    }

    /// Delete an unfinished build's files.
    fn remove(&self, file: &str) {
        remove_files(&self.dir, &[format!("{file}.part"), file.to_string()]);
    }

    /// Write the file as `<file>.part`, rename it, mark the export ready
    /// (its mail queued). The file's size.
    fn build(&self, db: &Db, id: i64, user_id: i64, file: &str) -> Result<u64, Stop> {
        let tables = privacy::USER_TABLES;
        let part = format!("{file}.part");
        let named = db.with(|c| {
            c.execute(
                "UPDATE data_exports SET state = 'building', file = ?2 WHERE id = ?1",
                params![id, part],
            )
        })?;
        if named == 0 {
            return Err(Stop::Gone);
        }
        std::fs::create_dir_all(&self.dir)?;
        let Some(head) = db.with(|c| privacy::export_header(c, user_id))? else {
            return Err(Stop::Gone);
        };
        let mut out = BufWriter::with_capacity(64 * 1024, File::create(self.dir.join(&part))?);
        out.write_all(b"{")?;
        for (k, v) in &head {
            json(&mut out, &Value::String(k.clone()))?;
            out.write_all(b":")?;
            json(&mut out, v)?;
            out.write_all(b",\n")?;
        }
        out.write_all(b"\"tables\":{")?;
        for (i, (table, _)) in tables.iter().enumerate() {
            open_table(&mut out, table, i == 0)?;
            let mut after = privacy::After::new();
            let mut written = 0;
            loop {
                let (rows, last) = db.with(|c| {
                    privacy::page(c, table, user_id, &after, PAGE_ROWS, PAGE_BYTES)
                })?;
                // A page can end early on its byte budget: the table is done when one comes back empty.
                if rows.is_empty() {
                    break;
                }
                for row in &rows {
                    if written > 0 {
                        out.write_all(b",")?;
                    }
                    out.write_all(b"\n")?;
                    json(&mut out, row)?;
                    written += 1;
                }
                after = last;
                if !wanted(db, id) {
                    return Err(Stop::Gone);
                }
            }
            out.write_all(b"]")?;
        }
        out.write_all(b"\n}}\n")?;
        let f = out.into_inner().map_err(|e| Stop::Failed(e.to_string()))?;
        f.sync_all()?;
        let bytes = f.metadata()?.len();
        drop(f);
        std::fs::rename(self.dir.join(&part), self.dir.join(file))?;
        let token = crate::auth::generate_token();
        if !db.with(|c| {
            finish(
                c,
                id,
                file,
                &token,
                bytes as i64,
                crate::db::now(),
                self.wake_mail.is_some(),
            )
        })? {
            return Err(Stop::Gone);
        }
        Ok(bytes)
    }
}

fn json(out: &mut impl Write, v: &Value) -> Result<(), Stop> {
    serde_json::to_writer(out, v).map_err(|e| Stop::Failed(e.to_string()))
}

/// `"name":[` on a new line, after a comma unless it is the first table.
fn open_table(out: &mut impl Write, table: &str, first: bool) -> Result<(), Stop> {
    if !first {
        out.write_all(b",")?;
    }
    out.write_all(b"\n")?;
    json(out, &Value::String(table.to_string()))?;
    out.write_all(b":[")?;
    Ok(())
}

/// The file is complete: store the link's hash and, with `mail`, queue the
/// mail, in one transaction. `false` when the row (or the account) is gone.
fn finish(conn: &mut Connection, id: i64, file: &str, token: &str, bytes: i64, now: i64, mail: bool) -> rusqlite::Result<bool> {
    let tx = conn.transaction()?;
    let row: Option<(String, String)> = tx
        .query_row("SELECT u.email, e.link_base FROM data_exports e JOIN users u ON u.id = e.user_id WHERE e.id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    let Some((email, base)) = row else { return Ok(false) };
    let user_id: i64 = tx.query_row("SELECT user_id FROM data_exports WHERE id = ?1", [id], |r| r.get(0))?;
    tx.execute(
        "UPDATE data_exports SET state = 'ready', file = ?2, token_hash = ?3, bytes = ?4, ready_at = ?5, expires_at = ?6 WHERE id = ?1",
        params![id, file, hash_token(token), bytes, now, now + LINK_TTL_SECS],
    )?;
    if !mail {
        tx.commit()?;
        return Ok(true);
    }
    let link = format!("{}/profile/export/download?token={token}", base.trim_end_matches('/'));
    match message(&email, &link, user_id) {
        Ok(m) => {
            // No hourly ceiling here: one export a day per account bounds these.
            if let Err(capped) = crate::mail::enqueue(&tx, &m, now, None)? {
                tracing::warn!("export #{id}: mail not queued ({capped:?})");
            }
        }
        // No usable address: the profile still shows the export as ready,
        // but without the mail nobody has the link; it expires unused.
        Err(e) => tracing::warn!("export #{id}: no mail sent ({e})"),
    }
    tx.commit()?;
    Ok(true)
}

/// The mail with the link.
pub fn message(email: &str, link: &str, user_id: i64) -> Result<crate::mail::Mail, String> {
    let app = crate::APP_NAME;
    let subject = crate::t1("mail.data_export.subject", "app", app);
    let text = crate::t2("mail.data_export.text", "app", app, "link", link);
    let href = link.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
    let html = crate::t2("mail.data_export.html", "app", app, "link", href);
    Ok(crate::mail::Mail::new(email, &subject, &text, PURPOSE)?.html(&html).for_user(user_id))
}

/// The hourly sweep: delete files in the exports folder that no row names
/// (left by a crash, or by a build whose row went). The number deleted.
/// Builds run on the same `jobs` thread as this sweep, so a file being
/// written is always named on its row first.
pub fn remove_strays(db: &Db, dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    let Ok(known) = db.with(|c| {
        let mut stmt = c.prepare(KNOWN_FILES)?;
        let v: Vec<String> = stmt.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
        Ok(v)
    }) else {
        return 0;
    };
    let mut n = 0;
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !known.contains(&name) && std::fs::remove_file(e.path()).is_ok() {
            n += 1;
        }
    }
    n
}

/// The exports folder, kept in `AppState`.
pub struct Exports {
    pub dir: PathBuf,
}

impl Exports {
    /// The folder next to the database, `<db>.exports`, made if missing.
    pub fn open(db_path: &Path) -> std::io::Result<Exports> {
        let mut s = db_path.as_os_str().to_owned();
        s.push(".exports");
        let dir = PathBuf::from(s);
        std::fs::create_dir_all(&dir)?;
        Ok(Exports { dir })
    }

    /// Delete finished or half-built exports ([`take`], [`purge`], account
    /// deletion).
    pub fn remove(&self, files: &[String]) {
        remove_files(&self.dir, files);
    }
}

/// Build every due export now, on this thread (tests stand in for the `jobs`
/// thread with it). How many exports finished.
#[cfg(test)]
pub fn run_queued(db: &Db, dir: &Path, wake_mail: Option<Box<dyn Fn() + Send>>) -> usize {
    let builder = Builder::new(dir.to_path_buf(), wake_mail);
    let runner = jobs::Runner::new().handle(jobs::EXPORT, move |db, p| builder.run(db, p));
    let mut n = 0;
    while let Ok(Some(_)) = runner.run_one(db, crate::db::now()) {
        n += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// A mail waker that counts its calls.
    fn counter() -> (Arc<AtomicUsize>, Option<Box<dyn Fn() + Send>>) {
        let n = Arc::new(AtomicUsize::new(0));
        let m = Arc::clone(&n);
        (n, Some(Box::new(move || {
            m.fetch_add(1, Ordering::SeqCst);
        })))
    }

    fn tmp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("skrongle-exports-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn user(db: &Db, email: &str) -> i64 {
        db.with(|c| crate::auth::create_user(c, email, "$argon2id$secret-hash")).unwrap()
    }

    /// One in flight, then one a day; a failed one does not count.
    #[test]
    fn one_export_in_flight_and_one_a_day() {
        let db = Db::open_in_memory().unwrap();
        let ann = user(&db, "ann@example.org");
        db.with(|c| {
            let Requested::Queued(id) = request(c, ann, "http://x", 100)? else { panic!("not queued") };
            assert_eq!(request(c, ann, "http://x", 101)?, Requested::InFlight);
            // The export's job is queued with it, and only once.
            let payloads: Vec<String> = c.prepare("SELECT payload_json FROM jobs WHERE kind = 'export'")?.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
            assert_eq!(payloads, vec![format!(r#"{{"export":{id}}}"#)]);
            c.execute("UPDATE data_exports SET state = 'ready' WHERE id = ?1", [id])?;
            assert_eq!(request(c, ann, "http://x", 200)?, Requested::TooSoon(100 + COOLDOWN_SECS));
            assert_eq!(status(c, ann, 200)?.unwrap().next_at, Some(100 + COOLDOWN_SECS));
            assert!(matches!(request(c, ann, "http://x", 100 + COOLDOWN_SECS)?, Requested::Queued(_)));
            c.execute("UPDATE data_exports SET state = 'failed'", [])?;
            assert!(matches!(request(c, ann, "http://x", 100 + COOLDOWN_SECS + 1)?, Requested::Queued(_)));
            Ok(())
        })
        .unwrap();
    }

    /// The job's work, start to end: the file is the export, written
    /// in pages (more rows than one page holds), the link is mailed and
    /// stored hashed, works once, for its owner only, and the file goes.
    #[test]
    fn an_export_is_built_in_pages_mailed_and_downloaded_once() {
        let db = Db::open_in_memory().unwrap();
        let dir = tmp_dir("build");
        let ann = user(&db, "ann@example.org");
        let bob = user(&db, "bob@example.org");
        db.with(|c| {
            for i in 0..(PAGE_ROWS * 2 + 7) {
                c.execute(
                    "INSERT INTO quiz_history (user_id, course_id, lesson_id, question_id, selected_answer, was_correct, answered_at) VALUES (?1, 'spanish', 'a1', ?2, 'ann-answer', 1, 1)",
                    params![ann, format!("q{i}")],
                )?;
            }
            c.execute("INSERT INTO courses (id, owner_id, title, created_at, updated_at) VALUES ('big', ?1, 'Big', 1, 1)", [ann])?;
            for i in 0..5 {
                c.execute("INSERT INTO course_files (course_id, path, content, updated_at) VALUES ('big', ?1, ?2, 1)", params![format!("lessons/{i}.json"), "x".repeat(PAGE_BYTES / 2)])?;
            }
            Ok(())
        })
        .unwrap();
        let Requested::Queued(id) = db.with(|c| request(c, ann, "http://site/", 10)).unwrap() else { panic!() };
        let (woken, wake) = counter();
        assert_eq!(run_queued(&db, &dir, wake), 1);
        assert_eq!(woken.load(Ordering::SeqCst), 1);

        let (file, hash, state): (String, String, String) = db.with(|c| c.query_row("SELECT file, token_hash, state FROM data_exports WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))).unwrap();
        assert_eq!(state, "ready");
        let text = std::fs::read_to_string(dir.join(&file)).unwrap();
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["user_id"], ann);
        assert_eq!(v["tables"]["quiz_history"].as_array().unwrap().len(), PAGE_ROWS * 2 + 7);
        assert_eq!(v["tables"]["course_files"].as_array().unwrap().len(), 5);
        for (t, _) in privacy::USER_TABLES {
            assert!(v["tables"][*t].is_array(), "no {t}");
        }
        assert!(!text.contains("secret-hash") && !text.contains(&hash) && !text.contains("bob@"));
        // The same as the synchronous export, but for the time it was made.
        let mut whole = db.with(|c| privacy::export(c, ann)).unwrap().unwrap();
        whole["exported_at"] = v["exported_at"].clone();
        // The row and the mail made once the file was done are not in it.
        whole["tables"]["data_exports"] = v["tables"]["data_exports"].clone();
        whole["tables"]["mail_outbox"] = v["tables"]["mail_outbox"].clone();
        for (t, _) in privacy::USER_TABLES {
            assert!(whole["tables"][*t] == v["tables"][*t], "{t} differs: {} vs {}", whole["tables"][*t].to_string().chars().take(300).collect::<String>(), v["tables"][*t].to_string().chars().take(300).collect::<String>());
        }
        assert_eq!(whole, v);

        // The mail: the link, whose token is only stored hashed.
        let (purpose, body): (String, String) = db.with(|c| c.query_row("SELECT purpose, body_text FROM mail_outbox WHERE user_id = ?1", [ann], |r| Ok((r.get(0)?, r.get(1)?)))).unwrap();
        assert_eq!(purpose, PURPOSE);
        let token = body.split("token=").nth(1).unwrap().split_whitespace().next().unwrap().to_string();
        assert!(body.contains("http://site/profile/export/download?token="));
        assert_eq!(hash_token(&token), hash);
        assert!(!body.contains(&hash));

        let now = 20;
        assert_eq!(db.with(|c| take(c, bob, &token, now)).unwrap(), None, "another account");
        assert_eq!(db.with(|c| take(c, ann, "nope", now)).unwrap(), None);
        assert_eq!(db.with(|c| take(c, ann, &token, now)).unwrap(), Some(file.clone()));
        assert_eq!(db.with(|c| take(c, ann, &token, now)).unwrap(), None, "once");
        remove_files(&dir, &[file.clone()]);
        assert!(!dir.join(&file).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An expired link loses its file at the hourly sweep, old rows go,
    /// and so does a file nobody names.
    #[test]
    fn expired_exports_and_strays_are_deleted() {
        let db = Db::open_in_memory().unwrap();
        let dir = tmp_dir("purge");
        let ann = user(&db, "ann@example.org");
        let made = crate::db::now();
        let Requested::Queued(id) = db.with(|c| request(c, ann, "http://x", made)).unwrap() else { panic!() };
        run_queued(&db, &dir, counter().1);
        let file: String = db.with(|c| c.query_row("SELECT file FROM data_exports WHERE id = ?1", [id], |r| r.get(0))).unwrap();
        assert!(dir.join(&file).exists());
        let expires: i64 = db.with(|c| c.query_row("SELECT expires_at FROM data_exports WHERE id = ?1", [id], |r| r.get(0))).unwrap();
        assert_eq!(db.with(|c| purge(c, expires - 1)).unwrap(), Vec::<String>::new());
        let gone = db.with(|c| purge(c, expires)).unwrap();
        assert_eq!(gone, vec![file.clone()]);
        remove_files(&dir, &gone);
        assert!(!dir.join(&file).exists());
        assert_eq!(db.with(|c| status(c, ann, expires)).unwrap().unwrap().state, "expired");
        assert_eq!(db.with(|c| purge(c, made + 1 + KEEP_DAYS * 86_400)).unwrap(), Vec::<String>::new());
        assert_eq!(db.with(|c| status(c, ann, made + 1 + KEEP_DAYS * 86_400)).unwrap(), None);

        std::fs::write(dir.join("stray.json"), "{}").unwrap();
        std::fs::write(dir.join(&file), "{}").unwrap(); // the expired one's name, no longer on its row
        assert_eq!(remove_strays(&db, &dir), 2);
        assert!(!dir.join("stray.json").exists());
        assert_eq!(file_path(&dir, "../etc/passwd"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The housekeeping reads `data_exports` through its indexes, not the
    /// whole table (#626).
    #[test]
    fn export_housekeeping_uses_an_index() {
        let db = Db::open_in_memory().unwrap();
        db.with(|c| {
            for sql in [PURGE_FILES, PURGE_OLD, KNOWN_FILES, PURGE_EXPIRED] {
                let mut stmt = c.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?;
                let n = stmt.parameter_count();
                let plan: Vec<String> = stmt.query_map(rusqlite::params_from_iter(std::iter::repeat(1i64).take(n)), |r| r.get(3))?.collect::<Result<_, _>>()?;
                let plan = plan.join("\n");
                assert!(plan.lines().all(|l| !l.starts_with("SCAN") || l.contains("idx_data_exports_files")), "{sql} scans data_exports:\n{plan}");
            }
            Ok(())
        })
        .unwrap();
    }

    /// Deleting the account before its export runs leaves no file behind
    /// and sends no mail.
    #[test]
    fn a_deleted_account_leaves_no_export() {
        let db = Db::open_in_memory().unwrap();
        let dir = tmp_dir("deleted");
        let ann = user(&db, "ann@example.org");
        db.with(|c| request(c, ann, "http://x", 10)).unwrap();
        db.with(|c| privacy::delete_account(c, ann))
            .unwrap()
            .unwrap();
        let (woken, wake) = counter();
        assert_eq!(run_queued(&db, &dir, wake), 1);
        assert!(!dir.exists() || std::fs::read_dir(&dir).unwrap().count() == 0);
        assert_eq!(woken.load(Ordering::SeqCst), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
