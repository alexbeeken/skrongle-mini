//! What the course editor and the file page say when a course file is
//! refused: one `Problem` per refusal, whichever check found it (the JSON
//! parse, the course rules of `ContentStore::from_files`, or the lesson
//! table's own rules in `arrange.rs`). A problem says which kind it is, where
//! it is (the lesson by its title, the field, the line) and how to fix it, so
//! the editor can open that drawer and say the same thing there and under
//! the title. `text` is the whole sentence as the pages show it;
//! static/courses.js writes its own JSON parse errors the same way.

use serde::Serialize;
use serde_json::Value;

use crate::content::{ContentError, CourseFile, MANIFEST_FILE};

/// Which check refused it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// The text does not parse as JSON.
    Json,
    /// Valid JSON that the course format or rules refuse.
    Rules,
    /// Refused for another reason (the table itself, a file that is too big).
    Refused,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Json => "Not valid JSON",
            Kind::Rules => "Against the course rules",
            Kind::Refused => "Not saved",
        }
    }
}

/// One refusal, placed: `file` (a course path), `row` (the lesson table's
/// row, in the order the page sent them), the lesson's id and title, the
/// `field` (a top-level key of the file) and the line (and, for a parse
/// error, column) in the file's text.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Problem {
    pub kind: Kind,
    pub file: Option<String>,
    pub row: Option<usize>,
    pub lesson_id: Option<String>,
    pub lesson_title: Option<String>,
    pub field: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    /// What is wrong, in plain words.
    pub message: String,
    /// How to fix it, when there is something to say.
    pub hint: Option<String>,
    /// The kind, the place, the message and the hint as one sentence.
    pub text: String,
}

impl Problem {
    pub fn new(kind: Kind, message: impl Into<String>) -> Problem {
        Problem {
            kind,
            file: None,
            row: None,
            lesson_id: None,
            lesson_title: None,
            field: None,
            line: None,
            column: None,
            message: sentence(&message.into()),
            hint: None,
            text: String::new(),
        }
        .sealed()
    }
    pub fn refused(message: impl Into<String>) -> Problem {
        Problem::new(Kind::Refused, message)
    }
    pub fn rules(message: impl Into<String>) -> Problem {
        Problem::new(Kind::Rules, message)
    }

    /// A JSON text that `serde_json` refused: a parse error (with its line
    /// and column), or JSON that does not fit the format (a missing field, a
    /// wrong type), which is against the rules.
    pub fn from_serde(e: &serde_json::Error, content: Option<&str>) -> Problem {
        let raw = strip_position(&e.to_string());
        let (line, column) = (e.line(), e.column());
        let (line, column) = if line > 0 { (Some(line), Some(column)) } else { (None, None) };
        match e.classify() {
            serde_json::error::Category::Syntax | serde_json::error::Category::Eof => {
                let mut p = Problem::new(Kind::Json, plain_syntax(&raw)).hint(JSON_HINT);
                p.line = line;
                p.column = column;
                p.sealed()
            }
            serde_json::error::Category::Data => {
                let (message, field, hint) = plain_data(&raw);
                let field = field.or_else(|| line.and_then(|l| content.and_then(|c| key_on_line(c, l))));
                let mut p = Problem::rules(message);
                p.field = field;
                p.line = line;
                p.hint = hint;
                p.sealed()
            }
            serde_json::error::Category::Io => Problem::refused(raw),
        }
    }

    pub fn file(mut self, path: &str) -> Problem {
        self.file = Some(path.to_string());
        self.sealed()
    }
    pub fn row(mut self, i: usize, title: &str) -> Problem {
        self.row = Some(i);
        if !title.trim().is_empty() {
            self.lesson_title = Some(title.trim().to_string());
        }
        self.sealed()
    }
    pub fn lesson(mut self, id: Option<&str>, title: Option<&str>) -> Problem {
        if let Some(id) = id.filter(|s| !s.is_empty()) {
            self.lesson_id = Some(id.to_string());
        }
        if self.lesson_title.is_none() {
            self.lesson_title = title.map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
        }
        self.sealed()
    }
    pub fn field(mut self, f: &str) -> Problem {
        self.field = Some(f.to_string());
        self.sealed()
    }
    pub fn line(mut self, line: Option<usize>) -> Problem {
        if line.is_some() {
            self.line = line;
        }
        self.sealed()
    }
    pub fn hint(mut self, h: impl Into<String>) -> Problem {
        let h = h.into();
        self.hint = if h.trim().is_empty() { None } else { Some(sentence(&h)) };
        self.sealed()
    }

