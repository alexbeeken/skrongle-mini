//! "Describe what you want to learn" → a whole course, written by an LLM with
//! the server's API key (the owner's, on Manage Users).
//!
//! Every call goes through `complete`, which returns the reply with its
//! token usage.
//!
//! The key is used for one generation job and never stored: it lives in the
//! job struct in memory and is dropped when the job finishes. A second job
//! while one runs is refused, there is no line (`queue.rs`, #874, #43). Generation runs
//! on a plain OS thread (not Tokio's small blocking pool, which SQLite and
//! argon2 need), makes one request for the outline and then one per lesson,
//! validates every file with the normal course loader and saves it into the
//! `course_files` table as it goes. The editor's progress modal polls `/api/generate/{id}`.
//!
//! Two wire formats are supported with the `ureq` client already in the
//! binary: the Anthropic Messages API and OpenAI-compatible chat completions
//! (OpenAI, Google, DeepSeek, Mistral, xAI, OpenRouter and an Ollama-compatible
//! server, one client for all: `providers.rs`, #24).

use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use crate::arrange::{outline_entry, patch_lesson, without_prerequisites};
use crate::content::{ContentStore, CourseFile, MANIFEST_FILE};
use crate::courses;
use crate::db::Db;

mod queue;
pub use queue::*;
mod providers;
pub use providers::*;
mod prompt;
pub use prompt::{Depth, SubjectKind, DEFAULT_INSTRUCTIONS};
use prompt::{material_block, named_block, preview_only};
mod budget;
mod edit;
mod check;
pub use check::{notes_from_json, notes_to_json, LessonNote};
mod edit_lesson;
pub use edit_lesson::EditRun;
pub use budget::{job_tokens, Effort, JOB_PROMPT_TOKENS, LESSON_OUTPUT_TOKENS, OUTLINE_OUTPUT_TOKENS};

/// The course format reference, embedded so the model gets exactly what the
/// documentation says.
pub const FORMAT_DOC: &str = include_str!("../docs/course-format.md");

pub const MAX_LESSONS: usize = 15;
pub const DEFAULT_LESSONS: usize = 6;
/// Finished jobs are dropped from memory after this long.
pub const JOB_TTL_SECS: i64 = 6 * 3600;
/// How long one job may run in all, requests included
/// (`SKRONGLE_AI_JOB_BUDGET_SECS` overrides it): a slow or stalling base URL
/// cannot hold one of the slots (`queue::slots`) for hours.
pub const JOB_BUDGET: Duration = Duration::from_secs(3600);
/// Most bytes read from one model reply (ureq's default is 10 MB). A lesson
/// of `GENERATE_MAX_TOKENS` tokens is well under this.
pub const MAX_REPLY_BYTES: u64 = 1024 * 1024;

/// One turn of a conversation, provider-neutral.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatMessage {
    /// "user" or "assistant".
    pub role: &'static str,
    pub content: String,
}

#[derive(Clone)]
pub struct Provider {
    pub kind: ProviderKind,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    /// Course generation only: how hard the model works (`budget::Effort`).
    pub effort: Option<Effort>,
}

/// Tokens one call used, as the provider reports them (estimated from the
/// text when it does not). `input` excludes the cached part.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cached: u64,
}

impl Usage {
    /// About four characters a token, for providers that report nothing.
    pub fn estimate(system: &str, messages: &[ChatMessage], reply: &str) -> Usage {
        let input = system.chars().count() + messages.iter().map(|m| m.content.chars().count()).sum::<usize>();
        Usage { input: input.div_ceil(4) as u64, output: reply.chars().count().div_ceil(4) as u64, cached: 0 }
    }

    fn is_empty(&self) -> bool {
        self.input == 0 && self.output == 0 && self.cached == 0
    }
}

/// A model's answer and what it cost in tokens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    pub text: String,
    pub usage: Usage,
}

/// Never prints the key, so a `{:?}` in a log line cannot leak it.
impl std::fmt::Debug for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Provider")
            .field("kind", &self.kind)
            .field("base_url", &self.base_url)
            .field("api_key", &"<redacted>")
            .field("model", &self.model)
            .finish()
    }
}

/// What a model call is for: `purpose` names the call site
/// (`generate-outline`, `generate-lesson`, `tutor-turn`), the rest says
/// where it belongs. Sent along in the `x-skrongle-call` header, which the
/// dev simulator logs and picks its fixtures by.
#[derive(Clone, Debug, Default)]
pub struct Call {
    pub purpose: &'static str,
    pub course_id: String,
    pub job_id: String,
    /// What within the purpose: the lesson slug for `generate-lesson`.
    pub key: String,
    /// Facts about the course a canned reply may name as `{{name}}` (the
    /// added lesson goes after `{{last_lesson}}`). Fixtures only.
    pub vars: Vec<(&'static str, String)>,
}

impl Call {
    pub fn new(purpose: &'static str) -> Call {
        Call { purpose, ..Call::default() }
    }

    pub fn course(mut self, id: &str) -> Call {
        self.course_id = id.to_string();
        self
    }

    pub fn job(mut self, id: &str) -> Call {
        self.job_id = id.to_string();
        self
    }

    pub fn key(mut self, key: &str) -> Call {
        self.key = key.to_string();
        self
    }

