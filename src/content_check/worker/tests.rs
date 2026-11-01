use super::*;
use crate::content_check::{key_missing, nightly::NIGHTLY, review::REVIEW, Mode, FLAGGED, SCAN};

fn payload(id: &str) -> serde_json::Value {
    serde_json::json!({ "course": id })
}

#[test]
fn the_mode_is_internal_unless_set_to_external() {
    assert_eq!(Mode::parse(None), Ok(Mode::Internal));
    assert_eq!(Mode::parse(Some("")), Ok(Mode::Internal));
    assert_eq!(Mode::parse(Some("internal")), Ok(Mode::Internal));
    assert_eq!(Mode::parse(Some(" external ")), Ok(Mode::External));
    let why = Mode::parse(Some("remote")).unwrap_err();
    assert!(why.contains("SKRONGLE_CONTENT_CHECK") && why.contains("external"), "{why}");
}

#[test]
fn an_external_server_does_not_need_the_jev_key() {
    assert!(!key_missing(Mode::External), "the key lives on the worker");
}

#[test]
fn the_status_counts_waiting_checks_and_says_when_the_worker_went_silent() {
    let db = Db::open_in_memory().unwrap();
    db.with(|c| {
        let s = status(c)?;
        assert_eq!(s, Status { seen_at: None, waiting: 0 });
        assert!(s.silent(1_000), "never seen");
        jobs::enqueue(c, SCAN.name, &payload("a"), 0, 0)?;
        jobs::enqueue(c, REVIEW.name, &payload("a"), 0, 0)?;
        jobs::ensure(c, &NIGHTLY, 0)?;
        jobs::enqueue(c, FLAGGED.name, &payload("a"), 0, 0)?;
        check_in(c, "pid1@1", 1_000)?;
        check_in(c, "pid2@2", 2_000)?;
        let s = status(c)?;
        assert_eq!(s, Status { seen_at: Some(2_000), waiting: 2 }, "scans and reviews; not the nightly run or the server's own job");
        assert!(!s.silent(2_000 + SILENT - 1));
        assert!(s.silent(2_000 + SILENT));
        let n: i64 = c.query_row("SELECT COUNT(*) FROM content_check_worker", [], |r| r.get(0))?;
        assert_eq!(n, 1, "one row, the latest check-in");
        Ok(())
    })
    .unwrap();
}

/// The server and the worker open one SQLite file: a job goes to one of
/// them, and to the other only once its lease ran out (the worker stopped).
#[test]
fn two_processes_on_one_file_never_take_the_same_job() {
    let dir = std::env::temp_dir().join(format!("skrongle-check-worker-{}-{}", std::process::id(), crate::random::u64()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("learn.db");
    let server = Db::open(&path).unwrap();
    let worker = Db::open(&path).unwrap();
    let id = server.with(|c| jobs::enqueue(c, SCAN.name, &payload("a"), 100, 100)).unwrap().unwrap();
    // The server in external mode doesn't ask for content_check.* kinds at all.
    assert_eq!(server.with(|c| jobs::claim(c, &[FLAGGED.name], "server", 100)).unwrap(), None);
    let taken = worker.with(|c| jobs::claim(c, &[SCAN.name], "worker-1", 100)).unwrap().unwrap();
    assert_eq!(taken.id, id);
    assert_eq!(server.with(|c| jobs::claim(c, &[SCAN.name], "worker-2", 101)).unwrap(), None, "leased to worker-1");
    // worker-1 stopped mid-check: after the lease the job is taken again, once.
    let again = server.with(|c| jobs::claim(c, &[SCAN.name], "worker-2", 100 + jobs::LEASE)).unwrap().unwrap();
    assert_eq!((again.id, again.attempts), (id, 2));
    let mut jobs_left = 0;
    worker.with(|c| {
        jobs_left = c.query_row("SELECT COUNT(*) FROM jobs WHERE kind = ?1", [SCAN.name], |r| r.get::<_, i64>(0))?;
        Ok(())
    })
    .unwrap();
    assert_eq!(jobs_left, 1, "no duplicate row");
    drop((server, worker));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_worker_runs_only_the_checks() {
    let db = Db::open_in_memory().unwrap();
    let r = runner(&db, Arc::new(Meter::off()));
    for k in jobs::KINDS {
        if r.runs(k.name) {
            assert!(k.name.starts_with("content_check."), "the worker would take {} from the server", k.name);
        }
    }
    assert!(!r.runs(FLAGGED.name), "the server drops flagged courses from its own cache");
}

/// The module stays apart from the web side (#1041): it may reach these
/// parts of the crate and no others, and never axum, askama or the routes.
#[test]
fn the_module_depends_on_no_web_code() {
    let allowed = ["ai", "content", "content_check", "course_flags", "courses", "db", "env", "jobs", "mail", "metering", "url"];
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = vec![root.join("content_check.rs")];
    let mut dirs = vec![root.join("content_check")];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                dirs.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") && p.file_name().is_some_and(|n| n != "tests.rs") {
                files.push(p);
            }
        }
    }
    assert!(files.len() >= 6, "{files:?}");
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap();
        // The code before its tests, without comments.
        let code: String = text.split("\n#[cfg(test)]").next().unwrap_or_default().lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        for bad in ["axum", "askama", "AppState", "AppError", "Template", "crate::routes"] {
            assert!(!code.contains(bad), "{} mentions {bad}", f.display());
        }
        for (i, _) in code.match_indices("crate::") {
            let name: String = code[i + 7..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
            assert!(allowed.contains(&name.as_str()), "{} uses crate::{name}", f.display());
        }
    }
}
