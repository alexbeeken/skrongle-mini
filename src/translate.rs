//! Copy and translate a course (#630): which texts of a course's files the
//! AI translates, the request and reply of one file, and putting the
//! translations back in place.
//!
//! The model never rewrites a file. The app walks the file's JSON, collects
//! the texts a learner reads in their own language (each distinct text
//! once), sends them numbered (`{"1": "…", "2": "…"}`) and puts each
//! translation back wherever that text was. So the structure, the ids and
//! every reference stay as they were, and a `correct_answer` that is one of
//! the `options` stays one of them (the same text gets the same
//! translation). The translated file is then validated like any other.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// What a translation job is doing, kept in the copy's `courses.notes`
/// (as JSON) so a stopped job can resume where it stopped.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Plan {
    /// The course it is a copy of.
    pub from: String,
    /// The language translated into: an app language code (`es`).
    pub lang: String,
    /// A language course: what it teaches (`terms`) stays as it is.
    pub language_course: bool,
    /// The course's `terms` label and language (what it teaches).
    #[serde(default)]
    pub taught: String,
    #[serde(default)]
    pub taught_lang: String,
    /// The course's `meanings` label and language before the translation.
    #[serde(default)]
    pub learner: String,
    #[serde(default)]
    pub learner_lang: String,
    /// The files translated so far, by path.
    #[serde(default)]
    pub done: Vec<String>,
}

