//! Sprint mode: five-minute, phone-first sessions built from the same lessons.
//!
//! A *session* is an ordered list of exercises, serialised to JSON for the
//! player in `static/sprint.js`. Two kinds of exercise exist:
//!
//! - Quiz questions (`choice`, `tiles`) are the lesson's own `quiz` items and
//!   are graded through the normal `POST /api/answer`, so mastery, history and
//!   vocabulary collection work exactly as in Practice or the Guide.
//! - Sprint mode exercises (`sort`, `spot`, `build`, `ladder`, `scale`, `pick`, `match`) are
//!   graded by `POST /api/sprint/check`. Their ids encode where the content came
//!   from (`sort:<lesson id>:<n>`, `ladder:<lesson id>:<item>:<set>`, …) so
//!   the server can rebuild the answer key without keeping any session state.
//!   Rule exercises feed the lesson's mastery score like a quiz answer;
//!   vocabulary exercises collect words and count as flashcard reviews.
//! - Listening exercises (`hear-build`, `hear-gap`, `hear-pick`, `hear-which`, `hear-match`), for
//!   language courses only: the browser speaks the text (`static/speech.js`) and the learner taps
//!   or types what they heard. Sentence ones come from the lesson's `listening` sentences and count
//!   towards mastery; word ones from its `vocabulary`, as flashcard reviews. Skipped when audio is off.
//!
//! Finishing a session (`POST /api/sprint/finish`) records the run in
//! `game_activity` and, for a lesson session, completes the Guide stage.

use rusqlite::Connection;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

use crate::content::{words_of, Conjugation, ContentStore, Lesson, QuizItem, QuizType, VocabEntry};
use crate::mastery::Progress;
use crate::quiz::{build_quiz, normalize, strip_accents, Rng};

pub const GAME: &str = "sprint";
pub const BOARD_REVIEW: &str = "review";
pub const BOARD_LIGHTNING: &str = "lightning";
/// Lightning round length.
pub const LIGHTNING_SECS: u32 = 60;
/// Quiz questions in a lesson session.
const LESSON_QUESTIONS: usize = 4;
const MAX_TILES: usize = 6;
const MATCH_PAIRS: usize = 5;

// ---------- exercises ----------

#[derive(Serialize, Clone, Debug)]
pub struct MatchPair {
    pub term: String,
    pub meaning: String,
}

