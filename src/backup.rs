//! The owner's backup from the Class box page (#227, `docs/self-hosting.md`
//! "Backup and restore"): one `.tar.gz` that restores with the documented
//! command (`tar -xzf … -C /var/lib/skrongle`). It holds what the documented
//! backup holds: the database, as a consistent copy, and the key file next
//! to it (`secrets::key_file_path`) when there is one.
//!
//! - The database is copied with `VACUUM INTO` through `Db::call` into a
//!   temporary file beside it (the service may write only there,
//!   `ProtectSystem=strict`): a consistent snapshot while the class works,
//!   never a raw read of the live file and its journal.
//! - The archive is written by its own OS thread (not the Tokio blocking
//!   pool), through gzip, in chunks of [`CHUNK`] bytes into a channel of
//!   [`IN_FLIGHT`] chunks that the response body reads. A slow download
//!   makes the thread wait: memory stays at a few chunks, whatever the size
//!   of the database.
//! - The snapshot is deleted when the thread ends: done, failed, or the
//!   download stopped. A snapshot left by a crash is deleted by the next
//!   backup. One backup runs at a time.
//! - When the whole archive was handed to the connection, the time is kept
//!   in `settings` ([`LAST_BACKUP`]) for "Last backup: N days ago".

use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use rusqlite::Connection;

/// `settings` row: the Unix time the last backup was handed over in full.
pub const LAST_BACKUP: &str = "last_backup_at";

/// The size of one piece of the response body.
pub const CHUNK: usize = 64 * 1024;
/// Pieces written but not yet sent; the writer waits past it.
pub const IN_FLIGHT: usize = 4;

/// When the last backup was made; `None` when none was.
pub fn last_backup(conn: &Connection) -> rusqlite::Result<Option<i64>> {
    Ok(crate::settings::get(conn, LAST_BACKUP)?.and_then(|v| v.parse().ok()))
}

/// Keep `at` as the time of the last backup.
pub fn set_last_backup(conn: &Connection, at: i64) -> rusqlite::Result<()> {
    crate::settings::set(conn, LAST_BACKUP, &at.to_string())
}

/// The file name a download is saved as: the date in the owner's local day.
pub fn file_name(now: i64, tz: i64) -> String {
    let (y, m, d) = crate::profile::civil(now + tz);
    format!("skrongle-backup-{y:04}-{m:02}-{d:02}.tar.gz")
}

// ---------- one at a time ----------

static RUNNING: AtomicBool = AtomicBool::new(false);

/// The one backup that may run; dropping it lets the next one start.
pub struct Turn(&'static AtomicBool);

impl Turn {
    /// `None` while another backup is being made.
    pub fn take() -> Option<Turn> {
        Turn::take_from(&RUNNING)
    }

    fn take_from(flag: &'static AtomicBool) -> Option<Turn> {
        flag.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).ok().map(|_| Turn(flag))
    }
}

impl Drop for Turn {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

// ---------- the snapshot ----------

/// The temporary copy's name prefix, beside the database: `.learn.db.backup-`.
fn temp_prefix(db_path: &Path) -> String {
    let name = db_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "learn.db".into());
    format!(".{name}.backup-")
}

/// A fresh path for the snapshot, beside the database, after deleting any
/// snapshot an earlier backup left (only one runs at a time, so any there is
/// left over from a crash).
pub fn temp_path(db_path: &Path) -> PathBuf {
    let dir = dir_of(db_path);
    let prefix = temp_prefix(db_path);
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with(&prefix) && name.ends_with(".tmp") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    dir.join(format!("{prefix}{:016x}.tmp", crate::random::u64()))
}

/// The folder that holds the database ("." for a bare file name).
fn dir_of(db_path: &Path) -> PathBuf {
    match db_path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

/// Write a consistent copy of the open database to `to` (which must not
/// exist): SQLite's `VACUUM INTO`, a read transaction, so the class goes on
/// writing to the live file meanwhile.
pub fn snapshot(conn: &Connection, to: &Path) -> rusqlite::Result<()> {
    conn.execute("VACUUM INTO ?1", [to.to_string_lossy()])?;
    Ok(())
}

/// What goes in the archive: (name in the archive, file on disk). The
/// database under its own name, then its key file when there is one.
pub fn entries(db_path: &Path, snapshot: &Path) -> Vec<(String, PathBuf)> {
    let name = db_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "learn.db".into());
    let mut out = vec![(name, snapshot.to_path_buf())];
    let key = crate::secrets::key_file_path(db_path);
    if key.is_file() {
        let key_name = key.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        out.push((key_name, key));
    }
    out
}

// ---------- the archive ----------

/// Write `entries` as a gzipped tar to `out`, each file read in small pieces.
/// Plain files only, mode 0600, owned by root: `sudo tar -x` restores them
/// readable by root alone, and systemd hands them to the service when it
/// starts (`StateDirectory`).
pub fn write_archive<W: Write>(out: W, entries: &[(String, PathBuf)], mtime: i64) -> io::Result<W> {
    let mut gz = flate2::write::GzEncoder::new(out, flate2::Compression::fast());
    for (name, path) in entries {
        let mut file = File::open(path)?;
        let size = file.metadata()?.len();
        gz.write_all(&tar_header(name, size, mtime)?)?;
        let copied = io::copy(&mut (&mut file).take(size), &mut gz)?;
        if copied != size {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, format!("{name} shrank while it was read")));
        }
        let pad = (512 - (size % 512) as usize) % 512;
        gz.write_all([0u8; 512].get(..pad).unwrap_or_default())?;
    }
    // The end of the archive: two empty blocks.
    gz.write_all(&[0u8; 1024])?;
    gz.finish()
}

