//! Startup errors for the usual self-hosting mistakes, worded so the log line
//! says what to change: the port is taken, the database folder can't be
//! written, the secret key went missing. `skrongle doctor` reuses the checks.

use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

/// Why `bind` failed, and what to do about it.
pub fn bind_error(addr: SocketAddr, e: &io::Error) -> String {
    let port = addr.port();
    match e.kind() {
        io::ErrorKind::AddrInUse => format!(
            "cannot listen on {addr}: port {port} is already in use, by another copy of Skrongle \
             (`systemctl status skrongle`) or another program (`sudo ss -ltnp 'sport = :{port}'`). \
             Stop it, or pick a free port with SKRONGLE_BIND=0.0.0.0:{}",
            if port == 8081 { 8082 } else { 8081 }
        ),
        io::ErrorKind::PermissionDenied => format!(
            "cannot listen on {addr}: permission denied. Ports below 1024 need extra privileges; \
             use SKRONGLE_BIND=0.0.0.0:7576 and put a proxy (nginx, Caddy) in front for port 80/443"
        ),
        io::ErrorKind::AddrNotAvailable => format!(
            "cannot listen on {addr}: this machine has no address {}. Use SKRONGLE_BIND=0.0.0.0:{port} \
             (every interface) or 127.0.0.1:{port} (this machine only)",
            addr.ip()
        ),
        _ => format!("cannot listen on {addr}: {e}"),
    }
}

/// The folder the database lives in (`.` for a bare file name).
pub fn db_dir(db_path: &Path) -> PathBuf {
    match db_path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

/// `Ok` when this process may write `path` (a file or a folder).
#[cfg(unix)]
pub fn writable(path: &Path) -> io::Result<bool> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    // SAFETY: `c` is a valid NUL-terminated string that outlives the call.
    let rc = unsafe { libc::access(c.as_ptr(), libc::W_OK) };
    if rc == 0 {
        return Ok(true);
    }
    let err = io::Error::last_os_error();
    match err.raw_os_error() {
        Some(libc::EACCES) | Some(libc::EROFS) | Some(libc::EPERM) => Ok(false),
        _ => Err(err),
    }
}

#[cfg(not(unix))]
pub fn writable(path: &Path) -> io::Result<bool> {
    std::fs::metadata(path).map(|m| !m.permissions().readonly())
}

/// "uid 997" on Unix: who the server runs as, for permission errors.
pub fn who() -> String {
    #[cfg(unix)]
    {
        // SAFETY: geteuid has no preconditions and cannot fail.
        format!("uid {}", unsafe { libc::geteuid() })
    }
    #[cfg(not(unix))]
    {
        "this user".to_string()
    }
}

/// SQLite writes the database and, in WAL mode, `-wal` and `-shm` files next
/// to it, so both the folder and an existing file must be writable.
pub fn check_db_writable(db_path: &Path) -> Result<(), String> {
    let dir = db_dir(db_path);
    if !dir.is_dir() {
        return Err(format!(
            "cannot open the database {}: the folder {} does not exist. Create it, or set SKRONGLE_DB to a path \
             in a folder that exists (the systemd unit uses /var/lib/skrongle/learn.db)",
            db_path.display(),
            dir.display()
        ));
    }
    let hint = format!(
        "the server runs as {}. Give that user the folder (`sudo chown -R <user> {}`), or set SKRONGLE_DB to a \
         writable path; under systemd, StateDirectory=skrongle makes /var/lib/skrongle writable",
        who(),
        dir.display()
    );
    if matches!(writable(&dir), Ok(false)) {
        return Err(format!(
            "cannot write the database {}: the folder {} is not writable (SQLite also creates -wal and -shm files there); {hint}",
            db_path.display(),
            dir.display()
        ));
    }
    if db_path.exists() && matches!(writable(db_path), Ok(false)) {
        return Err(format!("cannot write the database {}: the file is read-only; {hint}", db_path.display()));
    }
    Ok(())
}

/// `Db::open` failed after the folder checks passed.
pub fn db_error(db_path: &Path, e: &rusqlite::Error) -> String {
    use rusqlite::ErrorCode;
    let code = match e {
        rusqlite::Error::SqliteFailure(f, _) => Some(f.code),
        _ => None,
    };
    let hint = match code {
        Some(ErrorCode::CannotOpen) | Some(ErrorCode::ReadOnly) | Some(ErrorCode::PermissionDenied) => {
            format!(" (the server runs as {}; it needs to write the file and its folder)", who())
        }
        Some(ErrorCode::NotADatabase) => " (this file is not a SQLite database: is SKRONGLE_DB pointing at the right file?)".to_string(),
        Some(ErrorCode::DatabaseCorrupt) => " (the file is damaged: restore it from a backup, see /docs/self-hosting#backup-and-restore)".to_string(),
        Some(ErrorCode::DatabaseBusy) | Some(ErrorCode::DatabaseLocked) => {
            " (another process holds it: is a second copy of Skrongle running on the same database?)".to_string()
        }
        _ => String::new(),
    };
    format!("cannot open the database {}: {e}{hint}", db_path.display())
}

