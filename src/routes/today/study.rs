//! A Study stop (#223): the lesson's text as the lesson page renders it
//! (its vocabulary, conjugation tables and conversations as plain tables
//! and lists), then two of its reading-check questions as one plain form,
//! so it works with JavaScript off. Only questions a form can answer are
//! asked ([`plain`]); a lesson with none has a single **I've read it**.
//!
//! The answers are graded and recorded as the reading check's are
//! (`quiz::grade_and_record`, so mastery and history move the same way).
//! Both right: the lesson is seen (`progress::mark_seen`, as
//! `POST /api/lesson-check`), the stop is studied (`daily::mark_studied`)
//! and finished. A miss shows what was wrong and asks two fresh ones.

use askama::Template;
use axum::response::{IntoResponse, Redirect, Response};
use std::collections::{HashMap, HashSet};

use super::StopCtx;
use crate::activity;
use crate::auth::{CurrentUser, Project};
use crate::content::{ContentStore, Lesson, QuizItem, QuizType};
use crate::daily;
use crate::error::AppError;
use crate::quiz::Rng;
use crate::routes::quiz::QuestionView;
use crate::routes::{CourseView, Page};
use crate::AppState;

/// Questions the stop asks at most.
const ASK: usize = 2;

/// A question a plain form can ask and answer: a choice (multiple choice,
/// true or false) or a typed answer (a word, a number, a formula), with no
/// music to draw or play.
pub fn plain(item: &QuizItem) -> bool {
    matches!(item.kind, QuizType::MultipleChoice | QuizType::TrueFalse | QuizType::FillInBlank | QuizType::Numeric | QuizType::Expression)
        && item.abc.is_empty()
        && item.play.is_empty()
        && item.option_kind.is_empty()
}

/// How many questions the stop asks of this lesson: two, one, or none.
fn asks(lesson: &Lesson) -> usize {
    lesson.quiz.iter().filter(|q| plain(q)).count().min(ASK)
}

/// Picks the questions: plain ones, preferring those not in `avoid` (just
/// asked, or answered right lately).
fn pick<'a>(lesson: &'a Lesson, avoid: &HashSet<String>, rng: &mut Rng) -> Vec<&'a QuizItem> {
    let mut fresh: Vec<&QuizItem> = lesson.quiz.iter().filter(|q| plain(q) && !avoid.contains(&q.question_id)).collect();
    let mut stale: Vec<&QuizItem> = lesson.quiz.iter().filter(|q| plain(q) && avoid.contains(&q.question_id)).collect();
    rng.shuffle(&mut fresh);
    rng.shuffle(&mut stale);
    fresh.extend(stale);
    fresh.truncate(ASK);
    fresh
}

/// One word of the lesson's vocabulary.
pub struct Word {
    pub term: String,
    pub meaning: String,
    pub example: String,
}

/// One row of the lesson's conjugation table.
pub struct ConjRow {
    pub item: String,
    pub meaning: String,
    pub set: String,
    pub forms: Vec<String>,
}

/// One conversation, its lines as "Name: text".
pub struct Conversation {
    pub title: String,
    pub scenario: String,
    /// (speaker, text, meaning)
    pub lines: Vec<(String, String, String)>,
}

/// One answered question after a miss.
pub struct Verdict {
    /// Not shown: the next questions avoid it.
    pub question_id: String,
    pub prompt: String,
    pub given: String,
    pub right: bool,
    pub answer: String,
    pub note: String,
    pub explanation: String,
}

#[derive(Template)]
#[template(path = "student_study.html")]
pub struct StudyTemplate {
    pub user: Option<CurrentUser>,
    pub subject: String,
    pub course: std::sync::Arc<CourseView>,
    pub title: String,
    pub summary: String,
    /// The lesson's text, rendered as on the lesson page.
    pub html: String,
    pub words: Vec<Word>,
    pub slot_labels: Vec<String>,
    pub conjugations: Vec<ConjRow>,
    pub conversations: Vec<Conversation>,
    /// The questions to answer; empty: the **I've read it** button.
    pub questions: Vec<QuestionView>,
    /// After a miss: how each answer went.
    pub missed: Vec<Verdict>,
    pub action: String,
    pub back: String,
}

fn words(lesson: &Lesson) -> Vec<Word> {
    lesson
        .vocabulary
        .iter()
        .map(|v| Word { term: v.term.clone(), meaning: v.meaning.clone(), example: v.example.clone().unwrap_or_default() })
        .collect()
}

fn conjugations(course: &ContentStore, lesson: &Lesson) -> (Vec<String>, Vec<ConjRow>) {
    let Some(pd) = course.paradigm() else { return (Vec::new(), Vec::new()) };
    let rows = lesson
        .conjugations
        .iter()
        .map(|k| ConjRow {
            item: k.item.clone(),
            meaning: k.meaning.clone(),
            set: pd.set_label(&k.set).to_string(),
            forms: pd.slots.iter().map(|s| k.form(&s.key).to_string()).collect(),
        })
        .collect();
    (pd.slots.iter().map(|s| s.label.clone()).collect(), rows)
}

fn conversations(lesson: &Lesson) -> Vec<Conversation> {
    lesson
        .conversations
        .iter()
        .filter(|c| !c.lines.is_empty())
        .map(|c| Conversation {
            title: c.title.clone(),
            scenario: c.scenario.clone(),
            lines: c.lines.iter().map(|l| (if l.speaker == crate::content::Speaker::B { c.role_b.clone() } else { c.role_a.clone() }, l.text.clone(), l.meaning.clone())).collect(),
        })
        .collect()
}

