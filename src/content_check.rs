//! The AI content check, stage 1 (#1036, part of #919): Jev scores every
//! part of a made course for harm. Cheap and fast (fractions of a cent a
//! course, see docs/testing.md "Content check calibration").
//!
//! - A course is split into parts ([`chunks`]): one per lesson file, or
//!   several when a lesson's text is long. Each part is sent to Jev with one
//!   yes/no question per category of harm ([`CATEGORIES`], #1046:
//!   misinformation, violence, self-harm, weapons, firearms, hate, mass
//!   tragedies, adult content, minors, sexual crimes, the Terms of Use), all
//!   in one call. Jev answers with a probability for each.
//! - Each part's row in `content_scores` has its hash, the model, the cost,
//!   the rules version ([`AI_CHECK_VERSION`]) and the categories it scored
//!   high on (`high`); its score per category is a row of
//!   `content_category_scores`. A part
//!   whose hash and model didn't change is not sent again at the same
//!   version, so an edit costs one lesson, not the course.
//! - A part scoring its category's threshold or more is queued for stage 2
//!   (`review = 'queued'`, and a [`review::REVIEW`] job): a model reads it
//!   in context and flags the course if it is harmful (#1037, [`review`]).
//!   While it waits the course shows as "needs review" in the superuser
//!   course table (#1034); nothing blocks it. Below the threshold the course
//!   passes.
//! - It runs when an AI generation finishes (a [`SCAN`] job on the jobs
//!   thread) and before a course is shared (`share.rs`, on its own thread,
//!   waited for). Edited courses wait for the nightly run at midnight
//!   ([`nightly`], #1038).
//!
//! Hosted only: Skrongle mini scans nothing. Built-in courses are never
//! scanned: they don't come from the database.
//!
//! One module, one boundary (#1041): it reads a course's files and the
//! jobs table and writes the verdict (scores, flag, report, version); no
//! routes, axum or templates (a test checks its `crate::` paths). The
//! checks run on the server's jobs thread (`SKRONGLE_CONTENT_CHECK=internal`,
//! the default), or in their own process, `skrongle content-check`
//! ([`worker`]) on the same database, with the server only queuing them
//! (`external`). The listed-term check (`course_flags`) always stays in
//! the server.

#[cfg(test)]
mod controls;
pub mod jev;
pub mod nightly;
pub mod review;
pub mod worker;

use std::sync::{Arc, OnceLock};

use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};

use crate::content::CourseFile;
use crate::db::Db;
use crate::jobs::{self, Kind};
pub use jev::Jev;

/// The version of the AI check's rules (#1039): the Jev questions and the
/// threshold here, and stage 2's prompt ([`review::PROMPT`]). Add 1 for a
/// change that could flip a verdict and add a line to
/// docs/content-check-versions.md; a typo or a comment needs none. Every
/// part's score saves the version it ran under
/// (`content_scores.ai_version`), and a stored score counts only at this
/// version: a part scored under an older one is sent to Jev again. The
/// rules are kept between `moderation-criteria(ai)` markers, which
/// scripts/moderation-version-lint.sh compares: CI fails a PR that changes
/// them without a bump unless its body says `moderation-version: unchanged`.
pub const AI_CHECK_VERSION: i64 = 2;

/// One thing a part is scored on (#1046): a Jev yes/no question and the
/// score from which the part goes to stage 2.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Category {
    pub name: &'static str,
    pub question: &'static str,
    pub threshold: f64,
}

impl Category {
    /// The category called `name`.
    pub fn named(name: &str) -> Option<&'static Category> {
        CATEGORIES.iter().find(|c| c.name == name)
    }

    /// The categories as Jev's `(name, question)` pairs.
    pub fn questions() -> Vec<(&'static str, &'static str)> {
        CATEGORIES.iter().map(|c| (c.name, c.question)).collect()
    }
}

