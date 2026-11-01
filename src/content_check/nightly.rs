//! The nightly content check (#1038, part of #919): once a night, at
//! midnight, every made course that is new or was edited since its last
//! check goes through the whole check.
//!
//! - A course is due when the content check never looked at it, or it was
//!   edited (`courses.updated_at`) at or after its last check
//!   (`content_checks.checked_at`, the time the check read its files; every
//!   stage 1 scan records it, the ones after a generation or before a share
//!   too). Taken-down courses and courses being written wait.
//! - Oldest edit first, at most [`Settings::per_night`] courses; the rest
//!   wait for the next night. One course at a time, on the jobs thread: the
//!   listed-term check (#1033), Jev (#1036), then the AI review of what
//!   scored high (#1037).
//! - A course whose check fails (Jev or the model down) gets its own
//!   `content_check.scan` or `content_check.review` job, which the jobs
//!   retry rules try again; the rest of the night goes on.
//! - First, every made course whose last term check ran under an older
//!   term list ([`crate::course_flags::terms::TERMS_VERSION`], #1039) is
//!   checked against the list again: plain string work, so no cap.
//! - Then, while the cap has room, the made courses whose AI check is
//!   [`Settings::recheck_behind`] or more versions behind
//!   [`AI_CHECK_VERSION`] go through both AI stages again (#1040), oldest
//!   check first: an unedited course would otherwise keep a verdict from
//!   old rules. A recheck that flags a course says so in its report.
//! - The run logs how many it checked, passed, flagged and left over, and
//!   what it cost; then it queues itself for the next midnight.
//! - Midnight is in [`zone::DEFAULT`] (Pacific time) unless
//!   `SKRONGLE_CONTENT_CHECK_TZ` names another zone.
//!
//! Hosted only, like the rest of the check. No routes or templates (#1041).

pub mod zone;

use std::sync::OnceLock;

use rusqlite::{params, Connection};

use super::review::{self, Model};
use super::{Judge, AI_CHECK_VERSION};
use crate::db::Db;
use crate::jobs::{self, Kind};
pub use zone::Zone;

/// The nightly job: no payload; it queues its next run at the next midnight.
pub const NIGHTLY: Kind = Kind { name: "content_check.nightly", max_attempts: 3, delays: &[600, 3600], every: None, at: Some(next_midnight) };

/// Courses checked in one night unless `SKRONGLE_CONTENT_CHECK_PER_NIGHT` says otherwise.
pub const PER_NIGHT: usize = 200;

/// How many versions behind [`AI_CHECK_VERSION`] a course's AI check may
/// fall before the nightly run checks it again (#1040), unless
/// `SKRONGLE_CONTENT_CHECK_RECHECK_BEHIND` says otherwise. Not 1: every
/// recheck costs money, and a small bump shouldn't recheck the whole site.
pub const RECHECK_BEHIND: i64 = 3;

/// The nightly run's settings, read from the environment once.
#[derive(Debug, Clone)]
pub struct Settings {
    /// The zone's name or rule, as set.
    pub zone_name: String,
    pub zone: Zone,
    pub per_night: usize,
    /// [`RECHECK_BEHIND`] or its setting.
    pub recheck_behind: i64,
}