    /// Where it is, in words: "the course settings", "“Greetings”", or the path.
    fn place(&self) -> Option<String> {
        if self.file.as_deref() == Some(MANIFEST_FILE) {
            return Some("the course settings".into());
        }
        if let Some(t) = &self.lesson_title {
            return Some(format!("“{t}”"));
        }
        if let Some(id) = &self.lesson_id {
            return Some(format!("the lesson “{id}”"));
        }
        self.file.clone()
    }

    fn sealed(mut self) -> Problem {
        let mut at = Vec::new();
        if let Some(p) = self.place() {
            at.push(p);
        }
        if let Some(f) = &self.field {
            at.push(format!("field “{f}”"));
        }
        match (self.line, self.column) {
            (Some(l), Some(c)) if self.kind == Kind::Json => at.push(format!("line {l}, column {c}")),
            (Some(l), _) => at.push(format!("line {l}")),
            _ => {}
        }
        let mut s = self.kind.label().to_string();
        if !at.is_empty() {
            s.push_str(" in ");
            s.push_str(&at.join(", "));
        }
        s.push_str(": ");
        s.push_str(&self.message);
        if let Some(h) = &self.hint {
            s.push_str(" To fix it: ");
            s.push_str(&lower_first(h));
        }
        self.text = s;
        self
    }
}

const JSON_HINT: &str = "look just before that point for a missing comma, quote or bracket, or a comma right before } or ]. Nothing in it is saved until it is valid JSON";

/// A course's files as the problems are explained against: the files that
/// were checked, the editor's rows (each row's file path and name, in the
/// order the page sent them), and the files as they were before the change,
/// to tell which of two clashing lessons was just edited.
pub struct Course<'a> {
    pub files: &'a [CourseFile],
    pub rows: &'a [(String, String)],
    pub before: Option<&'a [CourseFile]>,
}

