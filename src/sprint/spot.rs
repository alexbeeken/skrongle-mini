//! Spot the mistake (a lesson's `sprint.spots`): a sentence with one wrong
//! word, or a tune in ABC (`abc`, #1227) with one wrong note drawn as a
//! staff. In a sentence the learner taps the wrong word; on a staff they tap
//! the wrong note and pick its fix from three tiles, and both must be right.
//! A staff spot's `wrong` counts the notes from 1 (`notation::abc_notes`
//! counts them the way abcjs draws them) and its `fix` is one ABC note,
//! shown in the course's note names.

use serde::{Deserialize, Serialize};

use super::{Effect, Exercise, Graded};
use crate::content::{words_of, ContentStore, Lesson, SpotSpec};
use crate::notation::{abc_notes, note_name, NoteNames};
use crate::quiz::{normalize, Rng};

/// A fix tile on a staff spot: the note in the course's names, and its
/// letter (C to B), so a note dragged on the staff can pick its tile.
#[derive(Serialize, Clone, Debug)]
pub struct FixTile {
    pub name: String,
    pub letter: char,
}

/// `wrong` is a word (a sentence spot) or a note number (a staff spot).
pub fn text_or_number<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    match serde_json::Value::deserialize(d)? {
        serde_json::Value::String(s) => Ok(s),
        serde_json::Value::Number(n) => Ok(n.to_string()),
        _ => Err(serde::de::Error::custom("wrong must be a word, or a note number with abc")),
    }
}

fn names(content: &ContentStore) -> NoteNames {
    content.manifest().music.as_ref().map(|m| m.note_names).unwrap_or_default()
}

/// The number (from 1) of a staff spot's wrong note, if it is one of its notes.
fn wrong_note(s: &SpotSpec) -> Option<usize> {
    s.wrong.trim().parse::<usize>().ok().filter(|&n| n >= 1 && n <= abc_notes(&s.abc).len())
}

/// The validator's rules for one spot; the message follows "sprint spot N".
pub fn validate(s: &SpotSpec) -> Result<(), String> {
    if s.abc.trim().is_empty() {
        let wrong = normalize(&s.wrong);
        let hits = words_of(&s.sentence).iter().filter(|w| normalize(w) == wrong).count();
        if wrong.is_empty() || hits != 1 {
            return Err(format!(": the wrong word '{}' must appear exactly once in '{}'", s.wrong, s.sentence));
        }
    } else {
        let notes = abc_notes(&s.abc);
        if !s.sentence.trim().is_empty() {
            return Err(" has both a sentence and abc; a spot is one or the other".into());
        }
        if notes.len() < 2 {
            return Err(": its abc needs at least two notes".into());
        }
        let Some(n) = wrong_note(s) else {
            return Err(format!(": wrong '{}' must be a note number from 1 to {} (it counts the notes in abc)", s.wrong, notes.len()));
        };
        let Some(fix) = note_name(&s.fix, NoteNames::Letters) else {
            return Err(format!(": the fix '{}' must be one note in ABC, like F, ^C or _B", s.fix));
        };
        if notes.get(n - 1).and_then(|w| note_name(w, NoteNames::Letters)) == Some(fix) {
            return Err(format!(": the fix '{}' is the note already there", s.fix));
        }
    }
    if s.fix.trim().is_empty() {
        return Err(" has an empty fix".into());
    }
    Ok(())
}

/// The exercise for spot `i` of a lesson; a staff spot gets its fix tiles.
pub fn exercise(content: &ContentStore, lesson: &Lesson, i: usize, rng: &mut Rng) -> Option<Exercise> {
    let s = lesson.sprint.spots.get(i)?;
    let tiles = if s.abc.is_empty() { Vec::new() } else { fix_tiles(s, names(content), rng) };
    Some(Exercise::Spot { id: format!("spot:{}:{}", lesson.id, i), lesson: lesson.title.clone(), words: words_of(&s.sentence), abc: s.abc.clone(), tiles })
}

