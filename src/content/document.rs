//! A course made from one pasted document (#232, "Understand a document"):
//! the manifest's `document`, a lesson's `sources` and a quiz item's
//! `source`. The quote check lives here, in the course loader, so a
//! hand-written course, the editor, the AI job and the connected apps all
//! meet the same rule: a quote counts only when code finds it word for word
//! in the document. Whitespace does not count, and a quote that starts or
//! ends with a letter or digit must start or end on a word edge of the
//! document ("0 mg" is not found in "10 mg").
//!
//! In a course without a `document` the fields are kept but nothing checks or
//! shows them.

use serde::{Deserialize, Serialize};

use super::{strip_inline_markdown, ContentError, Lesson, Manifest, Section, MANIFEST_FILE};

/// Most characters of pasted text a document course keeps (about twelve
/// pages). At the worst JSON escaping (`\u0000`, six bytes a character) that
/// is 180 KB, far below a course file's cap (`courses::MAX_FILE_BYTES`, 1 MB).
pub const MAX_DOCUMENT_CHARS: usize = 30_000;

/// One quote for one section of a lesson's text: `section` names a `## `
/// heading the way a Guide step does (the start of it, any case), `quote` is
/// the document's own words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub section: String,
    pub quote: String,
}

/// Is `c` a character that runs on into the next one inside a word? Letters
/// and digits, except the scripts written without spaces (Chinese, Japanese),
/// where any character may start a quote.
fn word_char(c: char) -> bool {
    c.is_alphanumeric() && !unspaced(c)
}

/// A character of a script written without spaces between words.
fn unspaced(c: char) -> bool {
    matches!(c as u32, 0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0xFF66..=0xFF9F | 0x20000..=0x3134F)
}

/// `text` with whitespace set aside: every run of it is one space, none at
/// the ends, and none between two characters of a script written without
/// spaces (a line break inside a Chinese sentence).
pub fn squash(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut out = String::with_capacity(text.len());
    for (i, w) in words.iter().enumerate() {
        let joined = i > 0 && out.chars().next_back().is_some_and(unspaced) && w.chars().next().is_some_and(unspaced);
        if i > 0 && !joined {
            out.push(' ');
        }
        out.push_str(w);
    }
    out
}

/// Is `quote` in `document` word for word, whitespace aside? Both are
/// squashed first. An empty quote is never found.
pub fn quote_found(document: &str, quote: &str) -> bool {
    found_in(&squash(document), &squash(quote))
}

/// `quote_found` on texts already squashed (the job checks many quotes
/// against one document).
fn found_in(doc: &str, quote: &str) -> bool {
    if quote.is_empty() {
        return false;
    }
    let first = quote.chars().next().is_some_and(word_char);
    let last = quote.chars().next_back().is_some_and(word_char);
    let mut from = 0;
    while let Some(at) = doc.get(from..).and_then(|rest| rest.find(quote)) {
        let start = from + at;
        let end = start + quote.len();
        let before = doc[..start].chars().next_back().is_some_and(word_char);
        let after = doc[end..].chars().next().is_some_and(word_char);
        if !(first && before) && !(last && after) {
            return true;
        }
        // The next match may overlap this one: go on one character later.
        from = start + doc[start..].chars().next().map_or(1, char::len_utf8);
    }
    false
}

/// Does `section` (a `sources[].section`) name the section headed `heading`?
/// As a Guide step names one: the start of the heading, any case.
pub fn names_section(section: &str, heading: &str) -> bool {
    let needle = strip_inline_markdown(section.trim()).to_lowercase();
    !needle.is_empty() && heading.to_lowercase().starts_with(&needle)
}

/// The quotes of `lesson` for the section headed `heading`, in order.
pub fn quotes_for<'a>(lesson: &'a Lesson, heading: &str) -> Vec<&'a str> {
    lesson.sources.iter().filter(|s| names_section(&s.section, heading)).map(|s| s.quote.as_str()).collect()
}

