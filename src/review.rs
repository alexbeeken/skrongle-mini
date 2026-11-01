//! "Review AI changes" (#163, #193, #189): the files the latest AI run on a
//! course changed (`courses::AiRun`), as a list that reads like the course
//! page's lesson table. Each lesson says in plain words what the AI did to
//! it, and opens to a side-by-side diff: what was there on the left, what the
//! AI wrote on the right, with JSON shown as labelled text ("Question  What
//! is ½ of 4?") rather than code. A file the AI added or removed as a whole
//! is one column.
//!
//! The learner can leave changes out (`Exclusions`): single lines or groups
//! of them, or whole files. And edit any line of the AI's file as text, in
//! place (`Edits`, #287), like in a text editor: the line the AI wrote or an
//! unchanged one, into one line or several. `apply` then writes the course
//! as the AI left it minus those parts, with those edits.
//!
//! The diff is a longest common subsequence of lines (Hirschberg's
//! algorithm, so memory stays linear in the file length on the Pi).

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::content::CourseFile;
use crate::courses::{AiRun, AiRunFile};

/// Unchanged lines kept around a change; longer runs fold into a gap row.
const CONTEXT: usize = 3;
/// Above this many line pairs a file is shown as replaced as a whole.
const MAX_CELLS: usize = 16_000_000;

/// What the learner left out of an AI run in its review.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exclusions {
    /// Whole files: they stay as they were before the run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    /// Per file, groups of changed rows (`Row::key`) left out together: one
    /// "−" click, or a drag over several rows. A group comes back as a whole.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub lines: BTreeMap<String, Vec<Vec<String>>>,
}

impl Exclusions {
    /// From the review's form: `file=<path>` per file left out, and
    /// `lines=<path>:<key>,<key>…` per group of rows (without JavaScript each
    /// ticked row is its own group).
    pub fn from_form(pairs: &[(String, String)]) -> Exclusions {
        let mut x = Exclusions::default();
        for (k, v) in pairs {
            match k.as_str() {
                "file" if !v.is_empty() && !x.files.contains(v) => x.files.push(v.clone()),
                "lines" => {
                    let Some((path, keys)) = v.rsplit_once(':') else { continue };
                    let keys: Vec<String> = keys.split(',').map(str::trim).filter(|k| is_key(k)).map(str::to_string).collect();
                    if !path.is_empty() && !keys.is_empty() {
                        x.lines.entry(path.to_string()).or_default().push(keys);
                    }
                }
                _ => {}
            }
        }
        x.tidy();
        x
    }

    /// As kept on the run; nothing left out when empty or unreadable.
    pub fn from_json(s: &str) -> Exclusions {
        serde_json::from_str(s).unwrap_or_default()
    }

    pub fn to_json(&self) -> String {
        if self.is_empty() {
            String::new()
        } else {
            serde_json::to_string(self).unwrap_or_default()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty() && self.lines.is_empty()
    }

    /// No key in two groups, no empty group.
    fn tidy(&mut self) {
        for groups in self.lines.values_mut() {
            let mut seen = HashSet::new();
            for g in groups.iter_mut() {
                g.retain(|k| seen.insert(k.clone()));
            }
            groups.retain(|g| !g.is_empty());
        }
        self.lines.retain(|_, g| !g.is_empty());
    }

    /// Only the files the run changed.
    fn known(mut self, run: &AiRun) -> Exclusions {
        let paths: HashSet<&str> = run.files.iter().map(|f| f.path.as_str()).collect();
        self.files.retain(|p| paths.contains(p.as_str()));
        self.lines.retain(|p, _| paths.contains(p.as_str()));
        self
    }

    fn file_out(&self, path: &str) -> bool {
        self.files.iter().any(|p| p == path)
    }

    fn keys(&self, path: &str) -> HashSet<&str> {
        self.lines.get(path).map(|g| g.iter().flatten().map(String::as_str).collect()).unwrap_or_default()
    }

    /// The 1-based group `key` is in, 0 for none.
    fn group(&self, path: &str, key: &str) -> usize {
        self.lines.get(path).and_then(|g| g.iter().position(|g| g.iter().any(|k| k == key))).map_or(0, |n| n + 1)
    }
}

/// The learner's own text for lines of the AI's files (#287): per file, per
/// row (`Row::key`), the line as typed in place of the one there (the AI's,
/// or an unchanged one), as JSON text; it may hold several lines. The
/// result must still parse (Accept refuses it by lesson name otherwise). An
/// edited line is always in: it is never left out, and neither is its lesson
/// as a whole (what `Exclusions` says of it is ignored).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Edits(pub BTreeMap<String, BTreeMap<String, String>>);

impl Edits {
    /// From the review's form: `edit:<path>:<key>=<text>` per edited line.
    pub fn from_form(pairs: &[(String, String)]) -> Edits {
        let mut e = Edits::default();
        for (k, v) in pairs {
            let Some((path, key)) = k.strip_prefix("edit:").and_then(|k| k.rsplit_once(':')) else { continue };
            if !path.is_empty() && is_key(key) {
                // A form sends line breaks as CR LF.
                e.0.entry(path.to_string()).or_default().insert(key.to_string(), v.replace("\r\n", "\n"));
            }
        }
        e
    }

    /// As kept on the run; none when empty or unreadable.
    pub fn from_json(s: &str) -> Edits {
        serde_json::from_str(s).unwrap_or_default()
    }

