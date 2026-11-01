use super::*;
use crate::ai::{Call, ChatMessage, Provider};
use crate::db::Db;
use serde_json::json;

fn vault() -> Vault {
    Vault::load(Some("cd".repeat(32)), std::path::Path::new("unused.db")).unwrap().0
}

/// Plus $0.100 a month, cap $1, a made-up model at $1 / $2 per MTok.
fn meter(cap: &str) -> Meter {
    meter_with(cap, true)
}

/// `meter`, with plans on or off (Skrongle mini has none).
fn meter_with(cap: &str, plans: bool) -> Meter {
    let mut cfg = MeterConfig {
        plans,
        tutor_model: "tutor-model".into(),
        courses_model: "course-model".into(),
        allowance_plus: dollars_to_micros("0.100").unwrap(),
        daily_cap: dollars_to_micros(cap).unwrap(),
        rate_per_minute: 100,
        packs: parse_packs("price_3:2.00:$3").unwrap(),
        ..MeterConfig::default()
    };
    cfg.prices.merge("tutor-model=1/2/0.1,course-model=1/2").unwrap();
    // The server's key is the dev simulator's platform key, at the test simulator.
    cfg.base_url = crate::routes::simulator_base(ProviderKind::Anthropic);
    Meter::new(cfg, Some(crate::dev_ai::PLATFORM_KEYS[0].into()), &vault(), true)
}

fn user(db: &Db, name: &str) -> i64 {
    db.with(|c| crate::auth::create_user(c, &format!("{name}@example.org"), "h")).unwrap()
}

/// One call through the test simulator, metered as the app meters it
/// (`Meter::call`, the gate every call goes through).
fn sim_call(m: &Meter, db: &Db, uid: i64, provider: &Provider, call: &Call) -> Result<i64, Refusal> {
    let msgs = [ChatMessage { role: "user", content: "hola".into() }];
    let est = m.estimate(&provider.model, 1000, 700);
    let before = db.with(|c| m.status(c, uid)).unwrap();
    match m.call(db, uid, provider, call, est, || ai::chat(provider, call, "system", &msgs, 700)) {
        Ok(_) => {}
        Err(CallError::Refused(r)) => return Err(r),
        Err(CallError::Failed(e)) => panic!("simulator call failed: {e}"),
    }
    let after = db.with(|c| m.status(c, uid)).unwrap();
    Ok(after.used + after.credits_spent - before.used - before.credits_spent)
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
    assert_eq!(parse_packs("price_a:2.5:$3 pack, price_b:8").unwrap()[1], CreditPack { price_id: "price_b".into(), label: "$8.00".into(), credit_micros: 8_000_000 });
    assert!(parse_packs("price_a").is_err());
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
            "SELECT cost_micros, credit_micros, hosted_calls, own_calls FROM ai_usage_months WHERE user_id = 1 AND month = 0",
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
fn under_allowance_then_over_it() {
    let (mut m, db) = (meter("1"), Db::open_in_memory().unwrap());
    m.cfg.allowance_plus = 10_000;
    let uid = user(&db, "ana");
    db.with(|c| crate::billing::set_grant(c, uid, None, "plus")).unwrap();
    let p = m.provider("tutor-turn").unwrap();
    let call = Call::new("tutor-turn").course("spanish");
    // Under the allowance: the call goes ahead and is charged to it.
    let cost = sim_call(&m, &db, uid, &p, &call).unwrap();
    assert!(cost > 0);
    let s = db.with(|c| m.status(c, uid)).unwrap();
    assert_eq!((s.used, s.hosted_calls, s.allowance, s.credits_spent), (cost, 1, 10_000, 0));
    let row: (String, String, String, i64) = db
        .with(|c| c.query_row("SELECT purpose, model, key_source, input_tokens FROM ai_usage", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))))
        .unwrap();
    assert_eq!((row.0.as_str(), row.1.as_str(), row.2.as_str()), ("tutor-turn", "tutor-model", "hosted"));
    assert!(row.3 > 0);
    // Spend the allowance down: the next check refuses before calling.
    while db.with(|c| m.status(c, uid)).unwrap().available >= m.estimate("tutor-model", 1000, 700) {
        sim_call(&m, &db, uid, &p, &call).unwrap();
    }
    let refused = sim_call(&m, &db, uid, &p, &call).unwrap_err();
    assert!(matches!(refused, Refusal::OutOfAllowance { .. }), "{refused:?}");
    assert!(refused.message(true).contains("own API key") && refused.message(true).contains("buy AI credit"));
}

