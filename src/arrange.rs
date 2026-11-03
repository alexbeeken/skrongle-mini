//! Restructuring a user-made course by lesson: apply the editor's lesson
//! table (order, units, names, removals, new and edited lessons), or place
//! a new AI-written lesson where it belongs. Each returns the whole new set
//! of files, already validated, for `courses::commit_revision`, so every
//! change is a revision that can be undone. A lesson file stays byte for
//! byte unless the table changed it or its `unit`, `order` or
//! `prerequisites` have to change.

use std::collections::HashMap;

use crate::content::{ContentStore, CourseFile, MANIFEST_FILE};
use crate::problems::{Course, Problem};

/// One lesson file of a course, in course order.
#[derive(Debug, Clone)]
pub struct LessonFile {
    pub path: String,
    pub id: String,
    pub value: serde_json::Value,
}

impl LessonFile {
    pub fn unit(&self) -> &str {
        self.value.get("unit").or_else(|| self.value.get("level")).and_then(|u| u.as_str()).unwrap_or_default()
    }
}

pub fn is_lesson_path(path: &str) -> bool {
    path.starts_with("lessons/") && path.ends_with(".json")
}

/// The course loaded, and its lesson files in course order. A lesson file
/// must hold one lesson with an id (the format allows nothing else).
pub fn lessons_of(files: &[CourseFile]) -> Result<(ContentStore, Vec<LessonFile>), String> {
    let store = ContentStore::from_files(files)
        .map_err(|e| format!("The course does not load, so its lessons cannot be rearranged. {}", Course::new(files).explain(&e).text))?;
    let mut by_id = HashMap::new();
    for f in files.iter().filter(|f| is_lesson_path(&f.path)) {
        let value: serde_json::Value = serde_json::from_str(&f.content).map_err(|e| format!("{}: {e}", f.path))?;
        let id = value.get("id").and_then(|i| i.as_str()).ok_or(format!("{}: not a single lesson with an id; change it by hand instead", f.path))?;
        by_id.insert(id.to_string(), LessonFile { path: f.path.clone(), id: id.to_string(), value });
    }
    let ordered = store.lessons().iter().filter_map(|l| by_id.remove(&l.id)).collect();
    Ok((store, ordered))
}

/// The course settings with the course called `title` (its subject too, while
/// that was the old title); `None` when the title is already that.
pub fn retitle(manifest: &str, title: &str) -> Result<Option<String>, Problem> {
    let title = title.trim();
    if title.is_empty() || title.chars().count() > 80 {
        let what = if title.is_empty() { "The course has no title" } else { "The course title is longer than 80 characters" };
        return Err(Problem::refused(what).field("title").hint("click the title to give the course a name of up to 80 characters"));
    }
    let mut v: serde_json::Value = serde_json::from_str(manifest).map_err(|e| Problem::from_serde(&e, Some(manifest)).file(MANIFEST_FILE))?;
    let obj = v.as_object_mut().ok_or_else(not_an_object)?;
    let old = obj.get("title").cloned();
    if old.as_ref().and_then(|t| t.as_str()) == Some(title) {
        return Ok(None);
    }
    if obj.get("subject").is_some_and(|s| Some(s) == old.as_ref()) {
        obj.insert("subject".into(), serde_json::json!(title));
    }
    obj.insert("title".into(), serde_json::json!(title));
    Ok(Some(serde_json::to_string_pretty(&v).map_err(|e| Problem::refused(e.to_string()))? + "\n"))
}

/// What a file that is JSON but not one object is told.
fn not_an_object() -> Problem {
    Problem::rules("The file must be one JSON object, { … }").hint("put the whole file between one pair of { }")
}

/// A lesson file with `key` set to `value` (or removed for null), keeping
/// the rest; `None` when nothing changes.
pub fn patch_lesson(content: &str, key: &str, value: serde_json::Value) -> Result<Option<String>, String> {
    let mut v: serde_json::Value = serde_json::from_str(content).map_err(|e| e.to_string())?;
    let obj = v.as_object_mut().ok_or("the lesson is not a JSON object")?;
    if obj.get(key) == Some(&value) || (value.is_null() && !obj.contains_key(key)) {
        return Ok(None);
    }
    if value.is_null() {
        obj.remove(key);
    } else {
        obj.insert(key.to_string(), value);
    }
    Ok(Some(serde_json::to_string_pretty(&v).map_err(|e| e.to_string())? + "\n"))
}

/// The prerequisites of a lesson file, without those `drop` says.
pub fn without_prerequisites(content: &str, drop: impl Fn(&str) -> bool) -> Result<Option<String>, String> {
    let v: serde_json::Value = serde_json::from_str(content).map_err(|e| e.to_string())?;
    let Some(pre) = v.get("prerequisites").and_then(|p| p.as_array()) else { return Ok(None) };
    let kept: Vec<serde_json::Value> = pre.iter().filter(|p| !p.as_str().is_some_and(&drop)).cloned().collect();
    if kept.len() == pre.len() {
        return Ok(None);
    }
    patch_lesson(content, "prerequisites", serde_json::Value::Array(kept))
}

/// The identity of a lesson as an AI outline gives it.
pub fn outline_entry(v: &serde_json::Value, focus: Option<&serde_json::Value>) -> serde_json::Value {
    let mut e = serde_json::json!({
        "id": v.get("id"), "slug": v.get("slug"), "title": v.get("title"), "unit": v.get("unit"),
        "group": v.get("group"), "summary": v.get("summary"),
        "prerequisites": v.get("prerequisites").cloned().unwrap_or_else(|| serde_json::json!([])),
    });
    if let Some(f) = focus {
        e["focus"] = f.clone();
    }
    e
}

/// Largest list of written lessons a lesson request carries, in bytes.
pub const DIGEST_CAP: usize = 2_000;

