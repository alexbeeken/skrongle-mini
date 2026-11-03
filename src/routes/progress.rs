//! Lesson passes from Study mode, and the progress API for AI agents.
//!
//! - `POST /lessons/{slug}/read`: "I've read it" on a lesson with no quiz
//!   questions (the others are read by passing their reading check,
//!   `POST /api/lesson-check`).
//! - `POST /api/practice/finish`: a finished practice round, as one run per
//!   lesson it touched.
//! - `GET /api/progress`, `GET /api/progress/lessons/{lesson}`,
//!   `POST /api/progress/seen|answers|pass`: for agents teaching the learner
//!   outside the app. They sign in like the browser (the session cookie) and
//!   work in the learner's active course. The rules they must follow are
//!   `progress::RULES`, also at `/docs/progress`.

use axum::extract::{Path, State};
use axum::response::{IntoResponse, Redirect, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::auth::Project;
use crate::content::{ContentStore, Lesson};
use crate::db::now;
use crate::error::AppError;
use crate::mastery::{self, Status};
use crate::progress::{self, LessonPass, Outcome};
use crate::AppState;

/// A lesson by id or slug.
fn find<'a>(content: &'a ContentStore, key: &str) -> Result<&'a Lesson, AppError> {
    content
        .lesson_by_id(key)
        .or_else(|| content.lesson_by_slug(key))
        .ok_or_else(|| AppError::ApiBadRequest(format!("unknown lesson {key:?}: use an id or slug from GET /api/progress")))
}

// ---------- Study mode ----------

/// POST /lessons/{slug}/read — a lesson without questions was read.
pub async fn read(State(state): State<Arc<AppState>>, p: Project, Path(slug): Path<String>) -> Result<Response, AppError> {
    let lesson = p.course.lesson_by_slug(&slug).ok_or(AppError::NotFound)?;
    let (uid, cid, course) = (p.user.id, p.course_id().to_string(), Arc::clone(&p.course));
    let tz = p.user.tz;
    let id = lesson.id.clone();
    let since = now();
    state
        .db
        .call(move |c| {
            let l = course.lesson_by_id(&id).expect("lesson exists");
            crate::activity::record(c, uid, &cid, crate::activity::Kind::Reading, 1, tz)?;
            progress::mark_seen(c, uid, &cid, l, progress::VIA_STUDY)
        })
        .await?;
    // `read` lets the lesson page show the reward line for this reading.
    Ok(Redirect::to(&crate::url(&format!("/lessons/{slug}?read={since}"))).into_response())
}

#[derive(Deserialize)]
pub struct RunReport {
    /// Lesson id or slug.
    pub lesson: String,
    pub answered: i64,
    pub correct: i64,
}

#[derive(Deserialize)]
pub struct PracticeFinish {
    pub runs: Vec<RunReport>,
    /// How long the round took: its answers were logged as they came, so the
    /// reward line counts what was logged since it began.
    #[serde(default)]
    pub duration_ms: i64,
}

#[derive(Serialize)]
pub struct PracticeFinished {
    /// One outcome per run, in the order sent.
    pub runs: Vec<Outcome>,
    /// The worn motivator's line for the round, as HTML (#568).
    pub reward: Option<String>,
}

fn plausible(answered: i64, correct: i64) -> Result<(), AppError> {
    if !(0..=200).contains(&answered) || correct < 0 || correct > answered {
        return Err(AppError::ApiBadRequest("implausible result: need 0 <= correct <= answered <= 200".into()));
    }
    Ok(())
}