// moderation-criteria(ai): start
/// The categories every part is scored on (#1046), one Jev yes/no question
/// each, all asked in one call (Jev reads the text once, so eleven
/// questions cost about 16 % more than two, and a call per category 4.7
/// times as much; docs/testing.md "Content check calibration"). `name` is the row's `category` in
/// `content_category_scores` and the rule a stage 2 verdict names; a part
/// scoring `threshold` or more on any category goes to stage 2. The
/// thresholds come from the calibration run
/// (`scripts/content-check-calibrate.sh`).
pub const CATEGORIES: [Category; 11] = [
    Category {
        name: "misinformation",
        question: "This text presents false claims as facts in a way that could mislead learners about health, science, history or \
                   current events, such as saying vaccines cause autism or that a well-documented event never happened. A myth that \
                   the text corrects, a labelled opinion, or clearly made-up fiction does not count.",
        threshold: 0.5,
    },
    Category {
        name: "violence",
        question: "This text glorifies or encourages violence against people or animals, or dwells on it in gratuitous, graphic \
                   detail. Teaching words for violence, or a news, history or story text that tells of violence without celebrating \
                   it, does not count.",
        threshold: 0.5,
    },
    Category {
        name: "self_harm",
        question: "This text encourages, instructs or glorifies self-harm, suicide or starving oneself.",
        threshold: 0.5,
    },
    Category {
        name: "weapons",
        question: "This text tells how to make or get weapons or explosives to hurt people, or encourages using them on people. \
                   Naming weapons, their history, or sport, hunting and safety rules does not count.",
        threshold: 0.5,
    },
    Category {
        name: "firearms",
        question: "This text tells how to get, build or modify guns to hurt people or get around gun laws, or encourages shooting \
                   people. Words for guns (such as \"pistola\" or \"arma\"), their history, and lawful sport, hunting or gun safety \
                   do not count.",
        threshold: 0.5,
    },
    Category {
        name: "hate",
        question: "This text attacks, demeans or promotes hatred or exclusion of people for their race, ethnicity, nationality, \
                   religion, sex, gender, sexual orientation, disability, age or any other group. Listing a slur so learners know to \
                   avoid it, or describing discrimination critically, does not count.",
        threshold: 0.5,
    },
    Category {
        name: "tragedy",
        question: "This text mocks, trivialises, denies or celebrates a genocide, massacre, terrorist attack or other mass tragedy. \
                   Teaching its history plainly and respectfully does not count.",
        threshold: 0.5,
    },
    Category {
        name: "adult",
        question: "This text is sexually explicit or pornographic, or describes sex acts for arousal. Biology, health and sex \
                   education in plain words, and romance without explicit detail, do not count.",
        threshold: 0.5,
    },
    Category {
        name: "minors",
        question: "This text sexualises children, or encourages or teaches grooming, exploiting or abusing minors, such as telling \
                   a child to keep secrets with an adult from their parents.",
        threshold: 0.5,
    },
    Category {
        name: "sexual_crimes",
        question: "This text jokes about, makes light of, excuses or encourages rape, sexual assault or sexual harassment. \
                   Plainly explaining the law or pointing victims to help does not count.",
        threshold: 0.5,
    },
    Category {
        name: "terms",
        question: "This text breaks a learning site's Terms of Use in another way: it harasses, threatens or exposes a real person, \
                   promotes or teaches crime (selling drugs, fraud, scams, hacking), or tries to get passwords or money from \
                   learners. Teaching a word, its meaning or a neutral example sentence that uses it does not count.",
        threshold: 0.5,
    },
];
// moderation-criteria(ai): end

/// The longest part sent in one call, in characters (about 3,000 tokens).
/// A longer lesson is split at line ends.
pub const PART_CHARS: usize = 12_000;

/// Fields that hold identifiers, not text a learner reads.
const SKIP_KEYS: &[&str] = &["id", "slug", "type", "question_id", "teaches", "prerequisites", "emoji", "art", "who", "set", "group", "voice", "lang", "theme"];

/// The job that scans one course (payload `{"course": id}`).
pub const SCAN: Kind = Kind { name: "content_check.scan", max_attempts: crate::mail::RETRY_DELAYS.len() as i64 + 1, delays: crate::mail::RETRY_DELAYS, every: None, at: None };

