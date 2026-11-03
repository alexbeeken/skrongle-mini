//! Build the scale (#1229): Sprint mode's ladder with notes. A lesson's
//! `sprint.scales` names a scale (`title`) and its eight notes in ABC
//! (`notes`, bottom to top), plus `decoys` that are not in it. The first rung
//! is filled; the learner taps the other seven from a bank of the scale's
//! notes and the decoys, shown in the course's note names. As in the ladder
//! (#640), nothing is judged until Check, and the grade is how many of the
//! eight notes are on their rungs. `static/scale-ladder.js` draws each placed
//! note on a staff and plays it; `static/sprint.js` runs the ladder itself.

use serde::{Deserialize, Serialize};

use super::{Effect, Exercise, Graded};
use crate::content::{ContentStore, Lesson};
use crate::notation::{note_name, NoteNames};
use crate::quiz::Rng;

/// The rungs of a scale: seven degrees and the octave.
pub const RUNGS: usize = 8;

/// One of a lesson's `sprint.scales`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScaleSpec {
    pub title: String,
    /// Eight ABC notes separated by spaces, bottom to top (`D E ^F G A B ^c d`).
    pub notes: String,
    /// ABC notes that are not in the scale, mixed into the bank (`F c`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub decoys: String,
}

impl ScaleSpec {
    pub fn notes(&self) -> Vec<&str> {
        self.notes.split_whitespace().collect()
    }
    pub fn decoys(&self) -> Vec<&str> {
        self.decoys.split_whitespace().collect()
    }
    /// The scale as a tune, one quarter note per degree, accidentals written out (Study's lesson page).
    pub fn abc(&self) -> String {
        tune(&self.notes)
    }
}

/// ABC notes as a tune in C (so every sharp and flat is written on the note).
pub fn tune(notes: &str) -> String {
    format!("X:1\nL:1/4\nK:C\n{notes}|")
}

fn names(content: &ContentStore) -> NoteNames {
    content.manifest().music.as_ref().map(|m| m.note_names).unwrap_or_default()
}

fn name(n: &str, names: NoteNames) -> String {
    note_name(n, names).unwrap_or_else(|| n.to_string())
}

/// The validator's rules for a lesson's scales.
pub fn validate(lesson: &Lesson) -> Result<(), String> {
    for (i, s) in lesson.sprint.scales.iter().enumerate() {
        let at = format!("sprint scale {}", i + 1);
        if s.title.trim().is_empty() {
            return Err(format!("{at} needs a title"));
        }
        let notes = s.notes();
        if notes.len() != RUNGS {
            return Err(format!("{at} ('{}') needs {RUNGS} notes separated by spaces, found {}", s.title, notes.len()));
        }
        let one_note = |n: &str| note_name(n, NoteNames::Letters).ok_or(format!("{at} ('{}'): '{n}' is not one note in ABC, like F, ^C or _B", s.title));
        let scale: Vec<String> = notes.iter().map(|n| one_note(n)).collect::<Result<_, _>>()?;
        for d in s.decoys() {
            if scale.contains(&one_note(d)?) {
                return Err(format!("{at} ('{}'): the decoy '{d}' is a note of the scale", s.title));
            }
        }
    }
    Ok(())
}

/// A scale of the lesson, picked at random; none when it has none.
pub fn pick(content: &ContentStore, lesson: &Lesson, rng: &mut Rng) -> Option<Exercise> {
    if lesson.sprint.scales.is_empty() {
        return None;
    }
    exercise(content, lesson, rng.below(lesson.sprint.scales.len()), rng)
}

/// The ladder for scale `i`: `forms` is the key (every rung's note name),
/// `notes` their ABC; `tiles` the bank (rungs 2 to 8 and the decoys,
/// shuffled) with `tile_notes` their ABC, so a tile is drawn and played as it lands.
pub fn exercise(content: &ContentStore, lesson: &Lesson, i: usize, rng: &mut Rng) -> Option<Exercise> {
    let s = lesson.sprint.scales.get(i)?;
    let names = names(content);
    let notes: Vec<String> = s.notes().into_iter().map(str::to_string).collect();
    let mut bank: Vec<String> = notes.iter().skip(1).cloned().chain(s.decoys().into_iter().map(str::to_string)).collect();
    rng.shuffle(&mut bank);
    Some(Exercise::Scale {
        id: format!("scale:{}:{i}", lesson.id),
        lesson: lesson.title.clone(),
        title: s.title.clone(),
        slots: (1..=notes.len()).map(|n| n.to_string()).collect(),
        forms: notes.iter().map(|n| name(n, names)).collect(),
        tiles: bank.iter().map(|n| name(n, names)).collect(),
        tile_notes: bank,
        notes,
    })
}