    pub fn to_json(&self) -> String {
        if self.is_empty() {
            String::new()
        } else {
            serde_json::to_string(self).unwrap_or_default()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.0.values().all(BTreeMap::is_empty)
    }

    /// Only the files the run changed.
    fn known(mut self, run: &AiRun) -> Edits {
        let paths: HashSet<&str> = run.files.iter().map(|f| f.path.as_str()).collect();
        self.0.retain(|p, e| paths.contains(p.as_str()) && !e.is_empty());
        self
    }

    fn get(&self, path: &str, key: &str) -> Option<&str> {
        self.0.get(path).and_then(|e| e.get(key)).map(String::as_str)
    }
}

/// `text`, typed in place of the line `raw`, as Accept writes it: its lines
/// without trailing blanks, blank ones dropped. None when nothing is left
/// (the line stays as it is) or it is the line as it was.
fn edited_lines(raw: &str, text: &str) -> Option<Vec<String>> {
    let lines: Vec<String> = text.lines().map(|l| l.trim_end().to_string()).filter(|l| !l.is_empty()).collect();
    (!lines.is_empty() && lines != [raw]).then_some(lines)
}

/// "3-4": line 3 before, line 4 after; 0 for no line on that side.
fn is_key(k: &str) -> bool {
    k.split_once('-').is_some_and(|(a, b)| !a.is_empty() && !b.is_empty() && a.bytes().chain(b.bytes()).all(|c| c.is_ascii_digit()))
}

/// Put the run's why and check lines (`AiRun::why`, #1238) on the files
/// they belong to. Returns the lessons the AI looked at and left alone that
/// have check lines, for the review's "Looked at, left alone" list.
pub fn attach_notes(files: &mut [ReviewFile], why: &str) -> Vec<crate::ai::LessonNote> {
    let mut left_alone = Vec::new();
    for n in crate::ai::notes_from_json(why) {
        match files.iter_mut().find(|f| f.path == n.path) {
            Some(f) => (f.why, f.check) = (n.why, n.check),
            None if !n.check.is_empty() => left_alone.push(n),
            None => {}
        }
    }
    left_alone
}

/// One file of the review (`course_review_modal.html`).
pub struct ReviewFile {
    pub path: String,
    /// What the course page calls the file: the lesson title (else its
    /// id, else the file name), or "Course settings" for `course.json`.
    pub title: String,
    /// "added" | "removed" | "changed".
    pub kind: &'static str,
    pub added: usize,
    pub removed: usize,
    /// The lesson's number on the course page (0: none, e.g. a removed lesson).
    pub position: usize,
    /// What the AI did, in plain words ("Rewrote the reading; added 2 quiz questions.").
    pub summary: String,
    /// Left out as a whole.
    pub excluded: bool,
    /// Lines edited in the review (#287), outside the rows left out.
    pub edited: usize,
    /// For a changed file, side-by-side rows; for an added file, one row per
    /// line on the right; for a removed one, one per line on the left.
    pub rows: Vec<Row>,
    /// What the AI said it fixed or changed, and why (#1238), shown above the diff.
    pub why: Vec<String>,
    /// What it was unsure of and left alone ("Check: …").
    pub check: Vec<String>,
}

/// A row of the review. `kind` "gap" folds `gap` unchanged lines, "same" is
/// an unchanged line, "change" a removed and/or added line, which can be left
/// out (`key`, `excluded`, and `group`: the rows left out with it).
pub struct Row {
    pub kind: &'static str,
    pub gap: usize,
    pub key: String,
    pub excluded: bool,
    pub group: usize,
    pub left: Cell,
    pub right: Cell,
}

impl Row {
    /// Brackets only on both sides: drawn as a thin row.
    pub fn is_bare(&self) -> bool {
        let bare = |c: &Cell| c.bare || c.kind == "empty";
        self.kind != "gap" && bare(&self.left) && bare(&self.right)
    }
}

/// One side of a row. `kind`: "same", "del" (left), "add" (right) or
/// "empty". A JSON line shows as `label` and its value (`pre`, `mid`, `post`:
/// `mid` is the part that differs from the line it replaces); `raw` is the
/// line as it is in the file, and `bare` marks a line of brackets only.
///
/// A line of the AI's file (the AI's or an unchanged one, on the right) can
/// be edited as text (#287): `editable`, with `edit_name` its form field
/// (`edit:<path>:<key>`) and `ai` the line as the AI's file has it. `edited`:
/// the learner's text is shown instead, read like any line (`raw` is then
/// the edited text, and `more` its lines after the first, each read alike).
#[derive(Default)]
pub struct Cell {
    pub kind: &'static str,
    pub label: String,
    pub pre: String,
    pub mid: String,
    pub post: String,
    pub raw: String,
    pub indent: usize,
    pub bare: bool,
    pub editable: bool,
    pub edit_name: String,
    pub ai: String,
    pub more: Vec<Cell>,
    pub edited: bool,
    /// An id-only line ("Id", "Web address", "Group"…): dimmed (#282).
    pub quiet: bool,
}

impl Cell {
    /// Make a line of the AI's file editable, showing `text` (the learner's
    /// lines) in its place when it changes it.
    fn editable(&mut self, name: String, text: Option<&str>) {
        self.editable = true;
        self.edit_name = name;
        self.ai = self.raw.clone();
        let Some(lines) = text.and_then(|t| edited_lines(&self.raw, t)) else { return };
        let mut cells = lines.iter().map(|l| readable(l, true));
        let first = cells.next().unwrap_or_default();
        let more = cells.collect();
        let (kind, edit_name, ai) = (self.kind, std::mem::take(&mut self.edit_name), std::mem::take(&mut self.ai));
        *self = Cell { kind, editable: true, edit_name, ai, edited: true, raw: lines.join("\n"), more, ..first };
    }
}

/// The review of `run` with `excluded` ticked. `positions`: each lesson
/// file's number in the course as it is now; files are listed in that order
/// (course settings first, removed lessons last).
pub fn files(run: &AiRun, excluded: &Exclusions, edits: &Edits, positions: &HashMap<String, usize>) -> Vec<ReviewFile> {
    let mut out: Vec<ReviewFile> = run.files.iter().map(|f| file(f, excluded, edits, positions.get(&f.path).copied().unwrap_or(0))).collect();
    let rank = |f: &ReviewFile| (f.path != crate::content::MANIFEST_FILE, f.position == 0, f.position);
    out.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| a.path.cmp(&b.path)));
    out
}

