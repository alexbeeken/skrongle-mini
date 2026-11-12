use super::*;
use crate::db::Db;

const NOW: i64 = 1_000_000;

fn new_grant<'a>(scopes: &'a [Scope], courses: &'a [String], lifetime: Lifetime) -> NewGrant<'a> {
    NewGrant { client_id: PERSONAL_CLIENT_ID, client_name: "Test", scopes, courses, lifetime }
}

/// A personal token for `uid` with both scopes, 90 days.
fn personal(c: &Connection, uid: i64) -> String {
    make_personal(c, uid, &new_grant(&Scope::ALL, &[], Lifetime::Days90), NOW).unwrap().unwrap()
}

fn setup(c: &Connection) -> i64 {
    crate::settings::set_apps_on(c, true).unwrap();
    crate::perms::create_teacher(c, "ann@example.org", "h").unwrap()
}

fn found(l: Lookup) -> Option<ApiUser> {
    match l {
        Lookup::Found(u) => Some(u),
        _ => None,
    }
}

fn stored_hashes(c: &Connection) -> Vec<String> {
    let mut stmt = c.prepare("SELECT token_hash FROM api_tokens").unwrap();
    stmt.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
}

/// Hashed at rest: after making a token, no column of either table holds
/// it in plain text, only its SHA-256 is stored, and that hash is no token.
#[test]
fn tokens_are_stored_hashed_only() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let ann = setup(c);
        let token = personal(c, ann);
        assert!(token.starts_with(TOKEN_PREFIX) && token.len() == TOKEN_PREFIX.len() + 64, "{token}");
        assert_eq!(stored_hashes(c), vec![hash_token(&token)]);
        // Every column of every row, as text: the token is nowhere.
        let dumps = [
            "SELECT group_concat(id || kind || token_hash || expires_at) FROM api_tokens",
            "SELECT group_concat(id || client_id || client_name || scopes || courses || created_at || expires_at) FROM api_grants",
        ];
        for sql in dumps {
            let dump: String = c.query_row(sql, [], |r| r.get(0))?;
            assert!(!dump.contains(&token[TOKEN_PREFIX.len()..]), "{sql} holds the token");
        }
        assert_eq!(found(user_for_token(c, &token, NOW + 1)?).map(|u| u.id), Some(ann));
        assert!(found(user_for_token(c, &hash_token(&token), NOW + 1)?).is_none(), "the stored hash signs nobody in");
        Ok(())
    })
    .unwrap();
}

/// Checked on every request: the switch, the grant's expiry, Create
/// courses, the temporary-password gate. Each "no" is Refused (Off for the
/// switch), and each "yes" again works at once: nothing is cached.
#[test]
fn every_condition_is_checked_on_every_lookup() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let ann = setup(c);
        let token = personal(c, ann);
        let check = |c: &Connection, at: i64| user_for_token(c, &token, at).unwrap();
        assert!(matches!(check(c, NOW), Lookup::Found(_)));

        crate::settings::set_apps_on(c, false)?;
        assert!(matches!(check(c, NOW), Lookup::Off), "the switch is off");
        crate::settings::set_apps_on(c, true)?;
        assert!(matches!(check(c, NOW), Lookup::Found(_)), "on again: the grant is back");

        crate::perms::apply_preset(c, ann, crate::perms::STUDENT_PRESET)?;
        assert!(matches!(check(c, NOW), Lookup::Refused), "no Create courses");
        crate::perms::apply_preset(c, ann, crate::perms::TEACHER_PRESET)?;
        assert!(matches!(check(c, NOW), Lookup::Found(_)));
        // An owner has every permission, whatever is stored.
        crate::perms::apply_preset(c, ann, crate::perms::STUDENT_PRESET)?;
        c.execute("UPDATE users SET owner = 1 WHERE id = ?1", [ann])?;
        assert!(matches!(check(c, NOW), Lookup::Found(_)), "an owner may");
        c.execute("UPDATE users SET owner = 0 WHERE id = ?1", [ann])?;
        crate::perms::apply_preset(c, ann, crate::perms::TEACHER_PRESET)?;

        c.execute("UPDATE users SET must_change_password = 1 WHERE id = ?1", [ann])?;
        assert!(matches!(check(c, NOW), Lookup::Refused), "waiting behind a temporary password");
        c.execute("UPDATE users SET must_change_password = 0 WHERE id = ?1", [ann])?;

        let end = NOW + Lifetime::Days90.secs();
        assert!(matches!(check(c, end - 1), Lookup::Found(_)));
        assert!(matches!(check(c, end), Lookup::Refused), "expired");
        Ok(())
    })
    .unwrap();
}

