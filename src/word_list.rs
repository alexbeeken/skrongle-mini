//! "Paste a word list" (#453): pasted lines such as `hola — hello`,
//! `hola, hello` or a tab-separated spreadsheet export turned into a
//! lesson's vocabulary, with no model involved. The lesson editor shows the
//! pairs (and the lines it could not read) before the lesson is made, and
//! the new lesson goes in through the lesson table's normal save.

use serde::Serialize;

use crate::content::Manifest;

/// Most pairs one paste makes into a lesson.
pub const MAX_WORDS: usize = 500;
/// Longest paste read, in bytes (the lesson file must stay under the file cap).
pub const MAX_TEXT_BYTES: usize = 64 * 1024;

/// One pair read from a line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Pair {
    pub term: String,
    pub meaning: String,
}

/// Why a line was not taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Skip {
    /// No separator, or one side is empty.
    NoSplit,
    /// The term is already in the list (terms are unique in a lesson).
    Duplicate,
    /// Past `MAX_WORDS`.
    TooMany,
}

/// A line that was not taken, 1-based, as pasted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Skipped {
    pub line: usize,
    pub text: String,
    pub why: Skip,
}

#[derive(Debug, Default, PartialEq, Eq, Serialize)]
pub struct Parsed {
    pub rows: Vec<Pair>,
    pub skipped: Vec<Skipped>,
}

/// The separators tried on a line, in this order; a line is split at the
/// first one it holds. A tab comes first (spreadsheets, Anki and Quizlet
/// exports), then dashes and `=`, then `;`, `,` and `: `.
const SEPARATORS: &[&str] = &["\t", " — ", "—", " – ", "–", " - ", " = ", "=", ";", ",", ": "];

/// Read `text` line by line. Blank lines are passed over; a first line
/// that only names the two columns (the course's term and meaning labels,
/// or "term" and "meaning") is a header and passed over too.
pub fn parse(text: &str, manifest: &Manifest) -> Parsed {
    let mut out = Parsed::default();
    let mut first = true;
    for (i, raw) in text.lines().enumerate() {
        let line = strip_bullet(raw.trim());
        if line.is_empty() {
            continue;
        }
        let pair = split(line);
        if std::mem::take(&mut first) && pair.as_ref().is_some_and(|p| is_header(p, manifest)) {
            continue;
        }
        let skip = |why| Skipped { line: i + 1, text: raw.trim().to_string(), why };
        match pair {
            None => out.skipped.push(skip(Skip::NoSplit)),
            Some(p) if out.rows.iter().any(|r| r.term.to_lowercase() == p.term.to_lowercase()) => out.skipped.push(skip(Skip::Duplicate)),
            Some(_) if out.rows.len() >= MAX_WORDS => out.skipped.push(skip(Skip::TooMany)),
            Some(p) => out.rows.push(p),
        }
    }
    out
}

/// `- `, `* `, `• `, `1. ` or `1) ` in front of a list item.
fn strip_bullet(line: &str) -> &str {
    for b in ["- ", "* ", "• "] {
        if let Some(rest) = line.strip_prefix(b) {
            return rest.trim_start();
        }
    }
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && digits <= 4 {
        let rest = line.get(digits..).unwrap_or_default();
        if let Some(rest) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            return rest.trim_start();
        }
    }
    line
}

fn split(line: &str) -> Option<Pair> {
    let (term, meaning) = SEPARATORS.iter().find_map(|s| {
        let (a, b) = line.split_once(s)?;
        // A tab-separated row may carry more columns (an example, tags): the first two count.
        let b = if *s == "\t" { b.split('\t').next().unwrap_or_default() } else { b };
        Some((a, b))
    })?;
    let (term, meaning) = (unquote(term), unquote(meaning));
    if term.is_empty() || meaning.is_empty() {
        return None;
    }
    Some(Pair { term: term.to_string(), meaning: meaning.to_string() })
}

/// A side as a CSV export writes it: trimmed, one pair of double quotes off.
fn unquote(s: &str) -> &str {
    let s = s.trim();
    s.strip_prefix('"').and_then(|s| s.strip_suffix('"')).map(str::trim).unwrap_or(s)
}

fn is_header(p: &Pair, m: &Manifest) -> bool {
    let same = |a: &str, b: &str| !b.trim().is_empty() && a.to_lowercase() == b.trim().to_lowercase();
    (same(&p.term, &m.terms.label) && same(&p.meaning, &m.meanings.label)) || (same(&p.term, "term") && same(&p.meaning, "meaning"))
}

/// A new lesson's JSON for the lesson table to save: the title, the pairs
/// as its vocabulary, and a Markdown table of them (headed with the
/// course's labels) as its text. The table's save fills in id, slug, unit
/// and group, as for any new lesson.
pub fn lesson(title: &str, rows: &[Pair], manifest: &Manifest) -> String {
    let cell = |s: &str| s.replace('|', "\\|");
    let mut content = format!("## {}\n\n| {} | {} |\n|---|---|\n", title.trim(), cell(manifest.terms.label.trim()), cell(manifest.meanings.label.trim()));
    for r in rows {
        content.push_str(&format!("| {} | {} |\n", cell(&r.term), cell(&r.meaning)));
    }
    let vocabulary: Vec<serde_json::Value> = rows.iter().map(|r| serde_json::json!({ "term": r.term, "meaning": r.meaning })).collect();
    let v = serde_json::json!({
        "title": title.trim(),
        "summary": "",
        "content": content,
        "vocabulary": vocabulary,
    });
    serde_json::to_string_pretty(&v).expect("a JSON object serialises") + "\n" // panic-ok: a json! object of strings always serialises
}

#[cfg(test)]
mod tests;
