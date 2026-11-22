//! The backup archive (#227): it restores with the documented `tar -xzf`,
//! and a download in progress holds a few chunks, not the database.

use super::*;
use std::time::{Duration, Instant};

/// A fresh, empty folder of this test's own.
pub(crate) fn fresh_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("skrongle-backup-{name}-{}-{:x}", std::process::id(), crate::random::u64()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A database at `dir/learn.db` with its key file, an owner, a teacher
/// with a student, and a course of the teacher's given to the student.
pub(crate) fn class_db(dir: &Path, hash: &str) -> (crate::db::Db, PathBuf) {
    let path = dir.join("learn.db");
    let db = crate::db::Db::open(&path).unwrap();
    crate::secrets::Vault::load(None, &path).unwrap();
    db.with(|c| {
        let owner = crate::auth::create_user(c, "owner@example.org", hash)?;
        crate::owner::make_owner(c, owner)?;
        crate::students::tests::class_with_course(c, "t@example.org", "kid", 1, hash, "reading-1", "Reading, level 1")?;
        Ok(())
    })
    .unwrap();
    (db, path)
}

/// Unpack `archive` with the documented command into a fresh folder.
pub(crate) fn untar(archive: &[u8], name: &str) -> PathBuf {
    let into = fresh_dir(name);
    let file = into.with_extension("tar.gz");
    std::fs::write(&file, archive).unwrap();
    let out = std::process::Command::new("tar").arg("-xzf").arg(&file).arg("-C").arg(&into).output().unwrap();
    assert!(out.status.success(), "tar -xzf: {}", String::from_utf8_lossy(&out.stderr));
    let _ = std::fs::remove_file(&file);
    into
}

/// What a restored database holds: (accounts, students, user-made courses, given courses).
pub(crate) fn holds(path: &Path) -> (i64, i64, i64, i64) {
    let db = crate::db::Db::open(path).unwrap();
    db.with(|c| {
        c.query_row(
            "SELECT (SELECT COUNT(*) FROM users), (SELECT COUNT(*) FROM users WHERE teacher_id IS NOT NULL),
                    (SELECT COUNT(*) FROM courses), (SELECT COUNT(*) FROM course_shares WHERE given = 1)",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
    })
    .unwrap()
}

#[test]
fn an_archive_restores_with_the_documented_command() {
    let dir = fresh_dir("restore");
    let (db, path) = class_db(&dir, "h");
    let snap = temp_path(&path);
    assert!(snap.starts_with(&dir) && snap.file_name().unwrap().to_string_lossy().starts_with(".learn.db.backup-"));
    db.with(|c| snapshot(c, &snap)).unwrap();
    // The class goes on writing while the archive is made; the copy does not change.
    db.with(|c| crate::auth::create_user(c, "later@example.org", "h")).unwrap();

    let entries = entries(&path, &snap);
    assert_eq!(entries.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["learn.db", "learn.db.key"]);
    let archive = write_archive(Vec::new(), &entries, 1_700_000_000).unwrap();
    std::fs::remove_file(&snap).unwrap();

    let restored = untar(&archive, "restored");
    let mut names: Vec<String> = std::fs::read_dir(&restored).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    assert_eq!(names, ["learn.db", "learn.db.key"], "the two files of the documented backup, nothing else");
    assert_eq!(std::fs::read(restored.join("learn.db.key")).unwrap(), std::fs::read(dir.join("learn.db.key")).unwrap());
    assert_eq!(holds(&restored.join("learn.db")), (3, 1, 1, 1), "owner, teacher, student; the course, given");
    // The key opens on the restored database as the server opens it.
    assert!(crate::secrets::existing_vault(None, &restored.join("learn.db")).is_some(), "the restored key file reads");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(restored.join("learn.db")).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "readable by its owner only");
    }
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&restored);
}

#[test]
fn a_snapshot_left_by_a_crash_goes_with_the_next_backup() {
    let dir = fresh_dir("stale");
    let db_path = dir.join("learn.db");
    std::fs::write(dir.join(".learn.db.backup-0123.tmp"), b"left over").unwrap();
    std::fs::write(dir.join("learn.db.key"), b"key").unwrap();
    let fresh = temp_path(&db_path);
    assert!(!dir.join(".learn.db.backup-0123.tmp").exists());
    assert!(dir.join("learn.db.key").exists(), "only snapshots are deleted");
    assert!(!fresh.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn only_one_backup_runs_at_a_time() {
    static FLAG: AtomicBool = AtomicBool::new(false);
    let turn = Turn::take_from(&FLAG).expect("free");
    assert!(Turn::take_from(&FLAG).is_none());
    drop(turn);
    assert!(Turn::take_from(&FLAG).is_some());
}

/// A turn of a test's own, apart from the server's one.
fn own_turn() -> Turn {
    Turn::take_from(Box::leak(Box::new(AtomicBool::new(false)))).unwrap()
}

#[test]
fn the_file_is_named_by_the_local_day() {
    assert_eq!(file_name(1_763_510_400, 0), "skrongle-backup-2025-11-19.tar.gz");
    assert_eq!(file_name(1_763_510_400, -3600), "skrongle-backup-2025-11-18.tar.gz");
}

/// Bytes that do not compress: a gzip of them is as large as they are.
fn noise(len: usize) -> Vec<u8> {
    let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect()
}

/// A download that is not read holds `IN_FLIGHT` chunks: the writer waits
/// with the rest of a 16 MB database unread. Read, it all arrives; stopped,
/// the writer ends with an error and deletes its snapshot.
#[test]
fn a_download_holds_a_few_chunks_not_the_database() {
    const SIZE: usize = 16 << 20;
    let dir = fresh_dir("stream");
    let snap = dir.join(".learn.db.backup-test.tmp");
    std::fs::write(&snap, noise(SIZE)).unwrap();

    // Read it all, slowly at first.
    let (tx, rx) = std::sync::mpsc::channel();
    let mut body = spawn_writer(vec![("learn.db".into(), snap.clone())], snap.clone(), 0, own_turn(), move |ok| tx.send(ok).unwrap()).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(body.0.len(), IN_FLIGHT, "the writer waits once {IN_FLIGHT} chunks are unsent");
    assert!(rx.try_recv().is_err(), "and has not finished");
    let mut got = Vec::new();
    while let Some(piece) = body.0.blocking_recv() {
        let piece = piece.unwrap();
        assert!(piece.len() <= CHUNK);
        got.extend_from_slice(&piece);
    }
    assert!(rx.recv_timeout(Duration::from_secs(10)).unwrap(), "every byte was handed over");
    assert!(got.len() > SIZE, "the whole database, gzipped: {} bytes", got.len());
    assert!(!snap.exists(), "the snapshot is deleted");
    let restored = untar(&got, "stream-restored");
    assert_eq!(std::fs::metadata(restored.join("learn.db")).unwrap().len() as usize, SIZE);
    let _ = std::fs::remove_dir_all(&restored);

    // Stopped half way: the writer gives up and cleans up.
    std::fs::write(&snap, noise(SIZE)).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let mut body = spawn_writer(vec![("learn.db".into(), snap.clone())], snap.clone(), 0, own_turn(), move |ok| tx.send(ok).unwrap()).unwrap();
    let _ = body.0.blocking_recv();
    drop(body);
    let started = Instant::now();
    assert!(!rx.recv_timeout(Duration::from_secs(10)).unwrap(), "a stopped download is not a backup");
    assert!(started.elapsed() < Duration::from_secs(10));
    assert!(!snap.exists(), "the snapshot is deleted");
    let _ = std::fs::remove_dir_all(&dir);
}
