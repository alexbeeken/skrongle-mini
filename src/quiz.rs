//! Adaptive micro-quiz builder and answer checking.

use std::collections::{HashMap, HashSet};
#[cfg(feature = "dev")]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(feature = "dev")]
use std::sync::OnceLock;

use crate::content::{ContentStore, Lesson, QuizItem, QuizType};
use crate::mastery::{Progress, MASTERED_AT};

pub const DEFAULT_QUIZ_LEN: usize = 8;
pub const MIN_QUIZ_LEN: usize = 5;
pub const MAX_QUIZ_LEN: usize = 10;

/// Tiny xorshift64* PRNG. We only need "good enough" shuffling, and it saves
/// pulling the full `rand` crate into the binary.
pub struct Rng(u64);

/// `SKRONGLE_RNG_SEED` (dev builds only, `seed_draws`): every draw starts from this
/// seed and a counter instead of the OS, so a browser test replays exactly.
#[cfg(feature = "dev")]
static DRAW_SEED: OnceLock<u64> = OnceLock::new();
#[cfg(feature = "dev")]
static DRAWS: AtomicU64 = AtomicU64::new(0);

/// Make every later `Rng::from_entropy` reproducible: the n-th draw of the
/// process starts from `seed` and n. Dev servers only (`SKRONGLE_RNG_SEED`).
#[cfg(feature = "dev")]
pub fn seed_draws(seed: u64) {
    let _ = DRAW_SEED.set(seed);
}

impl Rng {
    /// A fresh generator for one request: from the OS, or from the dev seed.
    pub fn from_entropy() -> Rng {
        #[cfg(feature = "dev")]
        if let Some(seed) = DRAW_SEED.get() {
            // splitmix64 of seed + n: neighbouring draws get unrelated streams.
            let mut z = seed.wrapping_add(DRAWS.fetch_add(1, Ordering::Relaxed).wrapping_add(1).wrapping_mul(0x9E37_79B9_7F4A_7C15));
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            return Rng::seeded(z ^ (z >> 31));
        }
        let mut seed = crate::random::u64();
        if seed == 0 {
            seed = 0x9E37_79B9_7F4A_7C15;
        }
        Rng(seed)
    }

    pub fn seeded(seed: u64) -> Rng {
        Rng(if seed == 0 { 1 } else { seed })
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next_u64() % n as u64) as usize }
    }

    pub fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = self.below(i + 1);
            v.swap(i, j);
        }
    }
}

/// A lesson below the learner's starting level that they never read or
/// answered (`progress::is_skipped`).
pub fn is_skipped(p: Option<&Progress>, lesson: &Lesson, start: usize) -> bool {
    crate::progress::is_skipped(lesson, start, p.is_some_and(|p| p.is_read() || p.times_tested > 0))
}

/// Which unit the user is "working on": the index of the lowest unit from
/// `start` (the level they started at) that is not yet mastered on average
/// (or that has untested lessons).
pub fn current_unit(content: &ContentStore, progress: &HashMap<String, Progress>, start: usize, at: i64) -> usize {
    for unit in start..content.units().len() {
        let mut n = 0usize;
        let mut sum = 0.0;
        let mut untested = false;
        for lesson in content.lessons_in(unit) {
            n += 1;
            match progress.get(&lesson.id) {
                Some(p) if p.times_tested > 0 => sum += p.effective(at),
                _ => untested = true,
            }
        }
        if n == 0 {
            continue;
        }
        if untested || sum / (n as f64) < MASTERED_AT {
            return unit;
        }
    }
    content.units().len().saturating_sub(1)
}

