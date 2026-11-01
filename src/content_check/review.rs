//! The AI content check, stage 2 (#1037, part of #919): a model takes a
//! closer look at the parts Jev scored high in stage 1 and decides whether
//! the course is harmful.
//!
//! - Stage 1 marks a high part `review = 'queued'` in `content_scores` and
//!   queues a [`REVIEW`] job for the course ([`queue`]). The job runs on
//!   the jobs thread (an OS thread of its own, not the Tokio pool).
//! - The model gets the high parts with their Jev scores, the whole lesson
//!   around a part of a long lesson, the course title and languages, the
//!   list of its lessons and the superuser's override note if there is one
//!   (#1034). It may ask to read other lessons of the course, at most
//!   [`MAX_READS`] times; then it answers in JSON ([`Verdict`]).
//! - Harmful: the course is flagged (`course_flags`, source `ai`, #1033) with
//!   the model's report and the exact quotes, located to their file and
//!   field. Fine: the parts are marked `fine` on their hash, so the same
//!   text isn't reviewed again; an earlier AI flag whose harmful parts are
//!   all gone is cleared ([`settle`]).
//! - The calls go on the server's key (`ai::chat`, the hosted provider),
//!   each recorded in `ai_usage` under the course owner with purpose
//!   [`PURPOSE`]: the day's spend counts it (the superuser's AI-today bar),
//!   the owner's allowance doesn't (migration 76). Over the day's cap the
//!   review waits (the job tries again later).
//!
//! Hosted only. No routes or templates (#1041 runs it apart).

use std::sync::{Arc, OnceLock};

use rusqlite::{params, Connection};
use serde::Deserialize;

use super::{chunks, texts, Chunk};
use crate::ai::{self, ChatMessage};
use crate::content::CourseFile;
use crate::course_flags::{self, Flag, Match, Source, State};
use crate::db::Db;
use crate::jobs::{self, Kind};
use crate::metering::Meter;

/// The job that reviews one course's queued parts (payload `{"course": id}`).
pub const REVIEW: Kind = Kind { name: "content_check.review", max_attempts: crate::mail::RETRY_DELAYS.len() as i64 + 1, delays: crate::mail::RETRY_DELAYS, every: None, at: None };

/// The `ai::Call` purpose (and the `ai_usage.purpose`) of a review call;
/// the dev simulator answers from `data/test-fixtures/llm/content-review.json`.
pub const PURPOSE: &str = "content-review";

/// How many times the model may ask to read other lessons before it must answer.
pub const MAX_READS: usize = 2;
/// Lessons it may ask for at once.
const FILES_PER_READ: usize = 3;
/// The most of one lesson's text put in front of the model, in characters.
const LESSON_CHARS: usize = 16_000;
/// The longest answer (a verdict is a few hundred tokens).
const MAX_TOKENS: u32 = 1500;
/// Quotes kept from one verdict.
const MAX_QUOTES: usize = 3;

// moderation-criteria(ai): start (a change here may need a new super::AI_CHECK_VERSION)
/// What the model is told. `{reads}` is [`MAX_READS`].
pub const PROMPT: &str = r#"You review a course that a user made on Skrongle, a learning app. An automatic check scored some of its text high for possible harm. Read that text in context and decide whether the course is harmful.

Harmful means one of these:
- "misinformation": it presents false claims as facts that could mislead learners about health, science, history or current events.
- "violence": it glorifies or encourages violence against people or animals, or dwells on it in gratuitous, graphic detail.
- "self_harm": it encourages, instructs or glorifies self-harm, suicide or starving oneself.
- "weapons": it tells how to make or get weapons or explosives to hurt people, or encourages using them on people.
- "firearms": it tells how to get, build or modify guns to hurt people or get around gun laws, or encourages shooting people.
- "hate": it attacks, demeans or promotes hatred or exclusion of people for their race, ethnicity, nationality, religion, sex, gender, sexual orientation, disability, age or any other group.
- "tragedy": it mocks, trivialises, denies or celebrates a genocide, massacre, terrorist attack or other mass tragedy.
- "adult": it is sexually explicit or pornographic.
- "minors": it sexualises children, or encourages or teaches grooming, exploiting or abusing minors.
- "sexual_crimes": it jokes about, makes light of, excuses or encourages rape, sexual assault or sexual harassment.
- "terms": it breaks the Terms of Use in another way: it harasses, threatens or exposes a real person, promotes or teaches crime, or tries to get passwords or money from learners.

