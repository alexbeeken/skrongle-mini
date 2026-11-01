//! A fact check through Use AI (#1238). The plan's fourth action, `check`,
//! sends a lesson back to be read for errors: it comes back unchanged
//! unless a fact is wrong. A `check` or `rewrite` lesson request also
//! returns `why` lines (one per fix) and `check` lines (what the model was
//! unsure of and left alone). They are kept with the run's AI output
//! (`course_ai_runs.why`) and shown per lesson in Review AI changes.

use serde::{Deserialize, Serialize};

use super::{Change, Slot};
use crate::content::CourseFile;

/// Most lines kept of each kind per lesson, and most characters per line.
const MAX_LINES: usize = 12;
const MAX_LINE_CHARS: usize = 400;

/// What a `check` lesson request is asked to do.
pub(super) const CHECK_TASK: &str = "Read this lesson for errors of fact, and for questions whose correct answer is wrong. \
     Fix only what is wrong. Keep everything else exactly as it is, word for word.";

/// The reply a `check` lesson request asks for.
pub(super) const CHECK_REPLY: &str = "Reply with one JSON object only: {\"lesson\": <the complete corrected lesson JSON object, or null when nothing in it is wrong>, \
     \"why\": [<one line per fix: what it said, what it says now, and why>], \
     \"check\": [<one line per thing you are unsure of and left alone>]}.";

/// The reply a `rewrite` lesson request asks for.
pub(super) const REWRITE_REPLY: &str = "Reply with one JSON object only: {\"lesson\": <the complete lesson JSON object>, \
     \"why\": [<one short line per change you made, and why>], \
     \"check\": [<one line per thing you are unsure of and left alone>]}.";

/// The plan prompt's line for the `check` action.
pub(super) const PLAN_LINE: &str = "- \"check\": the lesson is read for errors and comes back unchanged unless a fact is wrong. \
     When the feedback asks for a check of the facts, use \"check\" for every lesson.\n";

/// The `why` and `check` lines of one lesson of a run.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LessonNote {
    /// The lesson's file.
    pub path: String,
    pub title: String,
    /// One line per fix or change.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub why: Vec<String>,
    /// What the model was unsure of and left alone.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub check: Vec<String>,
    /// The lesson came back different from what it was.
    #[serde(default)]
    pub changed: bool,
}

/// The notes of a run, as kept on it; none when empty or unreadable.
pub fn notes_from_json(s: &str) -> Vec<LessonNote> {
    serde_json::from_str(s).unwrap_or_default()
}

pub fn notes_to_json(notes: &[LessonNote]) -> String {
    if notes.is_empty() {
        String::new()
    } else {
        serde_json::to_string(notes).unwrap_or_default()
    }
}

/// A lesson reply, read: the lesson (`None`: a check found nothing wrong),
/// its why lines and its check lines. A bare lesson object (the reply of a
/// new lesson, or a model that ignored the wrapper) has no lines.
pub(super) fn read_reply(v: &serde_json::Value) -> (Option<serde_json::Value>, Vec<String>, Vec<String>) {
    let wrapped = v.get("lesson").is_some_and(|l| l.is_object() || l.is_null()) && v.get("content").is_none() && v.get("id").is_none();
    if !wrapped {
        return (Some(v.clone()), vec![], vec![]);
    }
    let lesson = v.get("lesson").filter(|l| l.is_object()).cloned();
    (lesson, lines(v.get("why")), lines(v.get("check")))
}

/// Strings of a JSON array, trimmed, without a leading "Check:", capped.
fn lines(v: Option<&serde_json::Value>) -> Vec<String> {
    let Some(items) = v.and_then(|v| v.as_array()) else { return vec![] };
    items
        .iter()
        .filter_map(|l| l.as_str())
        .map(|l| {
            let l = l.trim();
            let l = l.strip_prefix("Check:").map(str::trim_start).unwrap_or(l);
            l.chars().take(MAX_LINE_CHARS).collect::<String>()
        })
        .filter(|l| !l.is_empty())
        .take(MAX_LINES)
        .collect()
}

/// The checked lesson as a file: the old file byte for byte when the check
/// found nothing wrong, or when the lesson came back the same.
pub(super) fn unchanged_file(old: Option<&CourseFile>, candidate: Option<&serde_json::Value>) -> Option<CourseFile> {
    let old = old?;
    match candidate {
        None => Some(old.clone()),
        Some(v) => {
            let was: serde_json::Value = serde_json::from_str(&old.content).ok()?;
            (&was == v).then(|| old.clone())
        }
    }
}

/// The notes of the slots a run wrote, against the files the course had.
pub(super) fn notes_of(slots: &[Slot], files: &[CourseFile]) -> Vec<LessonNote> {
    slots
        .iter()
        .filter(|s| matches!(s.change, Change::Check | Change::Rewrite) && !(s.why.is_empty() && s.check.is_empty()))
        .filter_map(|s| {
            let done = s.done.as_ref()?;
            let before = s.old.as_ref().and_then(|(p, _)| files.iter().find(|f| &f.path == p));
            Some(LessonNote {
                path: done.path.clone(),
                title: s.entry.get("title").and_then(|t| t.as_str()).unwrap_or_default().to_string(),
                why: s.why.clone(),
                check: s.check.clone(),
                changed: before.map_or(true, |b| b.path != done.path || b.content != done.content),
            })
        })
        .collect()
}

/// A run whose planned lessons were all checks, and that changed nothing.
pub(super) const NOTHING_WRONG: &str = "The check found nothing to fix: every lesson came back as it was. The course is unchanged.";

/// Did the plan only keep and check lessons?
pub(super) fn only_checks(slots: &[Slot]) -> bool {
    slots.iter().any(|s| s.change == Change::Check) && slots.iter().all(|s| matches!(s.change, Change::Check | Change::Keep))
}
