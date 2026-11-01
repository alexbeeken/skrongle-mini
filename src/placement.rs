//! A course's placement test (#1106): `placement.json`, one per course, for
//! a course with `levels`. Multiple-choice questions, each about one unit:
//! written in the file, or drawn at random from the unit's lessons
//! (`random_per_unit`) for a unit with none written. A unit answered all
//! right can be skipped; the first unit with a wrong answer is where the
//! test suggests starting. The start wizard offers it beside the level
//! cards and picks the suggested one (`static/placement.js`).

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::content::{ContentError, ContentStore, Manifest, QuizType};
use crate::quiz::Rng;

/// Where the test lives in a course.
pub const FILE: &str = "placement.json";

/// Most questions a unit may draw at random.
pub const MAX_DRAW: usize = 10;

/// `placement.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlacementTest {
    /// For each unit without written questions: how many of its lessons'
    /// multiple-choice questions to draw at random (0: none).
    #[serde(default)]
    pub random_per_unit: usize,
    /// Questions written for the test, each about one unit.
    #[serde(default)]
    pub questions: Vec<Question>,
}

/// A written question: pick `answer` out of `options`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Question {
    pub unit: String,
    pub prompt: String,
    pub options: Vec<String>,
    pub answer: String,
}

/// The test as `placement.json` holds it, checked against the manifest.
pub fn parse(content: &str, m: &Manifest) -> Result<PlacementTest, ContentError> {
    let t: PlacementTest = serde_json::from_str(content).map_err(|e| ContentError::Parse(FILE.into(), e))?;
    let ctx = |msg: String| ContentError::Invalid(format!("{FILE}: {msg}"));
    if t.random_per_unit > MAX_DRAW {
        return Err(ctx(format!("random_per_unit is {}; at most {MAX_DRAW}", t.random_per_unit)));
    }
    if t.random_per_unit == 0 && t.questions.is_empty() {
        return Err(ctx("no questions: write some in \"questions\" or set \"random_per_unit\"".into()));
    }
    for (i, q) in t.questions.iter().enumerate() {
        let n = i + 1;
        if m.unit_index(&q.unit).is_none() {
            return Err(ctx(format!("question {n}: there is no unit '{}' in course.json", q.unit)));
        }
        if q.prompt.trim().is_empty() {
            return Err(ctx(format!("question {n} has no prompt")));
        }
        if q.options.len() < 2 {
            return Err(ctx(format!("question {n} needs at least 2 options")));
        }
        if q.options.iter().collect::<HashSet<_>>().len() != q.options.len() {
            return Err(ctx(format!("question {n} lists an option twice")));
        }
        if !q.options.contains(&q.answer) {
            return Err(ctx(format!("question {n}: answer must be one of the options")));
        }
    }
    Ok(t)
}

/// A question as the learner sees it: no answer.
#[derive(Debug, Clone, Serialize)]
pub struct Shown {
    /// `w<n>` for a written question (its place in the file), `q:<question_id>` for a drawn one.
    pub id: String,
    pub unit: String,
    pub prompt: String,
    pub options: Vec<String>,
}

/// The units the test is about: the course's start units (`start_units`)
/// that have questions, written or drawn, in course order.
fn tested(store: &ContentStore, t: &PlacementTest) -> Vec<usize> {
    store
        .start_units()
        .into_iter()
        .filter(|&u| {
            let id = &store.unit(u).id;
            t.questions.iter().any(|q| &q.unit == id) || (t.random_per_unit > 0 && drawable(store, u).next().is_some())
        })
        .collect()
}

/// Is the test on offer: a course with levels whose test has a question
/// for at least one of them.
pub fn offered(store: &ContentStore) -> bool {
    store.placement().is_some_and(|t| !tested(store, t).is_empty())
}

/// The multiple-choice questions of unit `u`'s lessons.
fn drawable(store: &ContentStore, u: usize) -> impl Iterator<Item = &crate::content::QuizItem> {
    store.lessons_in(u).flat_map(|l| l.quiz.iter()).filter(|q| q.kind == QuizType::MultipleChoice)
}