impl<'a> Course<'a> {
    pub fn new(files: &'a [CourseFile]) -> Course<'a> {
        Course { files, rows: &[], before: None }
    }

    fn content(&self, path: &str) -> Option<&str> {
        self.files.iter().find(|f| f.path == path).map(|f| f.content.as_str())
    }
    fn value(&self, path: &str) -> Option<Value> {
        serde_json::from_str(self.content(path)?).ok()
    }
    fn edited(&self, path: &str) -> bool {
        let Some(before) = self.before else { return false };
        let now = self.content(path);
        !before.iter().any(|f| f.path == path && Some(f.content.as_str()) == now)
    }
    /// The lesson files that hold a lesson: (path, id, title).
    fn lessons(&self) -> Vec<(String, String, String)> {
        self.files
            .iter()
            .filter(|f| crate::arrange::is_lesson_path(&f.path))
            .filter_map(|f| {
                let v: Value = serde_json::from_str(&f.content).ok()?;
                let id = v.get("id")?.as_str()?.to_string();
                let title = v.get("title").and_then(|t| t.as_str()).unwrap_or_default().to_string();
                Some((f.path.clone(), id, title))
            })
            .collect()
    }
    fn manifest(&self) -> Option<Value> {
        self.value(MANIFEST_FILE)
    }
    /// A file's name for a sentence: the row's name, else the lesson's title, else its path.
    fn name_of(&self, path: &str) -> String {
        if path == MANIFEST_FILE {
            return "the course settings".into();
        }
        if path == crate::placement::FILE {
            return "the placement test".into();
        }
        let title = self
            .rows
            .iter()
            .find(|(p, _)| p == path)
            .map(|(_, t)| t.clone())
            .filter(|t| !t.trim().is_empty())
            .or_else(|| self.value(path).and_then(|v| v.get("title").and_then(|t| t.as_str()).map(str::to_string)));
        match title {
            Some(t) if !t.trim().is_empty() => format!("“{}”", t.trim()),
            _ => path.to_string(),
        }
    }

    /// `p` placed at `path`: its row, lesson id and title.
    fn at(&self, p: Problem, path: &str) -> Problem {
        let mut p = p.file(path);
        if let Some(i) = self.rows.iter().position(|(r, _)| r == path) {
            p = p.row(i, &self.rows[i].1);
        }
        if path != MANIFEST_FILE && path != crate::placement::FILE {
            let v = self.value(path);
            let get = |k: &str| v.as_ref().and_then(|v| v.get(k)).and_then(|x| x.as_str()).map(str::to_string);
            p = p.lesson(get("id").as_deref(), get("title").as_deref());
        }
        p
    }

    /// The line of `"key":` (the first one) in a file.
    fn key_line(&self, path: &str, keys: &[&str]) -> Option<usize> {
        let c = self.content(path)?;
        keys.iter().find_map(|k| find_line(c, &format!("\"{k}\""), |rest| rest.trim_start().starts_with(':')))
    }

    /// A refusal of `ContentStore::from_files`, in plain words and placed.
    pub fn explain(&self, e: &ContentError) -> Problem {
        match e {
            ContentError::Io(p, err) => Problem::refused(format!("{} could not be read ({err})", p.display())),
            ContentError::Parse(path, err) => {
                let p = Problem::from_serde(err, self.content(path));
                if self.content(path).is_some() { self.at(p, path) } else { p.file(path) }
            }
            ContentError::Duplicate(what) => {
                Problem::rules(format!("The {what} is used twice")).hint("give each its own id")
            }
            ContentError::Clash { what, id, files, note } => self.clash(what, id, files, note.as_deref()),
            ContentError::MissingPrerequisite { lesson, prerequisite } => self.missing_prerequisite(lesson, prerequisite),
            ContentError::Invalid(msg) => self.invalid(msg),
        }
    }

    fn clash(&self, what: &str, id: &str, files: &[String], note: Option<&str>) -> Problem {
        let main = files.iter().find(|f| self.edited(f)).or(files.last()).cloned().unwrap_or_default();
        let other = files.iter().find(|f| **f != main).cloned();
        let (field, thing) = match what {
            "lesson id" => ("id", "the id"),
            "lesson slug" => ("slug", "the slug"),
            "question id" => ("quiz", "the question_id"),
            "conversation id" => ("conversations", "the conversation id"),
            "conjugation" => ("conjugations", "a conjugation of"),
            _ => ("", "the id"),
        };
        let in_set = note.and_then(|n| n.strip_prefix("in set ")).map(|s| format!(" in the set “{}”", unquote(s))).unwrap_or_default();
        let message = match &other {
            Some(o) => format!("{} and {} both use {thing} “{id}”{in_set}", self.name_of(&main), self.name_of(o)),
            None => format!("{} “{id}” is used twice in this lesson{in_set}", cap(thing)),
        };
        let taken: Vec<String> = match what {
            "lesson id" => self.lessons().into_iter().map(|(_, i, _)| i).collect(),
            "lesson slug" => self.files.iter().filter_map(|f| serde_json::from_str::<Value>(&f.content).ok()).filter_map(|v| v.get("slug").and_then(|s| s.as_str()).map(str::to_string)).collect(),
            _ => Vec::new(),
        };
        let free = free_id(id, &taken);
        let hint = match what {
            "lesson id" => format!("give one of them another \"id\" (lowercase a-z, 0-9 and '-'), for example “{free}”. Prerequisites name lessons by id, so change those that meant the renamed one too"),
            "lesson slug" => format!("give one of them another \"slug\" (lowercase a-z, 0-9 and '-'; it is the lesson's web address), for example “{free}”"),
            "question id" => format!("every question in the course needs its own \"question_id\"; rename one, for example to “{free}”"),
            "conversation id" => format!("every conversation in the course needs its own \"id\"; rename one, for example to “{free}”"),
            "conjugation" => "conjugate each verb once per set: remove one of them or change its \"set\"".into(),
            "picture id" => format!("every picture in the course needs its own \"id\"; rename one, for example to “{free}”"),
            _ => "give each its own id".into(),
        };
        let mut p = self.at(Problem::rules(message), &main).hint(hint);
        if !field.is_empty() {
            p = p.field(field);
        }
        let line = self.content(&main).and_then(|c| find_line(c, &format!("\"{id}\""), |_| true));
        p.line(line)
    }

    fn missing_prerequisite(&self, lesson: &str, prerequisite: &str) -> Problem {
        let lessons = self.lessons();
        let path = lessons.iter().find(|(_, i, _)| i == lesson).map(|(p, _, _)| p.clone());
        let others: Vec<&(String, String, String)> = lessons.iter().filter(|(_, i, _)| i != lesson).collect();
        let ids: Vec<String> = others.iter().map(|(_, i, _)| i.clone()).collect();
        let message = if prerequisite.trim().is_empty() {
            "One of its prerequisites is empty".to_string()
        } else {
            format!("“{prerequisite}” is not the id of a lesson in this course")
        };
        let by_title = others.iter().find(|(_, _, t)| t.trim().eq_ignore_ascii_case(prerequisite.trim()));
        let hint = match by_title {
            Some((_, id, t)) => format!("“{t}” is a lesson's title; prerequisites name lessons by their \"id\", so write “{id}”"),
            None if ids.is_empty() => "this is the only lesson, so it can have no prerequisites: remove it from the list".to_string(),
            None => {
                let mean = closest(prerequisite, &ids).map(|c| format!(" Did you mean “{c}”?")).unwrap_or_default();
                format!("a prerequisite must be the \"id\" of another lesson in this course, or remove it from the list.{mean} The lesson ids are: {}", list(&ids))
            }
        };
        let p = Problem::rules(message).field("prerequisites").hint(hint);
        let Some(path) = path else { return p.lesson(Some(lesson), None) };
        let line = self.content(&path).and_then(|c| {
            let from = c.find("\"prerequisites\"").unwrap_or(0);
            find_line(&c[from..], &format!("\"{}\"", json_escape(prerequisite)), |_| true).map(|l| l + c[..from].matches('\n').count())
        });
        self.at(p, &path).line(line.or_else(|| self.key_line(&path, &["prerequisites"])))
    }

    fn invalid(&self, msg: &str) -> Problem {
        // "<path>: …"; the longest path that fits.
        let path = self
            .files
            .iter()
            .map(|f| f.path.as_str())
            .filter(|p| msg.starts_with(p) && matches!(msg[p.len()..].chars().next(), Some(':') | Some(' ')))
            .max_by_key(|p| p.len());
        let Some(path) = path else {
            return Problem::rules(msg.replace("the manifest", "the course settings"));
        };
        let rest = msg[path.len()..].trim_start_matches(':').trim();
        let manifest = path == MANIFEST_FILE;
        // The placement test's fields are its own (#1106): no lesson hints.
        let field = if path == crate::placement::FILE {
            Some(if rest.starts_with("random_per_unit") { "random_per_unit" } else { "questions" }).filter(|_| !rest.starts_with("no questions"))
        } else {
            field_of(rest, manifest)
        };
        let said = if field == Some("content") { rest.trim_start_matches("guide: ") } else { rest };
        let message = said.replace("the manifest", "the course settings");
        let mut p = self.at(Problem::rules(message), path);
        if let Some(f) = field {
            let keys: Vec<&str> = match f {
                "unit" => vec!["unit", "level"],
                "group" => vec!["group", "category"],
                f => vec![f],
            };
            p = p.field(f).line(self.key_line(path, &keys));
            if path == crate::placement::FILE {
                p = p.hint("see “placement.json” in the course format (/docs/course-format#placementjson-placement-test)");
            } else if let Some(h) = self.hint_for(f, rest, manifest) {
                p = p.hint(h);
            }
        }
        p
    }

    fn hint_for(&self, field: &str, rest: &str, manifest: bool) -> Option<String> {
        let quoted = first_quoted(rest);
        let m = self.manifest();
        let pairs = |key: &str| -> Vec<(String, String)> {
            m.as_ref()
                .and_then(|m| m.get(key))
                .and_then(|u| u.as_array())
                .into_iter()
                .flatten()
                .filter_map(|u| Some((u.get("id")?.as_str()?.to_string(), u.get("title").and_then(|t| t.as_str()).unwrap_or_default().to_string())))
                .collect()
        };
        let has = |s: &str| rest.contains(s);
        if manifest {
            return Some(match field {
                "id" => "keep the course's \"id\" as it was: lowercase a-z, 0-9 and '-'".into(),
                "title" => "give the course a title".into(),
                "units" if has("duplicate") => "give every unit its own \"id\"".into(),
                "units" => "list the units as [{\"id\": \"1\", \"title\": \"…\"}, …], each with an id and a title".into(),
                "groups" if has("duplicate") => "give every group its own \"id\"".into(),
                "groups" => "give every group an \"id\" and a \"title\"".into(),
                "chars" => "write each helper character as its own text, for example [\"á\", \"é\"]".into(),
                f => format!("see “{f}” in the course format (/docs/course-format)"),
            });
        }
        Some(match field {
            "unit" => {
                let units = pairs("units");
                let ids: Vec<String> = units.iter().map(|(i, _)| i.clone()).collect();
                let mean = quoted.as_deref().and_then(|q| closest(q, &ids)).map(|c| format!(" Did you mean “{c}”?")).unwrap_or_default();
                let named: Vec<String> = units.iter().map(|(i, t)| if t.is_empty() { format!("“{i}”") } else { format!("“{i}” ({t})") }).collect();
                format!("use one of the course's units: {}.{mean} Or add the unit to \"units\" in the course settings first", named.join(", "))
            }
            "group" => {
                let groups: Vec<String> = pairs("groups").into_iter().map(|(i, _)| i).collect();
                if groups.is_empty() { "give it a group, for example “general”".into() } else { format!("use one of the course's groups: {}", list(&groups)) }
            }
            "id" | "slug" => format!("use only lowercase a-z, 0-9 and '-', for example “{}”", slug_of(quoted.as_deref().unwrap_or("lesson"))),
            "title" => "give the lesson a name".into(),
            "content" => "write some text in \"content\" (each ## heading becomes one card), or give the lesson vocabulary or questions".into(),
            "quiz" if has("correct_answer must be one of the options") => "copy one of the \"options\" into \"correct_answer\" exactly (same spelling, accents and capitals)".into(),
            "quiz" if has("teaches") => "add that word to this lesson's \"vocabulary\", or take it out of \"teaches\"".into(),
            "quiz" if has("at least 2 options") => "give it two or more \"options\"".into(),
            "quiz" if has("tolerance and unit") => "remove \"tolerance\" and \"unit\", or make it a \"numeric\" question".into(),
            "quiz" if has("true-false") => "write \"true\" or \"false\" as its \"correct_answer\"".into(),
            "quiz" if has("empty prompt") => "write the question in its \"prompt\"".into(),
            "quiz" if has("empty correct_answer") => "write the answer in \"correct_answer\"".into(),
            "quiz" if has("question_id") => "give every question a \"question_id\" that no other question in the course uses".into(),
            "vocabulary" if has("duplicate") => "keep one of them (or merge their meanings)".into(),
            "vocabulary" => "fill in both \"term\" and \"meaning\", or remove the entry".into(),
            "conjugations" if has("no paradigm") => "add a \"paradigm\" to the course settings, or remove \"conjugations\" from this lesson".into(),
            "conjugations" if has("is not in the paradigm") => "use one of the sets listed in the message as its \"set\"".into(),
            "conjugations" if has("is missing the") => "add that form to its \"forms\"".into(),
            "conjugations" if has("unknown slot") => "use only the paradigm's slots as keys of \"forms\"".into(),
            f => format!("see “{f}” in the course format (/docs/course-format)"),
        })
    }
}

/// The top-level field a rule message is about.
fn field_of(rest: &str, manifest: bool) -> Option<&'static str> {
    let starts = |p: &str| rest.starts_with(p);
    let has = |p: &str| rest.contains(p);
    if manifest {
        return Some(if starts("id ") {
            "id"
        } else if starts("title") {
            "title"
        } else if has("unit") {
            "units"
        } else if has("group") {
            "groups"
        } else if has("paradigm") || starts("art for slot") {
            "paradigm"
        } else if starts("chars") {
            "chars"
        } else if starts("calendar") {
            "calendar"
        } else {
            return None;
        });
    }
    Some(if starts("lesson id and slug") {
        "id"
    } else if starts("slug") {
        "slug"
    } else if starts("title") {
        "title"
    } else if starts("unit ") {
        "unit"
    } else if starts("group") {
        "group"
    } else if starts("question") {
        "quiz"
    } else if has("vocabulary entr") {
        "vocabulary"
    } else if starts("this lesson has conjugations") || starts("conjugation") || starts("art for '") {
        "conjugations"
    } else if starts("sprint") {
        "sprint"
    } else if starts("listening") {
        "listening"
    } else if starts("conversation") {
        "conversations"
    } else if has("produced no guide steps") {
        "content"
    } else if starts("guide") {
        "guide"
    } else {
        return None;
    })
}

