//! Place the note (#1226). A `staff-note` question shows an empty staff with
//! a `clef` (`treble` or `bass`) and a prompt naming a note; the learner taps
//! the staff, the note snaps to the nearest line or space, and they may drag
//! it, or move it with the arrow keys, before Check. `correct_answer` is one
//! note in ABC with its octave (`G` is the G on the treble staff's second
//! line, `c` the C an octave above middle C, `^F` F sharp, `_B` B flat). The
//! answer is right when it is the same note in the same octave, or in any
//! octave with `"any_octave": true`. The same note in another octave is a
//! near miss: wrong, with a note saying so. `static/staff-note.js` draws the
//! staff and sends the note placed, in ABC.

use std::collections::BTreeMap;

use crate::content::{ContentStore, QuizItem, QuizType};
use crate::quiz::Verdict;

/// How far a note may sit from the staff, in steps (a line or a space) from
/// its bottom line: two ledger lines below and above, and the space past
/// each. The same numbers are in static/staff-note.js.
const LOW: i32 = -5;
const HIGH: i32 = 13;

/// A note as written: its step (one per letter, middle C = 28) and its
/// accidental (-1 flat, 0 none, 1 sharp).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Pitch {
    step: i32,
    shift: i8,
}

/// One ABC note with its octave (`G`, `^F`, `_b`, `c'`, `C,`), no length.
fn pitch(s: &str) -> Option<Pitch> {
    let s = s.trim();
    let acc_len = s.chars().take_while(|c| matches!(c, '^' | '_' | '=')).count();
    let (acc, rest) = s.split_at(acc_len);
    let shift = match acc {
        "" | "=" => 0,
        "^" => 1,
        "_" => -1,
        _ => return None,
    };
    let mut chars = rest.chars();
    let c = chars.next()?;
    let letter = i32::try_from("CDEFGAB".find(c.to_ascii_uppercase())?).ok()?;
    let mut octave = if c.is_ascii_lowercase() { 5 } else { 4 };
    for m in chars {
        octave += match m {
            '\'' => 1,
            ',' => -1,
            _ => return None,
        };
    }
    Some(Pitch { step: octave * 7 + letter, shift })
}

/// The step of a clef's bottom line: E4 on a treble staff, G2 on a bass staff.
fn bottom_line(clef: &str) -> Option<i32> {
    match clef {
        "treble" => Some(30),
        "bass" => Some(18),
        _ => None,
    }
}

/// A step as ABC, without an accidental (`G,`, `c'`).
fn abc(step: i32) -> String {
    let octave = step.div_euclid(7);
    let letter = "CDEFGAB".chars().nth(usize::try_from(step.rem_euclid(7)).unwrap_or(0)).unwrap_or('C');
    if octave >= 5 {
        format!("{}{}", letter.to_ascii_lowercase(), "'".repeat(usize::try_from(octave - 5).unwrap_or(0)))
    } else {
        format!("{letter}{}", ",".repeat(usize::try_from(4 - octave).unwrap_or(0)))
    }
}

/// Validate a staff-note question: a clef, and answers that are one note
/// each, on the staff or its ledger lines.
pub fn check(q: &QuizItem) -> Result<(), String> {
    if !q.abc.is_empty() {
        return Err("a staff-note question draws its own staff: leave abc out".into());
    }
    let bottom = bottom_line(&q.clef).ok_or_else(|| format!("clef must be \"treble\" or \"bass\", found '{}'", q.clef))?;
    let (low, high) = (abc(bottom + LOW), abc(bottom + HIGH));
    for a in std::iter::once(&q.correct_answer).chain(&q.accepted_answers) {
        let p = pitch(a).ok_or_else(|| format!("answer '{a}' must be one note in ABC with its octave, such as G, ^F, _B or c"))?;
        if !(LOW..=HIGH).contains(&(p.step - bottom)) {
            return Err(format!("answer '{a}' is off the {} staff: notes from {low} to {high} fit", q.clef));
        }
    }
    Ok(())
}

/// Grade a staff-note answer: the same note and accidental, in the same
/// octave unless `any_octave`. The right note in another octave gets the
/// "wrong octave" note.
pub fn grade(q: &QuizItem, given: &str) -> Verdict {
    let Some(g) = pitch(given) else { return Verdict { correct: false, note: None } };
    let wants: Vec<Pitch> = std::iter::once(&q.correct_answer).chain(&q.accepted_answers).filter_map(|a| pitch(a)).collect();
    let same_letter = |w: &Pitch| w.shift == g.shift && w.step.rem_euclid(7) == g.step.rem_euclid(7);
    if wants.iter().any(|w| *w == g || (q.any_octave && same_letter(w))) {
        return Verdict { correct: true, note: None };
    }
    Verdict { correct: false, note: wants.iter().any(same_letter).then(|| crate::t("quiz.check.staff_octave")) }
}

