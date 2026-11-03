use super::*;
use crate::db::Db;

fn vault() -> Vault {
    Vault::from_master(&[9u8; 32])
}

fn user(c: &Connection, email: &str) -> i64 {
    crate::auth::create_user(c, email, "h").unwrap()
}

/// An authenticator app set up and in use for `u`.
fn with_app(c: &Connection, u: i64, now: i64) -> totp::Key {
    let key = totp_start(c, &vault(), u, now).unwrap().unwrap();
    assert!(totp_confirm(c, &vault(), u, &key.code_at(now as u64), now).unwrap());
    key
}

#[test]
fn the_app_must_be_set_up_first_and_an_old_email_method_reads_as_off() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let u = user(c, "ann@example.org");
        assert_eq!(set_method(c, u, Some(Method::Totp))?, Err(crate::t("auth.two_factor.needs_app")));
        assert_eq!(Method::parse("email"), None);
        c.execute("UPDATE users SET two_factor = 'email' WHERE id = ?1", [u])?;
        assert_eq!(settings(c, u)?.method, None);
        assert_eq!(start_login(c, u, 1)?, None, "no second step: sign in at once");
        Ok(())
    })
    .unwrap();
}

#[test]
fn an_authenticator_is_enrolled_by_a_code_used_once_and_removed() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let u = user(c, "ann@example.org");
        let now = 1_800_000_000i64;
        let v = vault();
        let key = totp_start(c, &v, u, now)?.unwrap();
        assert_eq!((key.issuer.as_str(), key.account.as_str()), (crate::APP_NAME, "ann@example.org"));
        assert!(settings(c, u)?.totp_pending);
        assert_eq!(pending_key(c, &v, u)?.map(|k| k.secret), Some(key.secret.clone()));
        assert!(!totp_confirm(c, &v, u, "000000", now)? || key.code_at(now as u64) == "000000");
        assert!(totp_confirm(c, &v, u, &key.code_at(now as u64), now)?);
        let s = settings(c, u)?;
        assert_eq!((s.method, s.totp_since, s.totp_pending), (Some(Method::Totp), Some(now), false));
        assert_eq!(pending_key(c, &v, u)?, None);
        assert!(totp_start(c, &v, u, now)?.is_err(), "one app at a time");

        // Signing in: the enrolment's code (same step) is spent already.
        let ch = start_login(c, u, now)?.unwrap();
        assert_eq!(ch.method, Method::Totp);
        assert_eq!(c.query_row("SELECT token_hash FROM two_factor_challenges", [], |r| r.get::<_, String>(0))?, crate::auth::hash_token(&ch.token), "only the token's hash is stored");
        assert_eq!(check_login(c, &v, &ch.token, &key.code_at(now as u64), now)?, (Checked::Wrong(4), Some(u)));
        let later = now + 30;
        assert_eq!(check_login(c, &v, &ch.token, &key.code_at(later as u64), later)?, (Checked::Ok, Some(u)));
        assert_eq!(check_login(c, &v, &ch.token, &key.code_at(later as u64), later)?.0, Checked::Gone, "the challenge is used up");
        let ch = start_login(c, u, later)?.unwrap();
        assert_eq!(check_login(c, &v, &ch.token, &key.code_at(later as u64), later)?.0, Checked::Wrong(4), "a code works once");

        // A proof without a password: the app's next code, once.
        let next = later + 30;
        assert_eq!(check_confirm_code(c, &v, u, &key.code_at(later as u64), next)?, Checked::Wrong(MAX_ATTEMPTS), "spent at sign-in");
        assert_eq!(check_confirm_code(c, &v, u, &key.code_at(next as u64), next)?, Checked::Ok);
        assert_eq!(check_confirm_code(c, &v, u, &key.code_at(next as u64), next)?, Checked::Wrong(MAX_ATTEMPTS), "each step once");
        // Under another master secret the key does not open: no codes.
        assert_eq!(check_confirm_code(c, &Vault::from_master(&[1u8; 32]), u, &key.code_at(next as u64 + 30), next + 30)?, Checked::Wrong(MAX_ATTEMPTS));

        // Turning it off removes the app's key.
        assert_eq!(set_method(c, u, None)?, Ok(()));
        assert_eq!(c.query_row("SELECT COUNT(*) FROM totp_keys", [], |r| r.get::<_, i64>(0))?, 0);
        let key = totp_start(c, &v, u, now)?.unwrap();
        assert!(totp_confirm(c, &v, u, &key.code_at(now as u64 + 60), now + 60)?);
        totp_remove(c, u)?;
        assert_eq!(settings(c, u)?.method, None);
        Ok(())
    })
    .unwrap();
}

