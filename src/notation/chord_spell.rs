//! Spell the chord (#1228). A `chord-spell` question's `correct_answer` is
//! the chord's notes in ABC (`D ^F A`), three or four of them, and its
//! `tiles` are notes to pick from (`D E F ^F _G A B`). The learner taps tiles
//! into as many slots as the chord has notes, in any order, and the answer is
//! the tiles in the slots, separated by spaces. It is right when it spells
//! the same notes as the answer (or an accepted answer), in any order and in
//! any octave. The same sounds spelled otherwise (`D _G A` for D major) are a
//! near miss: wrong, with a note saying so. `static/chord-spell.js` builds the
//! slots and draws the chord as it fills.

use serde::{Deserialize, Deserializer, Serialize};

use crate::content::{ContentStore, QuizItem, QuizType};
use crate::quiz::{Rng, Verdict};

/// A note as spelled: its letter (0 = C … 6 = B) and its accidental
/// (-2 double flat … 2 double sharp). The octave is not part of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct Spelled {
    letter: u8,
    shift: i8,
}

impl Spelled {
    /// The pitch class it sounds: 0 = C … 11 = B.
    fn sound(self) -> i32 {
        let base = [0, 2, 4, 5, 7, 9, 11].get(usize::from(self.letter)).copied().unwrap_or(0);
        (base + i32::from(self.shift)).rem_euclid(12)
    }
}

/// One ABC note without a length (`D`, `^F`, `__B`, `=E`, `c'`, `C,`).
fn spelled(s: &str) -> Option<Spelled> {
    let acc_len = s.chars().take_while(|c| matches!(c, '^' | '_' | '=')).count();
    let (acc, rest) = s.split_at(acc_len);
    let mut chars = rest.chars();
    let letter = u8::try_from("CDEFGAB".find(chars.next()?.to_ascii_uppercase())?).ok()?;
    if !chars.all(|c| c == ',' || c == '\'') {
        return None;
    }
    let shift = match acc {
        "" | "=" => 0,
        "^" => 1,
        "^^" => 2,
        "_" => -1,
        "__" => -2,
        _ => return None,
    };
    Some(Spelled { letter, shift })
}

/// The notes of an answer, sorted; None when a part isn't a note.
fn chord(answer: &str) -> Option<Vec<Spelled>> {
    let mut notes: Vec<Spelled> = answer.split_whitespace().map(spelled).collect::<Option<_>>()?;
    notes.sort();
    Some(notes)
}

/// `tiles` as written: a string of notes separated by spaces, or a list.
pub fn tiles_de<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Tiles {
        One(String),
        Many(Vec<String>),
    }
    Ok(match Tiles::deserialize(d)? {
        Tiles::One(s) => s.split_whitespace().map(String::from).collect(),
        Tiles::Many(v) => v,
    })
}

/// Validate a chord-spell question: the tiles are notes, the answers are 3
/// or 4 different notes and every one of them is a tile.
pub fn check(q: &QuizItem) -> Result<(), String> {
    if !q.abc.is_empty() {
        return Err("a chord-spell question draws its own staff: leave abc out".into());
    }
    let mut seen = Vec::new();
    for t in &q.tiles {
        let s = spelled(t).ok_or_else(|| format!("tile '{t}' must be one note in ABC, such as D, ^F or _B"))?;
        if seen.contains(&s) {
            return Err(format!("tile '{t}' spells a note another tile spells"));
        }
        seen.push(s);
    }
    if q.correct_answer.trim().is_empty() {
        return Err("a chord-spell question needs correct_answer: the chord's notes, separated by spaces".into());
    }
    for a in std::iter::once(&q.correct_answer).chain(&q.accepted_answers) {
        let notes = chord(a).ok_or_else(|| format!("answer '{a}' must be notes in ABC separated by spaces, such as D ^F A"))?;
        if !(3..=4).contains(&notes.len()) {
            return Err(format!("answer '{a}' must have 3 or 4 notes"));
        }
        if notes.windows(2).any(|w| w.first() == w.get(1)) {
            return Err(format!("answer '{a}' spells a note twice"));
        }
        if let Some(n) = a.split_whitespace().find(|n| !spelled(n).is_some_and(|s| seen.contains(&s))) {
            return Err(format!("answer note '{n}' must be one of the tiles"));
        }
        if notes.len() != q.correct_answer.split_whitespace().count() {
            return Err(format!("answer '{a}' must have as many notes as correct_answer"));
        }
    }
    if q.tiles.len() <= q.correct_answer.split_whitespace().count() {
        return Err("a chord-spell question needs more tiles than the chord has notes".into());
    }
    Ok(())
}

/// Grade a chord-spell answer: the same notes in any order. The same sounds
/// spelled otherwise get the "right sound, wrong spelling" note.
pub fn grade(q: &QuizItem, given: &str) -> Verdict {
    let wrong = |note: Option<String>| Verdict { correct: false, note };
    let Some(notes) = chord(given) else { return wrong(None) };
    if std::iter::once(&q.correct_answer).chain(&q.accepted_answers).any(|a| chord(a).as_ref() == Some(&notes)) {
        return Verdict { correct: true, note: None };
    }
    let sounds = |n: &[Spelled]| {
        let mut s: Vec<i32> = n.iter().map(|x| x.sound()).collect();
        s.sort_unstable();
        s
    };
    match chord(&q.correct_answer) {
        Some(want) if sounds(&want) == sounds(&notes) => wrong(Some(crate::t("quiz.check.chord_spelling"))),
        _ => wrong(None),
    }
}