/// The review of an undone run (#281): nothing to leave out, nothing to edit.
pub fn read_only(files: &mut [ReviewFile]) {
    for f in files {
        f.excluded = false;
        f.edited = 0;
        for r in &mut f.rows {
            r.excluded = false;
            r.group = 0;
            r.right.editable = false;
        }
    }
}

/// "The AI added 1 lesson and changed 2." for the top of the review.
pub fn overview(files: &[ReviewFile]) -> String {
    let count = |kind: &str| files.iter().filter(|f| f.kind == kind && f.path != crate::content::MANIFEST_FILE).count();
    let mut parts = Vec::new();
    for (kind, verb) in [("added", "courses.review.verb_added"), ("changed", "courses.review.verb_changed"), ("removed", "courses.review.verb_removed")] {
        let n = count(kind);
        if n > 0 {
            let what = if parts.is_empty() { crate::tn("courses.review.lessons", n) } else { n.to_string() };
            parts.push(crate::t1(verb, "what", what));
        }
    }
    if files.iter().any(|f| f.path == crate::content::MANIFEST_FILE) {
        parts.push(crate::t("courses.review.changed_settings"));
    }
    if parts.is_empty() {
        return crate::t("courses.review.nothing");
    }
    crate::t1("courses.review.overview", "parts", join(&parts))
}

/// "2 changes and 1 lesson left out, 1 line edited", or "" for nothing.
pub fn left_out(files: &[ReviewFile]) -> String {
    let whole = files.iter().filter(|f| f.excluded).count();
    let groups: usize = files.iter().filter(|f| !f.excluded).map(|f| f.rows.iter().filter(|r| r.excluded).map(|r| r.group).collect::<HashSet<_>>().len()).sum();
    let mut parts = Vec::new();
    if groups > 0 {
        parts.push(crate::tn("courses.review.changes", groups));
    }
    if whole > 0 {
        parts.push(crate::tn("courses.review.lessons", whole));
    }
    let mut out = if parts.is_empty() { String::new() } else { crate::t1("courses.review.left_out", "parts", join(&parts)) };
    let edited: usize = files.iter().filter(|f| !f.excluded).map(|f| f.edited).sum();
    if edited > 0 {
        if !out.is_empty() {
            out.push_str(&crate::t("courses.review.sep"));
        }
        out.push_str(&crate::tn("courses.review.lines_edited", edited));
    }
    out
}

fn file(f: &AiRunFile, excluded: &Exclusions, edits: &Edits, position: usize) -> ReviewFile {
    let (before, after) = normalized(f);
    let title = title_of(&f.path, after.as_deref().or(before.as_deref()).unwrap_or(""));
    let json = f.path.ends_with(".json");
    let summary = summary(&f.path, before.as_deref(), after.as_deref());
    let (a, b) = (split(before.as_deref().unwrap_or("")), split(after.as_deref().unwrap_or("")));
    let ops = diff(&a, &b);
    let kind = match (&f.before, &f.after) {
        (None, _) => "added",
        (_, None) => "removed",
        _ => "changed",
    };
    let added = ops.iter().filter(|o| matches!(o, Op::Add)).count();
    let removed = ops.iter().filter(|o| matches!(o, Op::Del)).count();
    let mut rows = rows(&a, &b, &ops, json);
    if kind == "changed" {
        rows = fold(rows);
    }
    // A removed file comes back as a whole, never in part. Any line of a
    // JSON file on the right (the AI's, or an unchanged one) can be edited.
    let mut edited = 0;
    if kind != "removed" {
        for r in rows.iter_mut().filter(|r| r.kind != "gap") {
            if json && matches!(r.right.kind, "add" | "same") {
                r.right.editable(format!("edit:{}:{}", f.path, r.key), edits.get(&f.path, &r.key));
                edited += usize::from(r.right.edited);
            }
            if r.kind != "change" {
                continue;
            }
            // An edited line is always in (#287).
            r.group = if r.right.edited { 0 } else { excluded.group(&f.path, &r.key) };
            r.excluded = r.group > 0;
        }
    }
    let whole = excluded.file_out(&f.path) && edited == 0;
    ReviewFile { path: f.path.clone(), title, kind, added, removed, position, summary, excluded: whole, edited, rows, why: vec![], check: vec![] }
}

/// JSON on both sides is compared pretty-printed, so a file written on one
/// line still diffs line by line (the AI's files are written this way already).
fn normalized(f: &AiRunFile) -> (Option<String>, Option<String>) {
    let pretty = |s: &str| serde_json::from_str::<serde_json::Value>(s).ok().and_then(|v| serde_json::to_string_pretty(&v).ok());
    if !f.path.ends_with(".json") {
        return (f.before.clone(), f.after.clone());
    }
    let (b, a) = (f.before.as_deref().map(pretty), f.after.as_deref().map(pretty));
    // Pretty-print only when every side there parses, so both sides match.
    let parsed = |side: &Option<Option<String>>| side.as_ref().map_or(true, Option::is_some);
    if parsed(&b) && parsed(&a) {
        (b.flatten(), a.flatten())
    } else {
        (f.before.clone(), f.after.clone())
    }
}