/// A sort item with its answer key: the player gives instant feedback per tap
/// (the server still re-grades the final state and the mistake count).
#[derive(Serialize, Clone, Debug)]
pub struct SortItemView {
    pub text: String,
    pub bucket: usize,
    #[serde(skip_serializing_if = "String::is_empty")] pub play: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Exercise {
    /// The idea in one breath. Not graded.
    Bite { title: String, rule: String, example: String, example_meaning: String, emoji: String },
    /// Multiple-choice quiz question (graded by /api/answer). `abc`: music drawn above the prompt; `play`: music a speaker plays; `staves`: the options as tunes.
    Choice { id: String, lesson: String, prompt: String, options: Vec<String>, hint: String, #[serde(skip_serializing_if = "String::is_empty")] abc: String, #[serde(skip_serializing_if = "String::is_empty")] play: String, #[serde(skip_serializing_if = "Vec::is_empty")] staves: Vec<String> },
    /// Set the clock (graded by /api/answer): drag the hands in `step`s; or,
    /// with `options`, read the clock drawn at `hands` ("hour minute" degrees).
    Clock { id: String, lesson: String, prompt: String, step: u32, hands: String, options: Vec<String>, hint: String },
    /// Fill-in-blank quiz question offered as tiles, typing optional (graded by /api/answer).
    /// Maths questions set `math` (no speech, no accent bar); with no tiles
    /// (an expression) the answer is typed.
    Tiles {
        id: String, lesson: String, prompt: String, tiles: Vec<String>, hint: String,
        #[serde(skip_serializing_if = "std::ops::Not::not")] math: bool,
        #[serde(skip_serializing_if = "String::is_empty")] unit: String,
        #[serde(skip_serializing_if = "String::is_empty")] placeholder: String,
        #[serde(skip_serializing_if = "String::is_empty")] abc: String, #[serde(skip_serializing_if = "String::is_empty")] play: String,
    },
    /// Fill the bar: tap note-value `tiles` into the gap of `abc` (graded by /api/answer; `notation::fill_bar`).
    FillBar { id: String, lesson: String, prompt: String, abc: String, tiles: Vec<String>, hint: String },
    /// Spell the chord: tap note `tiles` into `slots` (graded by /api/answer; `notation::chord_spell`).
    ChordSpell { id: String, lesson: String, prompt: String, tiles: Vec<crate::notation::chord_spell::Tile>, slots: usize, hint: String },
    /// Place the note on a staff (graded by /api/answer; `notation::staff_note`).
    StaffNote { id: String, lesson: String, prompt: String, clef: String, names: std::collections::BTreeMap<String, String>, hint: String },
    /// Find it on the keys: tap a key of a piano keyboard of `octaves` octaves (graded by /api/answer; `notation::keyboard`).
    Keyboard { id: String, lesson: String, prompt: String, hint: String, #[serde(skip_serializing_if = "String::is_empty")] abc: String, #[serde(skip_serializing_if = "String::is_empty")] play: String, octaves: u8, octave: bool },
    /// Tap each item into the right bucket.
    Sort { id: String, lesson: String, title: String, buckets: Vec<String>, items: Vec<SortItemView> },
    /// Tap the one wrong word, or the wrong note on the staff (`abc`) and its fix from `tiles`.
    Spot { id: String, lesson: String, words: Vec<String>, #[serde(skip_serializing_if = "String::is_empty")] abc: String, #[serde(skip_serializing_if = "Vec::is_empty")] tiles: Vec<spot::FixTile> },
    /// Rebuild the sentence from a word bank.
    Build { id: String, lesson: String, meaning: String, tiles: Vec<String> },
    /// Put the forms of an item on the right rungs (`forms` is the key, in `slots` order).
    Ladder { id: String, lesson: String, item: String, meaning: String, set: String, slots: Vec<String>, forms: Vec<String>, tiles: Vec<String> },
    /// Build the scale (`scale.rs`): the ladder with notes, the first rung filled; `notes` and `tile_notes` are the ABC of `forms` and `tiles`.
    Scale { id: String, lesson: String, title: String, slots: Vec<String>, forms: Vec<String>, notes: Vec<String>, tiles: Vec<String>, tile_notes: Vec<String> },
    /// Pick the translation of a word.
    Pick { id: String, lesson: String, prompt: String, options: Vec<String>, to_term: bool },
    /// Match words with their meanings.
    Match { id: String, lesson: String, pairs: Vec<MatchPair> },
    /// Tap what you hear: the sentence is spoken; rebuild it from the tiles.
    HearBuild { id: String, lesson: String, text: String, meaning: String, tiles: Vec<String> },
    /// Fill the gap from what you hear: the sentence is spoken and shown with
    /// word `gap` missing; pick it from the tiles.
    HearGap { id: String, lesson: String, text: String, gap: usize, tiles: Vec<String> },
    /// What do you hear?: `text` is spoken; pick it among written options
    /// (`to_meaning`: pick what it means instead).
    HearPick { id: String, lesson: String, text: String, options: Vec<String>, to_meaning: bool },
    /// Select the matching audio: a meaning is shown; each option is spoken,
    /// pick the one that says it.
    HearWhich { id: String, lesson: String, prompt: String, options: Vec<String> },
    /// Listen and match: spoken terms on one side, the same terms written on the other.
    HearMatch { id: String, lesson: String, terms: Vec<String> },
}

/// Course-level bits the player needs; `say_lang` (words are read aloud in it) is empty outside a language course.
#[derive(Serialize, Debug, Clone)]
pub struct CourseInfo {
    pub id: String,
    pub title: String,
    pub term_label: String,
    pub term_lang: String,
    pub meaning_label: String,
    pub meaning_lang: String,
    pub say_lang: String,
    pub chars: Vec<String>,
    pub praise: Vec<String>,
    pub almost: Vec<String>,
    /// The app's icons (streak, words, the modes…).
    pub icons: crate::content::Icons,
}

impl CourseInfo {
    pub fn of(content: &ContentStore) -> CourseInfo {
        let m = content.manifest();
        CourseInfo {
            id: m.id.clone(),
            title: m.title.clone(),
            term_label: m.terms.label.clone(),
            term_lang: m.terms.lang.clone(),
            meaning_label: m.meanings.label.clone(),
            meaning_lang: m.meanings.lang.clone(),
            say_lang: if m.is_language() { m.terms.lang.clone() } else { String::new() },
            chars: m.chars.clone(),
            praise: content.praise().to_vec(),
            almost: content.almost().to_vec(),
            icons: crate::content::Icons::default(),
        }
    }
}

#[derive(Serialize, Debug)]
pub struct Session {
    pub mode: &'static str,
    pub title: String,
    pub subtitle: String,
    pub lesson_id: Option<String>,
    pub slug: Option<String>,
    pub timed_secs: Option<u32>,
    pub course: CourseInfo,
    pub exercises: Vec<Exercise>,
}

/// Plain text of a rendered Markdown fragment (tags dropped, entities decoded).
pub fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    let out = out
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A term's example with its translation, for the explanation after a vocabulary exercise.
fn example_with_meaning(v: &VocabEntry) -> Option<String> {
    let e = v.example.as_deref().filter(|e| !e.trim().is_empty())?;
    Some(match v.example_meaning.as_deref().filter(|m| !m.trim().is_empty()) {
        Some(m) => format!("{e} — {m}"),
        None => e.to_string(),
    })
}

fn truncate_words(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out = String::new();
    for w in s.split(' ') {
        if out.chars().count() + w.chars().count() + 1 > max.saturating_sub(1) {
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(w);
    }
    out.push('…');
    out
}

/// Bites: hand-written ones, or derived from the lesson's sections.
pub fn bites_for(lesson: &Lesson, icons: &crate::content::Icons) -> Vec<Exercise> {
    if !lesson.sprint.bites.is_empty() {
        return lesson
            .sprint
            .bites
            .iter()
            .map(|b| Exercise::Bite {
                title: b.title.clone(),
                rule: b.rule.clone(),
                example: b.example.clone(),
                example_meaning: b.example_meaning.clone(),
                emoji: if b.emoji.is_empty() { icons.bite.clone() } else { b.emoji.clone() },
            })
            .collect();
    }
    lesson
        .sections
        .iter()
        .filter(|s| !s.heading.to_lowercase().starts_with("quick check"))
        .take(3)
        .map(|s| {
            // First paragraph only; tables and lists make poor one-liners.
            let first = s.html.split("</p>").next().unwrap_or("");
            Exercise::Bite {
                title: s.heading.clone(),
                rule: truncate_words(&strip_tags(first), 200),
                example: String::new(),
                example_meaning: String::new(),
                emoji: icons.bite.clone(),
            }
        })
        .collect()
}

fn push_unique(tiles: &mut Vec<String>, seen: &mut HashSet<String>, t: &str) {
    let key = normalize(t);
    if !key.is_empty() && seen.insert(key) {
        tiles.push(t.to_string());
    }
}

/// A quiz item as a one-tap exercise. Fill-in-blank questions become a word
/// bank (`gap::gap_tiles`), or a typed answer when too few tiles are found.
pub fn quiz_exercise(content: &ContentStore, lesson: &Lesson, item: &QuizItem, rng: &mut Rng) -> Exercise {
    let hint = item.hints.first().cloned().unwrap_or_default();
    match item.kind {
        QuizType::Numeric | QuizType::Expression | QuizType::AlgebraTiles | QuizType::Measure => {
            let tiles = if item.kind == QuizType::Numeric { numeric_tiles(lesson, item, rng) } else { Vec::new() };
            Exercise::Tiles {
                id: item.question_id.clone(),
                lesson: lesson.title.clone(),
                prompt: item.prompt.clone(),
                tiles, hint, abc: item.abc.clone(), play: item.play.clone(),
                math: true,
                unit: item.unit.clone(),
                placeholder: item.kind.placeholder().to_string(),
            }
        }
        QuizType::MultipleChoice | QuizType::TrueFalse | QuizType::Clock => {
            let mut options = item.options.clone();
            rng.shuffle(&mut options);
            let (play, staves) = (item.play.clone(), item.option_staves(&options));
            if item.kind == QuizType::Clock {
                let (step, hands) = (item.clock_step(), item.clock_hands());
                return Exercise::Clock { id: item.question_id.clone(), lesson: lesson.title.clone(), prompt: item.prompt.clone(), step, hands, options, hint };
            }
            Exercise::Choice { id: item.question_id.clone(), lesson: lesson.title.clone(), prompt: item.prompt.clone(), options, hint, abc: item.abc.clone(), play, staves }
        }
        QuizType::FillBar => Exercise::FillBar { id: item.question_id.clone(), lesson: lesson.title.clone(), prompt: item.prompt.clone(), abc: item.abc.clone(), tiles: { let mut t = item.tiles.clone(); rng.shuffle(&mut t); t }, hint },
        QuizType::ChordSpell => Exercise::ChordSpell { id: item.question_id.clone(), lesson: lesson.title.clone(), prompt: item.prompt.clone(), tiles: crate::notation::chord_spell::tiles(content, item, rng), slots: crate::notation::chord_spell::slots(item), hint },
        QuizType::StaffNote => { let (clef, names) = crate::notation::staff_note::staff(content, item); Exercise::StaffNote { id: item.question_id.clone(), lesson: lesson.title.clone(), prompt: item.prompt.clone(), clef, names, hint } }
        QuizType::Keyboard => Exercise::Keyboard { id: item.question_id.clone(), lesson: lesson.title.clone(), prompt: item.prompt.clone(), hint, abc: item.abc.clone(), play: item.play.clone(), octaves: crate::notation::keyboard::octaves(item), octave: item.octave },
        QuizType::FillInBlank => {
            let tiles = gap::gap_tiles(content, lesson, item, rng);
            Exercise::Tiles {
                id: item.question_id.clone(),
                lesson: lesson.title.clone(),
                prompt: item.prompt.clone(),
                tiles, hint, abc: item.abc.clone(), play: item.play.clone(),
                math: false,
                unit: String::new(),
                placeholder: String::new(),
            }
        }
    }
}

/// Number tiles for a numeric question: the answer plus the classic slips
/// (off by one, wrong sign, a power of ten, doubled or halved) written the
/// same way as the answer, and other numeric answers of the lesson. A list
/// answer (`2; -3`) gets no tiles: it is typed.
fn numeric_tiles(lesson: &Lesson, item: &QuizItem, rng: &mut Rng) -> Vec<String> {
    let answer = item.correct_answer.trim();
    if answer.contains(';') {
        return Vec::new();
    }
    let Ok(v) = crate::mathcheck::number(answer) else { return Vec::new() };
    let mut tiles = Vec::new();
    let mut seen = HashSet::new();
    push_unique(&mut tiles, &mut seen, answer);
    // The value is what counts: never offer a second tile worth the answer,
    // or worth one of its accepted answers (#680), or graded right at all
    // (a slip inside the question's tolerance).
    let mut values: Vec<f64> = vec![v];
    values.extend(item.accepted_answers.iter().filter_map(|a| crate::mathcheck::number(a.trim()).ok()));
    let mut offer = |tiles: &mut Vec<String>, seen: &mut HashSet<String>, t: String| {
        if let Ok(x) = crate::mathcheck::number(&t) {
            if values.iter().all(|y| (x - y).abs() > 1e-9 * y.abs().max(1.0)) && !crate::quiz::check_answer(item, &t).correct {
                values.push(x);
                push_unique(tiles, seen, &t);
            }
        }
    };
    let mut slips: Vec<String> = Vec::new();
    if let Some((n, d)) = answer.split_once('/') {
        if let (Ok(n), Ok(d)) = (n.trim().parse::<i64>(), d.trim().parse::<i64>()) {
            slips.extend([format!("{d}/{n}"), format!("{}/{d}", n + 1), format!("{n}/{}", d + 1), format!("{}/{d}", n - 1)]);
        }
    } else {
        let places = answer.split_once('.').map(|(_, f)| f.chars().take_while(|c| c.is_ascii_digit()).count()).unwrap_or(0);
        let fmt = |x: f64| format!("{x:.places$}");
        let step = 10f64.powi(-(places as i32));
        slips.extend([fmt(v + step), fmt(v - step), fmt(-v), fmt(v * 10.0), fmt(v / 10.0), fmt(v * 2.0), fmt(v / 2.0)]);
    }
    rng.shuffle(&mut slips);
    let mut others: Vec<&str> = lesson
        .quiz
        .iter()
        .filter(|q| q.kind == QuizType::Numeric && q.question_id != item.question_id && !q.correct_answer.contains(';'))
        .map(|q| q.correct_answer.as_str())
        .collect();
    rng.shuffle(&mut others);
    // Two from the lesson's other answers at most, the rest slips.
    for t in others.into_iter().take(2).map(str::to_string).chain(slips) {
        if tiles.len() >= 4 {
            break;
        }
        offer(&mut tiles, &mut seen, t);
    }
    rng.shuffle(&mut tiles);
    tiles
}

pub fn sort_exercise(lesson: &Lesson, i: usize, rng: &mut Rng) -> Option<Exercise> {
    let s = lesson.sprint.sorts.get(i)?;
    let mut items: Vec<SortItemView> = s.items.iter().map(|it| SortItemView { text: it.text.clone(), bucket: it.bucket, play: it.play.clone() }).collect();
    rng.shuffle(&mut items);
    Some(Exercise::Sort {
        id: format!("sort:{}:{}", lesson.id, i),
        lesson: lesson.title.clone(),
        title: s.title.clone(),
        buckets: s.buckets.clone(),
        items,
    })
}

pub fn build_exercise(lesson: &Lesson, i: usize, rng: &mut Rng) -> Option<Exercise> {
    let b = lesson.sprint.builds.get(i)?;
    let mut tiles = words_of(&b.text);
    tiles.extend(b.distractors.iter().cloned());
    rng.shuffle(&mut tiles);
    Some(Exercise::Build { id: format!("build:{}:{}", lesson.id, i), lesson: lesson.title.clone(), meaning: b.meaning.clone(), tiles })
}

/// Sentence building from a conversation line with 3–9 words and a meaning.
pub fn conversation_build(lesson: &Lesson, ci: usize, li: usize, rng: &mut Rng) -> Option<Exercise> {
    let line = lesson.conversations.get(ci)?.lines.get(li)?;
    let mut tiles = words_of(&line.text);
    if !(3..=9).contains(&tiles.len()) || line.meaning.trim().is_empty() {
        return None;
    }
    rng.shuffle(&mut tiles);
    Some(Exercise::Build { id: format!("cbuild:{}:{}:{}", lesson.id, ci, li), lesson: lesson.title.clone(), meaning: line.meaning.clone(), tiles })
}

/// All conversation lines of a lesson that make a decent build.
fn buildable_lines(lesson: &Lesson) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for (ci, c) in lesson.conversations.iter().enumerate() {
        for (li, l) in c.lines.iter().enumerate() {
            if (3..=9).contains(&words_of(&l.text).len()) && !l.meaning.trim().is_empty() {
                out.push((ci, li));
            }
        }
    }
    out
}

pub fn ladder_exercise(content: &ContentStore, lesson: &Lesson, k: &Conjugation, rng: &mut Rng) -> Option<Exercise> {
    let p = content.paradigm()?;
    let forms: Vec<String> = p.slots.iter().map(|s| k.form(&s.key).to_string()).collect();
    let mut tiles = forms.clone();
    rng.shuffle(&mut tiles);
    Some(Exercise::Ladder {
        id: format!("ladder:{}:{}:{}", lesson.id, k.item, k.set),
        lesson: lesson.title.clone(),
        item: k.item.clone(),
        meaning: k.meaning.clone(),
        set: p.set_label(&k.set).to_string(),
        slots: p.slots.iter().map(|s| s.label.clone()).collect(),
        forms, tiles,
    })
}

/// Pick the translation of `lesson.vocabulary[idx]` among four options.
pub fn pick_exercise(lesson: &Lesson, idx: usize, to_term: bool, rng: &mut Rng) -> Option<Exercise> {
    let target = lesson.vocabulary.get(idx)?;
    let side = |v: &VocabEntry| if to_term { v.term.clone() } else { v.meaning.clone() };
    let mut options = vec![side(target)];
    let mut seen: HashSet<String> = options.iter().map(|o| normalize(o)).collect();
    let mut others: Vec<&VocabEntry> = lesson.vocabulary.iter().enumerate().filter(|(i, _)| *i != idx).map(|(_, v)| v).collect();
    rng.shuffle(&mut others);
    for v in others {
        if options.len() >= 4 {
            break;
        }
        let o = side(v);
        if seen.insert(normalize(&o)) {
            options.push(o);
        }
    }
    if options.len() < 2 {
        return None;
    }
    rng.shuffle(&mut options);
    Some(Exercise::Pick {
        id: format!("pick:{}:{}:{}", if to_term { "s" } else { "e" }, lesson.id, idx),
        lesson: lesson.title.clone(),
        prompt: if to_term { target.meaning.clone() } else { target.term.clone() },
        options, to_term,
    })
}

/// Five pairs from the lesson's vocabulary (distinct meanings).
pub fn match_exercise(lesson: &Lesson, rng: &mut Rng) -> Option<Exercise> {
    let mut entries: Vec<&VocabEntry> = lesson.vocabulary.iter().collect();
    rng.shuffle(&mut entries);
    let mut pairs = Vec::new();
    let mut seen_meaning = HashSet::new();
    let mut seen_term = HashSet::new();
    for v in entries {
        if pairs.len() >= MATCH_PAIRS {
            break;
        }
        if seen_meaning.insert(normalize(&v.meaning)) && seen_term.insert(normalize(&v.term)) {
            pairs.push(MatchPair { term: v.term.clone(), meaning: v.meaning.clone() });
        }
    }
    if pairs.len() < 3 {
        return None;
    }
    Some(Exercise::Match { id: format!("match:{}", lesson.id), lesson: lesson.title.clone(), pairs })
}

// ---------- listening exercises ----------

/// A word without the punctuation around it, case kept ("azul." -> "azul").
fn bare(word: &str) -> String {
    word.trim_matches(|c: char| !c.is_alphanumeric()).to_string()
}

/// A sentence as word tiles: split on spaces, the punctuation around each
/// word taken off ("¿Dónde", "mano." -> "Dónde", "mano"), what is inside a
/// word kept (l'eau, peut-être), a bare punctuation mark ("—") dropped.
fn tile_words(text: &str) -> Vec<String> {
    words_of(text).iter().map(|w| bare(w)).filter(|w| !w.is_empty()).collect()
}

/// A built sentence for comparing: its tile words, normalized, so the
/// learner's tiles match the sentence whatever its punctuation.
fn tile_key(text: &str) -> String {
    tile_words(text).iter().map(|w| normalize(w)).collect::<Vec<_>>().join(" ")
}

/// Words of the lesson's other listening sentences that are not in `text`,
/// each once, shuffled: tempting tiles for a sentence exercise.
fn other_words(lesson: &Lesson, text: &str, rng: &mut Rng) -> Vec<String> {
    let used: HashSet<String> = words_of(text).iter().map(|w| normalize(w)).collect();
    let mut seen = used.clone();
    let mut out = Vec::new();
    for item in lesson.listening.iter().filter(|i| i.text != text) {
        for w in words_of(&item.text) {
            let b = bare(&w);
            let key = normalize(&b);
            if key.chars().count() >= 2 && seen.insert(key) {
                out.push(b);
            }
        }
    }
    rng.shuffle(&mut out);
    out
}

/// Listening sentences short enough for word tiles (2–10 words).
fn tileable_sentences(lesson: &Lesson) -> Vec<usize> {
    lesson.listening.iter().enumerate().filter(|(_, i)| (2..=10).contains(&tile_words(&i.text).len())).map(|(n, _)| n).collect()
}

/// Tap what you hear: `listening[n]` as word tiles plus a few words from the
/// lesson's other sentences.
pub fn hear_build(lesson: &Lesson, n: usize, rng: &mut Rng) -> Option<Exercise> {
    let item = lesson.listening.get(n)?;
    let mut tiles = tile_words(&item.text);
    if !(2..=10).contains(&tiles.len()) {
        return None;
    }
    tiles.extend(other_words(lesson, &item.text, rng).into_iter().take(3));
    rng.shuffle(&mut tiles);
    Some(Exercise::HearBuild { id: format!("hbuild:{}:{n}", lesson.id), lesson: lesson.title.clone(), text: item.text.clone(), meaning: item.meaning.clone(), tiles })
}

/// The word of a sentence worth hiding: one of the lesson's vocabulary terms
/// if any appears, else a word of four letters or more, else any word.
fn gap_word(lesson: &Lesson, words: &[String], rng: &mut Rng) -> Option<usize> {
    let terms: HashSet<String> = lesson.vocabulary.iter().flat_map(|v| v.term.split_whitespace().map(normalize)).filter(|t| t.chars().count() >= 3).collect();
    let keys: Vec<String> = words.iter().map(|w| normalize(w)).collect();
    let pick = |ok: &dyn Fn(&str) -> bool, rng: &mut Rng| -> Option<usize> {
        let c: Vec<usize> = keys.iter().enumerate().filter(|(_, k)| ok(k)).map(|(i, _)| i).collect();
        (!c.is_empty()).then(|| c[rng.below(c.len())])
    };
    pick(&|k| terms.contains(k), rng)
        .or_else(|| pick(&|k| k.chars().count() >= 4, rng))
        .or_else(|| pick(&|k| !k.is_empty(), rng))
}

/// Fill the gap from what you hear: `listening[n]` with word `gap` (chosen
/// here when `None`) to pick from four tiles: the word, its accent-less
/// spelling and words from the lesson's other sentences of a similar length.
pub fn hear_gap(lesson: &Lesson, n: usize, gap: Option<usize>, rng: &mut Rng) -> Option<Exercise> {
    let item = lesson.listening.get(n)?;
    let words = words_of(&item.text);
    let gap = match gap {
        Some(g) if g < words.len() => g,
        Some(_) => return None,
        None => gap_word(lesson, &words, rng)?,
    };
    let word = bare(&words[gap]);
    let mut tiles = Vec::new();
    let mut seen = HashSet::new();
    push_unique(&mut tiles, &mut seen, &word);
    let plain = strip_accents(&word);
    if plain != word {
        push_unique(&mut tiles, &mut seen, &plain);
    }
    let len = word.chars().count();
    let mut others = other_words(lesson, &item.text, rng);
    others.sort_by_key(|w| (w.chars().count() as i64 - len as i64).abs());
    for w in others {
        if tiles.len() >= 4 {
            break;
        }
        push_unique(&mut tiles, &mut seen, &w);
    }
    if tiles.len() < 2 {
        return None;
    }
    rng.shuffle(&mut tiles);
    Some(Exercise::HearGap { id: format!("hgap:{}:{n}:{gap}", lesson.id), lesson: lesson.title.clone(), text: item.text.clone(), gap, tiles })
}

/// Decoy terms for `lesson.vocabulary[idx]`: the ones that look most alike first.
fn lookalike_terms(lesson: &Lesson, idx: usize, rng: &mut Rng) -> Vec<String> {
    let target = &lesson.vocabulary[idx].term;
    let tk = strip_accents(&normalize(target));
    let mut others: Vec<&VocabEntry> = lesson.vocabulary.iter().enumerate().filter(|(i, _)| *i != idx).map(|(_, v)| v).collect();
    rng.shuffle(&mut others);
    // Same first letter and a similar length sound closest.
    others.sort_by_key(|v| {
        let k = strip_accents(&normalize(&v.term));
        let first = usize::from(k.chars().next() != tk.chars().next());
        first * 10 + (k.chars().count() as i64 - tk.chars().count() as i64).unsigned_abs() as usize
    });
    others.into_iter().map(|v| v.term.clone()).collect()
}

/// What do you hear?: `vocabulary[idx]` spoken; pick the written term among
/// its accent-less spelling and look-alike terms of the lesson.
pub fn hear_pick(lesson: &Lesson, idx: usize, rng: &mut Rng) -> Option<Exercise> {
    let v = lesson.vocabulary.get(idx)?;
    let mut options = Vec::new();
    let mut seen = HashSet::new();
    push_unique(&mut options, &mut seen, &v.term);
    let plain = strip_accents(&v.term);
    if plain != v.term {
        push_unique(&mut options, &mut seen, &plain);
    }
    for t in lookalike_terms(lesson, idx, rng) {
        if options.len() >= 4 {
            break;
        }
        push_unique(&mut options, &mut seen, &t);
    }
    if options.len() < 2 {
        return None;
    }
    rng.shuffle(&mut options);
    Some(Exercise::HearPick { id: format!("hpick:{}:{idx}", lesson.id), lesson: lesson.title.clone(), text: v.term.clone(), options, to_meaning: false })
}

/// What does it mean?: `listening[n]` spoken; pick its meaning among the
/// meanings of the lesson's other sentences.
pub fn hear_meaning(lesson: &Lesson, n: usize, rng: &mut Rng) -> Option<Exercise> {
    let item = lesson.listening.get(n)?;
    if item.meaning.trim().is_empty() {
        return None;
    }
    let mut options = vec![item.meaning.clone()];
    let mut seen: HashSet<String> = options.iter().map(|o| normalize(o)).collect();
    let mut others: Vec<&crate::content::ListeningItem> = lesson.listening.iter().enumerate().filter(|(i, it)| *i != n && !it.meaning.trim().is_empty()).map(|(_, it)| it).collect();
    rng.shuffle(&mut others);
    for it in others {
        if options.len() >= 4 {
            break;
        }
        if seen.insert(normalize(&it.meaning)) {
            options.push(it.meaning.clone());
        }
    }
    if options.len() < 2 {
        return None;
    }
    rng.shuffle(&mut options);
    Some(Exercise::HearPick { id: format!("hmean:{}:{n}", lesson.id), lesson: lesson.title.clone(), text: item.text.clone(), options, to_meaning: true })
}

/// Select the matching audio: the meaning of `vocabulary[idx]` is shown; the
/// term and two look-alikes are spoken, pick the one that says it.
pub fn hear_which(lesson: &Lesson, idx: usize, rng: &mut Rng) -> Option<Exercise> {
    let v = lesson.vocabulary.get(idx)?;
    let mut options = Vec::new();
    let mut seen = HashSet::new();
    push_unique(&mut options, &mut seen, &v.term);
    for t in lookalike_terms(lesson, idx, rng) {
        if options.len() >= 3 {
            break;
        }
        push_unique(&mut options, &mut seen, &t);
    }
    if options.len() < 2 {
        return None;
    }
    rng.shuffle(&mut options);
    Some(Exercise::HearWhich { id: format!("hwhich:{}:{idx}", lesson.id), lesson: lesson.title.clone(), prompt: v.meaning.clone(), options })
}

/// Listen and match: up to five of the lesson's terms, spoken on one side and
/// written on the other.
pub fn hear_match(lesson: &Lesson, rng: &mut Rng) -> Option<Exercise> {
    let mut entries: Vec<&VocabEntry> = lesson.vocabulary.iter().collect();
    rng.shuffle(&mut entries);
    let mut terms = Vec::new();
    let mut seen = HashSet::new();
    for v in entries {
        if terms.len() >= MATCH_PAIRS {
            break;
        }
        // Spoken, "el libro" and "libro" would be the same clue.
        if seen.insert(strip_accents(&normalize(&v.term))) {
            terms.push(v.term.clone());
        }
    }
    if terms.len() < 3 {
        return None;
    }
    Some(Exercise::HearMatch { id: format!("hmatch:{}", lesson.id), lesson: lesson.title.clone(), terms })
}

/// The listening exercises of a lesson session: one of each family in a
/// random order (a sentence by tiles or with a gap, a word by ear, a sentence
/// understood, a match), as far as the lesson has the material. Typing a
/// sentence from the audio alone was too hard for a phone session, so that
/// stays in Study mode's Listening practice. Empty for courses that are not
/// language courses.
pub fn listening_exercises(content: &ContentStore, lesson: &Lesson, rng: &mut Rng) -> Vec<Exercise> {
    if !content.manifest().is_language() {
        return Vec::new();
    }
    let mut sentences = tileable_sentences(lesson);
    rng.shuffle(&mut sentences);
    let mut any: Vec<usize> = (0..lesson.listening.len()).filter(|n| !lesson.listening[*n].meaning.trim().is_empty()).collect();
    rng.shuffle(&mut any);
    let mut words: Vec<usize> = (0..lesson.vocabulary.len()).collect();
    rng.shuffle(&mut words);
    let mut out = Vec::new();
    let mut sentences = sentences.into_iter();
    let mut words = words.into_iter();
    let build_first = rng.below(2) == 0;
    if let Some(n) = sentences.next() {
        out.extend(if build_first { hear_build(lesson, n, rng) } else { hear_gap(lesson, n, None, rng) });
    }
    if let Some(i) = words.next() {
        out.extend(if rng.below(2) == 0 { hear_pick(lesson, i, rng) } else { hear_which(lesson, i, rng) });
    }
    if let Some(n) = any.first().copied() {
        out.extend(hear_meaning(lesson, n, rng));
    }
    if rng.below(2) == 0 {
        out.extend(hear_match(lesson, rng));
    } else if let Some(i) = words.next() {
        out.extend(hear_pick(lesson, i, rng));
    }
    // The other sentence family, so a session has both when it has two.
    if let Some(n) = sentences.next() {
        out.extend(if build_first { hear_gap(lesson, n, None, rng) } else { hear_build(lesson, n, rng) });
    }
    rng.shuffle(&mut out);
    out
}

/// One listening exercise from a lesson, any family, for the review mix.
fn one_listening(content: &ContentStore, lesson: &Lesson, rng: &mut Rng) -> Option<Exercise> {
    listening_exercises(content, lesson, rng).into_iter().next()
}

/// Fresh questions first (not answered correctly recently), then the rest.
fn questions_in_order<'a>(lesson: &'a Lesson, avoid: &HashSet<String>, rng: &mut Rng) -> Vec<&'a QuizItem> {
    let asked = lesson.quiz.iter().filter(|q| !q.kind.needs_widget());
    let (mut stale, mut fresh): (Vec<&QuizItem>, Vec<&QuizItem>) = asked.partition(|q| avoid.contains(&q.question_id));
    rng.shuffle(&mut fresh);
    rng.shuffle(&mut stale);
    fresh.extend(stale);
    fresh
}

/// Session slots: Grammar (rule games), Quiz, Vocabulary, Listening, Bite.
#[derive(Clone, Copy)]
enum Slot {
    G,
    Q,
    V,
    L,
    B,
}

fn assemble(mut bites: Vec<Exercise>, mut grammar: Vec<Exercise>, mut quiz: Vec<Exercise>, mut vocab: Vec<Exercise>, mut listening: Vec<Exercise>, pattern: &[Slot]) -> Vec<Exercise> {
    // Queues pop from the front.
    bites.reverse();
    grammar.reverse();
    quiz.reverse();
    vocab.reverse();
    listening.reverse();
    let mut out = Vec::new();
    for slot in pattern {
        let next = match slot {
            Slot::B => bites.pop(),
            Slot::G => grammar.pop().or_else(|| quiz.pop()),
            Slot::Q => quiz.pop().or_else(|| grammar.pop()),
            Slot::V => vocab.pop().or_else(|| quiz.pop()),
            Slot::L => listening.pop().or_else(|| vocab.pop()),
        };
        if let Some(e) = next {
            out.push(e);
        }
    }
    out
}

/// A ~5 minute session on one lesson: a bite, then rule games, quiz
/// questions, vocabulary and (language courses) listening, interleaved.
pub fn lesson_session(content: &ContentStore, lesson: &Lesson, avoid: &HashSet<String>, rng: &mut Rng) -> Session {
    let mut bites = bites_for(lesson, &crate::content::Icons::default());
    bites.truncate(3);

    // Rule games: one or two sorts, two spots, a build (language and music courses only, #1312), a conversation line, a ladder.
    let mut sorts: Vec<usize> = (0..lesson.sprint.sorts.len()).collect();
    let mut spots: Vec<usize> = (0..lesson.sprint.spots.len()).collect();
    let builds_on = content.manifest().is_language() || content.manifest().music.is_some();
    let mut builds: Vec<usize> = (0..if builds_on { lesson.sprint.builds.len() } else { 0 }).collect();
    let mut lines = buildable_lines(lesson);
    let mut conj: Vec<&Conjugation> = lesson.conjugations.iter().collect();
    rng.shuffle(&mut sorts);
    rng.shuffle(&mut spots);
    rng.shuffle(&mut builds);
    rng.shuffle(&mut lines);
    rng.shuffle(&mut conj);
    let mut grammar = Vec::new();
    if let Some(&i) = sorts.first() {
        grammar.extend(sort_exercise(lesson, i, rng));
    }
    if let Some(&i) = spots.first() {
        grammar.extend(spot::exercise(content, lesson, i, rng));
    }
    if let Some(&i) = builds.first() {
        grammar.extend(build_exercise(lesson, i, rng));
    }
    grammar.extend(conj.first().and_then(|k| ladder_exercise(content, lesson, k, rng)));
    grammar.extend(scale::pick(content, lesson, rng));
    if let Some(&i) = spots.get(1) {
        grammar.extend(spot::exercise(content, lesson, i, rng));
    }
    if let Some(&i) = sorts.get(1) {
        grammar.extend(sort_exercise(lesson, i, rng));
    }
    if let Some(&(ci, li)) = lines.first() {
        grammar.extend(conversation_build(lesson, ci, li, rng));
    }
    if let Some(&i) = builds.get(1) {
        grammar.extend(build_exercise(lesson, i, rng));
    }

    let quiz: Vec<Exercise> = questions_in_order(lesson, avoid, rng)
        .into_iter()
        .take(LESSON_QUESTIONS)
        .map(|q| quiz_exercise(content, lesson, q, rng))
        .collect();

    let mut vocab = Vec::new();
    if !lesson.vocabulary.is_empty() {
        vocab.extend(pick_exercise(lesson, rng.below(lesson.vocabulary.len()), true, rng));
    }
    vocab.extend(match_exercise(lesson, rng));

    let mut listening = listening_exercises(content, lesson, rng);
    listening.truncate(4);

    use Slot::*;
    let exercises = assemble(bites, grammar, quiz, vocab, listening, &[B, G, L, Q, V, G, Q, L, B, G, Q, L, G, V, Q, L, B, G]);
    Session {
        mode: "lesson",
        title: lesson.title.clone(),
        subtitle: format!("{} · {}", content.unit(lesson.unit_index).title, content.group_title(&lesson.group)),
        lesson_id: Some(lesson.id.clone()),
        slug: Some(lesson.slug.clone()),
        timed_secs: None,
        course: CourseInfo::of(content),
        exercises,
    }
}

/// Words the learner has collected, as (lesson, vocab index).
pub fn learned_entries<'a>(content: &'a ContentStore, learned: &HashSet<(String, String)>) -> Vec<(&'a Lesson, usize)> {
    let mut out = Vec::new();
    for l in content.lessons() {
        for (i, v) in l.vocabulary.iter().enumerate() {
            if learned.contains(&(l.id.clone(), v.term.clone())) {
                out.push((l, i));
            }
        }
    }
    out
}