/// Grade a scale at Check: `answer` is the note names on rungs 2 to 8, in
/// order. The first rung came filled and counts as right.
pub fn grade(content: &ContentStore, lesson: &Lesson, i: usize, answer: &serde_json::Value) -> Result<Graded, String> {
    let s = lesson.sprint.scales.get(i).ok_or("unknown scale")?;
    let names = names(content);
    let key: Vec<String> = s.notes().iter().map(|n| name(n, names)).collect();
    let given = super::arr_of_strings(answer).ok_or("expected the note names")?;
    if given.len() + 1 != key.len() {
        return Err(format!("expected {} notes", key.len() - 1));
    }
    let n = key.len();
    let right = 1 + key.iter().skip(1).zip(&given).filter(|(k, g)| k.as_str() == g.trim()).count();
    Ok(Graded {
        correct: right == n,
        correct_answer: key.join(" "),
        explanation: String::new(),
        note: Some(crate::t2("sprint.history.notes_right", "right", right, "n", n)),
        effect: Effect::Mastery { lesson_id: lesson.id.clone(), selected: format!("{right}/{n}") },
    })
}

/// The history's prompt and answer for scale `i` of `lesson`.
pub fn describe(content: &ContentStore, lesson: &Lesson, i: usize) -> Option<(String, String)> {
    let s = lesson.sprint.scales.get(i)?;
    let names = names(content);
    Some((crate::t1("sprint.describe.scale", "title", &s.title), s.notes().iter().map(|n| name(n, names)).collect::<Vec<_>>().join(" ")))
}

#[cfg(test)]
mod tests {
    use super::super::{check, describe, history_answer, Exercise};
    use super::*;
    use std::path::Path;

    fn course() -> ContentStore {
        ContentStore::load_dir(Path::new("data/test-courses/test-course")).expect("test course loads")
    }

    #[test]
    fn a_scale_is_a_ladder_of_notes_graded_at_check() {
        let c = course();
        let l = c.lesson_by_slug("ear").unwrap();
        let Some(Exercise::Scale { id, title, slots, forms, notes, tiles, tile_notes, .. }) = exercise(&c, l, 0, &mut Rng::seeded(5)) else { panic!("no scale") };
        assert_eq!(title, "D major");
        assert_eq!(slots, ["1", "2", "3", "4", "5", "6", "7", "8"]);
        // The course writes note names as letters; the octave is not part of a name.
        assert_eq!(forms, ["D", "E", "F♯", "G", "A", "B", "C♯", "D"]);
        assert_eq!(notes[2], "^F");
        // The bank: rungs 2 to 8 and the decoys F and C, each with its ABC.
        let mut bank = tiles.clone();
        bank.sort();
        assert_eq!(bank, ["A", "B", "C", "C♯", "D", "E", "F", "F♯", "G"]);
        assert_eq!(tile_notes[tiles.iter().position(|t| t == "C♯").unwrap()], "^c");
        let grade = |given: &[&str]| check(&c, &id, &serde_json::json!(given), 0).unwrap();
        let g = grade(&["E", "F♯", "G", "A", "B", "C♯", "D"]);
        assert!(g.correct && g.note.as_deref() == Some("8 of 8 notes right"), "{:?}", g.note);
        // The decoys F and C on rungs 3 and 7: 6 of 8, with the key.
        let g = grade(&["E", "F", "G", "A", "B", "C", "D"]);
        assert!(!g.correct);
        assert_eq!(g.note.as_deref(), Some("6 of 8 notes right"));
        assert_eq!(g.correct_answer, "D E F♯ G A B C♯ D");
        assert!(check(&c, &id, &serde_json::json!(["E"]), 0).is_err(), "seven notes or none");
        assert_eq!(history_answer(&c, &id, "6/8"), "6 of 8 notes right");
        let (_, prompt, answer) = describe(&c, &id).unwrap();
        assert_eq!((prompt.as_str(), answer.as_str()), ("Build the scale: D major", "D E F♯ G A B C♯ D"));
    }

    #[test]
    fn a_scale_is_validated() {
        let lesson = |scale: serde_json::Value| -> Lesson {
            serde_json::from_value(serde_json::json!({ "id": "x", "slug": "x", "title": "X", "unit": "u", "content": "", "sprint": { "scales": [scale] } })).unwrap()
        };
        let ok = serde_json::json!({ "title": "D major", "notes": "D E ^F G A B ^c d", "decoys": "F c" });
        assert_eq!(validate(&lesson(ok)), Ok(()));
        for (scale, msg) in [
            (serde_json::json!({ "title": "D major", "notes": "D E ^F G A B ^c", "decoys": "F" }), "needs 8 notes"),
            (serde_json::json!({ "title": "D major", "notes": "D E F# G A B c# d" }), "'F#' is not one note in ABC"),
            (serde_json::json!({ "title": "D major", "notes": "D E ^F G A B ^c d", "decoys": "F G" }), "the decoy 'G' is a note of the scale"),
            (serde_json::json!({ "title": "D major", "notes": "D E ^F G A B ^c d", "decoys": "d'" }), "the decoy 'd'' is a note of the scale"),
            (serde_json::json!({ "title": "D major", "notes": "D E ^F G A B ^c d", "decoys": "Fis" }), "'Fis' is not one note"),
            (serde_json::json!({ "title": " ", "notes": "D E ^F G A B ^c d" }), "needs a title"),
        ] {
            let e = validate(&lesson(scale)).unwrap_err();
            assert!(e.contains(msg), "{e}");
        }
    }
}
