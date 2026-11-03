//! Who may make an account (#634): `SKRONGLE_SIGNUP`.
//!
//! - `open` (the default): anyone who reaches the server may register, as
//!   on the hosted site.
//! - `invite`: only through an invitation link a superuser made on the
//!   profile (Superuser → Accounts). A link works once, for 7 days.
//! - `closed`: nobody.
//!
//! In `invite` and `closed` mode the first account is the owner's (the
//! person who runs the server) and becomes a superuser. It is made only
//! through the owner's link: while the server has no account, every start
//! makes a new one and writes it to the log ([`owner_link`]), so only
//! someone who can read the server's log can claim the server. Whoever
//! reaches `/register` first gets the closed page. With the setting unset
//! nothing changes.
//!
//! Table `signup_invites`: one row per link; the owner's link is the row
//! with no `created_by`. The token is stored as its SHA-256 only, like
//! course share links (`share.rs`).

use rusqlite::{params, Connection, OptionalExtension};

/// The environment variable (read as is: no older spelling).
pub const VAR: &str = "SKRONGLE_SIGNUP";
/// How long an invitation link works.
pub const INVITE_TTL_SECS: i64 = 7 * 86_400;
/// How long the owner's link works; a restart replaces it sooner.
pub const OWNER_LINK_TTL_SECS: i64 = 365 * 86_400;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Open,
    Invite,
    Closed,
}

impl Mode {
    /// The setting's value; unset or empty is `open`.
    pub fn parse(v: Option<&str>) -> Result<Mode, String> {
        match v.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
            None | Some("") | Some("open") => Ok(Mode::Open),
            Some("invite") => Ok(Mode::Invite),
            Some("closed") => Ok(Mode::Closed),
            Some(_) => Err(format!("{VAR} must be open, invite or closed, got {:?}", v.unwrap_or_default())),
        }
    }

    /// `SKRONGLE_SIGNUP` from the environment.
    pub fn from_env() -> Result<Mode, String> {
        Mode::parse(std::env::var(VAR).ok().as_deref())
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Open => "open",
            Mode::Invite => "invite",
            Mode::Closed => "closed",
        }
    }

    /// The startup log line.
    pub fn describe(self) -> &'static str {
        match self {
            Mode::Open => "sign-up: open, anyone who reaches the server may register (SKRONGLE_SIGNUP)",
            Mode::Invite => "sign-up: by invitation; the first account is the owner's, made through the owner link, and makes the links (SKRONGLE_SIGNUP)",
            Mode::Closed => "sign-up: closed; only the first account, the owner's, may be made, through the owner link (SKRONGLE_SIGNUP)",
        }
    }
}

/// What an invitation token names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Invite {
    /// Usable: the row's id.
    Valid(i64),
    /// The owner's link, usable while the server has no account: the row's id.
    Owner(i64),
    Used,
    Expired,
    Unknown,
}

/// Why a new account may be made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admit {
    /// `open` mode.
    Open,
    /// No account yet, in `invite` or `closed` mode, through the owner's
    /// link (its id): this one is the owner.
    Owner(i64),
    /// Through this invitation (its id).
    Invited(i64),
}

/// Why it may not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// `closed`, or `invite` without a token.
    Closed,
    /// No account yet, and no owner's link: the owner finds it in the log.
    NoOwner,
    /// The server holds [`crate::caps::MAX_ACCOUNTS`] accounts already.
    Full,
    /// `invite` with a token that cannot be used.
    Used,
    Expired,
    Unknown,
}

impl Refusal {
    /// The page's text.
    pub fn message(self) -> String {
        crate::t(match self {
            Refusal::Full => "auth.signup.full",
            Refusal::Closed => "auth.signup.closed",
            Refusal::NoOwner => "auth.signup.no_owner",
            Refusal::Used => "auth.signup.invite_used",
            Refusal::Expired => "auth.signup.invite_expired",
            Refusal::Unknown => "auth.signup.invite_unknown",
        })
    }
}

/// Does the server have no account yet?
pub fn no_accounts(conn: &Connection) -> rusqlite::Result<bool> {
    conn.query_row("SELECT NOT EXISTS (SELECT 1 FROM users)", [], |r| r.get(0))
}

/// Does the server hold [`crate::caps::MAX_ACCOUNTS`] accounts already?
pub fn is_full(conn: &Connection, dev: bool) -> rusqlite::Result<bool> {
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))?;
    Ok(crate::caps::accounts_full(n, dev))
}