/// What the lessons written so far teach, for the next lesson's request
/// (#446): one line per lesson with its title, vocabulary terms and `## `
/// headings, so the model builds on them and calls things by the same
/// words. The list stays within `DIGEST_CAP`: past it the older lessons
/// lose their headings first (the latest keeps its), then whole lessons go,
/// oldest first. Empty before the first lesson is written.
pub fn written_digest(files: &[CourseFile]) -> String {
    let lessons: Vec<(String, String)> = files
        .iter()
        .filter(|f| is_lesson_path(&f.path))
        .filter_map(|f| {
            let v: serde_json::Value = serde_json::from_str(&f.content).ok()?;
            let title = v.get("title").and_then(|t| t.as_str()).unwrap_or_default();
            let terms: Vec<&str> = v.get("vocabulary").and_then(|a| a.as_array()).into_iter().flatten().filter_map(|t| t.get("term")?.as_str()).collect();
            let headings: Vec<&str> = v.get("content").and_then(|c| c.as_str()).unwrap_or_default().lines().filter_map(|l| l.strip_prefix("## ")).map(str::trim).collect();
            let short = if terms.is_empty() { format!("- {title}") } else { format!("- {title}: terms {}", terms.join(", ")) };
            let full = if headings.is_empty() { short.clone() } else { format!("{short}; sections {}", headings.join("; ")) };
            Some((full, short))
        })
        .collect();
    if lessons.is_empty() {
        return String::new();
    }
    // Headings go first, oldest lesson first; the latest lesson keeps its.
    let mut total: usize = lessons.iter().map(|(full, _)| full.len() + 1).sum();
    let mut trimmed = 0;
    for (full, short) in lessons.iter().take(lessons.len() - 1) {
        if total <= DIGEST_CAP {
            break;
        }
        total -= full.len() - short.len();
        trimmed += 1;
    }
    let lines: Vec<&str> = lessons.iter().enumerate().map(|(i, (full, short))| if i < trimmed { short.as_str() } else { full.as_str() }).collect();
    // Then whole lessons, oldest first, keeping at least the latest and
    // leaving room for the line that says how many went.
    let room = if total > DIGEST_CAP { DIGEST_CAP - 40 } else { DIGEST_CAP };
    let mut dropped = 0;
    for line in &lines {
        if total <= room || dropped + 1 == lines.len() {
            break;
        }
        total -= line.len() + 1;
        dropped += 1;
    }
    let left_out = if dropped > 0 { format!("({dropped} earlier lessons are left out.)\n") } else { String::new() };
    let mut list = left_out + &lines.iter().skip(dropped).copied().collect::<Vec<_>>().join("\n");
    let mut end = list.len().min(DIGEST_CAP);
    while !list.is_char_boundary(end) {
        end -= 1;
    }
    list.truncate(end);
    format!(
        "The lessons written so far, with the terms they teach and their sections:\n{list}\n\n\
         Build on these lessons and don't teach their terms or sections again. A term may come back for practice, \
         but don't explain it as new, and call it by the same word.\n\n"
    )
}

/// The rest of a lesson request (#446): the course laid out unit by unit
/// with this lesson marked, then the lessons just before and after it in
/// full. A neighbour not written yet (the next one, in a fresh run) comes as
/// its outline entry. `entries` is the outline in course order, `i` this
/// lesson's place in it.
pub fn lesson_context(manifest: &str, entries: &[serde_json::Value], i: usize, written: &[CourseFile]) -> String {
    let field = |e: &serde_json::Value, key: &str| e.get(key).and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let manifest: serde_json::Value = serde_json::from_str(manifest).unwrap_or_default();
    let mut units: Vec<(String, String)> = manifest
        .get("units")
        .and_then(|u| u.as_array())
        .into_iter()
        .flatten()
        .map(|u| (field(u, "id"), field(u, "title")))
        .collect();
    for e in entries {
        let unit = field(e, "unit");
        if !units.iter().any(|(id, _)| *id == unit) {
            units.push((unit, String::new()));
        }
    }
    let unit_of = field(entries.get(i).unwrap_or(&serde_json::Value::Null), "unit");
    let in_unit: Vec<usize> = (0..entries.len()).filter(|&j| entries.get(j).is_some_and(|e| field(e, "unit") == unit_of)).collect();
    let place = in_unit.iter().position(|&j| j == i).unwrap_or_default() + 1;
    let mut map = format!(
        "How the course is laid out, unit by unit (this is lesson {} of {}, lesson {place} of {} in unit {unit_of}):\n",
        i + 1,
        entries.len(),
        in_unit.len()
    );
    for (id, unit_title) in &units {
        map += &format!("Unit {id} \"{unit_title}\":\n");
        for (j, e) in entries.iter().enumerate().filter(|(_, e)| field(e, "unit") == *id) {
            let mut line = format!("  {}. {} \"{}\" (group {})", j + 1, field(e, "id"), field(e, "title"), field(e, "group"));
            for (key, label) in [("summary", ": "), ("focus", ". Focus: ")] {
                if !field(e, key).is_empty() {
                    line += &format!("{label}{}", field(e, key));
                }
            }
            map += &line;
            map += if j == i { "   <- this lesson\n" } else { "\n" };
        }
    }
    let neighbour = |j: Option<usize>, which: &str| {
        let Some(e) = j.and_then(|j| entries.get(j)) else { return String::new() };
        let path = format!("lessons/{}.json", field(e, "slug"));
        match written.iter().find(|f| f.path == path) {
            Some(f) => format!("The lesson just {which} this one ({}), in full:\n{}\n\n", field(e, "id"), f.content.trim()),
            None => format!(
                "The lesson just {which} this one ({}) is not written yet. Its plan:\n{}\n\n",
                field(e, "id"),
                serde_json::to_string_pretty(e).unwrap_or_default()
            ),
        }
    };
    let (before, after) = (neighbour(i.checked_sub(1), "before"), neighbour(Some(i + 1), "after"));
    let ask = match (before.is_empty(), after.is_empty()) {
        (false, false) => "Pick up where the lesson before leaves off, and lead into the lesson after.\n\n",
        (false, true) => "Pick up where the lesson before leaves off.\n\n",
        (true, false) => "Lead into the lesson after.\n\n",
        (true, true) => "",
    };
    format!("{map}\n{before}{after}{ask}")
}

/// Add `lesson` (a file under `lessons/`) to the course, right after lesson
/// `after` (`None`: first), and renumber the order around it.
pub fn place(files: &[CourseFile], lesson: CourseFile, after: Option<&str>) -> Result<Vec<CourseFile>, String> {
    let (_, lessons) = lessons_of(files)?;
    let v: serde_json::Value = serde_json::from_str(&lesson.content).map_err(|e| e.to_string())?;
    let id = v.get("id").and_then(|i| i.as_str()).filter(|i| crate::content::is_slug(i)).ok_or("the lesson needs an id of lowercase a-z, 0-9 and '-'")?;
    let unit = v.get("unit").and_then(|u| u.as_str()).ok_or("the lesson has no unit")?;
    if lessons.iter().any(|l| l.id == id) {
        return Err(format!("the id '{id}' is taken by another lesson; choose a new one"));
    }
    if files.iter().any(|f| f.path == lesson.path) {
        return Err(format!("the slug of '{}' is taken by another lesson; choose a new one", lesson.path));
    }
    let mut wanted: Vec<(String, String)> = lessons.iter().map(|l| (l.id.clone(), l.unit().to_string())).collect();
    let at = match after {
        None => 0,
        Some(a) => wanted.iter().position(|(i, _)| i == a).ok_or(format!("\"after\" is '{a}', which is not a lesson of the course"))? + 1,
    };
    wanted.insert(at, (id.to_string(), unit.to_string()));
    let mut all = files.to_vec();
    all.push(lesson);
    let out = renumber(&all, &wanted)?;
    ContentStore::from_files(&out).map_err(|e| e.to_string())?;
    Ok(out)
}

/// The rule of "Only add" (`ai::add_lesson`), checked on the result rather
/// than trusted to the prompt: `after` keeps every file of `before`, the
/// course settings byte for byte and each lesson as it was except for its
/// `order`. Anything else is refused, however small.
pub fn only_adds(before: &[CourseFile], after: &[CourseFile]) -> Result<(), String> {
    let order_free = |content: &str| {
        serde_json::from_str::<serde_json::Value>(content).ok().map(|mut v| {
            if let Some(m) = v.as_object_mut() {
                m.remove("order");
            }
            v
        })
    };
    for old in before {
        let Some(new) = after.iter().find(|f| f.path == old.path) else {
            return Err(format!("it would remove {}", old.path));
        };
        if new.content == old.content {
            continue;
        }
        if old.path == MANIFEST_FILE || !is_lesson_path(&old.path) || order_free(&new.content).is_none() || order_free(&new.content) != order_free(&old.content) {
            return Err(format!("it would change {}", old.path));
        }
    }
    if after.len() <= before.len() {
        return Err("it adds nothing".into());
    }
    Ok(())
}

