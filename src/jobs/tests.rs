use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

const T: i64 = 1_800_000_000;

fn db() -> Db {
    Db::open_in_memory().unwrap()
}

fn state(db: &Db, id: i64) -> (String, i64, i64, Option<String>) {
    db.with(|c| c.query_row("SELECT state, run_at, attempts, last_error FROM jobs WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))))
        .unwrap()
}

#[test]
fn claim_takes_due_rows_in_order_once() {
    let db = db();
    let kinds = ["dev.fail"];
    let later = db.with(|c| enqueue(c, "dev.fail", &serde_json::json!({}), T + 10, T)).unwrap().unwrap();
    let first = db.with(|c| enqueue(c, "dev.fail", &serde_json::json!({"n": 1}), T - 5, T)).unwrap().unwrap();
    let c1 = db.with(|c| claim(c, &kinds, "a", T)).unwrap().unwrap();
    assert_eq!((c1.id, c1.attempts, c1.payload.as_str()), (first, 1, r#"{"n":1}"#));
    // The only other row is not due yet, and the taken one is leased.
    assert_eq!(db.with(|c| claim(c, &kinds, "b", T)).unwrap(), None);
    assert_eq!(db.with(|c| claim(c, &kinds, "b", T + 10)).unwrap().unwrap().id, later);
    // A kind this server doesn't run is left alone.
    assert_eq!(db.with(|c| claim(c, &["housekeeping"], "a", T + 100)).unwrap(), None);
}

#[test]
fn an_unknown_kind_is_refused() {
    let db = db();
    assert_eq!(db.with(|c| enqueue(c, "nonsense", &serde_json::json!({}), T, T)).unwrap(), None);
    assert!(db.with(|c| recent(c, 10)).unwrap().is_empty());
}

#[test]
fn a_job_whose_lease_ran_out_is_taken_again() {
    let db = db();
    let id = db.with(|c| enqueue(c, "dev.fail", &serde_json::json!({}), T, T)).unwrap().unwrap();
    db.with(|c| claim(c, &["dev.fail"], "dead-server", T)).unwrap().unwrap();
    assert_eq!(db.with(|c| claim(c, &["dev.fail"], "b", T + LEASE - 1)).unwrap(), None);
    let again = db.with(|c| claim(c, &["dev.fail"], "b", T + LEASE)).unwrap().unwrap();
    assert_eq!((again.id, again.attempts), (id, 2));
    let by: String = db.with(|c| c.query_row("SELECT locked_by FROM jobs WHERE id = ?1", [id], |r| r.get(0))).unwrap();
    assert_eq!(by, "b");
}

#[test]
fn failures_follow_the_retry_delays_then_fail() {
    let db = db();
    let runner = dev(Runner::new());
    let id = db.with(|c| enqueue(c, "dev.fail", &serde_json::json!({}), T, T)).unwrap().unwrap();
    let mut now = T;
    for (i, delay) in RETRY_DELAYS.iter().enumerate() {
        let (job, outcome) = runner.run_one(&db, now).unwrap().unwrap();
        assert_eq!(job.attempts, i as i64 + 1);
        let at = match outcome {
            Outcome::Retry(at) => at,
            o => panic!("attempt {}: {o:?}", i + 1),
        };
        assert!(at - now >= *delay && at - now <= delay + 2, "attempt {}: retried after {}s, not {delay}s", i + 1, at - now);
        assert_eq!(runner.run_one(&db, at - 1).unwrap(), None, "not due before its delay");
        now = at;
    }
    let (_, outcome) = runner.run_one(&db, now).unwrap().unwrap();
    assert_eq!(outcome, Outcome::Failed);
    let (st, _, attempts, err) = state(&db, id);
    assert_eq!((st.as_str(), attempts), ("failed", RETRY_DELAYS.len() as i64 + 1));
    assert!(err.unwrap().contains("always fails"));
    assert_eq!(runner.run_one(&db, now + 100_000).unwrap(), None, "a failed job stays failed");
}

#[test]
fn a_periodic_job_queues_its_next_run() {
    let db = db();
    let runs = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&runs);
    let runner = Runner::new().handle(HOUSEKEEPING, move |_, _| {
        counter.fetch_add(1, Ordering::SeqCst);
        Ok(String::new())
    });
    assert_eq!(runner.seed(&db, T).unwrap(), 1);
    assert_eq!(runner.seed(&db, T).unwrap(), 0, "seeding twice adds nothing");
    let (job, outcome) = runner.run_one(&db, T).unwrap().unwrap();
    assert_eq!(outcome, Outcome::Done);
    assert_eq!(state(&db, job.id).0, "done");
    let rows = db.with(|c| recent(c, 10)).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!((rows[0].kind.as_str(), rows[0].state.as_str()), (HOUSEKEEPING.name, "queued"));
    assert!(rows[0].run_at >= T + HOUR);
    assert_eq!(runner.run_one(&db, T + HOUR - 1).unwrap(), None);
    assert!(runner.run_one(&db, rows[0].run_at).unwrap().is_some());
    assert_eq!(runs.load(Ordering::SeqCst), 2);
    // A boot while the next run is queued does not add another.
    assert_eq!(runner.seed(&db, T + HOUR).unwrap(), 0);
}

#[test]
fn a_periodic_job_that_fails_for_good_still_comes_back() {
    let db = db();
    let runner = Runner::new().handle(HOUSEKEEPING, |_, _| Err("disk on fire".into()));
    runner.seed(&db, T).unwrap();
    let mut now = T;
    loop {
        match runner.run_one(&db, now).unwrap().unwrap().1 {
            Outcome::Retry(at) => now = at,
            Outcome::Failed => break,
            Outcome::Done | Outcome::Again | Outcome::Handed => panic!("it always fails"),
        }
    }
    let rows = db.with(|c| recent(c, 10)).unwrap();
    assert_eq!((rows[0].state.as_str(), rows[1].state.as_str()), ("queued", "failed"));
    assert!(rows[0].run_at >= now + HOUR);
}

#[test]
fn a_fresh_boot_runs_due_housekeeping_at_once() {
    let db = db();
    let mut runner = Runner::new();
    for k in KINDS.iter().filter(|k| k.every.is_some()) {
        runner = runner.handle(*k, |_, _| Ok(String::new()));
    }
    let periodic = KINDS.iter().filter(|k| k.every.is_some()).count();
    assert_eq!(runner.seed(&db, T).unwrap(), periodic);
    let mut ran = 0;
    while runner.run_one(&db, T).unwrap().is_some() {
        ran += 1;
    }
    assert_eq!(ran, periodic, "every housekeeping job ran at boot, none waited an hour");
    let next = db.with(|c| next_due(c, &runner.kinds())).unwrap().unwrap();
    assert!(next >= T + HOUR);
}

#[test]
fn old_rows_are_purged() {
    let db = db();
    let runner = Runner::new().handle(HOUSEKEEPING, |_, _| Ok(String::new()));
    runner.seed(&db, T).unwrap();
    runner.run_one(&db, T).unwrap();
    assert_eq!(db.with(|c| purge(c, T + KEEP_DONE - 10)).unwrap(), 0);
    assert_eq!(db.with(|c| purge(c, T + KEEP_DONE + 10)).unwrap(), 1);
    assert_eq!(db.with(|c| recent(c, 10)).unwrap().len(), 1, "the queued next run stays");
}

#[test]
fn dev_run_now_makes_rows_due() {
    let db = db();
    let id = db.with(|c| enqueue(c, "dev.fail", &serde_json::json!({}), T + 5000, T)).unwrap().unwrap();
    assert_eq!(db.with(|c| dev_run_now(c, id, T)).unwrap(), 1);
    db.with(|c| claim(c, &["dev.fail"], "a", T)).unwrap().unwrap();
    assert_eq!(db.with(|c| dev_run_now(c, id, T + 1)).unwrap(), 1);
    assert!(db.with(|c| claim(c, &["dev.fail"], "b", T + 1)).unwrap().is_some(), "its lease was ended");
}

/// A sliced job comes back due now with its new place, its attempts start
/// over, and a job already due runs before its next slice.
#[test]
fn a_sliced_job_is_queued_again_with_its_place() {
    let db = db();
    let runner = Runner::new()
        .handle_steps(EXPORT, |_, payload| {
            let n = serde_json::from_str::<serde_json::Value>(payload).unwrap()["n"].as_i64().unwrap();
            Ok(if n < 2 { Step::Again { payload: format!(r#"{{"n":{}}}"#, n + 1), note: String::new() } } else { Step::Done(format!("done at {n}")) })
        })
        .handle(HOUSEKEEPING, |_, _| Ok(String::new()));
    let id = db.with(|c| enqueue(c, EXPORT.name, &serde_json::json!({"n": 0}), T, T)).unwrap().unwrap();
    assert_eq!(runner.run_one(&db, T).unwrap().unwrap().1, Outcome::Again);
    assert_eq!(state(&db, id), ("queued".to_string(), T, 0, None));
    let other = db.with(|c| enqueue(c, HOUSEKEEPING.name, &serde_json::json!({}), T - 1, T)).unwrap().unwrap();
    assert_eq!(runner.run_one(&db, T).unwrap().unwrap().0.id, other);
    let (job, outcome) = runner.run_one(&db, T).unwrap().unwrap();
    assert_eq!((job.id, job.payload.as_str(), job.attempts, outcome), (id, r#"{"n":1}"#, 1, Outcome::Again));
    assert_eq!(runner.run_one(&db, T).unwrap().unwrap().1, Outcome::Done);
    assert_eq!(state(&db, id).0, "done");
}

/// #873: each pass sends a batch from every outbox before one job, so mail
/// never waits behind housekeeping and a big backlog still lets jobs in.
#[test]
fn outboxes_go_first_a_batch_per_pass() {
    let db = db();
    let left = Arc::new(AtomicUsize::new(OUTBOX_BATCH * 2 + 3));
    let l = Arc::clone(&left);
    let drain = Drain::new(
        "test",
        move |_, worker, _| {
            assert!(worker.starts_with("pid"));
            let more = l.load(Ordering::SeqCst) > 0;
            if more {
                l.fetch_sub(1, Ordering::SeqCst);
            }
            Ok(more)
        },
        |_| Ok(Some(T + 5)),
    );
    let runner = Runner::new().drain(drain).handle(HOUSEKEEPING, |_, _| Ok(String::new()));
    let job = db.with(|c| enqueue(c, HOUSEKEEPING.name, &serde_json::json!({}), T, T)).unwrap().unwrap();
    assert!(runner.pass(&db, T));
    assert_eq!(left.load(Ordering::SeqCst), OUTBOX_BATCH + 3, "one batch");
    assert_eq!(state(&db, job).0, "done", "then the job");
    assert!(runner.pass(&db, T));
    assert!(runner.pass(&db, T));
    assert_eq!(left.load(Ordering::SeqCst), 0);
    assert!(!runner.pass(&db, T), "nothing left");
    // The runner's wait counts the outboxes too.
    assert_eq!(runner.next_due(&db, &runner.kinds()), Some(T + 5));
}

/// A gated kind (AI jobs, #874) is taken only while its gate is open, and a
/// handler that hands the row on leaves it running for its own thread.
#[test]
fn a_gated_kind_waits_for_its_gate_and_a_handed_row_stays_running() {
    let db = db();
    let open = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let gate = Arc::clone(&open);
    let runner = Runner::new().handle_claimed(crate::ai::GENERATE, move || gate.load(std::sync::atomic::Ordering::SeqCst), |_, job| Ok(Step::Handed(format!("row {}", job.id))));
    let id = db.with(|c| enqueue(c, crate::ai::GENERATE.name, &serde_json::json!({}), T, T)).unwrap().unwrap();
    assert!(runner.run_one(&db, T).unwrap().is_none(), "gate closed: not taken");
    open.store(true, std::sync::atomic::Ordering::SeqCst);
    let (job, outcome) = runner.run_one(&db, T).unwrap().unwrap();
    assert_eq!((job.id, outcome), (id, Outcome::Handed));
    let row = db.with(|c| recent(c, 1)).unwrap().remove(0);
    assert_eq!((row.state.as_str(), row.locked_until), ("running", Some(T + LEASE)));
}

#[test]
fn housekeeping_is_the_one_hourly_kind_and_one_row_at_idle() {
    let hourly: Vec<_> = KINDS.iter().filter(|k| k.every == Some(HOUR)).map(|k| k.name).collect();
    assert_eq!(hourly, ["housekeeping"]);
    let db = db();
    let runner = Runner::new().handle(HOUSEKEEPING, |_, _| Ok(String::new()));
    assert_eq!(runner.seed(&db, T).unwrap(), 1);
    assert_eq!(runner.seed(&db, T).unwrap(), 0, "a second boot adds no row");
    assert_eq!(db.with(|c| recent(c, 50)).unwrap().len(), 1);
}