/// What `token` (as typed in `?invite=`) names at `now`.
pub fn check_invite(conn: &Connection, token: &str, now: i64) -> rusqlite::Result<Invite> {
    let token = token.trim();
    if token.is_empty() || token.len() > 128 {
        return Ok(Invite::Unknown);
    }
    let row: Option<(i64, i64, Option<i64>, bool)> = conn
        .query_row(
            "SELECT id, expires_at, used_at, created_by IS NULL FROM signup_invites WHERE token_hash = ?1",
            [crate::auth::hash_token(token)],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    Ok(match row {
        None => Invite::Unknown,
        Some((_, _, Some(_), _)) => Invite::Used,
        Some((_, exp, None, _)) if exp <= now => Invite::Expired,
        Some((id, _, None, true)) => Invite::Owner(id),
        Some((id, _, None, false)) => Invite::Valid(id),
    })
}

/// May an account be made now, in `mode`, with this invitation token ('' for
/// none), for `email` (`None` before the address is known: the form)?
pub fn admit(conn: &Connection, mode: Mode, invite: &str, _email: Option<&str>, now: i64, dev: bool) -> rusqlite::Result<Result<Admit, Refusal>> {
    // First, in every mode: callers run this in the transaction that
    // inserts the account, so the count and the insert can't be split.
    if is_full(conn, dev)? {
        return Ok(Err(Refusal::Full));
    }
    if mode == Mode::Open {
        return Ok(Ok(Admit::Open));
    }
    if no_accounts(conn)? {
        // Only the owner's link; an old or wrong one says where to find it.
        return Ok(match check_invite(conn, invite, now)? {
            Invite::Owner(id) => Ok(Admit::Owner(id)),
            _ => Err(Refusal::NoOwner),
        });
    }
    if mode == Mode::Closed || invite.trim().is_empty() {
        return Ok(Err(Refusal::Closed));
    }
    Ok(match check_invite(conn, invite, now)? {
        Invite::Valid(id) => Ok(Admit::Invited(id)),
        Invite::Used => Err(Refusal::Used),
        Invite::Expired => Err(Refusal::Expired),
        // An owner's link left over once there are accounts lets nobody in.
        Invite::Owner(_) | Invite::Unknown => Err(Refusal::Unknown),
    })
}

/// After the account `user_id` was made on `admit`'s word, in the same
/// transaction: the link is used up, and the owner becomes a superuser
/// (only while it is the one account).
/// `false` when the link was used meanwhile, one made for an address names
/// another, or another account came first: the caller rolls back.
pub fn finish(conn: &Connection, admitted: Admit, user_id: i64, now: i64) -> rusqlite::Result<bool> {
    let use_up = |id: i64| {
        conn.execute(
            "UPDATE signup_invites SET used_at = ?2, used_by = ?3 WHERE id = ?1 AND used_at IS NULL AND expires_at > ?2
             AND (email IS NULL OR email = (SELECT lower(email) FROM users WHERE id = ?3))",
            params![id, now, user_id],
        )
        .map(|n| n == 1)
    };
    let ok = match admitted {
        Admit::Open => true,
        Admit::Owner(id) => use_up(id)? && conn.execute("UPDATE users SET superuser = 1 WHERE id = ?1 AND (SELECT COUNT(*) FROM users) = 1", [user_id])? == 1,
        Admit::Invited(id) => use_up(id)?,
    };
    Ok(ok)
}

/// At startup: while the server has no account, a new owner's link (its
/// token), replacing any earlier one; `None` once it has accounts, when
/// unused owner's links are deleted.
pub fn owner_link(conn: &Connection, now: i64) -> rusqlite::Result<Option<String>> {
    conn.execute("DELETE FROM signup_invites WHERE created_by IS NULL AND used_at IS NULL", [])?;
    if !no_accounts(conn)? {
        return Ok(None);
    }
    let token = crate::auth::generate_token();
    conn.execute(
        "INSERT INTO signup_invites (token_hash, created_by, created_at, expires_at) VALUES (?1, NULL, ?2, ?3)",
        params![crate::auth::hash_token(&token), now, now + OWNER_LINK_TTL_SECS],
    )?;
    Ok(Some(token))
}

/// Where the owner's link points: `SKRONGLE_PUBLIC_URL`, else this machine's
/// name on the local network (`<hostname>.local`) and the port, or the
/// loopback address when the server listens only there.
pub fn owner_link_url(public_url: Option<&str>, bind: std::net::SocketAddr, token: &str) -> String {
    let path = crate::url(&format!("/register?invite={token}"));
    if let Some(base) = public_url {
        return format!("{base}{path}");
    }
    let host = if bind.ip().is_loopback() {
        bind.ip().to_string()
    } else {
        std::fs::read_to_string("/proc/sys/kernel/hostname")
            .ok()
            .map(|h| h.trim().to_string())
            .filter(|h| !h.is_empty())
            .map_or_else(|| "localhost".to_string(), |h| format!("{h}.local"))
    };
    format!("http://{host}:{}{path}", bind.port())
}

/// A new invitation link's token (only its hash is stored).
pub fn make(conn: &Connection, created_by: i64, now: i64) -> rusqlite::Result<String> {
    let token = crate::auth::generate_token();
    conn.execute(
        "INSERT INTO signup_invites (token_hash, created_by, created_at, expires_at) VALUES (?1, ?2, ?3, ?4)",
        params![crate::auth::hash_token(&token), created_by, now, now + INVITE_TTL_SECS],
    )?;
    Ok(token)
}

/// An invitation not used yet and not expired.
#[derive(Debug, Clone)]
pub struct OpenInvite {
    pub id: i64,
    pub created_at: i64,
    pub expires_at: i64,
}

/// Every open invitation, newest first (not the owner's link).
pub fn open_invites(conn: &Connection, now: i64) -> rusqlite::Result<Vec<OpenInvite>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, created_at, expires_at FROM signup_invites WHERE used_at IS NULL AND expires_at > ?1 AND created_by IS NOT NULL ORDER BY id DESC",
    )?;
    let rows = stmt.query_map([now], |r| Ok(OpenInvite { id: r.get(0)?, created_at: r.get(1)?, expires_at: r.get(2)? }))?;
    rows.collect()
}