fn title_of(path: &str, content: &str) -> String {
    if path == crate::content::MANIFEST_FILE {
        return crate::t("courses.review.course_settings");
    }
    // As the lesson table names it (`lessons_view`): the title, else the id.
    let v = serde_json::from_str::<serde_json::Value>(content).ok();
    let field = |k: &str| v.as_ref().and_then(|v| v.get(k)).and_then(|t| t.as_str()).filter(|t| !t.trim().is_empty()).map(str::to_string);
    field("title").or_else(|| field("id")).unwrap_or_else(|| {
        let name = path.rsplit('/').next().unwrap_or(path);
        name.strip_suffix(".json").unwrap_or(name).to_string()
    })
}

fn split(s: &str) -> Vec<String> {
    s.lines().map(str::to_string).collect()
}

fn join(parts: &[String]) -> String {
    match parts {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => crate::t2("courses.review.and", "a", rest.join(&crate::t("courses.review.sep")), "b", last),
    }
}

// ---------- plain words ----------

/// What a field of a lesson (or of the course settings) is called in a
/// sentence: a list's item (singular, plural), or the field as a whole.
enum Name {
    List(String, String),
    Whole(String),
}

fn name_of(key: &str) -> Option<Name> {
    let list = |k: &str| Name::List(crate::t(&format!("courses.review.{k}.one")), crate::t(&format!("courses.review.{k}.other")));
    Some(match key {
        "content" => Name::Whole(crate::t("courses.review.name_reading")),
        "title" => Name::Whole(crate::t("courses.review.name_title")),
        "focus" => Name::Whole(crate::t("courses.review.name_focus")),
        "quiz" => list("name_quiz"),
        "vocabulary" => list("name_word"),
        "conjugations" => list("name_conjugation"),
        "conversations" => list("name_conversation"),
        "listening" => list("name_listening"),
        "guide" => list("name_guide"),
        "units" => list("name_unit"),
        "sprint" => Name::Whole(crate::t("courses.review.name_sprint")),
        "themes" => Name::Whole(crate::t("courses.review.name_themes")),
        "unit" | "order" | "prerequisites" => Name::Whole(crate::t("courses.review.name_place")),
        _ => return None,
    })
}

fn count(n: usize, one: &str, many: &str) -> String {
    crate::t2("courses.review.count", "n", n, "noun", if n == 1 { one } else { many })
}

/// A field's name for a sentence, unknown ones in words ("correct_answer" → "the correct answer").
fn whole(key: &str) -> String {
    match name_of(key) {
        Some(Name::Whole(w)) => w,
        Some(Name::List(_, many)) => crate::t1("courses.review.the_list", "name", many),
        None => crate::t1("courses.review.the_field", "field", key.replace(['_', '-'], " ").to_lowercase()),
    }
}

fn sentence(parts: Vec<String>) -> String {
    let mut parts = parts;
    parts.dedup();
    let s = parts.join(&crate::t("courses.review.sentence_sep"));
    let mut c = s.chars();
    match c.next() {
        Some(f) => crate::t1("courses.review.sentence", "text", format!("{}{}", f.to_uppercase(), c.as_str())),
        None => String::new(),
    }
}

/// What the AI did to a file, in a sentence a learner can read.
fn summary(path: &str, before: Option<&str>, after: Option<&str>) -> String {
    let parse = |s: Option<&str>| s.and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok());
    let (b, a) = (parse(before), parse(after));
    match (before, after) {
        (None, Some(_)) => match a.as_ref().and_then(|a| a.as_object()) {
            Some(o) => {
                let mut parts = Vec::new();
                for (k, v) in o {
                    match (name_of(k), v) {
                        (Some(Name::List(one, many)), serde_json::Value::Array(items)) if !items.is_empty() => parts.push(count(items.len(), &one, &many)),
                        (Some(Name::Whole(_)), serde_json::Value::String(t)) if k == "content" && !t.trim().is_empty() => parts.insert(0, crate::t("courses.review.a_reading")),
                        _ => {}
                    }
                }
                if parts.is_empty() {
                    crate::t("courses.review.written_by_ai")
                } else {
                    crate::t1("courses.review.new_with", "parts", join(&parts))
                }
            }
            None => crate::t("courses.review.written_by_ai"),
        },
        (Some(_), None) => crate::t("courses.review.taken_out"),
        _ => match (b.as_ref().and_then(|b| b.as_object()), a.as_ref().and_then(|a| a.as_object())) {
            (Some(b), Some(a)) => {
                let mut parts = Vec::new();
                // The title first, then the reading, then the rest.
                let mut keys: Vec<&String> = a.keys().collect();
                keys.sort_by_key(|k| (k.as_str() != "title", k.as_str() != "content", name_of(k).is_none()));
                for k in keys {
                    let new = &a[k.as_str()];
                    match b.get(k) {
                        Some(old) if old == new => {}
                        Some(old) => parts.extend(changed_field(k, old, new)),
                        None => parts.push(match (name_of(k), new) {
                            (Some(Name::List(one, many)), serde_json::Value::Array(items)) => crate::t1("courses.review.added", "what", count(items.len(), &one, &many)),
                            _ => crate::t1("courses.review.added", "what", whole(k)),
                        }),
                    }
                }
                for (k, old) in b {
                    if !a.contains_key(k) {
                        parts.push(match (name_of(k), old) {
                            (Some(Name::List(one, many)), serde_json::Value::Array(items)) => crate::t1("courses.review.removed_all", "what", count(items.len(), &one, &many)),
                            _ => crate::t1("courses.review.removed", "what", whole(k)),
                        });
                    }
                }
                if parts.is_empty() {
                    crate::t("courses.review.layout_only")
                } else {
                    sentence(parts)
                }
            }
            _ if path.ends_with(".svg") => crate::t("courses.review.redrew"),
            _ => crate::t("courses.review.edited"),
        },
    }
}