impl Plan {
    /// The plan saved in a copy's notes, if it is one.
    pub fn parse(notes: &str) -> Option<Plan> {
        serde_json::from_str(notes).ok()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// The language translated into, as it calls itself ("Español").
    pub fn language(&self) -> String {
        crate::i18n::language(&self.lang).map(|l| l.name.clone()).unwrap_or_else(|| self.lang.clone())
    }
}

/// Keys whose strings are never text for the learner to read: ids,
/// references, codes, pictures, layout. Skipped in every course. A guide
/// step's `section` names a heading of the content: it follows the
/// translated heading (`fix_sections`).
const STRUCTURAL: &[&str] = &[
    "id", "slug", "question_id", "unit", "group", "type", "lang", "icon", "emoji", "image", "box", "set", "speaker", "prerequisites",
    "question", "section", "levels", "chars", "art", "role_a", "role_b", "tolerance", "bucket",
];

/// Keys whose strings are in the language a language course teaches: kept
/// as they are there (in any other course they are translated too).
const TAUGHT: &[&str] = &[
    "term", "example", "item", "forms", "text", "sentence", "wrong", "fix", "buckets", "distractors", "teaches", "terms", "paradigm", "calendar",
];

fn skipped(key: &str, language_course: bool) -> bool {
    STRUCTURAL.contains(&key) || (language_course && TAUGHT.contains(&key))
}

/// A text worth translating: it has letters, and is not a true/false answer.
fn worth(s: &str) -> bool {
    let t = s.trim();
    t.chars().any(char::is_alphabetic) && t != "true" && t != "false"
}

fn walk(v: &mut Value, language_course: bool, f: &mut dyn FnMut(&mut String)) {
    match v {
        Value::Object(map) => {
            for (k, child) in map.iter_mut() {
                if !skipped(k, language_course) {
                    walk(child, language_course, f);
                }
            }
        }
        Value::Array(items) => {
            for child in items {
                walk(child, language_course, f);
            }
        }
        Value::String(s) if worth(s) => f(s),
        _ => {}
    }
}

/// The distinct texts of a file to translate, in the order they come.
pub fn texts(v: &Value, language_course: bool) -> Vec<String> {
    let mut copy = v.clone();
    let mut out: Vec<String> = Vec::new();
    walk(&mut copy, language_course, &mut |s| {
        if !out.iter().any(|o| o == s) {
            out.push(s.clone());
        }
    });
    out
}

/// The texts as the request numbers them: `{"1": "…", "2": "…"}`.
pub fn numbered(texts: &[String]) -> String {
    let map: serde_json::Map<String, Value> = texts.iter().enumerate().map(|(i, t)| ((i + 1).to_string(), Value::String(t.clone()))).collect();
    serde_json::to_string_pretty(&Value::Object(map)).unwrap_or_default()
}

/// The translations in a reply, by original text. A number the reply leaves
/// out keeps its text; one it does not know, or an empty translation, is
/// ignored. Not an object: an error.
pub fn translations(reply: &Value, texts: &[String]) -> Result<HashMap<String, String>, String> {
    let map = reply.as_object().ok_or("the reply is not a JSON object of numbered translations")?;
    let mut out = HashMap::new();
    for (k, v) in map {
        let (Ok(n), Some(t)) = (k.trim().parse::<usize>(), v.as_str()) else { continue };
        if let Some(original) = n.checked_sub(1).and_then(|i| texts.get(i)) {
            if !t.trim().is_empty() {
                out.insert(original.clone(), t.to_string());
            }
        }
    }
    Ok(out)
}

/// Put the translations in place of the texts they translate.
pub fn apply(v: &mut Value, language_course: bool, translations: &HashMap<String, String>) {
    walk(v, language_course, &mut |s| {
        if let Some(t) = translations.get(s.as_str()) {
            *s = t.clone();
        }
    });
}

/// The `## ` headings of a lesson's content, in order.
fn headings(content: &str) -> Vec<String> {
    content.lines().filter_map(|l| l.strip_prefix("## ")).map(|h| h.trim().to_string()).collect()
}

/// A guide step's `section` names the start of a heading: point each at
/// the translated heading in the same place as the one it named.
pub fn fix_sections(original: &Value, translated: &mut Value) {
    let before = headings(original.get("content").and_then(Value::as_str).unwrap_or_default());
    let after = headings(translated.get("content").and_then(Value::as_str).unwrap_or_default());
    let Some(guide) = translated.get_mut("guide").and_then(Value::as_array_mut) else { return };
    for step in guide {
        let Some(section) = step.get("section").and_then(Value::as_str).map(str::to_lowercase) else { continue };
        let at = before.iter().position(|h| h.to_lowercase().starts_with(&section));
        if let (Some(heading), Some(step)) = (at.and_then(|i| after.get(i)), step.as_object_mut()) {
            step.insert("section".into(), Value::String(heading.clone()));
        }
    }
}

/// The copy's manifest once translated: its own id and title, and its
/// learner's side in the new language (the taught side too, in a course
/// that teaches no language and wrote both in one).
pub fn fix_manifest(v: &mut Value, id: &str, title: &str, plan: &Plan) {
    if let Some(m) = v.as_object_mut() {
        m.insert("id".into(), Value::String(id.to_string()));
        m.insert("title".into(), Value::String(title.to_string()));
    }
    if let Some(m) = v.get_mut("meanings").and_then(Value::as_object_mut) {
        m.insert("lang".into(), Value::String(plan.lang.clone()));
    }
    if !plan.language_course && !plan.taught_lang.is_empty() && plan.taught_lang == plan.learner_lang {
        if let Some(t) = v.get_mut("terms").and_then(Value::as_object_mut) {
            t.insert("lang".into(), Value::String(plan.lang.clone()));
        }
    }
}

/// The system prompt of every translation request.
pub fn system(plan: &Plan) -> String {
    let target = format!("{} ({})", plan.language(), plan.lang);
    let side = if plan.language_course {
        let taught = if plan.taught_lang.is_empty() { plan.taught.clone() } else { format!("{} ({})", plan.taught, plan.taught_lang) };
        format!(
            "The course teaches {taught}. Leave every word, phrase and sentence in {taught} exactly as it is, also inside a text that \
             mixes both languages; translate only the parts written in the learner's language ({}), the explanations, meanings, \
             instructions, questions and answers.",
            if plan.learner.is_empty() { "the course's other language".to_string() } else { plan.learner.clone() }
        )
    } else {
        "The course teaches no language: translate every text.".to_string()
    };
    format!(
        "You translate the texts of a learning course for a study app into {target}, the language the learner reads it in. {side} \
         Keep Markdown, line breaks, emoji, numbers, formulas, code and {{placeholders}} as they are. Translate naturally, as a \
         teacher writing for learners would, and keep the meaning exact: a right answer must stay right. \
         You get a JSON object of numbered texts; reply with one JSON object with the same numbers as keys and the translations as \
         values, and nothing else."
    )
}

/// The request for one file of the course `title`.
pub fn prompt(title: &str, path: &str, texts: &[String], feedback: &str) -> String {
    format!("The texts of {path} in the course \"{title}\":\n\n{}{feedback}", numbered(texts))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(language_course: bool) -> Plan {
        Plan { from: "spanish".into(), lang: "fr".into(), language_course, taught: "Español".into(), taught_lang: "es".into(), learner: "English".into(), learner_lang: "en".into(), done: vec![] }
    }

    /// The learner's side goes out once per text and comes back in every
    /// place it was; the taught side, ids and references stay.
    #[test]
    fn translates_the_learners_side_in_place() {
        let lesson = serde_json::json!({
            "id": "a1-greetings", "slug": "greetings", "title": "Greetings", "unit": "A1",
            "content": "## Say hello\n\nUse **hola**.\n\n## Say goodbye\n\nUse adiós.",
            "vocabulary": [{ "term": "hola", "meaning": "hello", "example": "¡Hola, Ana!", "example_meaning": "Hello, Ana!" }],
            "quiz": [
                { "question_id": "a1-greetings-01", "type": "multiple-choice", "prompt": "What does hola mean?", "options": ["hello", "goodbye"], "correct_answer": "hello" },
                { "question_id": "a1-greetings-02", "type": "true-false", "prompt": "Adiós means goodbye.", "correct_answer": "true" }
            ],
            "guide": [{ "section": "say good" }, { "question": "a1-greetings-01" }]
        });
        let texts = texts(&lesson, true);
        let mut sorted = texts.clone();
        sorted.sort();
        let content = "## Say hello\n\nUse **hola**.\n\n## Say goodbye\n\nUse adiós.";
        assert_eq!(sorted, [content, "Adiós means goodbye.", "Greetings", "Hello, Ana!", "What does hola mean?", "goodbye", "hello"]);
        // Numbered as the request numbers them; "Adiós means goodbye." is left out of the reply.
        let french = [
            (content, "## Dire bonjour\n\nUtilisez **hola**.\n\n## Dire au revoir\n\nUtilisez adiós."),
            ("Greetings", "Salutations"),
            ("Hello, Ana!", ""),
            ("What does hola mean?", "Que veut dire hola ?"),
            ("goodbye", "au revoir"),
            ("hello", "bonjour"),
        ];
        let mut reply = serde_json::json!({ "99": "unknown", "x": "nope" });
        for (en, fr) in french {
            let n = texts.iter().position(|t| t == en).unwrap() + 1;
            reply[n.to_string()] = Value::String(fr.into());
        }
        assert!(numbered(&texts).contains("\"1\": "));
        let map = translations(&reply, &texts).unwrap();
        let mut out = lesson.clone();
        apply(&mut out, true, &map);
        fix_sections(&lesson, &mut out);
        assert_eq!(out["title"], "Salutations");
        assert_eq!(out["vocabulary"][0]["term"], "hola", "the taught side stays");
        assert_eq!(out["vocabulary"][0]["meaning"], "bonjour");
        assert_eq!(out["vocabulary"][0]["example_meaning"], "Hello, Ana!", "an empty translation keeps the text");
        assert_eq!(out["quiz"][0]["options"], serde_json::json!(["bonjour", "au revoir"]));
        assert_eq!(out["quiz"][0]["correct_answer"], "bonjour", "the answer is still one of the options");
        assert_eq!(out["quiz"][1]["correct_answer"], "true");
        assert_eq!(out["quiz"][1]["prompt"], "Adiós means goodbye.", "left out of the reply: kept");
        assert_eq!(out["guide"][0]["section"], "Dire au revoir", "the guide follows its heading");
        assert_eq!(out["guide"][1]["question"], "a1-greetings-01");
        assert!(translations(&serde_json::json!(["a"]), &texts).is_err());
    }

    /// A course that teaches no language has everything translated, and
    /// both sides written in one language move to the new one.
    #[test]
    fn a_non_language_course_translates_everything() {
        let lesson = serde_json::json!({ "vocabulary": [{ "term": "numerator", "meaning": "the top number" }], "quiz": [{ "teaches": ["numerator"] }] });
        assert_eq!(texts(&lesson, false), ["numerator", "the top number"]);
        let mut manifest = serde_json::json!({ "id": "x", "title": "Fractions", "terms": { "label": "Term", "lang": "en" }, "meanings": { "label": "Meaning", "lang": "en" } });
        let p = Plan { language_course: false, taught_lang: "en".into(), ..plan(false) };
        fix_manifest(&mut manifest, "fractions-fr-abc123", "Fractions (Français)", &p);
        assert_eq!((manifest["id"].as_str(), manifest["title"].as_str()), (Some("fractions-fr-abc123"), Some("Fractions (Français)")));
        assert_eq!((manifest["terms"]["lang"].as_str(), manifest["meanings"]["lang"].as_str()), (Some("fr"), Some("fr")));
        // A language course keeps what it teaches.
        let mut spanish = serde_json::json!({ "terms": { "label": "Español", "lang": "es" }, "meanings": { "label": "English", "lang": "en" } });
        fix_manifest(&mut spanish, "s", "Spanish (Français)", &plan(true));
        assert_eq!((spanish["terms"]["lang"].as_str(), spanish["meanings"]["lang"].as_str()), (Some("es"), Some("fr")));
        assert!(system(&plan(true)).contains("Español (es)") && system(&plan(false)).contains("translate every text"));
        let saved = plan(true).to_json();
        assert_eq!(Plan::parse(&saved), Some(plan(true)));
        assert_eq!(Plan::parse("my class notes"), None);
    }
}
