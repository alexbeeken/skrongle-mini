//! Listening practice (Study mode): the browser reads a lesson's sentence aloud,
//! the learner types what they heard, and the answer comes back marked word
//! by word. This module deals the sentences, grades the typed text and
//! describes a finished round for the activity log; the speech itself is the
//! browser's (`static/speech.js`), so there is no audio to host.

use serde::Serialize;
use std::collections::{HashMap, HashSet};

use crate::content::{words_of, ContentStore, Lesson, ListeningItem};
use crate::mastery::Progress;
use crate::quiz::{current_unit, lesson_weight, normalize, strip_accents, Rng};

/// Activity-log game id.
pub const GAME: &str = "listening";
/// Board id of a mixed round (sentences from every lesson, weighted to the weak ones).
pub const BOARD_ALL: &str = "all";
/// Sentences in a round when the page does not say.
pub const DEFAULT_LEN: usize = 8;
/// The most sentences one round may hold.
pub const MAX_LEN: usize = 20;

/// Lessons with at least one listening sentence, in course order.
pub fn playable(content: &ContentStore) -> impl Iterator<Item = &Lesson> {
    content.lessons().iter().filter(|l| !l.listening.is_empty())
}

/// A sentence chosen for a round.
#[derive(Debug, Clone, Copy)]
pub struct Dealt<'a> {
    pub lesson: &'a Lesson,
    pub index: usize,
}

impl<'a> Dealt<'a> {
    pub fn item(&self) -> &'a ListeningItem {
        &self.lesson.listening[self.index]
    }
}

/// Deal `len` sentences. With `only` set they all come from that lesson (in
/// a random order, each at most once); otherwise lessons are sampled by the
/// same mastery-derived weight as the practice quiz, so weak and untested
/// lessons come up most, and no sentence repeats within the round. Lessons
/// skipped below the starting level `start` are left out.
pub fn deal<'a>(
    content: &'a ContentStore,
    progress: &HashMap<String, Progress>,
    only: Option<&'a Lesson>,
    start: usize,
    len: usize,
    at: i64,
    rng: &mut Rng,
) -> Vec<Dealt<'a>> {
    let len = len.clamp(1, MAX_LEN);
    if let Some(lesson) = only {
        let mut order: Vec<usize> = (0..lesson.listening.len()).collect();
        rng.shuffle(&mut order);
        return order.into_iter().take(len).map(|index| Dealt { lesson, index }).collect();
    }

    let current = current_unit(content, progress, start, at);
    let lessons: Vec<(&Lesson, f64)> = playable(content)
        .filter(|l| !crate::quiz::is_skipped(progress.get(&l.id), l, start))
        .map(|l| (l, lesson_weight(l, progress, current, at))).collect();
    let mut pools: HashMap<&str, Vec<usize>> = HashMap::new();
    for (l, _) in &lessons {
        let mut order: Vec<usize> = (0..l.listening.len()).collect();
        rng.shuffle(&mut order);
        pools.insert(l.id.as_str(), order);
    }
    let available: usize = pools.values().map(Vec::len).sum();
    let target = len.min(available);
    let mut picked = Vec::with_capacity(target);
    while picked.len() < target {
        let live: Vec<&(&Lesson, f64)> =
            lessons.iter().filter(|(l, _)| pools.get(l.id.as_str()).is_some_and(|p| !p.is_empty())).collect();
        if live.is_empty() {
            break;
        }
        let total_w: f64 = live.iter().map(|(_, w)| *w).sum();
        let mut r = rng.next_f64() * total_w;
        let mut chosen = live[live.len() - 1].0;
        for (l, w) in &live {
            if r < *w {
                chosen = l;
                break;
            }
            r -= *w;
        }
        if let Some(index) = pools.get_mut(chosen.id.as_str()).and_then(Vec::pop) {
            picked.push(Dealt { lesson: chosen, index });
        }
    }
    rng.shuffle(&mut picked);
    picked
}

// ---------- grading ----------

