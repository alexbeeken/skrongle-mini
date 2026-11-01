use super::*;
use crate::ai::{Call, ChatMessage, Provider};
use crate::db::Db;

fn vault() -> Vault {
    Vault::load(Some("cd".repeat(32)), std::path::Path::new("unused.db")).unwrap().0
}

/// Cap as given, a made-up model at $1 / $2 per MTok.
fn meter(cap: &str) -> Meter {
    let mut cfg = MeterConfig {
        tutor_model: "tutor-model".into(),
        courses_model: "course-model".into(),
        daily_cap: dollars_to_micros(cap).unwrap(),
        rate_per_minute: 100,
        ..MeterConfig::default()
    };
    cfg.prices.merge("tutor-model=1/2/0.1,course-model=1/2").unwrap();
    // The server's key is the dev simulator's platform key, at the test simulator.
    cfg.base_url = crate::routes::simulator_base(ProviderKind::Anthropic);
    Meter::new(cfg, Some(crate::dev_ai::PLATFORM_KEYS[0].into()), &vault())
}

/// Midnight UTC on the first of the month `t` falls in (the month the
/// `ai_usage_months` totals are keyed by).
fn month_start(t: i64) -> i64 {
    // Howard Hinnant's civil-from-days, then days-from-civil for day 1.
    let z = t.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let first_doy = (153 * mp + 2) / 5;
    let first_doe = yoe * 365 + yoe / 4 - yoe / 100 + first_doy;
    (era * 146_097 + first_doe - 719_468) * 86_400
}

/// What `uid`'s calls cost so far, hosted ones.
fn spent(db: &Db, uid: i64) -> i64 {
    db.with(|c| c.query_row("SELECT COALESCE(SUM(cost_micros), 0) FROM ai_usage WHERE user_id = ?1", [uid], |r| r.get(0))).unwrap()
}

