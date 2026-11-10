//! Music notation (#1176, #1218). A ```abc fenced block in a lesson's
//! Markdown is ABC notation; pulldown-cmark writes it as
//! `<pre><code class="language-abc">`. A quiz item's `abc` field is drawn
//! the same way above its prompt (`<pre class="abc-staff">` in the
//! templates, `abc` in the Sprint mode session JSON). `static/notation.js`
//! draws both as an SVG staff with abcjs (`static/vendor/`). Pages load those
//! two scripts only when there is music to draw.
//!
//! A music course says how notes are named on screen (`course.json`
//! `music.note_names`). In a multiple-choice question with `abc`, an option
//! written as an ABC note (`A`, `^F`, `_B`, `c`) is shown in those names.
//!
//! Sound (#1219): a quiz item's `play` is ABC text a speaker button plays,
//! and a Sprint sort item with `play` and no `text` is a speaker tile.
//! `static/music-sound.js` turns the ABC into Web Audio oscillator tones;
//! pages load it only when there is something to play. With `option_kind:
//! "abc"` the options of a multiple-choice question are ABC too, each drawn
//! as a staff with the header (key, metre, note length) of `play` or `abc`.
//!
//! Find it on the keys (#1231): a `keyboard` question is answered on a piano
//! keyboard (`static/keyboard.js`), whose keys sound through music-sound.js;
//! `keyboard.rs` reads and grades its answers.

use serde::{Deserialize, Serialize};

pub mod fill_bar;

pub mod chord_spell;

pub mod staff_note;

pub mod keyboard;

use crate::content::{Lesson, QuizItem, QuizType, SortItem};

/// What pulldown-cmark writes for a ```abc fenced block.
const ABC_BLOCK: &str = "<code class=\"language-abc\">";

impl Lesson {
    /// Does the lesson hold music to draw: a ```abc block in its text, a
    /// quiz item with music (`abc`, or `play`, drawn after the answer), a chord-spell question (its staff) or a scale?
    pub fn has_notation(&self) -> bool {
        self.sections.iter().any(|s| s.html.contains(ABC_BLOCK)) || self.quiz.iter().any(|q| q.has_music() || q.kind == QuizType::ChordSpell) || !self.sprint.scales.is_empty()
    }

    /// Does the lesson have something to play (a quiz item or sort item with
    /// `play`, a keyboard, whose keys sound, or a scale)?
    pub fn has_sound(&self) -> bool {
        self.quiz.iter().any(|q| !q.play.is_empty()) || self.has_keyboard() || self.sprint.sorts.iter().any(|s| s.items.iter().any(|i| !i.play.is_empty())) || !self.sprint.scales.is_empty()
    }

    /// Does the lesson ask a keyboard question (load static/keyboard.js)?
    pub fn has_keyboard(&self) -> bool {
        self.quiz.iter().any(|q| q.kind == QuizType::Keyboard)
    }

    /// Does the lesson ask a fill-bar question (load static/fill-bar.js)?
    pub fn has_fill_bar(&self) -> bool {
        self.quiz.iter().any(|q| q.kind == QuizType::FillBar)
    }

    /// Does the lesson ask a chord-spell question (load static/chord-spell.js)?
    pub fn has_chord_spell(&self) -> bool {
        self.quiz.iter().any(|q| q.kind == QuizType::ChordSpell)
    }

    /// Does the lesson ask a staff-note question (load static/staff-note.js)?
    pub fn has_staff_note(&self) -> bool {
        self.quiz.iter().any(|q| q.kind == QuizType::StaffNote)
    }
}

impl QuizItem {
    /// Music to draw or play: `abc`, `play` or staves as options.
    pub fn has_music(&self) -> bool {
        !self.abc.is_empty() || !self.play.is_empty() || self.abc_options()
    }

    /// Are the options ABC, each drawn as a staff (`option_kind: "abc"`)?
    pub fn abc_options(&self) -> bool {
        self.option_kind == "abc"
    }

    /// An option as a whole tune: the header lines (`X:`, `L:`, `K:` …) of
    /// `play` (else `abc`) and the option's notes.
    pub fn option_staff(&self, option: &str) -> String {
        let tune = if self.play.is_empty() { &self.abc } else { &self.play };
        let head: Vec<&str> = tune.lines().filter(|l| is_header(l)).collect();
        let head = if head.is_empty() { "X:1".to_string() } else { head.join("\n") };
        format!("{head}\n{option}")
    }

    /// The options as whole tunes (`option_staff`), when they are ABC; none otherwise.
    pub fn option_staves(&self, options: &[String]) -> Vec<String> {
        if self.abc_options() { options.iter().map(|o| self.option_staff(o)).collect() } else { vec![] }
    }
}