/// serde_json's message without its " at line 3 column 5".
fn strip_position(s: &str) -> String {
    match s.rfind(" at line ") {
        Some(i) => s[..i].to_string(),
        None => s.to_string(),
    }
}

/// A parse error in plain words (the same as static/courses.js says them).
fn plain_syntax(raw: &str) -> String {
    let s = raw;
    let plain = if s.starts_with("EOF while parsing") {
        "The text ends before every { and [ is closed"
    } else if s.starts_with("trailing comma") {
        "There is a comma right before } or ], which JSON does not allow"
    } else if s.starts_with("key must be a string") {
        "Expected a key in double quotes"
    } else if s.starts_with("expected `:`") {
        "Expected a : after the key"
    } else if s.starts_with("expected `,` or `}`") {
        "Expected a comma or }"
    } else if s.starts_with("expected `,` or `]`") {
        "Expected a comma or ]"
    } else if s.starts_with("expected value") || s.starts_with("expected ident") {
        "Expected a value: text in double quotes, a number, true, false, null, { or ["
    } else if s.starts_with("trailing characters") {
        "There is more text after the end of the JSON"
    } else if s.starts_with("control character") {
        "A line break or tab inside a text in quotes; write \\n or \\t instead"
    } else if s.starts_with("invalid escape") || s.starts_with("invalid unicode") {
        "A backslash in a text starts an escape JSON does not know; write \\\\ for a backslash"
    } else if s.starts_with("invalid number") || s.starts_with("number out of range") {
        "A number is not written as JSON writes numbers"
    } else {
        return sentence(&cap(s));
    };
    plain.to_string()
}

