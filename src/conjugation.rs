//! Paradigm drill ("Conjugation Drill" for a language course): which items
//! a learner has unlocked, dealing prompts, and checking typed forms. The
//! shape of the tables comes from the course manifest's `paradigm`.

use rusqlite::Connection;
use serde::Serialize;
use std::collections::HashSet;

use crate::content::ContentStore;
use crate::db::now;
use crate::mastery::{self, MASTERED_AT};
use crate::quiz::{normalize, strip_accents, Rng};

/// Lesson ids the user has "completed": passed in any mode
/// (`progress.rs`), or the lesson's mastery is at the mastered level.
pub fn completed_lessons(conn: &Connection, user_id: i64, course_id: &str) -> rusqlite::Result<HashSet<String>> {
    let at = now();
    let mut done: HashSet<String> = crate::progress::passed_ids(conn, user_id, course_id)?;
    for (id, p) in mastery::load_progress(conn, user_id, course_id)? {
        if p.times_tested > 0 && p.effective(at) >= MASTERED_AT {
            done.insert(id);
        }
    }
    Ok(done)
}

#[derive(Serialize, Clone)]
pub struct Prompt {
    pub item: String,
    pub meaning: String,
    pub set: String,
    pub set_label: String,
    pub slot: String,
    pub art: Vec<String>,
}

/// Deal up to `count` prompts (0 = every form once) for the items in `set`.
pub fn deal(content: &ContentStore, set: &str, items: &[String], count: usize, rng: &mut Rng) -> Vec<Prompt> {
    let Some(paradigm) = content.paradigm() else { return Vec::new() };
    let set_label = paradigm.set_label(set).to_string();
    let mut prompts = Vec::new();
    for item in items {
        let Some((_, k)) = content.conjugation(item, set) else { continue };
        for slot in &paradigm.slots {
            prompts.push(Prompt {
                item: k.item.clone(),
                meaning: k.meaning.clone(),
                set: set.to_string(),
                set_label: set_label.clone(),
                slot: slot.key.clone(),
                art: content.item_art(&k.item).to_vec(),
            });
        }
    }
    rng.shuffle(&mut prompts);
    if count > 0 && count < prompts.len() {
        prompts.truncate(count);
    }
    // Avoid the same item twice in a row where possible (cheap pass).
    for i in 1..prompts.len() {
        if prompts[i].item == prompts[i - 1].item {
            if let Some(j) = (i + 1..prompts.len()).find(|&j| prompts[j].item != prompts[i - 1].item) {
                prompts.swap(i, j);
            }
        }
    }
    prompts
}

#[derive(Serialize, PartialEq, Eq, Debug)]
pub struct Verdict {
    pub correct: bool,
    pub expected: String,
    pub note: Option<String>,
}

/// Every way of reading a typed answer once a leading slot label is
/// allowed in front of it: "yo hablo" -> ["yo hablo", "hablo"], so typing
/// the pronoun along with the form still counts, and a form that itself
/// starts with a label word ("nous appelons" for nous nous appelons) is not
/// lost. `labels` are the paradigm's slot labels, split on " / " (e.g.
/// "él / ella / usted"). An elided label glued to the form ("j'ai" for the
/// slot "je") is read too: a label ending in -e may lose it before an
/// apostrophe.
fn readings(s: &str, labels: &[String]) -> Vec<String> {
    let words: Vec<&str> = s.split(' ').collect();
    let is_label = |w: &str| labels.iter().any(|l| normalize(l) == w || strip_accents(&normalize(l)) == strip_accents(w));
    let mut out = vec![s.to_string()];
    let mut i = 0;
    while i + 1 < words.len() && is_label(words[i]) {
        i += 1;
        out.push(words[i..].join(" "));
    }
    let mut elided = Vec::new();
    for reading in &out {
        let Some(first) = reading.split(' ').next() else { continue };
        for l in labels {
            let l = normalize(l);
            if let Some(stem) = l.strip_suffix('e') {
                if !stem.is_empty() {
                    if let Some(rest) = first.strip_prefix(format!("{stem}'").as_str()) {
                        if !rest.is_empty() {
                            elided.push(format!("{rest}{}", &reading[first.len()..]));
                        }
                    }
                }
            }
        }
    }
    out.extend(elided);
    out
}