Many courses teach a language. Words that look bad out of context are ordinary there: a colour ("negro"), a verb with a slang meaning ("coger"), words for weapons ("pistola", "arma"), drugs, crime, war or illness, an insult listed so learners know to avoid it, a news or history text about a war or a massacre, a lesson on health or mental health that names suicide and points to help. Teaching a word, its meaning or a neutral example sentence that uses it is fine. Judge what the text does to a learner, not which words it contains. When in doubt, answer fine: a person looks at every course you flag, but only at those.

A superuser override note, when one is given, is a moderator's reading of this course after an earlier flag. Follow it, unless the content has clearly changed or goes beyond what the note covers. If you still find the course harmful, the report must say that you considered the note and why it doesn't cover this content.

Before you decide you may read other lessons of the course, {reads} times at most: reply with only {"read": ["<path>", ...]} (up to 3 paths). Otherwise reply with only this JSON object:
{"verdict": "harmful" or "fine", "question": "<one of the names above>" (empty when fine), "quotes": [{"path": "<file path>", "text": "<exact text>"}], "report": "<2 to 4 sentences>"}

For harmful: "question" names the rule it breaks; "quotes" holds the shortest passages that break it, copied character for character from the course, 3 at most; "report" tells a moderator in 2 to 4 plain sentences what is wrong and why the context doesn't excuse it. For fine: "quotes" is empty and "report" says in 1 or 2 sentences why it is fine."#;
// moderation-criteria(ai): end

// ---------- the model ----------

/// What answers a review: the hosted model, or a fake in tests.
pub trait Model {
    /// The model's name, for the log.
    fn name(&self) -> String;
    /// No call now (the day's cap is reached): the job tries again later.
    fn refused(&self, _conn: &Connection) -> rusqlite::Result<bool> {
        Ok(false)
    }
    /// One turn: blocking (the caller is on the jobs thread).
    fn ask(&self, call: &ai::Call, system: &str, messages: &[ChatMessage]) -> Result<ai::Reply, String>;
    /// Record a call that answered; its cost in micro-dollars.
    fn record(&self, _conn: &Connection, _owner: i64, _call: &ai::Call, _usage: ai::Usage) -> rusqlite::Result<i64> {
        Ok(0)
    }
}

/// The server's own key (Skrongle AI).
pub struct Hosted {
    pub meter: Arc<Meter>,
}

impl Model for Hosted {
    fn name(&self) -> String {
        self.meter.cfg.model_for(PURPOSE).to_string()
    }

    fn refused(&self, conn: &Connection) -> rusqlite::Result<bool> {
        self.meter.over_daily_cap(conn)
    }

    fn ask(&self, call: &ai::Call, system: &str, messages: &[ChatMessage]) -> Result<ai::Reply, String> {
        let p = self.meter.provider(PURPOSE).ok_or("no hosted AI on this server")?;
        ai::chat(&p, call, system, messages, MAX_TOKENS)
    }

    fn record(&self, conn: &Connection, owner: i64, call: &ai::Call, usage: ai::Usage) -> rusqlite::Result<i64> {
        self.meter.record_own_cost(conn, owner, call, &self.name(), usage)
    }
}

static MODEL: OnceLock<Hosted> = OnceLock::new();

/// Is stage 2 on (a server that scans and has Skrongle AI)?
pub fn on() -> bool {
    MODEL.get().is_some()
}

/// The model, when stage 2 is on (the nightly run reviews in place, #1038).
pub fn hosted() -> Option<&'static Hosted> {
    MODEL.get()
}

// ---------- what the model answers ----------

/// The model's final answer.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Verdict {
    pub verdict: String,
    #[serde(default)]
    pub question: String,
    #[serde(default)]
    pub quotes: Vec<Quote>,
    #[serde(default)]
    pub report: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Quote {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub text: String,
}

impl Verdict {
    pub fn harmful(&self) -> bool {
        self.verdict.trim().eq_ignore_ascii_case("harmful")
    }