/// Learners' saved AI keys are encrypted with the secret key. A database
/// restored (or moved) without its `<db>.key` file would otherwise start
/// with a fresh key and every saved key would silently stop working, so
/// refuse to start unless the operator says the old key is gone for good.
pub fn check_secret_key(env_set: bool, db_path: &Path, saved_keys: i64, new_key_ok: bool) -> Result<(), String> {
    if env_set || saved_keys == 0 || new_key_ok {
        return Ok(());
    }
    let key_file = crate::secrets::key_file_path(db_path);
    if key_file.exists() {
        return Ok(());
    }
    Err(format!(
        "the secret key is missing: {} has {saved_keys} saved AI key(s) encrypted with it, but {} does not exist \
         and SKRONGLE_SECRET_KEY is not set. Restore the .key file from the same backup as the database, or set \
         SKRONGLE_SECRET_KEY. If the key is lost for good, start once with SKRONGLE_NEW_SECRET_KEY=1: a new key is \
         created and learners enter their AI keys again",
        db_path.display(),
        key_file.display()
    ))
}

/// A secret key that exists but is not the one the saved AI keys were
/// sealed with (a `SKRONGLE_SECRET_KEY` from another install, a `.key` file from
/// another backup than the database) opens none of them: refuse to start the
/// way a missing key does. `readable` of `total` saved keys open with it;
/// `source` is where it came from.
pub fn check_secret_opens(db_path: &Path, source: &str, readable: usize, total: usize, new_key_ok: bool) -> Result<(), String> {
    if total == 0 || readable > 0 || new_key_ok {
        return Ok(());
    }
    Err(format!(
        "the secret key is wrong: the key from {source} cannot read any of the {total} saved AI key(s) in {}. \
         Most likely SKRONGLE_SECRET_KEY belongs to another install, or the .key file comes from a different backup \
         than the database: use the key that was saved with this database. If that key is lost for good, start once \
         with SKRONGLE_NEW_SECRET_KEY=1: the saved keys it cannot read are removed and learners enter their AI keys again",
        db_path.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_in_use_says_what_to_do() {
        let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = taken.local_addr().unwrap();
        let e = std::net::TcpListener::bind(addr).unwrap_err();
        let msg = bind_error(addr, &e);
        assert!(msg.contains("already in use"), "{msg}");
        assert!(msg.contains("SKRONGLE_BIND="), "{msg}");
    }

    #[test]
    fn missing_db_folder_is_named() {
        let msg = check_db_writable(Path::new("/definitely/not/here/learn.db")).unwrap_err();
        assert!(msg.contains("/definitely/not/here") && msg.contains("does not exist"), "{msg}");
        assert!(check_db_writable(Path::new("learn.db")).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn read_only_db_folder_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        // root can write anywhere; nothing to check then.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let dir = std::env::temp_dir().join(format!("skrongle-ro-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let msg = check_db_writable(&dir.join("learn.db")).unwrap_err();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(msg.contains("not writable") && msg.contains("uid "), "{msg}");
    }

    #[test]
    fn lost_secret_key_with_saved_keys_stops_startup() {
        let dir = std::env::temp_dir().join(format!("skrongle-key-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("learn.db");
        let key = crate::secrets::key_file_path(&db);
        let _ = std::fs::remove_file(&key);
        // A fresh install, or nobody saved a key: a new key is fine.
        assert!(check_secret_key(false, &db, 0, false).is_ok());
        let msg = check_secret_key(false, &db, 2, false).unwrap_err();
        assert!(msg.contains("learn.db.key") && msg.contains("SKRONGLE_NEW_SECRET_KEY=1"), "{msg}");
        assert!(check_secret_key(true, &db, 2, false).is_ok());
        assert!(check_secret_key(false, &db, 2, true).is_ok());
        std::fs::write(&key, "x").unwrap();
        assert!(check_secret_key(false, &db, 2, false).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wrong_secret_key_with_saved_keys_stops_startup() {
        use crate::secrets::{readable_count, save, Provider, Vault};
        let db = crate::db::Db::open_in_memory().unwrap();
        let (right, wrong) = (Vault::from_master(&[7; 32]), Vault::from_master(&[9; 32]));
        let path = Path::new("/srv/skrongle/learn.db");
        db.with(|c| {
            crate::auth::create_user(c, "ann@example.org", "h")?;
            // Nobody saved a key yet: any key will do.
            assert_eq!(readable_count(c, &wrong)?, (0, 0));
            assert!(check_secret_opens(path, "SKRONGLE_SECRET_KEY", 0, 0, false).is_ok());
            save(c, &right, Provider::Anthropic, "sk-ant-api03-abc")?;
            // The key the database was saved with opens it.
            let (n, total) = readable_count(c, &right)?;
            assert!(check_secret_opens(path, "SKRONGLE_SECRET_KEY", n, total, false).is_ok());
            // Another install's key opens nothing: refused, with the cause and the way out.
            let (n, total) = readable_count(c, &wrong)?;
            assert_eq!((n, total), (0, 1));
            let msg = check_secret_opens(path, "SKRONGLE_SECRET_KEY", n, total, false).unwrap_err();
            assert!(msg.contains("wrong") && msg.contains("another install") && msg.contains("different backup"), "{msg}");
            assert!(msg.contains("/srv/skrongle/learn.db") && msg.contains("SKRONGLE_NEW_SECRET_KEY=1"), "{msg}");
            // SKRONGLE_NEW_SECRET_KEY=1 starts, and the unreadable key is forgotten.
            assert!(check_secret_opens(path, "SKRONGLE_SECRET_KEY", n, total, true).is_ok());
            assert_eq!(crate::secrets::forget_unreadable(c, &wrong)?, 1);
            assert_eq!(readable_count(c, &wrong)?, (0, 0));
            Ok(())
        })
        .unwrap();
    }
}
