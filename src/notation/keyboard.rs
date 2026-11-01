//! Find it on the keys (#1231). A `keyboard` question shows a piano keyboard
//! of one or two octaves from middle C (`octaves`); each key sounds when
//! tapped, and the last key tapped before Check is the answer. The page posts
//! that key as an ABC note with sharps (`C`, `^C`, `c`), so every spelling of
//! a pitch is the same key: an answer of `_D` is right for the key `^C`.
//!
//! `correct_answer` (and any `accepted_answers`) are ABC notes. They count by
//! pitch class, so any C is right for `C`, unless `"octave": true`, when
//! only that key is. The loader keeps them in `keys` and shows
//! `correct_answer` in the course's note names (`super::name_notes`).
//! `static/keyboard.js` draws the keys.

use crate::content::{QuizItem, QuizType};
use crate::quiz::Verdict;

/// Middle C (ABC `C`) as a MIDI number: the lowest key of the keyboard.
pub const LOW: i32 = 60;

/// An ABC note (`C`, `^F`, `_B`, `=E`, `c`, `C,`, `c'`) as a MIDI number;
/// None when `s` is not one note.
pub fn midi(s: &str) -> Option<i32> {
    let s = s.trim();
    let acc_len = s.chars().take_while(|c| matches!(c, '^' | '_' | '=')).count();
    let (acc, rest) = s.split_at(acc_len);
    let shift = match acc {
        "" | "=" => 0,
        "^" => 1,
        "^^" => 2,
        "_" => -1,
        "__" => -2,
        _ => return None,
    };
    let mut chars = rest.chars();
    let letter = chars.next()?;
    let step = [0, 2, 4, 5, 7, 9, 11].get("CDEFGAB".find(letter.to_ascii_uppercase())?).copied()?;
    let mut m = LOW + step + shift + if letter.is_ascii_lowercase() { 12 } else { 0 };
    for c in chars {
        match c {
            '\'' => m += 12,
            ',' => m -= 12,
            _ => return None,
        }
    }
    Some(m)
}

/// How many octaves of keys the question shows: 1 unless it says 2.
pub fn octaves(q: &QuizItem) -> u8 {
    q.octaves.unwrap_or(1)
}

/// Is the MIDI note `m` a key of the question's keyboard?
fn on_keys(q: &QuizItem, m: i32) -> bool {
    (LOW..LOW + 12 * i32::from(octaves(q))).contains(&m)
}

/// The answers as ABC notes: kept by the loader, or as written (the editor's preview).
fn answers(q: &QuizItem) -> Vec<&str> {
    if q.keys.is_empty() {
        std::iter::once(&q.correct_answer).chain(&q.accepted_answers).map(String::as_str).collect()
    } else {
        q.keys.iter().map(String::as_str).collect()
    }
}

/// The right key, as an ABC note, for a page to light up after the answer;
/// empty for any other question type.
pub fn key(q: &QuizItem) -> String {
    if q.kind != QuizType::Keyboard {
        return String::new();
    }
    answers(q).first().map(|a| a.trim().to_string()).unwrap_or_default()
}

/// Validate a keyboard question: its answers are ABC notes, its keyboard one
/// or two octaves, and with `octave` every answer is one of its keys.
pub fn check(q: &QuizItem) -> Result<(), String> {
    if !matches!(q.octaves, None | Some(1 | 2)) {
        return Err("octaves must be 1 or 2".into());
    }
    if !q.options.is_empty() {
        return Err("a keyboard question has no options: the keys are the options".into());
    }
    for a in answers(q) {
        let m = midi(a).ok_or_else(|| format!("answer '{a}' must be one ABC note (C, ^F, _B, c)"))?;
        if q.octave && !on_keys(q, m) {
            let top = if octaves(q) == 2 { "b" } else { "B" };
            return Err(format!("answer '{a}' is not a key of the keyboard: with \"octave\": true it must be from C to {top}"));
        }
    }
    Ok(())
}