    pub fn var(mut self, name: &'static str, value: &str) -> Call {
        self.vars.push((name, value.to_string()));
        self
    }
}

/// Largest block of notes / homework a learner can hand in (characters).
pub const MAX_NOTES: usize = 60_000;
/// Largest edited system prompt (characters).
pub const MAX_INSTRUCTIONS: usize = 20_000;
/// Longest "What do you want to learn?" on the Generate dialog (characters).
pub const MAX_WISH: usize = 2000;
/// Fewest characters a box that forms a prompt takes when it is filled in:
/// the Generate dialog's wish and notes, Use AI's text. The live count under
/// each box turns red below this and above the box's maximum.
pub const MIN_PROMPT: usize = 20;
/// The Generate dialog's refusal when there is too little to go on (the
/// dialog's script says the same before posting).
pub const PROMPT_TOO_SHORT: &str =
    "Your prompt is too short! Please tell us what you'd like to learn or paste your notes so the AI has enough information to get to work.";

/// `n` with thousands separators ("60,000"), as the live counts show it.
pub fn grouped(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[derive(Clone, Debug)]
pub struct Request {
    pub user_id: i64,
    pub provider: Provider,
    /// The course row this job writes into (created when it was asked for).
    pub course_id: String,
    /// What the learner wants to learn, in their words.
    pub wish: String,
    /// The lessons the job expects to write, for its progress until the
    /// outline or plan says.
    pub lesson_count: usize,
    /// Full lessons, one preview lesson, or outline lessons (#1119).
    pub depth: Depth,
    /// Notes, homework, a syllabus: material to build the course from.
    pub notes: String,
    pub kind: SubjectKind,
    /// The system prompt: `DEFAULT_INSTRUCTIONS` unless the learner edited it.
    pub instructions: String,
    /// The saved outline of an earlier run of this course: resume it,
    /// writing only the lessons that are missing.
    pub resume_outline: Option<String>,
    /// Revise the finished course with this feedback instead of writing a new
    /// one: plan what changes, rewrite only those lessons, swap them in at once.
    pub revise: Option<String>,
    /// Add one lesson to the finished course from this brief: the AI writes it
    /// and says where it goes; it joins the course as one revision.
    pub add_lesson: Option<String>,
    /// Edit lessons (#1240) from this feedback: rewrite lessons, add, remove or move none (`edit.rs`);
    /// with `lesson`, only that one (#1242, `edit_lesson.rs`).
    pub edit: Option<EditRun>,
    /// Names the learner gave the course before generating it: the outline
    /// keeps the course title and plans these lessons first.
    pub named: crate::courses::Draft,
    /// Translate the course (a copy, #630) instead of writing it: one
    /// request per file, the files not done yet.
    pub translate: Option<Translation>,
    /// The language of the request that started the job (`i18n::snapshot`):
    /// its thread runs in it, so text it words reads as the learner's.
    pub i18n: usize,
}

/// A translation job's plan (`translate::Plan`, saved with the copy).
#[derive(Clone, Debug)]
pub struct Translation {
    pub plan: crate::translate::Plan,
}

/// Longest brief for a new lesson (characters).
pub const MAX_BRIEF: usize = 1000;

/// Longest feedback a revision takes (characters).
pub const MAX_FEEDBACK: usize = 4000;

#[derive(Serialize, Clone, Debug)]
pub struct Status {
    pub id: String,
    pub phase: String,
    pub log: Vec<String>,
    pub lessons_done: usize,
    pub lessons_total: usize,
    pub done: bool,
    pub error: Option<String>,
    pub course_id: Option<String>,
    pub course_title: Option<String>,
    /// A revision of a finished course (feedback, or a lesson added) rather
    /// than a new course: it changes nothing unless it succeeds.
    pub revision: bool,
    /// The revision adds one lesson.
    pub adding: bool,
    /// The revision only rewrites lessons (Edit lessons, #1240).
    pub editing: bool,
    /// "Edit with AI" on one row (#1242): that lesson's title, for the heading.
    pub lesson_title: Option<String>,
    /// A copy being translated (#630).
    pub translating: bool,
    #[serde(skip)]
    pub finished_at: Option<i64>,
    /// The why and check lines of the lessons it wrote (#1238), kept with the run.
    #[serde(skip)]
    pub notes: Vec<LessonNote>,
}

impl Status {
    /// The progress modal's heading.
    pub fn heading(&self) -> String {
        let t = self.lesson_title.as_deref().or(self.course_title.as_deref()).map(|t| format!("\u{201c}{t}\u{201d}")).unwrap_or_else(|| "your course".into());
        match (self.done, self.error.is_some()) {
            (true, true) if self.translating => crate::t("courses.translate.heading_stopped"),
            (true, false) if self.translating => crate::t("courses.translate.heading_done"),
            (false, _) if self.translating => crate::t1("courses.translate.heading_running", "title", t),
            (true, true) if self.adding => "Adding stopped".into(),
            (true, true) if self.editing => "Editing stopped".into(),
            (true, true) if self.revision => "Revision stopped".into(),
            (true, true) => "Generation stopped".into(),
            (true, false) if self.adding => "The new lesson is in".into(),
            (true, false) if self.revision => "Your changes are in".into(),
            (true, false) => "Your course is ready".into(),
            (false, _) if self.adding => format!("Adding a lesson to {t}\u{2026}"),
            (false, _) if self.editing => format!("Editing {t}\u{2026}"),
            (false, _) if self.revision => format!("Revising {t}\u{2026}"),
            (false, _) => format!("Writing {t}\u{2026}"),
        }
    }
}

pub struct Job {
    pub user_id: i64,
    pub course_id: String,
    pub status: Mutex<Status>,
    /// Set by the learner's Cancel; checked before every request.
    cancel: AtomicBool,
    /// When the job's budget (`JOB_BUDGET`) runs out: no request starts
    /// after it, and none runs past it. Set again when a job that waited starts.
    deadline: Mutex<std::time::Instant>,
}

impl Job {
    fn update(&self, f: impl FnOnce(&mut Status)) {
        let mut s = self.status.lock().unwrap_or_else(|p| p.into_inner());
        f(&mut s);
    }

    fn log(&self, line: impl Into<String>) {
        let line = line.into();
        tracing::info!("generate: {line}");
        self.update(|s| s.log.push(line));
    }

    pub fn snapshot(&self) -> Status {
        self.status.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// Ask the job to stop after the request in flight.
    pub fn cancel(&self) {
        if !self.cancel.swap(true, Ordering::SeqCst) {
            self.log("Cancel requested; stopping after the current request.");
            self.update(|s| s.phase = "Cancelling…".into());
        }
    }

    fn check_cancelled(&self) -> Result<(), String> {
        let revision = || self.snapshot().revision;
        if self.cancel.load(Ordering::SeqCst) {
            if revision() {
                Err("Cancelled. The course is unchanged.".into())
            } else {
                Err("Cancelled. The lessons written so far are kept; resume to write the rest.".into())
            }
        } else if self.remaining().is_zero() {
            let minutes = job_budget().as_secs().div_ceil(60);
            if revision() {
                Err(format!("Stopped: the AI took longer than {minutes} minutes in all. The course is unchanged."))
            } else {
                Err(format!("Stopped: the AI took longer than {minutes} minutes in all. The lessons written so far are kept; resume to write the rest."))
            }
        } else {
            Ok(())
        }
    }

    /// Time left of the job's budget.
    fn remaining(&self) -> Duration {
        self.deadline.lock().unwrap_or_else(|p| p.into_inner()).saturating_duration_since(std::time::Instant::now())
    }

    /// One generation request for this job: not after a Cancel or past the
    /// budget, and cut off when the budget runs out. Metered: with the
    /// server's key the allowance is checked first (the job's reservation
    /// counts as available to it), and every call that answers is recorded.
    /// Blocking; the job's thread writes through `Db::with` as for its files.
    fn llm(&self, req: &Request, call: &Call, system: &str, user: &str) -> Result<String, String> {
        self.check_cancelled()?;
        let msgs = [ChatMessage { role: "user", content: user.to_string() }];
        let timeout = request_timeout(GENERATE_TIMEOUT).min(self.remaining()).max(Duration::from_secs(1));
        let send = || complete(&req.provider, call, system, &msgs, budget::max_tokens(&req.provider), timeout);
        send().map(|r| r.text)
    }
}

/// `JOB_BUDGET`, or `SKRONGLE_AI_JOB_BUDGET_SECS`.
fn job_budget() -> Duration {
    crate::env("AI_JOB_BUDGET_SECS").and_then(|v| v.parse::<u64>().ok()).filter(|s| *s > 0).map(Duration::from_secs).unwrap_or(JOB_BUDGET)
}

/// The learner's material, fenced so the model reads it as content.
fn run(job: &Job, db: &Db, registry: &crate::courses::Courses, req: &Request) -> Result<(), String> {
    let system = if req.instructions.trim().is_empty() { DEFAULT_INSTRUCTIONS } else { req.instructions.as_str() };
    if system != DEFAULT_INSTRUCTIONS {
        job.log("Using your edited instructions.");
    }
    if !req.notes.trim().is_empty() {
        job.log(format!("Building from your material ({} characters).", req.notes.trim().chars().count()));
    }
    let course_id = req.course_id.clone();
    let material = material_block(&req.notes);
    let job_id = job.snapshot().id;

    // ---- 1. outline + manifest (or the saved ones, to resume) ----
    let (title, manifest_text, entries, mut written, kind, depth) = match &req.resume_outline {
        Some(saved) => {
            let outline: serde_json::Value = serde_json::from_str(saved).map_err(|e| format!("the saved outline is not valid: {e}"))?;
            let entries = outline.get("lessons").and_then(|l| l.as_array()).cloned().ok_or("the saved outline has no lessons")?;
            let cid = course_id.clone();
            let files = db.with(move |c| crate::courses::load_files(c, &cid)).map_err(|e| format!("database: {e}"))?;
            let manifest = files.iter().find(|f| f.path == MANIFEST_FILE).ok_or("the course has no course.json to resume from")?;
            let title = outline.pointer("/course/title").and_then(|t| t.as_str()).unwrap_or("Course").to_string();
            let manifest_text = manifest.content.clone();
            // Keep the files that still load together; anything else is rewritten.
            let mut written = vec![manifest.clone()];
            for f in files.iter().filter(|f| f.path != MANIFEST_FILE) {
                let mut trial = written.clone();
                trial.push(f.clone());
                if ContentStore::from_files(&trial).is_ok() {
                    written.push(f.clone());
                }
            }
            job.log(format!("Resuming \"{title}\": {} of {} lessons already written.", written.len() - 1, entries.len()));
            // The kind it was planned as; a run from before #953 takes the form's.
            let kind = match SubjectKind::of_course(Some(saved), &manifest_text) {
                SubjectKind::Auto => req.kind,
                k => k,
            };
            // The depth it was planned at; a run from before #1119 wrote full lessons.
            let depth = Depth::parse(outline.get("depth").and_then(|d| d.as_str()).unwrap_or_default());
            (title, manifest_text, entries, written, kind, depth)
        }
        None => {
            let depth = req.depth;
            // A preview keeps only the first lesson the learner named.
            let mut named = req.named.clone();
            if depth == Depth::Preview {
                named.lessons.truncate(1);
            }
            job.update(|s| s.phase = "Planning the course".into());
            let what = match depth {
                Depth::Full => "a course outline",
                Depth::Preview => "a one-lesson preview",
                Depth::Outline => "an outline of every lesson",
            };
            job.log(format!("Asking {} ({}) for {what}…", provider_name(req.provider.kind), req.provider.model));
            let outline_prompt = format!(
                "The learner says they want to learn: \"{}\"\n\n{}\n\n\
                 Design a course for them. Reply with one JSON object of the form\n\
                 {{\"course\": <course.json manifest>, \"lessons\": [<outline entries>]}}\n\
                 where each outline entry is {{\"id\": ..., \"slug\": ..., \"title\": ..., \"unit\": <unit id>, \"group\": <group id>, \
                 \"summary\": <one line>, \"focus\": <the one thing the lesson teaches>, \"prerequisites\": [<lesson ids>]}}.\n\
                 {} Choose `terms`/`meanings` labels and \
                 `lang` codes that fit the subject (for a language course the terms are the language being learnt; for any other course \
                 give both sides the learner's language or leave `lang` empty). Add a `paradigm` only if \
                 the subject has real paradigm tables (verb conjugations, declensions); for a language course with verbs, include the paradigm \
                 with ASCII `art` for each slot. Add `chars` for any accented letters or symbols the learner will need to type \
                 (for maths: ² √ π ≤ ≥ and the like). \
                 Keep the manifest `id` as \"placeholder\"; it will be replaced.{}{material}",
                req.wish.trim(),
                req.kind.guidance(),
                depth.plan_line(MAX_LESSONS),
                named_block(&named),
            );
            let call = Call::new("generate-outline").course(&course_id).job(&job_id);
            let text = job.llm(req, &call, system, &format!("{outline_prompt}{}\n\n----- Course format reference -----\n\n{FORMAT_DOC}", req.kind.rules()))?;
            let outline: serde_json::Value = parse_json(&text)?;
            let mut manifest = outline.get("course").cloned().ok_or("the outline has no \"course\" object")?;
            let mut entries = outline
                .get("lessons")
                .and_then(|l| l.as_array())
                .cloned()
                .ok_or("the outline has no \"lessons\" array")?;
            if entries.is_empty() {
                return Err("the outline lists no lessons".into());
            }
            if depth == Depth::Preview {
                preview_only(&mut manifest, &mut entries);
            }
            check_outline_length(entries.len())?;
            let title = manifest.get("title").and_then(|t| t.as_str()).unwrap_or("New course").trim().to_string();
            let title = if title.is_empty() { "New course".to_string() } else { title };
            // A title the learner chose stays.
            let title = req.named.title.clone().unwrap_or(title);
            manifest["title"] = serde_json::Value::String(title.clone());
            manifest["id"] = serde_json::Value::String(course_id.clone());
            let manifest_text = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())? + "\n";
            // The manifest must load on its own before anything is saved.
            let manifest_file = CourseFile { path: MANIFEST_FILE.into(), content: manifest_text.clone() };
            check_file_caps(std::slice::from_ref(&manifest_file)).map_err(|e| format!("the generated course.json is not valid: {e}"))?;
            ContentStore::from_files(&[manifest_file]).map_err(|e| format!("the generated course.json is not valid: {e}"))?;
            // The kind is settled here, once, and saved for every later request.
            let kind = req.kind.settled(&outline, &manifest_text);
            let mut saved = serde_json::json!({ "course": manifest, "lessons": entries });
            if let Some(o) = saved.as_object_mut() {
                if kind != SubjectKind::Auto {
                    o.insert("kind".into(), kind.as_str().into());
                }
                if depth != Depth::Full {
                    o.insert("depth".into(), depth.as_str().into());
                }
            }
            let saved = saved.to_string();
            let (cid, t, mt) = (course_id.clone(), title.clone(), manifest_text.clone());
            db.with(move |c| {
                courses::write_file(c, &cid, MANIFEST_FILE, &mt)?;
                courses::set_outline(c, &cid, &t, &saved)
            })
            .map_err(|e| format!("database: {e}"))?;
            job.log(format!("Outline ready: \"{title}\" with {} lessons.", entries.len()));
            let written = vec![CourseFile { path: MANIFEST_FILE.into(), content: manifest_text.clone() }];
            (title, manifest_text, entries, written, kind, depth)
        }
    };
    // A saved outline was checked when it was made; check it again anyway.
    check_outline_length(entries.len())?;
    let already =|entry: &serde_json::Value, written: &[CourseFile]| {
        entry
            .get("slug")
            .and_then(|s| s.as_str())
            .map(|slug| written.iter().any(|f| f.path == format!("lessons/{slug}.json")))
            .unwrap_or(false)
    };
    let done_before = entries.iter().filter(|e| already(e, &written)).count();
    job.update(|s| {
        s.course_title = Some(title.clone());
        s.lessons_total = entries.len();
        s.lessons_done = done_before;
        s.phase = "Writing lessons".into();
    });

    // ---- 2. one request per lesson ----
    let mut done = done_before;
    for (i, entry) in entries.iter().enumerate() {
        if already(entry, &written) {
            continue;
        }
        let lesson_title = entry.get("title").and_then(|t| t.as_str()).unwrap_or("lesson").to_string();
        job.update(|s| s.phase = format!("Writing lesson {} of {}: {lesson_title}", i + 1, entries.len()));
        // What the lessons before it teach, the course map and its neighbours
        // in full (#446), so it builds on them.
        let digest = crate::arrange::written_digest(&written);
        let base_prompt = format!(
            "{digest}Write the complete lesson JSON file for this lesson of the course \"{title}\":\n{}\n\n\
             Use exactly these values for id, slug, title, unit, group and prerequisites. \
             The course manifest is:\n{}\n\n{}\
             Reply with the lesson JSON object only.{}{}{material}",
            serde_json::to_string_pretty(entry).unwrap_or_default(),
            manifest_text,
            crate::arrange::lesson_context(&manifest_text, &entries, i, &written),
            prompt::lesson_line(kind),
            depth.lesson_line(),
        );
        let prompt = format!("{base_prompt}{}\n\n----- Course format reference -----\n\n{FORMAT_DOC}", kind.rules());
        let slug = entry.get("slug").and_then(|s| s.as_str()).unwrap_or_default();
        let call = Call::new("generate-lesson").course(&course_id).job(&job_id).key(slug);
        let mut attempt = 0;
        let mut feedback = String::new();
        let file = loop {
            attempt += 1;
            let text = job.llm(req, &call, system, &format!("{prompt}{feedback}"))?;
            let candidate = match parse_json(&text).and_then(|v| lesson_file(&v, entry)) {
                Ok(f) => f,
                Err(e) => {
                    if attempt >= 3 {
                        return Err(format!("lesson {}: {e}", i + 1));
                    }
                    job.log(format!("Lesson {} came back malformed ({e}); asking again.", i + 1));
                    feedback = format!("\n\nYour previous reply could not be used: {e}. Reply with the corrected lesson JSON only.");
                    continue;
                }
            };
            // Validate against the manifest and everything written so far,
            // within the editor's caps on a course's files.
            let mut trial = written.clone();
            trial.push(candidate.clone());
            match check_file_caps(&trial).and_then(|()| ContentStore::from_files(&trial).map(drop).map_err(|e| e.to_string())) {
                Ok(_) => break candidate,
                Err(e) if attempt < 3 => {
                    job.log(format!("Lesson {} did not validate ({e}); asking for a fix.", i + 1));
                    feedback = format!("\n\nYour previous reply was rejected by the validator: {e}. Fix it and reply with the corrected lesson JSON only.");
                }
                Err(e) => return Err(format!("lesson {} did not validate after {attempt} attempts: {e}", i + 1)),
            }
        };
        let (cid, path, content) = (course_id.clone(), file.path.clone(), file.content.clone());
        db.with(move |c| courses::write_file(c, &cid, &path, &content)).map_err(|e| format!("database: {e}"))?;
        written.push(file);
        done += 1;
        job.update(|s| s.lessons_done = done);
        job.log(format!("Lesson {} of {} saved: {lesson_title}", i + 1, entries.len()));
    }

    // ---- 3. final check ----
    let cid = course_id.clone();
    let err = db.with(move |c| courses::revalidate(c, registry, &cid)).map_err(|e| format!("database: {e}"))?;
    if let Some(e) = err {
        return Err(format!("the finished course does not load: {e}"));
    }
    // The course lints (#1199), one log line each; the repeated-term ones are #446's dedupe report.
    let to_check = crate::lint::log_files(&written);
    job.log(if to_check > 0 { format!("Course complete and ready to open, with {to_check} things to check in the editor's report.") } else { "Course complete and ready to open.".to_string() });
    Ok(())
}

/// What a revision plan does with one lesson.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Change {
    Keep,
    Rewrite,
    New,
    /// Read for errors, unchanged unless a fact is wrong (#1238).
    Check,
}

/// One lesson of a revised course: its outline entry, what happens to it,
/// the file it has now (none for a new lesson) and, once written, its new file.
#[derive(Debug)]
struct Slot {
    entry: serde_json::Value,
    change: Change,
    instructions: String,
    old: Option<(String, serde_json::Value)>,
    done: Option<CourseFile>,
    /// The lesson request's why and check lines (`check::read_reply`).
    why: Vec<String>,
    check: Vec<String>,
}

impl Slot {
    fn id(&self) -> &str {
        self.entry.get("id").and_then(|v| v.as_str()).unwrap_or_default()
    }
}

/// Read a revision plan against the course as it is. Lenient where the
/// intent is clear (a "new" lesson that exists is a rewrite, a "keep" of an
/// unknown id is new), strict where it is not.
fn read_plan(
    plan: &serde_json::Value,
    lessons: &HashMap<String, (String, serde_json::Value)>,
    course_id: &str,
    manifest: &serde_json::Value,
) -> Result<(serde_json::Value, Vec<Slot>), String> {
    let mut new_manifest = match plan.get("course") {
        Some(m) if m.is_object() => m.clone(),
        _ => manifest.clone(),
    };
    new_manifest["id"] = serde_json::Value::String(course_id.to_string());
    let entries = plan.get("lessons").and_then(|l| l.as_array()).ok_or("the plan has no \"lessons\" array")?;
    if entries.is_empty() {
        return Err("the plan removes every lesson".into());
    }
    if entries.len() > MAX_LESSONS {
        return Err(format!("the plan has {} lessons; a course may have at most {MAX_LESSONS}", entries.len()));
    }
    let mut slots: Vec<Slot> = Vec::new();
    let mut slugs = std::collections::HashSet::new();
    for (i, e) in entries.iter().enumerate() {
        let id = e.get("id").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()).ok_or(format!("plan entry {} has no id", i + 1))?;
        if slots.iter().any(|s| s.id() == id) {
            return Err(format!("the plan lists lesson '{id}' twice"));
        }
        let asked = match e.get("change").and_then(|c| c.as_str()).unwrap_or("") {
            "rewrite" => Change::Rewrite,
            "new" => Change::New,
            "keep" => Change::Keep,
            "check" => Change::Check,
            _ if lessons.contains_key(id) => Change::Keep,
            _ => Change::New,
        };
        let old = lessons.get(id).cloned();
        let change = match (asked, &old) {
            (Change::New, Some(_)) => Change::Rewrite,
            (_, None) => Change::New,
            (c, Some(_)) => c,
        };
        let instructions = e.get("instructions").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        let entry = match (&old, change) {
            // A kept or checked lesson is what its file says, whatever the plan wrote.
            (Some((_, v)), Change::Keep | Change::Check) => outline_entry(v, e.get("focus")),
            (Some((_, v)), _) => {
                // A rewrite keeps its id and slug, so progress and links stay with it.
                let mut entry = e.clone();
                entry["slug"] = v.get("slug").cloned().unwrap_or_default();
                if let Some(m) = entry.as_object_mut() {
                    // number and has describe the old lesson in the prompt; they are no lesson fields.
                    for k in ["change", "instructions", "number", "has"] { m.remove(k); }
                }
                entry
            }
            (None, _) => {
                let slug = e.get("slug").and_then(|v| v.as_str()).unwrap_or_default();
                if !crate::content::is_slug(id) || !crate::content::is_slug(slug) {
                    return Err(format!("new lesson '{id}': id and slug must be lowercase a-z, 0-9 and '-'"));
                }
                let mut entry = e.clone();
                if let Some(m) = entry.as_object_mut() {
                    // number and has describe the old lesson in the prompt; they are no lesson fields.
                    for k in ["change", "instructions", "number", "has"] { m.remove(k); }
                }
                entry
            }
        };
        let slug = entry.get("slug").and_then(|v| v.as_str()).unwrap_or_default().to_string();
        if !slugs.insert(slug.clone()) || (old.is_none() && lessons.values().any(|(_, v)| v.get("slug").and_then(|s| s.as_str()) == Some(slug.as_str()))) {
            return Err(format!("the plan uses the slug '{slug}' twice"));
        }
        slots.push(Slot { entry, change, instructions, old, done: None, why: vec![], check: vec![] });
    }
    // Prerequisites may only name lessons that stay.
    let ids: Vec<String> = slots.iter().map(|s| s.id().to_string()).collect();
    for s in &mut slots {
        if let Some(pre) = s.entry.get("prerequisites").and_then(|p| p.as_array()) {
            let kept: Vec<serde_json::Value> = pre.iter().filter(|p| p.as_str().is_some_and(|p| ids.iter().any(|i| i == p))).cloned().collect();
            s.entry["prerequisites"] = serde_json::Value::Array(kept);
        }
    }
    Ok((new_manifest, slots))
}