    /// Read a reply: a verdict, or why it isn't one. A harmful verdict
    /// needs a quote and a report.
    fn parse(v: serde_json::Value) -> Result<Verdict, String> {
        let mut out: Verdict = serde_json::from_value(v).map_err(|e| format!("the review's answer is not a verdict: {e}"))?;
        out.verdict = out.verdict.trim().to_lowercase();
        if out.verdict != "harmful" && out.verdict != "fine" {
            return Err(format!("the review's verdict is {:?}, not harmful or fine", out.verdict));
        }
        // A name it made up, or none, counts as the Terms of Use.
        let named = out.question.trim().to_lowercase().replace('-', "_");
        out.question = match super::Category::named(&named) {
            _ if !out.harmful() => String::new(),
            Some(c) => c.name.into(),
            None => "terms".into(),
        };
        out.quotes.retain(|q| !q.text.trim().is_empty());
        out.quotes.truncate(MAX_QUOTES);
        out.report = out.report.trim().to_string();
        if out.harmful() && (out.quotes.is_empty() || out.report.is_empty()) {
            return Err("the review called the course harmful without a quote or a report".into());
        }
        Ok(out)
    }
}

// ---------- the prompt ----------

/// A lesson's readable text (its title first), as stage 1 sends it.
fn lesson_text(f: &CourseFile) -> String {
    match serde_json::from_str::<serde_json::Value>(&f.content) {
        Ok(v) if f.path.ends_with(".json") => {
            let mut t = Vec::new();
            texts(&v, &mut t);
            t.join("\n")
        }
        _ => f.content.clone(),
    }
}

fn cut(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push_str("\n(the rest is cut)");
    out
}

/// A file's title, or its path.
fn title_of(f: &CourseFile) -> String {
    serde_json::from_str::<serde_json::Value>(&f.content)
        .ok()
        .and_then(|v| v.get("title").and_then(|t| t.as_str()).map(str::to_string))
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| f.path.clone())
}

/// "Español (es)" from the manifest's `terms` or `meanings`.
fn language(manifest: &serde_json::Value, side: &str) -> String {
    let field = |name: &str| manifest.get(side).and_then(|s| s.get(name)).and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let (label, lang) = (field("label"), field("lang"));
    match (label.is_empty(), lang.is_empty()) {
        (true, true) => "not set".into(),
        (false, true) => label,
        (true, false) => lang,
        (false, false) => format!("{label} ({lang})"),
    }
}

/// One queued part, as the review reads it.
#[derive(Debug, Clone)]
pub struct Queued {
    pub chunk: Chunk,
    /// The categories it scored high on, with their scores (#1046).
    pub high: Vec<(String, f64)>,
}

/// The first message: the course, the note, the parts that scored high.
pub fn brief(title: &str, files: &[CourseFile], flag: Option<&Flag>, parts: &[Queued]) -> String {
    let manifest: serde_json::Value = files.iter().find(|f| f.path == "course.json").and_then(|f| serde_json::from_str(&f.content).ok()).unwrap_or_default();
    let mut out = format!(
        "Course: {title}\nLanguages: the terms taught are in {}; their meanings are in {}.\n\nThe course's files:\n",
        language(&manifest, "terms"),
        language(&manifest, "meanings")
    );
    for f in files.iter().filter(|f| f.path.ends_with(".json")) {
        out.push_str(&format!("- {}: {}\n", f.path, title_of(f)));
    }
    if let Some((f, note)) = flag.and_then(|f| f.override_note.as_deref().map(|n| (f, n))) {
        out.push_str(&format!("\nSuperuser override note on an earlier flag of this course: \"{note}\"\n"));
        if !f.reason.is_empty() {
            out.push_str(&format!("The earlier flag's report: {}\n", f.reason));
        }
        for m in f.matches.iter().take(MAX_QUOTES) {
            out.push_str(&format!("It was about: \"{}\" in {} ({})\n", m.text, m.lesson, m.path));
        }
    }
    out.push_str("\nThe parts that scored high:\n");
    let all = chunks(title, files);
    for (i, q) in parts.iter().enumerate() {
        out.push_str(&format!(
            "\n### Part {}: {} (lesson \"{}\"), scored high on: {}\n{}\n",
            i + 1,
            q.chunk.key,
            q.chunk.lesson,
            q.high.iter().map(|(c, s)| format!("{c} {s:.2}")).collect::<Vec<_>>().join(", "),
            q.chunk.state
        ));
        // A piece of a long lesson: the whole lesson around it.
        let path = q.chunk.key.split('#').next().unwrap_or(&q.chunk.key);
        if all.iter().any(|c| c.key.starts_with(&format!("{path}#"))) {
            if let Some(f) = files.iter().find(|f| f.path == path) {
                out.push_str(&format!("\nThe whole lesson {path}, for context:\n{}\n", cut(&lesson_text(f), LESSON_CHARS)));
            }
        }
    }
    out
}