/// A paying Stripe subscription to `plan` (the webhook stores the plan from its price).
fn subscribe(db: &Db, uid: i64, plan: crate::billing::Plan) {
    db.with(|c| {
        c.execute("DELETE FROM subscriptions WHERE user_id = ?1", [uid])?;
        c.execute(
            "INSERT INTO subscriptions (user_id, customer_id, subscription_id, status, plan, event_at, updated_at) VALUES (?1, 'cus', 'sub', 'active', ?2, 1, 1)",
            rusqlite::params![uid, plan.as_str()],
        )
    })
    .unwrap();
}

/// #931: only Plus has a monthly allowance. Free and Basic have no
/// Skrongle AI until they buy credits, which then pay for it on any
/// plan; on Plus the allowance goes first and the credits after it.
#[test]
fn only_plus_has_an_allowance_credits_work_on_any_plan() {
    use crate::billing::Plan;
    let (m, db) = (meter("1"), Db::open_in_memory().unwrap());
    let uid = user(&db, "ana");
    let p = m.provider("tutor-turn").unwrap();
    let call = Call::new("tutor-turn");
    assert_eq!((m.allowance(Plan::Free), m.allowance(Plan::Basic)), (0, 0));
    for plan in [Plan::Free, Plan::Basic] {
        if plan == Plan::Basic {
            subscribe(&db, uid, Plan::Basic);
        }
        let s = db.with(|c| m.status(c, uid)).unwrap();
        assert_eq!((s.plan, s.allowance, s.credits, s.available), (plan, 0, 0, 0));
        assert!(!s.offered() && !s.covers(m.cheapest(Need::Tutor)));
        let refused = sim_call(&m, &db, uid, &p, &call).unwrap_err();
        assert!(matches!(refused, Refusal::OutOfAllowance { available: 0, .. }), "{plan:?}: {refused:?}");
        assert!(db.with(|c| m.reserve(c, uid, "j", 1)).unwrap().is_err());
    }
    // Basic with credits: they are its Skrongle AI.
    db.with(|c| add_credits(c, uid, 1_000_000)).unwrap();
    let s = db.with(|c| m.status(c, uid)).unwrap();
    assert_eq!((s.plan, s.allowance, s.credits, s.available), (Plan::Basic, 0, 1_000_000, 1_000_000));
    assert!(s.offered() && s.covers(m.cheapest(Need::Tutor)));
    let cost = sim_call(&m, &db, uid, &p, &call).unwrap();
    let s = db.with(|c| m.status(c, uid)).unwrap();
    assert_eq!((s.credits_spent, s.credits), (cost, 1_000_000 - cost), "paid from credits");
    // On Plus the allowance is added and spent first.
    subscribe(&db, uid, Plan::Plus);
    let s = db.with(|c| m.status(c, uid)).unwrap();
    assert_eq!((s.plan, s.allowance, s.credits), (Plan::Plus, 100_000, 1_000_000 - cost));
    sim_call(&m, &db, uid, &p, &call).unwrap();
    let s = db.with(|c| m.status(c, uid)).unwrap();
    assert_eq!(s.credits, 1_000_000 - cost, "the allowance paid");
}