/// The manifest's part of the rule: a document within the cap.
pub(super) fn check_manifest(m: &Manifest) -> Result<(), ContentError> {
    let n = m.document.chars().count();
    if n > MAX_DOCUMENT_CHARS {
        return Err(ContentError::Invalid(format!("{MANIFEST_FILE}: document has {n} characters; at most {MAX_DOCUMENT_CHARS} are allowed")));
    }
    Ok(())
}

/// A lesson's part, once its sections are split: every `sources` entry names
/// a section of it and has a quote; with a document, every quote of it (and
/// every quiz item's `source`) is found there.
pub(super) fn check_lesson(lesson: &Lesson, m: &Manifest, path: &str) -> Result<(), ContentError> {
    let ctx = |msg: String| ContentError::Invalid(format!("{path}: {msg}"));
    for s in &lesson.sources {
        if s.quote.trim().is_empty() {
            return Err(ctx(format!("sources: the entry for section '{}' has no quote", s.section)));
        }
        if !lesson.sections.iter().any(|sec: &Section| names_section(&s.section, &sec.heading)) {
            return Err(ctx(format!("sources: no section starting with '{}'", s.section)));
        }
    }
    if m.document.trim().is_empty() {
        return Ok(());
    }
    let doc = squash(&m.document);
    for s in &lesson.sources {
        if !found_in(&doc, &squash(&s.quote)) {
            return Err(ctx(format!("sources: the quote for section '{}' is not in the course's document word for word: \"{}\"", s.section, s.quote.trim())));
        }
    }
    for q in lesson.quiz.iter().filter(|q| !q.source.trim().is_empty()) {
        if !found_in(&doc, &squash(&q.source)) {
            return Err(ctx(format!("question '{}': its source is not in the course's document word for word: \"{}\"", q.question_id, q.source.trim())));
        }
    }
    Ok(())
}

/// What `keep_quoted` took out of a lesson.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Dropped {
    pub questions: usize,
    pub sections: usize,
}

/// The AI job's side of the rule, on a lesson as the model wrote it: every
/// quiz item whose `source` is missing or not found in `document` goes, and
/// so does every `## ` section of `content` without a found quote (and any
/// text before the first heading), with the `sources` entries that are not
/// found or name a section that went. `keep_last`: the lesson's last section
/// stays without a quote (the course's last section, *Questions to ask*,
/// is about what the document does not say). A hand-ordered `guide` goes
/// too: it may name what was dropped, and the Guide is derived without it.
pub fn keep_quoted(lesson: &mut serde_json::Value, document: &str, keep_last: bool) -> Dropped {
    let doc = squash(document);
    let found = |q: Option<&str>| q.is_some_and(|q| found_in(&doc, &squash(q)));
    let mut dropped = Dropped::default();
    let Some(obj) = lesson.as_object_mut() else { return dropped };
    obj.remove("guide");
    if let Some(quiz) = obj.get_mut("quiz").and_then(|q| q.as_array_mut()) {
        let before = quiz.len();
        quiz.retain(|q| found(q.get("source").and_then(|s| s.as_str())));
        dropped.questions = before - quiz.len();
    }
    let mut sources: Vec<Source> = obj
        .get("sources")
        .and_then(|s| serde_json::from_value::<Vec<Source>>(s.clone()).ok())
        .unwrap_or_default();
    sources.retain(|s| found(Some(&s.quote)));
    let content = obj.get("content").and_then(|c| c.as_str()).unwrap_or_default().to_string();
    let parts = markdown_parts(&content);
    let count = parts.len();
    let mut kept = Vec::new();
    for (i, (heading, text)) in parts.into_iter().enumerate() {
        let quoted = heading.as_deref().is_some_and(|h| sources.iter().any(|s| names_section(&s.section, &strip_inline_markdown(h.trim()))));
        let last = keep_last && heading.is_some() && i + 1 == count;
        if quoted || last {
            kept.push((heading, text));
        } else if heading.is_some() || !text.trim().is_empty() {
            dropped.sections += 1;
        }
    }
    let headings: Vec<String> = kept.iter().filter_map(|(h, _)| h.as_deref().map(|h| strip_inline_markdown(h.trim()))).collect();
    sources.retain(|s| headings.iter().any(|h| names_section(&s.section, h)));
    let content: String = kept.iter().map(|(h, text)| format!("## {}\n{text}", h.as_deref().unwrap_or_default().trim())).collect();
    obj.insert("content".into(), content.trim().to_string().into());
    obj.insert("sources".into(), serde_json::to_value(&sources).unwrap_or_default());
    dropped
}