/// A token past its own expiry stops even when its grant runs longer, and
/// a refresh token is no access token.
#[test]
fn token_expiry_and_kind_are_checked() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let ann = setup(c);
        let g = create_grant(c, ann, &new_grant(&[Scope::Read], &[], Lifetime::Year), NOW)?.unwrap();
        let access = mint(c, g, Kind::Access, NOW + 3600)?.unwrap();
        let refresh = mint(c, g, Kind::Refresh, NOW + 86_400)?.unwrap();
        assert!(found(user_for_token(c, &access, NOW + 3599)?).is_some());
        assert!(found(user_for_token(c, &access, NOW + 3600)?).is_none());
        assert!(found(user_for_token(c, &refresh, NOW)?).is_none(), "a refresh token opens nothing");
        Ok(())
    })
    .unwrap();
}

/// Expiry is real: the purge deletes expired tokens and grants, and every
/// grant has an expiry (the column is NOT NULL).
#[test]
fn expired_grants_and_tokens_are_purged() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let ann = setup(c);
        personal(c, ann);
        let short = create_grant(c, ann, &new_grant(&[Scope::Read], &[], Lifetime::Days30), NOW)?.unwrap();
        mint(c, short, Kind::Access, NOW + 60)?.unwrap();
        assert_eq!(purge_expired(c, NOW + 61)?, 1, "the short token");
        assert_eq!(purge_expired(c, NOW + Lifetime::Days30.secs())?, 1, "the 30-day grant");
        assert_eq!(grants(c, ann, NOW)?.len(), 1);
        assert_eq!(purge_expired(c, NOW + Lifetime::Days90.secs())?, 2, "the personal token and its grant");
        assert!(stored_hashes(c).is_empty());
        let no_expiry = c.execute("INSERT INTO api_grants (user_id, client_id, client_name, scopes, created_at, expires_at) VALUES (?1, 'x', 'x', 'courses:read', 1, NULL)", [ann]);
        assert!(no_expiry.is_err(), "a grant without an expiry");
        Ok(())
    })
    .unwrap();
}

/// At most four grants per account; the fifth is refused with a message
/// saying to revoke one, and another account is not counted against it.
#[test]
fn a_fifth_grant_is_refused() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let ann = setup(c);
        let bea = crate::perms::create_teacher(c, "bea@example.org", "h")?;
        for _ in 0..MAX_GRANTS_PER_ACCOUNT {
            personal(c, ann);
        }
        let fifth = make_personal(c, ann, &new_grant(&Scope::ALL, &[], Lifetime::Days30), NOW)?;
        assert_eq!(fifth, Err(Refused::TooManyGrants));
        assert!(Refused::TooManyGrants.message().contains('4'));
        assert!(make_personal(c, bea, &new_grant(&Scope::ALL, &[], Lifetime::Days30), NOW)?.is_ok());
        assert_eq!(create_grant(c, bea, &new_grant(&[], &[], Lifetime::Days30), NOW)?, Err(Refused::NoScope));
        Ok(())
    })
    .unwrap();
}