/// #931: `SKRONGLE_AI_ALLOWANCE_FREE` is a one-time credit, not a monthly
/// allowance. A new account gets it once its address is confirmed, once
/// only (a later confirmation, or a new month, gives nothing more); an
/// account made before v87, an unconfirmed one, and any with the setting at
/// 0 or plans off get none.
#[test]
fn a_new_account_gets_the_sign_up_credit_once_confirmed() {
    let db = Db::open_in_memory().unwrap();
    let with = |credit: &str, plans: bool| {
        let mut m = meter_with("1", plans);
        m.cfg.signup_credit = dollars_to_micros(credit).unwrap();
        m
    };
    let m = with("0.25", true);
    assert_eq!(m.allowance(crate::billing::Plan::Free), 0, "not monthly");
    let ana = user(&db, "ana");
    let credits = |uid| db.with(|c| m.status(c, uid)).unwrap().credits;
    assert_eq!(credits(ana), 0, "not before the address is confirmed");
    db.with(|c| crate::verify::mark_verified(c, ana, 1)).unwrap();
    assert_eq!(credits(ana), 250_000);
    assert_eq!(credits(ana), 250_000, "once");
    db.with(|c| c.execute("UPDATE users SET email_verified_at = NULL WHERE id = ?1", [ana])).unwrap();
    db.with(|c| crate::verify::mark_verified(c, ana, 2)).unwrap();
    assert_eq!(credits(ana), 250_000, "confirming again gives nothing");
    // An account from before v87 is marked done by the migration.
    let old = user(&db, "old");
    db.with(|c| c.execute("UPDATE users SET signup_credit_at = 0, email_verified_at = 1 WHERE id = ?1", [old])).unwrap();
    assert_eq!(credits(old), 0);
    // Nothing given, and nothing recorded, at 0 or with plans off.
    for m in [with("0", true), with("0.25", false)] {
        let uid = user(&db, &format!("none-{}", m.cfg.plans));
        db.with(|c| crate::verify::mark_verified(c, uid, 1)).unwrap();
        assert!(!db.with(|c| m.grant_signup_credit(c, uid)).unwrap());
        assert!(db.with(|c| c.query_row("SELECT signup_credit_at FROM users WHERE id = ?1", [uid], |r| r.get::<_, Option<i64>>(0))).unwrap().is_none());
    }
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
    let s = db.with(|c| m.status(c, uid)).unwrap();
    assert_eq!((s.own_calls, s.hosted_calls, s.used), (5, 0, 0));
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
fn credits_are_spent_after_the_allowance() {
    let (mut m, db) = (meter("1"), Db::open_in_memory().unwrap());
    m.cfg.allowance_plus = 10_000;
    let uid = user(&db, "ana");
    db.with(|c| crate::billing::set_grant(c, uid, None, "plus")).unwrap();
    db.with(|c| add_credits(c, uid, 5_000)).unwrap();
    let call = Call::new("tutor-turn");
    let rec = |input: u64| Record { user_id: uid, call: &call, provider: "anthropic", model: "tutor-model", usage: Usage { input, output: 0, cached: 0 }, source: KeySource::Hosted };
    // $0.008 of a $0.010 allowance: all from the allowance.
    db.with(|c| m.record(c, &rec(8_000))).unwrap();
    let s = db.with(|c| m.status(c, uid)).unwrap();
    assert_eq!((s.used, s.credits, s.available), (8_000, 5_000, 7_000));
    // $0.004 more: $0.002 from the allowance, $0.002 from credits.
    db.with(|c| m.record(c, &rec(4_000))).unwrap();
    let s = db.with(|c| m.status(c, uid)).unwrap();
    assert_eq!((s.used, s.credits_spent, s.credits, s.allowance_left), (10_000, 2_000, 3_000, 0));
    // Over both: credits stop at zero.
    db.with(|c| m.record(c, &rec(9_000))).unwrap();
    let s = db.with(|c| m.status(c, uid)).unwrap();
    assert_eq!((s.credits, s.available, s.credits_spent), (0, 0, 5_000));
}

#[test]
fn jobs_reserve_settle_and_release() {
    let (m, db) = (meter("1"), Db::open_in_memory().unwrap());
    let uid = user(&db, "ana");
    db.with(|c| crate::billing::set_grant(c, uid, None, "basic")).unwrap(); // $0.100
    // More than the allowance: refused up front.
    assert!(matches!(db.with(|c| m.reserve(c, uid, "big", 150_000)).unwrap(), Err(Refusal::OutOfAllowance { available: 100_000, needed: 150_000 })));
    db.with(|c| m.reserve(c, uid, "j1", 60_000)).unwrap().unwrap();
    let s = db.with(|c| m.status(c, uid)).unwrap();
    assert_eq!((s.reserved, s.available), (60_000, 40_000));
    // A tutor turn meanwhile only sees what the job does not hold.
    assert!(db.with(|c| m.check_call(c, uid, None, 50_000)).unwrap().is_err());
    // Each lesson settles against the reservation.
    let p = m.provider("generate-lesson").unwrap();
    let call = Call::new("generate-lesson").job("j1").key("intro");
    let cost = sim_call(&m, &db, uid, &p, &call).unwrap();
    assert_eq!(m.reservation("j1"), Some(60_000 - cost));
    let s = db.with(|c| m.status(c, uid)).unwrap();
    assert_eq!((s.used, s.reserved), (cost, 60_000 - cost));
    // Stopping gives the rest back.
    assert_eq!(m.release("j1"), 60_000 - cost);
    let s = db.with(|c| m.status(c, uid)).unwrap();
    assert_eq!((s.reserved, s.available), (0, 100_000 - cost));
    // A job whose estimate ran out keeps going while the learner has
    // allowance, and stops (before calling) when nothing is left.
    db.with(|c| m.reserve(c, uid, "j2", 1)).unwrap().unwrap();
    assert!(db.with(|c| m.check_call(c, uid, Some("j2"), 0)).unwrap().is_ok());
    let spend_all = Record { user_id: uid, call: &Call::new("generate-lesson").job("j2"), provider: "anthropic", model: "course-model", usage: Usage { input: 200_000, output: 0, cached: 0 }, source: KeySource::Hosted };
    db.with(|c| m.record(c, &spend_all)).unwrap();
    let refused = db.with(|c| m.check_call(c, uid, Some("j2"), 0)).unwrap().unwrap_err();
    assert!(refused.job_message(false).contains("Nothing written so far is lost"));
}

#[test]
fn the_daily_cap_stops_everyone() {
    let (m, db) = (meter("0.050"), Db::open_in_memory().unwrap());
    let (ana, bob) = (user(&db, "ana"), user(&db, "bob"));
    for u in [ana, bob] {
        db.with(|c| crate::billing::set_grant(c, u, None, "basic")).unwrap();
    }
    let call = Call::new("tutor-turn");
    let rec = Record { user_id: ana, call: &call, provider: "anthropic", model: "tutor-model", usage: Usage { input: 45_000, output: 0, cached: 0 }, source: KeySource::Hosted };
    db.with(|c| m.record(c, &rec)).unwrap();
    // Bob has his whole allowance, but the server's day is nearly spent.
    assert_eq!(db.with(|c| m.check_call(c, bob, None, 10_000)).unwrap(), Err(Refusal::DailyCap));
    assert!(db.with(|c| m.check_call(c, bob, None, 4_000)).unwrap().is_ok());
    // A reservation counts toward the day too.
    assert_eq!(db.with(|c| m.reserve(c, bob, "j", 6_000)).unwrap(), Err(Refusal::DailyCap));
    db.with(|c| m.record(c, &Record { user_id: bob, ..rec.clone() })).unwrap();
    assert_eq!(db.with(|c| m.check_call(c, ana, None, 1)).unwrap(), Err(Refusal::DailyCap));
    assert_eq!(m.alerted_day.load(Ordering::SeqCst), day_start(now()));
    assert!(Refusal::DailyCap.message(false).contains("own API key still works"));
}

/// Skrongle mini (#636): a Free account is offered the server's
/// AI with no monthly allowance; the day's cap still stops everyone, and
/// every call still gets its `ai_usage` row.
#[test]
fn with_plans_off_only_the_daily_cap_applies() {
    let (m, db) = (meter_with("0.500", false), Db::open_in_memory().unwrap());
    let ana = user(&db, "ana");
    assert!(!m.credits_offered(), "no packs are sold without plans");
    let s = db.with(|c| m.status(c, ana)).unwrap();
    assert_eq!((s.plan, s.allowance, s.unmetered), (crate::billing::Plan::Plus, UNMETERED, true));
    assert!(s.covers(m.cheapest(Need::Course)) && s.percent_left() == 100);

    // $0.30 of tutor turns: three times Plus's $0.10 allowance, still allowed.
    let call = Call::new("tutor-turn");
    let rec = Record { user_id: ana, call: &call, provider: "anthropic", model: "tutor-model", usage: Usage { input: 300_000, output: 0, cached: 0 }, source: KeySource::Hosted };
    assert_eq!(db.with(|c| m.record(c, &rec)).unwrap(), 300_000);
    assert!(db.with(|c| m.check_call(c, ana, None, 100_000)).unwrap().is_ok());
    assert!(db.with(|c| m.reserve(c, ana, "j", 100_000)).unwrap().is_ok());
    m.release("j");
    let (rows, credit): (i64, i64) =
        db.with(|c| c.query_row("SELECT COUNT(*), SUM(credit_micros) FROM ai_usage WHERE user_id = ?1", [ana], |r| Ok((r.get(0)?, r.get(1)?)))).unwrap();
    assert_eq!((rows, credit), (1, 0));

    // The day's cap: $0.30 spent of $0.50, so a $0.25 call is refused.
    assert_eq!(db.with(|c| m.check_call(c, ana, None, 250_000)).unwrap(), Err(Refusal::DailyCap));
    assert_eq!(db.with(|c| m.reserve(c, ana, "j2", 250_000)).unwrap(), Err(Refusal::DailyCap));
}

#[test]
fn credit_purchases_and_refunds_from_webhooks() {
    let db = Db::open_in_memory().unwrap();
    let uid = user(&db, "ana");
    let billing_ref = db.with(|c| crate::billing::account(c, uid)).unwrap().unwrap().billing_ref;
    let paid = json!({ "type": "checkout.session.completed", "data": { "object": {
        "id": "cs_1", "mode": "payment", "payment_status": "paid", "payment_intent": "pi_1", "amount_total": 300,
        "metadata": { "kind": "ai-credits", "billing_ref": billing_ref, "credit_micros": "2000000", "pack": "price_3" } } } });
    db.with(|c| {
        // The row id is not how an account is named (#364): nothing is added.
        let by_id = json!({ "type": "checkout.session.completed", "data": { "object": {
            "id": "cs_0", "mode": "payment", "payment_status": "paid", "payment_intent": "pi_0", "amount_total": 300,
            "metadata": { "kind": "ai-credits", "user_id": uid.to_string(), "credit_micros": "2000000", "pack": "price_3" } } } });
        assert_eq!(apply_credit_event(c, &by_id)?, None);
        assert_eq!(apply_credit_event(c, &paid)?, Some(CreditChange { user_id: uid, micros: 2_000_000 }));
        // Stripe retries: applied once.
        assert_eq!(apply_credit_event(c, &paid)?, None);
        assert_eq!(credit_balance(c, uid)?, 2_000_000);
        // A subscription checkout is not a credits event.
        let sub = json!({ "type": "checkout.session.completed", "data": { "object": { "id": "cs_2", "mode": "subscription", "metadata": {} } } });
        assert_eq!(apply_credit_event(c, &sub)?, None);
        // Half refunded, then all of it.
        let refund = |back: i64| json!({ "type": "charge.refunded", "data": { "object": { "payment_intent": "pi_1", "amount": 300, "amount_refunded": back } } });
        assert_eq!(apply_credit_event(c, &refund(150))?, Some(CreditChange { user_id: uid, micros: -1_000_000 }));
        assert_eq!(apply_credit_event(c, &refund(150))?, None);
        assert_eq!(apply_credit_event(c, &refund(300))?.unwrap().micros, -1_000_000);
        assert_eq!(credit_balance(c, uid)?, 0);
        Ok(())
    })
    .unwrap();
}

/// #283: Skrongle AI is offered for a use only when the month's AI (the
/// plan's allowance, or credits) could pay for its smallest request.
#[test]
fn offered_only_for_what_the_month_can_pay_for() {
    use crate::billing::Plan;
    let m = meter("1");
    let (tutor, lesson, course) = (m.cheapest(Need::Tutor), m.cheapest(Need::Lesson), m.cheapest(Need::Course));
    assert!(0 < tutor && tutor < lesson && lesson < course, "{tutor} {lesson} {course}");
    let status = |plan, allowance, credits| AiStatus { hosted: true, plan, allowance, credits, available: allowance + credits, ..AiStatus::default() };
    // None included and no credits: not offered for anything.
    let free = status(Plan::Free, 0, 0);
    assert!(!free.offered() && !free.covers(tutor) && !free.covers(course));
    // Enough for a tutor reply, too little for a lesson or a course.
    let small = status(Plan::Plus, tutor, 0);
    assert!(small.covers(tutor) && !small.covers(lesson) && !small.covers(course));
    // Credits count like the allowance (`Meter::status` counts them on every plan, #931).
    assert!(status(Plan::Free, 0, course).covers(course));
    // Used up this month is still offered: the refusal shows the ways on.
    let spent = AiStatus { used: course, available: 0, ..status(Plan::Plus, course, 0) };
    assert!(spent.covers(course));
    // Without the server's key, nothing.
    assert!(!AiStatus { hosted: false, ..status(Plan::Plus, course, 0) }.covers(tutor));
    // The defaults: Free and Basic include none; Plus's allowance covers every use.
    let d = Meter::new(MeterConfig::default(), Some("sk-ant-x".into()), &vault(), false);
    assert_eq!((d.allowance(Plan::Free), d.allowance(Plan::Basic)), (0, 0));
    let plus = status(Plan::Plus, d.allowance(Plan::Plus), 0);
    assert!([Need::Tutor, Need::Lesson, Need::Course].iter().all(|n| plus.covers(d.cheapest(*n))));
}