/// A message about JSON that does not fit the format: (message, field, hint).
fn plain_data(raw: &str) -> (String, Option<String>, Option<String>) {
    let ticked = |s: &str| -> Vec<String> { s.split('`').skip(1).step_by(2).map(str::to_string).collect() };
    if let Some(rest) = raw.strip_prefix("missing field ") {
        let f = ticked(rest).into_iter().next().unwrap_or_default();
        return (format!("It has no “{f}”, which it needs"), Some(f.clone()), Some(format!("add \"{f}\" (see the course format, /docs/course-format)")));
    }
    if let Some(rest) = raw.strip_prefix("duplicate field ") {
        let f = ticked(rest).into_iter().next().unwrap_or_default();
        return (format!("“{f}” is there twice"), Some(f), Some("keep one of them".into()));
    }
    if let Some(rest) = raw.strip_prefix("unknown variant ") {
        let all = ticked(rest);
        let (got, allowed) = all.split_first().map(|(g, a)| (g.clone(), a.to_vec())).unwrap_or_default();
        let mean = closest(&got, &allowed).map(|c| format!(" Did you mean “{c}”?")).unwrap_or_default();
        return (format!("“{got}” is not one of the values allowed here"), None, Some(format!("use one of: {}.{mean}", list(&allowed))));
    }
    if let Some(rest) = raw.strip_prefix("unknown field ") {
        let all = ticked(rest);
        let (got, allowed) = all.split_first().map(|(g, a)| (g.clone(), a.to_vec())).unwrap_or_default();
        let mean = closest(&got, &allowed).map(|c| format!("did you mean “{c}”?")).unwrap_or_else(|| "remove it".into());
        return (format!("“{got}” is not a field the course format knows"), Some(got), Some(mean));
    }
    if let Some(rest) = raw.strip_prefix("invalid type: ") {
        if let Some((got, want)) = rest.split_once(", expected ") {
            return (format!("This value is {}, but it should be {}", plain_type(got), plain_type(want)), None, None);
        }
    }
    (sentence(&cap(raw)), None, None)
}