impl Settings {
    fn from_env() -> Settings {
        let name = crate::env("CONTENT_CHECK_TZ").unwrap_or_else(|| zone::DEFAULT.into());
        let (zone_name, zone) = match Zone::named(&name) {
            Ok(z) => (name, z),
            Err(e) => {
                tracing::warn!("SKRONGLE_CONTENT_CHECK_TZ: {e}; the nightly content check uses {}", zone::DEFAULT);
                (zone::DEFAULT.into(), Zone::named(zone::DEFAULT).or_else(|_| Zone::parse(zone::DEFAULT_RULE)).unwrap_or(Zone::UTC))
            }
        };
        let per_night = match crate::env("CONTENT_CHECK_PER_NIGHT").map(|n| n.trim().parse::<usize>()) {
            Some(Ok(n)) if n > 0 => n,
            Some(_) => {
                tracing::warn!("SKRONGLE_CONTENT_CHECK_PER_NIGHT is not a number above 0; using {PER_NIGHT}");
                PER_NIGHT
            }
            None => PER_NIGHT,
        };
        let recheck_behind = match crate::env("CONTENT_CHECK_RECHECK_BEHIND").map(|n| n.trim().parse::<i64>()) {
            Some(Ok(n)) if n > 0 => n,
            Some(_) => {
                tracing::warn!("SKRONGLE_CONTENT_CHECK_RECHECK_BEHIND is not a number above 0; using {RECHECK_BEHIND}");
                RECHECK_BEHIND
            }
            None => RECHECK_BEHIND,
        };
        Settings { zone_name, zone, per_night, recheck_behind }
    }
}

static SETTINGS: OnceLock<Settings> = OnceLock::new();

pub fn settings() -> &'static Settings {
    SETTINGS.get_or_init(Settings::from_env)
}

/// The first midnight after `now` in the setting's zone ([`NIGHTLY`]'s `at`).
pub fn next_midnight(now: i64) -> i64 {
    settings().zone.next_midnight(now)
}

// ---------- which courses ----------

/// The courses due for a check, oldest edit first, at most `limit` of
/// them; and how many are due in all.
pub fn due(conn: &Connection, limit: usize) -> rusqlite::Result<(Vec<String>, usize)> {
    const DUE: &str = "FROM courses c LEFT JOIN content_checks k ON k.course_id = c.id
        WHERE c.state NOT IN ('generating', 'revising') AND c.taken_down_at IS NULL
          AND (k.checked_at IS NULL OR c.updated_at >= k.checked_at)";
    let total: i64 = conn.query_row(&format!("SELECT COUNT(*) {DUE}"), [], |r| r.get(0))?;
    let mut stmt = conn.prepare_cached(&format!("SELECT c.id {DUE} ORDER BY c.updated_at, c.id LIMIT ?1"))?;
    let ids = stmt.query_map([limit as i64], |r| r.get(0))?.collect::<Result<Vec<String>, _>>()?;
    Ok((ids, total as usize))
}

/// The made courses whose listed-term check is older than
/// [`TERMS_VERSION`](crate::course_flags::terms::TERMS_VERSION), or never
/// ran (#1039). Courses being written wait.
pub fn terms_behind(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare_cached(
        "SELECT c.id FROM courses c LEFT JOIN course_flags f ON f.course_id = c.id
         WHERE c.state NOT IN ('generating', 'revising') AND (f.course_id IS NULL OR f.terms_version < ?1) ORDER BY c.id",
    )?;
    let ids = stmt.query_map([crate::course_flags::terms::TERMS_VERSION], |r| r.get(0))?.collect::<Result<Vec<String>, _>>()?;
    Ok(ids)
}

/// The SQL condition on `c` (courses) for "its AI check is `behind` or
/// more versions behind" (#1040): its oldest stored Jev score is. A course
/// never scanned has no version and is not behind (it is due instead).
pub fn behind_sql(behind: i64) -> String {
    format!("(SELECT MIN(s.ai_version) FROM content_scores s WHERE s.course_id = c.id) <= {}", AI_CHECK_VERSION - behind)
}