/// `content` cut at its `## ` headings (outside code blocks), as `split_sections`
/// cuts it: the text before the first heading (no heading), then each heading
/// with the text under it.
fn markdown_parts(md: &str) -> Vec<(Option<String>, String)> {
    let mut parts = vec![(None, String::new())];
    let mut in_code = false;
    for line in md.lines() {
        if line.trim_start().starts_with("```") {
            in_code = !in_code;
        }
        match line.strip_prefix("## ").filter(|_| !in_code) {
            Some(h) => parts.push((Some(h.to_string()), String::new())),
            None => {
                if let Some((_, text)) = parts.last_mut() {
                    text.push_str(line);
                    text.push('\n');
                }
            }
        }
    }
    if parts.first().is_some_and(|(_, t)| t.trim().is_empty()) {
        parts.remove(0);
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEAFLET: &str = "Take 1 tablet twice a day,\nwith food.\n\nDo not take more than 10 mg  in 24 hours.\n“Keep out of the reach of children.”\n每天服用\n两次。";

    #[test]
    fn a_quote_is_found_word_for_word() {
        assert!(quote_found(LEAFLET, "Take 1 tablet twice a day, with food."));
        assert!(quote_found(LEAFLET, "Do not take more than 10 mg in 24 hours."));
        assert!(quote_found(LEAFLET, "Keep out of the reach of children."), "quotation marks around it are punctuation");
    }

    #[test]
    fn whitespace_does_not_count() {
        assert!(quote_found(LEAFLET, "  Take 1 tablet\ttwice a day,\n\n with food. "));
        assert!(quote_found(LEAFLET, "more than 10 mg in 24"));
        assert!(quote_found(LEAFLET, "每天服用两次。"), "a line break inside a Chinese sentence is whitespace too");
        assert_eq!(squash(" a \n\n b\u{a0}c "), "a b c");
    }

    #[test]
    fn a_changed_or_partial_word_is_not_found() {
        assert!(!quote_found(LEAFLET, "Take 2 tablets twice a day, with food."));
        assert!(!quote_found(LEAFLET, "take 1 tablet twice a day"), "case counts");
        assert!(!quote_found(LEAFLET, "0 mg in 24 hours"), "a quote may not start inside a number");
        assert!(!quote_found(LEAFLET, "more than 10 m"), "or end inside a word");
        assert!(!quote_found(LEAFLET, "Takewith"), "whitespace aside is not whitespace removed between words");
        assert!(!quote_found(LEAFLET, ""));
        assert!(!quote_found(LEAFLET, "  \n "));
        assert!(!quote_found("", "Take"));
    }

    #[test]
    fn a_quote_from_another_document_is_not_found() {
        let lease = "The tenant pays the rent on the first day of each month.";
        assert!(quote_found(lease, "The tenant pays the rent on the first day of each month."));
        assert!(!quote_found(LEAFLET, "The tenant pays the rent on the first day of each month."));
        assert!(!quote_found(lease, "Take 1 tablet twice a day, with food."));
    }

    #[test]
    fn an_overlapping_match_is_still_found() {
        // The first "10 mg" sits inside "110 mg"; the second is whole.
        assert!(quote_found("Not 110 mg. Take 10 mg.", "10 mg."));
    }

    /// A one-lesson course with `document` and the lesson's quotes.
    fn course(document: &str, section: &str, quote: &str, source: &str) -> Result<crate::content::ContentStore, ContentError> {
        use crate::content::CourseFile;
        let manifest = serde_json::json!({ "id": "doc", "title": "Doc", "units": [{ "id": "1", "title": "One" }], "document": document });
        let lesson = serde_json::json!({
            "id": "l1", "slug": "l1", "title": "L", "unit": "1",
            "content": "## What to take\n\nOne tablet twice a day.",
            "sources": [{ "section": section, "quote": quote }],
            "quiz": [{ "question_id": "q1", "prompt": "How many?", "type": "multiple-choice", "options": ["1", "2"], "correct_answer": "1", "source": source }]
        });
        crate::content::ContentStore::from_files(&[
            CourseFile { path: MANIFEST_FILE.into(), content: manifest.to_string() },
            CourseFile { path: "lessons/l1.json".into(), content: lesson.to_string() },
        ])
    }

    #[test]
    fn the_validator_checks_every_quote_against_the_document() {
        let take = "Take 1 tablet twice a day, with food.";
        let store = course(LEAFLET, "what to", take, "Do not take more than 10 mg in 24 hours.").expect("quotes found");
        assert_eq!(quotes_for(&store.lessons()[0], "What to take"), vec![take]);
        assert_eq!(store.lessons()[0].quiz[0].source, "Do not take more than 10 mg in 24 hours.");

        let err = course(LEAFLET, "What to take", "Take 2 tablets.", "").unwrap_err().to_string();
        assert!(err.contains("lessons/l1.json") && err.contains("not in the course's document word for word"), "{err}");
        let err = course(LEAFLET, "What to take", take, "Take three tablets at bedtime.").unwrap_err().to_string();
        assert!(err.contains("question 'q1'"), "{err}");
        let err = course(LEAFLET, "Side effects", take, "").unwrap_err().to_string();
        assert!(err.contains("no section starting with 'Side effects'"), "{err}");
        let err = course(LEAFLET, "What to take", " ", "").unwrap_err().to_string();
        assert!(err.contains("has no quote"), "{err}");

        // Without a document nothing is checked against one.
        assert!(course("", "What to take", "Anything at all.", "Anything else.").is_ok());
        // A document over the cap is refused, at the cap it loads.
        assert!(course(&"x".repeat(MAX_DOCUMENT_CHARS), "What to take", "x", "").is_err(), "a part of a word is not a quote");
        let err = course(&"x ".repeat(MAX_DOCUMENT_CHARS / 2 + 1), "What to take", "x", "").unwrap_err().to_string();
        assert!(err.contains("course.json: document has"), "{err}");
        assert!(course(&"x ".repeat(MAX_DOCUMENT_CHARS / 2), "What to take", "x", "").is_ok());
    }

    #[test]
    fn the_cap_keeps_a_document_well_inside_a_course_file() {
        assert!(MAX_DOCUMENT_CHARS * 6 < crate::courses::MAX_FILE_BYTES / 2);
    }

    #[test]
    fn keep_quoted_drops_what_is_not_found() {
        let mut lesson = serde_json::json!({
            "id": "l1", "slug": "l1", "title": "L", "unit": "1",
            "content": "Intro without a heading.\n\n## What to take\n\nOne tablet.\n\n## Made up\n\nInvented.\n\n## Questions to ask\n\n- How long?",
            "sources": [
                { "section": "What to take", "quote": "Take 1 tablet twice a day, with food." },
                { "section": "Made up", "quote": "Take three tablets at bedtime." }
            ],
            "quiz": [
                { "question_id": "q1", "source": "Do not take more than 10 mg in 24 hours." },
                { "question_id": "q2", "source": "Take three tablets at bedtime." },
                { "question_id": "q3" }
            ],
            "guide": [{ "question": "q2" }]
        });
        let dropped = keep_quoted(&mut lesson, LEAFLET, true);
        assert_eq!(dropped, Dropped { questions: 2, sections: 2 });
        assert_eq!(lesson["content"], "## What to take\n\nOne tablet.\n\n## Questions to ask\n\n- How long?");
        assert_eq!(lesson["sources"], serde_json::json!([{ "section": "What to take", "quote": "Take 1 tablet twice a day, with food." }]));
        assert_eq!(lesson["quiz"].as_array().unwrap().len(), 1);
        assert!(lesson.get("guide").is_none());

        // Without `keep_last` the unquoted last section goes as well.
        let mut again = lesson.clone();
        assert_eq!(keep_quoted(&mut again, LEAFLET, false).sections, 1);
        assert_eq!(again["content"], "## What to take\n\nOne tablet.");
    }
}
