//! The listed-term check (#1033): `data/moderation/terms.txt`, built into
//! the binary, matched whole word, case-insensitive and accent-folded over
//! every text field of every file of a made course.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::Match;
use crate::content::CourseFile;

/// The list as it is in the repository.
const LIST: &str = include_str!("../../data/moderation/terms.txt");

/// The version of the term list (#1039). Add 1 for a change that could flip
/// a verdict (a term added or taken out, a new matching rule) and add a line
/// to docs/content-check-versions.md; a typo in a comment needs none. Every
/// term check saves the version it ran under (`course_flags.terms_version`),
/// and the nightly run checks every course on an older one again
/// (`content_check::nightly`). CI fails a PR that changes terms.txt without
/// a bump unless its body says `moderation-version: unchanged`
/// (scripts/moderation-version-lint.sh).
pub const TERMS_VERSION: i64 = 1;

/// At most this many matches are kept for a course: enough for its owner
/// to find them, and the row stays small.
const MAX_MATCHES: usize = 50;

/// Words of context kept on each side of a match.
const CONTEXT_WORDS: usize = 6;

struct Terms {
    /// The terms by their first folded word: (all folded words, the term as listed).
    by_first: HashMap<String, Vec<(Vec<String>, String)>>,
    hash: String,
}

fn terms() -> &'static Terms {
    static TERMS: OnceLock<Terms> = OnceLock::new();
    TERMS.get_or_init(|| parse(LIST))
}

fn parse(list: &str) -> Terms {
    let mut by_first: HashMap<String, Vec<(Vec<String>, String)>> = HashMap::new();
    for line in list.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let words: Vec<String> = words(line).into_iter().map(|(_, _, w)| w).collect();
        if let Some(first) = words.first() {
            by_first.entry(first.clone()).or_default().push((words, line.to_string()));
        }
    }
    let hash = Sha256::digest(list.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
    Terms { by_first, hash }
}

/// SHA-256 (hex) of the list: a new list checks every course again.
pub fn list_hash() -> &'static str {
    &terms().hash
}

/// A character lower-cased with its accent taken off.
fn fold(c: char) -> impl Iterator<Item = char> {
    c.to_lowercase().map(|l| match l {
        'á' | 'à' | 'ä' | 'â' | 'ã' | 'å' | 'ā' => 'a',
        'é' | 'è' | 'ë' | 'ê' | 'ē' => 'e',
        'í' | 'ì' | 'ï' | 'î' | 'ī' => 'i',
        'ó' | 'ò' | 'ö' | 'ô' | 'õ' | 'ø' | 'ō' => 'o',
        'ú' | 'ù' | 'ü' | 'û' | 'ū' => 'u',
        'ý' | 'ÿ' => 'y',
        'ñ' => 'n',
        'ç' => 'c',
        'ß' => 's',
        other => other,
    })
}

/// The words of `text`: (start byte, end byte, folded word). A word is a run
/// of letters and digits; anything else separates words.
fn words(text: &str) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    let mut cur: Option<(usize, String)> = None;
    for (i, c) in text.char_indices() {
        if c.is_alphanumeric() {
            cur.get_or_insert_with(|| (i, String::new())).1.extend(fold(c));
        } else if let Some((start, w)) = cur.take() {
            out.push((start, i, w));
        }
    }
    if let Some((start, w)) = cur {
        out.push((start, text.len(), w));
    }
    out
}

/// The listed terms in one text: (term as listed, text around it).
fn find(text: &str) -> Vec<(String, String)> {
    let terms = terms();
    let ws = words(text);
    let mut out = Vec::new();
    for (i, (_, _, w)) in ws.iter().enumerate() {
        let Some(candidates) = terms.by_first.get(w) else { continue };
        for (term, listed) in candidates {
            let window = ws.get(i..i + term.len());
            if window.is_some_and(|win| win.iter().zip(term).all(|((_, _, a), b)| a == b)) {
                let from = ws.get(i.saturating_sub(CONTEXT_WORDS)).map_or(0, |w| w.0);
                let to = ws.get((i + term.len() + CONTEXT_WORDS - 1).min(ws.len() - 1)).map_or(text.len(), |w| w.1);
                let mut around = text.get(from..to).unwrap_or_default().to_string();
                if from > 0 {
                    around.insert_str(0, "...");
                }
                if to < text.len() {
                    around.push_str("...");
                }
                out.push((listed.clone(), around));
            }
        }
    }
    out
}

/// Every string of a JSON value with its pointer (`/sections/0/body`).
fn strings<'a>(v: &'a Value, at: String, out: &mut Vec<(String, &'a str)>) {
    match v {
        Value::String(s) => out.push((at, s)),
        Value::Array(a) => a.iter().enumerate().for_each(|(i, x)| strings(x, format!("{at}/{i}"), out)),
        Value::Object(o) => o.iter().for_each(|(k, x)| strings(x, format!("{at}/{}", k.replace('~', "~0").replace('/', "~1")), out)),
        _ => {}
    }
}

/// The listed terms in a course's files, one match per term and field.
pub fn scan(files: &[CourseFile]) -> Vec<Match> {
    let mut out: Vec<Match> = Vec::new();
    for f in files {
        let json: Option<Value> = serde_json::from_str(&f.content).ok();
        let mut fields = Vec::new();
        match &json {
            Some(v) => strings(v, String::new(), &mut fields),
            None => fields.push((String::new(), f.content.as_str())),
        }
        let lesson = json.as_ref().and_then(|v| v.get("title")).and_then(Value::as_str).unwrap_or(&f.path).to_string();
        for (field, text) in fields {
            for (term, around) in find(text) {
                if out.len() >= MAX_MATCHES {
                    return out;
                }
                if !out.iter().any(|m| m.term == term && m.path == f.path && m.field == field) {
                    out.push(Match { term, path: f.path.clone(), field: field.clone(), lesson: lesson.clone(), text: around });
                }
            }
        }
    }
    out
}