/// Put `bytes` into `block` from `at` on.
fn put(block: &mut [u8; 512], at: usize, bytes: &[u8]) {
    for (slot, b) in block.iter_mut().skip(at).zip(bytes) {
        *slot = *b;
    }
}

/// `n` as `width - 1` octal digits and a NUL, as tar numbers are.
fn octal(n: u64, width: usize) -> Vec<u8> {
    let mut v = format!("{n:0w$o}", w = width - 1).into_bytes();
    v.push(0);
    v
}

/// One ustar header for a plain file.
fn tar_header(name: &str, size: u64, mtime: i64) -> io::Result<[u8; 512]> {
    if name.is_empty() || name.len() > 100 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("a file name tar cannot hold: {name:?}")));
    }
    if size >= 8 << 30 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "a file of 8 GB or more"));
    }
    let mut h = [0u8; 512];
    put(&mut h, 0, name.as_bytes());
    put(&mut h, 100, &octal(0o600, 8));
    put(&mut h, 108, &octal(0, 8));
    put(&mut h, 116, &octal(0, 8));
    put(&mut h, 124, &octal(size, 12));
    put(&mut h, 136, &octal(mtime.max(0) as u64, 12));
    put(&mut h, 148, b"        ");
    put(&mut h, 156, b"0");
    put(&mut h, 257, b"ustar\0");
    put(&mut h, 263, b"00");
    put(&mut h, 265, b"root");
    put(&mut h, 297, b"root");
    let sum: u64 = h.iter().map(|b| u64::from(*b)).sum();
    let mut check = format!("{sum:06o}").into_bytes();
    check.extend_from_slice(b"\0 ");
    put(&mut h, 148, &check);
    Ok(h)
}

// ---------- the stream ----------

/// A `Write` that hands its bytes on in pieces of [`CHUNK`] through a
/// bounded channel, waiting while [`IN_FLIGHT`] pieces are unsent. The
/// receiver gone (the download stopped) is an error, which ends the archive.
pub struct Chunks {
    tx: tokio::sync::mpsc::Sender<io::Result<Vec<u8>>>,
    buf: Vec<u8>,
}

impl Chunks {
    pub fn new(tx: tokio::sync::mpsc::Sender<io::Result<Vec<u8>>>) -> Chunks {
        Chunks { tx, buf: Vec::with_capacity(CHUNK) }
    }

    fn send(&mut self) -> io::Result<()> {
        if self.buf.is_empty() {
            return Ok(());
        }
        let piece = std::mem::replace(&mut self.buf, Vec::with_capacity(CHUNK));
        self.tx.blocking_send(Ok(piece)).map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "the download stopped"))
    }

    /// Send what is left.
    pub fn end(mut self) -> io::Result<()> {
        self.send()
    }
}

impl Write for Chunks {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let room = CHUNK - self.buf.len();
        let n = data.len().min(room);
        self.buf.extend_from_slice(data.get(..n).unwrap_or_default());
        if self.buf.len() >= CHUNK {
            self.send()?;
        }
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The receiving end as a response body's stream.
pub struct Body(pub tokio::sync::mpsc::Receiver<io::Result<Vec<u8>>>);

impl futures_core::Stream for Body {
    type Item = io::Result<Vec<u8>>;

    fn poll_next(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Option<Self::Item>> {
        self.0.poll_recv(cx)
    }
}

/// Write the archive of `entries` into the channel on a thread of its own,
/// then delete `snapshot` and call `done` with whether every byte was handed
/// over. The `turn` is held until then.
pub fn spawn_writer(
    entries: Vec<(String, PathBuf)>,
    snapshot: PathBuf,
    mtime: i64,
    turn: Turn,
    done: impl FnOnce(bool) + Send + 'static,
) -> io::Result<Body> {
    let (tx, rx) = tokio::sync::mpsc::channel(IN_FLIGHT);
    let cleanup = snapshot.clone();
    let spawned = std::thread::Builder::new().name("backup".into()).spawn(move || {
        let _turn = turn;
        let failed = tx.clone();
        let result = write_archive(Chunks::new(tx), &entries, mtime).and_then(Chunks::end);
        if let Err(e) = &result {
            tracing::warn!("backup stopped: {e}");
            // The body ends in an error, so the client sees a broken download, not a short file.
            let _ = failed.blocking_send(Err(io::Error::new(e.kind(), e.to_string())));
        }
        drop(failed);
        let _ = std::fs::remove_file(&snapshot);
        done(result.is_ok());
    });
    match spawned {
        Ok(_) => Ok(Body(rx)),
        Err(e) => {
            let _ = std::fs::remove_file(&cleanup);
            Err(e)
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