/// The reply to a read request: each lesson asked for, as text.
fn read_reply(files: &[CourseFile], paths: &[String], reads_left: usize) -> String {
    let mut out = String::new();
    for p in paths.iter().take(FILES_PER_READ) {
        match files.iter().find(|f| f.path == *p) {
            Some(f) => out.push_str(&format!("### {p}\n{}\n\n", cut(&lesson_text(f), LESSON_CHARS))),
            None => out.push_str(&format!("### {p}\n(no such file in this course)\n\n")),
        }
    }
    if reads_left == 0 {
        out.push_str("You have read all you may. Answer now with the verdict JSON.");
    } else {
        out.push_str(&format!("You may read {reads_left} more time(s), or answer with the verdict JSON."));
    }
    out
}

// ---------- finding a quote ----------

/// Walk `v` for a string containing `needle` (already lower-cased when
/// `lower`): its JSON pointer.
fn find_field(v: &serde_json::Value, needle: &str, lower: bool, at: &str) -> Option<String> {
    match v {
        serde_json::Value::String(s) => {
            let hit = if lower { s.to_lowercase().contains(needle) } else { s.contains(needle) };
            hit.then(|| at.to_string())
        }
        serde_json::Value::Array(a) => a.iter().enumerate().find_map(|(i, x)| find_field(x, needle, lower, &format!("{at}/{i}"))),
        serde_json::Value::Object(o) => o.iter().find_map(|(k, x)| find_field(x, needle, lower, &format!("{at}/{}", k.replace('~', "~0").replace('/', "~1")))),
        _ => None,
    }
}

/// The file and field a quote is in: the file the model named first, then
/// every other; exact, then ignoring case. `None` when it is nowhere.
pub fn locate(files: &[CourseFile], quote: &Quote) -> Option<(String, String)> {
    let text = quote.text.trim();
    let order = files.iter().filter(|f| f.path == quote.path).chain(files.iter().filter(|f| f.path != quote.path));
    for lower in [false, true] {
        let needle = if lower { text.to_lowercase() } else { text.to_string() };
        for f in order.clone() {
            match serde_json::from_str::<serde_json::Value>(&f.content) {
                Ok(v) if f.path.ends_with(".json") => {
                    if let Some(field) = find_field(&v, &needle, lower, "") {
                        return Some((f.path.clone(), field));
                    }
                }
                _ => {
                    let hit = if lower { f.content.to_lowercase().contains(&needle) } else { f.content.contains(&needle) };
                    if hit {
                        return Some((f.path.clone(), String::new()));
                    }
                }
            }
        }
    }
    None
}

/// The verdict's quotes as flag matches: `term` is the rule (a
/// [`super::CATEGORIES`] name), `text` the exact quote. A quote found nowhere keeps the
/// file the model named, with no field.
pub fn matches(files: &[CourseFile], v: &Verdict) -> Vec<Match> {
    v.quotes
        .iter()
        .map(|q| {
            let (path, field) = locate(files, q).unwrap_or_else(|| (q.path.clone(), String::new()));
            let lesson = files.iter().find(|f| f.path == path).map(title_of).unwrap_or_else(|| path.clone());
            Match { term: v.question.clone(), path, field, lesson, text: q.text.trim().to_string() }
        })
        .collect()
}

// ---------- the review ----------

/// What one review did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Outcome {
    /// Parts reviewed.
    pub parts: usize,
    pub calls: usize,
    /// What the calls cost, in micro-dollars.
    pub cost_micros: i64,
    /// `Some(true)` harmful, `Some(false)` fine, `None` nothing to review.
    pub harmful: Option<bool>,
    /// The course was flagged by this review (drop it from the cache).
    pub flagged: bool,
    /// Not reviewed: gone, being written, or nothing queued.
    pub skipped: bool,
}