/// "a sequence" → "a list ([…])" and so on.
fn plain_type(t: &str) -> String {
    let t = t.trim();
    if t.contains("sequence") {
        return "a list ([…])".into();
    }
    if t.starts_with("map") || t.contains("a map") || t.starts_with("struct") || t.contains("struct ") {
        return "an object ({…})".into();
    }
    if let Some(s) = t.strip_prefix("string ") {
        return format!("the text {s}");
    }
    if t == "a string" || t.contains("string") {
        return "a text in double quotes".into();
    }
    if t.starts_with("integer") || t.starts_with("floating point") {
        return format!("the number {}", t.split('`').nth(1).unwrap_or(""));
    }
    if t.starts_with("boolean") {
        return format!("{}", t.split('`').nth(1).unwrap_or("true or false"));
    }
    if matches!(t, "u8" | "u16" | "u32" | "u64" | "usize" | "i8" | "i16" | "i32" | "i64" | "isize") {
        return "a whole number".into();
    }
    if matches!(t, "f32" | "f64") {
        return "a number".into();
    }
    if t == "a boolean" {
        return "true or false".into();
    }
    t.to_string()
}

/// The key on line `line` (1-based) of a JSON text, if one starts there.
fn key_on_line(content: &str, line: usize) -> Option<String> {
    let l = content.lines().nth(line.checked_sub(1)?)?;
    let start = l.find('"')?;
    let rest = &l[start + 1..];
    let end = rest.find('"')?;
    rest[end + 1..].trim_start().starts_with(':').then(|| rest[..end].to_string())
}

/// The 1-based line of the first `needle` whose text after it passes `after`.
fn find_line(content: &str, needle: &str, after: impl Fn(&str) -> bool) -> Option<usize> {
    let mut from = 0;
    while let Some(i) = content[from..].find(needle) {
        let at = from + i;
        if after(&content[at + needle.len()..]) {
            return Some(content[..at].matches('\n').count() + 1);
        }
        from = at + needle.len();
    }
    None
}

fn json_escape(s: &str) -> String {
    let q = serde_json::to_string(s).unwrap_or_default();
    q.strip_prefix('"').and_then(|q| q.strip_suffix('"')).unwrap_or(&q).to_string()
}

fn first_quoted(s: &str) -> Option<String> {
    let start = s.find('\'')?;
    let rest = &s[start + 1..];
    Some(rest[..rest.find('\'')?].to_string())
}

fn unquote(s: &str) -> String {
    s.trim().trim_matches('\'').to_string()
}

fn cap(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        // Leave "JSON", "“Greetings”" and the like alone.
        Some(f) if f.is_uppercase() && !c.clone().next().is_some_and(|n| n.is_uppercase()) => f.to_lowercase().collect::<String>() + c.as_str(),
        Some(f) => f.to_string() + c.as_str(),
        None => String::new(),
    }
}

/// Capitalised and ending in a full stop (or ? or !).
fn sentence(s: &str) -> String {
    let s = cap(s.trim());
    if s.is_empty() || s.ends_with(['.', '?', '!']) { s } else { s + "." }
}

/// Up to 12 values, quoted, then "…".
fn list(values: &[String]) -> String {
    let mut out: Vec<String> = values.iter().take(12).map(|v| format!("“{v}”")).collect();
    if values.len() > 12 {
        out.push(format!("and {} more", values.len() - 12));
    }
    out.join(", ")
}