/// Quiz questions in a review mix.
const REVIEW_QUIZ_LEN: usize = 8;

/// Up to `n` quiz questions from the next lessons on the path the learner has
/// not read (course order, skipped levels left out), first lesson first.
fn taste_of_next<'a>(
    content: &'a ContentStore,
    progress: &HashMap<String, Progress>,
    start: usize,
    avoid: &HashSet<String>,
    n: usize,
    rng: &mut Rng,
) -> Vec<(&'a Lesson, &'a QuizItem)> {
    let mut out = Vec::new();
    for l in content.lessons() {
        if out.len() >= n {
            break;
        }
        if l.quiz.is_empty() || crate::quiz::is_read(progress, l) || crate::quiz::is_skipped(progress.get(&l.id), l, start) {
            continue;
        }
        let mut items: Vec<&QuizItem> = l.quiz.iter().filter(|q| !q.kind.needs_widget()).collect();
        rng.shuffle(&mut items);
        // Fresh questions first.
        items.sort_by_key(|q| avoid.contains(&q.question_id));
        out.extend(items.into_iter().take(n - out.len()).map(|q| (l, q)));
    }
    out
}

/// Mixed review: the adaptive quiz mix over read lessons (weak topics first),
/// a rule game from the weakest lesson in the mix, and vocabulary from
/// collected words. With fewer than a round of read questions, the rest are
/// a taste of the next lessons on the path, tagged as such.
pub fn review_session(
    content: &ContentStore,
    progress: &HashMap<String, Progress>,
    avoid: &HashSet<String>,
    learned: &crate::vocab::Learned,
    start: usize,
    at: i64,
    rng: &mut Rng,
) -> Session {
    let picked = build_quiz(content, progress, None, true, start, avoid, REVIEW_QUIZ_LEN, at, false, rng);
    let taste = taste_of_next(content, progress, start, avoid, REVIEW_QUIZ_LEN.saturating_sub(picked.len()), rng);
    let mut lessons_in_mix: Vec<&Lesson> = Vec::new();
    for p in &picked {
        if !lessons_in_mix.iter().any(|l| l.id == p.lesson.id) {
            lessons_in_mix.push(p.lesson);
        }
    }
    lessons_in_mix.sort_by(|a, b| {
        let ea = progress.get(&a.id).map(|p| p.effective(at)).unwrap_or(0.0);
        let eb = progress.get(&b.id).map(|p| p.effective(at)).unwrap_or(0.0);
        ea.partial_cmp(&eb).unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut quiz: Vec<Exercise> = picked.iter().map(|p| quiz_exercise(content, p.lesson, p.item, rng)).collect();
    for &(l, item) in &taste {
        let mut e = quiz_exercise(content, l, item, rng);
        if let Exercise::Choice { lesson, .. } | Exercise::Tiles { lesson, .. } | Exercise::FillBar { lesson, .. } | Exercise::ChordSpell { lesson, .. } | Exercise::StaffNote { lesson, .. } | Exercise::Keyboard { lesson, .. } = &mut e {
            *lesson = crate::t1("sprint.session.taste_tag", "lesson", &l.title);
        }
        quiz.push(e);
    }

    let mut grammar = Vec::new();
    for l in lessons_in_mix.iter().take(3) {
        let n_sorts = l.sprint.sorts.len();
        let n_spots = l.sprint.spots.len();
        let e = if n_spots > 0 && (n_sorts == 0 || rng.below(2) == 0) {
            { let i = rng.below(n_spots); spot::exercise(content, l, i, rng) }
        } else if n_sorts > 0 {
            sort_exercise(l, rng.below(n_sorts), rng)
        } else if !l.conjugations.is_empty() {
            ladder_exercise(content, l, &l.conjugations[rng.below(l.conjugations.len())], rng)
        } else if !l.sprint.scales.is_empty() {
            scale::pick(content, l, rng)
        } else {
            let lines = buildable_lines(l);
            lines.first().and_then(|&(ci, li)| conversation_build(l, ci, li, rng))
        };
        grammar.extend(e);
    }

    let mut vocab = Vec::new();
    let mut known = learned_entries(content, learned);
    crate::vocab::due_first(&mut known, learned, rng);
    if let Some(&(l, i)) = known.first() {
        vocab.extend(pick_exercise(l, i, rng.below(2) == 0, rng));
    } else if let Some(l) = lessons_in_mix.first() {
        if !l.vocabulary.is_empty() {
            vocab.extend(pick_exercise(l, rng.below(l.vocabulary.len()), true, rng));
        }
    }
    // Match pairs across everything learned, else from the first lesson in the mix.
    if known.len() >= MATCH_PAIRS {
        let mut pairs = Vec::new();
        let mut seen_meaning = HashSet::new();
        let mut seen_term = HashSet::new();
        for &(l, i) in &known {
            if pairs.len() >= MATCH_PAIRS {
                break;
            }
            let v = &l.vocabulary[i];
            if seen_meaning.insert(normalize(&v.meaning)) && seen_term.insert(normalize(&v.term)) {
                pairs.push(MatchPair { term: v.term.clone(), meaning: v.meaning.clone() });
            }
        }
        if pairs.len() >= 3 {
            vocab.push(Exercise::Match { id: "match:*".into(), lesson: crate::t("sprint.session.your_words"), pairs });
        }
    } else if let Some(l) = lessons_in_mix.first() {
        vocab.extend(match_exercise(l, rng));
    }

    // Listening: one exercise each from the two weakest lessons that have sentences.
    let listening: Vec<Exercise> = lessons_in_mix.iter().filter(|l| !l.listening.is_empty()).take(2).filter_map(|l| one_listening(content, l, rng)).collect();

    use Slot::*;
    let exercises = assemble(Vec::new(), grammar, quiz, vocab, listening, &[Q, G, L, Q, V, Q, G, Q, L, Q, V, Q, G, Q, Q]);
    Session {
        mode: "review",
        title: crate::t("sprint.session.review_title"),
        subtitle: crate::t(match (picked.is_empty(), taste.is_empty()) {
            (_, true) => "sprint.session.review_subtitle",
            (false, false) => "sprint.session.review_subtitle_taste",
            (true, false) => "sprint.session.review_subtitle_new",
        }),
        lesson_id: None,
        slug: None,
        timed_secs: None,
        course: CourseInfo::of(content),
        exercises,
    }
}

/// Lightning: one-tap questions against the clock.
pub fn lightning_session(
    content: &ContentStore,
    progress: &HashMap<String, Progress>,
    avoid: &HashSet<String>,
    learned: &HashSet<(String, String)>,
    start: usize,
    at: i64,
    rng: &mut Rng,
) -> Session {
    let picked = build_quiz(content, progress, None, false, start, avoid, 10, at, false, rng);
    let mut exercises: Vec<Exercise> = picked.iter().map(|p| quiz_exercise(content, p.lesson, p.item, rng)).collect();
    let mut known = learned_entries(content, learned);
    rng.shuffle(&mut known);
    let mut picks = 0;
    for &(l, i) in &known {
        if picks >= 6 {
            break;
        }
        if let Some(e) = pick_exercise(l, i, rng.below(2) == 0, rng) {
            exercises.push(e);
            picks += 1;
        }
    }
    if picks == 0 {
        // Nothing collected yet: pull from the lessons in the mix.
        for p in picked.iter().take(4) {
            if !p.lesson.vocabulary.is_empty() {
                exercises.extend(pick_exercise(p.lesson, rng.below(p.lesson.vocabulary.len()), true, rng));
            }
        }
    }
    rng.shuffle(&mut exercises);
    Session {
        mode: "lightning",
        title: crate::t("sprint.session.lightning_title"),
        subtitle: crate::t1("sprint.session.lightning_subtitle", "secs", LIGHTNING_SECS),
        lesson_id: None,
        slug: None,
        timed_secs: Some(LIGHTNING_SECS),
        course: CourseInfo::of(content),
        exercises,
    }
}

// ---------- grading ----------

/// What a graded exercise does to the learner's records.
#[derive(Debug, PartialEq)]
pub enum Effect {
    /// Counts as a quiz answer for the lesson (mastery + history).
    Mastery { lesson_id: String, selected: String },
    /// Collects / reviews vocabulary entries (by their term).
    Vocab { lesson_id: String, known: Vec<String>, missed: Vec<String> },
    /// The same, spread over several lessons: (lesson id, known, missed).
    VocabMany(Vec<(String, Vec<String>, Vec<String>)>),
    None,
}

#[derive(Debug)]
pub struct Graded {
    pub correct: bool,
    pub correct_answer: String,
    pub explanation: String,
    pub note: Option<String>,
    pub effect: Effect,
}

fn arr_of_strings(v: &serde_json::Value) -> Option<Vec<String>> {
    v.as_array()?.iter().map(|x| x.as_str().map(str::to_string)).collect()
}

/// Grade a Sprint mode exercise. `mistakes` is the number of wrong taps the player
/// made before reaching the submitted state (exercises with instant feedback).
pub fn check(content: &ContentStore, id: &str, answer: &serde_json::Value, mistakes: u32) -> Result<Graded, String> {
    let parts: Vec<&str> = id.split(':').collect();
    let kind = parts.first().copied().unwrap_or("");
    let lesson_of = |i: usize| -> Result<&Lesson, String> {
        parts.get(i).and_then(|id| content.lesson_by_id(id)).ok_or_else(|| "unknown lesson".to_string())
    };
    let index_of = |i: usize| -> Result<usize, String> { parts.get(i).and_then(|n| n.parse().ok()).ok_or_else(|| "bad id".to_string()) };

    match kind {
        "sort" => {
            let lesson = lesson_of(1)?;
            let spec = lesson.sprint.sorts.get(index_of(2)?).ok_or("unknown sort")?;
            let given = answer.as_array().ok_or("expected a list of {text, bucket}")?;
            let mut placed: HashMap<String, usize> = HashMap::new();
            for g in given {
                let text = g.get("text").and_then(|t| t.as_str()).ok_or("item without text")?;
                let bucket = g.get("bucket").and_then(|b| b.as_u64()).ok_or("item without bucket")? as usize;
                placed.insert(text.trim().to_string(), bucket);
            }
            let all_right = spec.items.iter().all(|it| it.placed_in(&placed) == Some(&it.bucket));
            let correct = all_right && mistakes == 0;
            let in_bucket = |bi: usize| spec.items.iter().filter(|it| it.bucket == bi).map(|it| it.key()).collect::<Vec<_>>().join(", ");
            let key = spec.buckets.iter().enumerate().map(|(bi, b)| format!("{b}: {}", in_bucket(bi))).collect::<Vec<_>>().join(" · ");
            let right = spec.items.iter().filter(|it| it.placed_in(&placed) == Some(&it.bucket)).count();
            // Stored without words; `history_answer` puts it into the reader's language.
            let selected = if mistakes > 0 { format!("{right}/{} +{mistakes}", spec.items.len()) } else { format!("{right}/{}", spec.items.len()) };
            Ok(Graded {
                correct,
                correct_answer: key,
                explanation: spec.why.clone(),
                note: if all_right && mistakes > 0 { Some(crate::tn("sprint.check.sorted_slips", mistakes)) } else { None },
                effect: Effect::Mastery { lesson_id: lesson.id.clone(), selected },
            })
        }
        "spot" => {
            let lesson = lesson_of(1)?;
            spot::grade(content, lesson, lesson.sprint.spots.get(index_of(2)?).ok_or("unknown spot")?, answer)
        }
        "build" | "cbuild" => {
            let lesson = lesson_of(1)?;
            let (text, meaning) = if kind == "build" {
                let b = lesson.sprint.builds.get(index_of(2)?).ok_or("unknown build")?;
                (b.text.clone(), b.meaning.clone())
            } else {
                let line = lesson
                    .conversations
                    .get(index_of(2)?)
                    .and_then(|c| c.lines.get(index_of(3).ok()?))
                    .ok_or("unknown line")?;
                (line.text.clone(), line.meaning.clone())
            };
            let given = answer.as_str().ok_or("expected the built sentence")?;
            if given.chars().count() > 300 {
                return Err("answer too long".into());
            }
            let correct = normalize(given) == normalize(&text);
            let note = if !correct && strip_accents(&normalize(given)) == strip_accents(&normalize(&text)) {
                Some(crate::t("sprint.check.accents"))
            } else {
                None
            };
            Ok(Graded {
                correct,
                correct_answer: text,
                explanation: crate::t1("sprint.check.means", "meaning", meaning),
                note,
                effect: Effect::Mastery { lesson_id: lesson.id.clone(), selected: given.trim().to_string() },
            })
        }
        "ladder" => {
            let lesson = lesson_of(1)?;
            let p = content.paradigm().ok_or("this course has no paradigm")?;
            let item = parts.get(2).ok_or("bad id")?;
            let set = parts.get(3).ok_or("bad id")?;
            let k = lesson.conjugations.iter().find(|k| k.item == *item && k.set == *set).ok_or("unknown item")?;
            let given = arr_of_strings(answer).ok_or("expected the forms")?;
            let n = p.slots.len();
            if given.len() != n {
                return Err(format!("expected {n} forms"));
            }
            let right = p.slots.iter().zip(&given).filter(|(s, g)| normalize(k.form(&s.key)) == normalize(g)).count();
            // Graded on the placement alone, at Check (#640): the client sends no wrong taps
            // for a ladder any more, and one sent by an older client is ignored.
            let correct = right == n;
            let key = p.slots.iter().map(|s| format!("{} {}", s.label.split(" /").next().unwrap_or(""), k.form(&s.key))).collect::<Vec<_>>().join(" · ");
            Ok(Graded {
                correct,
                correct_answer: key,
                explanation: crate::i18n::fill(&crate::t("sprint.check.ladder_explanation"), &[("item", &k.item), ("meaning", &k.meaning), ("set", &p.set_label(&k.set).to_lowercase())]),
                note: Some(crate::t2("sprint.history.forms_right", "right", right, "n", n)),
                effect: Effect::Mastery { lesson_id: lesson.id.clone(), selected: format!("{right}/{n}") },
            })
        }
        "scale" => scale::grade(content, lesson_of(1)?, index_of(2)?, answer),
        "pick" => {
            let to_term = match parts.get(1) {
                Some(&"s") => true,
                Some(&"e") => false,
                _ => return Err("bad id".into()),
            };
            let lesson = lesson_of(2)?;
            let v = lesson.vocabulary.get(index_of(3)?).ok_or("unknown word")?;
            let given = answer.as_str().ok_or("expected a string")?;
            let target = if to_term { &v.term } else { &v.meaning };
            let correct = normalize(given) == normalize(target);
            let (known, missed) = if correct { (vec![v.term.clone()], vec![]) } else { (vec![], vec![v.term.clone()]) };
            Ok(Graded {
                correct,
                correct_answer: target.clone(),
                explanation: example_with_meaning(v).unwrap_or_default(),
                note: None,
                effect: Effect::Vocab { lesson_id: lesson.id.clone(), known, missed },
            })
        }
        "hbuild" => {
            let lesson = lesson_of(1)?;
            let item = lesson.listening.get(index_of(2)?).ok_or("unknown sentence")?;
            let given = answer.as_str().ok_or("expected the built sentence")?;
            if given.chars().count() > 300 {
                return Err("answer too long".into());
            }
            let correct = tile_key(given) == tile_key(&item.text);
            let note = if !correct && strip_accents(&tile_key(given)) == strip_accents(&tile_key(&item.text)) {
                Some(crate::t("sprint.check.accents"))
            } else {
                None
            };
            Ok(Graded {
                correct,
                correct_answer: item.text.clone(),
                explanation: crate::t1("sprint.check.means", "meaning", &item.meaning),
                note,
                effect: Effect::Mastery { lesson_id: lesson.id.clone(), selected: given.trim().to_string() },
            })
        }
        // No longer dealt (typing a sentence from audio alone was too hard for
        // a phone session), still graded for sessions already open.
        "htype" => {
            let lesson = lesson_of(1)?;
            let item = lesson.listening.get(index_of(2)?).ok_or("unknown sentence")?;
            let given = answer.as_str().ok_or("expected the typed sentence")?;
            if given.chars().count() > 300 {
                return Err("answer too long".into());
            }
            let v = crate::listening::check(&item.text, given);
            let note = if v.correct {
                None
            } else if v.accents_only {
                Some(crate::t("sprint.check.accents"))
            } else {
                Some(crate::t2("sprint.check.words_right", "n", v.words_right, "total", v.expected.len()))
            };
            Ok(Graded {
                correct: v.correct,
                correct_answer: item.text.clone(),
                explanation: crate::t1("sprint.check.means", "meaning", &item.meaning),
                note,
                effect: Effect::Mastery { lesson_id: lesson.id.clone(), selected: given.trim().to_string() },
            })
        }
        "hgap" => {
            let lesson = lesson_of(1)?;
            let item = lesson.listening.get(index_of(2)?).ok_or("unknown sentence")?;
            let words = words_of(&item.text);
            let word = bare(words.get(index_of(3)?).ok_or("gap out of range")?);
            let given = answer.as_str().ok_or("expected the missing word")?;
            let correct = normalize(given) == normalize(&word);
            let note = if !correct && strip_accents(&normalize(given)) == strip_accents(&normalize(&word)) {
                Some(crate::t("sprint.check.accents"))
            } else {
                None
            };
            Ok(Graded {
                correct,
                correct_answer: word,
                explanation: crate::t1("sprint.check.means", "meaning", &item.meaning), // the player shows the sentence, with a speaker
                note,
                effect: Effect::Mastery { lesson_id: lesson.id.clone(), selected: given.trim().to_string() },
            })
        }
        "hmean" => {
            let lesson = lesson_of(1)?;
            let item = lesson.listening.get(index_of(2)?).ok_or("unknown sentence")?;
            let given = answer.as_str().ok_or("expected a meaning")?;
            let correct = normalize(given) == normalize(&item.meaning);
            Ok(Graded {
                correct,
                correct_answer: item.meaning.clone(),
                explanation: String::new(), // the player shows the sentence itself, with a speaker
                note: None,
                effect: Effect::Mastery { lesson_id: lesson.id.clone(), selected: given.trim().to_string() },
            })
        }
        "hpick" | "hwhich" => {
            let lesson = lesson_of(1)?;
            let v = lesson.vocabulary.get(index_of(2)?).ok_or("unknown word")?;
            let given = answer.as_str().ok_or("expected a term")?;
            let correct = normalize(given) == normalize(&v.term);
            let note = if !correct && strip_accents(&normalize(given)) == strip_accents(&normalize(&v.term)) {
                Some(crate::t("sprint.check.accents"))
            } else {
                None
            };
            let (known, missed) = if correct { (vec![v.term.clone()], vec![]) } else { (vec![], vec![v.term.clone()]) };
            Ok(Graded {
                correct,
                correct_answer: v.term.clone(),
                explanation: format!("{}{}", crate::t2("sprint.check.term_means", "term", &v.term, "meaning", &v.meaning), example_with_meaning(v).map(|e| format!(" · {e}")).unwrap_or_default()),
                note,
                effect: Effect::Vocab { lesson_id: lesson.id.clone(), known, missed },
            })
        }
        "match" | "hmatch" => {
            let known = answer.get("known").and_then(arr_of_strings).unwrap_or_default();
            let missed = answer.get("missed").and_then(arr_of_strings).unwrap_or_default();
            if known.len() + missed.len() > 50 {
                return Err("too many pairs".into());
            }
            let lesson_id = parts.get(1).copied().unwrap_or("*");
            // Only words that really belong to a lesson are recorded; one effect per
            // lesson is enough for the common case, so "*" boards pick each word's lesson.
            let mut effects: Vec<(String, Vec<String>, Vec<String>)> = Vec::new();
            let find = |term: &str| -> Option<&Lesson> {
                if lesson_id == "*" {
                    content.lessons().iter().find(|l| l.vocabulary.iter().any(|v| v.term == term))
                } else {
                    content.lesson_by_id(lesson_id).filter(|l| l.vocabulary.iter().any(|v| v.term == term))
                }
            };
            for (w, ok) in known.iter().map(|w| (w, true)).chain(missed.iter().map(|w| (w, false))) {
                if let Some(l) = find(w) {
                    let slot = match effects.iter_mut().find(|e| e.0 == l.id) {
                        Some(e) => e,
                        None => {
                            effects.push((l.id.clone(), vec![], vec![]));
                            effects.last_mut().expect("just pushed")
                        }
                    };
                    if ok {
                        slot.1.push(w.clone());
                    } else {
                        slot.2.push(w.clone());
                    }
                }
            }
            let correct = missed.is_empty();
            let effect = match effects.len() {
                0 => Effect::None,
                1 => {
                    let (l, k, m) = effects.remove(0);
                    Effect::Vocab { lesson_id: l, known: k, missed: m }
                }
                _ => Effect::VocabMany(effects),
            };
            Ok(Graded {
                correct,
                correct_answer: String::new(),
                explanation: if correct { crate::t("sprint.check.pairs_first_time") } else { crate::tn("sprint.check.pairs_retried", missed.len()) },
                note: None,
                effect,
            })
        }
        _ => Err("unknown exercise".into()),
    }
}

/// Human description of a Sprint mode exercise id for the History page:
/// (lesson, prompt, correct answer). `None` for ids that are not Sprint mode ids.
pub fn describe<'a>(content: &'a ContentStore, id: &str) -> Option<(&'a Lesson, String, String)> {
    let parts: Vec<&str> = id.split(':').collect();
    let idx = |i: usize| parts.get(i).and_then(|n| n.parse::<usize>().ok());
    match parts.first().copied()? {
        "sort" => {
            let l = content.lesson_by_id(parts.get(1)?)?;
            let s = l.sprint.sorts.get(idx(2)?)?;
            Some((l, crate::t1("sprint.describe.sort", "title", &s.title), String::new()))
        }
        "spot" => {
            let l = content.lesson_by_id(parts.get(1)?)?;
            let (prompt, answer) = spot::describe(content, l.sprint.spots.get(idx(2)?)?);
            Some((l, prompt, answer))
        }
        "build" => {
            let l = content.lesson_by_id(parts.get(1)?)?;
            let b = l.sprint.builds.get(idx(2)?)?;
            Some((l, crate::t1("sprint.describe.build", "meaning", &b.meaning), b.text.clone()))
        }
        "cbuild" => {
            let l = content.lesson_by_id(parts.get(1)?)?;
            let line = l.conversations.get(idx(2)?)?.lines.get(idx(3)?)?;
            Some((l, crate::t1("sprint.describe.build", "meaning", &line.meaning), line.text.clone()))
        }
        "ladder" => {
            let l = content.lesson_by_id(parts.get(1)?)?;
            let set = content.paradigm()?.set_label(parts.get(3)?).to_lowercase();
            Some((l, crate::t2("sprint.describe.ladder", "item", parts.get(2)?, "set", set), String::new()))
        }
        "scale" => content.lesson_by_id(parts.get(1)?).and_then(|l| scale::describe(content, l, idx(2)?).map(|(p, a)| (l, p, a))),
        "hbuild" | "htype" | "hmean" | "hgap" => {
            let l = content.lesson_by_id(parts.get(1)?)?;
            let item = l.listening.get(idx(2)?)?;
            Some(match parts[0] {
                "hbuild" => (l, crate::t("sprint.describe.hear_build"), item.text.clone()),
                "htype" => (l, crate::t("sprint.describe.hear_type"), item.text.clone()),
                "hmean" => (l, crate::t1("sprint.describe.hear_meaning", "text", &item.text), item.meaning.clone()),
                _ => {
                    let words = words_of(&item.text);
                    let word = bare(words.get(idx(3)?)?);
                    (l, crate::t1("sprint.describe.hear_gap", "text", &item.text), word)
                }
            })
        }
        _ => None,
    }
}

/// The "your answer" text of a History row for a Sprint exercise graded as a
/// whole (Sort it, ladders), whose stored answer is a count: `right/total`, plus
/// ` +wrong taps` for a sort with wrong taps (and old ladder rows). Older wording
/// ("sorted with 5 slip(s)", "sorted wrongly", "6/6 rungs right") reads the same.
/// Other answers pass through.
pub fn history_answer(content: &ContentStore, id: &str, selected: &str) -> String {
    let kind = id.split(':').next().unwrap_or("");
    if !matches!(kind, "sort" | "ladder" | "scale") {
        return selected.to_string();
    }
    let parse = |s: &str| -> Option<(usize, usize, usize)> {
        let (score, taps) = match s.split_once(" +") {
            Some((a, b)) => (a, b.parse().ok()?),
            None => (s, 0),
        };
        let (r, n) = score.split_once('/')?;
        Some((r.parse().ok()?, n.parse().ok()?, taps))
    };
    let legacy_slips = selected.strip_prefix("sorted with ").and_then(|r| r.strip_suffix(" slip(s)")).and_then(|m| m.parse::<usize>().ok());
    let (right, n, taps) = if let Some(c) = parse(selected).or_else(|| selected.strip_suffix(" rungs right").and_then(parse)) {
        c
    } else if let (Some(m), Some(n)) = (legacy_slips, sort_size(content, id)) {
        (n, n, m)
    } else if selected == "sorted wrongly" {
        return crate::t("sprint.history.sorted_wrongly");
    } else {
        return selected.to_string();
    };
    // Wrong taps: a sort judges each tap at once (#1148), and ladder rows stored
    // before #640 did too. Such a row was marked ✗, so it never leads with
    // "3 of 3 right": the taps come first ("1 wrong tap, then all 3 sorted correctly").
    if taps > 0 && right == n {
        let key = if kind == "sort" { "sprint.history.taps_then_sorted" } else { "sprint.history.taps_then_forms" };
        return crate::t2(key, "taps", crate::tn("sprint.history.wrong_taps", taps), "n", n);
    }
    let key = match kind { "sort" => "sprint.history.sorted", "scale" => "sprint.history.notes_right", _ => "sprint.history.forms_right" };
    let mut text = crate::t2(key, "right", right, "n", n);
    if taps > 0 {
        text.push_str(" · ");
        text.push_str(&crate::tn("sprint.history.wrong_taps", taps));
    }
    text
}

/// How many items the Sort it exercise `sort:<lesson>:<i>` has.
fn sort_size(content: &ContentStore, id: &str) -> Option<usize> {
    let parts: Vec<&str> = id.split(':').collect();
    let l = content.lesson_by_id(parts.get(1)?)?;
    Some(l.sprint.sorts.get(parts.get(2)?.parse::<usize>().ok()?)?.items.len())
}

// ---------- streaks & stats ----------

/// Days in a row with at least one answer, finished game or anything else
/// that counts towards the daily goal (`activity.rs`) in this course, in
/// the learner's local time (`tz_offset_secs` east of UTC). The streak is alive
/// if there is activity today or yesterday. Returns (streak, practised today).
pub fn day_streak(conn: &Connection, user_id: i64, course_id: &str, tz_offset_secs: i64, now: i64) -> rusqlite::Result<(i64, bool)> {
    crate::earnings::streak(conn, user_id, course_id, tz_offset_secs, now)
}

/// Start of the learner's local day, as a Unix timestamp.
pub fn local_day_start(tz_offset_secs: i64, now: i64) -> i64 {
    (now + tz_offset_secs).div_euclid(86_400) * 86_400 - tz_offset_secs
}

/// An emoji for a lesson's node on the path: the lesson's own `sprint.icon`,
/// else one derived from its position so the path still looks varied.
pub fn lesson_icon(lesson: &Lesson, position: usize) -> String {
    if !lesson.sprint.icon.is_empty() {
        return lesson.sprint.icon.clone();
    }
    crate::content::Icons::lesson(position)
}

/// Mastery as 0–3 chillies.
pub fn chilis(mastery: f64) -> u8 {
    if mastery >= crate::mastery::MASTERED_AT {
        3
    } else if mastery >= crate::mastery::WEAK_BELOW {
        2
    } else if mastery > 0.0 {
        1
    } else {
        0
    }
}

mod gap;
pub mod scale;

pub mod spot;

#[cfg(test)]
mod tests;
