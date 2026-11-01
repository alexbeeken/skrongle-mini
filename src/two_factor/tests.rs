use super::*;
use crate::db::Db;

fn vault() -> Vault {
    Vault::from_master(&[9u8; 32])
}

fn user(c: &Connection, email: &str) -> i64 {
    crate::auth::create_user(c, email, "h").unwrap()
}

fn verify_email(c: &Connection, u: i64) {
    crate::verify::mark_verified(c, u, 1).unwrap();
}

#[test]
fn codes_are_six_digits() {
    for _ in 0..200 {
        let c = new_code();
        assert_eq!(c.len(), 6);
        assert!(c.bytes().all(|b| b.is_ascii_digit()));
    }
    assert_ne!(new_salt(), new_salt());
}

#[test]
fn methods_need_a_verified_address_or_app() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let u = user(c, "ann@example.org");
        assert_eq!(set_method(c, u, Some(Method::Email))?, Err(crate::t("auth.two_factor.needs_email")));
        assert_eq!(set_method(c, u, Some(Method::Totp))?, Err(crate::t("auth.two_factor.needs_app")));
        verify_email(c, u);
        assert_eq!(set_method(c, u, Some(Method::Email))?, Ok(()));
        assert_eq!(settings(c, u)?.method, Some(Method::Email));
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
        assert_eq!((ch.method, ch.send.clone()), (Method::Totp, None));
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

        // Switching to e-mail removes the app's key.
        verify_email(c, u);
        assert_eq!(set_method(c, u, Some(Method::Email))?, Ok(()));
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
fn a_login_code_is_bound_to_its_token_expires_and_allows_five_tries() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let v = vault();
        let u = user(c, "ann@example.org");
        assert_eq!(start_login(c, u, 1)?, None, "no second step: sign in at once");
        verify_email(c, u);
        assert_eq!(set_method(c, u, Some(Method::Email))?, Ok(()));
        let now = 10_000;
        let ch = start_login(c, u, now)?.unwrap();
        let (to, code) = ch.send.clone().unwrap();
        assert_eq!(to, "ann@example.org");
        // Only the token's hash is stored, and the code's hash needs the token.
        let (th, ch_hash): (String, String) = c.query_row("SELECT token_hash, code_hash FROM two_factor_challenges", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
        assert_eq!(th, crate::auth::hash_token(&ch.token));
        assert!(!ch_hash.contains(&code) && th != ch.token);
        let other = crate::auth::generate_token();
        assert_eq!(check_login(c, &v, &other, &code, now)?, (Checked::Gone, None));
        assert_eq!(check_login(c, &v, &ch.token, "000000", now + 1)?.0, if code == "000000" { Checked::Ok } else { Checked::Wrong(4) });
        if code != "000000" {
            assert_eq!(check_login(c, &v, &ch.token, &code, now + CODE_TTL_SECS)?.0, Checked::Gone, "expired");
        }

        let ch = start_login(c, u, now)?.unwrap();
        let (_, code) = ch.send.clone().unwrap();
        let wrong = if code == "999999" { "999998" } else { "999999" };
        for left in (1..MAX_ATTEMPTS).rev() {
            assert_eq!(check_login(c, &v, &ch.token, wrong, now)?.0, Checked::Wrong(left));
        }
        assert_eq!(check_login(c, &v, &ch.token, wrong, now)?.0, Checked::Gone);
        assert_eq!(check_login(c, &v, &ch.token, &code, now)?.0, Checked::Gone, "five wrong codes end the sign-in");

        // A resend replaces the code; a new sign-in replaces the challenge.
        let ch = start_login(c, u, now)?.unwrap();
        let (_, first) = ch.send.clone().unwrap();
        let (_, second) = new_login_code(c, &ch.token, now + 5)?.unwrap();
        if first != second {
            assert_eq!(check_login(c, &v, &ch.token, &first, now + 6)?.0, Checked::Wrong(4));
        }
        assert_eq!(check_login(c, &v, &ch.token, &second, now + 6)?, (Checked::Ok, Some(u)));
        Ok(())
    })
    .unwrap();
}

#[test]
fn a_resend_keeps_the_wrong_codes_already_typed() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let v = vault();
        let u = user(c, "ann@example.org");
        verify_email(c, u);
        assert_eq!(set_method(c, u, Some(Method::Email))?, Ok(()));
        let now = 10_000;
        let ch = start_login(c, u, now)?.unwrap();
        let mut code = ch.send.clone().unwrap().1;
        // Two wrong codes, a resend, two more, a resend, one more: the
        // fifth wrong code ends the sign-in, however many codes were sent.
        for (i, left) in (1..MAX_ATTEMPTS).rev().enumerate() {
            if i % 2 == 0 && i > 0 {
                code = new_login_code(c, &ch.token, now + i as i64)?.unwrap().1;
            }
            let wrong = if code == "999999" { "999998" } else { "999999" };
            assert_eq!(check_login(c, &v, &ch.token, wrong, now + i as i64)?.0, Checked::Wrong(left));
        }
        code = new_login_code(c, &ch.token, now + 10)?.unwrap().1;
        let wrong = if code == "999999" { "999998" } else { "999999" };
        assert_eq!(check_login(c, &v, &ch.token, wrong, now + 10)?, (Checked::Gone, Some(u)), "the last wrong code says whose it was");
        assert_eq!(new_login_code(c, &ch.token, now + 11)?, None, "no resend after the last try");
        assert_eq!(check_login(c, &v, &ch.token, &code, now + 11)?.0, Checked::Gone);
        Ok(())
    })
    .unwrap();
}