impl Outcome {
    /// The log line.
    pub fn note(&self, course_id: &str) -> String {
        match self.harmful {
            _ if self.skipped => format!("course {course_id}: nothing to review"),
            Some(h) => format!(
                "course {course_id}: {} part(s) reviewed in {} call(s), cost {}: {}",
                self.parts,
                self.calls,
                crate::metering::dollars(self.cost_micros),
                if !h {
                    "fine"
                } else if self.flagged {
                    "harmful, flagged"
                } else {
                    "harmful (flagged already, or overridden)"
                }
            ),
            None => format!("course {course_id}: nothing to review"),
        }
    }
}

/// Clear an AI flag that no longer has a reason: no part is harmful or
/// waiting for review. An override stays. True when it cleared one.
pub fn settle(conn: &Connection, course_id: &str) -> rusqlite::Result<bool> {
    let Some(f) = course_flags::get(conn, course_id)? else { return Ok(false) };
    if f.state != State::Flagged || f.source != Source::Ai {
        return Ok(false);
    }
    let pending: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM content_scores WHERE course_id = ?1 AND review IN ('queued', 'harmful'))",
        [course_id],
        |r| r.get(0),
    )?;
    if pending {
        return Ok(false);
    }
    // No hash: the listed-term check looks at the files again on the next load.
    course_flags::record(conn, course_id, State::Clear, Source::Ai, &[], "", "", crate::db::now())?;
    course_flags::stamp(conn, course_id, Source::Ai)?;
    tracing::info!("course {course_id}: the AI flag is cleared (its harmful text is gone)");
    Ok(true)
}

