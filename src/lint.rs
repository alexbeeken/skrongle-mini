//! Course lints (#445, #1199): cheap checks, no AI, for a course that loads
//! but teaches badly. The validator (`ContentStore::from_files`) keeps the
//! job of refusing broken files; these never block a save. They are listed
//! at the top of the editor's feature usage report modal
//! (`templates/course_report.html`) and counted on its button, shown as
//! an amber ! on the field in the lesson editor (`static/lesson-editor.js` reads the
//! report's rows for its lesson), and logged at the end of a generation
//! job (`log`), where the repeated-term rule is #446's dedupe report.

use std::collections::HashMap;

use crate::content::{ContentStore, Lesson, QuizItem, QuizType};

pub mod language;

/// One thing to check in one lesson.
pub struct Finding {
    pub lesson_id: String,
    pub lesson_title: String,
    /// Which check found it (`data-kind` in the page, for tests).
    pub kind: &'static str,
    /// The field of the lesson file it is about, dotted (`quiz.2.prompt`,
    /// `prerequisites`), or empty for the lesson as a whole. The lesson
    /// editor shows the finding under that field's own check line.
    pub path: String,
    /// What is wrong, in plain words, with what to do about it.
    pub message: String,
}

/// A lesson's vocabulary is taught again from this many lessons on.
const REPEATED_TERM_LESSONS: usize = 3;

/// A blank's answer of this many letters or fewer is not looked for in its
/// prompt: "de", "el" or "a" stand in most sentences.
const SHORT_ANSWER: usize = 2;

/// Every finding of the course, in course order.
pub fn lint(store: &ContentStore) -> Vec<Finding> {
    let lessons = store.lessons();
    let man = store.manifest();
    let explained_in = language::primary(&man.meanings.lang);
    let check_language = man.is_language() && !explained_in.is_empty();
    // id -> (place in the course, title).
    let position: HashMap<&str, (usize, &str)> = lessons.iter().enumerate().map(|(i, l)| (l.id.as_str(), (i, l.title.as_str()))).collect();
    // term -> the lessons that teach it, in course order.
    let mut taught: HashMap<String, Vec<usize>> = HashMap::new();
    let mut out = Vec::new();
    for (i, l) in lessons.iter().enumerate() {
        let mut add = |kind, path: String, message| out.push(Finding { lesson_id: l.id.clone(), lesson_title: l.title.clone(), kind, path, message });
        for p in &l.prerequisites {
            let Some(&(at, title)) = position.get(p.as_str()) else { continue };
            if at >= i {
                if at == i {
                    add("prerequisite-self", "prerequisites".into(), crate::t("courses.lint.prerequisite_self"));
                } else {
                    add("prerequisite-later", "prerequisites".into(), crate::t1("courses.lint.prerequisite_later", "lesson", title));
                }
            }
        }
        let n = l.quiz.len();
        if n > 0 && one_mistake_fails(n) {
            add("few-questions", String::new(), crate::tn("courses.lint.few_questions", n));
        }
        for (k, q) in l.quiz.iter().enumerate() {
            if answer_in_prompt(q) {
                add("answer-in-prompt", format!("quiz.{k}.prompt"), crate::t1("courses.lint.answer_in_prompt", "prompt", short(&q.prompt)));
            }
            // A one-word blank with one answer is the common case (and
            // `quiz::normalize` already forgives case and punctuation): only
            // an answer of several words, with its many ways of writing it, is one to check.
            if q.kind == QuizType::FillInBlank && q.accepted_answers.is_empty() && q.correct_answer.split_whitespace().count() > 1 {
                add("one-spelling", format!("quiz.{k}.accepted_answers"), crate::t1("courses.lint.one_spelling", "answer", q.correct_answer.trim()));
            }
        }
        if sprint_is_reading_only(l) {
            add("sprint-reading-only", String::new(), crate::t("courses.lint.sprint_reading_only"));
        }
        if check_language {
            if let Some(lang) = language::reads_like(&l.content, &explained_in) {
                let name = |code: &str| crate::t(&format!("courses.lint.lang.{code}"));
                add("other-language", "content".into(), crate::t2("courses.lint.other_language", "found", name(lang), "expected", name(&explained_in)));
            }
        }
        let mut seen_here = Vec::new();
        for (k, v) in l.vocabulary.iter().enumerate() {
            let term = crate::quiz::normalize(&v.term);
            if term.is_empty() || seen_here.contains(&term) {
                continue;
            }
            seen_here.push(term.clone());
            let before = taught.entry(term).or_default();
            if before.len() + 1 >= REPEATED_TERM_LESSONS {
                let names: Vec<String> = before.iter().filter_map(|&k| lessons.get(k)).map(|l| crate::t1("courses.lint.quoted", "title", &l.title)).collect();
                add("repeated-term", format!("vocabulary.{k}.term"), crate::t2("courses.lint.repeated_term", "term", v.term.trim(), "lessons", names.join(", ")));
            }
            before.push(i);
        }
    }
    out
}