/// The course as it would be with the slots as they stand (and `candidate`
/// in slot `at`): written lessons, kept ones, old versions of lessons still to
/// be rewritten. New lessons not written yet are left out, and so are
/// prerequisites that point at them.
fn trial(manifest: &str, slots: &[Slot], others: &[CourseFile], at: Option<(usize, &CourseFile)>) -> Result<Vec<CourseFile>, String> {
    let pending: Vec<&str> = slots
        .iter()
        .enumerate()
        .filter(|(i, s)| s.done.is_none() && s.change == Change::New && at.map(|(a, _)| a) != Some(*i))
        .map(|(_, s)| s.id())
        .collect();
    let mut files = vec![CourseFile { path: MANIFEST_FILE.into(), content: manifest.to_string() }];
    files.extend(others.iter().cloned());
    for (i, s) in slots.iter().enumerate() {
        let file = match (at, &s.done, &s.old) {
            (Some((a, c)), _, _) if a == i => c.clone(),
            (_, Some(f), _) => f.clone(),
            (_, None, Some((_, v))) => lesson_file(v, &s.entry)?,
            (_, None, None) => continue,
        };
        let content = without_prerequisites(&file.content, |p| pending.contains(&p))?.unwrap_or(file.content);
        files.push(CourseFile { path: file.path, content });
    }
    Ok(files)
}