/// "Edit lessons" (#1240), the mirror of `only_adds`: the same file paths,
/// `course.json` and every file that is not a lesson byte for byte, each
/// lesson keeping its id, unit and `order` and its place in the course, and
/// at least one lesson changed. A lesson's text and title may change.
pub fn only_edits(before: &[CourseFile], after: &[CourseFile]) -> Result<(), String> {
    if let Some(f) = after.iter().find(|f| !before.iter().any(|b| b.path == f.path)) {
        return Err(format!("it added or removed a lesson ({} is new)", f.path));
    }
    let place = |content: &str| serde_json::from_str::<serde_json::Value>(content).ok().map(|v| ["id", "unit", "level", "order"].map(|k| v.get(k).cloned()));
    let mut changed = false;
    for old in before {
        let Some(new) = after.iter().find(|f| f.path == old.path) else {
            return Err(format!("it added or removed a lesson ({} is gone)", old.path));
        };
        if new.content == old.content {
            continue;
        }
        if old.path == MANIFEST_FILE || !is_lesson_path(&old.path) {
            return Err(format!("it would change {}", old.path));
        }
        if place(&new.content).is_none() || place(&new.content) != place(&old.content) {
            return Err(format!("it would move {}", old.path));
        }
        changed = true;
    }
    if !changed {
        return Err("it changes no lesson".into());
    }
    let (_, was) = lessons_of(before)?;
    let (_, now) = lessons_of(after)?;
    match now.iter().zip(&was).find(|(n, w)| n.id != w.id) {
        Some((n, _)) => Err(format!("it would move {}", n.path)),
        None => Ok(()),
    }
}

/// "Edit with AI" on one lesson's row (#1242): `only_edits`, and the lesson
/// at `path` is the only file that changed, with the same id and slug.
pub fn only_edits_one(before: &[CourseFile], after: &[CourseFile], path: &str) -> Result<(), String> {
    same_lesson(before, after, path)?;
    only_edits(before, after)
}

/// The part of `only_edits_one` that needs no loading: every file but
/// `path` byte for byte, and the lesson at `path` with its id and slug.
pub fn same_lesson(before: &[CourseFile], after: &[CourseFile], path: &str) -> Result<(), String> {
    if let Some(f) = after.iter().find(|f| f.path != path && !before.iter().any(|b| b.path == f.path && b.content == f.content)) {
        return Err(format!("it would change {}, and only {path} may change", f.path));
    }
    let field = |files: &[CourseFile], key: &str| {
        let file = files.iter().find(|f| f.path == path)?;
        serde_json::from_str::<serde_json::Value>(&file.content).ok()?.get(key).cloned()
    };
    for key in ["id", "slug"] {
        if let (Some(was), Some(now)) = (field(before, key), field(after, key)) {
            if was != now {
                return Err(format!("it changed the lesson's {key} from {was} to {now}"));
            }
        }
    }
    Ok(())
}

/// One row of the editor's lesson table, as it stands when Save is pressed.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct Edit {
    /// The lesson id the row was loaded with; `None` for a row added since.
    #[serde(default)]
    pub key: Option<String>,
    pub unit: String,
    pub title: String,
    /// The whole lesson file, when it was changed in the table.
    #[serde(default)]
    pub content: Option<String>,
}

/// The course as the lesson table says, all in one go: the course settings
/// (`course.json`) when changed, rows in their new order and units, renamed,
/// with their JSON replaced or written new from a blank lesson, and the
/// lessons left out removed (with the prerequisites that name them).
/// Returns the new files and a label for Undo.
#[cfg(test)]
pub fn edit(files: &[CourseFile], rows: &[Edit], manifest: Option<&str>) -> Result<(Vec<CourseFile>, String), String> {
    edit_rows(files, rows, manifest, None).map(|(out, label, _)| (out, label)).map_err(|p| p.text)
}

/// What a save did to the placement test, for its Undo label.
fn placement_change(was: bool, now: bool) -> &'static str {
    match (was, now) {
        (false, _) => "placement test added",
        (true, false) => "placement test removed",
        (true, true) => "placement test edited",
    }
}

/// What `edit` refuses a table that changes nothing with.
pub const NOTHING_TO_SAVE: &str = "Nothing to save: the lessons are already like this.";

