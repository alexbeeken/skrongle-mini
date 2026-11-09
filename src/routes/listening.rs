//! Listening practice (Study mode): pick a lesson, hear its sentences, type them.
//! The browser speaks (`static/speech.js`); the server deals the sentences,
//! grades each answer word by word and logs the finished round.

use askama::Template;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Redirect, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::pages::{load_progress, TopicRow};
use super::{CourseView, Page};
use crate::auth::{CurrentUser, Project};
use crate::db::now;
use crate::error::AppError;
use crate::listening::{self, Verdict, Word, BOARD_ALL, DEFAULT_LEN, GAME};
use crate::quiz::Rng;
use crate::AppState;

pub struct ListeningRow {
    pub topic: TopicRow,
    pub sentences: usize,
}

pub struct ListeningUnit {
    pub unit: String,
    pub description: String,
    pub lessons: Vec<ListeningRow>,
}

#[derive(Template)]
#[template(path = "listening.html")]
pub struct ListeningTemplate {
    pub user: Option<CurrentUser>,
    pub course: std::sync::Arc<CourseView>,
    pub units: Vec<ListeningUnit>,
    pub sentences: usize,
    pub rounds_played: i64,
    pub mixed_len: usize,
}

/// GET /listening -> the lessons that have sentences, plus a mixed round.
pub async fn index(State(state): State<Arc<AppState>>, p: Project) -> Result<Response, AppError> {
    let content = &p.course;
    if !content.has_listening() {
        return Ok(Redirect::to(&crate::url("/lessons")).into_response());
    }
    let at = now();
    let progress = load_progress(&state, p.user.id, content.id()).await?;
    let passes = crate::routes::pages::load_passes(&state, p.user.id, content.id()).await?;
    let uid = p.user.id;
    let cid = content.id().to_string();
    let rounds_played = state
        .db
        .call(move |c| crate::games::rounds_played(c, uid, &cid, GAME))
        .await?;
    let mut units = Vec::new();
    for (ui, unit) in content.units().iter().enumerate() {
        let lessons: Vec<ListeningRow> = content
            .lessons_in(ui)
            .filter(|l| !l.listening.is_empty())
            .map(|l| ListeningRow { topic: TopicRow::new(content, l, progress.get(&l.id), p.start, at).with_pass(passes.get(&l.id)), sentences: l.listening.len() })
            .collect();
        if !lessons.is_empty() {
            units.push(ListeningUnit { unit: unit.title.clone(), description: unit.description.clone(), lessons });
        }
    }
    Ok(Page(ListeningTemplate {
        course: content.view(),
        user: Some(p.user),
        units,
        sentences: listening::playable(content).map(|l| l.listening.len()).sum(),
        rounds_played,
        mixed_len: DEFAULT_LEN,
    })
    .into_response())
}

/// One sentence of a round, as the page's script sees it. The text is there
/// for the browser to speak; grading still goes through the server.
#[derive(Serialize)]
pub struct DealtView {
    pub lesson: String,
    pub index: usize,
    pub text: String,
    pub title: String,
    pub slug: String,
}

#[derive(Template)]
#[template(path = "listening_play.html")]
pub struct PlayTemplate {
    pub user: Option<CurrentUser>,
    pub course: std::sync::Arc<CourseView>,
    pub heading: String,
    pub subheading: String,
    pub board: String,
    pub lesson_slug: Option<String>,
    /// The lesson for "Quiz on this lesson"; `None` when it has no quiz (#1306).
    pub quiz_slug: Option<String>,
    pub total: usize,
    pub items_json: String,
}

#[derive(Deserialize)]
pub struct PlayParams {
    pub lesson: Option<String>,
    pub n: Option<usize>,
}

/// GET /listening/play?lesson=slug  -> every sentence of that lesson, shuffled
/// GET /listening/play              -> a mixed round weighted towards weak lessons
pub async fn play(State(state): State<Arc<AppState>>, p: Project, Query(params): Query<PlayParams>) -> Result<Response, AppError> {
    let content = &p.course;
    if !content.has_listening() {
        return Ok(Redirect::to(&crate::url("/lessons")).into_response());
    }
    let only = match params.lesson.as_deref().filter(|s| !s.is_empty() && *s != BOARD_ALL) {
        Some(slug) => {
            let l = content.lesson_by_slug(slug).ok_or(AppError::NotFound)?;
            if l.listening.is_empty() {
                return Ok(Redirect::to(&crate::url("/listening")).into_response());
            }
            Some(l)
        }
        None => None,
    };
    let at = now();
    let progress = load_progress(&state, p.user.id, content.id()).await?;
    let mut rng = Rng::from_entropy();
    let len = match only {
        Some(l) => params.n.unwrap_or(l.listening.len()),
        None => params.n.unwrap_or(DEFAULT_LEN),
    };
    let dealt = listening::deal(content, &progress, only, p.start, len, at, &mut rng);
    let items: Vec<DealtView> = dealt
        .iter()
        .map(|d| DealtView {
            lesson: d.lesson.id.clone(),
            index: d.index,
            text: d.item().text.clone(),
            title: d.lesson.title.clone(),
            slug: d.lesson.slug.clone(),
        })
        .collect();
    let (heading, subheading, board) = match only {
        Some(l) => (
            l.title.clone(),
            format!("{} · {}", content.unit(l.unit_index).title, content.group_title(&l.group)),
            l.id.clone(),
        ),
        None => (
            crate::t("listening.play.mixed_title"),
            crate::t("listening.play.mixed_subtitle"),
            BOARD_ALL.to_string(),
        ),
    };
    let items_json = super::script_json(&items)?;
    Ok(Page(PlayTemplate {
        course: content.view(),
        user: Some(p.user),
        heading,
        subheading,
        board,
        lesson_slug: only.map(|l| l.slug.clone()),
        quiz_slug: only.filter(|l| !l.quiz.is_empty()).map(|l| l.slug.clone()),
        total: items.len(),
        items_json,
    })
    .into_response())
}