impl SortItem {
    /// What names the item when it is graded: its text, or for a speaker tile its music.
    pub fn key(&self) -> &str {
        if self.text.trim().is_empty() { &self.play } else { &self.text }
    }

    /// The bucket a learner put the item in (`placed`: answer text, trimmed, to bucket).
    /// A speaker tile's ABC is matched as written, since case and `'` `,` are its
    /// octave (`C`, `c`, `c'`); text is matched as `quiz::normalize` reads it.
    pub fn placed_in<'a>(&self, placed: &'a std::collections::HashMap<String, usize>) -> Option<&'a usize> {
        if self.text.trim().is_empty() {
            return placed.get(self.play.trim());
        }
        let want = crate::quiz::normalize(&self.text);
        placed.iter().find(|(t, _)| crate::quiz::normalize(t) == want).map(|(_, b)| b)
    }
}

/// The `i`-th option as a tune, for a template's options loop (askama hands it references).
pub fn nth<'a>(staves: &'a [String], i: &usize) -> Option<&'a String> {
    staves.get(*i)
}

/// An ABC header line: one letter, a colon (`K:C`, `L:1/4`).
fn is_header(line: &str) -> bool {
    let mut c = line.trim().chars();
    matches!((c.next(), c.next()), (Some(a), Some(':')) if a.is_ascii_alphabetic())
}

/// The music fields of a lesson: `option_kind` is `text` or `abc`, ABC options
/// only on a multiple-choice question with `play` or `abc` to take a header from;
/// a Sprint scale has eight notes and decoys that are not among them.
pub fn validate(lesson: &Lesson) -> Result<(), String> {
    crate::sprint::scale::validate(lesson)?;
    for q in &lesson.quiz {
        match q.option_kind.as_str() {
            "" | "text" => {}
            "abc" if q.kind == QuizType::MultipleChoice && (!q.play.is_empty() || !q.abc.is_empty()) => {}
            "abc" => return Err(format!("question '{}': option_kind \"abc\" needs a multiple-choice question with play or abc", q.question_id)),
            k => return Err(format!("question '{}': option_kind must be \"text\" or \"abc\", found '{k}'", q.question_id)),
        }
        if q.kind != QuizType::Keyboard && (q.octave || q.octaves.is_some()) {
            return Err(format!("question '{}': octave and octaves are for keyboard questions", q.question_id));
        }
    }
    Ok(())
}

/// `course.json` `music`: settings of a course that teaches music.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Music {
    #[serde(default)]
    pub note_names: NoteNames,
}

/// How note names are written on screen.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NoteNames {
    /// C D E F G A B, with ♯ and ♭.
    #[default]
    Letters,
    /// Do Re Mi Fa Sol La Si, with ♯ and ♭.
    Solfege,
    /// C D E F G A H, with -is and -es (Fis, Es, B for B flat).
    German,
}

/// An ABC note (`A`, `^F`, `__B`, `c'`, `C,`) in `names`; None when `s` is
/// not one note. The octave marks and the case (which octave) are not part
/// of a note's name.
pub fn note_name(s: &str, names: NoteNames) -> Option<String> {
    let s = s.trim();
    let acc_len = s.chars().take_while(|c| matches!(c, '^' | '_' | '=')).count();
    let (acc, rest) = s.split_at(acc_len);
    let mut chars = rest.chars();
    let letter = chars.next()?.to_ascii_uppercase();
    let octave: String = chars.collect();
    let i = "CDEFGAB".find(letter)?;
    if !octave.chars().all(|c| c == ',' || c == '\'') {
        return None;
    }
    // -2 double flat … 2 double sharp; None for a natural sign.
    let shift: Option<i8> = match acc {
        "" => Some(0),
        "^" => Some(1),
        "^^" => Some(2),
        "_" => Some(-1),
        "__" => Some(-2),
        "=" => None,
        _ => return None,
    };
    let sign = |n: Option<i8>| match n {
        Some(1) => "♯",
        Some(2) => "𝄪",
        Some(-1) => "♭",
        Some(-2) => "𝄫",
        None => "♮",
        _ => "",
    };
    Some(match names {
        NoteNames::Letters => format!("{letter}{}", sign(shift)),
        NoteNames::Solfege => format!("{}{}", ["Do", "Re", "Mi", "Fa", "Sol", "La", "Si"].get(i)?, sign(shift)),
        NoteNames::German => {
            let base = if letter == 'B' { 'H' } else { letter };
            match (letter, shift.unwrap_or(0)) {
                ('B', -1) => "B".to_string(),
                ('B', -2) => "Heses".to_string(),
                ('E' | 'A', -1) => format!("{base}s"),
                ('E' | 'A', -2) => format!("{base}ses"),
                (_, -1) => format!("{base}es"),
                (_, -2) => format!("{base}eses"),
                (_, 1) => format!("{base}is"),
                (_, 2) => format!("{base}isis"),
                _ => base.to_string(),
            }
        }
    })
}

