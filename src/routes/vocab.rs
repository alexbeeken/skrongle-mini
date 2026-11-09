//! Vocabulary: terms collected by passing quiz questions, studied as flashcards.

use askama::Template;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Redirect, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use super::{time_ago, CourseView, Page};
use crate::auth::{CurrentUser, Project};
use crate::db::now;
use crate::error::AppError;
use crate::quiz::Rng;
use crate::vocab::{self, LearnedWord};
use crate::AppState;

pub struct WordRow {
    pub term: String,
    pub meaning: String,
    pub example: Option<String>,
    pub example_meaning: Option<String>,
    pub learned: String,
    pub times_seen: i64,
    pub times_known: i64,
    pub reviewed: Option<String>,
}

pub struct LessonVocab {
    pub slug: String,
    pub title: String,
    pub unit: String,
    pub total: usize,
    pub learned: usize,
    pub words: Vec<WordRow>,
}

#[derive(Template)]
#[template(path = "vocabulary.html")]
pub struct VocabularyTemplate {
    pub user: Option<CurrentUser>,
    pub course: std::sync::Arc<CourseView>,
    pub total_learned: usize,
    pub total_words: usize,
    pub lessons: Vec<LessonVocab>,
    /// How many of `lessons` have nothing collected yet (folded away on the page).
    pub empty_lessons: usize,
}

pub struct Card {
    pub lesson_id: String,
    pub lesson_title: String,
    pub term: String,
    pub meaning: String,
    pub example: Option<String>,
    pub example_meaning: Option<String>,
}

#[derive(Template)]
#[template(path = "vocab_study.html")]
pub struct StudyTemplate {
    pub user: Option<CurrentUser>,
    pub course: std::sync::Arc<CourseView>,
    pub heading: String,
    pub subheading: String,
    pub lesson_slug: Option<String>,
    /// The lesson for "Practise the lesson"; `None` when it has no quiz (#1306).
    pub quiz_slug: Option<String>,
    pub cards: Vec<Card>,
    /// A due round (`?due=1`, #1360): a missed card comes back once at the
    /// end, and the summary says what comes back when.
    pub due: bool,
}

async fn learned(state: &AppState, user_id: i64, course_id: &str) -> Result<HashMap<(String, String), LearnedWord>, AppError> {
    let cid = course_id.to_string();
    state.db.call(move |c| vocab::load_all(c, user_id, &cid)).await
}

/// GET /vocabulary
pub async fn index(State(state): State<Arc<AppState>>, p: Project) -> Result<Response, AppError> {
    let at = now();
    let content = &p.course;
    let known = learned(&state, p.user.id, content.id()).await?;
    let mut lessons = Vec::new();
    let mut total_learned = 0;
    let mut total_words = 0;
    for lesson in content.lessons() {
        if lesson.vocabulary.is_empty() {
            continue;
        }
        let words: Vec<WordRow> = lesson
            .vocabulary
            .iter()
            .filter_map(|v| {
                let w = known.get(&(lesson.id.clone(), v.term.clone()))?;
                Some(WordRow {
                    term: v.term.clone(),
                    meaning: v.meaning.clone(),
                    example: v.example.clone(),
                    example_meaning: v.example_meaning.clone(),
                    learned: time_ago(w.learned_at, at),
                    times_seen: w.times_seen,
                    times_known: w.times_known,
                    reviewed: w.last_reviewed_at.map(|t| time_ago(t, at)),
                })
            })
            .collect();
        total_learned += words.len();
        total_words += lesson.vocabulary.len();
        lessons.push(LessonVocab {
            slug: lesson.slug.clone(),
            title: lesson.title.clone(),
            unit: content.unit(lesson.unit_index).title.clone(),
            total: lesson.vocabulary.len(),
            learned: words.len(),
            words,
        });
    }
    let empty_lessons = lessons.iter().filter(|l| l.learned == 0).count();
    Ok(Page(VocabularyTemplate { course: content.view(), user: Some(p.user), total_learned, total_words, lessons, empty_lessons })
        .into_response())
}