/// The courses waiting for an AI recheck (#1040): made, not taken down, not
/// edited since their last check (those are [`due`]), with an AI check
/// `behind` or more versions old. Oldest check first, at most `limit`, each
/// with the version it was checked under; and how many wait in all.
pub fn ai_behind(conn: &Connection, behind: i64, limit: usize) -> rusqlite::Result<(Vec<(String, i64)>, usize)> {
    let from = format!(
        "FROM courses c JOIN content_checks k ON k.course_id = c.id
         WHERE c.state NOT IN ('generating', 'revising') AND c.taken_down_at IS NULL AND c.updated_at < k.checked_at AND {}",
        behind_sql(behind)
    );
    let total: i64 = conn.query_row(&format!("SELECT COUNT(*) {from}"), [], |r| r.get(0))?;
    let mut stmt = conn.prepare(&format!(
        "SELECT c.id, (SELECT MIN(s.ai_version) FROM content_scores s WHERE s.course_id = c.id) {from} ORDER BY k.checked_at, c.id LIMIT ?1"
    ))?;
    let ids = stmt.query_map([limit as i64], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<Vec<_>, _>>()?;
    Ok((ids, total as usize))
}

// ---------- the run ----------

/// What one night did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Night {
    pub checked: usize,
    pub passed: usize,
    pub flagged: usize,
    /// Checked; a part still waits for the AI review (its job retries).
    pub waiting: usize,
    /// Not checked: the check failed, and a job tries again.
    pub failed: usize,
    /// Due, but past the night's cap (or being written): the next night.
    pub left: usize,
    pub cost_usd: f64,
    /// Checked against a newer term list only (#1039), on top of the cap.
    pub terms_rechecked: usize,
    /// The AI rechecks of unedited courses (#1040), within the cap. Their
    /// cost is in [`Night::cost_usd`] too.
    pub recheck: Recheck,
}

/// What one night's AI rechecks did (#1040).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Recheck {
    /// Went through both AI stages again under the current rules.
    pub ran: usize,
    pub flagged: usize,
    /// Rechecked; a part still waits for the AI review (its job retries).
    pub waiting: usize,
    /// The check failed, and a job tries again.
    pub failed: usize,
    /// Behind, but past the night's cap: a later night.
    pub left: usize,
    pub cost_usd: f64,
}

impl Night {
    /// The log line.
    pub fn note(&self) -> String {
        format!(
            "nightly content check: {} checked against a new term list, {} checked ({} passed, {} flagged, {} waiting for the AI review), {} failed and queued to try again, {} left for the next night, cost ${:.6}",
            self.terms_rechecked, self.checked, self.passed, self.flagged, self.waiting, self.failed, self.left, self.cost_usd
        ) + &format!(
            "; AI rechecks under newer rules: {} rechecked ({} flagged, {} waiting for the AI review, {} failed), {} still waiting for a recheck, cost ${:.6}",
            self.recheck.ran, self.recheck.flagged, self.recheck.waiting, self.recheck.failed, self.recheck.left, self.recheck.cost_usd
        )
    }
}

/// Check up to `per_night` due courses with `judge` (stage 1) and `model`
/// (stage 2; `None` leaves high parts queued), then fill what is left of
/// `per_night` with courses whose AI check is `recheck_behind` or more
/// versions old. `on_flag` hears of each course that became flagged (drop
/// it from the cache).
pub fn run(db: &Db, judge: &dyn Judge, model: Option<&dyn Model>, per_night: usize, recheck_behind: i64, on_flag: &dyn Fn(&str)) -> Result<Night, String> {
    let behind = db.with(|c| terms_behind(c)).map_err(|e| e.to_string())?;
    for id in &behind {
        if term_check(db, id).map_err(|e| e.to_string())? {
            on_flag(id);
        }
    }
    let (ids, total) = db.with(|c| due(c, per_night)).map_err(|e| e.to_string())?;
    let mut night = Night { left: total - ids.len(), terms_rechecked: behind.len(), ..Night::default() };
    for id in &ids {
        check(db, judge, model, id, None, on_flag, &mut night).map_err(|e| e.to_string())?;
    }
    // AI rechecks (#1040) take what the cap has left, after the edited courses.
    let (behind, total) = db.with(|c| ai_behind(c, recheck_behind, per_night - ids.len())).map_err(|e| e.to_string())?;
    let mut again = Night::default();
    for (id, was) in &behind {
        check(db, judge, model, id, Some(*was), on_flag, &mut again).map_err(|e| e.to_string())?;
    }
    night.recheck = Recheck {
        ran: again.checked,
        flagged: again.flagged,
        waiting: again.waiting,
        failed: again.failed,
        left: total - behind.len() + again.left,
        cost_usd: again.cost_usd,
    };
    night.cost_usd += again.cost_usd;
    Ok(night)
}