/// Selection weight for a lesson in the recommended-practice quiz.
pub fn lesson_weight(
    lesson: &Lesson,
    progress: &HashMap<String, Progress>,
    current: usize,
    at: i64,
) -> f64 {
    let p = progress.get(&lesson.id);
    let eff = p.map(|p| p.effective(at)).unwrap_or(0.0);
    let tested = p.map(|p| p.times_tested > 0).unwrap_or(false);

    // Core idea: weak topics are asked a lot, strong ones occasionally (for retention).
    let mut w = (100.0 - eff) + 8.0;
    if !tested {
        w += 15.0; // discover new topics
    }

    // Prefer the unit the learner is working on; peek ahead a little.
    if lesson.unit_index > current {
        let gap = lesson.unit_index - current;
        w *= if gap == 1 { 0.25 } else { 0.05 };
    }

    // Prerequisites not yet reasonably known: still possible, but de-prioritised.
    let prereqs_ok = lesson.prerequisites.iter().all(|pid| {
        progress.get(pid).map(|p| p.effective(at) >= 40.0).unwrap_or(false)
    });
    if !prereqs_ok && !lesson.prerequisites.is_empty() {
        w *= 0.35;
    }
    w.max(0.5)
}

/// Whether the learner has read `lesson`: passed its reading check (or
/// practised it before the check existed).
pub fn is_read(progress: &HashMap<String, Progress>, lesson: &Lesson) -> bool {
    progress.get(&lesson.id).map(|p| p.is_read()).unwrap_or(false)
}

/// The lesson to read next: the first unread lesson with quiz questions in
/// the current unit, else the first anywhere in course order that is not
/// skipped.
pub fn next_unread<'a>(content: &'a ContentStore, progress: &HashMap<String, Progress>, start: usize, at: i64) -> Option<&'a Lesson> {
    let current = current_unit(content, progress, start, at);
    let unread = |l: &&Lesson| !l.quiz.is_empty() && !is_read(progress, l) && !is_skipped(progress.get(&l.id), l, start);
    content
        .lessons_in(current)
        .find(unread)
        .or_else(|| content.lessons().iter().find(unread))
}

/// How many questions a lesson's reading check asks (two, or fewer if the
/// lesson has fewer).
pub const CHECK_LEN: usize = 2;

pub fn check_len(lesson: &Lesson) -> usize {
    lesson.quiz.len().min(CHECK_LEN)
}

/// The questions for a lesson's reading check: random, preferring ones not
/// in `avoid` (answered correctly recently) so a retry gets a fresh pair.
pub fn check_questions<'a>(lesson: &'a Lesson, avoid: &HashSet<String>, rng: &mut Rng) -> Vec<&'a QuizItem> {
    let mut fresh: Vec<&QuizItem> = lesson.quiz.iter().filter(|q| !avoid.contains(&q.question_id)).collect();
    let mut stale: Vec<&QuizItem> = lesson.quiz.iter().filter(|q| avoid.contains(&q.question_id)).collect();
    rng.shuffle(&mut fresh);
    rng.shuffle(&mut stale);
    fresh.extend(stale);
    fresh.truncate(check_len(lesson));
    fresh
}

/// A question chosen for a quiz, with its lesson.
pub struct Picked<'a> {
    pub lesson: &'a Lesson,
    pub item: &'a QuizItem,
}