/// The server-wide caps: a full `api_grants` or `api_tokens` refuses.
#[test]
fn full_tables_refuse() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let ann = setup(c);
        let g = create_grant(c, ann, &new_grant(&[Scope::Read], &[], Lifetime::Days30), NOW)?.unwrap();
        c.execute(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < ?2)
             INSERT INTO api_tokens (grant_id, kind, token_hash, expires_at) SELECT ?1, 'access', 'h' || i, ?3 FROM n",
            params![g, MAX_API_TOKENS as i64, NOW + 10],
        )?;
        assert_eq!(mint(c, g, Kind::Access, NOW + 10)?, Err(Refused::Full));
        c.execute("DELETE FROM api_grants", [])?;
        let mut users = Vec::new();
        for i in 0..MAX_API_GRANTS / MAX_GRANTS_PER_ACCOUNT {
            users.push(crate::perms::create_teacher(c, &format!("u{i}@example.org"), "h")?);
        }
        for u in &users {
            for _ in 0..MAX_GRANTS_PER_ACCOUNT {
                create_grant(c, *u, &new_grant(&[Scope::Read], &[], Lifetime::Days30), NOW)?.unwrap();
            }
        }
        let bea = crate::perms::create_teacher(c, "bea@example.org", "h")?;
        assert_eq!(create_grant(c, bea, &new_grant(&[Scope::Read], &[], Lifetime::Days30), NOW)?, Err(Refused::Full));
        Ok(())
    })
    .unwrap();
}

/// Revoked by events and by hand: revoking takes the tokens with the grant;
/// one account cannot revoke another's; deleting the account deletes both
/// tables' rows (ON DELETE CASCADE).
#[test]
fn revoke_and_cascade() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let ann = setup(c);
        let bea = crate::perms::create_teacher(c, "bea@example.org", "h")?;
        let token = personal(c, ann);
        let g = grants(c, ann, NOW)?[0].id;
        assert!(!revoke(c, bea, g)?, "Bea can't revoke Ann's grant");
        assert!(found(user_for_token(c, &token, NOW)?).is_some());
        assert!(revoke(c, ann, g)?);
        assert!(found(user_for_token(c, &token, NOW)?).is_none());
        assert!(stored_hashes(c).is_empty(), "the tokens went with the grant");

        personal(c, ann);
        personal(c, ann);
        personal(c, bea);
        assert_eq!(revoke_all_for_user(c, ann)?, 2);
        assert_eq!(all_grants(c, NOW)?.len(), 1, "only Bea's");

        personal(c, ann);
        c.execute("DELETE FROM users WHERE id = ?1", [ann])?;
        let left: i64 = c.query_row("SELECT COUNT(*) FROM api_grants WHERE user_id = ?1", [ann], |r| r.get(0))?;
        assert_eq!(left, 0);
        assert_eq!(stored_hashes(c).len(), 1, "Bea's token stays");
        Ok(())
    })
    .unwrap();
}

/// `last_used_at` is written at most once a minute per grant.
#[test]
fn last_used_is_written_at_most_once_a_minute() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let ann = setup(c);
        let token = personal(c, ann);
        assert_eq!(grants(c, ann, NOW)?[0].last_used_at, None);
        user_for_token(c, &token, NOW + 10)?;
        assert_eq!(grants(c, ann, NOW)?[0].last_used_at, Some(NOW + 10));
        let writes = c.total_changes();
        user_for_token(c, &token, NOW + 10 + LAST_USED_EVERY_SECS - 1)?;
        assert_eq!(c.total_changes(), writes, "no write within the minute");
        assert_eq!(grants(c, ann, NOW)?[0].last_used_at, Some(NOW + 10));
        user_for_token(c, &token, NOW + 10 + LAST_USED_EVERY_SECS)?;
        assert_eq!(grants(c, ann, NOW)?[0].last_used_at, Some(NOW + 10 + LAST_USED_EVERY_SECS));
        Ok(())
    })
    .unwrap();
}

/// Constant work for a bad token: the lookup is one search of the unique
/// hash index, never a scan or a comparison of tokens.
#[test]
fn tokens_are_looked_up_by_hash_through_the_index() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        assert_eq!(crate::db::table_scans(c, TOKEN_SQL).unwrap(), Vec::<String>::new());
        let plan: Vec<String> = {
            let mut stmt = c.prepare(&format!("EXPLAIN QUERY PLAN {TOKEN_SQL}"))?;
            let rows = stmt.query_map(params!["x", 1], |r| r.get::<_, String>(3))?;
            rows.collect::<Result<_, _>>()?
        };
        assert!(plan.iter().any(|p| p.contains("api_tokens") && p.contains("token_hash")), "{plan:?}");
        Ok(())
    })
    .unwrap();
}