/// Take back an unused invitation. True when there was one.
pub fn revoke(conn: &Connection, id: i64) -> rusqlite::Result<bool> {
    Ok(conn.execute("DELETE FROM signup_invites WHERE id = ?1 AND used_at IS NULL AND created_by IS NOT NULL", [id])? > 0)
}

/// Used and expired links are no use to anyone after a while: gone 30 days
/// after they expire (hourly housekeeping).
pub fn purge(conn: &Connection, now: i64) -> rusqlite::Result<usize> {
    conn.execute("DELETE FROM signup_invites WHERE expires_at < ?1", [now - 30 * 86_400])
}

/// One account, for the Superuser section's list.
#[derive(Debug, Clone)]
pub struct Account {
    pub email: String,
    pub created_at: i64,
    /// The newest session still on record (sessions last 30 days), if any.
    pub last_sign_in: Option<i64>,
    pub superuser: bool,
}

/// Every account, newest first. The server holds at most
/// `caps::MAX_ACCOUNTS`, so there is no paging.
pub fn accounts(conn: &Connection) -> rusqlite::Result<Vec<Account>> {
    let mut stmt = conn.prepare_cached(
        "SELECT COALESCE(u.email, ''), u.created_at, (SELECT MAX(s.created_at) FROM sessions s WHERE s.user_id = u.id), u.superuser
         FROM users u ORDER BY u.created_at DESC, u.id DESC",
    )?;
    let rows = stmt.query_map([], |r| Ok(Account { email: r.get(0)?, created_at: r.get(1)?, last_sign_in: r.get(2)?, superuser: r.get(3)? }))?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    #[test]
    fn the_setting_parses() {
        assert_eq!(Mode::parse(None), Ok(Mode::Open));
        assert_eq!(Mode::parse(Some("")), Ok(Mode::Open));
        assert_eq!(Mode::parse(Some("open")), Ok(Mode::Open));
        assert_eq!(Mode::parse(Some(" Invite ")), Ok(Mode::Invite));
        assert_eq!(Mode::parse(Some("closed")), Ok(Mode::Closed));
        let e = Mode::parse(Some("invites")).unwrap_err();
        assert!(e.contains("SKRONGLE_SIGNUP") && e.contains("invites"), "{e}");
        assert!(Mode::parse(Some("1")).is_err());
    }

    /// In `invite` and `closed` mode the first account is the owner's, and
    /// only through the owner's link; the next is not. `open` never makes a
    /// superuser, nor an owner's link.
    #[test]
    fn the_first_account_is_the_owner() {
        for mode in [Mode::Invite, Mode::Closed] {
            let db = Db::open_in_memory().unwrap();
            db.with(|c| {
                assert_eq!(admit(c, mode, "", None, 1, false)?, Err(Refusal::NoOwner), "{mode:?}: no link, no account");
                let old = owner_link(c, 1)?.expect("an empty server has an owner's link");
                let token = owner_link(c, 2)?.expect("a restart makes a new one");
                assert_eq!(c.query_row("SELECT COUNT(*) FROM signup_invites", [], |r| r.get::<_, i64>(0))?, 1, "the old one is gone");
                assert_eq!(admit(c, mode, &old, None, 2, false)?, Err(Refusal::NoOwner));
                assert_eq!(admit(c, mode, "nonsense", None, 2, false)?, Err(Refusal::NoOwner));
                let Ok(a @ Admit::Owner(_)) = admit(c, mode, &token, None, 2, false)? else { panic!("{mode:?}: the owner's link lets the owner in") };
                assert!(open_invites(c, 2)?.is_empty(), "the owner's link is not listed");
                let id = crate::auth::create_user(c, "owner@example.org", "h")?;
                assert!(finish(c, a, id, 2)?);
                let su: bool = c.query_row("SELECT superuser FROM users WHERE id = ?1", [id], |r| r.get(0))?;
                assert!(su, "{mode:?}: the first account is a superuser");
                assert_eq!(admit(c, mode, "", None, 2, false)?, Err(Refusal::Closed));
                assert_ne!(admit(c, mode, &token, None, 2, false)?, Ok(a), "the owner's link is used up");
                assert!(!finish(c, a, id, 3)?, "and can't be used again");
                assert_eq!(owner_link(c, 3)?, None, "a server with accounts makes none");
                Ok(())
            })
            .unwrap();
        }
        // Two people with the owner's link at once: the second is rolled back.
        let db = Db::open_in_memory().unwrap();
        db.with(|c| {
            let token = owner_link(c, 1)?.unwrap();
            let a = admit(c, Mode::Invite, &token, None, 1, false)?.unwrap();
            crate::auth::create_user(c, "a@example.org", "h")?;
            let second = crate::auth::create_user(c, "b@example.org", "h")?;
            assert!(!finish(c, a, second, 1)?, "not the one account: refused");
            let su: i64 = c.query_row("SELECT COUNT(*) FROM users WHERE superuser = 1", [], |r| r.get(0))?;
            assert_eq!(su, 0);
            Ok(())
        })
        .unwrap();
        // An unused owner's link left over once there are accounts lets nobody in.
        let db = Db::open_in_memory().unwrap();
        db.with(|c| {
            let token = owner_link(c, 1)?.unwrap();
            crate::auth::create_user(c, "a@example.org", "h")?;
            assert_eq!(admit(c, Mode::Invite, &token, None, 1, false)?, Err(Refusal::Unknown));
            Ok(())
        })
        .unwrap();
        let db = Db::open_in_memory().unwrap();
        db.with(|c| {
            assert_eq!(admit(c, Mode::Open, "", None, 1, false)?, Ok(Admit::Open));
            let id = crate::auth::create_user(c, "a@example.org", "h")?;
            finish(c, Admit::Open, id, 1)?;
            let su: bool = c.query_row("SELECT superuser FROM users WHERE id = ?1", [id], |r| r.get(0))?;
            assert!(!su, "open mode: the first account is an ordinary one");
            Ok(())
        })
        .unwrap();
    }

    /// The ninth account is refused in every mode, with the one message;
    /// the register form, an invitation link and the proxy all ask `admit`.
    #[test]
    fn the_ninth_account_is_refused() {
        let max = crate::caps::MAX_ACCOUNTS;
        for mode in [Mode::Open, Mode::Invite, Mode::Closed] {
            let db = Db::open_in_memory().unwrap();
            db.with(|c| {
                for n in 0..max {
                    assert!(!is_full(c, false)?);
                    crate::auth::create_user(c, &format!("u{n}@example.org"), "h")?;
                }
                assert!(is_full(c, false)?);
                // An unused, valid invitation does not get past the cap.
                let token = make(c, 1, 1)?;
                assert_eq!(admit(c, mode, &token, None, 2, false)?, Err(Refusal::Full), "{mode:?}");
                assert_eq!(admit(c, mode, "", None, 2, false)?, Err(Refusal::Full), "{mode:?}");
                assert!(!is_full(c, true)?, "{mode:?}: the dev server is not capped");
                assert_eq!(admit(c, Mode::Open, "", None, 2, true)?, Ok(Admit::Open), "dev mode admits past the cap");
                Ok(())
            })
            .unwrap();
        }
        assert_eq!(Refusal::Full.message(), crate::t("auth.signup.full"));
    }

    #[test]
    fn the_owner_link_points_at_this_server() {
        let lan: std::net::SocketAddr = "0.0.0.0:8080".parse().unwrap();
        let local: std::net::SocketAddr = "127.0.0.1:8634".parse().unwrap();
        assert_eq!(owner_link_url(Some("https://learn.example.org"), lan, "ab"), format!("https://learn.example.org{}", crate::url("/register?invite=ab")));
        assert_eq!(owner_link_url(None, local, "ab"), format!("http://127.0.0.1:8634{}", crate::url("/register?invite=ab")));
        let u = owner_link_url(None, lan, "ab");
        assert!(u.starts_with("http://") && u.contains(":8080/") && u.ends_with("/register?invite=ab"), "{u}");
    }

    /// Every account shows, newest first, however many there are up to the cap.
    #[test]
    fn accounts_list_every_account() {
        let db = Db::open_in_memory().unwrap();
        db.with(|c| {
            assert!(accounts(c)?.is_empty());
            for n in 0..crate::caps::MAX_ACCOUNTS {
                crate::auth::create_user(c, &format!("u{n:02}@example.org"), "h")?;
            }
            let list = accounts(c)?;
            assert_eq!(list.len(), crate::caps::MAX_ACCOUNTS);
            assert_eq!(list[0].email, format!("u{:02}@example.org", crate::caps::MAX_ACCOUNTS - 1));
            assert_eq!(list[list.len() - 1].email, "u00@example.org");
            Ok(())
        })
        .unwrap();
    }

    /// An invitation: made, used once, then used up; expired; revoked; unknown.
    #[test]
    fn invitations_work_once_for_seven_days() {
        let db = Db::open_in_memory().unwrap();
        db.with(|c| {
            let owner = crate::auth::create_user(c, "owner@example.org", "h")?;
            let t0 = 1_000_000;
            let token = make(c, owner, t0)?;
            assert_eq!(token.len(), 64);
            let stored: String = c.query_row("SELECT token_hash FROM signup_invites", [], |r| r.get(0))?;
            assert_ne!(stored, token, "only the hash is stored");
            assert_eq!(open_invites(c, t0)?.len(), 1);

            // Closed mode ignores invitations.
            assert_eq!(admit(c, Mode::Closed, &token, None, t0, false)?, Err(Refusal::Closed));
            let Ok(a @ Admit::Invited(_)) = admit(c, Mode::Invite, &token, None, t0 + 60, false)? else { panic!("a fresh link lets one in") };
            let ann = crate::auth::create_user(c, "ann@example.org", "h")?;
            assert!(finish(c, a, ann, t0 + 60)?);
            assert!(!finish(c, a, ann, t0 + 61)?, "a second use is refused");
            assert_eq!(admit(c, Mode::Invite, &token, None, t0 + 120, false)?, Err(Refusal::Used));
            assert!(open_invites(c, t0)?.is_empty());
            let su: bool = c.query_row("SELECT superuser FROM users WHERE id = ?1", [ann], |r| r.get(0))?;
            assert!(!su, "an invited account is not a superuser");

            let late = make(c, owner, t0)?;
            assert!(matches!(admit(c, Mode::Invite, &late, None, t0 + INVITE_TTL_SECS - 1, false)?, Ok(Admit::Invited(_))));
            assert_eq!(admit(c, Mode::Invite, &late, None, t0 + INVITE_TTL_SECS, false)?, Err(Refusal::Expired));

            let gone = make(c, owner, t0)?;
            let id = open_invites(c, t0)?.into_iter().next().unwrap().id;
            assert!(revoke(c, id)?);
            assert!(!revoke(c, id)?);
            assert_eq!(admit(c, Mode::Invite, &gone, None, t0, false)?, Err(Refusal::Unknown));
            assert_eq!(admit(c, Mode::Invite, "nonsense", None, t0, false)?, Err(Refusal::Unknown));
            assert_eq!(admit(c, Mode::Invite, "", None, t0, false)?, Err(Refusal::Closed));

            // A used link can't be revoked; old rows go 30 days after they expire.
            let used_id: i64 = c.query_row("SELECT id FROM signup_invites WHERE used_at IS NOT NULL", [], |r| r.get(0))?;
            assert!(!revoke(c, used_id)?);
            assert_eq!(purge(c, t0 + INVITE_TTL_SECS + 30 * 86_400 + 1)?, 2);

            let p = accounts(c)?;
            assert_eq!(p.len(), 2);
            assert_eq!(p.first().map(|a| a.email.as_str()), Some("ann@example.org"));
            Ok(())
        })
        .unwrap();
    }
}