/// Revise a finished course from the learner's feedback. One request plans
/// the change (which lessons stay, are rewritten, are new; a lesson left out
/// is removed), then one request per lesson to (re)write. Nothing is saved
/// until the whole new course validates; then it replaces the old files in
/// one go and the old set is kept for Undo. Any failure leaves the course
/// exactly as it was.
fn revise(job: &Job, db: &Db, registry: &crate::courses::Courses, req: &Request, feedback: &str) -> Result<(), String> {
    let system = if req.instructions.trim().is_empty() { DEFAULT_INSTRUCTIONS } else { req.instructions.as_str() };
    let course_id = req.course_id.clone();
    let job_id = job.snapshot().id;
    job.update(|s| s.phase = "Reading the course".into());
    let cid = course_id.clone();
    let (files, row, notes) = db
        .with(move |c| Ok((courses::load_files(c, &cid)?, courses::course(c, &cid)?, courses::notes_of(c, &cid)?)))
        .map_err(|e| format!("database: {e}"))?;
    let row = row.ok_or("the course is gone")?;
    let store = ContentStore::from_files(&files).map_err(|e| format!("the course does not load, so it cannot be revised: {e}"))?;
    let manifest_file = files.iter().find(|f| f.path == MANIFEST_FILE).ok_or("the course has no course.json")?;
    let manifest: serde_json::Value = serde_json::from_str(&manifest_file.content).map_err(|e| format!("course.json: {e}"))?;
    let mut lessons: HashMap<String, (String, serde_json::Value)> = HashMap::new();
    let mut others = Vec::new();
    for f in files.iter().filter(|f| f.path != MANIFEST_FILE) {
        if !(f.path.starts_with("lessons/") && f.path.ends_with(".json")) {
            others.push(f.clone());
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(&f.content).map_err(|e| format!("{}: {e}", f.path))?;
        let id = v.get("id").and_then(|i| i.as_str()).ok_or(format!("{}: not a single lesson with an id; change it by hand instead", f.path))?;
        lessons.insert(id.to_string(), (f.path.clone(), v.clone()));
    }
    let focus: HashMap<String, serde_json::Value> = row
        .outline
        .as_deref()
        .and_then(|o| serde_json::from_str::<serde_json::Value>(o).ok())
        .and_then(|o| o.get("lessons").and_then(|l| l.as_array()).cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|e| Some((e.get("id")?.as_str()?.to_string(), e.get("focus")?.clone())))
        .collect();
    let current: Vec<serde_json::Value> = (store.lessons().iter().enumerate())
        .filter_map(|(i, l)| lessons.get(&l.id).map(|(_, v)| prompt::plan_entry(i + 1, l, outline_entry(v, focus.get(&l.id))))).collect();
    let title = store.title().to_string();
    job.update(|s| s.course_title = Some(title.clone()));
    let material = material_block(&notes);
    let kind = SubjectKind::of_course(row.outline.as_deref(), &manifest_file.content);
    let rules = kind.rules();

    // ---- 1. the plan ----
    job.update(|s| s.phase = "Planning the changes".into());
    job.log(format!("Asking {} ({}) how to change \"{title}\"…", provider_name(req.provider.kind), req.provider.model));
    // A course made with AI says what it was asked for; a hand-made one has no brief.
    let made = match row.brief.trim() {
        "" => "A learner is studying this course.".to_string(),
        brief => format!("A learner made this course with you earlier. They asked for: \"{brief}\""),
    };
    let plan_prompt = format!(
        "{made}\n\n\
         Its course.json manifest:\n{}\n\nIts lessons, in order:\n{}\n\n\
         {} Now they want it changed. Their feedback, between the <feedback> tags, says what to change or focus on:\n\
         <feedback>\n{}\n</feedback>\n\n\
         Plan the revision, changing only what the feedback asks for: a lesson the feedback does not concern stays exactly as it is. \
         Reply with one JSON object {{\"course\": <the course.json manifest>, \"lessons\": [<entries, in the new order>]}} where each entry is \
         {{\"id\": ..., \"slug\": ..., \"title\": ..., \"unit\": <unit id>, \"group\": <group id>, \"summary\": <one line>, \
         \"focus\": <the one thing the lesson teaches>, \"prerequisites\": [<lesson ids>], \"change\": \"keep\" | \"rewrite\" | \"new\", \
         \"instructions\": <for rewrite and new: what this lesson must cover or do differently>}}.\n\
         - \"keep\": the lesson stays as it is.\n\
         - \"rewrite\": the lesson is written again; keep its id and slug so the learner's progress stays with it.\n\
         - \"new\": a lesson that does not exist yet, with a new id and slug.\n\
         {}\
         - A lesson left out of the list is removed.\n\
         Return the manifest unchanged unless the feedback needs it changed (a new unit, a better description); keep its \"id\" as \
         \"{course_id}\". Prerequisites may only name lessons in your list. At most {MAX_LESSONS} lessons.{material}",
        manifest_file.content.trim(),
        serde_json::to_string_pretty(&current).unwrap_or_default(),
        prompt::NUMBER_LINE,
        feedback.trim(),
        check::PLAN_LINE,
    );
    let call = Call::new("revise-plan").course(&course_id).job(&job_id);
    let mut retry = String::new();
    let mut attempt = 0;
    let (new_manifest, mut slots) = loop {
        attempt += 1;
        let text = job.llm(req, &call, system, &format!("{plan_prompt}{retry}{rules}\n\n----- Course format reference -----\n\n{FORMAT_DOC}"))?;
        let planned = parse_json(&text).and_then(|p| read_plan(&p, &lessons, &course_id, &manifest)).and_then(|(m, slots)| {
            let text = serde_json::to_string_pretty(&m).map_err(|e| e.to_string())? + "\n";
            // The plan must fit the lessons it keeps before anything is written.
            ContentStore::from_files(&trial(&text, &slots, &others, None)?).map_err(|e| format!("the planned course does not load: {e}"))?;
            Ok((m, slots))
        });
        match planned {
            Ok(p) => break p,
            Err(e) if attempt < 2 => {
                job.log(format!("The plan could not be used ({e}); asking again."));
                retry = format!("\n\nYour previous plan could not be used: {e}. Reply with the corrected plan JSON only.");
            }
            Err(e) => return Err(format!("the plan could not be used: {e}. The course is unchanged.")),
        }
    };
    let manifest_text = if new_manifest == manifest {
        manifest_file.content.clone()
    } else {
        serde_json::to_string_pretty(&new_manifest).map_err(|e| e.to_string())? + "\n"
    };
    let count = |c: Change| slots.iter().filter(|s| s.change == c).count();
    let removed = lessons.len() - count(Change::Keep) - count(Change::Rewrite) - count(Change::Check);
    let to_write = count(Change::Rewrite) + count(Change::New) + count(Change::Check);
    job.log(format!(
        "Plan: keep {}, check {}, rewrite {}, add {}, remove {removed}{}.",
        count(Change::Keep),
        count(Change::Check),
        count(Change::Rewrite),
        count(Change::New),
        if manifest_text != manifest_file.content { ", and change course.json" } else { "" }
    ));
    let new_title = new_manifest.get("title").and_then(|t| t.as_str()).filter(|t| !t.trim().is_empty()).unwrap_or(&title).trim().to_string();
    job.update(|s| {
        s.course_title = Some(new_title.clone());
        s.lessons_total = to_write;
        s.lessons_done = 0;
        s.phase = "Writing lessons".into();
    });

    // Kept lessons stay byte for byte, unless a prerequisite they name is removed.
    let planned_ids: Vec<String> = slots.iter().map(|s| s.id().to_string()).collect();
    for s in slots.iter_mut().filter(|s| s.change == Change::Keep) {
        let path = s.old.as_ref().map(|(p, _)| p.clone()).unwrap_or_default();
        let content = files.iter().find(|f| f.path == path).map(|f| f.content.clone()).unwrap_or_default();
        let content = without_prerequisites(&content, |p| !planned_ids.iter().any(|i| i == p))?.unwrap_or(content);
        s.done = Some(CourseFile { path, content });
    }

    // ---- 2. one request per lesson to write ----
    let rewrite = edit::Rewrite { job, req, system, files: &files, others: &others, manifest_text: &manifest_text, title: &new_title, feedback, material: &material, kind, depth: req.depth };
    edit::write_slots(&rewrite, &mut slots, to_write)?;

    // ---- 3. the new course, in the planned order ----
    let mut final_files = trial(&manifest_text, &slots, &others, None)?;
    check_file_caps(&final_files).map_err(|e| format!("the revised course is too large: {e}. The course is unchanged."))?;
    let planned = planned_ids;
    let loaded = ContentStore::from_files(&final_files).map_err(|e| format!("the revised course does not load: {e}. The course is unchanged."))?;
    if loaded.lessons().iter().map(|l| l.id.as_str()).ne(planned.iter().map(String::as_str)) {
        // Lessons sort by unit, then `order`: number them as planned.
        for f in final_files.iter_mut().filter(|f| f.path != MANIFEST_FILE && f.path.starts_with("lessons/")) {
            let id = serde_json::from_str::<serde_json::Value>(&f.content).ok().and_then(|v| v.get("id")?.as_str().map(String::from));
            if let Some(pos) = id.and_then(|id| planned.iter().position(|p| *p == id)) {
                if let Some(c) = patch_lesson(&f.content, "order", serde_json::json!(pos + 1))? {
                    f.content = c;
                }
            }
        }
        ContentStore::from_files(&final_files).map_err(|e| format!("the revised course does not load: {e}. The course is unchanged."))?;
    }
    final_files.sort_by(|a, b| a.path.cmp(&b.path));
    let mut before = files.clone();
    before.sort_by(|a, b| a.path.cmp(&b.path));
    if final_files.iter().map(|f| (&f.path, &f.content)).eq(before.iter().map(|f| (&f.path, &f.content))) {
        return Err(edit::nothing_changed(job, &slots));
    }
    job.update(|s| s.notes = check::notes_of(&slots, &files));
    let mut outline = serde_json::json!({
        "course": new_manifest,
        "lessons": slots.iter().map(|s| s.entry.clone()).collect::<Vec<_>>(),
    });
    if kind != SubjectKind::Auto {
        if let Some(o) = outline.as_object_mut() {
            o.insert("kind".into(), kind.as_str().into());
        }
    }
    let outline = outline.to_string();
    let (cid, t, label) = (course_id.clone(), new_title.clone(), courses::quoted("Changed with feedback:", feedback));
    let err = db
        .with(move |c| {
            courses::commit_revision(c, &cid, &final_files, &t, Some(&outline), &label)?;
            courses::revalidate(c, registry, &cid)
        })
        .map_err(|e| format!("database: {e}"))?;
    if let Some(e) = err {
        return Err(format!("the revised course does not load: {e}. Undo it from the course page."));
    }
    job.log("Revision saved. Undo puts the previous version back.");
    Ok(())
}

/// Add one lesson to a finished course from the learner's brief. One request
/// writes the lesson and says which lesson it follows (the AI picks the unit
/// and the place); it is validated with the whole course, up to three tries,
/// and joins it as one revision, so Undo takes it out again.
fn add_lesson(job: &Job, db: &Db, registry: &crate::courses::Courses, req: &Request, brief: &str) -> Result<(), String> {
    let system = if req.instructions.trim().is_empty() { DEFAULT_INSTRUCTIONS } else { req.instructions.as_str() };
    let course_id = req.course_id.clone();
    let job_id = job.snapshot().id;
    job.update(|s| s.phase = "Reading the course".into());
    let cid = course_id.clone();
    let (files, row, notes) = db
        .with(move |c| Ok((courses::load_files(c, &cid)?, courses::course(c, &cid)?, courses::notes_of(c, &cid)?)))
        .map_err(|e| format!("database: {e}"))?;
    let row = row.ok_or("the course is gone")?;
    let (store, lessons) = crate::arrange::lessons_of(&files)?;
    if lessons.len() >= MAX_LESSONS {
        return Err(format!("The course has {MAX_LESSONS} lessons, the most a course may have; remove one first. The course is unchanged."));
    }
    let manifest = files.iter().find(|f| f.path == MANIFEST_FILE).ok_or("the course has no course.json")?;
    let focus: HashMap<String, serde_json::Value> = row
        .outline
        .as_deref()
        .and_then(|o| serde_json::from_str::<serde_json::Value>(o).ok())
        .and_then(|o| o.get("lessons").and_then(|l| l.as_array()).cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|e| Some((e.get("id")?.as_str()?.to_string(), e.get("focus")?.clone())))
        .collect();
    let current: Vec<serde_json::Value> = lessons.iter().map(|l| outline_entry(&l.value, focus.get(&l.id))).collect();
    let title = store.title().to_string();
    job.update(|s| {
        s.course_title = Some(title.clone());
        s.lessons_total = 1;
        s.phase = "Writing the new lesson".into();
    });
    let asked = if row.brief.trim().is_empty() { String::new() } else { format!(" They first asked for: \"{}\".", row.brief.trim()) };
    let kind = SubjectKind::of_course(row.outline.as_deref(), &manifest.content);
    let prompt = format!(
        "A learner is studying the course \"{title}\".{asked}\n\nIts course.json manifest:\n{}\n\nIts lessons, in order:\n{}\n\n\
         They want one more lesson. What it should be about, between the <brief> tags:\n<brief>\n{}\n</brief>\n\n\
         Write that lesson and decide where it goes in the course. Reply with one JSON object \
         {{\"after\": <the id of the lesson it comes right after, or null to make it the first lesson>, \"lesson\": <the complete lesson JSON>}}.\n\
         The lesson needs an id and a slug (lowercase a-z, 0-9 and '-') that no lesson above uses, a `unit` from the manifest's units \
         (the one where it fits) and a `group`. Its prerequisites may only name lessons above; prefix its question ids with its id. \
         Do not repeat what the other lessons already teach.{}{}{}{}\n\n\
         ----- Course format reference -----\n\n{FORMAT_DOC}",
        manifest.content.trim(),
        serde_json::to_string_pretty(&current).unwrap_or_default(),
        brief.trim(),
        prompt::lesson_line(kind),
        req.depth.lesson_line(),
        material_block(&notes),
        kind.rules(),
    );
    job.log(format!("Asking {} ({}) for a new lesson…", provider_name(req.provider.kind), req.provider.model));
    // Where a canned reply can put its lesson without knowing the course.
    let last = lessons.last();
    let first_unit = store.manifest().units.first().map(|u| u.id.clone()).unwrap_or_default();
    let call = Call::new("add-lesson")
        .course(&course_id)
        .job(&job_id)
        .var("last_lesson", last.map(|l| l.id.as_str()).unwrap_or(""))
        .var("last_unit", last.map(|l| l.unit()).filter(|u| !u.is_empty()).unwrap_or(&first_unit))
        .var("last_group", last.and_then(|l| l.value.get("group")).and_then(|g| g.as_str()).unwrap_or("core"));
    let mut attempt = 0;
    let mut retry = String::new();
    let (final_files, lesson_title) = loop {
        attempt += 1;
        let text = job.llm(req, &call, system, &format!("{prompt}{retry}"))?;
        let placed = parse_json(&text).and_then(|reply| {
            // A bare lesson (no wrapper) goes first.
            let (after, lesson) = match reply.get("lesson") {
                Some(l) => (reply.get("after").cloned().unwrap_or(serde_json::Value::Null), l.clone()),
                None if reply.get("content").is_some() => (serde_json::Value::Null, reply.clone()),
                None => return Err("the reply has no \"lesson\" object".into()),
            };
            let after = match after {
                serde_json::Value::Null => None,
                serde_json::Value::String(s) if s.trim().is_empty() => None,
                serde_json::Value::String(s) => Some(s.trim().to_string()),
                _ => return Err("\"after\" must be a lesson id or null".into()),
            };
            let id = lesson.get("id").and_then(|i| i.as_str()).unwrap_or_default();
            if !crate::content::is_slug(id) {
                return Err(format!("the lesson id '{id}' must be lowercase a-z, 0-9 and '-'"));
            }
            let lesson_title = lesson.get("title").and_then(|t| t.as_str()).unwrap_or(id).to_string();
            let file = lesson_file(&lesson, &serde_json::json!({}))?;
            let files = crate::arrange::place(&files, file, after.as_deref())?;
            check_file_caps(&files)?;
            Ok((files, lesson_title))
        });
        match placed {
            Ok(p) => break p,
            Err(e) if attempt < 3 => {
                job.log(format!("The new lesson could not be used ({e}); asking for a fix."));
                retry = format!("\n\nYour previous reply could not be used: {e}. Fix it and reply with the corrected JSON object only.");
            }
            Err(e) => return Err(format!("the new lesson could not be used after {attempt} attempts: {e}. The course is unchanged.")),
        }
    };
    // "Only add": whatever the reply said, nothing already there may change.
    crate::arrange::only_adds(&files, &final_files).map_err(|e| format!("the new lesson was refused because {e}. The course is unchanged."))?;
    job.update(|s| s.lessons_done = 1);
    job.log(format!("Lesson written: {lesson_title}"));
    let outline = crate::arrange::outline_for(row.outline.as_deref(), &final_files);
    let (cid, t, label) = (course_id.clone(), title.clone(), courses::quoted("Added with AI:", &lesson_title));
    let err = db
        .with(move |c| {
            courses::commit_revision(c, &cid, &final_files, &t, outline.as_deref(), &label)?;
            courses::revalidate(c, registry, &cid)
        })
        .map_err(|e| format!("database: {e}"))?;
    if let Some(e) = err {
        return Err(format!("with the new lesson the course does not load: {e}. Undo it from the course page."));
    }
    job.log("Lesson added. Undo takes it out again.");
    Ok(())
}

/// An outline may plan at most `MAX_LESSONS` lessons, as the form asks for
/// (the model, or whatever answers at a custom base URL, may list more).
/// Translate a copied course in place (#630): the manifest, then each
/// lesson, one request a file, skipping the files `plan.done` lists (a
/// resumed job). Each file is validated with the rest of the course before
/// it is saved, and marked done in the copy's notes as it is; a stop keeps
/// what was translated.
fn translate(job: &Job, db: &Db, registry: &crate::courses::Courses, req: &Request, plan: &crate::translate::Plan) -> Result<(), String> {
    use crate::translate as tr;
    let course_id = req.course_id.clone();
    let job_id = job.snapshot().id;
    let cid = course_id.clone();
    let mut files = db.with(move |c| courses::load_files(c, &cid)).map_err(|e| format!("database: {e}"))?;
    let manifest = files.iter().find(|f| f.path == MANIFEST_FILE).ok_or("the course has no course.json")?;
    let title = serde_json::from_str::<serde_json::Value>(&manifest.content)
        .ok()
        .and_then(|m| m.get("title").and_then(|t| t.as_str()).map(str::to_string))
        .unwrap_or_default();
    let mut todo: Vec<String> = files
        .iter()
        .map(|f| f.path.clone())
        .filter(|p| p == MANIFEST_FILE || crate::arrange::is_lesson_path(p))
        .collect();
    // The manifest first: it says which language the learner reads.
    todo.sort_by_key(|p| p != MANIFEST_FILE);
    let total = todo.len();
    let mut done = todo.iter().filter(|p| plan.done.contains(p)).count();
    job.update(|s| {
        s.course_title = Some(title.clone());
        s.lessons_total = total;
        s.lessons_done = done;
        s.phase = crate::t("courses.translate.phase_start");
    });
    let mut saved = plan.clone();
    let system = tr::system(plan);
    for path in todo.iter().filter(|p| !plan.done.contains(p)) {
        let Some(at) = files.iter().position(|f| &f.path == path) else { continue };
        let original: serde_json::Value = files.get(at).map(|f| serde_json::from_str(&f.content)).transpose().map_err(|e| format!("{path}: {e}"))?.unwrap_or_default();
        let phase = crate::i18n::fill(&crate::t("courses.translate.phase_file"), &[("file", path), ("n", &(done + 1)), ("total", &total)]);
        job.update(|s| s.phase = phase);
        let texts = tr::texts(&original, plan.language_course);
        let call = Call::new("translate-course").course(&course_id).job(&job_id).key(path);
        let mut feedback = String::new();
        let mut attempt = 0;
        let file = loop {
            attempt += 1;
            let mut value = original.clone();
            if !texts.is_empty() {
                let text = job.llm(req, &call, &system, &tr::prompt(&title, path, &texts, &feedback))?;
                let map = match parse_json(&text).and_then(|v| tr::translations(&v, &texts)) {
                    Ok(m) => m,
                    Err(e) if attempt < 3 => {
                        job.log(format!("{path}: the translation came back malformed ({e}); asking again."));
                        feedback = format!("\n\nYour previous reply could not be used: {e}. Reply with the JSON object of numbered translations only.");
                        continue;
                    }
                    Err(e) => return Err(format!("{path}: {e}")),
                };
                job.log(format!("{path}: {} of {} texts translated.", map.len(), texts.len()));
                tr::apply(&mut value, plan.language_course, &map);
            }
            if path == MANIFEST_FILE {
                tr::fix_manifest(&mut value, &course_id, &title, plan);
            } else {
                tr::fix_sections(&original, &mut value);
            }
            let content = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())? + "\n";
            let candidate = CourseFile { path: path.clone(), content };
            // Validated with the rest of the course, as the editor would.
            let mut trial = files.clone();
            if let Some(slot) = trial.get_mut(at) {
                *slot = candidate.clone();
            }
            match check_file_caps(&trial).and_then(|()| ContentStore::from_files(&trial).map(drop).map_err(|e| e.to_string())) {
                Ok(()) => break candidate,
                Err(e) if attempt < 3 && !texts.is_empty() => {
                    job.log(format!("{path}: the translation did not validate ({e}); asking for a fix."));
                    feedback = format!("\n\nWith your previous translations the file was rejected by the validator: {e}. Translate the texts again so that it loads.");
                }
                Err(e) => return Err(format!("{path}: the translation did not validate: {e}")),
            }
        };
        saved.done.push(path.clone());
        let (cid, p, content, notes) = (course_id.clone(), path.clone(), file.content.clone(), saved.to_json());
        db.with(move |c| {
            courses::write_file(c, &cid, &p, &content)?;
            courses::set_notes(c, &cid, &notes)
        })
        .map_err(|e| format!("database: {e}"))?;
        if let Some(slot) = files.get_mut(at) {
            *slot = file;
        }
        done += 1;
        job.update(|s| s.lessons_done = done);
    }
    let cid = course_id.clone();
    let err = db.with(move |c| courses::revalidate(c, registry, &cid)).map_err(|e| format!("database: {e}"))?;
    if let Some(e) = err {
        return Err(format!("the translated course does not load: {e}"));
    }
    job.log("Translation complete; the course is ready to open.");
    Ok(())
}