fn changed_field(key: &str, old: &serde_json::Value, new: &serde_json::Value) -> Vec<String> {
    use serde_json::Value;
    if key == "title" {
        if let Value::String(t) = new {
            return vec![crate::t1("courses.review.renamed_it", "title", t)];
        }
    }
    let (Value::Array(old), Value::Array(new)) = (old, new) else {
        let verb = if matches!(new, Value::String(_)) { "courses.review.rewrote" } else { "courses.review.changed" };
        return vec![crate::t1(verb, "what", whole(key))];
    };
    // Items compared whole: one gone and one new count as one rewritten.
    let mut left: HashMap<String, usize> = HashMap::new();
    for v in old {
        *left.entry(v.to_string()).or_default() += 1;
    }
    let mut added = 0;
    for v in new {
        match left.get_mut(&v.to_string()) {
            Some(n) if *n > 0 => *n -= 1,
            _ => added += 1,
        }
    }
    let removed: usize = left.values().sum();
    let rewritten = added.min(removed);
    let (one, many) = match name_of(key) {
        Some(Name::List(one, many)) => (one, many),
        _ => {
            let w = whole(key);
            let w = w.strip_prefix("the ").unwrap_or(&w).to_string();
            (crate::t1("courses.review.item_of.one", "field", &w), crate::t1("courses.review.item_of.other", "field", &w))
        }
    };
    let mut parts = Vec::new();
    if rewritten > 0 {
        parts.push(crate::t1("courses.review.rewrote", "what", count(rewritten, &one, &many)));
    }
    if added > rewritten {
        parts.push(crate::t1("courses.review.added", "what", count(added - rewritten, &one, &many)));
    }
    if removed > rewritten {
        parts.push(crate::t1("courses.review.removed", "what", count(removed - rewritten, &one, &many)));
    }
    if parts.is_empty() {
        parts.push(crate::t1("courses.review.reordered", "name", many));
    }
    parts
}

// ---------- readable lines ----------

/// A pretty-printed JSON line as a label and a value: `"question": "Half of
/// 4?",` is ("Question", "Half of 4?"); a line of brackets is bare. Lines of
/// other files are shown as they are.
fn readable(raw: &str, json: bool) -> Cell {
    let mut c = Cell { raw: raw.to_string(), ..Cell::default() };
    if !json {
        c.pre = raw.to_string();
        return c;
    }
    let t = raw.trim_start();
    c.indent = ((raw.len() - t.len()) / 2).min(8);
    let t = t.strip_suffix(',').unwrap_or(t);
    if matches!(t, "{" | "}" | "[" | "]" | "{}" | "[]") {
        c.bare = true;
        return c;
    }
    let (key, rest) = match t.starts_with('"').then(|| key_of(t)).flatten() {
        Some((k, rest)) => (Some(k), rest),
        None => (None, t),
    };
    if let Some(k) = key {
        c.label = label(&k);
        c.quiet = quiet_key(&k);
    }
    c.pre = match rest {
        "{" | "[" => String::new(),
        "{}" | "[]" => crate::t("courses.review.none"),
        s if s.starts_with('"') => serde_json::from_str::<String>(s).unwrap_or_else(|_| s.to_string()),
        s => s.to_string(),
    };
    c
}

/// The key of a `"key": value` line and the value as written.
fn key_of(t: &str) -> Option<(String, &str)> {
    let mut escaped = false;
    for (i, ch) in t.char_indices().skip(1) {
        match ch {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '"' => {
                let rest = t[i + 1..].strip_prefix(": ")?;
                return Some((serde_json::from_str(&t[..=i]).ok()?, rest));
            }
            _ => {}
        }
    }
    None
}

/// What a lesson's (or the course settings') key is called on the page, in
/// the words of `docs/course-format.md` and the editor (#282): "prompt" →
/// "Question". Keys not listed read as words ("example_meaning" → "Example meaning").
fn label(key: &str) -> String {
    if LABELS.contains(&key) { crate::t(&format!("courses.review.labels.{key}")) } else { humanised(key) }
}

/// Course-format keys with a plain name (`label`), at
/// `courses.review.labels.<key>` in `i18n/`.
const LABELS: &[&str] = &[
    "prompt", "question", "content", "correct_answer", "accepted_answers", "options", "hints", "teaches", "type", "tolerance", "question_id", "id", "slug", "order", "prerequisites", "vocabulary", "term", "conjugations", "guide", "vocab", "sprint", "themes", "role_a", "role_b", "markdown", "wrong", "fix", "chars",
];

/// `LABELS` as a JSON object, for the page's script that redraws an edited line.
pub fn labels_json() -> String {
    let m: serde_json::Map<String, serde_json::Value> = LABELS.iter().map(|k| (k.to_string(), serde_json::json!(label(k)))).collect();
    serde_json::Value::Object(m).to_string()
}

/// Lines that only identify or file a lesson: dimmed in the review (#282).
fn quiet_key(key: &str) -> bool {
    matches!(key, "id" | "question_id" | "slug" | "group" | "prerequisites" | "order")
}