#[test]
fn a_new_sign_in_replaces_the_challenge() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let u = user(c, "ann@example.org");
        verify_email(c, u);
        assert_eq!(set_method(c, u, Some(Method::Email))?, Ok(()));
        let now = 10_000;
        let a = start_login(c, u, now)?.unwrap();
        let b = start_login(c, u, now)?.unwrap();
        assert_eq!(pending(c, &a.token, now)?, None);
        assert!(pending(c, &b.token, now)?.is_some());
        cancel(c, &b.token)?;
        assert_eq!(pending(c, &b.token, now)?, None);
        Ok(())
    })
    .unwrap();
}

#[test]
fn the_mails_say_how_to_sign_out_everywhere_else() {
    let confirm = confirm_mail("ana@example.org", "123456", 1).unwrap();
    assert!(!confirm.text.contains("changing how you sign in"), "that never signed anyone out (#375)");
    assert!(confirm.text.contains("Sign out everywhere else") && confirm.html.unwrap().contains("Sign out everywhere else"));
    let off = changed_mail("ana@example.org", None, 1).unwrap();
    assert_eq!((off.purpose, off.user_id), (PURPOSE_CHANGED, Some(1)));
    assert!(off.text.contains("is now off") && off.text.contains("Sign out everywhere else"));
}

#[test]
fn every_change_of_method_needs_proof() {
    use Method::*;
    assert!(needs_proof(None, Some(Email)) && needs_proof(None, Some(Totp)), "turning it on too (#374)");
    assert!(!needs_proof(None, None) && !needs_proof(Some(Email), Some(Email)));
    assert!(needs_proof(Some(Email), None) && needs_proof(Some(Totp), Some(Email)) && needs_proof(Some(Email), Some(Totp)));
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
fn a_confirm_code_goes_to_the_current_method_and_allows_five_tries() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let v = vault();
        let u = user(c, "ann@example.org");
        assert_eq!(issue_confirm_code(c, u, 1)?, None, "nothing to turn off");
        verify_email(c, u);
        assert_eq!(set_method(c, u, Some(Method::Email))?, Ok(()));
        let (m, to, code) = issue_confirm_code(c, u, 100)?.unwrap();
        assert_eq!((m, to.as_str()), (Method::Email, "ann@example.org"));
        let wrong = if code == "999999" { "999998" } else { "999999" };
        assert_eq!(check_confirm_code(c, &v, u, wrong, 101)?, Checked::Wrong(4));
        assert_eq!(check_confirm_code(c, &v, u, &code, 100 + CODE_TTL_SECS)?, Checked::Gone, "expired");
        assert_eq!(check_confirm_code(c, &v, u, &code, 102)?, Checked::Ok);
        assert_eq!(check_confirm_code(c, &v, u, &code, 103)?, Checked::Gone, "used once");
        let (_, _, code) = issue_confirm_code(c, u, 200)?.unwrap();
        for left in (1..MAX_ATTEMPTS).rev() {
            assert_eq!(check_confirm_code(c, &v, u, if code == "999999" { "999998" } else { "999999" }, 201)?, Checked::Wrong(left));
        }
        assert_eq!(check_confirm_code(c, &v, u, if code == "999999" { "999998" } else { "999999" }, 201)?, Checked::Gone);
        assert_eq!(check_confirm_code(c, &v, u, &code, 201)?, Checked::Gone, "five wrong codes use it up");
        // An unused code is cleared by the hourly purge.
        issue_confirm_code(c, u, 300)?.unwrap();
        assert_eq!(purge(c, 300 + CODE_TTL_SECS + 1)?, 1);
        Ok(())
    })
    .unwrap();
}

#[test]
fn the_purge_clears_old_challenges_and_abandoned_enrolments() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let u = user(c, "ann@example.org");
        verify_email(c, u);
        assert_eq!(set_method(c, u, Some(Method::Email))?, Ok(()));
        start_login(c, u, 100)?.unwrap();
        assert_eq!(masked(Method::Email, "ann@example.org"), "a•••@example.org");
        totp_start(c, &vault(), u, 100)?.unwrap();
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