#[test]
fn scopes_and_course_lists() {
    assert_eq!(Scope::parse_list("courses:write nonsense courses:read"), vec![Scope::Read, Scope::Write]);
    assert_eq!(Scope::parse("courses:publish"), None, "no publish scope on a mini");
    assert_eq!(Scope::join(&Scope::ALL), "courses:read courses:write");
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let ann = setup(c);
        let only = vec!["mine".to_string()];
        let token = make_personal(c, ann, &new_grant(&[Scope::Read], &only, Lifetime::Days30), NOW)?.unwrap();
        let u = found(user_for_token(c, &token, NOW)?).unwrap();
        assert!(u.require(Scope::Read).is_ok());
        assert!(matches!(u.require(Scope::Write), Err(ApiError::InsufficientScope(Scope::Write))));
        assert!(u.allows_course("mine") && !u.allows_course("other"));
        Ok(())
    })
    .unwrap();
    for l in Lifetime::ALL {
        assert_eq!(Lifetime::parse(l.id()), Some(l));
    }
    assert_eq!(Lifetime::parse("forever"), None, "no grant without an expiry");
}

#[test]
fn bearer_header_and_token_in_the_address() {
    let mut h = HeaderMap::new();
    assert_eq!(bearer(&h), None);
    let tok = format!("{TOKEN_PREFIX}{}", "A".repeat(64));
    h.insert(AUTHORIZATION, format!("Bearer {tok}").parse().unwrap());
    assert_eq!(bearer(&h), Some(format!("{TOKEN_PREFIX}{}", "a".repeat(64))));
    h.insert(AUTHORIZATION, "Bearer short".parse().unwrap());
    assert_eq!(bearer(&h).as_deref(), Some(""));
    h.insert(AUTHORIZATION, format!("Basic {tok}").parse().unwrap());
    assert_eq!(bearer(&h).as_deref(), Some(""));
    h.insert(AUTHORIZATION, format!("Bearer {}", "a".repeat(64)).parse().unwrap());
    assert_eq!(bearer(&h).as_deref(), Some(""), "a session token is not ours");

    let uri = |s: &str| s.parse::<axum::http::Uri>().unwrap();
    assert!(!token_in_query(&uri("/api/v1/whoami")));
    assert!(!token_in_query(&uri("/api/v1/whoami?x=1")));
    for q in ["access_token=x", "token=x", "a=1&ACCESS_TOKEN=x", "access%5Ftoken=x", "token"] {
        assert!(token_in_query(&uri(&format!("/api/v1/whoami?{q}"))), "{q}");
    }
}

// ---------- #170: the security review ----------

/// The hourly housekeeping job (not only `purge_expired` on its own)
/// removes a grant whose expiry has passed, and its token stops working.
#[test]
fn the_hourly_job_purges_an_expired_grant() {
    let state = crate::routes::test_state(false);
    let db = &state.db;
    let now = crate::db::now();
    let (ann, token) = db
        .with(|c| {
            let ann = setup(c);
            let token = make_personal(c, ann, &new_grant(&Scope::ALL, &[], Lifetime::Days30), now)?.unwrap();
            Ok((ann, token))
        })
        .unwrap();
    assert!(db.with(|c| user_for_token(c, &token, now)).map(found).unwrap().is_some());
    db.with(|c| c.execute("UPDATE api_grants SET expires_at = ?1", [now - 1])).unwrap();
    let keep_all = crate::jobs::retention::Retention { history_days: None, keep_notes: false, failed_course_days: None };
    let runner = crate::jobs::housekeeping(crate::jobs::Runner::new(), &state, keep_all);
    runner.seed(db, now).unwrap();
    let (_, outcome) = runner.run_one(db, now).unwrap().unwrap();
    assert!(matches!(outcome, crate::jobs::Outcome::Done), "{outcome:?}");
    let rows: (i64, i64) = db.with(|c| c.query_row("SELECT (SELECT COUNT(*) FROM api_grants), (SELECT COUNT(*) FROM api_tokens)", [], |r| Ok((r.get(0)?, r.get(1)?)))).unwrap();
    assert_eq!(rows, (0, 0), "the expired grant and its token are gone");
    assert!(db.with(|c| grants(c, ann, now)).unwrap().is_empty());
    assert!(db.with(|c| user_for_token(c, &token, now)).map(found).unwrap().is_none());
}