/// One course through every stage, counted in `night`. Errors are the
/// database's; a stage that fails is queued as its own job. `recheck` is
/// the AI check version an unedited course was last checked under (#1040):
/// a flag from this check says it was a recheck.
fn check(db: &Db, judge: &dyn Judge, model: Option<&dyn Model>, id: &str, recheck: Option<i64>, on_flag: &dyn Fn(&str), night: &mut Night) -> rusqlite::Result<()> {
    // The listed-term check (#1033): it runs again only when the files, the list or its version changed.
    if term_check(db, id)? {
        on_flag(id);
    }
    let report = match super::scan(db, judge, id) {
        Ok(r) if r.skipped => {
            night.left += 1;
            return Ok(());
        }
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("nightly content check, course {id}: {e}; queued to try again");
            db.with(|c| super::enqueue_scan(c, id, crate::db::now()))?;
            night.failed += 1;
            return Ok(());
        }
    };
    night.cost_usd += report.cost_usd;
    if let (true, Some(model)) = (report.needs_review, model) {
        match review::review(db, model, id) {
            Ok(out) => {
                night.cost_usd += out.cost_micros as f64 / 1e6;
                if let (Some(true), Some(was)) = (out.harmful, recheck) {
                    db.with(|c| rechecked(c, id, was))?;
                }
                if out.flagged {
                    on_flag(id);
                }
                // Reviewed here: the job the scan queued has nothing left to do.
                let payload = serde_json::json!({ "course": id }).to_string();
                db.with(|c| c.execute("DELETE FROM jobs WHERE kind = ?1 AND payload_json = ?2 AND state = 'queued'", params![review::REVIEW.name, payload]))?;
            }
            // Its review job (queued by the scan) tries again.
            Err(e) => tracing::warn!("nightly content check, course {id}: the AI review failed: {e}"),
        }
    }
    let (blocks, waiting) = db.with(|c| Ok((crate::course_flags::blocks(c, id)?, super::summary(c, id)?.is_some_and(|s| s.needs_review))))?;
    night.checked += 1;
    if blocks {
        night.flagged += 1;
    } else if waiting {
        night.waiting += 1;
    } else {
        night.passed += 1;
    }
    Ok(())
}

/// Say in an AI flag's report that it came from a recheck under newer
/// rules (#1040): the course passed under version `was`.
fn rechecked(conn: &Connection, id: &str, was: i64) -> rusqlite::Result<usize> {
    let note = format!("Rechecked under AI check version {AI_CHECK_VERSION} (it was last checked under version {was}). ");
    conn.execute("UPDATE course_flags SET reason = ?2 || reason WHERE course_id = ?1 AND source = 'ai' AND state = 'flagged'", params![id, note])
}

/// The listed-term check of one course (#1033); true when it newly flagged it.
fn term_check(db: &Db, id: &str) -> rusqlite::Result<bool> {
    db.with(|c| {
        let was = crate::course_flags::blocks(c, id)?;
        let files = crate::courses::load_files(c, id)?;
        Ok(!was && crate::course_flags::check(c, id, &files)?.is_some_and(|f| f.blocks()))
    })
}

/// Register the nightly job (the content check is on: [`super::jobs`]).
pub fn jobs(runner: jobs::Runner, on_flag: super::OnFlag) -> jobs::Runner {
    let s = settings();
    tracing::info!(
        "nightly content check: at midnight in {}, at most {} courses a night, AI rechecks {} or more versions behind",
        s.zone_name,
        s.per_night,
        s.recheck_behind
    );
    runner.handle(NIGHTLY, move |db, _| {
        let jev = super::jev().ok_or("the content check is off")?;
        let model = review::hosted().map(|m| m as &dyn Model);
        let s = settings();
        Ok(run(db, jev, model, s.per_night, s.recheck_behind, &|id| on_flag(id))?.note())
    })
}

#[cfg(test)]
mod tests;
