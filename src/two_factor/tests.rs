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

/// The code `issue_phone_code` made can't be read back (hashed), so the
/// tests make their own row with a known code.
fn put_phone_code(c: &Connection, u: i64, code: &str, now: i64) {
    let phone: String = c.query_row("SELECT phone FROM users WHERE id = ?1", [u], |r| r.get(0)).unwrap();
    c.execute("DELETE FROM phone_verifications WHERE user_id = ?1", [u]).unwrap();
    c.execute(
        "INSERT INTO phone_verifications (user_id, phone, code_hash, salt, created_at, expires_at) VALUES (?1, ?2, ?3, 's', ?4, ?5)",
        params![u, phone, hash(&["s", code]), now, now + CODE_TTL_SECS],
    )
    .unwrap();
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
fn methods_need_a_verified_address_number_or_app() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let u = user(c, "ann@example.org");
        assert_eq!(set_method(c, u, Some(Method::Email))?, Err(crate::t("auth.two_factor.needs_email")));
        assert_eq!(set_method(c, u, Some(Method::Sms))?, Err(crate::t("auth.two_factor.needs_phone")));
        assert_eq!(set_method(c, u, Some(Method::Totp))?, Err(crate::t("auth.two_factor.needs_app")));
        verify_email(c, u);
        assert_eq!(set_method(c, u, Some(Method::Email))?, Ok(()));
        assert_eq!(settings(c, u)?.method, Some(Method::Email));
        // A number is not enough until a code confirms it.
        set_phone(c, u, "+447700900123")?;
        assert_eq!(set_method(c, u, Some(Method::Sms))?, Err(crate::t("auth.two_factor.needs_phone")));
        assert!(matches!(issue_phone_code(c, u, 100)?, PhoneCode::Send { .. }));
        put_phone_code(c, u, "123456", 100);
        assert_eq!(confirm_phone(c, u, "000000", 101)?, Checked::Wrong(4));
        assert_eq!(confirm_phone(c, u, "123 456", 102)?, Checked::Ok);
        assert!(settings(c, u)?.phone_verified);
        assert_eq!(issue_phone_code(c, u, 103)?, PhoneCode::Nothing, "already confirmed");
        assert_eq!(set_method(c, u, Some(Method::Sms))?, Ok(()));
        // The number can't change under SMS sign-in; once off it can, unconfirmed again.
        assert!(phone_change_refusal(c, u, "+447700900999")?.is_some());
        assert!(phone_change_refusal(c, u, "+447700900123")?.is_none());
        assert_eq!(set_method(c, u, None)?, Ok(()));
        assert!(phone_change_refusal(c, u, "+447700900999")?.is_none());
        set_phone(c, u, "+447700900999")?;
        assert!(!settings(c, u)?.phone_verified);
        Ok(())
    })
    .unwrap();
}

/// #431: a phone number (and its code) needs a confirmed address;
/// clearing one never does.
#[test]
fn a_phone_needs_a_confirmed_address() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let u = user(c, "ann@example.org");
        assert_eq!(phone_change_refusal(c, u, "+447700900123")?, Some(NEEDS_CONFIRMED_EMAIL));
        set_phone(c, u, "+447700900123")?;
        assert_eq!(issue_phone_code(c, u, 1)?, PhoneCode::Refused(NEEDS_CONFIRMED_EMAIL));
        assert_eq!(phone_change_refusal(c, u, "")?, None, "clearing is always allowed");
        verify_email(c, u);
        assert_eq!(phone_change_refusal(c, u, "+447700900999")?, None);
        assert!(matches!(issue_phone_code(c, u, 2)?, PhoneCode::Send { .. }));
        Ok(())
    })
    .unwrap();
}

/// #363: a number verified on one account is refused, not texted and
/// not verified on another; the database holds it to one account.
#[test]
fn a_number_verified_on_one_account_is_refused_on_another() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let (ann, bob) = (user(c, "ann@example.org"), user(c, "bob@example.org"));
        verify_email(c, ann);
        verify_email(c, bob);
        let n = "+447700900123";
        // Both set it before either verified it.
        set_phone(c, ann, n)?;
        set_phone(c, bob, n)?;
        put_phone_code(c, bob, "222222", 100);
        put_phone_code(c, ann, "111111", 100);
        assert_eq!(confirm_phone(c, ann, "111111", 101)?, Checked::Ok);
        assert_eq!(phone_change_refusal(c, bob, "+44 7700 900999")?, None);
        set_phone(c, bob, "")?;
        assert_eq!(phone_change_refusal(c, bob, n)?, Some(PHONE_TAKEN));
        c.execute("UPDATE users SET phone = ?2 WHERE id = ?1", params![bob, n])?;
        assert_eq!(issue_phone_code(c, bob, 102)?, PhoneCode::Refused(PHONE_TAKEN));
        put_phone_code(c, bob, "222222", 102);
        assert_eq!(confirm_phone(c, bob, "222222", 103)?, Checked::Gone);
        assert!(phone_taken(c, bob, n)? && !settings(c, bob)?.phone_verified);
        // The unique index refuses a second verified copy outright.
        assert!(c.execute("UPDATE users SET phone_verified_at = 1 WHERE id = ?1", [bob]).is_err());
        // Ann frees it: Bob may verify it.
        set_phone(c, ann, "")?;
        assert!(!phone_taken(c, bob, n)?);
        put_phone_code(c, bob, "222222", 104);
        assert_eq!(confirm_phone(c, bob, "222222", 105)?, Checked::Ok);
        Ok(())
    })
    .unwrap();
}