/// Show the ABC-note options of every multiple-choice question with `abc`
/// or `play` (and text options) in the course's note names (the answer too, so grading is unchanged).
/// Two options that name the same note are an error.
pub fn name_notes(quiz: &mut [QuizItem], names: NoteNames) -> Result<(), String> {
    // A keyboard question is graded by its ABC answers; it shows the first in the course's names.
    for q in quiz.iter_mut().filter(|q| q.kind == QuizType::Keyboard) {
        q.keys = std::iter::once(&q.correct_answer).chain(&q.accepted_answers).cloned().collect();
        q.correct_answer = note_name(&q.correct_answer, names).unwrap_or_else(|| q.correct_answer.clone());
    }
    for q in quiz.iter_mut().filter(|q| (!q.abc.is_empty() || !q.play.is_empty()) && !q.abc_options() && q.kind == QuizType::MultipleChoice) {
        let name = |o: &str| note_name(o, names).unwrap_or_else(|| o.to_string());
        q.options = q.options.iter().map(|o| name(o)).collect();
        q.correct_answer = name(&q.correct_answer);
        let mut seen = std::collections::HashSet::new();
        if let Some(twice) = q.options.iter().find(|o| !seen.insert(o.as_str())) {
            return Err(format!("question '{}': two options name the note {twice}", q.question_id));
        }
    }
    Ok(())
}