/// A wrong token, an expired one, a revoked one and a refresh token used as
/// a bearer all take the same path: one indexed lookup that finds no row,
/// so neither the answer nor the work done says which it was.
#[test]
fn unknown_expired_and_revoked_tokens_are_one_answer() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let ann = setup(c);
        let unknown = format!("{TOKEN_PREFIX}{}", "0".repeat(64));
        let expired = personal(c, ann);
        c.execute("UPDATE api_tokens SET expires_at = ?1 WHERE token_hash = ?2", params![NOW - 1, hash_token(&expired)])?;
        let revoked = personal(c, ann);
        let gid: i64 = c.query_row("SELECT grant_id FROM api_tokens WHERE token_hash = ?1", [hash_token(&revoked)], |r| r.get(0))?;
        revoke(c, ann, gid)?;
        let g = create_grant(c, ann, &new_grant(&Scope::ALL, &[], Lifetime::Days30), NOW)?.unwrap();
        let refresh = mint(c, g, Kind::Refresh, NOW + 3600)?.unwrap();
        for t in [&unknown, &expired, &revoked, &refresh] {
            assert!(matches!(user_for_token(c, t, NOW)?, Lookup::Refused), "{t}");
            // The same query plan for each: a search of the unique hash index.
            let plan: Vec<String> = c
                .prepare(&format!("EXPLAIN QUERY PLAN {TOKEN_SQL}"))?
                .query_map(params![hash_token(t), NOW], |r| r.get::<_, String>(3))?
                .collect::<Result<_, _>>()?;
            assert!(plan.first().is_some_and(|p| p.contains("USING INDEX") && p.contains("token_hash")), "{plan:?}");
        }
        Ok(())
    })
    .unwrap();
}

/// A name a person reads to decide whom to trust may not hide characters:
/// no direction overrides or isolates, no zero-width or invisible ones, no
/// control characters. Letters of any script and emoji are fine.
#[test]
fn names_cannot_hide_characters() {
    for hidden in ['\u{202E}', '\u{202D}', '\u{2066}', '\u{2069}', '\u{200B}', '\u{200F}', '\u{FEFF}', '\u{061C}', '\u{00AD}', '\u{E0041}', '\u{1B}', '\u{7F}'] {
        assert!(is_hidden_char(hidden), "{:04X}", hidden as u32);
        assert!(!crate::oauth_server::valid_name(&format!("Claude{hidden}x")), "{:04X} in an app's name", hidden as u32);
        assert!(crate::routes::profile::apps::validate_name(&format!("Laptop{hidden}")).is_err(), "{:04X} in a token's name", hidden as u32);
    }
    for fine in ["Claude", "Mi asistente", "Ассистент", "助手", "مساعد", "Laptop 🚀"] {
        assert!(fine.chars().all(|c| !is_hidden_char(c)), "{fine}");
        assert!(crate::oauth_server::valid_name(fine) && crate::routes::profile::apps::validate_name(fine).is_ok(), "{fine}");
    }
}

/// The owner's "tokens issued in the last day": by the hour, the oldest
/// hour dropping out after 24, a slot reused for its new hour.
#[test]
fn issued_tokens_are_counted_for_a_day() {
    let issued = Issued::default();
    let t = 1_800_000_000; // a whole hour
    assert_eq!(issued.last_day(t), 0);
    issued.count(t);
    issued.count(t + 10);
    issued.count(t + 3600 * 5);
    assert_eq!(issued.last_day(t + 3600 * 5), 3);
    assert_eq!(issued.last_day(t + 3600 * 23 + 3599), 3, "still inside the day");
    assert_eq!(issued.last_day(t + 3600 * 24), 1, "the first hour dropped out");
    issued.count(t + 3600 * 24);
    assert_eq!(issued.last_day(t + 3600 * 24), 2, "its slot counts the new hour from zero");
    assert_eq!(issued.last_day(t + 3600 * 60), 0);
}