#[test]
fn a_phone_code_expires_and_allows_five_tries() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let u = user(c, "ann@example.org");
        assert_eq!(issue_phone_code(c, u, 1)?, PhoneCode::Nothing, "no number");
        set_phone(c, u, "+447700900123")?;
        put_phone_code(c, u, "111111", 1000);
        assert_eq!(confirm_phone(c, u, "111111", 1000 + CODE_TTL_SECS)?, Checked::Gone, "expired");
        put_phone_code(c, u, "111111", 1000);
        for left in (1..MAX_ATTEMPTS).rev() {
            assert_eq!(confirm_phone(c, u, "222222", 1001)?, Checked::Wrong(left));
        }
        assert_eq!(confirm_phone(c, u, "222222", 1001)?, Checked::Gone);
        assert_eq!(confirm_phone(c, u, "111111", 1001)?, Checked::Gone, "used up");
        // A code sent to the old number does not confirm a new one.
        put_phone_code(c, u, "111111", 1000);
        set_phone(c, u, "+447700900999")?;
        assert_eq!(confirm_phone(c, u, "111111", 1001)?, Checked::Gone);
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
    assert!(changed_mail("ana@example.org", Some(Method::Sms), 1).unwrap().text.contains("texts a code"));
}

#[test]
fn every_change_of_method_needs_proof() {
    use Method::*;
    assert!(needs_proof(None, Some(Email)) && needs_proof(None, Some(Totp)) && needs_proof(None, Some(Sms)), "turning it on too (#374)");
    assert!(!needs_proof(None, None) && !needs_proof(Some(Sms), Some(Sms)));
    assert!(needs_proof(Some(Sms), None) && needs_proof(Some(Sms), Some(Email)) && needs_proof(Some(Totp), Some(Sms)) && needs_proof(Some(Email), Some(Totp)));
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
        // A code sent for e-mail does not count once the method changed.
        let (_, _, code) = issue_confirm_code(c, u, 300)?.unwrap();
        set_phone(c, u, "+447700900123")?;
        put_phone_code(c, u, "123456", 300);
        confirm_phone(c, u, "123456", 300)?;
        assert_eq!(set_method(c, u, Some(Method::Sms))?, Ok(()));
        assert_eq!(check_confirm_code(c, &v, u, &code, 301)?, Checked::Gone);
        assert_eq!(purge(c, 300 + CODE_TTL_SECS + 1)?, 1);
        Ok(())
    })
    .unwrap();
}

#[test]
fn sms_sign_in_texts_the_confirmed_number_and_purge_clears_old_rows() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let u = user(c, "ann@example.org");
        set_phone(c, u, "+447700900123")?;
        put_phone_code(c, u, "123456", 1);
        assert_eq!(confirm_phone(c, u, "123456", 2)?, Checked::Ok);
        assert_eq!(set_method(c, u, Some(Method::Sms))?, Ok(()));
        let ch = start_login(c, u, 100)?.unwrap();
        assert_eq!(ch.send.as_ref().unwrap().0, "+447700900123");
        assert_eq!(masked(Method::Sms, "", "+447700900123"), "•••• 23");
        assert_eq!(masked(Method::Email, "ann@example.org", ""), "a•••@example.org");
        totp_start(c, &vault(), u, 100)?.unwrap();
        assert_eq!(purge(c, 100 + CODE_TTL_SECS + 1)?, 1);
        assert_eq!(purge(c, 100 + 86_401)?, 1, "an abandoned enrolment");
        Ok(())
    })
    .unwrap();
}

/// #969: a number that opted out of texts gets its sign-in and confirm
/// codes by e-mail, and is offered a new phone code; a new number starts
/// over.
#[test]
fn an_opted_out_number_gets_its_codes_by_email() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let u = user(c, "ann@example.org");
        verify_email(c, u);
        set_phone(c, u, "+447700900123")?;
        put_phone_code(c, u, "123456", 1);
        assert_eq!(confirm_phone(c, u, "123456", 2)?, Checked::Ok);
        assert_eq!(set_method(c, u, Some(Method::Sms))?, Ok(()));
        assert_eq!(issue_phone_code(c, u, 3)?, PhoneCode::Nothing);
        c.execute("UPDATE users SET phone_opted_out_at = 50 WHERE id = ?1", [u])?;
        let ch = start_login(c, u, 100)?.unwrap();
        assert!(ch.by_email);
        let (to, code) = ch.send.clone().unwrap();
        assert_eq!(to, "ann@example.org");
        let p = pending(c, &ch.token, 101)?.unwrap();
        assert!(p.emailed && p.by_email());
        assert_eq!(p.sent_to(), "a•••@example.org");
        assert_eq!(check_login(c, &vault(), &ch.token, &code, 102)?.0, Checked::Ok);
        assert_eq!(issue_confirm_code(c, u, 103)?.map(|(m, to, _)| (m, to)), Some((Method::Email, "ann@example.org".to_string())));
        assert!(matches!(issue_phone_code(c, u, 104)?, PhoneCode::Send { .. }), "a code to check the number takes texts again");
        set_phone(c, u, "+447700900999")?;
        assert!(!settings(c, u)?.phone_opted_out);
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
