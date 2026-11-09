//! What the person who runs the server (the owner) does to an account
//! without any message being sent (#635): a temporary password, two-factor
//! turned off, the owner flag. The Manage Users page and the
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

/// Turn the account's second sign-in step off: the flag, an authenticator
/// app's key and sign-ins waiting for a code.
pub fn disable_two_factor(conn: &Connection, user_id: i64) -> rusqlite::Result<()> {
    conn.execute("UPDATE users SET two_factor = '' WHERE id = ?1", [user_id])?;
    conn.execute("DELETE FROM totp_keys WHERE user_id = ?1", [user_id])?;
    conn.execute("DELETE FROM two_factor_challenges WHERE user_id = ?1", [user_id])?;
    Ok(())
}

/// Make the account an owner (the Manage Users page).
pub fn make_owner(conn: &Connection, user_id: i64) -> rusqlite::Result<()> {
    conn.execute("UPDATE users SET owner = 1 WHERE id = ?1", [user_id])?;
    Ok(())
}

// ---------- the binary's commands ----------

/// `skrongle owner-link`: the first-run link on demand. Only while the
/// server has no account. Tokens are stored as hashes (`signup.rs`), so the
/// link the server logged cannot be read back: this makes a fresh one and
/// the earlier one stops working (`--new` says the same). `Ok(text, code)`.
pub fn owner_link_text(
    conn: &Connection,
    mode: crate::signup::Mode,
    public_url: Option<&str>,
    bind: std::net::SocketAddr,
    now: i64,
) -> rusqlite::Result<(String, i32)> {
    if mode == crate::signup::Mode::Open {
        return Ok((
            "SKRONGLE_SIGNUP=open: there is no owner link. The first account to register on the server becomes the owner; open the server's address and register.".into(),
            0,
        ));
    }
    match crate::signup::owner_link(conn, now)? {
        None => Ok(("An owner account already exists, so there is no owner link. To get back into it: skrongle reset-password <email>.".into(), 0)),
        Some(token) => {
            let link = crate::signup::owner_link_url(public_url, bind, &token);
            Ok((
                format!(
                    "Owner link, to make your own account (the owner's):\n{link}\n\nThis is a new link: it replaces any earlier one, including the one in the server log. Whoever opens it first claims the server."
                ),
                0,
            ))
        }
    }
}

/// Run `owner-link` (its arguments after the name); the exit code.
pub fn owner_link_command(args: &[String]) -> i32 {
    // `--new` is accepted for clarity: the command always makes a new link.
    if let Err(code) = crate::doctor::apply_settings_args("owner-link", args, &["--new"]) {
        return code;
    }
    let cfg = match crate::Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    let mode = match crate::signup::Mode::from_env() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    let path = cfg.db_path;
    // A mistyped SKRONGLE_DB must not quietly make a new, empty database.
    if !path.is_file() {
        eprintln!("{}: no database there. Start the server once first, or set SKRONGLE_DB (--service reads the installed unit's).", path.display());
        return 1;
    }
    let public_url = crate::env("PUBLIC_URL").map(|u| u.trim_end_matches('/').to_string());
    match crate::db::Db::open(&path).and_then(|db| db.with(|c| owner_link_text(c, mode, public_url.as_deref(), cfg.bind, crate::db::now()))) {
        Ok((text, code)) => {
            println!("{text}");
            code
        }
        Err(e) => {
            eprintln!("{}: {e}", path.display());
            1
        }
    }
}

const HELP: &str = "\
Commands (run beside the server or with it stopped; the database is SKRONGLE_DB, default learn.db):
  reset-password <email>   give the account a temporary password, printed here; it must be changed at the next sign-in
  disable-2fa <email>      turn the account's two-factor sign-in off
  make-owner <email>       make the account an owner (the Manage Users page)
  owner-link [--new] [--service | --env-file FILE]
                           print the link that makes the owner's account (only while there is no account)
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
            make_owner(conn, id)?;
            format!("{email} is an owner: the account menu shows Manage Users.")
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
    fn owner_link_is_made_on_demand_only_without_an_account() {
        use crate::signup::Mode;
        let bind: std::net::SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let db = crate::db::Db::open_in_memory().unwrap();
        db.with(|c| {
            let token_of = |t: &str| t.split("invite=").nth(1).unwrap().lines().next().unwrap().to_string();
            // No account: a link with the public URL.
            let (a, code) = owner_link_text(c, Mode::Invite, Some("https://learn.example.org"), bind, 1)?;
            assert_eq!(code, 0);
            assert!(a.contains("https://learn.example.org"), "{a}");
            let first = token_of(&a);
            assert!(matches!(crate::signup::check_invite(c, &first, 2)?, crate::signup::Invite::Owner(_)));
            // Again (or --new): a new link, and the first is dead.
            let (b, _) = owner_link_text(c, Mode::Closed, None, bind, 3)?;
            assert!(b.contains("http://127.0.0.1:8080"), "{b}");
            let second = token_of(&b);
            assert_ne!(first, second);
            assert!(!matches!(crate::signup::check_invite(c, &first, 4)?, crate::signup::Invite::Owner(_)), "the old link is invalidated");
            assert!(matches!(crate::signup::check_invite(c, &second, 4)?, crate::signup::Invite::Owner(_)));
            // Open mode: none, and nothing made.
            let before: i64 = c.query_row("SELECT COUNT(*) FROM signup_invites", [], |r| r.get(0))?;
            let (o, _) = owner_link_text(c, Mode::Open, None, bind, 5)?;
            assert!(o.contains("open") && !o.contains("invite="));
            assert_eq!(before, c.query_row("SELECT COUNT(*) FROM signup_invites", [], |r| r.get::<_, i64>(0))?);
            // An owner exists: no link, and none left over.
            crate::auth::create_user(c, "owner@example.org", "h")?;
            let (t, code) = owner_link_text(c, Mode::Invite, None, bind, 6)?;
            assert_eq!(code, 0);
            assert!(t.contains("already exists") && !t.contains("invite="), "{t}");
            assert_eq!(0, c.query_row("SELECT COUNT(*) FROM signup_invites", [], |r| r.get::<_, i64>(0))?);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn commands_change_the_account() {
        let dir = std::env::temp_dir().join(format!("owner-cmd-{}", crate::random::u64()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = crate::db::Db::open(&dir.join("learn.db")).unwrap();
        db.with(|c| {
            let ann = crate::auth::create_user(c, "ann@example.org", &crate::auth::hash_password("old-password").unwrap())?;
            let session = crate::auth::create_session(c, ann)?;
            c.execute("UPDATE users SET two_factor = 'totp' WHERE id = ?1", [ann])?;
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

            // disable-2fa: flag and key gone.
            run(c, "disable-2fa", "ann@example.org")?.unwrap();
            let method: String = c.query_row("SELECT two_factor FROM users WHERE id = ?1", [ann], |r| r.get(0))?;
            assert_eq!(method, "");
            assert_eq!(c.query_row("SELECT COUNT(*) FROM totp_keys", [], |r| r.get::<_, i64>(0))?, 0);

            // make-owner.
            run(c, "make-owner", "ann@example.org")?.unwrap();
            assert!(c.query_row("SELECT owner FROM users WHERE id = ?1", [ann], |r| r.get::<_, bool>(0))?);

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
