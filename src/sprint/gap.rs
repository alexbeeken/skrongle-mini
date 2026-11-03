//! The word bank of a fill-in-blank question in Sprint mode: the answer plus
//! tempting wrong tiles. A bank with fewer than three tiles that differ by
//! more than accents would give the answer away, so then there is none and
//! the learner types the answer (issue #678).

use super::*;

/// The fewest tiles, told apart by more than accents, worth showing.
const MIN_GAP_TILES: usize = 3;

/// The tiles for `item`, a fill-in-blank question, or none (typed). In order:
/// the answer, its accent-less spelling, other forms of the same paradigm
/// item, the lesson's other fill-in-blank answers, then words of a similar
/// size from the lesson's listening sentences and vocabulary terms. Only
/// fill-in-blank answers and course-language text: a true/false or
/// multiple-choice answer ("True") is no word for a gap (#410).
pub(super) fn gap_tiles(content: &ContentStore, lesson: &Lesson, item: &QuizItem, rng: &mut Rng) -> Vec<String> {
    let mut tiles = Vec::new();
    let mut seen = HashSet::new();
    push_unique(&mut tiles, &mut seen, &item.correct_answer);
    let plain = strip_accents(&item.correct_answer);
    if plain != item.correct_answer {
        push_unique(&mut tiles, &mut seen, &plain);
    }
    // If the answer is a paradigm form: the same slot of the lesson's
    // other items first (ser/estar, tener/haber…), then other forms of this item.
    if let Some(p) = content.paradigm() {
        let key = normalize(&item.correct_answer);
        for k in &lesson.conjugations {
            if let Some(slot) = p.slots.iter().find(|s| normalize(k.form(&s.key)) == key) {
                for other in lesson.conjugations.iter().filter(|o| o.item != k.item && o.set == k.set) {
                    push_unique(&mut tiles, &mut seen, other.form(&slot.key));
                }
                let mut forms: Vec<&str> = p.slots.iter().map(|s| k.form(&s.key)).collect();
                rng.shuffle(&mut forms);
                for f in forms {
                    if tiles.len() >= MAX_TILES {
                        break;
                    }
                    push_unique(&mut tiles, &mut seen, f);
                }
            }
        }
    }
    let words = item.correct_answer.split_whitespace().count();
    let short = |t: &str| t.split_whitespace().count() <= words.max(2);
    // Other short answers from this lesson, of fill-in-blank items only, but
    // not one this item also takes ("quien" where "que" is asked).
    let mut others: Vec<&QuizItem> = lesson.quiz.iter().filter(|q| q.question_id != item.question_id && q.kind == QuizType::FillInBlank).collect();
    rng.shuffle(&mut others);
    for q in others {
        if tiles.len() >= MAX_TILES {
            break;
        }
        if short(&q.correct_answer) && !crate::quiz::check_answer(item, &q.correct_answer).correct {
            push_unique(&mut tiles, &mut seen, &q.correct_answer);
        }
    }
    // Still short of a full bank: words of the lesson's sentences (for a
    // one-word answer; not a capitalised one, a sentence start or a name)
    // and its vocabulary terms, closest in length first, never one that
    // would also be graded right.
    if tiles.len() < MAX_TILES {
        let mut pad: Vec<String> = Vec::new();
        if words == 1 {
            let sentence_words = lesson.listening.iter().flat_map(|i| words_of(&i.text)).map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string());
            pad.extend(sentence_words.filter(|w| !w.chars().next().is_some_and(char::is_uppercase)));
        }
        pad.extend(lesson.vocabulary.iter().map(|v| v.term.clone()).filter(|t| short(t)));
        rng.shuffle(&mut pad);
        let len = item.correct_answer.chars().count() as i64;
        pad.sort_by_key(|w| (w.split_whitespace().count() != words, (w.chars().count() as i64 - len).abs()));
        for w in pad {
            if tiles.len() >= MAX_TILES {
                break;
            }
            if !w.is_empty() && !crate::quiz::check_answer(item, &w).correct {
                push_unique(&mut tiles, &mut seen, &w);
            }
        }
    }
    let distinct: HashSet<String> = tiles.iter().map(|t| strip_accents(&normalize(t))).collect();
    if distinct.len() < MIN_GAP_TILES {
        return Vec::new();
    }
    rng.shuffle(&mut tiles);
    tiles
}