/// The server's job that drops a course the worker flagged from its cache
/// (payload `{"course": id}`): the worker can't reach the server's memory.
/// Not a `content_check.*` kind: the server runs it in every mode.
pub const FLAGGED: Kind = Kind { name: "courses.flagged", max_attempts: 3, delays: &[60], every: None, at: None };

/// How long Share waits for a scan before it goes ahead (the scan then
/// finishes as a job).
pub const SHARE_WAIT: std::time::Duration = std::time::Duration::from_secs(20);

/// Hears of each course a check newly flagged (the server drops it from
/// its course cache).
pub type OnFlag = Arc<dyn Fn(&str) + Send + Sync>;

// ---------- where it runs ----------

/// The setting that says where the checks run.
pub const MODE_VAR: &str = "SKRONGLE_CONTENT_CHECK";

/// Where the checks run (#1041).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// On the server's jobs thread (the default).
    Internal,
    /// In `skrongle content-check`: the server queues them and skips them.
    External,
}

impl Mode {
    /// `SKRONGLE_CONTENT_CHECK`: unset, `internal` or `external`.
    pub fn parse(raw: Option<&str>) -> Result<Mode, String> {
        match raw.map(str::trim).unwrap_or("") {
            "" | "internal" => Ok(Mode::Internal),
            "external" => Ok(Mode::External),
            other => Err(format!("{MODE_VAR}={other}: expected internal (the default) or external")),
        }
    }

    pub fn from_env() -> Result<Mode, String> {
        Mode::parse(crate::env("CONTENT_CHECK").as_deref())
    }
}

static EXTERNAL: OnceLock<bool> = OnceLock::new();

/// This server queues the checks for `skrongle content-check`.
pub fn external() -> bool {
    EXTERNAL.get().copied().unwrap_or(false)
}

/// Are courses checked at all: here, or by the worker?
pub fn on() -> bool {
    jev().is_some() || external()
}

// ---------- where Jev is ----------

static JEV: OnceLock<Jev> = OnceLock::new();

/// The key setting, as the Node SDK names it.
pub const KEY_VAR: &str = "TYPESAFE_API_KEY";

fn key() -> String {
    std::env::var(KEY_VAR).unwrap_or_default().trim().to_string()
}

/// The hosted server needs the key (`require_providers`), unless the
/// checks run in the worker: then the key lives there.
pub fn key_missing(mode: Mode) -> bool {
    mode == Mode::Internal && key().is_empty()
}

/// The Jev to call: the real one with the key; in dev mode its stand-in
/// (`/dev/ai/jev` on the dev server at `port`, answering from
/// `data/test-fixtures/llm/jev.json`) unless `SKRONGLE_JEV_MODE=live` and
/// the key are set.
fn jev_for(dev: bool, port: u16) -> Option<Jev> {
    let live = !dev || crate::env("JEV_MODE").as_deref() == Some("live");
    if live && !key().is_empty() {
        let base = std::env::var("TYPESAFE_BASE_URL").ok().filter(|u| !u.trim().is_empty()).unwrap_or_else(|| jev::DEFAULT_BASE_URL.into());
        Some(Jev::new(&base, &key()))
    } else if dev {
        Some(Jev::new(&format!("http://127.0.0.1:{port}{}", crate::url("/dev/ai/jev")), jev::STAND_IN_KEY))
    } else {
        None
    }
}

fn set_jev(jev: Jev) -> String {
    let note = format!("content check: Jev {} at {} ({} categories)", jev.model, jev.base_url, CATEGORIES.len());
    let _ = JEV.set(jev);
    note
}

/// Read the settings once at server startup: in `internal` mode the
/// checks run here with Jev ([`jev_for`]); in `external` mode they are
/// queued for `skrongle content-check`; Skrongle mini has no content
/// check. What to log.
pub fn init(mini: bool, dev: bool, port: u16, mode: Mode) -> String {
    if mini {
        return "content check: off (Skrongle mini)".into();
    }
    if mode == Mode::External {
        let _ = EXTERNAL.set(true);
        return format!("content check: {MODE_VAR}=external: this server queues the checks, `skrongle content-check` runs them");
    }
    match jev_for(dev, port) {
        Some(jev) => set_jev(jev),
        None => format!("content check: off (no {KEY_VAR})"),
    }
}