/// Build a quiz of `len` questions. With `only` set, all questions come from
/// that lesson; otherwise lessons are sampled by mastery-derived weight, and
/// with `read_only` set only lessons the learner has read are candidates
/// (recommended practice is for retaining what you have studied). Lessons
/// skipped below the starting level `start` are never sampled.
/// `avoid` holds question ids answered correctly recently; they are only used
/// if nothing else is available. Without `widgets`, questions only a page can
/// draw (`QuizType::needs_widget`) are left out: Sprint mode and the tutor.
#[allow(clippy::too_many_arguments)] // each is a distinct input of the picker; a params struct would only rename them
pub fn build_quiz<'a>(
    content: &'a ContentStore,
    progress: &HashMap<String, Progress>,
    only: Option<&'a Lesson>,
    read_only: bool,
    start: usize,
    avoid: &HashSet<String>,
    len: usize,
    at: i64,
    widgets: bool,
    rng: &mut Rng,
) -> Vec<Picked<'a>> {
    let len = len.clamp(MIN_QUIZ_LEN, MAX_QUIZ_LEN);
    let mut picked: Vec<Picked<'a>> = Vec::with_capacity(len);
    let mut used: HashSet<&str> = HashSet::new();

    // Candidate lessons with weights.
    let current = current_unit(content, progress, start, at);
    let lessons: Vec<(&Lesson, f64)> = match only {
        Some(l) => vec![(l, 1.0)],
        None => content
            .lessons()
            .iter()
            .filter(|l| l.quiz.iter().any(|q| widgets || !q.kind.needs_widget()))
            .filter(|l| (!read_only || is_read(progress, l)) && !is_skipped(progress.get(&l.id), l, start))
            .map(|l| (l, lesson_weight(l, progress, current, at)))
            .collect(),
    };
    if lessons.is_empty() {
        return picked;
    }

    // Per-lesson question pools, shuffled; fresh questions first, then the recently-correct ones.
    let mut pools: HashMap<&str, Vec<&QuizItem>> = HashMap::new();
    for (l, _) in &lessons {
        let asked = l.quiz.iter().filter(|q| widgets || !q.kind.needs_widget());
        let (mut stale, mut fresh): (Vec<&QuizItem>, Vec<&QuizItem>) = asked.partition(|q| avoid.contains(&q.question_id));
        rng.shuffle(&mut fresh);
        rng.shuffle(&mut stale);
        fresh.extend(stale);
        fresh.reverse(); // so we can pop() from the end = fresh first
        pools.insert(l.id.as_str(), fresh);
    }

    let total_available: usize = pools.values().map(|p| p.len()).sum();
    let target = len.min(total_available);

    while picked.len() < target {
        // Weighted pick among lessons that still have questions.
        let live: Vec<&(&Lesson, f64)> = lessons
            .iter()
            .filter(|(l, _)| pools.get(l.id.as_str()).map(|p| !p.is_empty()).unwrap_or(false))
            .collect();
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
        let pool = pools.get_mut(chosen.id.as_str()).expect("pool exists");
        while let Some(item) = pool.pop() {
            if used.insert(item.question_id.as_str()) {
                picked.push(Picked { lesson: chosen, item });
                break;
            }
        }
    }

    // Avoid long runs of the same lesson in mixed quizzes: a light interleave.
    if only.is_none() {
        rng.shuffle(&mut picked);
    }
    picked
}

// ---------- answer checking ----------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub correct: bool,
    /// Short extra feedback (e.g. accent warning).
    pub note: Option<String>,
}

/// Lower-case, trim, collapse whitespace, drop surrounding punctuation. The
/// curly apostrophes phones type (’ ‘) become the straight one, so "j’ai"
/// matches "j'ai".
pub fn normalize(s: &str) -> String {
    let trimmed = s
        .trim()
        .trim_matches(|c: char| matches!(c, '.' | '!' | '¡' | '?' | '¿' | ',' | ';' | ':' | '"' | '\'' | '’' | '‘' | '«' | '»'));
    let mut out = String::with_capacity(trimmed.len());
    let mut last_space = false;
    for c in trimmed.chars() {
        if c.is_whitespace() {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        } else if matches!(c, '’' | '‘') {
            out.push('\'');
            last_space = false;
        } else {
            for lc in c.to_lowercase() {
                out.push(lc);
            }
            last_space = false;
        }
    }
    out
}

/// Strip common Latin diacritics (for the "almost right, check your accents" hint).
pub fn strip_accents(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'á' | 'à' | 'ä' | 'â' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            'ç' => 'c',
            'ß' => 's',
            other => other,
        })
        .collect()
}