/// `edit`, also returning each row's file path, in the order of `rows` (a
/// new row's is made up here), so the editor can tell its rows apart after a
/// save without reloading. A refusal is a `Problem` placed at its row,
/// file and field (`problems.rs`). `placement` is the placement test
/// (`placement.json`) when changed in the table, "" when it was taken out.
pub fn edit_rows(files: &[CourseFile], rows: &[Edit], manifest: Option<&str>, placement: Option<&str>) -> Result<(Vec<CourseFile>, String, Vec<String>), Problem> {
    let (store, lessons) = lessons_of(files).map_err(Problem::refused)?;
    let old_manifest = files.iter().find(|f| f.path == MANIFEST_FILE).map(|f| f.content.as_str()).unwrap_or_default();
    let manifest = manifest.filter(|m| *m != old_manifest);
    // The units rows may use: the new settings' when they change.
    let units: Vec<String> = match manifest {
        None => store.manifest().units.iter().map(|u| u.id.clone()).collect(),
        Some(m) => {
            let at = |p: Problem, key: &str| p.file(MANIFEST_FILE).line(line_of_key(m, key));
            let v: serde_json::Value = serde_json::from_str(m).map_err(|e| Problem::from_serde(&e, Some(m)).file(MANIFEST_FILE))?;
            if !v.is_object() {
                return Err(not_an_object().file(MANIFEST_FILE));
            }
            if v.get("id").and_then(|i| i.as_str()) != Some(store.id()) {
                let p = Problem::rules(format!("The course's \"id\" must stay “{}”", store.id()))
                    .field("id")
                    .hint(format!("change it back to \"{}\"; the course's address and everyone's progress hang on it", store.id()));
                return Err(at(p, "id"));
            }
            v.get("units")
                .and_then(|u| u.as_array())
                .ok_or_else(|| at(Problem::rules("\"units\" must be a list of units").field("units").hint("write it as [{\"id\": \"1\", \"title\": \"…\"}, …]"), "units"))?
                .iter()
                .filter_map(|u| u.get("id").and_then(|i| i.as_str()).map(str::to_string))
                .collect()
        }
    };
    let unit_index = |id: &str| units.iter().position(|u| u == id);
    if rows.is_empty() {
        return Err(Problem::refused("A course needs at least one lesson; delete the whole course instead").hint("to remove the course, use Delete this course at the bottom of the page"));
    }
    let mut kept = std::collections::HashSet::new();
    for (i, row) in rows.iter().enumerate() {
        let Some(key) = row.key.as_deref() else { continue };
        if !lessons.iter().any(|l| l.id == key) {
            return Err(Problem::refused(format!("There is no lesson '{key}' in this course any more")).row(i, &row.title).hint("reload the page"));
        }
        if !kept.insert(key) {
            return Err(Problem::refused(format!("Lesson '{key}' is listed twice in the table")).row(i, &row.title).hint("reload the page"));
        }
    }
    let removed: Vec<&LessonFile> = lessons.iter().filter(|l| !kept.contains(l.id.as_str())).collect();
    let mut taken_ids: std::collections::HashSet<String> = lessons.iter().filter(|l| kept.contains(l.id.as_str())).map(|l| l.id.clone()).collect();
    // A new lesson's id is also its slug, so it must clash with neither.
    let mut taken_names: std::collections::HashSet<String> = taken_ids.clone();
    taken_names.extend(lessons.iter().filter(|l| kept.contains(l.id.as_str())).filter_map(|l| l.value.get("slug").and_then(|s| s.as_str()).map(str::to_string)));
    let mut paths: std::collections::HashSet<String> = files.iter().map(|f| f.path.clone()).collect();
    let mut out: Vec<CourseFile> = files
        .iter()
        .filter(|f| !is_lesson_path(&f.path))
        .map(|f| match manifest {
            Some(m) if f.path == MANIFEST_FILE => CourseFile { path: f.path.clone(), content: m.to_string() },
            _ => f.clone(),
        })
        .collect();
    let old_placement = files.iter().find(|f| f.path == crate::placement::FILE).map(|f| f.content.as_str());
    let placement = placement.filter(|p| old_placement.unwrap_or_default() != *p);
    if let Some(p) = placement {
        out.retain(|f| f.path != crate::placement::FILE);
        if !p.is_empty() {
            let v: serde_json::Value = serde_json::from_str(p).map_err(|e| Problem::from_serde(&e, Some(p)).file(crate::placement::FILE))?;
            if !v.is_object() {
                return Err(not_an_object().file(crate::placement::FILE));
            }
            out.push(CourseFile { path: crate::placement::FILE.into(), content: p.to_string() });
        }
    }
    let mut wanted = Vec::with_capacity(rows.len());
    let mut row_paths = Vec::with_capacity(rows.len());
    let (mut added, mut renamed, mut rewritten) = (0, 0, 0);
    for (i, row) in rows.iter().enumerate() {
        let title = row.title.trim();
        if title.is_empty() {
            return Err(Problem::refused(format!("Lesson {} has no name", i + 1)).row(i, "").field("title").hint("click its name in the table and type one"));
        }
        if unit_index(&row.unit).is_none() {
            let known: Vec<&str> = units.iter().map(String::as_str).collect();
            let p = Problem::rules(format!("It sits in unit '{}', and there is no unit '{}' in the course settings", row.unit, row.unit))
                .row(i, title)
                .field("unit")
                .hint(format!("drag it into another unit ({}), or add a unit with \"id\": \"{}\" to \"units\" in the course settings", known.join(", "), row.unit));
            return Err(p);
        }
        let old = row.key.as_deref().and_then(|k| lessons.iter().find(|l| l.id == k));
        let at_row = |p: Problem| p.row(i, title);
        let (path, mut content) = match old {
            Some(l) => {
                let f = files.iter().find(|f| f.path == l.path).expect("lessons_of reads from files");
                (l.path.clone(), row.content.clone().unwrap_or_else(|| f.content.clone()))
            }
            None => {
                added += 1;
                let id = unique(&slug_of(title), |s| taken_names.contains(s));
                taken_names.insert(id.clone());
                let group = group_for(&lessons, &store, &row.unit);
                let content = match &row.content {
                    Some(c) => fill_new(c, &id, &row.unit, &group).map_err(at_row)?,
                    None => blank_lesson(&id, title, &row.unit, &group),
                };
                let path = unique(&format!("lessons/{id}"), |p| paths.contains(&format!("{p}.json"))) + ".json";
                paths.insert(path.clone());
                (path, content)
            }
        };
        let v: serde_json::Value = serde_json::from_str(&content).map_err(|e| at_row(Problem::from_serde(&e, Some(&content)).file(&path)))?;
        if !v.is_object() {
            return Err(at_row(not_an_object().file(&path)));
        }
        let id = v
            .get("id")
            .and_then(|i| i.as_str())
            .ok_or_else(|| at_row(Problem::rules("The lesson has no \"id\"").file(&path).field("id").hint(format!("add \"id\": \"{}\"", slug_of(title)))))?
            .to_string();
        if old.is_some() && row.content.as_ref().is_some_and(|c| Some(c) != files.iter().find(|f| f.path == path).map(|f| &f.content)) {
            rewritten += 1;
        }
        if v.get("title").and_then(|t| t.as_str()) != Some(title) {
            content = patch_lesson(&content, "title", serde_json::json!(title)).map_err(|e| at_row(Problem::refused(e)))?.unwrap_or(content);
            if old.is_some() {
                renamed += 1;
            }
        }
        if old.is_none() || old.is_some_and(|l| l.id != id) {
            if !taken_ids.insert(id.clone()) {
                let holder = lessons.iter().find(|l| l.id == id && kept.contains(l.id.as_str()));
                let other = holder.and_then(|l| l.value.get("title").and_then(|t| t.as_str())).map(|t| format!("“{t}”")).unwrap_or_else(|| "another lesson".into());
                let taken: Vec<&str> = taken_ids.iter().map(String::as_str).collect();
                let free = unique(&format!("{id}-2"), |s| taken.contains(&s));
                let p = Problem::rules(format!("“{title}” and {other} both use the id “{id}”"))
                    .file(&path)
                    .field("id")
                    .line(line_of_key(&content, "id"))
                    .hint(format!("give “{title}” another \"id\" (lowercase a-z, 0-9 and '-'), for example “{free}”. Prerequisites name lessons by id, so change those that meant it too"));
                return Err(at_row(p).lesson(Some(&id), Some(title)));
            }
        }
        wanted.push((id, row.unit.clone()));
        row_paths.push(path.clone());
        out.push(CourseFile { path, content });
    }
    if !removed.is_empty() {
        for f in out.iter_mut().filter(|f| is_lesson_path(&f.path)) {
            if let Some(c) = without_prerequisites(&f.content, |p| removed.iter().any(|l| l.id == p)).map_err(Problem::refused)? {
                f.content = c;
            }
        }
    }
    let untouched = out.iter().all(|f| files.iter().any(|g| g.path == f.path && g.content == f.content));
    let rank = |unit: &str| unit_index(unit).unwrap_or(usize::MAX);
    let mut asked: Vec<(usize, usize, &str, &str)> = wanted.iter().enumerate().map(|(i, (id, u))| (rank(u), i, id.as_str(), u.as_str())).collect();
    asked.sort();
    if untouched && manifest.is_none() && placement.is_none() && removed.is_empty() && asked.iter().map(|(_, _, id, u)| (*id, *u)).eq(lessons.iter().map(|l| (l.id.as_str(), l.unit()))) {
        return Err(Problem::refused(NOTHING_TO_SAVE));
    }
    // The renumbered files are what loads, but a refusal is placed in the
    // text as typed: renumbering adds `order` (and `unit`) lines, which
    // would push every line number after them one down.
    let typed = out;
    let out = renumber(&typed, &wanted).map_err(Problem::refused)?;
    if let Err(e) = ContentStore::from_files(&out) {
        let named: Vec<(String, String)> = row_paths.iter().cloned().zip(rows.iter().map(|r| r.title.trim().to_string())).collect();
        return Err(Course { files: &typed, rows: &named, before: Some(files) }.explain(&e));
    }
    let mut parts = Vec::new();
    if manifest.is_some() {
        parts.push("settings edited".to_string());
    }
    if let Some(p) = placement {
        parts.push(placement_change(old_placement.is_some(), !p.is_empty()).to_string());
    }
    let count = |n: usize, what: &str| format!("{n} {what}");
    if added > 0 {
        parts.push(count(added, "added"));
    }
    if !removed.is_empty() {
        parts.push(count(removed.len(), "removed"));
    }
    if renamed > 0 {
        parts.push(count(renamed, "renamed"));
    }
    if rewritten > 0 {
        parts.push(count(rewritten, "edited"));
    }
    let label = match parts.as_slice() {
        [] => "Rearranged the lessons".to_string(),
        [only] if manifest.is_some() && only == "settings edited" => "Saved the course settings".to_string(),
        [only] if placement.is_some() && only.starts_with("placement test") => format!("Saved the course settings ({only})"),
        _ => format!("Saved the lessons ({})", parts.join(", ")),
    };
    Ok((out, label, row_paths))
}