/// What the page needs to draw a staff-note question: the clef, and every
/// note spelling (`C`, `^C`, `_C` … in ABC) in the course's note names, as
/// JSON, so the answer can be shown in them. Empty for any other question.
pub fn staff(content: &ContentStore, q: &QuizItem) -> (String, BTreeMap<String, String>) {
    if q.kind != QuizType::StaffNote {
        return (String::new(), BTreeMap::new());
    }
    let names = content.manifest().music.as_ref().map(|m| m.note_names).unwrap_or_default();
    let mut out = BTreeMap::new();
    for letter in ["C", "D", "E", "F", "G", "A", "B"] {
        for acc in ["", "^", "_"] {
            let n = format!("{acc}{letter}");
            if let Some(name) = super::note_name(&n, names) {
                out.insert(n, name);
            }
        }
    }
    (q.clef.clone(), out)
}

/// `staff` for a server page: the clef, and the note names as JSON for a data attribute.
pub fn view(content: &ContentStore, q: &QuizItem) -> (String, String) {
    let (clef, names) = staff(content, q);
    let json = if clef.is_empty() { String::new() } else { serde_json::to_string(&names).unwrap_or_default() };
    (clef, json)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(clef: &str, answer: &str, extra: serde_json::Value) -> QuizItem {
        let mut v = serde_json::json!({ "question_id": "q", "prompt": "Tap where G goes.", "type": "staff-note", "clef": clef, "correct_answer": answer });
        v.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn pitches_are_read_with_their_octave() {
        assert_eq!(pitch("C").unwrap().step, 28, "middle C");
        assert_eq!(pitch("c").unwrap().step, 35);
        assert_eq!(pitch("c'").unwrap().step, 42);
        assert_eq!(pitch("C,").unwrap().step, 21);
        assert_eq!(pitch("^F").unwrap(), Pitch { step: 31, shift: 1 });
        assert_eq!(pitch("_b").unwrap(), Pitch { step: 41, shift: -1 });
        for not_one in ["", "H", "^^F", "G2", "GA", "ab"] {
            assert!(pitch(not_one).is_none(), "{not_one}");
        }
        for s in [13, 21, 28, 30, 35, 43] {
            assert_eq!(pitch(&abc(s)).unwrap().step, s, "{}", abc(s));
        }
    }

    #[test]
    fn the_octave_counts_unless_any_octave() {
        let q = item("treble", "G", serde_json::json!({}));
        check(&q).unwrap();
        assert!(grade(&q, "G").correct);
        assert!(grade(&q, "=G").correct, "a natural sign is no accidental here");
        let octave = grade(&q, "g");
        assert!(!octave.correct);
        assert_eq!(octave.note.unwrap(), crate::t("quiz.check.staff_octave"));
        let off = grade(&q, "A");
        assert!(!off.correct && off.note.is_none());
        assert!(!grade(&q, "^G").correct, "G sharp is not G");
        assert!(!grade(&q, "nonsense").correct);
        let any = item("treble", "^F", serde_json::json!({ "any_octave": true }));
        check(&any).unwrap();
        for right in ["^F", "^f", "^F,"] {
            assert!(grade(&any, right).correct, "{right}");
        }
        assert!(!grade(&any, "F").correct && !grade(&any, "_G").correct, "the spelling counts");
    }

    #[test]
    fn the_validator_names_what_is_wrong() {
        let err = |clef: &str, answer: &str, extra: serde_json::Value| check(&item(clef, answer, extra)).unwrap_err();
        assert!(err("alto", "G", serde_json::json!({})).contains("clef must be"));
        assert!(err("", "G", serde_json::json!({})).contains("clef must be"));
        assert!(err("treble", "G A", serde_json::json!({})).contains("one note"));
        assert!(err("treble", "^^G", serde_json::json!({})).contains("one note"));
        assert!(err("treble", "e'", serde_json::json!({})).contains("off the treble staff: notes from G, to d' fit"));
        assert!(err("bass", "G", serde_json::json!({})).contains("off the bass staff: notes from B,,, to F fit"));
        assert!(err("treble", "G", serde_json::json!({ "accepted_answers": ["F,,"] })).contains("off the treble staff"));
        assert!(err("treble", "G", serde_json::json!({ "abc": "X:1\nK:C\nG" })).contains("leave abc out"));
        check(&item("bass", "C", serde_json::json!({}))).unwrap();
        check(&item("treble", "A,", serde_json::json!({}))).unwrap();
    }
}