/// [`init`] for the worker process: Jev must be there. `port` is the dev
/// server's, for its stand-in.
pub fn init_worker(dev: bool, port: u16) -> Result<String, String> {
    jev_for(dev, port).map(set_jev).ok_or_else(|| format!("the content check needs {KEY_VAR}"))
}

/// Jev, when this server scans courses.
pub fn jev() -> Option<&'static Jev> {
    JEV.get()
}

// ---------- the parts of a course ----------

/// One part of a course as Jev sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    /// The file (`lessons/x.json`), with `#2`, `#3`… for the later parts of a long one.
    pub key: String,
    /// The lesson's title, or the file's path.
    pub lesson: String,
    /// What is sent: the course and lesson titles, then the text.
    pub state: String,
    /// SHA-256 (hex) of `state`.
    pub hash: String,
}

/// The texts of a JSON value, one per line, without the identifier fields.
fn texts(v: &serde_json::Value, out: &mut Vec<String>) {
    match v {
        serde_json::Value::String(s) if !s.trim().is_empty() => out.push(s.trim().to_string()),
        serde_json::Value::Array(a) => a.iter().for_each(|x| texts(x, out)),
        serde_json::Value::Object(o) => o.iter().filter(|(k, _)| !SKIP_KEYS.contains(&k.as_str())).for_each(|(_, x)| texts(x, out)),
        _ => {}
    }
}

/// Split `text` into pieces of at most `max` characters, at line ends where
/// it can (a longer line is cut).
fn split(text: &str, max: usize) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    for line in text.lines() {
        let mut line: String = line.to_string();
        while line.chars().count() > max {
            let head: String = line.chars().take(max).collect();
            line = line.chars().skip(max).collect();
            if !cur.is_empty() {
                parts.push(std::mem::take(&mut cur));
            }
            parts.push(head);
        }
        if !cur.is_empty() && cur.chars().count() + 1 + line.chars().count() > max {
            parts.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push('\n');
        }
        cur.push_str(&line);
    }
    if !cur.trim().is_empty() {
        parts.push(cur);
    }
    parts
}

fn sha256(s: &str) -> String {
    Sha256::digest(s.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// The parts of a course titled `title`: every JSON file's texts (a file
/// that doesn't parse, and Markdown or plain text, as it is); pictures and
/// other files are left out.
pub fn chunks(title: &str, files: &[CourseFile]) -> Vec<Chunk> {
    let mut out = Vec::new();
    for f in files {
        let (lesson, text) = if f.path.ends_with(".json") {
            match serde_json::from_str::<serde_json::Value>(&f.content) {
                Ok(v) => {
                    let mut t = Vec::new();
                    texts(&v, &mut t);
                    let name = v.get("title").and_then(|t| t.as_str()).filter(|t| !t.trim().is_empty()).unwrap_or(&f.path).to_string();
                    (name, t.join("\n"))
                }
                Err(_) => (f.path.clone(), f.content.clone()),
            }
        } else if f.path.ends_with(".md") || f.path.ends_with(".txt") {
            (f.path.clone(), f.content.clone())
        } else {
            continue;
        };
        let parts = split(&text, PART_CHARS);
        let many = parts.len() > 1;
        for (i, part) in parts.into_iter().enumerate() {
            let key = if many && i > 0 { format!("{}#{}", f.path, i + 1) } else { f.path.clone() };
            let state = format!("Course: {title}\nLesson: {lesson}\n\n{part}");
            out.push(Chunk { key, lesson: lesson.clone(), hash: sha256(&state), state });
        }
    }
    out
}

// ---------- scoring ----------

/// What scores a part: Jev, or a fake in tests.
pub trait Judge {
    fn model(&self) -> &str;
    /// The probabilities for [`CATEGORIES`], by name, and the input tokens.
    fn judge(&self, state: &str) -> Result<jev::Answers, String>;
}

impl Judge for Jev {
    fn model(&self) -> &str {
        &self.model
    }

    fn judge(&self, state: &str) -> Result<jev::Answers, String> {
        self.ask(state, &Category::questions())
    }
}

/// The categories a part with these answers scored high on (at or over
/// their threshold): when any, it goes to stage 2.
pub fn high(a: &jev::Answers) -> Vec<&'static str> {
    CATEGORIES.iter().filter(|c| a.get(c.name) >= c.threshold).map(|c| c.name).collect()
}

/// What one scan did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    pub parts: usize,
    /// Parts sent to Jev this time (the others had not changed).
    pub scanned: usize,
    pub cost_usd: f64,
    /// The course's score: its highest part score.
    pub score: f64,
    pub needs_review: bool,
    /// Not scanned: the course is gone or being written.
    pub skipped: bool,
}