/// POST /api/practice/finish { runs: [{ lesson, answered, correct }], duration_ms }
/// -> { runs: [outcome], reward }
pub async fn practice_finish(
    State(state): State<Arc<AppState>>,
    p: Project,
    headers: axum::http::HeaderMap,
    Json(req): Json<PracticeFinish>,
) -> Result<Json<PracticeFinished>, AppError> {
    super::limit_learning_write(&state, p.user.id)?;
    if req.runs.len() > 20 {
        return Err(AppError::ApiBadRequest("too many lessons in one round".into()));
    }
    let mut runs = Vec::new();
    for r in &req.runs {
        plausible(r.answered, r.correct)?;
        runs.push((find(&p.course, &r.lesson)?.id.clone(), r.answered, r.correct));
    }
    let (uid, cid, course) = (p.user.id, p.course_id().to_string(), Arc::clone(&p.course));
    let tz = super::sprint::tz_offset_secs(&headers);
    let since = now() - req.duration_ms.clamp(0, 6 * 3600 * 1000) / 1000 - 1;
    let (out, reward) = state
        .db
        .call(move |c| {
            let out = runs
                .iter()
                .map(|(id, answered, correct)| {
                    let l = course.lesson_by_id(id).expect("lesson exists");
                    progress::record_run(c, uid, &cid, l, progress::VIA_STUDY, *correct, *answered)
                })
                .collect::<rusqlite::Result<Vec<_>>>()?;
            // A round on one lesson is a run on it; a mixed round is on none.
            let lesson = match runs.as_slice() {
                [(id, _, _)] => Some(id.as_str()),
                _ => None,
            };
            let reward = crate::motivators::reward(c, uid, &course, tz, since, lesson)?;
            Ok((out, reward))
        })
        .await?;
    Ok(Json(PracticeFinished { runs: out, reward: crate::motivators::reward_html(reward) }))
}

// ---------- the agent API ----------

#[derive(Serialize)]
pub struct Rules {
    pub text: &'static str,
    pub pass_pct: i64,
    pub min_answers: i64,
    pub docs: String,
}

fn rules() -> Rules {
    Rules { text: progress::RULES, pass_pct: progress::PASS_PCT, min_answers: progress::MIN_ANSWERS, docs: crate::url("/docs/progress") }
}

#[derive(Serialize)]
pub struct LessonStatus {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub unit: String,
    pub focus: String,
    pub summary: String,
    pub quiz_questions: usize,
    /// Answers a run needs to count for this lesson.
    pub required_answers: i64,
    pub seen: bool,
    pub passed: bool,
    pub status: String,
    pub passed_via: Option<String>,
    pub best_pct: Option<i64>,
    /// Effective mastery, 0-100.
    pub mastery: u32,
    pub mastery_status: String,
    /// Below the level the learner started the course at and never studied:
    /// do not teach it unless they ask (`progress::is_skipped`).
    pub skipped: bool,
}

fn status_of(content: &ContentStore, l: &Lesson, pass: Option<&LessonPass>, m: Option<&mastery::Progress>, start: usize, at: i64) -> LessonStatus {
    let mastery_status = Status::of_lesson(m, at, l, start);
    LessonStatus {
        id: l.id.clone(),
        slug: l.slug.clone(),
        title: l.title.clone(),
        unit: content.unit(l.unit_index).title.clone(),
        focus: l.focus.clone(),
        summary: l.summary.clone(),
        quiz_questions: l.quiz.len(),
        required_answers: progress::required_answers(l),
        seen: pass.is_some_and(LessonPass::seen),
        passed: pass.is_some_and(LessonPass::passed),
        status: pass.map(LessonPass::summary).unwrap_or_else(|| crate::t("study.pass.not_started")),
        passed_via: pass.and_then(|p| p.passed_via.as_deref()).map(|v| progress::via_id(v).to_string()),
        best_pct: pass.map(|p| p.best_pct).filter(|&b| b >= 0),
        mastery: m.map(|m| m.effective(at).round() as u32).unwrap_or(0),
        mastery_status: mastery_status.label(),
        skipped: mastery_status == Status::Skipped,
    }
}

#[derive(Serialize)]
pub struct Overview {
    pub course: CourseInfo,
    pub rules: Rules,
    pub passed: usize,
    pub total: usize,
    /// The level the learner started at (a unit title), '' for the beginning.
    pub start_unit: String,
    pub lessons: Vec<LessonStatus>,
}

#[derive(Serialize)]
pub struct CourseInfo {
    pub id: String,
    pub title: String,
    pub teaches: String,
    pub in_language: String,
}

/// GET /api/progress — the rules, and every lesson of the active course
/// with its pass status and mastery, in course order.
pub async fn overview(State(state): State<Arc<AppState>>, p: Project) -> Result<Json<Overview>, AppError> {
    let content = &p.course;
    let (uid, cid) = (p.user.id, content.id().to_string());
    let (passes, mastery) =
        state.db.call(move |c| Ok((progress::load_all(c, uid, &cid)?, mastery::load_progress(c, uid, &cid)?))).await?;
    let at = now();
    let lessons: Vec<LessonStatus> =
        content.lessons().iter().map(|l| status_of(content, l, passes.get(&l.id), mastery.get(&l.id), p.start, at)).collect();
    let m = content.manifest();
    Ok(Json(Overview {
        course: CourseInfo {
            id: content.id().to_string(),
            title: m.title.clone(),
            teaches: format!("{} ({})", m.terms.label, m.terms.lang),
            in_language: format!("{} ({})", m.meanings.label, m.meanings.lang),
        },
        rules: rules(),
        passed: lessons.iter().filter(|l| l.passed).count(),
        total: lessons.len(),
        start_unit: if p.start == 0 { String::new() } else { content.unit(p.start).title.clone() },
        lessons,
    }))
}