/// A label for several saves taken together (one editor session's Undo
/// step): what differs between `before` and `after`, lesson files compared
/// without their `order`.
pub fn summary(before: &[CourseFile], after: &[CourseFile]) -> String {
    // A lesson without what the table sets by its place (order, unit), and without its title.
    let parsed = |content: &str, keep_title: bool| {
        serde_json::from_str::<serde_json::Value>(content).ok().map(|mut v| {
            if let Some(m) = v.as_object_mut() {
                for k in ["order", "unit", "level"] {
                    m.remove(k);
                }
                if !keep_title {
                    m.remove("title");
                }
            }
            v
        })
    };
    let lesson = |files: &[CourseFile], path: &str| files.iter().find(|f| f.path == path).map(|f| f.content.clone());
    let mut parts = Vec::new();
    if lesson(before, MANIFEST_FILE) != lesson(after, MANIFEST_FILE) {
        parts.push("settings edited".to_string());
    }
    let (was, now) = (lesson(before, crate::placement::FILE), lesson(after, crate::placement::FILE));
    if was != now {
        parts.push(placement_change(was.is_some(), now.is_some()).to_string());
    }
    let (mut added, mut removed, mut renamed, mut edited, mut moved) = (0, 0, 0, 0, false);
    for f in after.iter().filter(|f| is_lesson_path(&f.path)) {
        let Some(old) = lesson(before, &f.path) else {
            added += 1;
            continue;
        };
        if old == f.content {
            continue;
        }
        let (a, b) = (parsed(&old, true), parsed(&f.content, true));
        if a.is_some() && a == b {
            moved = true;
        } else if a.is_some() && parsed(&old, false) == parsed(&f.content, false) {
            renamed += 1;
        } else {
            edited += 1;
        }
    }
    for f in before.iter().filter(|f| is_lesson_path(&f.path)) {
        if lesson(after, &f.path).is_none() {
            removed += 1;
        }
    }
    for (n, what) in [(added, "added"), (removed, "removed"), (renamed, "renamed"), (edited, "edited")] {
        if n > 0 {
            parts.push(format!("{n} {what}"));
        }
    }
    match (parts.is_empty(), moved) {
        (true, true) => "Rearranged the lessons".into(),
        (true, false) => "Your edits".into(),
        (false, true) => format!("Your edits ({}, rearranged)", parts.join(", ")),
        (false, false) => format!("Your edits ({})", parts.join(", ")),
    }
}

/// A lesson id from a title: lowercase a-z, 0-9 and '-'.
fn slug_of(title: &str) -> String {
    let mut slug = title
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .take(6)
        .collect::<Vec<_>>()
        .join("-");
    slug.truncate(48);
    let slug = slug.trim_end_matches('-').to_string();
    if slug.is_empty() { "lesson".into() } else { slug }
}

/// `base`, or `base-2`, `base-3`… whichever is free.
fn unique(base: &str, taken: impl Fn(&str) -> bool) -> String {
    (1..).map(|n| if n == 1 { base.to_string() } else { format!("{base}-{n}") }).find(|s| !taken(s)).expect("some suffix is free")
}

/// The group a new lesson in `unit` joins: that of the unit's last lesson,
/// else the manifest's first group, else the format's default.
fn group_for(lessons: &[LessonFile], store: &ContentStore, unit: &str) -> String {
    lessons
        .iter()
        .rev()
        .find(|l| l.unit() == unit)
        .and_then(|l| l.value.get("group").or_else(|| l.value.get("category")).and_then(|g| g.as_str()).map(str::to_string))
        .or_else(|| store.manifest().groups.first().map(|g| g.id.clone()))
        .unwrap_or_else(|| "general".to_string())
}

/// The smallest lesson that loads, for the learner to fill in (Edit mode).
pub fn blank_lesson(id: &str, title: &str, unit: &str, group: &str) -> String {
    let v = serde_json::json!({
        "id": id,
        "slug": id,
        "title": title,
        "unit": unit,
        "group": group,
        "summary": "",
        "content": format!("## {title}\n\n{}", crate::courses::BLANK_LESSON_TEXT),
    });
    serde_json::to_string_pretty(&v).expect("a JSON object serialises") + "\n"
}

/// A new lesson's JSON as typed in the table, with what the table leaves
/// out filled in: id and slug (Forms mode never shows them), unit and group.
fn fill_new(content: &str, id: &str, unit: &str, group: &str) -> Result<String, Problem> {
    let mut v: serde_json::Value = serde_json::from_str(content).map_err(|e| Problem::from_serde(&e, Some(content)))?;
    let obj = v.as_object_mut().ok_or_else(not_an_object)?;
    let blank = |o: &serde_json::Map<String, serde_json::Value>, k: &str| o.get(k).and_then(|x| x.as_str()).is_none_or(|x| x.trim().is_empty());
    if blank(obj, "id") {
        obj.insert("id".into(), serde_json::json!(id));
    }
    let id = obj["id"].clone();
    if blank(obj, "slug") {
        obj.insert("slug".into(), id);
    }
    if blank(obj, "unit") && blank(obj, "level") {
        obj.insert("unit".into(), serde_json::json!(unit));
    }
    if blank(obj, "group") && blank(obj, "category") {
        obj.insert("group".into(), serde_json::json!(group));
    }
    Ok(serde_json::to_string_pretty(&v).map_err(|e| Problem::refused(e.to_string()))? + "\n")
}