/// A word the lookup found, with what the learner has of it.
pub struct LookupRow {
    pub hit: vocab::Hit,
    /// Collected in one of its lessons; with the flashcard tally (known, seen).
    pub collected: bool,
    pub times_known: i64,
    pub times_seen: i64,
}

#[derive(Template)]
#[template(path = "lookup.html")]
pub struct LookupTemplate {
    pub user: Option<CurrentUser>,
    pub course: std::sync::Arc<CourseView>,
    pub q: String,
    pub rows: Vec<LookupRow>,
    pub total: usize,
}

#[derive(Deserialize)]
pub struct LookupParams {
    pub q: Option<String>,
}

/// GET /lookup?q=: a word looked up across the course (#348), reached from
/// the lessons filter. An empty query goes back to the lessons.
pub async fn lookup(State(state): State<Arc<AppState>>, p: Project, Query(params): Query<LookupParams>) -> Result<Response, AppError> {
    let q = params.q.as_deref().unwrap_or("").trim().chars().take(100).collect::<String>();
    if q.is_empty() {
        return Ok(Redirect::to(&crate::url("/lessons")).into_response());
    }
    let content = &p.course;
    let found = vocab::lookup(content, &q);
    let known = learned(&state, p.user.id, content.id()).await?;
    let rows = found
        .hits
        .into_iter()
        .map(|hit| {
            let mine: Vec<&LearnedWord> = if hit.form_of.is_empty() {
                hit.lessons.iter().filter_map(|l| known.get(&(l.id.clone(), hit.term.clone()))).collect()
            } else {
                Vec::new()
            };
            LookupRow {
                collected: !mine.is_empty(),
                times_known: mine.iter().map(|w| w.times_known).sum(),
                times_seen: mine.iter().map(|w| w.times_seen).sum(),
                hit,
            }
        })
        .collect();
    Ok(Page(LookupTemplate { course: content.view(), user: Some(p.user), q, rows, total: found.total }).into_response())
}

#[derive(Deserialize)]
pub struct StudyParams {
    pub lesson: Option<String>,
    pub due: Option<String>,
}

/// A card for a learned word, if the course still teaches it.
fn card(content: &crate::content::ContentStore, lesson_id: &str, term: &str) -> Option<Card> {
    let l = content.lessons().iter().find(|l| l.id == lesson_id)?;
    let v = l.vocabulary.iter().find(|v| v.term == term)?;
    Some(Card {
        lesson_id: l.id.clone(),
        lesson_title: l.title.clone(),
        term: v.term.clone(),
        meaning: v.meaning.clone(),
        example: v.example.clone(),
        example_meaning: v.example_meaning.clone(),
    })
}

/// The words due by the end of today, longest overdue first, as cards: at
/// most `limit`. The course home's "Words due" card reads them too.
pub async fn due_cards(state: &AppState, p: &Project, limit: usize) -> Result<(i64, Vec<Card>), AppError> {
    let (uid, tz, cid) = (p.user.id, p.user.tz, p.course_id().to_string());
    let (n, keys) = state.db.call(move |c| Ok((vocab::due_count(c, uid, &cid, tz, now())?, vocab::due(c, uid, &cid, tz, now(), limit)?))).await?;
    Ok((n, keys.iter().filter_map(|(l, w)| card(&p.course, l, w)).collect()))
}