#[derive(Serialize)]
pub struct LessonDetail<'a> {
    pub status: LessonStatus,
    pub rules: Rules,
    /// The lesson file: text (Markdown), vocabulary, quiz with answers, …
    pub lesson: &'a Lesson,
}

/// GET /api/progress/lessons/{lesson} — one lesson's material and status,
/// for an agent about to teach it.
pub async fn lesson(State(state): State<Arc<AppState>>, p: Project, Path(key): Path<String>) -> Result<Response, AppError> {
    let content = &p.course;
    let l = find(content, &key)?;
    let (uid, cid, id) = (p.user.id, content.id().to_string(), l.id.clone());
    let (pass, m) = state
        .db
        .call(move |c| Ok((progress::load_one(c, uid, &cid, &id)?, mastery::load_one(c, uid, &cid, &id)?)))
        .await?;
    let detail = LessonDetail { status: status_of(content, l, pass.as_ref(), m.as_ref(), p.start, now()), rules: rules(), lesson: l };
    Ok(Json(detail).into_response())
}

#[derive(Deserialize)]
pub struct SeenReport {
    pub lesson: String,
}

/// POST /api/progress/seen { lesson } — the agent taught the lesson.
pub async fn seen(State(state): State<Arc<AppState>>, p: Project, Json(req): Json<SeenReport>) -> Result<Json<Outcome>, AppError> {
    super::limit_learning_write(&state, p.user.id)?;
    let id = find(&p.course, &req.lesson)?.id.clone();
    let (uid, cid, course) = (p.user.id, p.course_id().to_string(), Arc::clone(&p.course));
    let out = state
        .db
        .call(move |c| {
            crate::activity::touch(c, uid, &cid, &id, crate::activity::Mode::Agent)?;
            progress::mark_seen(c, uid, &cid, course.lesson_by_id(&id).expect("lesson exists"), "agent")
        })
        .await?;
    Ok(Json(out))
}

#[derive(Deserialize)]
pub struct AnswerReport {
    /// A quiz question of the course: the app grades `answer` itself.
    pub question_id: Option<String>,
    /// Otherwise an exercise the agent set and graded: which lesson it
    /// tests, what was asked, and whether the learner got it right.
    pub lesson: Option<String>,
    pub prompt: Option<String>,
    pub correct: Option<bool>,
    #[serde(default)]
    pub answer: String,
}

#[derive(Serialize)]
pub struct AnswerResult {
    pub lesson_id: String,
    pub correct: bool,
    /// Effective mastery before and after, 0-100.
    pub mastery_before: u32,
    pub mastery: u32,
    /// For a course question: the app's grading.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graded: Option<super::quiz::AnswerResponse>,
}