/// (hosted calls, own calls) recorded for `uid`.
fn calls(db: &Db, uid: i64) -> (i64, i64) {
    db.with(|c| {
        c.query_row(
            "SELECT COUNT(CASE WHEN key_source = 'hosted' THEN 1 END), COUNT(CASE WHEN key_source = 'own' THEN 1 END) FROM ai_usage WHERE user_id = ?1",
            [uid],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    })
    .unwrap()
}

fn user(db: &Db, name: &str) -> i64 {
    db.with(|c| crate::auth::create_user(c, &format!("{name}@example.org"), "h")).unwrap()
}

/// One call through the test simulator, metered as the app meters it
/// (`Meter::call`, the gate every call goes through).
fn sim_call(m: &Meter, db: &Db, uid: i64, provider: &Provider, call: &Call) -> Result<i64, Refusal> {
    let msgs = [ChatMessage { role: "user", content: "hola".into() }];
    let est = m.estimate(&provider.model, 1000, 700);
    let before = spent(db, uid);
    match m.call(db, uid, provider, call, est, || ai::chat(provider, call, "system", &msgs, 700)) {
        Ok(_) => {}
        Err(CallError::Refused(r)) => return Err(r),
        Err(CallError::Failed(e)) => panic!("simulator call failed: {e}"),
    }
    Ok(spent(db, uid) - before)
}

/// The learner's own key, at the test simulator.
fn own_key() -> Provider {
    crate::routes::simulated_provider(ProviderKind::Anthropic)
}

#[test]
fn prices_costs_and_money() {
    let t = PriceTable::parse(DEFAULT_PRICES).unwrap();
    assert_eq!(t.price("claude-sonnet-5-5"), Price { input: 2_000_000, output: 10_000_000, cached: 100_000 });
    // Unknown models cost the dearest rate, never nothing.
    assert_eq!(t.price("something-new").input, 10_000_000);
    // 1000 in + 500 out on Sonnet: $0.002 + $0.005.
    assert_eq!(t.cost("claude-sonnet-5-5", Usage { input: 1000, output: 500, cached: 0 }), 7000);
    assert_eq!(t.cost("claude-haiku-5-5", Usage { input: 1, output: 0, cached: 0 }), 1, "rounded up");
    assert!(PriceTable::parse("bad").is_err() && PriceTable::parse("m=1").is_err());
    assert_eq!(dollars(48_000), "$0.048");
    assert_eq!(dollars(12_300_000), "$12.30");
    assert_eq!((dollars(50_000), dollars(49_600), dollars(400), dollars(0)), ("$0.05".into(), "$0.0496".into(), "$0.0004".into(), "$0.00".into()));
}

/// What the totals replaced: the month's and the day's sums over `ai_usage`.
fn sums(c: &Connection, uid: i64, month: i64, day: i64) -> ((i64, i64, i64, i64), i64) {
    let m = c
        .query_row(
            "SELECT COALESCE(SUM(CASE WHEN key_source = 'hosted' THEN cost_micros END), 0),
                        COALESCE(SUM(CASE WHEN key_source = 'hosted' THEN credit_micros END), 0),
                        COUNT(CASE WHEN key_source = 'hosted' THEN 1 END),
                        COUNT(CASE WHEN key_source = 'own' THEN 1 END)
                 FROM ai_usage WHERE user_id = ?1 AND created_at >= ?2 AND created_at < ?3",
            params![uid, month, month_start(month + 32 * 86_400)],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    let d = c
        .query_row("SELECT COALESCE(SUM(cost_micros), 0) FROM ai_usage WHERE key_source = 'hosted' AND created_at >= ?1 AND created_at < ?2", [day, day + 86_400], |r| r.get(0))
        .unwrap();
    (m, d)
}

fn totals(c: &Connection, uid: i64, month: i64, day: i64) -> ((i64, i64, i64, i64), i64) {
    let m = c
        .query_row("SELECT cost_micros, credit_micros, hosted_calls, own_calls FROM ai_usage_months WHERE user_id = ?1 AND month = ?2", params![uid, month], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .optional()
        .unwrap()
        .unwrap_or((0, 0, 0, 0));
    let d = c.query_row("SELECT cost_micros FROM ai_spend_days WHERE day = ?1", [day], |r| r.get(0)).optional().unwrap().unwrap_or(0);
    (m, d)
}

/// #623, #624: the running totals equal the sums they replaced, on each
/// side of a month and a day boundary (UTC), for each learner, after
/// rows are added and after they are deleted (one by one, and with the
/// account).
#[test]
fn running_totals_equal_the_sums() {
    let db = Db::open_in_memory().unwrap();
    let (ana, bo) = (user(&db, "ana"), user(&db, "bo"));
    // 2026-10-31 23:59:59 UTC, then 2026-11-01 00:00:00 and 12:00.
    let (oct, nov) = (1_790_812_800, 1_793_491_200);
    let ats = [nov - 1, nov, nov + 43_200];
    db.with(|c| {
        for (i, &at) in ats.iter().enumerate() {
            for (uid, source) in [(ana, "hosted"), (ana, "own"), (bo, "hosted")] {
                let cost = if source == "hosted" { 100 * (i as i64 + 1) + uid } else { 0 };
                c.execute(
                    "INSERT INTO ai_usage (user_id, purpose, provider, model, cost_micros, credit_micros, key_source, created_at) VALUES (?1, 'tutor-turn', 'anthropic', 'm', ?2, ?3, ?4, ?5)",
                    params![uid, cost, cost / 3, source, at],
                )?;
            }
        }
        let check = |c: &Connection| {
            for uid in [ana, bo] {
                for (month, day) in [(oct, day_start(nov - 1)), (nov, nov)] {
                    assert_eq!(totals(c, uid, month, day), sums(c, uid, month, day), "user {uid}, month {month}");
                }
            }
        };
        check(c);
        assert_eq!(totals(c, ana, nov, nov).0 .2, 2, "two hosted calls in November");
        assert_eq!(totals(c, bo, oct, day_start(nov - 1)).1, (100 + ana) + (100 + bo), "the day is everyone's");
        c.execute("DELETE FROM ai_usage WHERE user_id = ?1 AND created_at = ?2", params![ana, nov])?;
        check(c);
        assert!(crate::privacy::delete_account(c, bo)?.is_some());
        check(c);
        assert_eq!(totals(c, ana, nov, nov).1, 300 + ana, "bo's spend left the day with his account");
        Ok(())
    })
    .unwrap();
}

/// The meter reads one row of each total: no scan of a table.
#[test]
fn the_meter_reads_its_totals_without_a_scan() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        for sql in [
            "SELECT cost_micros FROM ai_spend_days WHERE day = 0",
        ] {
            let plan: Vec<String> = c.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?.query_map([], |r| r.get::<_, String>(3))?.collect::<Result<_, _>>()?;
            assert!(!plan.is_empty() && plan.iter().all(|p| p.starts_with("SEARCH") && !p.contains("SCAN")), "{sql}: {plan:?}");
        }
        Ok(())
    })
    .unwrap();
}

#[test]
fn calendar() {
    // 2026-10-17 12:34:56 UTC
    let t = 1_792_240_496;
    assert_eq!(day_start(t), 1_792_195_200);
    assert_eq!(month_start(t), 1_790_812_800); // 2026-10-01
    assert_eq!(month_start(1_790_812_800), 1_790_812_800);
    assert_eq!(month_start(951_782_400), 949_363_200); // 2000-02-29 -> 2000-02-01
}

#[test]
fn the_server_key_is_sealed_and_hosted_ai_is_off_without_it() {
    let m = meter("1");
    assert!(!m.hosted.as_ref().unwrap().sealed.windows(8).any(|w| w == b"platform"));
    let p = m.provider("tutor-turn").unwrap();
    assert_eq!((p.api_key.as_str(), p.model.as_str(), p.source), (crate::dev_ai::PLATFORM_KEYS[0], "tutor-model", KeySource::Hosted));
    assert_eq!(m.provider("generate-lesson").unwrap().model, "course-model");
    let off = Meter::off();
    assert!(!off.hosted() && off.provider("tutor-turn").is_none());
}

