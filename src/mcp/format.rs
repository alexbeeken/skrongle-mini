//! `get_course_format` (#168): the course format contract,
//! `docs/course-format.md` (already compiled in for `/docs`, read through
//! `routes::Docs`, so the binary holds one copy), cut at its headings at
//! first use so an agent can ask for one part at a time; and the test
//! course (`data/test-courses/test-course/`) as a worked example.

use rust_embed::RustEmbed;
use serde_json::{json, Value};
use std::sync::OnceLock;

/// The test course, embedded: a server's data folder does not ship it.
#[derive(RustEmbed)]
#[folder = "data/test-courses/test-course/"]
struct Example;

/// The section name that answers with the worked example.
pub const EXAMPLE: &str = "example";

/// One part of the contract: a heading and everything under it, up to the
/// next heading of its level or higher (a `##` section holds its `###`s).
struct Section {
    id: String,
    title: String,
    level: usize,
    start: usize,
    end: usize,
}

/// The contract and its sections, cut once.
struct Contract {
    doc: String,
    sections: Vec<Section>,
}

/// A heading's section id: lower case, words joined by `-`, a bracketed
/// part left out (`placement.json (placement test)` is `placement-json`).
fn slug(title: &str) -> String {
    let bare = title.split(" (").next().unwrap_or(title);
    let words: Vec<String> = bare.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect();
    words.join("-")
}

/// Cut Markdown at its `##`, `###` and `####` headings (not inside code
/// fences). The text before the first is `overview`.
fn split(doc: &str) -> Vec<Section> {
    let mut heads: Vec<(String, String, usize, usize)> = Vec::new();
    let mut fenced = false;
    let mut at = 0;
    for line in doc.split_inclusive('\n') {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
        }
        let hashes = line.bytes().take_while(|&b| b == b'#').count();
        if !fenced && (2..=4).contains(&hashes) && line.as_bytes().get(hashes) == Some(&b' ') {
            let title = line.get(hashes + 1..).unwrap_or("").trim().to_string();
            heads.push((slug(&title), title, hashes, at));
        }
        at += line.len();
    }
    let first = heads.first().map_or(doc.len(), |h| h.3);
    let mut out = vec![Section { id: "overview".into(), title: "Overview".into(), level: 1, start: 0, end: first }];
    for (i, (id, title, level, start)) in heads.iter().enumerate() {
        let end = heads.iter().skip(i + 1).find(|h| h.2 <= *level).map_or(doc.len(), |h| h.3);
        out.push(Section { id: id.clone(), title: title.clone(), level: *level, start: *start, end });
    }
    out
}

fn contract() -> &'static Contract {
    static CONTRACT: OnceLock<Contract> = OnceLock::new();
    CONTRACT.get_or_init(|| {
        let doc = crate::routes::Docs::get("course-format.md").map(|f| String::from_utf8_lossy(&f.data).into_owned()).unwrap_or_default();
        let sections = split(&doc);
        Contract { doc, sections }
    })
}

impl Contract {
    fn text(&self, s: &Section) -> &str {
        self.doc.get(s.start..s.end).unwrap_or("")
    }

    /// The section `name` names: its id, else the first whose id starts with it.
    fn find(&self, name: &str) -> Option<&Section> {
        let name = slug(name);
        if name.is_empty() {
            return None;
        }
        self.sections.iter().find(|s| s.id == name).or_else(|| self.sections.iter().find(|s| s.id.starts_with(&name)))
    }

    /// The table of contents: every section's id, title and level.
    fn index(&self) -> Value {
        let mut list: Vec<Value> = self.sections.iter().map(|s| json!({ "section": s.id, "title": s.title, "level": s.level })).collect();
        list.push(json!({ "section": EXAMPLE, "title": "Worked example: a course that uses every feature", "level": 2 }));
        Value::Array(list)
    }
}

fn example_paths() -> Vec<String> {
    let mut paths: Vec<String> = Example::iter().map(|p| p.into_owned()).collect();
    paths.sort();
    paths
}

fn example_file(path: &str) -> Option<String> {
    Example::get(path).map(|f| String::from_utf8_lossy(&f.data).into_owned())
}

/// The tool's answer: `Ok(result)`, or `Err(message)` for a section or an
/// example file it does not know.
pub fn answer(section: Option<&str>, file: Option<&str>) -> Result<Value, String> {
    let c = contract();
    if let Some(path) = file.map(str::trim).filter(|f| !f.is_empty()) {
        return match example_file(path) {
            Some(content) => Ok(json!({ "section": EXAMPLE, "file": path, "content": content })),
            None => Err(format!("The example has no file \"{path}\". Its files: {}", example_paths().join(", "))),
        };
    }
    let Some(name) = section.map(str::trim).filter(|s| !s.is_empty()) else {
        let overview = c.sections.first().map_or("", |s| c.text(s));
        return Ok(json!({
            "overview": overview,
            "sections": c.index(),
            "how": "Call again with \"section\" set to one id from \"sections\" for that part of the contract. \
                    \"example\" lists the files of a worked course; \"file\" returns one of them.",
        }));
    };
    if slug(name) == EXAMPLE {
        let files: Vec<Value> = example_paths()
            .into_iter()
            .map(|p| {
                let bytes = Example::get(&p).map_or(0, |f| f.data.len());
                json!({ "path": p, "bytes": bytes })
            })
            .collect();
        return Ok(json!({
            "section": EXAMPLE,
            "about": "A course that switches on every feature, one lesson per feature. \
                      Ask for one file with \"file\". AUTHORING.md says what each lesson is for; it is not part of the course.",
            "files": files,
            "course.json": example_file(crate::content::MANIFEST_FILE).unwrap_or_default(),
        }));
    }
    match c.find(name) {
        Some(s) => Ok(json!({ "section": s.id, "title": s.title, "text": c.text(s) })),
        None => {
            let ids: Vec<&str> = c.sections.iter().map(|s| s.id.as_str()).chain([EXAMPLE]).collect();
            Err(format!("No section \"{name}\". The sections: {}", ids.join(", ")))
        }
    }
}

/// The example course's files (path, content), AUTHORING.md left out: what
/// an agent would send to make it.
#[cfg(test)]
pub fn example_course() -> Vec<(String, String)> {
    example_paths().into_iter().filter(|p| p.ends_with(".json")).filter_map(|p| example_file(&p).map(|c| (p, c))).collect()
}