/// How one word of the sentence (or of the typed text) came out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mark {
    /// Typed exactly (case and punctuation aside).
    Ok,
    /// Right letters, wrong or missing accents.
    Accent,
    /// Another word was typed in its place (on the typed side: the word that was typed instead).
    Wrong,
    /// Not typed at all (sentence side only).
    Missing,
    /// Typed but not in the sentence (typed side only).
    Extra,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Word {
    pub text: String,
    pub mark: Mark,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Verdict {
    /// Every word typed right, nothing extra.
    pub correct: bool,
    /// Not correct, but only because of accents.
    pub accents_only: bool,
    /// The sentence, word by word, each marked Ok / Accent / Wrong / Missing.
    pub expected: Vec<Word>,
    /// What was typed, word by word, each marked Ok / Accent / Wrong / Extra.
    pub typed: Vec<Word>,
    /// Words of the sentence typed exactly right.
    pub words_right: usize,
}

/// The comparable form of a word: lower-case, surrounding punctuation gone.
fn key(word: &str) -> String {
    normalize(word)
}

/// Levenshtein distance between two short words.
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        prev = cur;
    }
    prev[b.len()]
}

/// Pair the unmatched words of a gap up as substitutions, closest spelling
/// first ("cansado" for "cansada"); whatever is left over stays missing or
/// extra.
fn close_gap(ek: &[String], tk: &[String], gap_e: &mut Vec<usize>, gap_t: &mut Vec<usize>, e_marks: &mut [Mark], t_marks: &mut [Mark]) {
    let mut pairs: Vec<(usize, usize, usize)> =
        gap_e.iter().flat_map(|&a| gap_t.iter().map(move |&b| (edit_distance(&ek[a], &tk[b]), a, b))).collect();
    pairs.sort_unstable();
    let (mut used_e, mut used_t) = (HashSet::new(), HashSet::new());
    for (_, a, b) in pairs {
        if !used_e.contains(&a) && !used_t.contains(&b) {
            used_e.insert(a);
            used_t.insert(b);
            e_marks[a] = Mark::Wrong;
            t_marks[b] = Mark::Wrong;
        }
    }
    gap_e.clear();
    gap_t.clear();
}

/// Compare what was typed with the sentence, word by word. Words are matched
/// case-insensitively, ignoring punctuation and (for the alignment) accents,
/// so "esta" lines up with "está" and is marked `Accent` rather than wrong.
/// Unmatched words that sit opposite each other count as one substitution
/// (`Wrong` on both sides); the rest are `Missing` or `Extra`.
pub fn check(expected: &str, typed: &str) -> Verdict {
    let exp: Vec<String> = words_of(expected);
    let typ: Vec<String> = words_of(typed).into_iter().filter(|w| !key(w).is_empty()).collect();
    let ek: Vec<String> = exp.iter().map(|w| strip_accents(&key(w))).collect();
    let tk: Vec<String> = typ.iter().map(|w| strip_accents(&key(w))).collect();

    // Longest common subsequence over accent-stripped keys.
    let (n, m) = (ek.len(), tk.len());
    let mut lcs = vec![vec![0u16; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if ek[i] == tk[j] { lcs[i + 1][j + 1] + 1 } else { lcs[i + 1][j].max(lcs[i][j + 1]) };
        }
    }
    let mut e_marks = vec![Mark::Missing; n];
    let mut t_marks = vec![Mark::Extra; m];
    let (mut i, mut j) = (0, 0);
    // Unmatched words between two matches are paired up as substitutions,
    // closest spelling first ("cansado" for "cansada"), the rest left as
    // missing or extra.
    let mut gap_e: Vec<usize> = Vec::new();
    let mut gap_t: Vec<usize> = Vec::new();
    while i < n && j < m {
        if ek[i] == tk[j] {
            close_gap(&ek, &tk, &mut gap_e, &mut gap_t, &mut e_marks, &mut t_marks);
            let exact = key(&exp[i]) == key(&typ[j]);
            e_marks[i] = if exact { Mark::Ok } else { Mark::Accent };
            t_marks[j] = e_marks[i];
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            gap_e.push(i);
            i += 1;
        } else {
            gap_t.push(j);
            j += 1;
        }
    }
    gap_e.extend(i..n);
    gap_t.extend(j..m);
    close_gap(&ek, &tk, &mut gap_e, &mut gap_t, &mut e_marks, &mut t_marks);

    let words_right = e_marks.iter().filter(|m| **m == Mark::Ok).count();
    let correct = words_right == n && m == n;
    let accents_only = !correct && e_marks.iter().all(|m| matches!(m, Mark::Ok | Mark::Accent)) && m == n;
    Verdict {
        correct,
        accents_only,
        expected: exp.into_iter().zip(e_marks).map(|(text, mark)| Word { text, mark }).collect(),
        typed: typ.into_iter().zip(t_marks).map(|(text, mark)| Word { text, mark }).collect(),
        words_right,
    }
}