fn check_outline_length(n: usize) -> Result<(), String> {
    if n > MAX_LESSONS {
        return Err(format!("the outline has {n} lessons; a course may have at most {MAX_LESSONS}"));
    }
    Ok(())
}

/// The caps the editor puts on a course's files, for what the AI writes:
/// each file at most `courses::MAX_FILE_BYTES`, at most `courses::MAX_FILES`
/// files. `Err` reads as validator feedback, so the model can try again.
fn check_file_caps(files: &[CourseFile]) -> Result<(), String> {
    if files.len() > courses::MAX_FILES {
        return Err(format!("a course may have at most {} files", courses::MAX_FILES));
    }
    if let Some(f) = files.iter().find(|f| f.content.len() > courses::MAX_FILE_BYTES) {
        return Err(format!("{} is larger than {} KB; write it shorter", f.path, courses::MAX_FILE_BYTES / 1024));
    }
    Ok(())
}

/// Turn a parsed lesson into a file under `lessons/`, forcing the outline's
/// identity fields so the model cannot drift.
fn lesson_file(v: &serde_json::Value, entry: &serde_json::Value) -> Result<CourseFile, String> {
    let mut v = v.clone();
    let obj = v.as_object_mut().ok_or("the lesson is not a JSON object")?;
    for key in ["id", "slug", "title", "unit", "group", "prerequisites"] {
        if let Some(val) = entry.get(key) {
            if !val.is_null() {
                obj.insert(key.to_string(), val.clone());
            }
        }
    }
    let slug = obj.get("slug").and_then(|s| s.as_str()).ok_or("the lesson has no slug")?.to_string();
    if !crate::content::is_slug(&slug) {
        return Err(format!("slug '{slug}' must be lowercase a-z, 0-9 and '-'"));
    }
    let content = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())? + "\n";
    Ok(CourseFile { path: format!("lessons/{slug}.json"), content })
}