/// The 1-based line of the first `"key":` in a JSON text.
fn line_of_key(content: &str, key: &str) -> Option<usize> {
    let needle = format!("\"{key}\"");
    let mut from = 0;
    while let Some(i) = content[from..].find(&needle) {
        let at = from + i;
        if content[at + needle.len()..].trim_start().starts_with(':') {
            return Some(content[..at].matches('\n').count() + 1);
        }
        from = at + needle.len();
    }
    None
}

/// Every lesson in `wanted` gets its unit and `order` = its place in the list.
fn renumber(files: &[CourseFile], wanted: &[(String, String)]) -> Result<Vec<CourseFile>, String> {
    let mut out = Vec::with_capacity(files.len());
    for f in files {
        let mut content = f.content.clone();
        if is_lesson_path(&f.path) {
            let v: serde_json::Value = serde_json::from_str(&content).map_err(|e| format!("{}: {e}", f.path))?;
            let id = v.get("id").and_then(|i| i.as_str()).unwrap_or_default();
            if let Some(pos) = wanted.iter().position(|(i, _)| i == id) {
                let unit = &wanted[pos].1;
                if v.get("unit").and_then(|u| u.as_str()) != Some(unit) && v.get("level").and_then(|u| u.as_str()) != Some(unit) {
                    // Either spelling; write the current one.
                    let unit_key = if v.get("unit").is_none() && v.get("level").is_some() { "level" } else { "unit" };
                    content = patch_lesson(&content, unit_key, serde_json::json!(unit))?.unwrap_or(content);
                }
                content = patch_lesson(&content, "order", serde_json::json!(pos + 1))?.unwrap_or(content);
            }
        }
        out.push(CourseFile { path: f.path.clone(), content });
    }
    Ok(out)
}