// ---------- the finished round ----------

/// A finished round, as reported by the browser.
#[derive(Debug, Clone)]
pub struct Result {
    /// A lesson id, or `BOARD_ALL`.
    pub board: String,
    pub total: i64,
    pub correct: i64,
    pub duration_ms: i64,
}

impl Result {
    /// Reject what the game could not have produced (the sentences are
    /// checked one by one, but the round totals come from the browser).
    pub fn plausible(&self, content: &ContentStore) -> bool {
        (self.board == BOARD_ALL || content.lesson_by_id(&self.board).is_some_and(|l| !l.listening.is_empty()))
            && (1..=MAX_LEN as i64).contains(&self.total)
            && (0..=self.total).contains(&self.correct)
            && self.duration_ms >= 0
    }

    /// Human label for the activity log: the lesson, or "Mixed round".
    pub fn label(&self, content: &ContentStore) -> String {
        match content.lesson_by_id(&self.board) {
            Some(l) => l.title.clone(),
            // Stored in the activity log, so always English like every other stored label.
            None => "Mixed round".to_string(),
        }
    }
}

/// Where a logged round links back to.
pub fn href(content: &ContentStore, board: &str) -> String {
    match content.lesson_by_id(board) {
        Some(l) => crate::url(&format!("/listening/play?lesson={}", l.slug)),
        None => crate::url("/listening/play"),
    }
}