/// `id-2`, `id-3`… whichever is not taken.
fn free_id(id: &str, taken: &[String]) -> String {
    (2..).map(|n| format!("{id}-{n}")).find(|c| !taken.contains(c)).expect("some suffix is free")
}

fn slug_of(s: &str) -> String {
    let slug: String = s.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    let slug = slug.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    if slug.is_empty() { "lesson".into() } else { slug }
}

/// The closest of `options` to `s` (edit distance, ignoring case), when it is close enough to be a slip.
pub fn closest(s: &str, options: &[String]) -> Option<String> {
    let s = s.trim().to_lowercase();
    if s.is_empty() {
        return None;
    }
    let (best, d) = options.iter().map(|o| (o, distance(&s, &o.to_lowercase()))).min_by_key(|(_, d)| *d)?;
    let len = s.chars().count().max(best.chars().count());
    (d <= (len / 3).max(2) && d < len).then(|| best.clone())
}

fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            cur.push((prev[j] + usize::from(ca != *cb)).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::ContentStore;

    fn files(lessons: &[(&str, &str)]) -> Vec<CourseFile> {
        let mut out = vec![CourseFile {
            path: MANIFEST_FILE.into(),
            content: r#"{"id": "c", "title": "C", "units": [{"id": "1", "title": "Basics"}, {"id": "2", "title": "More"}]}"#.into(),
        }];
        for (path, content) in lessons {
            out.push(CourseFile { path: path.to_string(), content: content.to_string() });
        }
        out
    }
    fn lesson(id: &str, title: &str, extra: &str) -> String {
        format!("{{\n  \"id\": \"{id}\",\n  \"slug\": \"{id}\",\n  \"title\": \"{title}\",\n  \"unit\": \"1\",\n  \"group\": \"g\",\n  \"summary\": \"\",\n  \"content\": \"## {title}\\n\\nText.\"{extra}\n}}\n")
    }
    fn problem(f: &[CourseFile]) -> Problem {
        let e = ContentStore::from_files(f).err().expect("the course is refused");
        Course::new(f).explain(&e)
    }

    #[test]
    fn an_unknown_prerequisite_names_the_lesson_by_title_and_suggests_the_id() {
        let what = lesson("u1-what", "What it is", "");
        let hello = lesson("u1-hello", "Hello", ",\n  \"prerequisites\": [\n    \"u1-whta\"\n  ]");
        let f = files(&[("lessons/what.json", &what), ("lessons/hello.json", &hello)]);
        let p = problem(&f);
        assert_eq!(p.kind, Kind::Rules, "{}", p.text);
        assert_eq!(p.field.as_deref(), Some("prerequisites"), "{}", p.text);
        assert_eq!(p.file.as_deref(), Some("lessons/hello.json"));
        assert_eq!(p.lesson_title.as_deref(), Some("Hello"));
        assert_eq!(p.lesson_id.as_deref(), Some("u1-hello"));
        assert_eq!(p.field.as_deref(), Some("prerequisites"));
        assert_eq!(p.line, Some(10));
        assert_eq!(p.message, "“u1-whta” is not the id of a lesson in this course.");
        assert_eq!(
            p.text,
            "Against the course rules in “Hello”, field “prerequisites”, line 10: “u1-whta” is not the id of a lesson in this course. \
             To fix it: a prerequisite must be the \"id\" of another lesson in this course, or remove it from the list. Did you mean “u1-what”? The lesson ids are: “u1-what”."
        );
        // Nothing close: no guess, but the ids; a title is pointed at its id.
        let hello = lesson("u1-hello", "Hello", ",\n  \"prerequisites\": [\"uasdf asf ..asdf ads1\"]");
        let p = problem(&files(&[("lessons/what.json", &what), ("lessons/hello.json", &hello)]));
        assert!(!p.text.contains("Did you mean"), "{}", p.text);
        assert!(p.text.contains("The lesson ids are: “u1-what”"), "{}", p.text);
        let hello = lesson("u1-hello", "Hello", ",\n  \"prerequisites\": [\"What it is\"]");
        let p = problem(&files(&[("lessons/what.json", &what), ("lessons/hello.json", &hello)]));
        assert!(p.text.contains("“What it is” is a lesson's title; prerequisites name lessons by their \"id\", so write “u1-what”"), "{}", p.text);
    }

    #[test]
    fn a_duplicate_id_names_both_lessons_and_flags_the_one_just_edited() {
        let a = lesson("a", "Apples", "");
        let b = lesson("b", "Bananas", "");
        let before = files(&[("lessons/a.json", &a), ("lessons/b.json", &b)]);
        let b2 = lesson("a", "Bananas", "").replace("\"slug\": \"a\"", "\"slug\": \"b\"");
        let after = files(&[("lessons/a.json", &a), ("lessons/b.json", &b2)]);
        let e = ContentStore::from_files(&after).err().unwrap();
        let rows = vec![("lessons/a.json".to_string(), "Apples".to_string()), ("lessons/b.json".to_string(), "Bananas".to_string())];
        let p = Course { files: &after, rows: &rows, before: Some(&before) }.explain(&e);
        assert_eq!((p.kind, p.file.as_deref(), p.row, p.field.as_deref(), p.line), (Kind::Rules, Some("lessons/b.json"), Some(1), Some("id"), Some(2)));
        assert_eq!(p.message, "“Bananas” and “Apples” both use the id “a”.");
        assert!(p.text.starts_with("Against the course rules in “Bananas”, field “id”, line 2: "), "{}", p.text);
        assert!(p.text.contains("give one of them another \"id\""), "{}", p.text);
        assert!(p.text.contains("“a-2”"), "{}", p.text);
        // A slug, too.
        let b3 = lesson("b", "Bananas", "").replace("\"slug\": \"b\"", "\"slug\": \"a\"");
        let p = problem(&files(&[("lessons/a.json", &a), ("lessons/b.json", &b3)]));
        assert_eq!(p.field.as_deref(), Some("slug"));
        assert!(p.message.contains("both use the slug “a”"), "{}", p.message);
    }

    #[test]
    fn every_kind_of_refusal_is_placed_and_worded() {
        let a = lesson("a", "Apples", "");
        // A parse error: the line and column, in plain words.
        let f = files(&[("lessons/a.json", "{\n  \"id\": \"a\",\n}")]);
        let p = problem(&f);
        assert_eq!((p.kind, p.line, p.column), (Kind::Json, Some(3), Some(1)));
        assert!(p.text.starts_with("Not valid JSON in lessons/a.json, line 3, column 1: There is a comma right before } or ]"), "{}", p.text);
        // A missing field: against the rules, the field named.
        let p = problem(&files(&[("lessons/a.json", r#"{"id": "a", "slug": "a", "unit": "1", "group": "g", "summary": "", "content": ""}"#)]));
        assert_eq!((p.kind, p.field.as_deref()), (Kind::Rules, Some("title")));
        // A unit the course does not have: the units and a guess.
        let p = problem(&files(&[("lessons/a.json", &a.replace("\"unit\": \"1\"", "\"unit\": \"3\""))]));
        assert_eq!((p.field.as_deref(), p.line, p.lesson_title.as_deref()), (Some("unit"), Some(5), Some("Apples")));
        assert!(p.message.contains("is not in the course settings"), "{}", p.message);
        assert!(p.text.contains("use one of the course's units: “1” (Basics), “2” (More)"), "{}", p.text);
        // A quiz answer that is not an option.
        let quiz = ",\n  \"quiz\": [{\"question_id\": \"q\", \"type\": \"multiple-choice\", \"prompt\": \"?\", \"options\": [\"x\", \"y\"], \"correct_answer\": \"z\"}]";
        let p = problem(&files(&[("lessons/a.json", &lesson("a", "Apples", quiz))]));
        assert_eq!(p.field.as_deref(), Some("quiz"));
        assert!(p.text.contains("copy one of the \"options\" into \"correct_answer\""), "{}", p.text);
        // The same question id in two lessons.
        let p = problem(&files(&[("lessons/a.json", &lesson("a", "Apples", quiz.replace("\"z\"", "\"x\"").as_str())), ("lessons/b.json", &lesson("b", "Bananas", quiz.replace("\"z\"", "\"x\"").as_str()))]));
        assert_eq!(p.field.as_deref(), Some("quiz"));
        assert!(p.message.contains("both use the question_id “q”"), "{}", p.message);
        // The course settings.
        let mut f = files(&[("lessons/a.json", &a)]);
        f[0].content = r#"{"id": "c", "title": "C", "units": [{"id": "1", "title": "A"}, {"id": "1", "title": "B"}]}"#.into();
        let p = problem(&f);
        assert_eq!((p.file.as_deref(), p.field.as_deref()), (Some(MANIFEST_FILE), Some("units")));
        assert!(p.text.starts_with("Against the course rules in the course settings, field “units”"), "{}", p.text);
    }

    #[test]
    fn did_you_mean_only_guesses_near_misses() {
        let ids = vec!["u1-what".to_string(), "u1-hello".to_string()];
        assert_eq!(closest("u1-wat", &ids).as_deref(), Some("u1-what"));
        assert_eq!(closest("U1-HELLO", &ids).as_deref(), Some("u1-hello"));
        assert_eq!(closest("uasdf asf ..asdf ads1", &ids), None);
    }
}