/// GET /vocabulary/study            -> every learned word, shuffled
/// GET /vocabulary/study?lesson=slug -> that lesson's learned words, in lesson order
/// GET /vocabulary/study?due=1       -> the words due today, longest overdue first, at most 20
pub async fn study(
    State(state): State<Arc<AppState>>,
    p: Project,
    Query(params): Query<StudyParams>,
) -> Result<Response, AppError> {
    let content = &p.course;
    if params.due.is_some_and(|d| d == "1") {
        let (_, cards) = due_cards(&state, &p, vocab::DUE_CAP).await?;
        if cards.is_empty() {
            return Ok(Redirect::to(&crate::url("/vocabulary")).into_response());
        }
        let (heading, subheading) = (crate::t("vocab.study.due_heading"), crate::tn("vocab.study.subtitle_due", cards.len()));
        return Ok(Page(StudyTemplate { course: content.view(), user: Some(p.user), heading, subheading, lesson_slug: None, quiz_slug: None, cards, due: true })
            .into_response());
    }
    let known = learned(&state, p.user.id, content.id()).await?;
    let only = match params.lesson.as_deref() {
        Some(slug) => Some(content.lesson_by_slug(slug).ok_or(AppError::NotFound)?),
        None => None,
    };
    let mut cards: Vec<Card> = Vec::new();
    for l in content.lessons() {
        if only.map(|o| o.id != l.id).unwrap_or(false) {
            continue;
        }
        for v in &l.vocabulary {
            if known.contains_key(&(l.id.clone(), v.term.clone())) {
                cards.extend(card(content, &l.id, &v.term));
            }
        }
    }
    if cards.is_empty() {
        return Ok(Redirect::to(&crate::url("/vocabulary")).into_response());
    }
    let (heading, subheading, lesson_slug) = match only {
        Some(l) => (l.title.clone(), crate::t1("vocab.study.subtitle_lesson", "n", cards.len()), Some(l.slug.clone())),
        None => {
            Rng::from_entropy().shuffle(&mut cards);
            (crate::t("vocab.study.all_terms"), crate::t1("vocab.study.subtitle_all", "n", cards.len()), None)
        }
    };
    let quiz_slug = only.filter(|l| !l.quiz.is_empty()).map(|l| l.slug.clone());
    Ok(Page(StudyTemplate { course: content.view(), user: Some(p.user), heading, subheading, lesson_slug, quiz_slug, cards, due: false })
        .into_response())
}

#[derive(Deserialize)]
pub struct ReviewRequest {
    pub lesson_id: String,
    pub word: String,
    pub known: bool,
}

#[derive(Serialize)]
pub struct ReviewResponse {
    pub ok: bool,
}

/// POST /api/vocabulary/review { lesson_id, word, known }
pub async fn review(
    State(state): State<Arc<AppState>>,
    p: Project,
    Json(req): Json<ReviewRequest>,
) -> Result<Json<ReviewResponse>, AppError> {
    super::limit_learning_write(&state, p.user.id)?;
    if req.word.chars().count() > 100 {
        return Err(AppError::ApiBadRequest(crate::t("vocab.api.word_too_long")));
    }
    let uid = p.user.id;
    let tz = p.user.tz;
    let cid = p.course_id().to_string();
    let ok = state
        .db
        .call(move |c| {
            let ok = vocab::review(c, uid, &cid, &req.lesson_id, &req.word, req.known, tz)?;
            if ok {
                crate::activity::record(c, uid, &cid, crate::activity::Kind::Flashcard, 1, tz)?;
            }
            Ok(ok)
        })
        .await?;
    Ok(Json(ReviewResponse { ok }))
}

#[derive(Serialize)]
pub struct DueResponse {
    /// Words still due today.
    pub due: i64,
    /// The next day with words due, for the end of a due round: "Next: 7
    /// words tomorrow", or "Next: 7 words on {day}" with `next_at` (the
    /// start of that day) for the page to name the day in its language.
    pub next: Option<String>,
    pub next_at: Option<i64>,
}

/// GET /api/vocabulary/due: what is due now and next (#1360).
pub async fn due(State(state): State<Arc<AppState>>, p: Project) -> Result<Json<DueResponse>, AppError> {
    let (uid, tz, cid) = (p.user.id, p.user.tz, p.course_id().to_string());
    let at = now();
    let (due, next) = state.db.call(move |c| Ok((vocab::due_count(c, uid, &cid, tz, at)?, vocab::next_due(c, uid, &cid, tz, at)?))).await?;
    let tomorrow = crate::sprint::local_day_start(tz, at) + 86_400;
    let next_text = next.map(|(day, n)| crate::tn(if day == tomorrow { "vocab.study.next_tomorrow" } else { "vocab.study.next_on" }, n));
    Ok(Json(DueResponse { due, next: next_text, next_at: next.map(|(day, _)| day) }))
}