/// Lessons with listening sentences (tests).
#[cfg(test)]
pub fn boards(content: &ContentStore) -> HashSet<String> {
    playable(content).map(|l| l.id.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marks(v: &[Word]) -> Vec<Mark> {
        v.iter().map(|w| w.mark).collect()
    }

    #[test]
    fn exact_answer_is_correct_whatever_the_case_and_punctuation() {
        let v = check("¿Cómo estás, Marta?", "como estás marta");
        assert!(!v.correct, "accents matter");
        assert!(v.accents_only);
        assert_eq!(marks(&v.expected), vec![Mark::Accent, Mark::Ok, Mark::Ok]);
        let v = check("¿Cómo estás, Marta?", "CÓMO ESTÁS MARTA!!");
        assert!(v.correct);
        assert_eq!(v.words_right, 3);
        assert!(check("Hola.", "   ").expected.iter().all(|w| w.mark == Mark::Missing));
        assert!(!check("Hola.", "").correct);
    }

    #[test]
    fn substitutions_missing_and_extra_words_are_told_apart() {
        let v = check("Mi madre es médica.", "Mi padre es médica");
        assert_eq!(marks(&v.expected), vec![Mark::Ok, Mark::Wrong, Mark::Ok, Mark::Ok]);
        assert_eq!(marks(&v.typed), vec![Mark::Ok, Mark::Wrong, Mark::Ok, Mark::Ok]);
        assert_eq!(v.words_right, 3);
        assert!(!v.accents_only);

        let v = check("Los niños están en el parque.", "Los niños están en parque");
        assert_eq!(marks(&v.expected), vec![Mark::Ok, Mark::Ok, Mark::Ok, Mark::Ok, Mark::Missing, Mark::Ok]);
        assert_eq!(v.typed.len(), 5);

        let v = check("Estoy en casa.", "Estoy muy en casa");
        assert_eq!(marks(&v.expected), vec![Mark::Ok, Mark::Ok, Mark::Ok]);
        assert_eq!(marks(&v.typed), vec![Mark::Ok, Mark::Extra, Mark::Ok, Mark::Ok]);
        assert!(!v.correct && !v.accents_only);

        // Substitutions pair up by spelling: "cansado" stands in for "cansada",
        // "esta" for "estoy", and "muy" is simply missing.
        let v = check("Hoy estoy muy cansada.", "Hoy esta cansado");
        assert_eq!(marks(&v.expected), vec![Mark::Ok, Mark::Wrong, Mark::Missing, Mark::Wrong]);
        assert_eq!(marks(&v.typed), vec![Mark::Ok, Mark::Wrong, Mark::Wrong]);
        assert_eq!(edit_distance("cansada", "cansado"), 1);
        assert_eq!(edit_distance("", "abc"), 3);
    }

    #[test]
    fn accent_mistakes_are_marked_but_not_wrong() {
        let v = check("Él está aquí.", "el esta aqui");
        assert!(v.accents_only);
        assert_eq!(marks(&v.expected), vec![Mark::Accent; 3]);
        // ñ counts as a letter, not an accent.
        let v = check("Tengo un año.", "Tengo un ano");
        assert!(v.accents_only, "{v:?}");
    }

    #[test]
    fn dealing_covers_one_lesson_or_mixes_by_weight() {
        let c = crate::content::tests::store(&[
            r#"{"id":"a","slug":"a","title":"A","unit":"A1","content":"x","listening":[{"text":"Uno dos."},{"text":"Tres cuatro."},{"text":"Cinco seis."}]}"#,
            r#"{"id":"b","slug":"b","title":"B","unit":"A1","content":"x","listening":[{"text":"Siete ocho.","meaning":"7 8","note":"New word: ocho = eight."}]}"#,
            r#"{"id":"c","slug":"c","title":"C","unit":"A2","content":"x"}"#,
        ])
        .unwrap();
        assert!(c.has_listening());
        assert_eq!(playable(&c).count(), 2);
        let progress = HashMap::new();
        let mut rng = Rng::seeded(7);
        let only = c.lesson_by_id("a").unwrap();
        let d = deal(&c, &progress, Some(only), 0, 10, 0, &mut rng);
        assert_eq!(d.len(), 3, "a lesson round is every sentence once");
        let mut idx: Vec<usize> = d.iter().map(|d| d.index).collect();
        idx.sort();
        assert_eq!(idx, vec![0, 1, 2]);
        assert!(d.iter().all(|d| d.lesson.id == "a"));

        let d = deal(&c, &progress, None, 0, 20, 0, &mut rng);
        assert_eq!(d.len(), 4, "a mixed round never repeats a sentence");
        assert!(d.iter().any(|d| d.lesson.id == "b"));
        assert_eq!(deal(&c, &progress, None, 0, 2, 0, &mut rng).len(), 2);
        assert_eq!(d[0].item().text.split_whitespace().count(), 2);

        let r = Result { board: "b".into(), total: 1, correct: 1, duration_ms: 10 };
        assert!(r.plausible(&c));
        assert_eq!(r.label(&c), "B");
        assert!(!Result { board: "c".into(), total: 1, correct: 1, duration_ms: 10 }.plausible(&c));
        assert!(!Result { board: BOARD_ALL.into(), total: 2, correct: 3, duration_ms: 10 }.plausible(&c));
        assert_eq!(Result { board: BOARD_ALL.into(), total: 2, correct: 1, duration_ms: 10 }.label(&c), "Mixed round");
        assert_eq!(boards(&c).len(), 2);
    }

    #[test]
    fn listening_sentences_are_validated() {
        let long = (0..31).map(|_| "palabra").collect::<Vec<_>>().join(" ");
        let e = crate::content::tests::store(&[&format!(
            r#"{{"id":"a","slug":"a","title":"A","unit":"A1","content":"x","listening":[{{"text":"{long}"}}]}}"#
        )])
        .unwrap_err()
        .to_string();
        assert!(e.contains("listening 1") && e.contains("lessons/l0.json"), "{e}");
        let e = crate::content::tests::store(&[r#"{"id":"a","slug":"a","title":"A","unit":"A1","content":"x","listening":[{"text":"Hola."},{"text":"hola"}]}"#])
            .unwrap_err()
            .to_string();
        assert!(e.contains("appears twice"), "{e}");
        let e = crate::content::tests::store(&[r#"{"id":"a","slug":"a","title":"A","unit":"A1","content":"x","listening":[{"text":"Hola.","note":" "}]}"#])
            .unwrap_err()
            .to_string();
        assert!(e.contains("note"), "{e}");
    }
}