/// "correct_answer" → "Correct answer".
fn humanised(key: &str) -> String {
    let words = key.replace(['_', '-'], " ");
    let mut c = words.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

// ---------- applying the review ----------

/// One file after the review: what the AI wrote minus what `excluded` leaves
/// out, with the lines `edits` rewrites (`None`: the file is not there).
/// JSON left out in part or edited is mended (commas) and must still parse.
/// An edited line is always in (#287): leaving it out, or its lesson, is
/// ignored.
fn result(f: &AiRunFile, title: &str, excluded: &Exclusions, edits: &Edits) -> Result<Option<String>, String> {
    let json = f.path.ends_with(".json");
    let edited = edits.0.get(&f.path).filter(|e| json && !e.is_empty());
    if excluded.file_out(&f.path) && (edited.is_none() || f.after.is_none()) {
        return Ok(f.before.clone());
    }
    let keys = excluded.keys(&f.path);
    if (keys.is_empty() && edited.is_none() && !excluded.file_out(&f.path)) || f.after.is_none() {
        return Ok(f.after.clone());
    }
    let (before, after) = normalized(f);
    let (a, b) = (split(before.as_deref().unwrap_or("")), split(after.as_deref().unwrap_or("")));
    let ops = diff(&a, &b);
    let pairs = pairs(&ops);
    // The learner's text for a line on the right, when it changes the line.
    let text_of = |p: &Pair| p.r.zip(edited.and_then(|e| e.get(&p.key())));
    let is_edited = |p: &Pair| text_of(p).is_some_and(|(j, t)| edited_lines(&b[j], t).is_some());
    if excluded.file_out(&f.path) && !pairs.iter().any(is_edited) {
        return Ok(f.before.clone());
    }
    let out_of = |p: &Pair| !p.same && keys.contains(p.key().as_str()) && !is_edited(p);
    let changes = pairs.iter().filter(|p| !p.same).count();
    let left_out = pairs.iter().filter(|p| out_of(p)).count();
    if left_out > 0 && left_out == changes {
        return Ok(f.before.clone());
    }
    let mut rewritten = 0;
    let mut lines: Vec<std::borrow::Cow<str>> = Vec::new();
    for p in &pairs {
        if out_of(p) {
            lines.extend(p.l.map(|i| a[i].as_str().into()));
            continue;
        }
        let Some(j) = p.r else { continue };
        match text_of(p).and_then(|(_, t)| edited_lines(&b[j], t)) {
            Some(mine) => {
                rewritten += 1;
                lines.extend(mine.into_iter().map(Into::into));
            }
            None => lines.push(b[j].as_str().into()),
        }
    }
    if left_out == 0 && rewritten == 0 {
        return Ok(f.after.clone());
    }
    let lines: Vec<&str> = lines.iter().map(|l| l.as_ref()).collect();
    if !json {
        let mut text = lines.join("\n");
        if f.after.as_deref().is_some_and(|s| s.ends_with('\n')) {
            text.push('\n');
        }
        return Ok(Some(text));
    }
    let value = serde_json::from_str::<serde_json::Value>(&lines.join("\n")).or_else(|_| serde_json::from_str(&mend(&lines)));
    match value {
        Ok(v) => Ok(Some(serde_json::to_string_pretty(&v).unwrap_or_default())),
        Err(e) if rewritten > 0 => Err(crate::t2("courses.review.edits_unreadable", "title", title, "error", e)),
        Err(e) => Err(crate::t2("courses.review.left_out_unreadable", "title", title, "error", e)),
    }
}

/// Pretty-printed JSON with lines taken out: put back the commas between
/// items and drop the ones before a closing bracket. (A pretty-printed line
/// never holds a raw line break, so this works line by line.)
fn mend(lines: &[&str]) -> String {
    let lines: Vec<&str> = lines.iter().map(|l| l.trim_end()).filter(|l| !l.is_empty()).collect();
    let mut out = Vec::with_capacity(lines.len());
    for (i, l) in lines.iter().enumerate() {
        let closes = lines.get(i + 1).map_or(true, |n| n.trim_start().starts_with([']', '}']));
        let opens = l.ends_with('{') || l.ends_with('[');
        let line = match (l.strip_suffix(','), closes) {
            (Some(bare), true) => bare.to_string(),
            (None, false) if !opens => format!("{l},"),
            _ => l.to_string(),
        };
        out.push(line);
    }
    out.join("\n")
}

/// The course's files after accepting the review with `excluded` left out,
/// or `None` when they are already that. `stored` is what the review left
/// out last time: a file may be changed only while it is as the AI wrote it
/// or as that review left it; one edited since is refused when this Accept
/// would change it, and left alone when what is left out of it is as last
/// time (after an Undo, or a hand edit, Accept must not trip over a lesson
/// nobody asked it to touch).
pub fn apply(run: &AiRun, chosen: (&Exclusions, &Edits), stored: (&Exclusions, &Edits), current: &[CourseFile]) -> Result<Option<Vec<CourseFile>>, String> {
    let mut files: Vec<CourseFile> = current.to_vec();
    let mut changed = false;
    for f in &run.files {
        let title = title_of(&f.path, f.after.as_deref().or(f.before.as_deref()).unwrap_or(""));
        let target = result(f, &title, chosen.0, chosen.1)?;
        let at = files.iter().position(|c| c.path == f.path);
        let now = at.map(|i| files[i].content.clone());
        if target == now {
            continue;
        }
        let last = result(f, &title, stored.0, stored.1).ok().flatten();
        let mut target = target;
        if now != f.after && now != last {
            if target == last {
                continue;
            }
            // Changed since only where the lesson table writes (#280): the
            // review's choice goes in, with those changes kept.
            let merged = [f.after.as_deref(), last.as_deref()].into_iter().find_map(|base| with_table_changes(&f.path, base, now.as_deref(), target.as_deref()));
            match merged {
                Some(t) if t == now => continue,
                Some(t) => target = t,
                None => {
                    return Err(format!(
                        "“{title}” has been changed since the AI run, so the review cannot change it any more. Keep all of it, or change it on the course page."
                    ))
                }
            }
        }
        match (target, at) {
            (Some(content), Some(i)) => files[i].content = content,
            (Some(content), None) => files.push(CourseFile { path: f.path.clone(), content }),
            (None, Some(i)) => {
                files.remove(i);
            }
            (None, None) => {}
        }
        changed = true;
    }
    Ok(changed.then_some(files))
}

/// The keys of a lesson the lesson table writes when it saves: a lesson's
/// place and unit (every lesson gets an `order`), its name, and the
/// prerequisites naming a lesson it deletes.
const TABLE_KEYS: [&str; 5] = ["order", "unit", "level", "title", "prerequisites"];

fn object(s: &str) -> Option<serde_json::Map<String, serde_json::Value>> {
    match serde_json::from_str(s).ok()? {
        serde_json::Value::Object(m) => Some(m),
        _ => None,
    }
}

/// When lesson `now` is `base` with only `TABLE_KEYS` changed (by the lesson
/// table), those changes: key and new value (`None`: taken out).
fn table_changes(path: &str, base: &str, now: &str) -> Option<Vec<(String, Option<serde_json::Value>)>> {
    if !path.ends_with(".json") || path == crate::content::MANIFEST_FILE {
        return None;
    }
    let (b, n) = (object(base)?, object(now)?);
    let keys: std::collections::BTreeSet<&String> = b.keys().chain(n.keys()).collect();
    let mut out = Vec::new();
    for k in keys {
        if b.get(k) != n.get(k) {
            if !TABLE_KEYS.contains(&k.as_str()) {
                return None;
            }
            out.push((k.clone(), n.get(k).cloned()));
        }
    }
    Some(out)
}

/// `target` (what the review makes of a lesson that was `base`) with the
/// lesson table's changes from `base` to `now` laid over it (#280), or `None`
/// when `now` differs from `base` in more than that, or the review changes a
/// key the table changed too.
fn with_table_changes(path: &str, base: Option<&str>, now: Option<&str>, target: Option<&str>) -> Option<Option<String>> {
    let (base, now) = (base?, now?);
    let changes = table_changes(path, base, now)?;
    let Some(target) = target else { return Some(None) };
    let (mut t, b) = (object(target)?, object(base)?);
    for (k, v) in changes {
        if t.get(&k) != b.get(&k) && t.get(&k) != v.as_ref() {
            return None;
        }
        match v {
            Some(v) => t.insert(k, v),
            None => t.remove(&k),
        };
    }
    Some(Some(serde_json::to_string_pretty(&serde_json::Value::Object(t)).ok()? + "\n"))
}

/// The run has been undone (#281): every file it changed is as it was
/// before it (lesson-table changes aside), while what the review last kept
/// of it is not that. Its review then shows the run but cannot accept it.
pub fn undone(run: &AiRun, stored: (&Exclusions, &Edits), current: &[CourseFile]) -> bool {
    let now = |p: &str| current.iter().find(|c| c.path == p).map(|c| c.content.as_str());
    let same = |path: &str, a: Option<&str>, b: Option<&str>| match (a, b) {
        (Some(a), Some(b)) => a == b || serde_json::from_str::<serde_json::Value>(a).ok().is_some_and(|v| serde_json::from_str::<serde_json::Value>(b).ok() == Some(v)) || table_changes(path, b, a).is_some(),
        (a, b) => a == b,
    };
    let all_before = run.files.iter().all(|f| same(&f.path, now(&f.path), f.before.as_deref()));
    let kept_nothing = run.files.iter().all(|f| {
        let title = title_of(&f.path, f.after.as_deref().or(f.before.as_deref()).unwrap_or(""));
        result(f, &title, stored.0, stored.1).ok().is_some_and(|r| same(&f.path, r.as_deref(), f.before.as_deref()))
    });
    !run.files.is_empty() && all_before && !kept_nothing
}

/// `excluded` and `edits` without files the run did not change, ready to keep on it.
pub fn known(run: &AiRun, excluded: Exclusions, edits: Edits) -> (Exclusions, Edits) {
    (excluded.known(run), edits.known(run))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Same,
    Del,
    Add,
}

/// The edit script turning `a` into `b`: a longest common subsequence.
fn diff<'a>(a: &'a [String], b: &'a [String]) -> Vec<Op> {
    let mut ids: HashMap<&str, u32> = HashMap::new();
    let mut id = |s: &'a str| {
        let n = ids.len() as u32;
        *ids.entry(s).or_insert(n)
    };
    let a: Vec<u32> = a.iter().map(|s| id(s)).collect();
    let b: Vec<u32> = b.iter().map(|s| id(s)).collect();
    let pre = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suf = a[pre..].iter().rev().zip(b[pre..].iter().rev()).take_while(|(x, y)| x == y).count();
    let (am, bm) = (&a[pre..a.len() - suf], &b[pre..b.len() - suf]);
    let mut ops = vec![Op::Same; pre];
    if am.len().saturating_mul(bm.len()) > MAX_CELLS {
        ops.extend(std::iter::repeat(Op::Del).take(am.len()));
        ops.extend(std::iter::repeat(Op::Add).take(bm.len()));
    } else {
        hirschberg(am, bm, &mut ops);
    }
    ops.extend(std::iter::repeat(Op::Same).take(suf));
    ops
}

