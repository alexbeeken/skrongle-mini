//! "Download my data" as a background job (#430, #365).
//!
//! - The profile's button queues a row in `data_exports` ([`request`]): one
//!   export in flight per account, and one a day ([`COOLDOWN_SECS`]).
//! - The request also queues a `jobs` row of kind `export` (#875), and the
//!   shared runner (`jobs.rs`, one OS thread) builds the file: the export's
//!   header, then each table of `privacy::USER_TABLES` a page at a time
//!   ([`privacy::page`]), each page one short call on the connection with a
//!   pause after it, so other requests get it in between. Rows go straight
//!   to a file in the exports folder (next to the database,
//!   `<db>.exports/`); the account is never held in memory.
//! - A run writes at most [`SLICE_PAGES`] pages, saves its place in the job's
//!   payload ([`Place`]: table, last key, good bytes) and queues itself
//!   again, so mail and housekeeping never wait behind a big export. A
//!   server that stops mid-slice takes the row again when its lease runs
//!   out (after a restart), cuts the half-written tail off and goes on.
//!   No boot step: the lease is the recovery. Files no row names are
//!   deleted by the hourly sweep ([`remove_strays`]).
//! - When the file is complete the profile's Data section shows a Download
//!   button (`POST /profile/export/download`, [`take_ready`]), and, when the
//!   server can send mail, the learner is e-mailed a one-time link
//!   (`/profile/export/download?token=…`): 32 random bytes, stored only as
//!   their SHA-256. Either works for [`LINK_TTL_SECS`] and only for the
//!   account signed in. Downloading deletes the file; so does expiry (hourly
//!   housekeeping, [`purge`]) and deleting the account.
//! - Where the finished file lives is [`Storage`] (#871): it stays in the
//!   exports folder on Skrongle mini and the dev server; on the hosted site
//!   it is uploaded to the S3 bucket under `exports/` and the local copy
//!   deleted, and the download redirects to a pre-signed link
//!   ([`Exports::link`]) once the session and the one-time link check out.
//! - The file has the same content the synchronous export had: the columns
//!   `privacy::EXPORTED` lists. Pages are read at different moments, so an
//!   edit made while it is built may show in one table and not another.

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::db::Db;
use crate::storage::Storage;
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
/// The pause after each page, so a big export leaves the database to others.
const PAUSE: Duration = Duration::from_millis(15);
/// Pages per run of the `export` job; then it queues itself again, behind
/// any job already due.
const SLICE_PAGES: usize = 50;

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
    crate::verify::hash_token(token)
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

/// The object key of a finished export in the bucket.
pub fn s3_key(file: &str) -> String {
    format!("exports/{file}")
}

/// Delete files from the exports folder (names from [`take`], [`purge`] or
/// account deletion). A missing file is fine. On the hosted site use
/// [`Exports::remove`], which deletes the bucket's copies too.
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

// ---------- building, a slice per job run ----------

/// Where a build stands, kept in its `jobs` row's payload between slices.
/// Before the first slice only `export` is set.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Place {
    /// The `data_exports` row.
    pub export: i64,
    /// The finished file's name in the exports folder (`<id>-<random>.json`);
    /// it is written as `<file>.part` and renamed when complete.
    #[serde(default)]
    pub file: String,
    /// The table of `privacy::USER_TABLES` being written; its opening
    /// bracket is in the file.
    #[serde(default)]
    pub table: usize,
    /// The ordering key of the last row written ([`privacy::After`], as JSON).
    #[serde(default)]
    pub after: Vec<Value>,
    /// Rows of `table` written.
    #[serde(default)]
    pub written: usize,
    /// Bytes of the part file that are good. A slice cut short by a stop
    /// wrote past this; the next one cuts the file back to it.
    #[serde(default)]
    pub bytes: u64,
}