/// One sitting of the test: every tested unit's questions in course order,
/// the drawn ones picked by `rng`, each question's options shuffled.
pub fn draw(store: &ContentStore, rng: &mut Rng) -> Vec<Shown> {
    let Some(t) = store.placement() else { return Vec::new() };
    let mut out = Vec::new();
    for u in tested(store, t) {
        let unit = store.unit(u).id.clone();
        let written: Vec<Shown> = t
            .questions
            .iter()
            .enumerate()
            .filter(|(_, q)| q.unit == unit)
            .map(|(i, q)| Shown { id: format!("w{}", i + 1), unit: unit.clone(), prompt: q.prompt.clone(), options: q.options.clone() })
            .collect();
        if written.is_empty() {
            let mut pool: Vec<_> = drawable(store, u).collect();
            rng.shuffle(&mut pool);
            out.extend(pool.into_iter().take(t.random_per_unit).map(|q| Shown {
                id: format!("q:{}", q.question_id),
                unit: unit.clone(),
                prompt: q.prompt.clone(),
                options: q.options.clone(),
            }));
        } else {
            out.extend(written);
        }
    }
    for q in &mut out {
        rng.shuffle(&mut q.options);
    }
    out
}

/// How one unit went.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct UnitResult {
    pub unit: String,
    pub title: String,
    pub correct: usize,
    pub total: usize,
    /// "all", "some" or "none" right: the report card's green, yellow, red.
    pub status: &'static str,
}

/// The test marked: each unit's tally and the unit to start at.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Outcome {
    pub units: Vec<UnitResult>,
    /// The first unit with a wrong answer; with none wrong, the first start
    /// unit after the last one tested, or the last start unit.
    pub start_unit: String,
}

/// Mark `answers` ((question id, option picked)). A question that is not
/// the course's, or not about a tested unit, is left out.
pub fn mark(store: &ContentStore, answers: &[(String, String)]) -> Option<Outcome> {
    let t = store.placement()?;
    let units = tested(store, t);
    let mut tally: Vec<(usize, usize)> = vec![(0, 0); units.len()];
    let mut seen = HashSet::new();
    for (id, picked) in answers {
        if !seen.insert(id.as_str()) {
            continue;
        }
        let found = if let Some(n) = id.strip_prefix('w').and_then(|n| n.parse::<usize>().ok()) {
            n.checked_sub(1).and_then(|i| t.questions.get(i)).map(|q| (q.unit.clone(), &q.answer == picked))
        } else if let Some(qid) = id.strip_prefix("q:") {
            store
                .question(qid)
                .filter(|(_, q)| q.kind == QuizType::MultipleChoice)
                .map(|(l, q)| (store.unit(l.unit_index).id.clone(), &q.correct_answer == picked))
        } else {
            None
        };
        let Some((unit, right)) = found else { continue };
        let Some(t) = units.iter().position(|&u| store.unit(u).id == unit).and_then(|i| tally.get_mut(i)) else { continue };
        t.1 += 1;
        if right {
            t.0 += 1;
        }
    }
    let results: Vec<UnitResult> = units
        .iter()
        .zip(&tally)
        .filter(|(_, (_, total))| *total > 0)
        .map(|(&u, &(correct, total))| UnitResult {
            unit: store.unit(u).id.clone(),
            title: store.unit(u).title.clone(),
            correct,
            total,
            status: if correct == total { "all" } else if correct == 0 { "none" } else { "some" },
        })
        .collect();
    let starts = store.start_units();
    let start = match results.iter().find(|r| r.correct < r.total) {
        Some(r) => r.unit.clone(),
        None => {
            let last = results.last().and_then(|r| store.manifest().unit_index(&r.unit));
            let after = last.and_then(|l| starts.iter().find(|&&u| u > l)).or(starts.last()).or(starts.first());
            after.map(|&u| store.unit(u).id.clone()).unwrap_or_default()
        }
    };
    Some(Outcome { units: results, start_unit: start })
}

#[cfg(test)]
mod tests;
