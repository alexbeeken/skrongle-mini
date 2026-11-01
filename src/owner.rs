//! What the person who runs the server (the owner) does to an account
//! without any message being sent (#635): a temporary password, two-factor
//! turned off, the Superuser flag. The profile's Superuser section and the
//! binary's commands (`./spanish-learning reset-password <email>`, see
//! [`command`]) both call these, the commands also when the owner is locked
//! out of their own account.
//!
//! A temporary password sets `users.must_change_password`: the `CurrentUser`
//! extractor then sends the account to `/password/change` before anything
//! else, and choosing a new one there (`profile::set_password_hash`) clears it.

use rusqlite::{params, Connection, OptionalExtension};

/// Length of a generated temporary password.
pub const TEMP_PASSWORD_LEN: usize = 14;

/// Letters and digits that can't be mistaken for each other when read out
/// or copied by hand (no 0/O, 1/l/I).
const ALPHABET: &[u8] = b"abcdefghijkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";

/// A new random temporary password.
pub fn temp_password() -> String {
    let mut bytes = [0u8; TEMP_PASSWORD_LEN];
    crate::random::fill(&mut bytes);
    // 256 is not a multiple of the alphabet's size: the bias (a few letters
    // one 256th likelier) costs a fraction of a bit over 14 characters.
    bytes.iter().filter_map(|b| ALPHABET.get(usize::from(*b) % ALPHABET.len()).map(|&c| char::from(c))).collect()
}

/// The account an e-mail address names (any case, surrounding spaces ignored).
pub fn find(conn: &Connection, email: &str) -> rusqlite::Result<Option<i64>> {
    let email = email.trim();
    if email.is_empty() {
        return Ok(None);
    }
    conn.query_row("SELECT id FROM users WHERE lower(email) = lower(?1) ORDER BY id LIMIT 1", [email], |r| r.get(0)).optional()
}

/// Give the account a temporary password (already hashed) that must be
/// changed at the next sign-in. Every session of the account ends, except
/// `keep` (the session token of an owner resetting their own password).
pub fn set_temporary_password(conn: &Connection, user_id: i64, hash: &str, keep: Option<&str>) -> rusqlite::Result<usize> {
    conn.execute("UPDATE users SET password_hash = ?2, must_change_password = 1 WHERE id = ?1", params![user_id, hash])?;
    crate::auth::delete_other_sessions(conn, user_id, keep.unwrap_or(""))
}

/// Turn the account's second sign-in step off: the method, an authenticator
/// app's key, sign-ins waiting for a code and codes sent to confirm a
/// change. A confirmed phone number stays.
pub fn disable_two_factor(conn: &Connection, user_id: i64) -> rusqlite::Result<()> {
    conn.execute("UPDATE users SET two_factor = '' WHERE id = ?1", [user_id])?;
    conn.execute("DELETE FROM totp_keys WHERE user_id = ?1", [user_id])?;
    conn.execute("DELETE FROM two_factor_challenges WHERE user_id = ?1", [user_id])?;
    conn.execute("DELETE FROM two_factor_confirmations WHERE user_id = ?1", [user_id])?;
    Ok(())
}

/// Make the account a superuser (the profile's Superuser section).
pub fn make_superuser(conn: &Connection, user_id: i64) -> rusqlite::Result<()> {
    conn.execute("UPDATE users SET superuser = 1 WHERE id = ?1", [user_id])?;
    Ok(())
}

// ---------- the binary's commands ----------

