use super::*;
use crate::db::Db;

fn count(c: &Connection, sql: &str) -> i64 {
    c.query_row(sql, [], |r| r.get(0)).unwrap()
}

#[test]
fn a_pasted_list_is_read_as_addresses() {
    assert_eq!(parse_list("Ann@Example.org, bob@example.org;\n<cy@example.org> nonsense ann@example.org"), vec!["ann@example.org", "bob@example.org", "cy@example.org"]);
    assert!(parse_list("  \n").is_empty());
}

/// Everyone: every session but the superuser's own, waiting second steps
/// and unused reset links go; a used reset link stays as a record.
#[test]
fn signing_everyone_out_keeps_only_the_superusers_session() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let su = crate::auth::create_user(c, "boss@example.org", "x")?;
        let ann = crate::auth::create_user(c, "ann@example.org", "x")?;
        let mine = crate::auth::create_session(c, su)?;
        crate::auth::create_session(c, su)?;
        crate::auth::create_session(c, ann)?;
        c.execute("INSERT INTO password_resets (user_id, email, token_hash, created_at, expires_at) VALUES (?1, 'ann@example.org', 'a', 1, 9)", [ann])?;
        c.execute("INSERT INTO password_resets (user_id, email, token_hash, created_at, expires_at, used_at) VALUES (?1, 'ann@example.org', 'b', 1, 9, 2)", [ann])?;
        c.execute("INSERT INTO two_factor_challenges (user_id, token_hash, method, created_at, expires_at) VALUES (?1, 't', 'totp', 1, 9)", [ann])?;
        let out = sign_out(c, &Who::Everyone, &mine)?;
        assert_eq!(out, SignedOut { accounts: 2, sessions: 2 });
        assert_eq!(crate::auth::user_for_session(c, &mine)?.map(|u| u.id), Some(su));
        assert_eq!(count(c, "SELECT COUNT(*) FROM sessions"), 1);
        assert_eq!(count(c, "SELECT COUNT(*) FROM two_factor_challenges"), 0);
        assert_eq!(count(c, "SELECT COUNT(*) FROM password_resets"), 1);
        Ok(())
    })
    .unwrap();
}

/// A list signs out only the accounts it names.
#[test]
fn signing_a_list_out_leaves_the_others() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let ann = crate::auth::create_user(c, "ann@example.org", "x")?;
        let bob = crate::auth::create_user(c, "bob@example.org", "x")?;
        crate::auth::create_session(c, ann)?;
        crate::auth::create_session(c, ann)?;
        let kept = crate::auth::create_session(c, bob)?;
        let out = sign_out(c, &Who::List(parse_list("ANN@example.org nobody@example.org")), "")?;
        assert_eq!(out, SignedOut { accounts: 1, sessions: 2 });
        assert_eq!(crate::auth::user_for_session(c, &kept)?.map(|u| u.id), Some(bob));
        Ok(())
    })
    .unwrap();
}

/// The notice goes to confirmed addresses only, once per text: a second
/// press with the same text queues nothing, a new text goes out again.
#[test]
fn a_notice_goes_once_to_each_confirmed_address() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let ann = crate::auth::create_user(c, "ann@example.org", "x")?;
        crate::auth::create_user(c, "bob@example.org", "x")?;
        let cy = crate::auth::create_user(c, "cy@example.org", "x")?;
        c.execute("UPDATE users SET email_verified_at = 1 WHERE id IN (?1, ?2)", params![ann, cy])?;
        assert_eq!(counts(c)?, (3, 2));
        let first = send_notice(c, &Who::Everyone, "Incident", "Hello", 100, None)?;
        assert_eq!(first, Sent { queued: 2, already: 0, left: 0 });
        let again = send_notice(c, &Who::Everyone, "Incident", "Hello", 200, None)?;
        assert_eq!(again, Sent { queued: 0, already: 2, left: 0 });
        let listed = send_notice(c, &Who::List(vec!["cy@example.org".into(), "bob@example.org".into()]), "Incident", "Update", 300, None)?;
        assert_eq!(listed, Sent { queued: 1, already: 0, left: 0 });
        assert_eq!(count(c, &format!("SELECT COUNT(*) FROM mail_outbox WHERE purpose = '{PURPOSE}' AND body_text = 'Hello'")), 2);
        assert_eq!(count(c, "SELECT COUNT(*) FROM mail_outbox WHERE to_addr = 'bob@example.org'"), 0);
        Ok(())
    })
    .unwrap();
}

/// Over the hourly ceiling (every hour it may wait for full) the rest are
/// left unrecorded, so the next press carries on.
#[test]
fn the_hourly_ceiling_leaves_the_rest_for_the_next_press() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        for name in ["a", "b", "c"] {
            let id = crate::auth::create_user(c, &format!("{name}@example.org"), "x")?;
            c.execute("UPDATE users SET email_verified_at = 1 WHERE id = ?1", [id])?;
        }
        // One per window, and a message may wait 12 windows: 13 fit.
        let ceiling = Some(crate::mail::Ceiling { per_window: 1, window: 3600 });
        for _ in 0..crate::mail::MAX_WAIT_WINDOWS - 1 {
            c.execute("INSERT INTO mail_outbox (to_addr, subject, body_text, purpose, next_attempt_at, scheduled_at, created_at) VALUES ('x@example.org', 's', 'b', ?1, 0, 0, 0)", [PURPOSE])?;
        }
        c.execute("UPDATE mail_outbox SET scheduled_at = rowid * 3600 - 3600", [])?;
        let first = send_notice(c, &Who::Everyone, "S", "B", 0, ceiling)?;
        assert_eq!(first, Sent { queued: 2, already: 0, left: 1 });
        let later = send_notice(c, &Who::Everyone, "S", "B", 3600 * 20, ceiling)?;
        assert_eq!(later, Sent { queued: 1, already: 2, left: 0 });
        Ok(())
    })
    .unwrap();
}