pub fn check(expected: &str, answer: &str, labels: &[String]) -> Verdict {
    let want = normalize(expected);
    let got = normalize(answer);
    if got.is_empty() {
        return Verdict { correct: false, expected: expected.to_string(), note: Some(crate::t("play.drill.empty_answer")) };
    }
    let readings = readings(&got, labels);
    if readings.iter().any(|r| *r == want) {
        return Verdict { correct: true, expected: expected.to_string(), note: None };
    }
    if readings.iter().any(|r| strip_accents(r) == strip_accents(&want)) {
        return Verdict {
            correct: false,
            expected: expected.to_string(),
            note: Some(crate::t("play.drill.almost_accent")),
        };
    }
    Verdict { correct: false, expected: expected.to_string(), note: None }
}

/// Every single-word alternative in the paradigm's slot labels, for `check`.
pub fn slot_words(content: &ContentStore) -> Vec<String> {
    content
        .paradigm()
        .map(|p| p.slots.iter().flat_map(|s| s.label.split(" / ").map(|w| w.trim().to_string())).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_an_elided_pronoun() {
        let labels: Vec<String> = ["je", "tu", "il / elle / on"].iter().map(|s| s.to_string()).collect();
        assert!(check("ai", "j'ai", &labels).correct);
        assert!(check("ai", "J’ai", &labels).correct, "curly apostrophe");
        assert!(check("habite", "j'habite", &labels).correct);
        assert!(check("m'appelle", "je m'appelle", &labels).correct);
        assert!(check("m'appelle", "m'appelle", &labels).correct);
        assert!(!check("ai", "j'", &labels).correct);
        let labels: Vec<String> = ["nous", "vous"].iter().map(|s| s.to_string()).collect();
        assert!(check("nous appelons", "nous nous appelons", &labels).correct, "a reflexive form starting with a label word");
        assert!(check("nous appelons", "nous appelons", &labels).correct);
        assert!(!check("nous appelons", "appelons", &labels).correct);
    }

    #[test]
    fn checks_forms_with_pronouns_and_accents() {
        let labels: Vec<String> = ["yo", "tú", "vosotros"].iter().map(|s| s.to_string()).collect();
        assert!(check("hablo", "  Hablo ", &labels).correct);
        assert!(check("hablo", "yo hablo", &labels).correct);
        assert!(check("habláis", "vosotros habláis", &labels).correct);
        assert!(check("hablas", "tu hablas", &labels).correct);
        let v = check("habláis", "hablais", &labels);
        assert!(!v.correct);
        assert_eq!(v.note.as_deref(), Some("Almost! Check the accent."));
        assert!(!check("hablo", "habla", &labels).correct);
        assert!(check("hablo", "", &labels).note.is_some());
    }

    #[test]
    fn completed_lessons_from_a_pass_or_mastery() {
        let db = crate::db::Db::open_in_memory().unwrap();
        let l1: crate::content::Lesson = serde_json::from_str(r#"{"id":"l1","slug":"l1","title":"L","unit":"1","content":"x"}"#).unwrap();
        db.with(|c| {
            let uid = crate::auth::create_user(c, "ann@example.org", "h")?;
            assert!(completed_lessons(c, uid, "es")?.is_empty());
            // Passed (here in a Guide stage; any mode counts).
            crate::progress::record_taught_run(c, uid, "es", &l1, crate::progress::VIA_PLAY, 1, 1)?;
            // Mastery reached through practice.
            for i in 0..8 {
                crate::mastery::record_answer(c, uid, "es", "l2", &format!("q{i}"), "a", true)?;
            }
            // Started but not finished.
            crate::guide::complete_step(c, uid, "es", "l3", 0, 3, false, false)?;
            let done = completed_lessons(c, uid, "es")?;
            assert!(done.contains("l1") && done.contains("l2") && !done.contains("l3"));
            assert!(completed_lessons(c, uid, "fr")?.is_empty());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn deals_from_the_spanish_course() {
        let c = ContentStore::load_dir(std::path::Path::new("data/courses/spanish")).unwrap();
        let prompts = deal(&c, "present", &["hablar".into(), "nope".into()], 0, &mut Rng::seeded(1));
        assert_eq!(prompts.len(), 6);
        assert!(prompts.iter().all(|p| p.item == "hablar" && p.set_label == "Present"));
        assert!(slot_words(&c).contains(&"usted".to_string()));
    }
}