/// The stop's page: the lesson, then its questions (fresh ones, avoiding
/// `missed`'s and the ones answered right lately) or **I've read it**.
pub async fn show(state: &AppState, p: Project, ctx: StopCtx, missed: Vec<Verdict>) -> Result<Response, AppError> {
    let lesson_id = ctx.stop.activity.lessons.first().cloned().unwrap_or_default();
    let (uid, cid, lid) = (p.user.id, p.course_id().to_string(), lesson_id.clone());
    let found = p.course.lesson_by_id(&lesson_id).is_some();
    let recent = state
        .db
        .call(move |c| {
            if found {
                activity::touch(c, uid, &cid, &lid, activity::Mode::Study)?;
            }
            crate::mastery::recently_correct(c, uid, &cid, 24 * 3600)
        })
        .await?;
    let course = &p.course;
    let lesson = course.lesson_by_id(&lesson_id);
    let questions = match lesson {
        // Read already (a finish that did not go through): the button alone.
        Some(l) if !ctx.stop.marked => {
            let mut avoid: HashSet<String> = recent.into_iter().collect();
            avoid.extend(missed.iter().map(|m| m.question_id.clone()));
            let mut rng = Rng::from_entropy();
            pick(l, &avoid, &mut rng).into_iter().map(|item| QuestionView::new(course, l, item, &mut rng)).collect()
        }
        _ => Vec::new(),
    };
    let (slot_labels, conj) = lesson.map(|l| conjugations(course, l)).unwrap_or_default();
    let tpl = StudyTemplate {
        subject: course.title().to_string(),
        course: course.view(),
        title: lesson.map(|l| l.title.clone()).unwrap_or_else(|| crate::t("student.stop.lesson_gone")),
        summary: lesson.map(|l| l.summary.clone()).unwrap_or_default(),
        html: lesson.map(|l| l.html_content().to_string()).unwrap_or_default(),
        words: lesson.map(words).unwrap_or_default(),
        slot_labels,
        conjugations: conj,
        conversations: lesson.map(conversations).unwrap_or_default(),
        questions,
        missed,
        action: crate::url(&super::stop_url(ctx.id())),
        back: crate::url(&ctx.list_url()),
        user: Some(p.user.clone()),
    };
    Ok(Page(tpl).into_response())
}

/// Marks the lesson seen as the reading check does (`seen`), the stop
/// studied, and finishes it.
async fn studied(state: &AppState, p: &Project, ctx: &StopCtx, seen: bool) -> Result<Response, AppError> {
    let (uid, cid, tz, id) = (p.user.id, p.course_id().to_string(), p.user.tz, ctx.id());
    let lesson_id = ctx.stop.activity.lessons.first().cloned().unwrap_or_default();
    let course = std::sync::Arc::clone(&p.course);
    state
        .db
        .call(move |c| {
            if let Some(l) = course.lesson_by_id(&lesson_id).filter(|_| seen) {
                crate::progress::mark_seen(c, uid, &cid, l, crate::progress::VIA_STUDY)?;
                activity::record(c, uid, &cid, activity::Kind::Reading, 1, tz)?;
            }
            daily::mark_studied(c, uid, id)
        })
        .await?;
    super::finish(state, p, ctx).await
}

/// POST: the answers (`q1`/`a1`, `q2`/`a2`: question id and answer), or
/// `read` for **I've read it** when the lesson asks no question.
pub async fn post(state: &AppState, p: Project, ctx: StopCtx, form: HashMap<String, String>) -> Result<Response, AppError> {
    let lesson_id = ctx.stop.activity.lessons.first().cloned().unwrap_or_default();
    let course = std::sync::Arc::clone(&p.course);
    let Some(lesson) = course.lesson_by_id(&lesson_id) else {
        // The lesson is gone from the course: reading what is left finishes it.
        return studied(state, &p, &ctx, false).await;
    };
    if ctx.stop.marked {
        return super::finish(state, &p, &ctx).await;
    }
    let n = asks(lesson);
    if n == 0 {
        return studied(state, &p, &ctx, true).await;
    }
    let again = || Ok(Redirect::to(&crate::url(&super::stop_url(ctx.id()))).into_response());
    let mut answers: Vec<(&QuizItem, String)> = Vec::with_capacity(n);
    for i in 1..=n {
        let (Some(qid), Some(answer)) = (form.get(&format!("q{i}")), form.get(&format!("a{i}"))) else { return again() };
        let Some(item) = lesson.quiz.iter().find(|q| q.question_id == *qid && plain(q)) else { return again() };
        if answers.iter().any(|(a, _)| a.question_id == item.question_id) || answer.chars().count() > 200 {
            return again();
        }
        answers.push((item, answer.trim().to_string()));
    }
    super::super::limit_learning_write(state, p.user.id)?;
    let mut verdicts = Vec::with_capacity(n);
    for (item, answer) in &answers {
        let r = super::super::quiz::grade_and_record(state, &p, &item.question_id, answer, activity::Kind::Answer).await?;
        verdicts.push(Verdict {
            question_id: item.question_id.clone(),
            prompt: item.prompt.clone(),
            given: answer.clone(),
            right: r.correct,
            answer: r.correct_answer,
            note: r.note.unwrap_or_default(),
            explanation: r.explanation,
        });
    }
    if verdicts.iter().all(|v| v.right) {
        return studied(state, &p, &ctx, true).await;
    }
    // A miss: what went wrong, then two fresh questions.
    show(state, p, ctx, verdicts).await
}