pub fn check_answer(item: &QuizItem, given: &str) -> Verdict {
    let g = normalize(given);
    if g.is_empty() {
        return Verdict { correct: false, note: Some(crate::t("quiz.check.no_answer")) };
    }
    match item.kind {
        QuizType::MultipleChoice | QuizType::TrueFalse => Verdict { correct: g == normalize(&item.correct_answer), note: None },
        QuizType::FillBar => crate::notation::fill_bar::grade(item, given),
        QuizType::ChordSpell => crate::notation::chord_spell::grade(item, given),
        QuizType::StaffNote => crate::notation::staff_note::grade(item, given),
        QuizType::Keyboard => crate::notation::keyboard::grade(item, given),
        QuizType::AlgebraTiles => crate::algebra_tiles::check(item, given),
        QuizType::Numeric | QuizType::Expression => {
            // Any accepted spelling counts; the note comes from the main answer.
            let check = |want: &str| match item.kind {
                QuizType::Numeric => crate::mathcheck::check_numeric(want, given, item.tolerance, &item.unit),
                _ => crate::mathcheck::check_expression(want, given),
            };
            let main = check(&item.correct_answer);
            if main.correct || item.accepted_answers.iter().any(|a| check(a).correct) {
                return Verdict { correct: true, note: None };
            }
            Verdict { correct: false, note: main.note }
        }
        QuizType::Clock if item.options.is_empty() => {
            let right = std::iter::once(&item.correct_answer).chain(&item.accepted_answers).any(|a| crate::clock::same(a, given));
            Verdict { correct: right, note: None }
        }
        QuizType::Clock => Verdict { correct: g == normalize(&item.correct_answer), note: None },
        QuizType::Measure => {
            let v = crate::measure::check(item, given);
            Verdict { correct: v.correct, note: v.note }
        }
        QuizType::FillInBlank => {
            let accepted = std::iter::once(&item.correct_answer).chain(item.accepted_answers.iter());
            for a in accepted.clone() {
                if g == normalize(a) {
                    return Verdict { correct: true, note: None };
                }
            }
            let g_plain = strip_accents(&g);
            for a in accepted {
                if g_plain == strip_accents(&normalize(a)) {
                    return Verdict {
                        correct: false,
                        note: Some(crate::t1("quiz.check.accents", "answer", a)),
                    };
                }
            }
            Verdict { correct: false, note: None }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(kind: QuizType, correct: &str, accepted: &[&str]) -> QuizItem {
        QuizItem {
            question_id: "q".into(),
            prompt: "p".into(),
            kind,
            options: vec![correct.to_string(), "other".into()],
            correct_answer: correct.into(),
            accepted_answers: accepted.iter().map(|s| s.to_string()).collect(),
            explanation: String::new(),
            hints: vec![],
            teaches: vec![],
            tolerance: None,
            unit: String::new(),
            abc: String::new(), play: String::new(), option_kind: String::new(),
            tiles: vec![],
            exact: false,
            clef: String::new(),
            any_octave: false, octave: false, octaves: None, keys: vec![],
            form: String::new(),
            step_minutes: None,
            measure: Default::default(),
        }
    }

    #[test]
    fn true_false_exact() {
        let q = item(QuizType::TrueFalse, "true", &[]);
        assert!(check_answer(&q, "true").correct);
        assert!(check_answer(&q, "True").correct);
        assert!(!check_answer(&q, "false").correct);
        assert!(!check_answer(&q, "False").correct);
        assert!(!check_answer(&q, "").correct);
        let q = item(QuizType::TrueFalse, "false", &[]);
        assert!(check_answer(&q, "False").correct);
        assert!(!check_answer(&q, "True").correct);
    }

    #[test]
    fn maths_answers_are_checked_by_value() {
        let mut q = item(QuizType::Numeric, "0.75", &[]);
        assert!(check_answer(&q, "3/4").correct);
        assert!(!check_answer(&q, "0.7").correct);
        q.unit = "kg".into();
        assert!(check_answer(&q, "0.75 kg").correct);
        let q = item(QuizType::Expression, "x^2 - 1", &[]);
        assert!(check_answer(&q, "(x+1)(x-1)").correct);
        assert!(!check_answer(&q, "x^2 + 1").correct);
        let v = check_answer(&q, "(x+1");
        assert!(!v.correct && v.note.is_some());
    }

    #[test]
    fn curly_apostrophes_become_straight() {
        assert_eq!(normalize("J’ai faim"), "j'ai faim");
        assert_eq!(normalize("‘j’habite’"), "j'habite");
    }

    #[test]
    fn normalizes_input() {
        assert_eq!(normalize("  ¡Hola,   Mundo!  "), "hola, mundo");
        assert_eq!(normalize("ESTÁ"), "está");
    }

    #[test]
    fn fill_in_blank_accepts_variants_and_flags_accents() {
        let q = item(QuizType::FillInBlank, "está", &["ella está"]);
        assert!(check_answer(&q, "Está").correct);
        assert!(check_answer(&q, "ella está").correct);
        let v = check_answer(&q, "esta");
        assert!(!v.correct);
        assert!(v.note.unwrap().contains("accents"));
        assert_eq!(check_answer(&q, "es"), Verdict { correct: false, note: None });
    }

    #[test]
    fn multiple_choice_exact() {
        let q = item(QuizType::MultipleChoice, "soy", &[]);
        assert!(check_answer(&q, "soy").correct);
        assert!(!check_answer(&q, "estoy").correct);
    }

    #[test]
    fn current_unit_and_quiz_follow_mastery() {
        let c = crate::content::tests::store(&[
            r#"{"id":"a","slug":"a","title":"A","unit":"A1","content":"x","quiz":[{"question_id":"qa","prompt":"p","type":"fill-in-blank","correct_answer":"a"}]}"#,
            r#"{"id":"b","slug":"b","title":"B","unit":"A2","content":"x","quiz":[{"question_id":"qb","prompt":"p","type":"fill-in-blank","correct_answer":"b"}]}"#,
        ])
        .unwrap();
        let mut progress = HashMap::new();
        assert_eq!(current_unit(&c, &progress, 0, 0), 0);
        let mut p = Progress::new("a");
        for i in 0..8 {
            p.record_answer(true, 100 + i);
        }
        progress.insert("a".into(), p);
        assert_eq!(current_unit(&c, &progress, 0, 200), 1);
        let picked = build_quiz(&c, &progress, None, false, 0, &HashSet::new(), 5, 200, true, &mut Rng::seeded(1));
        assert_eq!(picked.len(), 2);
    }

    #[test]
    fn recommended_practice_only_draws_from_read_lessons() {
        let c = crate::content::tests::store(&[
            r#"{"id":"a","slug":"a","title":"A","unit":"A1","content":"x","quiz":[{"question_id":"qa","prompt":"p","type":"fill-in-blank","correct_answer":"a"}]}"#,
            r#"{"id":"b","slug":"b","title":"B","unit":"A1","content":"x","quiz":[{"question_id":"qb1","prompt":"p","type":"fill-in-blank","correct_answer":"b"},{"question_id":"qb2","prompt":"p","type":"fill-in-blank","correct_answer":"b"},{"question_id":"qb3","prompt":"p","type":"fill-in-blank","correct_answer":"b"}]}"#,
            r#"{"id":"c","slug":"c","title":"C","unit":"A2","content":"x","quiz":[{"question_id":"qc","prompt":"p","type":"fill-in-blank","correct_answer":"c"}]}"#,
        ])
        .unwrap();
        let mut progress = HashMap::new();
        // Nothing read: recommended practice is empty, the next lesson to read is the first one.
        assert!(build_quiz(&c, &progress, None, true, 0, &HashSet::new(), 5, 0, true, &mut Rng::seeded(1)).is_empty());
        assert_eq!(next_unread(&c, &progress, 0, 0).unwrap().id, "a");
        // Reading B (without any answers) makes it the only candidate; A is still next to read.
        progress.insert("b".into(), Progress { read_at: Some(5), ..Progress::new("b") });
        let picked = build_quiz(&c, &progress, None, true, 0, &HashSet::new(), 5, 10, true, &mut Rng::seeded(1));
        assert_eq!(picked.len(), 3);
        assert!(picked.iter().all(|p| p.lesson.id == "b"));
        assert_eq!(next_unread(&c, &progress, 0, 10).unwrap().id, "a");
        // Practising a lesson by hand does not count as reading it...
        let mut p = Progress::new("a");
        p.record_answer(true, 10);
        progress.insert("a".into(), p);
        assert!(build_quiz(&c, &progress, None, true, 0, &HashSet::new(), 5, 10, true, &mut Rng::seeded(1))
            .iter()
            .all(|p| p.lesson.id == "b"));
        // ...and the other modes still see every lesson.
        let all = build_quiz(&c, &progress, None, false, 0, &HashSet::new(), 5, 10, true, &mut Rng::seeded(1));
        assert_eq!(all.len(), 5);
        // The check asks two questions, fresh ones first.
        let b = c.lesson_by_id("b").unwrap();
        let avoid: HashSet<String> = ["qb1".to_string(), "qb2".to_string()].into_iter().collect();
        let q = check_questions(b, &avoid, &mut Rng::seeded(3));
        assert_eq!(q.len(), 2);
        assert_eq!(q[0].question_id, "qb3");
        assert_eq!(check_len(c.lesson_by_id("a").unwrap()), 1);
    }

    #[test]
    fn lessons_below_the_starting_level_are_skipped() {
        let c = crate::content::tests::store(&[
            r#"{"id":"a","slug":"a","title":"A","unit":"A1","content":"x","quiz":[{"question_id":"qa","prompt":"p","type":"fill-in-blank","correct_answer":"a"}]}"#,
            r#"{"id":"b","slug":"b","title":"B","unit":"A1","content":"x","quiz":[{"question_id":"qb","prompt":"p","type":"fill-in-blank","correct_answer":"b"}]}"#,
            r#"{"id":"c","slug":"c","title":"C","unit":"A2","content":"x","quiz":[{"question_id":"qc","prompt":"p","type":"fill-in-blank","correct_answer":"c"}]}"#,
        ])
        .unwrap();
        let mut progress = HashMap::new();
        // Started at A2: A2 is the unit worked on, its lesson the next to read,
        // and no mix asks about A1.
        assert_eq!(current_unit(&c, &progress, 1, 0), 1);
        assert_eq!(next_unread(&c, &progress, 1, 0).unwrap().id, "c");
        let all = build_quiz(&c, &progress, None, false, 1, &HashSet::new(), 5, 0, true, &mut Rng::seeded(1));
        assert!(!all.is_empty() && all.iter().all(|p| p.lesson.id == "c"));
        assert!(is_skipped(None, c.lesson_by_id("a").unwrap(), 1));
        assert!(!is_skipped(None, c.lesson_by_id("c").unwrap(), 1));
        assert_eq!(crate::mastery::Status::of_lesson(None, 0, c.lesson_by_id("a").unwrap(), 1), crate::mastery::Status::Skipped);
        // A lesson asked for by name still works.
        let a = c.lesson_by_id("a").unwrap();
        assert_eq!(build_quiz(&c, &progress, Some(a), false, 1, &HashSet::new(), 5, 0, true, &mut Rng::seeded(1)).len(), 1);
        // Studying a skipped lesson anyway makes it the learner's: read, it joins practice.
        progress.insert("b".into(), Progress { read_at: Some(5), ..Progress::new("b") });
        assert!(!is_skipped(progress.get("b"), c.lesson_by_id("b").unwrap(), 1));
        let picked = build_quiz(&c, &progress, None, true, 1, &HashSet::new(), 5, 10, true, &mut Rng::seeded(1));
        assert!(!picked.is_empty() && picked.iter().all(|p| p.lesson.id == "b"));
        // Started at the beginning nothing is skipped.
        assert_eq!(next_unread(&c, &progress, 0, 0).unwrap().id, "a");
    }

    #[test]
    fn rng_shuffle_is_a_permutation() {
        let mut r = Rng::seeded(42);
        let mut v: Vec<u32> = (0..20).collect();
        r.shuffle(&mut v);
        let mut s = v.clone();
        s.sort();
        assert_eq!(s, (0..20).collect::<Vec<_>>());
        assert_ne!(v, s);
    }
}