/// `log` for the files of a finished generation; nothing when they don't load.
pub fn log_files(files: &[crate::content::CourseFile]) -> usize {
    ContentStore::from_files(files).map_or(0, |store| log(&store))
}

/// Log every finding of a finished course, one line per finding with its
/// lesson (#1199; the repeated-term lines are #446's dedupe report). How many
/// there were, for the job's own log.
pub fn log(store: &ContentStore) -> usize {
    let found = lint(store);
    for f in &found {
        tracing::info!(course = %store.id(), lesson = %f.lesson_id, kind = f.kind, "lint: {}", f.message);
    }
    found.len()
}

/// With `n` questions, does a run with one wrong answer fall under the pass mark?
fn one_mistake_fails(n: usize) -> bool {
    let n = n as i64;
    crate::progress::pct(n - 1, n) < crate::progress::PASS_PCT
}

/// A multiple choice or fill in the blank whose answer stands in its own
/// prompt as a whole word or phrase. A multiple choice prompt that names
/// another option too ("ser or estar?") lists the choices and is fine. A
/// blank's answer of `SHORT_ANSWER` letters or fewer is not looked for.
fn answer_in_prompt(q: &QuizItem) -> bool {
    // A loaded true-false is a two-option multiple choice of True / False.
    if !matches!(q.kind, QuizType::MultipleChoice | QuizType::FillInBlank) || q.options.as_slice() == ["True", "False"] {
        return false;
    }
    let answer = crate::quiz::normalize(&q.correct_answer);
    let (prompt, min) = if q.kind == QuizType::FillInBlank { (crate::quiz::normalize(sentence_of(&q.prompt)), SHORT_ANSWER + 1) } else { (crate::quiz::normalize(&q.prompt), 2) };
    if answer.chars().count() < min || !contains_words(&prompt, &answer) {
        return false;
    }
    !q.options.iter().map(|o| crate::quiz::normalize(o)).any(|o| o != answer && !o.is_empty() && contains_words(&prompt, &o))
}

/// The sentence a blank sits in: what follows an instruction's colon
/// ("Complete with du, de la or des: Elle mange ___ soupe"), which names the
/// choices the way a multiple choice prompt may.
fn sentence_of(prompt: &str) -> &str {
    match prompt.split_once(':') {
        Some((_, rest)) if rest.contains("___") => rest,
        _ => prompt,
    }
}

/// Does `hay` hold `needle` with no letter or digit right before or after it?
fn contains_words(hay: &str, needle: &str) -> bool {
    let word = |c: Option<char>| c.is_some_and(char::is_alphanumeric);
    hay.match_indices(needle).any(|(at, m)| !word(hay.get(..at).and_then(|s| s.chars().next_back())) && !word(hay.get(at + m.len()..).and_then(|s| s.chars().next())))
}

/// The lesson's Sprint session would be bites only: nothing to answer, sort,
/// build or listen to (`sprint::lesson_session` draws from these).
fn sprint_is_reading_only(l: &Lesson) -> bool {
    let has_bites = !l.sprint.bites.is_empty() || !l.sections.is_empty();
    has_bites
        && l.quiz.is_empty()
        && l.vocabulary.is_empty()
        && l.listening.is_empty()
        && l.conversations.is_empty()
        && l.conjugations.is_empty()
        && l.sprint.sorts.is_empty()
        && l.sprint.spots.is_empty()
        && l.sprint.builds.is_empty()
}

/// A prompt cut to a few words, for a finding's message.
fn short(s: &str) -> String {
    const MAX: usize = 60;
    let s = s.trim();
    if s.chars().count() <= MAX {
        return s.to_string();
    }
    let cut: String = s.chars().take(MAX).collect();
    format!("{}…", cut.trim_end())
}

#[cfg(test)]
mod tests;