impl Report {
    /// The log line.
    pub fn note(&self, course_id: &str) -> String {
        if self.skipped {
            return format!("course {course_id}: not scanned (gone, or being generated)");
        }
        format!(
            "course {course_id}: {} parts, {} sent to Jev, cost ${:.6}, score {:.2}: {}",
            self.parts,
            self.scanned,
            self.cost_usd,
            self.score,
            if self.needs_review { "needs review (queued for stage 2)" } else { "passes" }
        )
    }
}

/// Scan a made course: send the parts that changed since their last score
/// to `judge`, store each score as it comes (a retry starts where a failure
/// stopped), drop the scores of parts that are gone. The database is not
/// held while Jev answers.
pub fn scan(db: &Db, judge: &dyn Judge, course_id: &str) -> Result<Report, String> {
    let model = judge.model().to_string();
    // The check covers the files as they are now: an edit from this second on is due again.
    let read_at = crate::db::now();
    let loaded = db
        .with(|c| {
            let Some(row) = crate::courses::course(c, course_id)? else { return Ok(None) };
            if row.is_busy() {
                return Ok(None);
            }
            let files = crate::courses::load_files(c, course_id)?;
            // A score counts only at the current rules (#1039).
            let mut stmt = c.prepare_cached("SELECT chunk, hash, model FROM content_scores WHERE course_id = ?1 AND ai_version = ?2")?;
            let known = stmt
                .query_map(params![course_id, AI_CHECK_VERSION], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Some((row.title, files, known)))
        })
        .map_err(|e| e.to_string())?;
    let Some((title, files, known)) = loaded else { return Ok(Report { skipped: true, ..Report::default() }) };
    let parts = chunks(&title, &files);
    let mut report = Report { parts: parts.len(), ..Report::default() };
    for p in &parts {
        if known.iter().any(|(k, h, m)| *k == p.key && *h == p.hash && *m == model) {
            continue;
        }
        let a = judge.judge(&p.state)?;
        let high = high(&a).join(",");
        let cost = a.cost_usd();
        report.scanned += 1;
        report.cost_usd += cost;
        db.with(|c| {
            c.execute(
                "INSERT INTO content_scores (course_id, chunk, lesson, hash, high, model, input_tokens, cost_usd, review, scanned_at, ai_version)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                 ON CONFLICT(course_id, chunk) DO UPDATE SET lesson = excluded.lesson, hash = excluded.hash, high = excluded.high,
                     model = excluded.model, input_tokens = excluded.input_tokens,
                     cost_usd = excluded.cost_usd, review = excluded.review, scanned_at = excluded.scanned_at, ai_version = excluded.ai_version",
                params![
                    course_id,
                    p.key,
                    p.lesson,
                    p.hash,
                    high,
                    model,
                    a.input_tokens,
                    cost,
                    (!high.is_empty()).then_some("queued"),
                    crate::db::now(),
                    AI_CHECK_VERSION
                ],
            )?;
            // One row per category (#1046); a category that is no longer asked goes.
            c.execute("DELETE FROM content_category_scores WHERE course_id = ?1 AND chunk = ?2", params![course_id, p.key])?;
            let mut ins = c.prepare_cached("INSERT INTO content_category_scores (course_id, chunk, category, score) VALUES (?1, ?2, ?3, ?4)")?;
            for cat in &CATEGORIES {
                ins.execute(params![course_id, p.key, cat.name, a.get(cat.name)])?;
            }
            Ok(())
        })
        .map_err(|e: rusqlite::Error| e.to_string())?;
    }
    let keys: Vec<&str> = parts.iter().map(|p| p.key.as_str()).collect();
    let summary = db
        .with(|c| {
            let mut stmt = c.prepare_cached("SELECT chunk FROM content_scores WHERE course_id = ?1")?;
            let stored = stmt.query_map([course_id], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
            for gone in stored.iter().filter(|k| !keys.contains(&k.as_str())) {
                c.execute("DELETE FROM content_category_scores WHERE course_id = ?1 AND chunk = ?2", params![course_id, gone])?;
                c.execute("DELETE FROM content_scores WHERE course_id = ?1 AND chunk = ?2", params![course_id, gone])?;
            }
            // Stage 2 (#1037): a closer look at what scored high; an AI flag whose text is gone clears.
            review::settle(c, course_id)?;
            // When it was last checked, for the nightly run (#1038) and the course table.
            c.execute(
                "INSERT INTO content_checks (course_id, checked_at) VALUES (?1, ?2) ON CONFLICT(course_id) DO UPDATE SET checked_at = excluded.checked_at",
                params![course_id, read_at],
            )?;
            let s = summary(c, course_id)?;
            if s.as_ref().is_some_and(|s| s.needs_review) {
                review::queue(c, course_id, crate::db::now())?;
            }
            Ok(s)
        })
        .map_err(|e| e.to_string())?;
    if let Some(s) = summary {
        report.score = s.top();
        report.needs_review = s.needs_review;
    }
    Ok(report)
}

/// A part that scored high, for the superuser.
#[derive(Debug, Clone, PartialEq)]
pub struct HighPart {
    pub chunk: String,
    pub lesson: String,
    /// The categories it scored high on (#1046), in [`CATEGORIES`] order.
    pub categories: Vec<String>,
    /// Its score per category, in [`CATEGORIES`] order.
    pub scores: Vec<(String, f64)>,
    /// Stage 2's word on it (#1037): queued, fine or harmful.
    pub review: String,
}

impl HighPart {
    /// Its score on `category`, 0 when it has none.
    pub fn score(&self, category: &str) -> f64 {
        score_of(&self.scores, category)
    }
}

fn score_of(scores: &[(String, f64)], category: &str) -> f64 {
    scores.iter().find(|(n, _)| n == category).map_or(0.0, |(_, s)| *s)
}

/// A course's scan as it stands.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    /// The highest score of any part, per category (#1046): every
    /// category it has scores for, in [`CATEGORIES`] order, then any old
    /// name.
    pub scores: Vec<(String, f64)>,
    pub parts: i64,
    /// What the stored scores cost (each part's latest scan).
    pub cost_usd: f64,
    pub scanned_at: i64,
    /// A part is waiting for stage 2.
    pub needs_review: bool,
    pub high: Vec<HighPart>,
    /// The oldest [`AI_CHECK_VERSION`] among its parts' scores (#1039).
    pub ai_version: i64,
}