/// POST /api/progress/answers — one answer, either a course question for
/// the app to grade ({ question_id, answer }) or an exercise the agent
/// graded ({ lesson, prompt, answer, correct }). Moves mastery and history
/// like an answer in any mode.
pub async fn answers(State(state): State<Arc<AppState>>, p: Project, Json(req): Json<AnswerReport>) -> Result<Json<AnswerResult>, AppError> {
    super::limit_learning_write(&state, p.user.id)?;
    if let Some(qid) = req.question_id.as_deref() {
        let r = super::quiz::grade_and_record(&state, &p, qid, &req.answer, crate::activity::Kind::Answer).await?;
        return Ok(Json(AnswerResult {
            lesson_id: r.lesson_id.clone(),
            correct: r.correct,
            mastery_before: r.mastery_before,
            mastery: r.mastery,
            graded: Some(r),
        }));
    }
    let (Some(key), Some(prompt), Some(correct)) = (req.lesson.as_deref(), req.prompt.as_deref(), req.correct) else {
        return Err(AppError::ApiBadRequest(
            "send { question_id, answer } for a course question, or { lesson, prompt, answer, correct } for your own exercise".into(),
        ));
    };
    let prompt = prompt.trim();
    if prompt.is_empty() || prompt.chars().count() > 300 || req.answer.chars().count() > 200 {
        return Err(AppError::ApiBadRequest("prompt must be 1-300 characters and answer at most 200".into()));
    }
    let id = find(&p.course, key)?.id.clone();
    let (uid, cid) = (p.user.id, p.course_id().to_string());
    let tz = p.user.tz;
    let qid = format!("{}{prompt}", AGENT_PREFIX);
    let answer = req.answer.trim().to_string();
    let at = now();
    let lesson_id = id.clone();
    let (before, after) = state
        .db
        .call(move |c| {
            if agent_answers_today(c, uid, at)? >= AGENT_ANSWERS_PER_DAY {
                return Ok(None);
            }
            let before = mastery::load_one(c, uid, &cid, &id)?.map(|m| m.effective(at)).unwrap_or(0.0);
            let after = mastery::record_answer(c, uid, &cid, &id, &qid, &answer, correct)?;
            crate::activity::record(c, uid, &cid, crate::activity::Kind::Answer, 1, tz)?;
            Ok(Some((before, after)))
        })
        .await?
        .ok_or_else(|| {
            AppError::ApiTooManyRequests(format!(
                "{AGENT_ANSWERS_PER_DAY} exercises you graded yourself in 24 hours is the most one account records; course questions ({{ question_id, answer }}) still count"
            ))
        })?;
    Ok(Json(AnswerResult {
        lesson_id,
        correct,
        mastery_before: before.round() as u32,
        mastery: after.effective(at).round() as u32,
        graded: None,
    }))
}

/// History rows of agent-graded answers carry the prompt after this prefix.
pub const AGENT_PREFIX: &str = "agent:";

/// Agent-graded answers one account may record in 24 hours (#381): each is a
/// history row with the agent's free-text prompt. Generous for a tutor
/// teaching all day; a script looping on it stops here.
pub const AGENT_ANSWERS_PER_DAY: i64 = 1000;

/// Agent-graded answers this user recorded in the 24 hours before `now`.
fn agent_answers_today(c: &rusqlite::Connection, user_id: i64, now: i64) -> rusqlite::Result<i64> {
    c.query_row(
        "SELECT COUNT(*) FROM quiz_history WHERE user_id = ?1 AND answered_at > ?2 AND question_id LIKE 'agent:%'",
        rusqlite::params![user_id, now - 86_400],
        |r| r.get(0),
    )
}

#[derive(Deserialize)]
pub struct PassReport {
    pub lesson: String,
    /// The exercises of this lesson the learner answered in this sitting.
    pub answered: i64,
    pub correct: i64,
    /// How the learner showed it, in a sentence or two.
    pub evidence: String,
}

/// POST /api/progress/pass { lesson, answered, correct, evidence } — the
/// agent taught the lesson and the learner showed it under the rules.
/// Refused unless the numbers meet them.
pub async fn pass(State(state): State<Arc<AppState>>, p: Project, Json(req): Json<PassReport>) -> Result<Json<Outcome>, AppError> {
    super::limit_learning_write(&state, p.user.id)?;
    plausible(req.answered, req.correct)?;
    let l = find(&p.course, &req.lesson)?;
    if req.evidence.trim().is_empty() {
        return Err(AppError::ApiBadRequest("say in evidence how the learner showed it".into()));
    }
    if !progress::run_passes(l, req.correct, req.answered) {
        return Err(AppError::ApiBadRequest(format!(
            "not a pass under the rules: {} of {} right ({}%); this lesson needs at least {} answers and {}% right",
            req.correct,
            req.answered,
            progress::pct(req.correct, req.answered),
            progress::required_answers(l),
            progress::PASS_PCT
        )));
    }
    let (uid, cid, course, id) = (p.user.id, p.course_id().to_string(), Arc::clone(&p.course), l.id.clone());
    let (answered, correct, evidence) = (req.answered, req.correct, req.evidence);
    let out = state
        .db
        .call(move |c| {
            let l = course.lesson_by_id(&id).expect("lesson exists");
            let out = progress::record_taught_run(c, uid, &cid, l, "agent", correct, answered)?;
            crate::activity::touch(c, uid, &cid, &id, crate::activity::Mode::Agent)?;
            progress::set_evidence(c, uid, &cid, &id, &evidence)?;
            Ok(out)
        })
        .await?;
    tracing::info!("user {}: an agent passed {} ({correct}/{answered})", p.user.id, l.slug);
    Ok(Json(out))
}