fn key_to_json(v: &rusqlite::types::Value) -> Value {
    use rusqlite::types::Value as V;
    match v {
        V::Null => Value::Null,
        V::Integer(n) => Value::from(*n),
        V::Real(f) => Value::from(*f),
        V::Text(t) => Value::String(t.clone()),
        V::Blob(b) => serde_json::json!({ "blob": b }),
    }
}

fn key_from_json(v: &Value) -> rusqlite::types::Value {
    use rusqlite::types::Value as V;
    match v {
        Value::Number(n) => n.as_i64().map(V::Integer).unwrap_or_else(|| V::Real(n.as_f64().unwrap_or(0.0))),
        Value::String(s) => V::Text(s.clone()),
        Value::Object(o) => V::Blob(o.get("blob").and_then(Value::as_array).map(|a| a.iter().filter_map(|b| b.as_u64().map(|b| b as u8)).collect()).unwrap_or_default()),
        _ => V::Null,
    }
}

/// Is the export still wanted (the row, and so the account, still there)?
fn wanted(db: &Db, id: i64) -> bool {
    db.with(|c| c.query_row("SELECT EXISTS(SELECT 1 FROM data_exports WHERE id = ?1)", [id], |r| r.get(0))).unwrap_or(false)
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

/// The `export` job's handler: builds files a slice at a time.
pub struct Builder {
    /// The exports folder.
    pub dir: PathBuf,
    /// Wakes the mail sender. `None` on a server that cannot send mail: no
    /// mail, the profile's Download button is the way to the file.
    pub wake_mail: Option<Box<dyn Fn() + Send>>,
    /// Pages per slice ([`SLICE_PAGES`]).
    pub slice_pages: usize,
    /// The pause after each page ([`PAUSE`]).
    pub pause: Duration,
    /// Where the finished file goes ([`Storage::Disk`]: it stays in `dir`).
    pub storage: Arc<Storage>,
}

impl Builder {
    pub fn new(dir: PathBuf, wake_mail: Option<Box<dyn Fn() + Send>>) -> Builder {
        Builder { dir, wake_mail, slice_pages: SLICE_PAGES, pause: PAUSE, storage: Arc::new(Storage::Disk) }
    }

    /// One run of the `export` job: one slice of the export its payload names.
    pub fn run(&self, db: &Db, payload: &str) -> Result<jobs::Step, String> {
        let mut place: Place = serde_json::from_str(payload).map_err(|e| format!("bad payload: {e}"))?;
        let id = place.export;
        let row: Option<(i64, String)> = db
            .with(|c| c.query_row("SELECT user_id, state FROM data_exports WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).optional())
            .map_err(|e| e.to_string())?;
        let gone = |place: &Place| {
            self.remove(place);
            Ok(jobs::Step::Done(format!("export #{id}: the account went while it was built; file deleted")))
        };
        let Some((user_id, state)) = row else { return gone(&place) };
        if !matches!(state.as_str(), "queued" | "building") {
            // The server stopped after the export was finished but before the job was.
            return Ok(jobs::Step::Done(String::new()));
        }
        match self.slice(db, &mut place, user_id) {
            Ok(false) => {
                let note = format!("export #{id}: table {} of {}, {} bytes", place.table + 1, privacy::USER_TABLES.len(), place.bytes);
                let payload = serde_json::to_string(&place).map_err(|e| e.to_string())?;
                Ok(jobs::Step::Again { payload, note })
            }
            Ok(true) => {
                let how = match &self.wake_mail {
                    Some(wake) => {
                        wake();
                        "link mailed"
                    }
                    None => "on the profile (no mail)",
                };
                Ok(jobs::Step::Done(format!("export #{id} for user {user_id} ready ({} bytes); {how}", place.bytes)))
            }
            Err(Stop::Gone) => gone(&place),
            Err(Stop::Failed(why)) => {
                self.remove(&place);
                let _ = db.with(|c| c.execute("UPDATE data_exports SET state = 'failed', error = ?2, file = '' WHERE id = ?1", params![id, why]));
                Err(format!("export #{id} for user {user_id} failed: {why}"))
            }
        }
    }

    /// Delete an unfinished build's files.
    fn remove(&self, place: &Place) {
        if !place.file.is_empty() {
            remove_files(&self.dir, &[format!("{}.part", place.file), place.file.clone()]);
        }
    }

    /// Write up to `slice_pages` pages of the file, then save the place in
    /// `place`. `true` when the file is complete and the export ready (its
    /// mail queued).
    fn slice(&self, db: &Db, place: &mut Place, user_id: i64) -> Result<bool, Stop> {
        let id = place.export;
        let tables = privacy::USER_TABLES;
        let mut out = if place.file.is_empty() {
            // The first slice: name the file on the row first, so the stray
            // sweep and account deletion know it, then start it.
            place.file = format!("{id}-{}.json", &crate::auth::generate_token()[..16]); // panic-ok: the token is 64 hex digits
            let part = format!("{}.part", place.file);
            let named = db.with(|c| c.execute("UPDATE data_exports SET state = 'building', file = ?2 WHERE id = ?1", params![id, part]))?;
            if named == 0 {
                return Err(Stop::Gone);
            }
            std::fs::create_dir_all(&self.dir)?;
            let Some(head) = db.with(|c| privacy::export_header(c, user_id))? else { return Err(Stop::Gone) };
            let mut out = BufWriter::with_capacity(64 * 1024, File::create(self.dir.join(&part))?);
            out.write_all(b"{")?;
            for (k, v) in &head {
                json(&mut out, &Value::String(k.clone()))?;
                out.write_all(b":")?;
                json(&mut out, v)?;
                out.write_all(b",\n")?;
            }
            out.write_all(b"\"tables\":{")?;
            if let Some((t, _)) = tables.first() {
                open_table(&mut out, t, true)?;
            }
            out
        } else {
            if self.dir.join(&place.file).exists() {
                // Stopped between the rename and marking it ready.
                return self.done(db, place).map(|()| true);
            }
            let f = std::fs::OpenOptions::new().write(true).open(self.dir.join(format!("{}.part", place.file)))?;
            f.set_len(place.bytes)?;
            let mut out = BufWriter::with_capacity(64 * 1024, f);
            out.seek(SeekFrom::Start(place.bytes))?;
            out
        };
        let mut after: privacy::After = place.after.iter().map(key_from_json).collect();
        let mut pages = 0;
        while let (Some((table, _)), true) = (tables.get(place.table), pages < self.slice_pages) {
            let (rows, last) = db.with(|c| privacy::page(c, table, user_id, &after, PAGE_ROWS, PAGE_BYTES))?;
            for (j, row) in rows.iter().enumerate() {
                if place.written + j > 0 {
                    out.write_all(b",")?;
                }
                out.write_all(b"\n")?;
                json(&mut out, row)?;
            }
            place.written += rows.len();
            pages += 1;
            // A page can end early on its byte budget: the table is done when one comes back empty.
            if rows.is_empty() {
                out.write_all(b"]")?;
                place.table += 1;
                place.written = 0;
                after = Vec::new();
                if let Some((t, _)) = tables.get(place.table) {
                    open_table(&mut out, t, false)?;
                }
            } else {
                after = last;
            }
            drop(rows);
            std::thread::sleep(self.pause);
            if !wanted(db, id) {
                return Err(Stop::Gone);
            }
        }
        let complete = place.table >= tables.len();
        if complete {
            out.write_all(b"\n}}\n")?;
        }
        let file = out.into_inner().map_err(|e| Stop::Failed(e.to_string()))?;
        file.sync_all()?;
        place.bytes = file.metadata()?.len();
        place.after = after.iter().map(key_to_json).collect();
        if !complete {
            return Ok(false);
        }
        drop(file);
        std::fs::rename(self.dir.join(format!("{}.part", place.file)), self.dir.join(&place.file))?;
        self.done(db, place).map(|()| true)
    }

    /// The file is complete and in place: upload it when the files live in
    /// the bucket, mark the export ready and queue its mail. The local copy
    /// of an uploaded file goes once the row is ready; a stop before that
    /// leaves it named on the row, for [`purge`].
    fn done(&self, db: &Db, place: &mut Place) -> Result<(), Stop> {
        let local = self.dir.join(&place.file);
        place.bytes = std::fs::metadata(&local)?.len();
        if let Some(bucket) = self.storage.bucket() {
            bucket.put_file(&s3_key(&place.file), &local, crate::db::now()).map_err(Stop::Failed)?;
        }
        let token = crate::auth::generate_token();
        if !db.with(|c| finish(c, place.export, &place.file, &token, place.bytes as i64, crate::db::now(), self.wake_mail.is_some()))? {
            if let Some(bucket) = self.storage.bucket() {
                let _ = bucket.delete(&s3_key(&place.file), crate::db::now());
            }
            return Err(Stop::Gone);
        }
        if self.storage.bucket().is_some() {
            remove_files(&self.dir, std::slice::from_ref(&place.file));
        }
        Ok(())
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
        // `<file>` of a row naming `<file>.part`: a stop between the rename
        // and "ready"; the next run marks it ready.
        let renamed = known.contains(&format!("{name}.part"));
        if !known.contains(&name) && !renamed && std::fs::remove_file(e.path()).is_ok() {
            n += 1;
        }
    }
    n
}

/// The exports folder and where finished files live, kept in `AppState`.
pub struct Exports {
    pub dir: PathBuf,
    pub storage: Arc<Storage>,
}

impl Exports {
    /// The folder next to the database, `<db>.exports`, made if missing.
    pub fn open(db_path: &Path, storage: Arc<Storage>) -> std::io::Result<Exports> {
        let mut s = db_path.as_os_str().to_owned();
        s.push(".exports");
        let dir = PathBuf::from(s);
        std::fs::create_dir_all(&dir)?;
        Ok(Exports { dir, storage })
    }

    /// Delete finished or half-built exports ([`take`], [`purge`], account
    /// deletion): the local files, and on the hosted site the bucket's
    /// copies, on a thread of their own (a call may wait on the network).
    /// One the server misses goes with the bucket's two-day rule.
    pub fn remove(&self, files: &[String]) {
        remove_files(&self.dir, files);
        self.remove_uploaded(files.to_vec(), Duration::ZERO);
    }

    /// Delete the bucket's copies of `files` after `delay` (nothing on disk storage).
    fn remove_uploaded(&self, files: Vec<String>, delay: Duration) {
        let files: Vec<String> = files.into_iter().filter(|f| !f.is_empty() && !f.ends_with(".part")).collect();
        if self.storage.bucket().is_none() || files.is_empty() {
            return;
        }
        let storage = Arc::clone(&self.storage);
        let spawned = std::thread::Builder::new().name("export-delete".into()).spawn(move || {
            std::thread::sleep(delay);
            if let Some(bucket) = storage.bucket() {
                for f in &files {
                    if let Err(e) = bucket.delete(&s3_key(f), crate::db::now()) {
                        tracing::warn!("exports: {e} (the bucket's two-day rule deletes it)");
                    }
                }
            }
        });
        if let Err(e) = spawned {
            tracing::warn!("exports: no thread to delete uploaded files: {e}");
        }
    }

    /// On the hosted site, the download of `file` (from [`take`] or
    /// [`take_ready`]): a pre-signed link to the bucket's copy that works
    /// for [`crate::storage::LINK_SECS`], and the copy deleted a little after
    /// it stops working. `None` on disk storage (the caller streams the file)
    /// and `Some(Err)` when the link can't be signed.
    pub async fn link(&self, file: &str) -> Option<Result<String, String>> {
        self.storage.bucket()?;
        let storage = Arc::clone(&self.storage);
        let key = s3_key(file);
        let (tx, rx) = tokio::sync::oneshot::channel();
        // Signing may fetch the instance role's credentials first: off the async threads.
        let spawned = std::thread::Builder::new().name("export-link".into()).spawn(move || {
            let disposition = format!("attachment; filename=\"{}-my-data.json\"", crate::APP_NAME.to_lowercase());
            let extra = [("response-content-disposition", disposition.as_str()), ("response-content-type", "application/json; charset=utf-8")];
            let link = storage.bucket().ok_or_else(|| "no bucket".to_string()).and_then(|b| b.presign_get(&key, crate::storage::LINK_SECS, &extra, crate::db::now()));
            let _ = tx.send(link);
        });
        if let Err(e) = spawned {
            return Some(Err(e.to_string()));
        }
        let link = rx.await.unwrap_or_else(|_| Err("the signing thread stopped".to_string()));
        self.remove_uploaded(vec![file.to_string()], Duration::from_secs(crate::storage::LINK_SECS as u64 * 5));
        Some(link)
    }
}

/// Build every due export now, slice after slice, on this thread (tests
/// stand in for the `jobs` thread with it). How many exports finished.
#[cfg(test)]
pub fn run_queued(db: &Db, dir: &Path, wake_mail: Option<Box<dyn Fn() + Send>>, pause: Duration) -> usize {
    let builder = Builder { pause, ..Builder::new(dir.to_path_buf(), wake_mail) };
    let runner = jobs::Runner::new().handle_steps(jobs::EXPORT, move |db, p| builder.run(db, p));
    let mut n = 0;
    while let Ok(Some((_, outcome))) = runner.run_one(db, crate::db::now()) {
        if outcome != jobs::Outcome::Again {
            n += 1;
        }
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
        assert_eq!(run_queued(&db, &dir, wake, Duration::ZERO), 1);
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
        run_queued(&db, &dir, counter().1, Duration::ZERO);
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

    /// A server that stopped between renaming the finished file and marking
    /// the export ready: the row still names `<file>.part`. The hourly sweep
    /// (due at boot) keeps the finished file, and the next run marks it ready.
    #[test]
    fn a_stop_after_the_rename_survives_the_stray_sweep() {
        let db = Db::open_in_memory().unwrap();
        let dir = tmp_dir("renamed");
        let ann = user(&db, "ann@example.org");
        let Requested::Queued(id) = db.with(|c| request(c, ann, "http://x", crate::db::now())).unwrap() else { panic!() };
        run_queued(&db, &dir, counter().1, Duration::ZERO);
        let file: String = db.with(|c| c.query_row("SELECT file FROM data_exports WHERE id = ?1", [id], |r| r.get(0))).unwrap();
        // Back to just after the rename: building, the row naming the part file, the job leased.
        let payload = serde_json::json!({ "export": id, "file": file, "table": privacy::USER_TABLES.len(), "bytes": 1 }).to_string();
        db.with(|c| {
            c.execute("UPDATE data_exports SET state = 'building', file = ?2, token_hash = '' WHERE id = ?1", params![id, format!("{file}.part")])?;
            c.execute("UPDATE jobs SET state = 'queued', payload_json = ?1 WHERE kind = 'export'", [payload])?;
            Ok(())
        })
        .unwrap();
        assert_eq!(remove_strays(&db, &dir), 0, "the finished file of a building export is not a stray");
        assert!(dir.join(&file).exists());
        assert_eq!(run_queued(&db, &dir, counter().1, Duration::ZERO), 1);
        assert_eq!(db.with(|c| status(c, ann, crate::db::now())).unwrap().unwrap().state, "ready");
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

    /// Deleting the account while its export is built leaves no file
    /// behind and sends no mail.
    #[test]
    fn a_deleted_account_leaves_no_export() {
        let db = Db::open_in_memory().unwrap();
        let dir = tmp_dir("deleted");
        let ann = user(&db, "ann@example.org");
        db.with(|c| request(c, ann, "http://x", 10)).unwrap();
        let (woken, wake) = counter();
        let builder = Builder { slice_pages: 1, pause: Duration::ZERO, ..Builder::new(dir.clone(), wake) };
        let runner = jobs::Runner::new().handle_steps(jobs::EXPORT, move |db, p| builder.run(db, p));
        let now = crate::db::now() + 1000; // ahead of the clock, so a requeued slice is due at once
        assert_eq!(runner.run_one(&db, now).unwrap().unwrap().1, jobs::Outcome::Again);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "the part file");
        let deleted = db.with(|c| privacy::delete_account(c, ann)).unwrap().unwrap();
        assert_eq!(deleted.export_files.len(), 1);
        assert!(deleted.export_files[0].ends_with(".json.part"));
        assert_eq!(runner.run_one(&db, now).unwrap().unwrap().1, jobs::Outcome::Done);
        assert_eq!(runner.run_one(&db, now).unwrap(), None);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        assert_eq!(woken.load(Ordering::SeqCst), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// One page per slice, and the server stops halfway through a slice
    /// (bytes written past the saved place, the row still leased). After
    /// the lease runs out the next run cuts the tail off and goes on: the
    /// file is the same as the one-shot export, made once, mailed once.
    /// Other jobs due meanwhile run between slices.
    #[test]
    fn a_sliced_export_survives_a_stop_and_lets_other_jobs_in() {
        let db = Db::open_in_memory().unwrap();
        let dir = tmp_dir("slices");
        let ann = user(&db, "ann@example.org");
        db.with(|c| {
            for i in 0..(PAGE_ROWS + 3) {
                c.execute(
                    "INSERT INTO quiz_history (user_id, course_id, lesson_id, question_id, selected_answer, was_correct, answered_at) VALUES (?1, 'spanish', 'a1', ?2, 'x', 1, 1)",
                    params![ann, format!("q{i}")],
                )?;
            }
            Ok(())
        })
        .unwrap();
        let Requested::Queued(id) = db.with(|c| request(c, ann, "http://site", 10)).unwrap() else { panic!() };
        let (woken, wake) = counter();
        let builder = Builder { slice_pages: 1, pause: Duration::ZERO, ..Builder::new(dir.clone(), wake) };
        let runner = jobs::Runner::new().handle_steps(jobs::EXPORT, move |db, p| builder.run(db, p)).handle(jobs::GAMES, |_, _| Ok(String::new()));
        let now = crate::db::now() + 1000; // ahead of the clock, so a requeued slice is due at once
        let (job, outcome) = runner.run_one(&db, now).unwrap().unwrap();
        assert_eq!(outcome, jobs::Outcome::Again);
        assert_eq!(db.with(|c| status(c, ann, now)).unwrap().unwrap().state, "building");
        // A job queued meanwhile (mail, say) goes before the next slice.
        let other = db.with(|c| jobs::enqueue(c, jobs::GAMES.name, &serde_json::json!({}), now - 1, now)).unwrap().unwrap();
        assert_eq!(runner.run_one(&db, now).unwrap().unwrap().0.id, other);
        // A few slices on, the server stops mid-slice: the row stays leased
        // and the part file has a half-written tail.
        for _ in 0..3 {
            assert_eq!(runner.run_one(&db, now).unwrap().unwrap().1, jobs::Outcome::Again);
        }
        let payload: String = db.with(|c| c.query_row("SELECT payload_json FROM jobs WHERE id = ?1", [job.id], |r| r.get(0))).unwrap();
        let place: Place = serde_json::from_str(&payload).unwrap();
        assert!(place.bytes > 0 && !place.file.is_empty());
        db.with(|c| jobs::claim(c, &[jobs::EXPORT.name], "dead-server", now)).unwrap().unwrap();
        let part = dir.join(format!("{}.part", place.file));
        std::fs::OpenOptions::new().append(true).open(&part).unwrap().write_all(b",\n{\"half\":").unwrap();
        assert_eq!(runner.run_one(&db, now).unwrap(), None, "leased to the stopped server");
        let mut t = now + jobs::LEASE;
        while let Some((_, outcome)) = runner.run_one(&db, t).unwrap() {
            t += 1;
            if outcome == jobs::Outcome::Done {
                break;
            }
        }
        assert_eq!(runner.run_one(&db, t).unwrap(), None);
        let (file, state): (String, String) = db.with(|c| c.query_row("SELECT file, state FROM data_exports WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))).unwrap();
        assert_eq!((state.as_str(), file.as_str()), ("ready", place.file.as_str()));
        assert!(!part.exists());
        let v: Value = serde_json::from_str(&std::fs::read_to_string(dir.join(&file)).unwrap()).unwrap();
        let mut whole = db.with(|c| privacy::export(c, ann)).unwrap().unwrap();
        whole["exported_at"] = v["exported_at"].clone();
        whole["tables"]["data_exports"] = v["tables"]["data_exports"].clone();
        whole["tables"]["mail_outbox"] = v["tables"]["mail_outbox"].clone();
        assert_eq!(whole, v);
        assert_eq!(v["tables"]["quiz_history"].as_array().unwrap().len(), PAGE_ROWS + 3);
        assert_eq!(woken.load(Ordering::SeqCst), 1);
        let mails: i64 = db.with(|c| c.query_row("SELECT COUNT(*) FROM mail_outbox WHERE purpose = ?1", [PURPOSE], |r| r.get(0))).unwrap();
        assert_eq!(mails, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// On the hosted site the finished file goes to the bucket under
    /// `exports/` and the local copy is deleted; the download is a short
    /// pre-signed link to it (#871).
    #[tokio::test]
    async fn on_the_bucket_the_export_is_uploaded_and_linked() {
        let (endpoint, seen) = crate::storage::tests::stand_in();
        let creds = crate::storage::s3::Creds { key_id: "AKID".into(), secret: "secret".into(), token: None };
        let storage = Arc::new(Storage::S3(Box::new(crate::storage::s3::Bucket::at(&endpoint, "us-west-2", creds))));
        let db = Db::open_in_memory().unwrap();
        let dir = tmp_dir("bucket");
        let ann = user(&db, "ann@example.org");
        let Requested::Queued(id) = db.with(|c| request(c, ann, "http://x", 10)).unwrap() else { panic!() };
        let builder = Builder { pause: Duration::ZERO, storage: Arc::clone(&storage), ..Builder::new(dir.clone(), None) };
        let runner = jobs::Runner::new().handle_steps(jobs::EXPORT, move |db, p| builder.run(db, p));
        while runner.run_one(&db, crate::db::now()).unwrap().is_some() {}
        let (file, state): (String, String) = db.with(|c| c.query_row("SELECT file, state FROM data_exports WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))).unwrap();
        assert_eq!(state, "ready");
        assert!(!dir.join(&file).exists(), "the local copy is deleted once uploaded");
        let put = seen.lock().unwrap().iter().find(|h| h[":method"] == "PUT").cloned().expect("uploaded");
        assert!(put.contains_key("authorization"));
        let bucket = storage.bucket().unwrap();
        let v: Value = serde_json::from_slice(&bucket.get(&s3_key(&file), crate::db::now()).unwrap()).unwrap();
        assert_eq!(v["user_id"], ann);

        let exports = Exports { dir: dir.clone(), storage };
        let link = exports.link(&file).await.unwrap().unwrap();
        assert!(link.starts_with(&format!("{endpoint}/exports/{file}?")), "{link}");
        assert!(link.contains("X-Amz-Expires=60&") && link.contains("response-content-disposition=attachment"), "{link}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The place survives the payload's trip through JSON, keys of every type.
    #[test]
    fn ordering_keys_round_trip() {
        use rusqlite::types::Value as V;
        let keys = vec![V::Integer(-7), V::Text("lessons/a.json".into()), V::Real(1.5), V::Null, V::Blob(vec![0, 255])];
        let json: Vec<Value> = keys.iter().map(key_to_json).collect();
        let back: Vec<Value> = serde_json::from_str(&serde_json::to_string(&json).unwrap()).unwrap();
        assert_eq!(back.iter().map(key_from_json).collect::<Vec<_>>(), keys);
    }
}