/// Pull the JSON object out of a reply (tolerates code fences and chatter).
pub fn parse_json(text: &str) -> Result<serde_json::Value, String> {
    let trimmed = text.trim();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
        return Ok(v);
    }
    let start = trimmed.find('{').ok_or("the reply contains no JSON object")?;
    let end = trimmed.rfind('}').ok_or("the reply contains no JSON object")?;
    if end <= start {
        return Err("the reply contains no JSON object".into());
    }
    serde_json::from_str(&trimmed[start..=end]).map_err(|e| format!("the reply is not valid JSON: {e}"))
}

/// No redirects: the providers never send one, and following one would let a
/// public base URL bounce the request (and the key) to an internal address.
/// Error statuses come back as responses, so `send` can read the provider's
/// own message (a quota, a bad model name) instead of only the code.
fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(timeout)).max_redirects(0).http_status_as_error(false).build().into()
}

// ---------- custom base URLs (SSRF) ----------

/// May a learner point the AI at a server on this machine or its network?
/// Only in dev mode or when the operator says so
/// (`SKRONGLE_ALLOW_PRIVATE_AI_URLS=1`, for a self-hosted Ollama on the LAN);
/// otherwise anyone with an account could make the server call its own
/// loopback, the LAN or a cloud metadata address and read the reply. The
/// same switch allows ports other than 443 and 80.
pub fn private_urls_allowed(dev: bool) -> bool {
    dev || crate::env("ALLOW_PRIVATE_AI_URLS").is_some_and(|v| v == "1" || v == "true")
}