/// Grade the key tapped (an ABC note). With `octave` the right note in
/// another octave is wrong, and says so.
pub fn grade(q: &QuizItem, given: &str) -> Verdict {
    let wrong = |note: Option<String>| Verdict { correct: false, note };
    let Some(g) = midi(given) else { return wrong(None) };
    let wants: Vec<i32> = answers(q).into_iter().filter_map(midi).collect();
    let same_class = wants.iter().any(|w| (w - g).rem_euclid(12) == 0);
    if wants.contains(&g) || (same_class && !q.octave) {
        return Verdict { correct: true, note: None };
    }
    wrong(same_class.then(|| crate::t("quiz.check.keyboard_octave")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(extra: serde_json::Value) -> QuizItem {
        let mut v = serde_json::json!({ "question_id": "q", "prompt": "Find the note you hear.", "type": "keyboard", "play": "E", "correct_answer": "E" });
        v.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn abc_notes_are_midi_numbers() {
        let m = |s: &str| midi(s).unwrap();
        assert_eq!(m("C"), 60);
        assert_eq!(m("^C"), 61);
        assert_eq!(m("_D"), 61);
        assert_eq!(m("=E"), 64);
        assert_eq!(m("^E"), 65, "E sharp is F");
        assert_eq!(m("_C"), 59, "C flat is the B below");
        assert_eq!(m("c"), 72);
        assert_eq!(m("c'"), 84);
        assert_eq!(m("C,"), 48);
        assert_eq!(m("__B"), 69);
        for bad in ["", "H", "E4", "^_C", "C#", "Do", "c,x"] {
            assert_eq!(midi(bad), None, "{bad}");
        }
    }

    #[test]
    fn any_key_of_the_pitch_class_is_right() {
        let q = item(serde_json::json!({}));
        check(&q).unwrap();
        assert!(grade(&q, "E").correct);
        assert!(grade(&q, "e").correct, "the E an octave up");
        assert!(!grade(&q, "F").correct);
        assert!(grade(&q, "F").note.is_none());
        assert!(!grade(&q, "nonsense").correct);
        // Any spelling of the pitch heard: the black key between C and D is C sharp and D flat.
        let flat = item(serde_json::json!({ "correct_answer": "_D" }));
        assert!(grade(&flat, "^C").correct);
        assert_eq!(key(&flat), "_D");
        let named = item(serde_json::json!({ "correct_answer": "_B", "accepted_answers": ["G"] }));
        assert!(grade(&named, "^A").correct && grade(&named, "g").correct);
    }

    #[test]
    fn with_octave_only_that_key_counts() {
        let q = item(serde_json::json!({ "correct_answer": "c", "octave": true, "octaves": 2 }));
        check(&q).unwrap();
        assert!(grade(&q, "c").correct);
        let v = grade(&q, "C");
        assert!(!v.correct);
        assert_eq!(v.note.unwrap(), crate::t("quiz.check.keyboard_octave"));
    }

    #[test]
    fn the_loader_names_the_answer_and_grades_by_the_key() {
        let mut quiz = vec![item(serde_json::json!({ "correct_answer": "^F" }))];
        crate::notation::name_notes(&mut quiz, crate::notation::NoteNames::Solfege).unwrap();
        assert_eq!(quiz[0].correct_answer, "Fa♯");
        assert_eq!(quiz[0].keys, ["^F"]);
        assert!(grade(&quiz[0], "_G").correct);
        assert_eq!(key(&quiz[0]), "^F");
    }

    #[test]
    fn the_validator_names_what_is_wrong() {
        let err = |v: serde_json::Value| check(&item(v)).unwrap_err();
        assert!(err(serde_json::json!({ "correct_answer": "E4" })).contains("one ABC note"));
        assert!(err(serde_json::json!({ "accepted_answers": ["Mi"] })).contains("'Mi' must be one ABC note"));
        assert!(err(serde_json::json!({ "octaves": 3 })).contains("1 or 2"));
        assert!(err(serde_json::json!({ "correct_answer": "c", "octave": true })).contains("from C to B"));
        assert!(err(serde_json::json!({ "options": ["C", "D"] })).contains("no options"));
        check(&item(serde_json::json!({ "correct_answer": "c", "octave": false }))).unwrap();
    }
}