const HELP: &str = "\
Commands (run beside the server or with it stopped; the database is SKRONGLE_DB, default learn.db):
  reset-password <email>   give the account a temporary password, printed here; it must be changed at the next sign-in
  disable-2fa <email>      turn the account's two-factor sign-in off
  make-owner <email>       make the account a superuser (the profile's Superuser section)
  help                     this list
Without a command the server starts.";

/// The command the binary was started with, if any: `None` starts the
/// server, `Some(code)` is the exit code of a command that ran.
pub fn command(args: &[String]) -> Option<i32> {
    let name = args.first()?;
    let email = args.get(1).map(String::as_str).unwrap_or_default();
    let code = match name.as_str() {
        "help" | "--help" | "-h" => {
            println!("{HELP}");
            0
        }
        "reset-password" | "disable-2fa" | "make-owner" if email.trim().is_empty() || args.len() > 2 => {
            eprintln!("usage: {name} <email>\n\n{HELP}");
            2
        }
        "reset-password" | "disable-2fa" | "make-owner" => {
            let path = crate::env("DB").unwrap_or_else(|| "learn.db".to_string());
            // A mistyped SKRONGLE_DB must not quietly make a new, empty database.
            if !std::path::Path::new(&path).is_file() {
                eprintln!("{path}: no database there (set SKRONGLE_DB to the server's)");
                return Some(1);
            }
            match crate::db::Db::open(std::path::Path::new(&path)).and_then(|db| db.with(|c| run(c, name, email))) {
                Ok(Ok(out)) => {
                    println!("{out}");
                    0
                }
                Ok(Err(why)) => {
                    eprintln!("{why}");
                    1
                }
                Err(e) => {
                    eprintln!("{path}: {e}");
                    1
                }
            }
        }
        other => {
            eprintln!("unknown command {other:?}\n\n{HELP}");
            2
        }
    };
    Some(code)
}

/// One command against the database: what to print, or why it did nothing.
pub fn run(conn: &Connection, name: &str, email: &str) -> rusqlite::Result<Result<String, String>> {
    let Some(id) = find(conn, email)? else {
        return Ok(Err(format!("no account has the address {:?}", email.trim())));
    };
    let email = email.trim();
    Ok(Ok(match name {
        "reset-password" => {
            let password = temp_password();
            let hash = match crate::auth::hash_password(&password) {
                Ok(h) => h,
                Err(e) => return Ok(Err(e.to_string())),
            };
            let ended = set_temporary_password(conn, id, &hash, None)?;
            format!("Temporary password for {email}: {password}\nIt must be changed at the next sign-in ({ended} sessions signed out).")
        }
        "disable-2fa" => {
            disable_two_factor(conn, id)?;
            format!("Two-factor sign-in is off for {email}.")
        }
        "make-owner" => {
            make_superuser(conn, id)?;
            format!("{email} is a superuser: the profile shows the Superuser section.")
        }
        other => return Ok(Err(format!("unknown command {other:?}"))),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temporary_passwords_are_long_and_random() {
        let (a, b) = (temp_password(), temp_password());
        assert_eq!(a.chars().count(), TEMP_PASSWORD_LEN);
        assert!(a.chars().count() >= 12 && crate::auth::validate_password(&a).is_ok());
        assert!(a.bytes().all(|c| ALPHABET.contains(&c)));
        assert_ne!(a, b);
    }

    /// The commands, on a database file as the binary opens it.
    #[test]
    fn commands_change_the_account() {
        let dir = std::env::temp_dir().join(format!("owner-cmd-{}", crate::random::u64()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = crate::db::Db::open(&dir.join("learn.db")).unwrap();
        db.with(|c| {
            let ann = crate::auth::create_user(c, "ann@example.org", &crate::auth::hash_password("old-password").unwrap())?;
            let session = crate::auth::create_session(c, ann)?;
            c.execute("UPDATE users SET two_factor = 'totp', phone = '+15555555555', phone_verified_at = 1 WHERE id = ?1", [ann])?;
            c.execute("INSERT INTO totp_keys (user_id, secret, created_at, confirmed_at) VALUES (?1, x'00', 1, 1)", [ann])?;
            let flag = |c: &Connection| c.query_row("SELECT must_change_password FROM users WHERE id = ?1", [ann], |r| r.get::<_, bool>(0));

            // reset-password: a new hash, the flag, and the sessions end.
            let out = run(c, "reset-password", " ANN@example.org ")?.unwrap();
            let password = out.lines().next().unwrap().rsplit(' ').next().unwrap().to_string();
            assert_eq!(password.len(), TEMP_PASSWORD_LEN, "{out}");
            assert!(!out.contains("old-password"));
            let hash = crate::profile::password_hash(c, ann)?.unwrap();
            assert!(crate::auth::verify_password(&password, &hash) && !crate::auth::verify_password("old-password", &hash));
            assert!(flag(c)?);
            assert!(crate::auth::user_for_session(c, &session)?.is_none(), "signed out everywhere");
            // Choosing a new password clears it.
            crate::profile::set_password_hash(c, ann, "h")?;
            assert!(!flag(c)?);

            // disable-2fa: method and key gone, the confirmed phone stays.
            run(c, "disable-2fa", "ann@example.org")?.unwrap();
            let (method, phone): (String, Option<i64>) = c.query_row("SELECT two_factor, phone_verified_at FROM users WHERE id = ?1", [ann], |r| Ok((r.get(0)?, r.get(1)?)))?;
            assert_eq!((method.as_str(), phone), ("", Some(1)));
            assert_eq!(c.query_row("SELECT COUNT(*) FROM totp_keys", [], |r| r.get::<_, i64>(0))?, 0);

            // make-owner.
            run(c, "make-owner", "ann@example.org")?.unwrap();
            assert!(c.query_row("SELECT superuser FROM users WHERE id = ?1", [ann], |r| r.get::<_, bool>(0))?);

            // Nobody by that address.
            assert!(run(c, "make-owner", "nobody@example.org")?.is_err());
            Ok(())
        })
        .unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_or_incomplete_commands_say_what_exists() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(command(&[]), None, "no command: the server starts");
        assert_eq!(command(&args(&["help"])), Some(0));
        assert_eq!(command(&args(&["serve"])), Some(2));
        assert_eq!(command(&args(&["reset-password"])), Some(2));
        assert_eq!(command(&args(&["make-owner", "a@b.c", "extra"])), Some(2));
    }
}