#[test]
fn own_keys_are_recorded_free_and_never_limited() {
    let mut m = meter("0.000001");
    m.cfg.rate_per_minute = 1;
    let db = Db::open_in_memory().unwrap();
    let uid = user(&db, "ana");
    let own = own_key();
    assert_eq!(own.source, KeySource::Own);
    let call = Call::new("tutor-turn");
    for _ in 0..5 {
        assert_eq!(sim_call(&m, &db, uid, &own, &call), Ok(0));
    }
    assert_eq!((calls(&db, uid), spent(&db, uid)), ((0, 5), 0));
    // The same account on the server's key hits every limit.
    assert!(m.rate_limit(uid).is_ok());
    assert_eq!(m.rate_limit(uid), Err(Refusal::RateLimited));
}

#[test]
fn rate_limit_window_slides() {
    let mut m = meter("1");
    m.cfg.rate_per_minute = 2;
    assert!(m.rate_limit_at(1, 100).is_ok() && m.rate_limit_at(1, 110).is_ok());
    assert_eq!(m.rate_limit_at(1, 120), Err(Refusal::RateLimited));
    assert!(m.rate_limit_at(2, 120).is_ok(), "per account");
    assert!(m.rate_limit_at(1, 161).is_ok(), "the first call left the window");
}

#[test]
fn jobs_reserve_settle_and_release() {
    let (m, db) = (meter("0.100"), Db::open_in_memory().unwrap());
    let uid = user(&db, "ana");
    // More than the day's cap: refused up front.
    assert_eq!(db.with(|c| m.reserve(c, "big", 150_000)).unwrap(), Err(Refusal::DailyCap));
    db.with(|c| m.reserve(c, "j1", 60_000)).unwrap().unwrap();
    // A tutor turn meanwhile sees what the job holds against the day.
    assert_eq!(db.with(|c| m.check_call(c, None, 50_000)).unwrap(), Err(Refusal::DailyCap));
    assert!(db.with(|c| m.check_call(c, None, 30_000)).unwrap().is_ok());
    // Each lesson settles against the reservation.
    let p = m.provider("generate-lesson").unwrap();
    let call = Call::new("generate-lesson").job("j1").key("intro");
    let cost = sim_call(&m, &db, uid, &p, &call).unwrap();
    assert_eq!(m.reservation("j1"), Some(60_000 - cost));
    // Stopping gives the rest back.
    assert_eq!(m.release("j1"), 60_000 - cost);
    assert_eq!(m.reservation("j1"), None);
    // A job whose estimate ran out keeps going while the day has room,
    // and stops (before calling) once the day's cap is spent.
    db.with(|c| m.reserve(c, "j2", 1)).unwrap().unwrap();
    assert!(db.with(|c| m.check_call(c, Some("j2"), 0)).unwrap().is_ok());
    let spend_all = Record { user_id: uid, call: &Call::new("generate-lesson").job("j2"), provider: "anthropic", model: "course-model", usage: Usage { input: 200_000, output: 0, cached: 0 }, source: KeySource::Hosted };
    db.with(|c| m.record(c, &spend_all)).unwrap();
    let refused = db.with(|c| m.check_call(c, Some("j2"), 0)).unwrap().unwrap_err();
    assert!(refused.job_message().contains("Nothing written so far is lost"));
}

/// The day's cap stops everyone, and every call still gets its `ai_usage`
/// row, however much one learner has used this month.
#[test]
fn the_daily_cap_stops_everyone() {
    let (m, db) = (meter("0.500"), Db::open_in_memory().unwrap());
    let (ana, bob) = (user(&db, "ana"), user(&db, "bob"));
    let call = Call::new("tutor-turn");
    let rec = Record { user_id: ana, call: &call, provider: "anthropic", model: "tutor-model", usage: Usage { input: 300_000, output: 0, cached: 0 }, source: KeySource::Hosted };
    assert_eq!(db.with(|c| m.record(c, &rec)).unwrap(), 300_000);
    assert_eq!(calls(&db, ana), (1, 0));
    // $0.30 of $0.50 spent: a $0.10 call and reservation are fine.
    assert!(db.with(|c| m.check_call(c, None, 100_000)).unwrap().is_ok());
    assert!(db.with(|c| m.reserve(c, "j", 100_000)).unwrap().is_ok());
    m.release("j");
    // A $0.25 call is refused, for Bob too, and a reservation counts toward the day.
    assert_eq!(db.with(|c| m.check_call(c, None, 250_000)).unwrap(), Err(Refusal::DailyCap));
    assert_eq!(db.with(|c| m.reserve(c, "j2", 250_000)).unwrap(), Err(Refusal::DailyCap));
    db.with(|c| m.record(c, &Record { user_id: bob, usage: Usage { input: 200_000, output: 0, cached: 0 }, ..rec.clone() })).unwrap();
    assert_eq!(db.with(|c| m.check_call(c, None, 1)).unwrap(), Err(Refusal::DailyCap));
    assert_eq!(m.alerted_day.load(Ordering::SeqCst), day_start(now()));
    assert!(Refusal::DailyCap.message().contains("own API key still works"));
}