/// Scheme, host and port of an `http(s)://` URL. `None` when it is not one,
/// or carries credentials (`user@host`).
pub fn url_host_port(url: &str) -> Option<(String, u16)> {
    let (scheme, rest) = url.split_once("://")?;
    let default_port = match scheme.to_ascii_lowercase().as_str() {
        "http" => 80,
        "https" => 443,
        _ => return None,
    };
    let authority = rest.split(['/', '?', '#']).next()?;
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
        let (h, after) = v6.split_once(']')?;
        let port = match after.strip_prefix(':') {
            Some(p) => p.parse().ok()?,
            None if after.is_empty() => default_port,
            None => return None,
        };
        (h.to_string(), port)
    } else {
        match authority.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), p.parse().ok()?),
            None => (authority.to_string(), default_port),
        }
    };
    (!host.is_empty()).then_some((host.to_ascii_lowercase(), port))
}

/// Addresses no learner-supplied URL may reach: loopback, private,
/// link-local (cloud metadata), carrier-grade NAT, unspecified, multicast,
/// reserved, and IPv6 forms that embed one of those.
pub fn is_internal_ip(ip: std::net::IpAddr) -> bool {
    use std::net::IpAddr;
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                || v4.is_documentation()
                || o[0] == 0
                || (o[0] == 100 && (64..128).contains(&o[1])) // 100.64.0.0/10
                || (o[0] == 192 && o[1] == 0 && o[2] == 0) // 192.0.0.0/24
                || (o[0] == 198 && (o[1] == 18 || o[1] == 19)) // 198.18.0.0/15
                || o[0] >= 240 // 240.0.0.0/4
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_internal_ip(IpAddr::V4(v4));
            }
            let s = v6.segments();
            // NAT64 (64:ff9b::/96) carries an IPv4 address in its last 32 bits.
            if s[0] == 0x64 && s[1] == 0xff9b && s[2..6] == [0, 0, 0, 0] {
                let v4 = std::net::Ipv4Addr::new((s[6] >> 8) as u8, s[6] as u8, (s[7] >> 8) as u8, s[7] as u8);
                return is_internal_ip(IpAddr::V4(v4));
            }
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00 // unique local fc00::/7
                || (s[0] & 0xffc0) == 0xfe80 // link-local fe80::/10
                || (s[0] & 0xffc0) == 0xfec0 // site-local fec0::/10
                || s[..6] == [0, 0, 0, 0, 0, 0] // IPv4-compatible ::a.b.c.d
        }
    }
}

/// Check a learner's custom base URL before any request goes to it: an
/// `http(s)://` URL on port 443 or 80 whose host resolves only to public
/// addresses (unless `allow_private`, which also allows any port, e.g.
/// Ollama's 11434). `Err` is a sentence for the form.
///
/// Known gap: the host is resolved again when the request is made, so a DNS
/// server that answers differently the second time (rebinding) can still
/// reach an internal address.
pub async fn check_base_url(url: &str, allow_private: bool) -> Result<(), String> {
    let Some((host, port)) = url_host_port(url) else {
        return Err("The base URL must be an http:// or https:// address, without a user name or password.".into());
    };
    if allow_private {
        return Ok(());
    }
    // Only the web ports: otherwise any account could have the server post
    // to any service on any public host and read the start of its reply.
    if port != 443 && port != 80 {
        return Err(format!(
            "The base URL may only use port 443 (https) or 80 (http), not {port} (the server's operator can allow others with SKRONGLE_ALLOW_PRIVATE_AI_URLS=1)."
        ));
    }
    let refuse =|| {
        Err("That base URL points at this server or a private network, which this site does not allow. Use a public address (the server's operator can allow private ones with SKRONGLE_ALLOW_PRIVATE_AI_URLS=1).".to_string())
    };
    if host == "localhost" || host.ends_with(".localhost") {
        return refuse();
    }
    let addrs: Vec<std::net::SocketAddr> = match tokio::net::lookup_host((host.as_str(), port)).await {
        Ok(a) => a.collect(),
        Err(_) => return Err(format!("The base URL's server name {host:?} could not be found.")),
    };
    if addrs.is_empty() {
        return Err(format!("The base URL's server name {host:?} could not be found."));
    }
    if addrs.iter().any(|a| is_internal_ip(a.ip())) {
        return refuse();
    }
    Ok(())
}

/// Generation: minutes of waiting per lesson. Chat: a reply or an error soon.
const GENERATE_TIMEOUT: Duration = Duration::from_secs(900);
const CHAT_TIMEOUT: Duration = Duration::from_secs(120);
/// A conversational turn: the whole transcript goes in, the next assistant
/// message comes out. Blocking; callers put it on its own thread.
pub fn chat(p: &Provider, call: &Call, system: &str, messages: &[ChatMessage], max_tokens: u32) -> Result<Reply, String> {
    complete(p, call, system, messages, max_tokens, CHAT_TIMEOUT)
}

/// `SKRONGLE_AI_TIMEOUT_SECS`: how long one model request may take, in place
/// of the defaults above (15 minutes for generation, 2 for chat).
fn request_timeout(default: Duration) -> Duration {
    crate::env("AI_TIMEOUT_SECS").and_then(|v| v.parse::<u64>().ok()).filter(|s| *s > 0).map(Duration::from_secs).unwrap_or(default)
}

/// The one place every model call goes through.
fn complete(p: &Provider, call: &Call, system: &str, messages: &[ChatMessage], max_tokens: u32, timeout: Duration) -> Result<Reply, String> {
    let messages = merge_roles(messages);
    let timeout = request_timeout(timeout);
    let result = match p.kind {
        ProviderKind::Anthropic => call_anthropic(p, call, system, &messages, max_tokens, timeout),
        _ => call_openai(p, call, system, &messages, max_tokens, timeout),
    };
    // Errors are logged and shown: never with the key in them, even when a
    // server echoes it back (a proxy's error page, a reply that is not JSON).
    let (text, usage) = result.map_err(|e| redact_key(&e, &p.api_key))?;
    let usage = usage.filter(|u| !u.is_empty()).unwrap_or_else(|| Usage::estimate(system, &messages, &text));
    Ok(Reply { text, usage })
}

/// `text` with every occurrence of `key` replaced.
pub fn redact_key(text: &str, key: &str) -> String {
    let key = key.trim();
    if key.len() < 8 {
        return text.to_string();
    }
    text.replace(key, "[your key]")
}

/// Both wire formats want strictly alternating roles starting with the
/// user; consecutive messages of one role are joined with a blank line.
fn merge_roles(messages: &[ChatMessage]) -> Vec<ChatMessage> {
    let mut out: Vec<ChatMessage> = Vec::with_capacity(messages.len());
    for m in messages {
        match out.last_mut() {
            Some(last) if last.role == m.role => {
                last.content.push_str("\n\n");
                last.content.push_str(&m.content);
            }
            _ => out.push(m.clone()),
        }
    }
    if out.first().is_some_and(|m| m.role != "user") {
        out.insert(0, ChatMessage { role: "user", content: "(session start)".into() });
    }
    out
}