/// The notes of an ABC tune in order, one entry per note or chord the way
/// abcjs draws them (`^F`, `c'`, `[CEG]`), so the n-th entry is the n-th
/// note a learner can tap. Header lines (`K:`, `w:`), rests, bar lines,
/// lengths, ties, chord symbols, decorations, grace notes and comments are
/// left out.
pub fn abc_notes(abc: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in abc.lines() {
        let line = line.split('%').next().unwrap_or("");
        let mut head = line.chars();
        if head.next().is_some_and(|c| c.is_ascii_alphabetic()) && head.next() == Some(':') {
            continue;
        }
        let mut it = line.chars().peekable();
        while let Some(c) = it.next() {
            match c {
                '"' | '!' | '+' | '{' => {
                    let close = if c == '{' { '}' } else { c };
                    it.by_ref().find(|&x| x == close);
                }
                // `[1` is a repeat ending and `[|` a bar line, not a chord.
                '[' if it.peek().is_some_and(|x| x.is_ascii_digit() || *x == '|') => {}
                '[' => {
                    let inner: String = it.by_ref().take_while(|&x| x != ']').collect();
                    // `[K:G]` is an inline field.
                    if !inner.contains(':') && inner.chars().any(|x| x.is_ascii_alphabetic() && "ABCDEFGabcdefg".contains(x)) {
                        out.push(format!("[{}]", inner.trim_end_matches(|x: char| x.is_ascii_digit() || x == '/')));
                    }
                }
                '^' | '_' | '=' | 'A'..='G' | 'a'..='g' => {
                    let mut note = c.to_string();
                    while !note.ends_with(|x: char| x.is_ascii_alphabetic()) {
                        match it.next_if(|x| matches!(x, '^' | '_' | '=' | 'A'..='G' | 'a'..='g')) {
                            Some(x) => note.push(x),
                            None => break,
                        }
                    }
                    if note.ends_with(|x: char| x.is_ascii_alphabetic()) {
                        while let Some(x) = it.next_if(|x| matches!(x, ',' | '\'')) {
                            note.push(x);
                        }
                        out.push(note);
                    }
                }
                _ => {}
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{abc_notes, note_name, NoteNames::*};
    use crate::content::render_markdown;

    #[test]
    fn an_abc_block_is_marked_and_a_plain_code_block_is_not() {
        assert!(render_markdown("```abc\nX:1\nK:C\nCDE|\n```").contains(super::ABC_BLOCK));
        assert!(!render_markdown("```\nX:1\n```\n\n`abc`").contains(super::ABC_BLOCK));
    }

    #[test]
    fn the_notes_of_a_tune_are_counted_the_way_the_staff_draws_them() {
        assert_eq!(abc_notes("X:1\nT:C major\nL:1/8\nK:C\nCDEG GABc|"), ["C", "D", "E", "G", "G", "A", "B", "c"]);
        let tune = "X:1\nK:G\n\"G\"!p!G2 ^F/ z [CEG]2 | {g}A,- A,3/2 c'' x2 [K:D] _B =e [|[1 d :|\nw: la la la\nC % C in a comment";
        assert_eq!(abc_notes(tune), ["G", "^F", "[CEG]", "A,", "A,", "c''", "_B", "=e", "d", "C"]);
        assert!(abc_notes("X:1\nK:C\nz4 | x2 |]").is_empty());
    }

    #[test]
    fn notes_are_named_in_the_course_naming() {
        let names = |s: &str| [Letters, Solfege, German].map(|n| note_name(s, n).unwrap_or_default());
        assert_eq!(names("A"), ["A", "La", "A"]);
        assert_eq!(names("c'"), ["C", "Do", "C"]);
        assert_eq!(names("^F"), ["F♯", "Fa♯", "Fis"]);
        assert_eq!(names("_B"), ["B♭", "Si♭", "B"]);
        assert_eq!(names("B,"), ["B", "Si", "H"]);
        assert_eq!(names("_E"), ["E♭", "Mi♭", "Es"]);
        assert_eq!(names("_A"), ["A♭", "La♭", "As"]);
        assert_eq!(names("_D"), ["D♭", "Re♭", "Des"]);
        assert_eq!(names("^^C"), ["C𝄪", "Do𝄪", "Cisis"]);
        assert_eq!(names("=B"), ["B♮", "Si♮", "H"]);
        for not_a_note in ["", "H", "minor 3rd", "G major", "^", "A2", "^_A", "Ab"] {
            assert_eq!(note_name(not_a_note, Letters), None, "{not_a_note}");
        }
    }

    #[test]
    fn staves_as_options_take_the_header_of_play_and_are_checked() {
        let item = |v: serde_json::Value| -> crate::content::QuizItem {
            let mut q = serde_json::json!({ "question_id": "q", "prompt": "Which melody plays?", "type": "multiple-choice", "options": ["CDEC", "CEDC"], "correct_answer": "CDEC" });
            q.as_object_mut().unwrap().extend(v.as_object().unwrap().clone());
            serde_json::from_value(q).unwrap()
        };
        let q = item(serde_json::json!({ "play": "X:1\nL:1/4\nK:C\nCDEC", "option_kind": "abc" }));
        assert_eq!(q.option_staves(&q.options), ["X:1\nL:1/4\nK:C\nCDEC", "X:1\nL:1/4\nK:C\nCEDC"]);
        assert!(q.has_music());
        let plain = item(serde_json::json!({ "play": "X:1\nK:C\nC G" }));
        assert!(plain.option_staves(&plain.options).is_empty(), "text options stay text");
        let lesson = |q: crate::content::QuizItem| -> crate::content::Lesson {
            let mut l: crate::content::Lesson = serde_json::from_value(serde_json::json!({ "id": "l", "slug": "l", "title": "L", "unit": "u", "content": "" })).unwrap();
            l.quiz = vec![q];
            l
        };
        assert!(super::validate(&lesson(q)).is_ok());
        assert!(super::validate(&lesson(item(serde_json::json!({ "option_kind": "abc" })))).unwrap_err().contains("needs a multiple-choice question with play or abc"));
        assert!(super::validate(&lesson(item(serde_json::json!({ "option_kind": "staff" })))).unwrap_err().contains("found 'staff'"));
        let tile: crate::content::SortItem = serde_json::from_value(serde_json::json!({ "play": "[CEG]2", "bucket": 0 })).unwrap();
        assert_eq!(tile.key(), "[CEG]2");
    }

    #[test]
    fn a_question_with_music_shows_its_note_options_in_the_course_names() {
        let item = |abc: &str, options: &[&str]| -> crate::content::QuizItem {
            serde_json::from_value(serde_json::json!({ "question_id": "q", "prompt": "Which note is this?", "type": "multiple-choice", "abc": abc, "options": options, "correct_answer": "A" })).unwrap()
        };
        let mut quiz = vec![item("X:1\nK:C\nA", &["F", "^F", "A", "a perfect 5th"]), item("", &["F", "A"])];
        super::name_notes(&mut quiz, Solfege).unwrap();
        assert_eq!(quiz[0].options, ["Fa", "Fa♯", "La", "a perfect 5th"]);
        assert_eq!(quiz[0].correct_answer, "La");
        assert_eq!(quiz[1].options, ["F", "A"], "a question without abc keeps its options");
        let mut twice = vec![item("X:1\nK:C\nA", &["A", "a"])];
        assert!(super::name_notes(&mut twice, Letters).unwrap_err().contains("two options name the note A"));
    }
}