/// Review a course's queued parts with `model` and record the verdict. The
/// database is not held while the model answers.
pub fn review(db: &Db, model: &dyn Model, course_id: &str) -> Result<Outcome, String> {
    type Loaded = (String, i64, Vec<CourseFile>, Vec<(String, String, Vec<(String, f64)>)>, Option<Flag>);
    let loaded: Option<Loaded> = db
        .with(|c| {
            let Some(row) = crate::courses::course(c, course_id)? else { return Ok(None) };
            if row.is_busy() {
                return Ok(None);
            }
            let files = crate::courses::load_files(c, course_id)?;
            let mut stmt = c.prepare_cached("SELECT chunk, hash, high FROM content_scores WHERE course_id = ?1 AND review = 'queued' ORDER BY chunk")?;
            let rows = stmt.query_map([course_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?.collect::<Result<Vec<_>, _>>()?;
            let mut queued = Vec::new();
            for (chunk, hash, high) in rows {
                let scores = super::part_scores(c, course_id, &chunk)?;
                let high: Vec<(String, f64)> = scores.into_iter().filter(|(n, _)| high.split(',').any(|h| h == n)).collect();
                queued.push((chunk, hash, high));
            }
            Ok(Some((row.title, row.owner_id, files, queued, course_flags::get(c, course_id)?)))
        })
        .map_err(|e| e.to_string())?;
    let Some((title, owner, files, queued, flag)) = loaded else { return Ok(Outcome { skipped: true, ..Outcome::default() }) };
    // The parts as they are now; one whose text changed since its score waits for its next scan.
    let now_parts = chunks(&title, &files);
    let parts: Vec<Queued> = queued
        .iter()
        .filter_map(|(key, hash, high)| now_parts.iter().find(|p| p.key == *key && p.hash == *hash).map(|p| Queued { chunk: p.clone(), high: high.clone() }))
        .collect();
    if parts.is_empty() {
        db.with(|c| settle(c, course_id)).map_err(|e| e.to_string())?;
        return Ok(Outcome { skipped: true, ..Outcome::default() });
    }
    if db.with(|c| model.refused(c)).map_err(|e| e.to_string())? {
        return Err("Skrongle AI reached its daily cap; the review waits".into());
    }

    // No key: the dev simulator picks its reply by a [fixture:NAME] token in the text.
    let call = ai::Call::new(PURPOSE).course(course_id);
    let system = PROMPT.replace("{reads}", &MAX_READS.to_string());
    let mut messages = vec![ChatMessage { role: "user", content: brief(&title, &files, flag.as_ref(), &parts) }];
    let mut out = Outcome { parts: parts.len(), ..Outcome::default() };
    let mut reads_left = MAX_READS;
    let verdict = loop {
        let reply = model.ask(&call, &system, &messages)?;
        out.calls += 1;
        out.cost_micros += db.with(|c| model.record(c, owner, &call, reply.usage)).unwrap_or_else(|e| {
            tracing::warn!("content review: could not record a call for course {course_id}: {e}");
            0
        });
        let json = ai::parse_json(&reply.text)?;
        let wants: Vec<String> = json.get("read").and_then(|r| r.as_array()).map(|a| a.iter().filter_map(|p| p.as_str().map(str::to_string)).collect()).unwrap_or_default();
        if json.get("verdict").is_none() && !wants.is_empty() {
            if reads_left == 0 {
                return Err(format!("the review asked to read more than {MAX_READS} times"));
            }
            reads_left -= 1;
            messages.push(ChatMessage { role: "assistant", content: reply.text });
            messages.push(ChatMessage { role: "user", content: read_reply(&files, &wants, reads_left) });
            continue;
        }
        break Verdict::parse(json)?;
    };
    out.harmful = Some(verdict.harmful());
    let found = matches(&files, &verdict);
    let reviewed: Vec<(String, String)> = parts.iter().map(|p| (p.chunk.key.clone(), p.chunk.hash.clone())).collect();
    out.flagged = db
        .with(|c| {
            let mark = if verdict.harmful() { "harmful" } else { "fine" };
            for (key, hash) in &reviewed {
                c.execute("UPDATE content_scores SET review = ?4 WHERE course_id = ?1 AND chunk = ?2 AND hash = ?3 AND review = 'queued'", params![course_id, key, hash, mark])?;
            }
            if !verdict.harmful() {
                settle(c, course_id)?;
                return Ok(false);
            }
            let old = course_flags::get(c, course_id)?;
            // A term flag's matches stay with the AI's quotes.
            let mut all: Vec<Match> = old.as_ref().filter(|f| f.blocks() && f.source == Source::Terms).map(|f| f.matches.clone()).unwrap_or_default();
            all.extend(found.iter().cloned());
            course_flags::record(c, course_id, State::Flagged, Source::Ai, &all, &verdict.report, &course_flags::content_hash(&files), crate::db::now())?;
            course_flags::stamp(c, course_id, Source::Ai)?;
            let was_overridden = old.as_ref().is_some_and(|f| f.state == State::Overridden);
            tracing::info!("course {course_id} is flagged: the AI review found it harmful ({}){}", verdict.question, if was_overridden { ", in spite of the override note" } else { "" });
            Ok(!old.as_ref().is_some_and(|f| f.blocks()))
        })
        .map_err(|e| e.to_string())?;
    Ok(out)
}

// ---------- when it runs ----------

/// Queue a review of the course on the jobs thread, unless one is waiting
/// or stage 2 is off (the parts stay queued; the course table shows them).
pub fn queue(conn: &Connection, course_id: &str, now: i64) -> rusqlite::Result<()> {
    if !on() {
        return Ok(());
    }
    let payload = serde_json::json!({ "course": course_id });
    let waiting: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM jobs WHERE kind = ?1 AND payload_json = ?2 AND state = 'queued')",
        params![REVIEW.name, payload.to_string()],
        |r| r.get(0),
    )?;
    if !waiting {
        jobs::enqueue(conn, REVIEW.name, &payload, now, now)?;
    }
    Ok(())
}

/// Register the review job when this process scans and has Skrongle AI.
/// `on_flag` hears of a course it flags (the server drops it from its
/// cache, so it stops opening).
pub fn jobs(runner: jobs::Runner, meter: Arc<Meter>, on_flag: super::OnFlag) -> jobs::Runner {
    if super::jev().is_none() || !meter.hosted() {
        return runner;
    }
    let _ = MODEL.set(Hosted { meter });
    runner.handle(REVIEW, move |db, payload| {
        let v: serde_json::Value = serde_json::from_str(payload).map_err(|e| format!("bad payload: {e}"))?;
        let id = v.get("course").and_then(|c| c.as_str()).ok_or("no course in the payload")?;
        let model = MODEL.get().ok_or("the review is off")?;
        let out = review(db, model, id)?;
        if out.flagged {
            on_flag(id);
        }
        Ok(out.note(id))
    })
}

#[cfg(test)]
mod tests;