/// Models that take the server-side refusal fallback parameter.
fn supports_fallbacks(model: &str) -> bool {
    ["claude-opus-5-5", "claude-opus-5", "claude-fable-5-1", "claude-sonnet-5-5"].contains(&model)
}

fn anthropic_body(p: &Provider, system: &str, messages: &[ChatMessage], max_tokens: u32) -> serde_json::Value {
    let mut body = serde_json::json!({
        "model": p.model,
        "max_tokens": max_tokens,
        "system": system,
        "messages": messages.iter().map(|m| serde_json::json!({ "role": m.role, "content": m.content })).collect::<Vec<_>>(),
    });
    if supports_fallbacks(&p.model) {
        // A safety-classifier decline on these models re-runs the request on a
        // fallback model inside the same call instead of returning nothing.
        body["fallbacks"] = serde_json::Value::String("default".into());
    }
    budget::anthropic(&mut body, p);
    body
}

/// The `x-skrongle-call` request header: which call site this is (`purpose`),
/// the lesson it is about (`key`), the job and the facts a canned reply may
/// name (`var.<name>`), URL-encoded. Every request carries it, so a server
/// that stands in for a provider (dev mode's simulator) can answer the way
/// the fixtures do; the providers ignore headers they do not know. Nothing
/// in it is more than the prompt already says.
pub const CALL_HEADER: &str = "x-skrongle-call";

pub fn call_header(call: &Call) -> String {
    let enc = crate::urlenc::urlencode;
    let mut out = format!("purpose={}", enc(call.purpose));
    if !call.key.is_empty() {
        out.push_str(&format!("&key={}", enc(&call.key)));
    }
    if !call.job_id.is_empty() {
        out.push_str(&format!("&job={}", enc(&call.job_id)));
    }
    for (name, value) in &call.vars {
        out.push_str(&format!("&var.{}={}", enc(name), enc(value)));
    }
    out
}

fn call_anthropic(p: &Provider, call: &Call, system: &str, messages: &[ChatMessage], max_tokens: u32, timeout: Duration) -> Result<(String, Option<Usage>), String> {
    let url = format!("{}/v1/messages", p.base_url.trim_end_matches('/'));
    let body = anthropic_body(p, system, messages, max_tokens);
    let mut req = agent(timeout)
        .post(&url)
        .header("x-api-key", &p.api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .header(CALL_HEADER, &call_header(call));
    if supports_fallbacks(&p.model) {
        req = req.header("anthropic-beta", "server-side-fallback-2026-07-01");
    }
    let json = send(req, &body, timeout)?;
    if json.get("stop_reason").and_then(|s| s.as_str()) == Some("refusal") {
        let why = json.get("stop_details").and_then(|d| d.get("explanation")).and_then(|e| e.as_str()).unwrap_or("");
        return Err(format!("the model declined the request. {why}").trim().to_string());
    }
    let text: String = json
        .get("content")
        .and_then(|c| c.as_array())
        .map(|blocks| {
            blocks
                .iter()
                .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
                .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default();
    if text.trim().is_empty() {
        return Err("the model returned no text".into());
    }
    Ok((text, Some(anthropic_usage(&json))))
}

/// `usage` of a Messages API reply. A cache write costs 1.25 times an input
/// token (5-minute cache), so it counts as that many input tokens, rounded
/// up: the meter never charges less than Anthropic bills.
fn anthropic_usage(json: &serde_json::Value) -> Usage {
    let n = |k: &str| json.pointer(&format!("/usage/{k}")).and_then(|v| v.as_u64()).unwrap_or(0);
    let writes = (n("cache_creation_input_tokens") * 5).div_ceil(4);
    Usage { input: n("input_tokens") + writes, output: n("output_tokens"), cached: n("cache_read_input_tokens") }
}

fn send(req: ureq::RequestBuilder<ureq::typestate::WithBody>, body: &serde_json::Value, timeout: Duration) -> Result<serde_json::Value, String> {
    let timed_out = || format!("request failed: timed out after {} s", timeout.as_secs());
    let mut resp = match req.send_json(body) {
        Ok(r) => r,
        Err(ureq::Error::StatusCode(code)) => return Err(format!("the API answered HTTP {code} (check the key, model name and base URL)")),
        Err(ureq::Error::Timeout(_)) => return Err(timed_out()),
        Err(e) => return Err(format!("request failed: {e}")),
    };
    let status = resp.status().as_u16();
    let text = match resp.body_mut().with_config().limit(MAX_REPLY_BYTES).read_to_string() {
        Ok(t) => t,
        Err(ureq::Error::Timeout(_)) => return Err(timed_out()),
        Err(ureq::Error::BodyExceedsLimit(_)) => return Err(format!("the reply is larger than {} KB, more than a course file may be", MAX_REPLY_BYTES / 1024)),
        Err(e) => return Err(format!("could not read the reply: {e}")),
    };
    let json: serde_json::Value = serde_json::from_str(&text).map_err(|_| format!("HTTP {status}: reply is not JSON: {}", text.chars().take(200).collect::<String>()))?;
    if status >= 300 {
        let msg = json
            .pointer("/error/message")
            .and_then(|m| m.as_str())
            .unwrap_or("")
            .to_string();
        return Err(format!("the API answered HTTP {status}. {msg}").trim().to_string());
    }
    Ok(json)
}

/// The failures a learner can do something about.
#[derive(Clone, Copy)]
enum Failure {
    Key,
    /// The account's credit or usage limit is used up (a 400 or 429 that says so).
    Quota,
    Busy,
    Unusable,
    TurnedDown,
    ProviderError,
    Unreachable,
}

/// What the providers say when an account's money or quota is used up:
/// Anthropic's 400 "credit balance is too low" and "reached your specified
/// API usage limits", OpenAI's 429 `insufficient_quota` ("exceeded your
/// current quota"), Google's 429 "exceeded your current quota".
fn quota_exhausted(raw: &str) -> bool {
    let lower = raw.to_ascii_lowercase();
    ["credit balance", "usage limit", "insufficient_quota", "exceeded your current quota", "quota exceeded", "billing"].iter().any(|w| lower.contains(w))
}

fn failure_kind(raw: &str) -> Option<Failure> {
    let http = raw.split("HTTP ").nth(1).and_then(|r| r.get(..3)).and_then(|c| c.parse::<u16>().ok());
    if matches!(http, Some(400 | 402 | 403 | 429)) && quota_exhausted(raw) {
        return Some(Failure::Quota);
    }
    Some(match http {
        Some(401 | 403) => Failure::Key,
        Some(429 | 529) | Some(500..=599) => Failure::Busy,
        Some(200..=299) => Failure::Unusable,
        Some(400 | 404) => Failure::TurnedDown,
        Some(_) => Failure::ProviderError,
        None if raw.starts_with("request failed") => Failure::Unreachable,
        None if ["JSON", "no text", "did not validate", "does not load", "no lessons", "slug '", "reply"].iter().any(|w| raw.contains(w)) => Failure::Unusable,
        None => return None,
    })
}

/// What a learner reads when a generation stops: a plain sentence and a next
/// step for the failures we know (the key, a busy provider, an unusable
/// reply). `None` when `raw` is already written for a learner (a cancel, a
/// restart); pages show the raw text underneath as the detail.
pub fn plain_failure(raw: &str) -> Option<&'static str> {
    Some(match failure_kind(raw)? {
        Failure::Key => "Writing your course stopped. The AI provider didn't accept the server's API key. The owner can check it on Manage Users; then try again.",
        Failure::Quota => "Writing your course stopped. Your AI provider account has run out of credit or reached its usage limit. Check your plan and billing with the provider, then try again.",
        Failure::Busy => "Writing your course stopped. The AI provider is busy right now. Wait a minute, then try again.",
        Failure::Unusable => "Writing your course stopped. The AI's reply couldn't be used to build the course. Try again.",
        Failure::TurnedDown => "Writing your course stopped. The AI provider turned the request down. Try again; if it stops again, the owner can check the key on Manage Users.",
        Failure::ProviderError => "Writing your course stopped. The AI provider answered with an error. Try again.",
        Failure::Unreachable => "Writing your course stopped. The AI provider didn't answer in time or couldn't be reached. Try again in a minute.",
    })
}

/// `plain_failure` for an "Add content with AI" job (a lesson added, or the
/// course revised), which has no Resume: it changed nothing, so try again.
pub fn plain_revision_failure(raw: &str) -> Option<&'static str> {
    Some(match failure_kind(raw)? {
        Failure::Key => "The AI provider didn't accept the server's API key. The owner can check it on Manage Users; then try again. The course is unchanged.",
        Failure::Quota => "Your AI provider account has run out of credit or reached its usage limit. Check your plan and billing with the provider, then try again. The course is unchanged.",
        Failure::Busy => "The AI provider is busy right now. Wait a minute, then try again. The course is unchanged.",
        Failure::Unusable => "The AI's reply couldn't be used. Try again, or say it differently. The course is unchanged.",
        Failure::TurnedDown => "The AI provider turned the request down. Try again; if it stops again, the owner can check the key on Manage Users. The course is unchanged.",
        Failure::ProviderError => "The AI provider answered with an error. Try again. The course is unchanged.",
        Failure::Unreachable => "The AI provider didn't answer in time or couldn't be reached. Try again in a minute. The course is unchanged.",
    })
}

#[cfg(test)]
mod tests;