impl Summary {
    /// The highest score of all.
    pub fn top(&self) -> f64 {
        self.scores.iter().map(|(_, s)| *s).fold(0.0, f64::max)
    }

    /// The categories that sent a part to stage 2 and that it didn't
    /// call fine (queued or harmful), in [`CATEGORIES`] order: what the
    /// course table shows.
    pub fn high_categories(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for h in self.high.iter().filter(|h| h.review != "fine") {
            for c in &h.categories {
                if !out.contains(c) {
                    out.push(c.clone());
                }
            }
        }
        out.sort_by_key(|c| order(c));
        out
    }
}

/// Where a category sorts: [`CATEGORIES`] order, unknown names last.
fn order(name: &str) -> usize {
    CATEGORIES.iter().position(|c| c.name == name).unwrap_or(CATEGORIES.len())
}

/// A part's (or a course's) scores per category, sorted.
fn sorted(mut scores: Vec<(String, f64)>) -> Vec<(String, f64)> {
    scores.sort_by(|a, b| order(&a.0).cmp(&order(&b.0)).then_with(|| a.0.cmp(&b.0)));
    scores
}

/// The course's scan, `None` when it was never scanned.
pub fn summary(conn: &Connection, course_id: &str) -> rusqlite::Result<Option<Summary>> {
    type Row = (i64, Option<f64>, Option<i64>, i64, Option<i64>);
    let (parts, cost, at, queued, version): Row = conn.query_row(
        "SELECT COUNT(*), SUM(cost_usd), MAX(scanned_at), COALESCE(SUM(review = 'queued'), 0), MIN(ai_version)
         FROM content_scores WHERE course_id = ?1",
        [course_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    )?;
    if parts == 0 {
        return Ok(None);
    }
    let mut stmt = conn.prepare_cached("SELECT category, MAX(score) FROM content_category_scores WHERE course_id = ?1 GROUP BY category")?;
    let scores = sorted(stmt.query_map([course_id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<Vec<_>, _>>()?);
    let mut stmt = conn.prepare_cached("SELECT chunk, lesson, high, review FROM content_scores WHERE course_id = ?1 AND review IS NOT NULL ORDER BY chunk")?;
    let rows = stmt.query_map([course_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?)))?.collect::<Result<Vec<_>, _>>()?;
    let mut high = Vec::new();
    for (chunk, lesson, cats, review) in rows {
        let mut categories: Vec<String> = cats.split(',').filter(|c| !c.is_empty()).map(str::to_string).collect();
        categories.sort_by_key(|c| order(c));
        high.push(HighPart { scores: part_scores(conn, course_id, &chunk)?, chunk, lesson, categories, review });
    }
    // The highest-scoring part first.
    high.sort_by(|a, b| top_of(&b.scores).total_cmp(&top_of(&a.scores)).then_with(|| a.chunk.cmp(&b.chunk)));
    Ok(Some(Summary { scores, parts, cost_usd: cost.unwrap_or(0.0), scanned_at: at.unwrap_or(0), needs_review: queued > 0, high, ai_version: version.unwrap_or(AI_CHECK_VERSION) }))
}

fn top_of(scores: &[(String, f64)]) -> f64 {
    scores.iter().map(|(_, s)| *s).fold(0.0, f64::max)
}

/// One part's scores per category, sorted.
pub fn part_scores(conn: &Connection, course_id: &str, chunk: &str) -> rusqlite::Result<Vec<(String, f64)>> {
    let mut stmt = conn.prepare_cached("SELECT category, score FROM content_category_scores WHERE course_id = ?1 AND chunk = ?2")?;
    let rows = stmt.query_map(params![course_id, chunk], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<Vec<_>, _>>()?;
    Ok(sorted(rows))
}

// ---------- when it runs ----------

/// Queue a scan of the course on the jobs thread (or for the worker),
/// unless one is already waiting or courses aren't checked. The jobs
/// thread looks within a minute.
pub fn queue_scan(conn: &Connection, course_id: &str, now: i64) -> rusqlite::Result<()> {
    if !on() {
        return Ok(());
    }
    enqueue_scan(conn, course_id, now).map(|_| ())
}

/// Queue a [`SCAN`] job for the course, due now, unless one is waiting:
/// the id of the waiting or new job.
fn enqueue_scan(conn: &Connection, course_id: &str, now: i64) -> rusqlite::Result<Option<i64>> {
    let payload = serde_json::json!({ "course": course_id });
    let waiting: Option<i64> = conn
        .query_row(
            "SELECT id FROM jobs WHERE kind = ?1 AND payload_json = ?2 AND state IN ('queued', 'running') ORDER BY id LIMIT 1",
            params![SCAN.name, payload.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    match waiting {
        Some(id) => Ok(Some(id)),
        None => jobs::enqueue(conn, SCAN.name, &payload, now, now),
    }
}

/// The [`SCAN`] job's handler: Jev must be set (it is, when this is registered).
fn run_job(db: &Db, payload: &str) -> Result<String, String> {
    let v: serde_json::Value = serde_json::from_str(payload).map_err(|e| format!("bad payload: {e}"))?;
    let id = v.get("course").and_then(|c| c.as_str()).ok_or("no course in the payload")?;
    let jev = jev().ok_or("the content check is off")?;
    Ok(scan(db, jev, id)?.note(id))
}

/// The server's jobs: [`FLAGGED`] when courses are checked, and in
/// `internal` mode the checks themselves ([`checks`]). In `external` mode
/// its jobs thread skips every `content_check.*` kind: the worker takes
/// them. `forget` drops a course from the server's cache.
pub fn jobs(runner: jobs::Runner, meter: Arc<crate::metering::Meter>, forget: OnFlag) -> jobs::Runner {
    if !on() {
        return runner;
    }
    let f = forget.clone();
    let runner = runner.handle(FLAGGED, move |_, payload| {
        f(&course_of(payload)?);
        Ok(String::new())
    });
    if jev().is_none() {
        return runner;
    }
    checks(runner, meter, forget)
}

/// Register the scan job and the nightly run (Jev is set), and stage 2's
/// review when there is Skrongle AI (`meter`). `on_flag` hears of each
/// course a check flagged.
pub fn checks(runner: jobs::Runner, meter: Arc<crate::metering::Meter>, on_flag: OnFlag) -> jobs::Runner {
    if jev().is_none() {
        return runner;
    }
    let runner = nightly::jobs(runner.handle(SCAN, run_job), on_flag.clone());
    review::jobs(runner, meter, on_flag)
}

/// The course id in a job's payload.
fn course_of(payload: &str) -> Result<String, String> {
    let v: serde_json::Value = serde_json::from_str(payload).map_err(|e| format!("bad payload: {e}"))?;
    v.get("course").and_then(|c| c.as_str()).map(str::to_string).ok_or_else(|| "no course in the payload".into())
}

/// `external` mode: queue a scan for the worker and wait until it is done
/// or failed, at most [`SHARE_WAIT`].
async fn wait_for_worker(db: &Db, course_id: String) {
    let Ok(Some(job)) = db.call(move |c| enqueue_scan(c, &course_id, crate::db::now())).await else { return };
    let until = std::time::Instant::now() + SHARE_WAIT;
    while std::time::Instant::now() < until {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let state = db.call(move |c| c.query_row("SELECT state FROM jobs WHERE id = ?1", [job], |r| r.get::<_, String>(0)).optional()).await;
        if !matches!(state.ok().flatten().as_deref(), Some("queued" | "running")) {
            return;
        }
    }
}

/// Scan now on a thread of its own and wait for it, at most [`SHARE_WAIT`]
/// (before a share). A scan that fails or takes longer is queued as a job
/// instead. `None` when this server doesn't scan; in `external` mode the
/// scan is queued for the worker and waited for (at most [`SHARE_WAIT`]),
/// also `None`.
pub async fn scan_and_wait(db: Db, course_id: String) -> Option<Result<Report, String>> {
    if external() {
        wait_for_worker(&db, course_id).await;
        return None;
    }
    let jev = jev()?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    let (db2, id) = (db.clone(), course_id.clone());
    let spawned = std::thread::Builder::new().name("content-check".into()).spawn(move || {
        let _ = tx.send(scan(&db2, jev, &id));
    });
    let result = match spawned {
        Err(e) => Err(format!("could not start the scan: {e}")),
        Ok(_) => match tokio::time::timeout(SHARE_WAIT, rx).await {
            Ok(Ok(r)) => r,
            Ok(Err(_)) => Err("the scan stopped".into()),
            Err(_) => Err(format!("no answer within {} s", SHARE_WAIT.as_secs())),
        },
    };
    match &result {
        Ok(r) => tracing::info!("content check before sharing: {}", r.note(&course_id)),
        Err(e) => {
            tracing::warn!("content check before sharing course {course_id}: {e}; queued to try again");
            let id = course_id.clone();
            let _ = db.call(move |c| queue_scan(c, &id, crate::db::now())).await;
        }
    }
    Some(result)
}

#[cfg(test)]
mod tests;