fn hirschberg(a: &[u32], b: &[u32], out: &mut Vec<Op>) {
    if a.is_empty() {
        out.extend(std::iter::repeat(Op::Add).take(b.len()));
    } else if b.is_empty() {
        out.extend(std::iter::repeat(Op::Del).take(a.len()));
    } else if a.len() == 1 {
        match b.iter().position(|x| *x == a[0]) {
            Some(j) => {
                out.extend(std::iter::repeat(Op::Add).take(j));
                out.push(Op::Same);
                out.extend(std::iter::repeat(Op::Add).take(b.len() - j - 1));
            }
            None => {
                out.push(Op::Del);
                out.extend(std::iter::repeat(Op::Add).take(b.len()));
            }
        }
    } else {
        let mid = a.len() / 2;
        let front = lcs_lengths(a[..mid].iter(), b.iter(), b.len());
        let back = lcs_lengths(a[mid..].iter().rev(), b.iter().rev(), b.len());
        let k = (0..=b.len()).max_by_key(|&k| (front[k] + back[b.len() - k], std::cmp::Reverse(k))).unwrap_or(0);
        hirschberg(&a[..mid], &b[..k], out);
        hirschberg(&a[mid..], &b[k..], out);
    }
}

/// The last row of the LCS table of `a` against every prefix of `b`.
fn lcs_lengths<'a>(a: impl Iterator<Item = &'a u32>, b: impl Iterator<Item = &'a u32> + Clone, n: usize) -> Vec<u32> {
    let mut prev = vec![0u32; n + 1];
    let mut cur = vec![0u32; n + 1];
    for x in a {
        for (j, y) in b.clone().enumerate() {
            cur[j + 1] = if x == y { prev[j] + 1 } else { cur[j].max(prev[j + 1]) };
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev
}

/// A row of the edit script: a line on each side (`same`), or a removed
/// line next to the added line that replaces it (either may be missing).
struct Pair {
    l: Option<usize>,
    r: Option<usize>,
    same: bool,
}

impl Pair {
    /// "3-4": line 3 before, line 4 after (1-based; 0 for none). Stable for
    /// a run, so a review left out in part can be shown and applied again.
    fn key(&self) -> String {
        format!("{}-{}", self.l.map_or(0, |i| i + 1), self.r.map_or(0, |j| j + 1))
    }
}

/// A run of removed lines sits next to the added lines that replace it.
fn pairs(ops: &[Op]) -> Vec<Pair> {
    let mut all = Vec::new();
    let (mut i, mut j, mut k) = (0, 0, 0);
    while k < ops.len() {
        if ops[k] == Op::Same {
            all.push(Pair { l: Some(i), r: Some(j), same: true });
            (i, j, k) = (i + 1, j + 1, k + 1);
            continue;
        }
        let (mut dels, mut adds) = (Vec::new(), Vec::new());
        while k < ops.len() && ops[k] != Op::Same {
            if ops[k] == Op::Del {
                dels.push(i);
                i += 1;
            } else {
                adds.push(j);
                j += 1;
            }
            k += 1;
        }
        for n in 0..dels.len().max(adds.len()) {
            all.push(Pair { l: dels.get(n).copied(), r: adds.get(n).copied(), same: false });
        }
    }
    all
}

/// Side-by-side rows, every line shown readable.
fn rows(a: &[String], b: &[String], ops: &[Op], json: bool) -> Vec<Row> {
    pairs(ops)
        .into_iter()
        .map(|p| {
            let side = |line: Option<usize>, lines: &[String], kind: &'static str| match line {
                Some(n) => Cell { kind, ..readable(&lines[n], json) },
                None => Cell { kind: "empty", ..Cell::default() },
            };
            let (lk, rk) = if p.same { ("same", "same") } else { ("del", "add") };
            let mut row = Row {
                kind: if p.same { "same" } else { "change" },
                gap: 0,
                key: p.key(),
                excluded: false,
                group: 0,
                left: side(p.l, a, lk),
                right: side(p.r, b, rk),
            };
            if !p.same && p.l.is_some() && p.r.is_some() {
                mark(&mut row);
            }
            row
        })
        .collect()
}