#[derive(Deserialize)]
pub struct CheckRequest {
    pub lesson: String,
    pub index: usize,
    pub answer: String,
}

#[derive(Serialize)]
pub struct CheckResponse {
    pub correct: bool,
    pub accents_only: bool,
    pub expected: Vec<Word>,
    pub typed: Vec<Word>,
    pub words_right: usize,
    pub text: String,
    pub meaning: String,
    pub note: Option<String>,
    pub lesson_title: String,
    pub lesson_slug: String,
}

/// POST /api/listening/check { lesson, index, answer }
/// Grades one typed sentence; nothing is recorded until the round ends.
pub async fn check(p: Project, Json(req): Json<CheckRequest>) -> Result<Json<CheckResponse>, AppError> {
    let content = &p.course;
    let lesson = content.lesson_by_id(&req.lesson).ok_or_else(|| AppError::ApiBadRequest(crate::t("listening.api.unknown_lesson")))?;
    let item = lesson.listening.get(req.index).ok_or_else(|| AppError::ApiBadRequest(crate::t("listening.api.unknown_sentence")))?;
    if req.answer.chars().count() > 500 {
        return Err(AppError::ApiBadRequest(crate::t("listening.api.answer_too_long")));
    }
    let Verdict { correct, accents_only, expected, typed, words_right } = listening::check(&item.text, &req.answer);
    Ok(Json(CheckResponse {
        correct,
        accents_only,
        expected,
        typed,
        words_right,
        text: item.text.clone(),
        meaning: item.meaning.clone(),
        note: item.note.clone(),
        lesson_title: lesson.title.clone(),
        lesson_slug: lesson.slug.clone(),
    }))
}

#[derive(Deserialize)]
pub struct FinishRequest {
    pub board: String,
    pub total: i64,
    pub correct: i64,
    pub duration_ms: i64,
}

/// POST /api/listening/finish { board, total, correct, duration_ms }
/// Records a finished round for the overview and history.
pub async fn finish(
    State(state): State<Arc<AppState>>,
    p: Project,
    headers: axum::http::HeaderMap,
    Json(req): Json<FinishRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    super::limit_learning_write(&state, p.user.id)?;
    let (since, tz) = (now(), super::sprint::tz_offset_secs(&headers));
    let content = &p.course;
    if !content.has_listening() {
        return Err(AppError::ApiBadRequest(crate::t("listening.api.no_listening")));
    }
    let result = listening::Result {
        board: req.board,
        total: req.total,
        correct: req.correct,
        duration_ms: req.duration_ms.min(6 * 3600 * 1000),
    };
    if !result.plausible(content) {
        return Err(AppError::ApiBadRequest(crate::t("listening.api.implausible")));
    }
    let label = result.label(content);
    let uid = p.user.id;
    let cid = content.id().to_string();
    let course = Arc::clone(&p.course);
    let (pass, reward) = state
        .db
        .call(move |c| {
            crate::games::record_activity(c, uid, &cid, GAME, &result.board, &label, result.duration_ms, result.total, result.correct, None)?;
            crate::activity::record(c, uid, &cid, crate::activity::Kind::Listening, result.total, tz)?;
            // A lesson round is one run of that lesson's exercises (Study mode).
            let lesson = course.lesson_by_id(&result.board);
            let pass = match lesson {
                Some(l) => {
                    crate::activity::touch(c, uid, &cid, &l.id, crate::activity::Mode::Study)?;
                    Some(crate::progress::record_run(c, uid, &cid, l, crate::progress::VIA_STUDY, result.correct, result.total)?)
                }
                None => None,
            };
            let reward = crate::motivators::reward(c, uid, &course, tz, since, lesson.map(|l| l.id.as_str()))?;
            Ok((pass, reward))
        })
        .await?;
    Ok(Json(serde_json::json!({ "ok": true, "pass": pass, "reward": crate::motivators::reward_html(reward) })))
}