fn letter_of(note: &str) -> Option<char> {
    note.chars().find(|c| c.is_ascii_alphabetic()).map(|c| c.to_ascii_uppercase())
}

/// The fix and two near notes (a step or two away, never the wrong note
/// itself), shuffled.
fn fix_tiles(s: &SpotSpec, names: NoteNames, rng: &mut Rng) -> Vec<FixTile> {
    const LETTERS: &str = "CDEFGAB";
    let name = |n: &str| note_name(n, names).unwrap_or_else(|| n.trim().to_string());
    let fix = letter_of(&s.fix).unwrap_or('C');
    let wrong = wrong_note(s).and_then(|n| abc_notes(&s.abc).get(n - 1).map(|w| name(w)));
    let at = LETTERS.find(fix).unwrap_or(0);
    let mut tiles = vec![FixTile { name: name(&s.fix), letter: fix }];
    // A step up, a step down, two up, two down.
    for step in [1, 6, 2, 5] {
        let Some(letter) = LETTERS.chars().nth((at + step) % 7) else { continue };
        let tile = FixTile { name: name(&letter.to_string()), letter };
        if tiles.len() < 3 && Some(&tile.name) != wrong.as_ref() && tiles.iter().all(|t| t.name != tile.name) {
            tiles.push(tile);
        }
    }
    rng.shuffle(&mut tiles);
    tiles
}

/// Grade a spot. A sentence spot's answer is the index of the tapped word;
/// a staff spot's is `{ "note": index of the tapped note, "fix": the tile picked }`.
pub fn grade(content: &ContentStore, lesson: &Lesson, s: &SpotSpec, answer: &serde_json::Value) -> Result<Graded, String> {
    if s.abc.is_empty() {
        let words = words_of(&s.sentence);
        let idx = answer.as_u64().ok_or("expected the index of the tapped word")? as usize;
        let tapped = words.get(idx).ok_or("index out of range")?;
        return Ok(Graded {
            correct: normalize(tapped) == normalize(&s.wrong),
            correct_answer: format!("{} → {}", s.wrong, s.fix),
            explanation: s.why.clone(),
            note: None,
            effect: Effect::Mastery { lesson_id: lesson.id.clone(), selected: tapped.clone() },
        });
    }
    let n = wrong_note(s).ok_or("unknown spot")?;
    let tapped = answer.get("note").and_then(|v| v.as_u64()).ok_or("expected { note, fix }")? as usize + 1;
    let fix = answer.get("fix").and_then(|v| v.as_str()).ok_or("expected { note, fix }")?.trim();
    if fix.chars().count() > 20 {
        return Err("fix too long".into());
    }
    let right_fix = note_name(&s.fix, names(content)).unwrap_or_default();
    let (found, fixed) = (tapped == n, fix == right_fix);
    Ok(Graded {
        correct: found && fixed,
        correct_answer: crate::t2("sprint.check.wrong_note", "n", n, "fix", &right_fix),
        explanation: s.why.clone(),
        note: (found && !fixed).then(|| crate::t("sprint.check.note_found")),
        effect: Effect::Mastery { lesson_id: lesson.id.clone(), selected: format!("{tapped} → {fix}") },
    })
}

/// The history's prompt and answer for a spot.
pub fn describe(content: &ContentStore, s: &SpotSpec) -> (String, String) {
    if s.abc.is_empty() {
        return (crate::t1("sprint.describe.spot", "sentence", &s.sentence), format!("{} → {}", s.wrong, s.fix));
    }
    let names = names(content);
    let tune = abc_notes(&s.abc).iter().map(|n| note_name(n, names).unwrap_or_else(|| n.clone())).collect::<Vec<_>>().join(" ");
    let fix = note_name(&s.fix, names).unwrap_or_else(|| s.fix.clone());
    (crate::t1("sprint.describe.spot", "sentence", tune), crate::t2("sprint.check.wrong_note", "n", &s.wrong, "fix", fix))
}