#[test]
fn a_new_sign_in_replaces_the_challenge_and_five_wrong_codes_end_it() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let u = user(c, "ann@example.org");
        let now = 1_800_000_000;
        let key = with_app(c, u, now);
        let a = start_login(c, u, now)?.unwrap();
        let b = start_login(c, u, now)?.unwrap();
        assert_eq!(pending(c, &a.token, now)?, None);
        assert!(pending(c, &b.token, now)?.is_some());
        cancel(c, &b.token)?;
        assert_eq!(pending(c, &b.token, now)?, None);

        let v = vault();
        let ch = start_login(c, u, now)?.unwrap();
        assert_eq!(check_login(c, &v, &crate::auth::generate_token(), "123456", now)?, (Checked::Gone, None));
        let right = key.code_at(now as u64 + 30);
        let wrong = if right == "999999" { "999998" } else { "999999" };
        for left in (1..MAX_ATTEMPTS).rev() {
            assert_eq!(check_login(c, &v, &ch.token, wrong, now + 30)?.0, Checked::Wrong(left));
        }
        assert_eq!(check_login(c, &v, &ch.token, wrong, now + 30)?, (Checked::Gone, Some(u)), "the last wrong code says whose it was");
        assert_eq!(check_login(c, &v, &ch.token, &right, now + 30)?.0, Checked::Gone, "five wrong codes end the sign-in");
        let ch = start_login(c, u, now)?.unwrap();
        assert_eq!(check_login(c, &v, &ch.token, &right, now + CODE_TTL_SECS)?.0, Checked::Gone, "expired");
        Ok(())
    })
    .unwrap();
}

#[test]
fn every_change_of_method_needs_proof() {
    use Method::*;
    assert!(needs_proof(None, Some(Totp)), "turning it on too (#374)");
    assert!(!needs_proof(None, None) && !needs_proof(Some(Totp), Some(Totp)));
    assert!(needs_proof(Some(Totp), None));
}

#[test]
fn the_authenticator_secret_is_sealed_in_its_row() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let u = user(c, "ann@example.org");
        let key = totp_start(c, &vault(), u, 1_800_000_000)?.unwrap();
        let blob: Vec<u8> = c.query_row("SELECT secret FROM totp_keys WHERE user_id = ?1", [u], |r| r.get(0))?;
        let b32 = key.secret_base32();
        assert!(!String::from_utf8_lossy(&blob).contains(&b32), "the base32 secret is stored as is");
        assert!(!blob.windows(key.secret.len()).any(|w| w == key.secret.as_slice()), "the raw secret is stored as is");
        assert_eq!(secrets::open_totp(&vault(), u, &blob), Some(key.secret.clone()));
        // Moved to another user's row, it does not open.
        let bob = user(c, "bob@example.org");
        c.execute("INSERT INTO totp_keys (user_id, secret, created_at) VALUES (?1, ?2, 1)", params![bob, blob])?;
        assert_eq!(pending_key(c, &vault(), bob)?, None);
        Ok(())
    })
    .unwrap();
}

#[test]
fn the_purge_clears_old_challenges_and_abandoned_enrolments() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let u = user(c, "ann@example.org");
        with_app(c, u, 100);
        start_login(c, u, 100)?.unwrap();
        let bob = user(c, "bob@example.org");
        totp_start(c, &vault(), bob, 100)?.unwrap();
        assert_eq!(purge(c, 100 + CODE_TTL_SECS + 1)?, 1);
        assert_eq!(purge(c, 100 + 86_401)?, 1, "an abandoned enrolment");
        Ok(())
    })
    .unwrap();
}

/// #613: the hourly purge of abandoned enrolments reads only unconfirmed keys.
#[test]
fn the_enrolment_purge_uses_its_index() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        crate::db::tests::assert_plan_uses(c, PURGE_ENROLMENTS, "totp_keys", "idx_totp_keys_pending");
        Ok(())
    })
    .unwrap();
}