/// A tile as the page shows it: the note in ABC (what is sent) and its name
/// in the course's note names (`music.note_names`).
#[derive(Debug, Clone, Serialize)]
pub struct Tile {
    pub abc: String,
    pub name: String,
}

/// A chord-spell question's tiles, shuffled and named; none for any other question.
pub fn tiles(content: &ContentStore, q: &QuizItem, rng: &mut Rng) -> Vec<Tile> {
    if q.kind != QuizType::ChordSpell {
        return Vec::new();
    }
    let names = content.manifest().music.as_ref().map(|m| m.note_names).unwrap_or_default();
    let mut out: Vec<Tile> = q.tiles.iter().map(|t| Tile { abc: t.clone(), name: super::note_name(t, names).unwrap_or_else(|| t.clone()) }).collect();
    rng.shuffle(&mut out);
    out
}

/// How many slots a chord-spell question has: the notes of its chord.
pub fn slots(q: &QuizItem) -> usize {
    if q.kind == QuizType::ChordSpell { q.correct_answer.split_whitespace().count() } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(answer: &str, tiles: serde_json::Value, extra: serde_json::Value) -> QuizItem {
        let mut v = serde_json::json!({ "question_id": "q", "prompt": "Spell D major.", "type": "chord-spell", "correct_answer": answer, "tiles": tiles });
        v.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        serde_json::from_value(v).unwrap()
    }
    const TILES: &str = "D E F ^F _G A B";

    #[test]
    fn tiles_are_a_string_or_a_list() {
        assert_eq!(item("D ^F A", serde_json::json!(TILES), serde_json::json!({})).tiles, ["D", "E", "F", "^F", "_G", "A", "B"]);
        assert_eq!(item("D ^F A", serde_json::json!(["D", "^F", "A", "B"]), serde_json::json!({})).tiles, ["D", "^F", "A", "B"]);
    }

    #[test]
    fn the_notes_count_in_any_order_and_spelling_matters() {
        let q = item("D ^F A", serde_json::json!(TILES), serde_json::json!({}));
        check(&q).unwrap();
        for right in ["D ^F A", "A D ^F", "^F A D", "d ^f a"] {
            assert!(grade(&q, right).correct, "{right}");
        }
        let near = grade(&q, "D _G A");
        assert!(!near.correct);
        assert_eq!(near.note.unwrap(), crate::t("quiz.check.chord_spelling"));
        let off = grade(&q, "D F A");
        assert!(!off.correct && off.note.is_none(), "D minor is not a near miss");
        assert!(!grade(&q, "D ^F").correct, "a note short");
        assert!(!grade(&q, "D ^F A B").correct, "a note too many");
        assert!(!grade(&q, "D ^F H").correct && grade(&q, "D ^F H").note.is_none(), "not a note");
    }

    #[test]
    fn an_accepted_answer_counts_too() {
        let q = item("_G _B _D", serde_json::json!("_G _B _D ^F ^A ^C E"), serde_json::json!({ "accepted_answers": ["^F ^A ^C"] }));
        check(&q).unwrap();
        assert!(grade(&q, "^C ^F ^A").correct);
        assert!(grade(&q, "_D _G _B").correct);
    }

    #[test]
    fn the_validator_names_what_is_wrong() {
        let err = |answer: &str, tiles: &str, extra: serde_json::Value| check(&item(answer, serde_json::json!(tiles), extra)).unwrap_err();
        assert!(err("D ^F A", "D ^F A H", serde_json::json!({})).contains("tile 'H' must be one note"));
        assert!(err("D ^F A", "D ^F A F2", serde_json::json!({})).contains("tile 'F2'"));
        assert!(err("D ^F A", "D ^F A d", serde_json::json!({})).contains("spells a note another tile spells"));
        assert!(err("D ^F", "D ^F A B", serde_json::json!({})).contains("3 or 4 notes"));
        assert!(err("C E G B D", "C E G B D F", serde_json::json!({})).contains("3 or 4 notes"));
        assert!(err("D ^F D", "D ^F A B", serde_json::json!({})).contains("twice"));
        assert!(err("D ^F A", "D F A B", serde_json::json!({})).contains("answer note '^F' must be one of the tiles"));
        assert!(err("D ^F A", "D ^F A", serde_json::json!({})).contains("more tiles"));
        assert!(err("", TILES, serde_json::json!({})).contains("needs correct_answer"));
        assert!(err("D ^F A", TILES, serde_json::json!({ "abc": "X:1\nK:C\nD" })).contains("leave abc out"));
        assert!(err("D ^F A", TILES, serde_json::json!({ "accepted_answers": ["D ^F A B"] })).contains("as many notes"));
    }
}