/// The saved AI outline brought in line with `files` (lessons as they now
/// are, in order, each keeping its `focus`). `None` for a course without one.
pub fn outline_for(old: Option<&str>, files: &[CourseFile]) -> Option<String> {
    let old: serde_json::Value = serde_json::from_str(old?).ok()?;
    let focus: HashMap<String, serde_json::Value> = old
        .get("lessons")
        .and_then(|l| l.as_array())
        .into_iter()
        .flatten()
        .filter_map(|e| Some((e.get("id")?.as_str()?.to_string(), e.get("focus")?.clone())))
        .collect();
    let (_, lessons) = lessons_of(files).ok()?;
    let manifest: serde_json::Value = serde_json::from_str(&files.iter().find(|f| f.path == MANIFEST_FILE)?.content).ok()?;
    let entries: Vec<serde_json::Value> = lessons.iter().map(|l| outline_entry(&l.value, focus.get(&l.id))).collect();
    let mut outline = serde_json::json!({ "course": manifest, "lessons": entries });
    // The kind the course was written as (#953) stays with it.
    if let (Some(kind), Some(o)) = (old.get("kind"), outline.as_object_mut()) {
        o.insert("kind".into(), kind.clone());
    }
    Some(outline.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn retitle_renames_the_course_and_a_matching_subject() {
        let m = r#"{"id": "c", "title": "Old", "subject": "Old"}"#;
        let v: serde_json::Value = serde_json::from_str(&retitle(m, " New ").unwrap().unwrap()).unwrap();
        assert_eq!((v["title"].as_str(), v["subject"].as_str()), (Some("New"), Some("New")));
        let v: serde_json::Value = serde_json::from_str(&retitle(r#"{"id": "c", "title": "Old", "subject": "Maths"}"#, "New").unwrap().unwrap()).unwrap();
        assert_eq!(v["subject"].as_str(), Some("Maths"));
        assert_eq!(retitle(m, "Old").unwrap(), None);
        assert!(retitle(m, "  ").is_err());
        assert!(retitle(m, &"x".repeat(81)).is_err());
    }

    use super::*;

    fn lesson(id: &str, unit: &str, pre: &[&str]) -> CourseFile {
        CourseFile {
            path: format!("lessons/{id}.json"),
            content: serde_json::to_string_pretty(&serde_json::json!({
                "id": id, "slug": id, "title": id.to_uppercase(), "unit": unit, "group": "core",
                "content": "## One\n\nText.", "prerequisites": pre,
            }))
            .unwrap()
                + "\n",
        }
    }

    fn course() -> Vec<CourseFile> {
        let manifest = serde_json::json!({ "id": "c", "title": "C", "units": [ { "id": "1", "title": "One" }, { "id": "2", "title": "Two" } ] });
        vec![
            CourseFile { path: MANIFEST_FILE.into(), content: manifest.to_string() },
            lesson("a", "1", &[]),
            lesson("b", "1", &["a"]),
            lesson("c", "2", &["b"]),
        ]
    }

    fn order(files: &[CourseFile]) -> Vec<(String, String)> {
        lessons_of(files).unwrap().1.iter().map(|l| (l.id.clone(), l.unit().to_string())).collect()
    }

    fn w(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn places_a_new_lesson_after_another() {
        let files = course();
        let out = place(&files, lesson("n", "1", &["a"]), Some("a")).unwrap();
        assert_eq!(order(&out), w(&[("a", "1"), ("n", "1"), ("b", "1"), ("c", "2")]));
        let out = place(&files, lesson("n", "1", &[]), None).unwrap();
        assert_eq!(order(&out)[0].0, "n");
        assert!(place(&files, lesson("a", "1", &[]), None).unwrap_err().contains("taken"));
        assert!(place(&files, lesson("n", "1", &[]), Some("zz")).unwrap_err().contains("not a lesson"));
        // The loader still has the last word (an unknown unit, a missing prerequisite).
        assert!(place(&files, lesson("n", "1", &["nope"]), None).is_err());
        assert!(place(&files, lesson("n", "7", &[]), None).is_err());
    }

    #[test]
    fn only_adds_allows_new_files_and_renumbering_and_nothing_else() {
        let files = course();
        let out = place(&files, lesson("n", "1", &["a"]), Some("a")).unwrap();
        assert_eq!(only_adds(&files, &out), Ok(()));
        assert!(only_adds(&files, &files).unwrap_err().contains("adds nothing"));
        let mut changed = out.clone();
        changed[1] = lesson("a", "1", &["c"]);
        assert!(only_adds(&files, &changed).unwrap_err().contains("lessons/a.json"));
        let mut retitled = out.clone();
        retitled[0].content = retitled[0].content.replace("\"C\"", "\"Other\"");
        assert!(only_adds(&files, &retitled).unwrap_err().contains("course.json"));
        let removed: Vec<CourseFile> = out.iter().filter(|f| f.path != "lessons/c.json").cloned().collect();
        assert!(only_adds(&files, &removed).unwrap_err().contains("remove lessons/c.json"));
    }

    #[test]
    fn only_edits_one_allows_that_lesson_only() {
        let files = course();
        let rewrite = |files: &[CourseFile], path: &str, from: &str, to: &str| {
            let mut out = files.to_vec();
            let f = out.iter_mut().find(|f| f.path == path).unwrap();
            assert!(f.content.contains(from), "{}", f.content);
            f.content = f.content.replacen(from, to, 1);
            out
        };
        let edited = rewrite(&files, "lessons/b.json", "\"B\"", "\"Bb\"");
        assert_eq!(only_edits_one(&files, &edited, "lessons/b.json"), Ok(()));
        assert_eq!(only_edits_one(&files, &edited, "lessons/a.json").unwrap_err(), "it would change lessons/b.json, and only lessons/a.json may change");
        let also = rewrite(&edited, "lessons/a.json", "\"A\"", "\"Aa\"");
        assert!(only_edits_one(&files, &also, "lessons/b.json").unwrap_err().contains("lessons/a.json"));
        let renamed = rewrite(&edited, "lessons/b.json", "\"id\": \"b\"", "\"id\": \"b2\"");
        assert_eq!(only_edits_one(&files, &renamed, "lessons/b.json").unwrap_err(), "it changed the lesson's id from \"b\" to \"b2\"");
        assert!(only_edits_one(&files, &files, "lessons/b.json").unwrap_err().contains("changes no lesson"));
    }

    #[test]
    fn only_edits_allows_rewritten_lessons_and_nothing_else() {
        let files = course();
        let rewrite = |files: &[CourseFile], path: &str, from: &str, to: &str| {
            let mut out = files.to_vec();
            let f = out.iter_mut().find(|f| f.path == path).unwrap();
            f.content = f.content.replace(from, to);
            out
        };
        // New text, and a new title that leaves the lesson where it is.
        let edited = rewrite(&files, "lessons/b.json", "Text.", "Better text.");
        assert_eq!(only_edits(&files, &edited), Ok(()));
        assert_eq!(only_edits(&files, &rewrite(&edited, "lessons/a.json", "\"A\"", "\"Aa\"")), Ok(()));
        assert!(only_edits(&files, &files).unwrap_err().contains("changes no lesson"));
        // Added or removed lessons, by path.
        let added = place(&edited, lesson("n", "1", &[]), Some("a")).unwrap();
        assert_eq!(only_edits(&files, &added).unwrap_err(), "it added or removed a lesson (lessons/n.json is new)");
        let removed: Vec<CourseFile> = edited.iter().filter(|f| f.path != "lessons/c.json").cloned().collect();
        assert_eq!(only_edits(&files, &removed).unwrap_err(), "it added or removed a lesson (lessons/c.json is gone)");
        // course.json byte for byte.
        let retitled = rewrite(&edited, MANIFEST_FILE, "\"C\"", "\"Other\"");
        assert!(only_edits(&files, &retitled).unwrap_err().contains("course.json"));
        // Another unit, an `order`, or a title that sorts it elsewhere moves it.
        let unit = rewrite(&edited, "lessons/c.json", "\"unit\": \"2\"", "\"unit\": \"1\"");
        assert_eq!(only_edits(&files, &unit).unwrap_err(), "it would move lessons/c.json");
        let numbered = rewrite(&edited, "lessons/b.json", "\"group\"", "\"order\": 9, \"group\"");
        assert_eq!(only_edits(&files, &numbered).unwrap_err(), "it would move lessons/b.json");
        let sorted = rewrite(&edited, "lessons/a.json", "\"A\"", "\"Z\"");
        assert!(only_edits(&files, &sorted).unwrap_err().starts_with("it would move"));
    }

    #[test]
    fn the_outline_follows_the_files() {
        let files = course();
        let old = r#"{"course": {}, "lessons": [{"id": "a", "focus": "letters"}]}"#;
        let (out, _) = edit(&files, &[row(Some("a"), "1", "A"), row(Some("c"), "2", "C")], None).unwrap();
        let outline: serde_json::Value = serde_json::from_str(&outline_for(Some(old), &out).unwrap()).unwrap();
        let ids: Vec<&str> = outline["lessons"].as_array().unwrap().iter().map(|e| e["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["a", "c"]);
        assert_eq!(outline["lessons"][0]["focus"], "letters");
        assert!(outline_for(None, &out).is_none());
    }

    #[test]
    fn a_refusal_names_the_line_as_typed_not_as_renumbered() {
        // The lesson has no "order" yet; the save adds one above
        // "prerequisites", but the line is the one in the box.
        let files = course();
        let typed = lesson("c", "2", &["zzz"]).content;
        let want = typed.lines().position(|l| l.contains("\"zzz\"")).unwrap() + 1;
        let mut rows = vec![row(Some("a"), "1", "A"), row(Some("b"), "1", "B"), row(Some("c"), "2", "C")];
        rows[2].content = Some(typed.clone());
        let p = edit_rows(&files, &rows, None, None).err().unwrap();
        assert_eq!((p.field.as_deref(), p.line, p.row), (Some("prerequisites"), Some(want), Some(2)), "{}", p.text);
        let renumbered = renumber(&[CourseFile { path: "lessons/c.json".into(), content: typed }], &w(&[("c", "2")])).unwrap();
        assert_ne!(renumbered[0].content.lines().position(|l| l.contains("\"zzz\"")).unwrap() + 1, want, "the test must cover a shifted line");
    }

    fn row(key: Option<&str>, unit: &str, title: &str) -> Edit {
        Edit { key: key.map(str::to_string), unit: unit.into(), title: title.into(), content: None }
    }

    #[test]
    fn edits_the_table_in_one_go() {
        let files = course();
        // b removed, c moved to unit 1 first, a renamed, a new lesson at the end of unit 2.
        let rows = [row(Some("c"), "1", "C"), row(Some("a"), "1", "Ay"), row(None, "2", "Fresh one")];
        let (out, label) = edit(&files, &rows, None).unwrap();
        assert_eq!(order(&out), w(&[("c", "1"), ("a", "1"), ("fresh-one", "2")]));
        assert_eq!(label, "Saved the lessons (1 added, 1 removed, 1 renamed)");
        let a = out.iter().find(|f| f.path == "lessons/a.json").unwrap();
        assert!(a.content.contains("\"title\": \"Ay\""), "{}", a.content);
        // c named b as a prerequisite; b is gone, so is the prerequisite.
        let c = out.iter().find(|f| f.path == "lessons/c.json").unwrap();
        assert!(!c.content.contains("\"b\""), "{}", c.content);
        let new = out.iter().find(|f| f.path == "lessons/fresh-one.json").unwrap();
        assert!(new.content.contains("\"group\": \"core\""), "joins its unit's group: {}", new.content);
        assert!(!out.iter().any(|f| f.path == "lessons/b.json"));
    }

    #[test]
    fn a_new_lesson_typed_without_ids_gets_them() {
        let files = course();
        let mut fresh = row(None, "2", "Fresh one");
        fresh.content = Some(serde_json::json!({ "id": "", "title": "Fresh one", "summary": "Typed first", "content": "## Fresh one\n\nText." }).to_string());
        let (out, _) = edit(&files, &[row(Some("a"), "1", "A"), row(Some("b"), "1", "B"), row(Some("c"), "2", "C"), fresh], None).unwrap();
        let new: serde_json::Value = serde_json::from_str(&out.iter().find(|f| f.path == "lessons/fresh-one.json").unwrap().content).unwrap();
        assert_eq!((new["id"].as_str(), new["slug"].as_str(), new["unit"].as_str(), new["group"].as_str()), (Some("fresh-one"), Some("fresh-one"), Some("2"), Some("core")));
        assert_eq!(new["summary"], "Typed first");
    }

    #[test]
    fn a_reorder_alone_is_labelled_so_and_nothing_changed_is_refused() {
        let files = course();
        let (out, label) = edit(&files, &[row(Some("b"), "1", "B"), row(Some("a"), "1", "A"), row(Some("c"), "2", "C")], None).unwrap();
        assert_eq!(label, "Rearranged the lessons");
        assert_eq!(order(&out)[0].0, "b");
        let same = [row(Some("a"), "1", "A"), row(Some("b"), "1", "B"), row(Some("c"), "2", "C")];
        assert!(edit(&files, &same, None).unwrap_err().contains("Nothing to save"));
    }

    #[test]
    fn edited_json_is_saved_and_the_name_in_the_table_wins() {
        let files = course();
        let mut v: serde_json::Value = serde_json::from_str(&files[1].content).unwrap();
        v["summary"] = serde_json::json!("New summary");
        let mut a = row(Some("a"), "1", "Renamed");
        a.content = Some(serde_json::to_string_pretty(&v).unwrap());
        let (out, label) = edit(&files, &[a, row(Some("b"), "1", "B"), row(Some("c"), "2", "C")], None).unwrap();
        assert_eq!(label, "Saved the lessons (1 renamed, 1 edited)");
        let a = out.iter().find(|f| f.path == "lessons/a.json").unwrap();
        assert!(a.content.contains("New summary") && a.content.contains("\"Renamed\""), "{}", a.content);
        let mut bad = row(Some("a"), "1", "A");
        bad.content = Some("{ nope".into());
        assert!(edit(&files, &[bad, row(Some("b"), "1", "B"), row(Some("c"), "2", "C")], None).unwrap_err().starts_with("Not valid JSON in “A”"));
    }

    #[test]
    fn new_lessons_get_free_ids_and_paths() {
        let files = course();
        // "A" would be id "a", taken; two new rows with the same name get -2 and -3.
        let rows = [row(Some("a"), "1", "A"), row(Some("b"), "1", "B"), row(Some("c"), "2", "C"), row(None, "1", "A"), row(None, "1", "A!")];
        let (out, _) = edit(&files, &rows, None).unwrap();
        let ids: Vec<String> = order(&out).into_iter().map(|(i, _)| i).collect();
        assert_eq!(ids, ["a", "b", "a-2", "a-3", "c"]);
        assert!(out.iter().any(|f| f.path == "lessons/a-2.json") && out.iter().any(|f| f.path == "lessons/a-3.json"));
    }

    #[test]
    fn table_refusals() {
        let files = course();
        assert!(edit(&files, &[], None).unwrap_err().contains("at least one lesson"));
        assert!(edit(&files, &[row(Some("zz"), "1", "Z")], None).unwrap_err().contains("no lesson 'zz'"));
        assert!(edit(&files, &[row(Some("a"), "1", "A"), row(Some("a"), "1", "A")], None).unwrap_err().contains("twice"));
        assert!(edit(&files, &[row(Some("a"), "1", "  ")], None).unwrap_err().contains("no name"));
        assert!(edit(&files, &[row(Some("a"), "9", "A")], None).unwrap_err().contains("there is no unit '9' in the course settings"));
        // A new lesson whose JSON reuses a kept lesson's id.
        let mut dup = row(None, "1", "Dup");
        dup.content = Some(blank_lesson("a", "Dup", "1", "core"));
        assert!(edit(&files, &[row(Some("a"), "1", "A"), dup], None).unwrap_err().contains("both use the id"));
    }

    #[test]
    fn course_settings_save_with_the_table_and_may_move_units() {
        let files = course();
        let rows = [row(Some("a"), "1", "A"), row(Some("b"), "1", "B"), row(Some("c"), "2", "C")];
        // A third unit, and the title: settings alone.
        let three = serde_json::json!({ "id": "c", "title": "Cee", "units": [ { "id": "1", "title": "One" }, { "id": "2", "title": "Two" }, { "id": "3", "title": "Three" } ] }).to_string();
        let (out, label) = edit(&files, &rows, Some(&three)).unwrap();
        assert_eq!(label, "Saved the course settings");
        assert_eq!(out.iter().find(|f| f.path == MANIFEST_FILE).unwrap().content, three);
        // Unit 2 dropped from the settings: its lesson has to move in the same save.
        let one = serde_json::json!({ "id": "c", "title": "C", "units": [ { "id": "1", "title": "One" } ] }).to_string();
        assert!(edit(&files, &rows, Some(&one)).unwrap_err().contains("no unit '2'"));
        let moved = [row(Some("a"), "1", "A"), row(Some("b"), "1", "B"), row(Some("c"), "1", "C")];
        let (out, label) = edit(&files, &moved, Some(&one)).unwrap();
        assert_eq!(label, "Saved the course settings");
        assert_eq!(order(&out), w(&[("a", "1"), ("b", "1"), ("c", "1")]));
        // Units in a new order take their lessons along.
        let swapped = serde_json::json!({ "id": "c", "title": "C", "units": [ { "id": "2", "title": "Two" }, { "id": "1", "title": "One" } ] }).to_string();
        let (out, _) = edit(&files, &[row(Some("c"), "2", "C"), row(Some("a"), "1", "A"), row(Some("b"), "1", "B")], Some(&swapped)).unwrap();
        assert_eq!(order(&out), w(&[("c", "2"), ("a", "1"), ("b", "1")]));
        // The same settings are no change; a new id or broken JSON is refused.
        let same = &files[0].content;
        assert!(edit(&files, &rows, Some(same)).unwrap_err().contains("Nothing to save"));
        assert!(edit(&files, &rows, Some(&three.replace("\"c\"", "\"d\""))).unwrap_err().contains("must stay"));
        assert!(edit(&files, &rows, Some("{ nope")).unwrap_err().starts_with("Not valid JSON in the course settings, line 1"));
    }

    #[test]
    fn a_session_of_saves_is_summed_up() {
        let f = |p: &str, c: &str| CourseFile { path: p.into(), content: c.into() };
        let before = vec![f("course.json", "{}"), f("lessons/a.json", r#"{"id":"a","order":1}"#), f("lessons/b.json", r#"{"id":"b","order":2}"#)];
        let swapped = vec![f("course.json", "{}"), f("lessons/a.json", r#"{"id":"a","order":2}"#), f("lessons/b.json", r#"{"id":"b","order":1}"#)];
        assert_eq!(summary(&before, &swapped), "Rearranged the lessons");
        assert_eq!(summary(&before, &before), "Your edits");
        let after = vec![f("course.json", r#"{"t":1}"#), f("lessons/a.json", r#"{"id":"a","order":1,"title":"x"}"#), f("lessons/c.json", "{}")];
        assert_eq!(summary(&before, &after), "Your edits (settings edited, 1 added, 1 removed, 1 renamed)");
        let moved = vec![f("course.json", "{}"), f("lessons/a.json", r#"{"id":"a","order":2,"unit":"2","summary":"s"}"#), f("lessons/b.json", r#"{"id":"b","order":1}"#)];
        assert_eq!(summary(&before, &moved), "Your edits (1 edited, rearranged)");
    }
}