/// Split a replaced value and its replacement into the common start, the
/// part that differs, and the common end (only when both have the same label).
fn mark(r: &mut Row) {
    if r.left.label != r.right.label || r.left.bare || r.right.bare {
        return;
    }
    let (a, b): (Vec<char>, Vec<char>) = (r.left.pre.chars().collect(), r.right.pre.chars().collect());
    let pre = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let post = a[pre..].iter().rev().zip(b[pre..].iter().rev()).take_while(|(x, y)| x == y).count();
    let part = |s: &[char]| s.iter().collect::<String>();
    (r.left.mid, r.left.post) = (part(&a[pre..a.len() - post]), part(&a[a.len() - post..]));
    (r.right.mid, r.right.post) = (part(&b[pre..b.len() - post]), part(&b[b.len() - post..]));
    let start = part(&a[..pre]);
    (r.left.pre, r.right.pre) = (start.clone(), start);
}

fn fold(all: Vec<Row>) -> Vec<Row> {
    let near: Vec<bool> = (0..all.len())
        .map(|n| all[n.saturating_sub(CONTEXT)..(n + CONTEXT + 1).min(all.len())].iter().any(|r| r.kind == "change"))
        .collect();
    let mut out = Vec::new();
    let mut skipped = 0;
    for (r, keep) in all.into_iter().zip(near) {
        if keep {
            if skipped > 0 {
                out.push(gap(skipped));
                skipped = 0;
            }
            out.push(r);
        } else {
            skipped += 1;
        }
    }
    if skipped > 0 {
        out.push(gap(skipped));
    }
    out
}

fn gap(n: usize) -> Row {
    Row {
        kind: "gap",
        gap: n,
        key: String::new(),
        excluded: false,
        group: 0,
        left: Cell { kind: "empty", ..Cell::default() },
        right: Cell { kind: "empty", ..Cell::default() },
    }
}

#[cfg(test)]
mod tests;
