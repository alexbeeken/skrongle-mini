//! Guide: a short, guided, game-like path through the lessons.

use askama::Template;
use axum::extract::{Path, State};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use super::{CourseView, Page};
use crate::activity;
use crate::auth::{CurrentUser, Project};
use crate::content::{ContentStore, GuideStep, Lesson, VocabEntry};
use crate::error::AppError;
use crate::guide::{self, StageProgress};
use crate::progress::{self, LessonPass};
use crate::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageState {
    Locked,
    Available,
    InProgress,
    Complete,
    /// Below the level the learner started at and never studied: open,
    /// never the current stage, and it does not hold back the next one.
    Skipped,
}

pub struct StageCard {
    pub number: usize,
    pub slug: String,
    pub title: String,
    pub unit: String,
    pub group: String,
    pub summary: String,
    pub steps: usize,
    pub questions: usize,
    pub step: i64,
    pub percent: i64,
    pub stars: i64,
    pub state: &'static str,
    pub is_current: bool,
    /// The lesson's pass status in words ("Passed in Study mode", …).
    pub pass: String,
    /// A run was scored (a finished Guide run, or a run in any mode): the
    /// button says "Try again" rather than "Start" (#417).
    pub tried: bool,
}

/// One unit of the Guide list: its open and passed stages as cards, then
/// its locked stages as quiet rows under one lock (#1213).
pub struct GuideSection {
    pub title: String,
    pub cards: Vec<StageCard>,
    pub locked: Vec<StageCard>,
}

/// Split the stages into one section per unit, in course order.
fn sections(all: Vec<(StageCard, StageState)>) -> Vec<GuideSection> {
    let mut out: Vec<GuideSection> = Vec::new();
    for (card, st) in all {
        if out.last().is_none_or(|s| s.title != card.unit) {
            out.push(GuideSection { title: card.unit.clone(), cards: Vec::new(), locked: Vec::new() });
        }
        if let Some(sec) = out.last_mut() {
            if st == StageState::Locked {
                sec.locked.push(card);
            } else {
                sec.cards.push(card);
            }
        }
    }
    out
}

#[derive(Template)]
#[template(path = "guide.html")]
pub struct GuideTemplate {
    pub user: Option<CurrentUser>,
    pub course: std::sync::Arc<CourseView>,
    pub sections: Vec<GuideSection>,
    pub completed: usize,
    pub three_star: usize,
    pub total: usize,
    pub current_slug: Option<String>,
    pub current_title: Option<String>,
}

pub struct OptionView {
    pub text: String,
}

pub struct StepView {
    pub index: usize,
    pub kind: &'static str,
    pub title: String,
    pub html: std::sync::Arc<str>,
    pub text: String,
    // question
    pub question_id: String,
    pub prompt: String,
    pub qkind: &'static str,
    pub placeholder: String,
    pub answer_unit: String,
    pub math: bool,
    pub board: Option<crate::algebra_tiles::Tiles>,
    /// As in `QuestionView`: picked from the options, a clock to set or to read.
    pub choice: bool,
    pub clock_step: u32,
    pub clock_hands: String,
    /// A `measure` question's drawing.
    pub measure: Option<crate::measure::MeasureView>,
    pub options: Vec<OptionView>,
    pub hints: Vec<String>,
    pub abc: String,
    /// Music the speaker button plays, and the options as tunes (`QuestionView`).
    pub play: String,
    pub staves: Vec<String>,
    /// A fill-bar question's bar and tiles (static/fill-bar.js); `abc` stays empty.
    pub bar: String,
    pub tiles: Vec<String>,
    /// A chord-spell question's tiles and slots (static/chord-spell.js).
    pub chord: Vec<crate::notation::chord_spell::Tile>,
    pub slots: usize,
    /// A staff-note question's clef and note names as JSON (static/staff-note.js).
    pub clef: String,
    pub note_names: String,
    /// A keyboard question's octaves of keys and whether only the exact key counts (`QuestionView`).
    pub octaves: u8,
    pub octave: bool,
    // vocab
    pub vocab: Vec<VocabEntry>,
}

#[derive(Template)]
#[template(path = "stage.html")]
pub struct StageTemplate<'a> {
    pub user: Option<CurrentUser>,
    pub course: std::sync::Arc<CourseView>,
    pub lesson: &'a Lesson,
    pub unit: String,
    pub number: usize,
    pub steps: Vec<StepView>,
    pub start_step: i64,
    pub best_stars: i64,
    pub next_slug: Option<String>,
    pub next_title: Option<String>,
}

/// Complete means passed (`progress.rs`), whichever mode passed it.
fn stage_state(p: Option<&StageProgress>, pass: Option<&LessonPass>, total: usize, unlocked: bool) -> StageState {
    if pass.is_some_and(LessonPass::passed) {
        return StageState::Complete;
    }
    match p {
        Some(p) if p.step > 0 && (p.step as usize) < total => StageState::InProgress,
        _ if pass.is_some_and(LessonPass::seen) => StageState::InProgress,
        _ if unlocked => StageState::Available,
        _ => StageState::Locked,
    }
}

fn state_str(s: StageState) -> &'static str {
    match s {
        StageState::Locked => "locked",
        StageState::Available => "available",
        StageState::InProgress => "progress",
        StageState::Complete => "complete",
        StageState::Skipped => "skipped",
    }
}

/// Build the ordered stage list with lock state. A stage is complete once the
/// lesson is passed in any mode, and unlocks when the one before it is. The
/// stages of lessons below `start` (the unit the learner started at) that
/// were never begun are skipped: open, and the path starts after them. The
/// current stage is the lesson every mode resumes at (`activity::resume`,
/// from `touched`; changing the level touches its first lesson, #333), and it
/// is open even if the one before it is not passed.
pub(crate) fn stages(
    content: &ContentStore,
    progress: &HashMap<String, StageProgress>,
    passes: &HashMap<String, LessonPass>,
    touched: &HashMap<String, (i64, activity::Mode)>,
    start: usize,
) -> Vec<(StageCard, StageState)> {
    let resume = activity::resume(content, passes, touched, start).map(|r| r.lesson.id.as_str());
    let mut out = Vec::new();
    let mut unlocked = true;
    for (i, lesson) in content.lessons().iter().enumerate() {
        let p = progress.get(&lesson.id);
        let total = lesson.guide_steps.len();
        let pass = passes.get(&lesson.id);
        let is_current = resume == Some(lesson.id.as_str());
        let mut st = stage_state(p, pass, total, unlocked || is_current);
        if matches!(st, StageState::Available | StageState::Locked) && progress::is_skipped(lesson, start, touched.contains_key(&lesson.id)) {
            st = StageState::Skipped;
        }
        out.push((
            StageCard {
                number: i + 1,
                slug: lesson.slug.clone(),
                title: lesson.title.clone(),
                unit: content.unit(lesson.unit_index).title.clone(),
                group: content.group_title(&lesson.group).to_string(),
                summary: lesson.summary.clone(),
                steps: total,
                questions: lesson.guide_question_count(),
                step: p.map(|p| p.step.min(total as i64)).unwrap_or(0),
                percent: if total == 0 { 0 } else { p.map(|p| p.step.min(total as i64) * 100 / total as i64).unwrap_or(0) },
                stars: p.map(|p| p.best_stars).unwrap_or(0),
                state: state_str(st),
                is_current,
                pass: pass.map(LessonPass::summary).unwrap_or_default(),
                tried: p.is_some_and(|p| p.completed > 0) || pass.is_some_and(|x| x.runs > 0),
            },
            st,
        ));
        // Below the level the learner chose, nothing holds the path back.
        unlocked = matches!(st, StageState::Complete | StageState::Skipped) || lesson.unit_index < start;
    }
    out
}

/// GET /guide
pub async fn map(State(state): State<Arc<AppState>>, p: Project) -> Result<Response, AppError> {
    let uid = p.user.id;
    let content = &p.course;
    let cid = content.id().to_string();
    let (progress, passes, touched) = state
        .db
        .call(move |c| Ok((guide::load_all(c, uid, &cid)?, progress::load_all(c, uid, &cid)?, activity::last_touched(c, uid, &cid)?)))
        .await?;
    let all = stages(content, &progress, &passes, &touched, p.start);
    let completed = all.iter().filter(|(_, s)| *s == StageState::Complete).count();
    let three_star = all.iter().filter(|(c, _)| c.stars == 3).count();
    let current = all.iter().find(|(c, _)| c.is_current).map(|(c, _)| (c.slug.clone(), c.title.clone()));
    let total = all.iter().filter(|(_, s)| *s != StageState::Skipped).count();
    Ok(Page(GuideTemplate {
        course: content.view(),
        user: Some(p.user),
        sections: sections(all),
        completed,
        three_star,
        total,
        current_slug: current.as_ref().map(|c| c.0.clone()),
        current_title: current.map(|c| c.1),
    })
    .into_response())
}

/// GET /guide/{slug}
pub async fn stage(
    State(state): State<Arc<AppState>>,
    p: Project,
    Path(slug): Path<String>,
) -> Result<Response, AppError> {
    let content = &p.course;
    let lesson = content.lesson_by_slug(&slug).ok_or(AppError::NotFound)?;
    let uid = p.user.id;
    let cid = content.id().to_string();
    let (progress, passes, touched) = state
        .db
        .call(move |c| Ok((guide::load_all(c, uid, &cid)?, progress::load_all(c, uid, &cid)?, activity::last_touched(c, uid, &cid)?)))
        .await?;
    let all = stages(content, &progress, &passes, &touched, p.start);
    let (number, st) = all
        .iter()
        .find(|(c, _)| c.slug == slug)
        .map(|(c, s)| (c.number, *s))
        .ok_or(AppError::NotFound)?;
    if st == StageState::Locked {
        return Ok(Redirect::to(&crate::url("/guide")).into_response());
    }
    let (cid, lid) = (content.id().to_string(), lesson.id.clone());
    state.db.call(move |c| activity::touch(c, uid, &cid, &lid, activity::Mode::Guide)).await?;
    let pr = progress.get(&lesson.id);
    let total = lesson.guide_steps.len() as i64;
    // A finished stage starts over when reopened.
    let start_step = pr.map(|p| p.step).filter(|&s| s < total).unwrap_or(0);

    // Quick-check options in a mixed order, as in Practice: course files
    // mostly list the right answer first (#962).
    let mut rng = crate::quiz::Rng::from_entropy();
    let steps = lesson
        .guide_steps
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let mut v = StepView {
                index: i,
                kind: "",
                title: String::new(),
                html: "".into(),
                text: String::new(),
                question_id: String::new(),
                prompt: String::new(),
                qkind: "",
                placeholder: String::new(),
                answer_unit: String::new(),
                math: false,
                board: None,
                choice: false,
                clock_step: 0,
                clock_hands: String::new(),
                measure: None,
                options: vec![],
                hints: vec![],
                abc: String::new(),
                play: String::new(),
                staves: vec![],
                bar: String::new(),
                tiles: vec![],
                chord: vec![],
                slots: 0,
                clef: String::new(),
                note_names: String::new(),
                octaves: 1,
                octave: false,
                vocab: vec![],
            };
            match s {
                GuideStep::Concept { title, html } => {
                    v.kind = "concept";
                    v.title = title.clone();
                    v.html = html.clone();
                }
                GuideStep::Question { index } => {
                    let q = &lesson.quiz[*index];
                    v.kind = "question";
                    v.question_id = q.question_id.clone();
                    v.prompt = q.prompt.clone();
                    v.qkind = q.kind.as_str();
                    v.placeholder = q.kind.placeholder();
                    v.answer_unit = q.unit.clone();
                    v.math = q.kind.is_math();
                    v.board = crate::algebra_tiles::of(q);
                    (v.choice, v.clock_step, v.clock_hands) = (q.is_choice(), q.clock_step(), q.clock_hands());
                    v.measure = q.measure_view();
                    v.options = q.options.iter().map(|o| OptionView { text: o.clone() }).collect();
                    rng.shuffle(&mut v.options);
                    v.hints = q.hints.clone();
                    v.abc = q.abc.clone();
                    v.play = q.play.clone();
                    (v.octaves, v.octave) = (crate::notation::keyboard::octaves(q), q.octave);
                    v.staves = q.option_staves(&v.options.iter().map(|o| o.text.clone()).collect::<Vec<_>>());
                    if q.kind == crate::content::QuizType::FillBar {
                        v.bar = std::mem::take(&mut v.abc);
                        v.tiles = q.tiles.clone();
                        rng.shuffle(&mut v.tiles);
                    }
                    v.chord = crate::notation::chord_spell::tiles(content, q, &mut rng);
                    v.slots = crate::notation::chord_spell::slots(q);
                    (v.clef, v.note_names) = crate::notation::staff_note::view(content, q);
                }
                GuideStep::Vocab { entries } => {
                    v.kind = "vocab";
                    v.title = crate::t("guide.play.terms_to_know");
                    v.vocab = entries.clone();
                }
                GuideStep::Tip { text } => {
                    v.kind = "tip";
                    v.text = text.clone();
                }
            }
            v
        })
        .collect();

    let next = content.lessons().get(number).map(|l| (l.slug.clone(), l.title.clone()));
    let tpl = StageTemplate {
        course: content.view(),
        user: Some(p.user.clone()),
        lesson,
        unit: content.unit(lesson.unit_index).title.clone(),
        number,
        steps,
        start_step,
        best_stars: pr.map(|p| p.best_stars).unwrap_or(0),
        next_slug: next.as_ref().map(|n| n.0.clone()),
        next_title: next.map(|n| n.1),
    };
    Ok(Html(tpl.render()?).into_response())
}

#[derive(Deserialize)]
pub struct StepRequest {
    pub lesson_id: String,
    pub step: i64,
    #[serde(default)]
    pub correct: bool,
    /// How long the player has been on the stage page: when the stage
    /// finishes, the reward line counts what was logged in that time.
    #[serde(default)]
    pub duration_ms: i64,
}

#[derive(Serialize)]
pub struct StepResponse {
    pub step: i64,
    pub total: i64,
    pub finished: bool,
    pub stars: Option<i64>,
    pub run_correct: i64,
    pub run_answered: i64,
    /// Set when the stage finished: what the run did for the lesson's pass.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pass: Option<progress::Outcome>,
    /// Set when the stage finished: the worn motivator's line, as HTML (#568).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reward: Option<String>,
}

/// POST /api/guide/step  { lesson_id, step, correct?, duration_ms? }
pub async fn complete_step(
    State(state): State<Arc<AppState>>,
    p: Project,
    headers: axum::http::HeaderMap,
    Json(req): Json<StepRequest>,
) -> Result<Json<StepResponse>, AppError> {
    super::limit_learning_write(&state, p.user.id)?;
    let since = crate::db::now() - req.duration_ms.clamp(0, 6 * 3600 * 1000) / 1000 - 1;
    let content = &p.course;
    let lesson = content.lesson_by_id(&req.lesson_id).ok_or_else(|| AppError::ApiBadRequest("unknown lesson".into()))?;
    let total = lesson.guide_steps.len() as i64;
    let step = lesson
        .guide_steps
        .get(usize::try_from(req.step).unwrap_or(usize::MAX))
        .ok_or_else(|| AppError::ApiBadRequest("step out of range".into()))?;
    let is_question = step.is_question();
    let uid = p.user.id;
    let tz = p.user.tz;
    let cid = content.id().to_string();
    let lesson_id = lesson.id.clone();
    let (index, correct) = (req.step, req.correct);
    let lesson_arc = std::sync::Arc::clone(&p.course);
    let (r, pass) = state
        .db
        .call(move |c| {
            let r = guide::complete_step(c, uid, &cid, &lesson_id, index, total, is_question, correct)?;
            // A question step was graded (and counted) by /api/answer; a card, a
            // flashcard hand or a tip counts here.
            if r.counted && !is_question {
                activity::record(c, uid, &cid, activity::Kind::Card, 1, tz)?;
            }
            // A finished stage taught the lesson (its cards) and is one run of it.
            let pass = match (r.finished, lesson_arc.lesson_by_id(&lesson_id)) {
                (true, Some(l)) => Some(progress::record_taught_run(c, uid, &cid, l, progress::VIA_PLAY, r.progress.run_correct, r.progress.run_answered)?),
                _ => None,
            };
            Ok((r, pass))
        })
        .await?;
    let reward = if r.finished { super::reward_line(&state, &p, super::sprint::tz_offset_secs(&headers), since, Some(lesson.id.clone())).await? } else { None };
    Ok(Json(StepResponse {
        step: r.progress.step,
        total,
        finished: r.finished,
        stars: r.stars,
        run_correct: r.progress.run_correct,
        run_answered: r.progress.run_answered,
        pass,
        reward,
    }))
}

#[derive(Deserialize)]
pub struct RestartRequest {
    pub lesson_id: String,
}

/// POST /api/guide/restart { lesson_id }
pub async fn restart(
    State(state): State<Arc<AppState>>,
    p: Project,
    Json(req): Json<RestartRequest>,
) -> Result<Json<StepResponse>, AppError> {
    let content = &p.course;
    let lesson = content.lesson_by_id(&req.lesson_id).ok_or_else(|| AppError::ApiBadRequest("unknown lesson".into()))?;
    let total = lesson.guide_steps.len() as i64;
    let uid = p.user.id;
    let cid = content.id().to_string();
    let lesson_id = lesson.id.clone();
    let pr = state.db.call(move |c| guide::restart(c, uid, &cid, &lesson_id)).await?;
    Ok(Json(StepResponse {
        step: pr.step,
        total,
        finished: false,
        stars: None,
        run_correct: 0,
        run_answered: 0,
        pass: None,
        reward: None,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn course() -> ContentStore {
        crate::content::tests::store(&[
            r#"{"id":"a","slug":"a","title":"A","unit":"A1","content":"x"}"#,
            r#"{"id":"b","slug":"b","title":"B","unit":"A1","content":"x"}"#,
            r#"{"id":"c","slug":"c","title":"C","unit":"A2","content":"x"}"#,
            r#"{"id":"d","slug":"d","title":"D","unit":"A2","content":"x"}"#,
        ])
        .unwrap()
    }

    fn passed(id: &str) -> LessonPass {
        LessonPass { lesson_id: id.into(), seen_at: Some(1), passed_at: Some(1), ..Default::default() }
    }

    fn current(all: &[(StageCard, StageState)]) -> Vec<&str> {
        all.iter().filter(|(c, _)| c.is_current).map(|(c, _)| c.slug.as_str()).collect()
    }

    #[test]
    fn changing_the_level_moves_the_current_stop_and_keeps_passes() {
        let c = course();
        let progress = HashMap::new();
        // A passed, B seen (begun) but not passed, and touched last.
        let seen = LessonPass { lesson_id: "b".into(), seen_at: Some(1), ..Default::default() };
        let passes = HashMap::from([("a".to_string(), passed("a")), ("b".to_string(), seen.clone())]);
        let mut touched = HashMap::from([("a".to_string(), (1, activity::Mode::Study)), ("b".to_string(), (2, activity::Mode::Study))]);

        let from_start = stages(&c, &progress, &passes, &touched, 0);
        assert_eq!(current(&from_start), vec!["b"]);
        assert_eq!(from_start[2].1, StageState::Locked);

        // Move up to A2, which touches its first lesson (`pages::set_start`):
        // A stays complete, B stays in progress, and the path continues at C,
        // open, not locked behind B.
        touched.insert("c".into(), (3, activity::Mode::Study));
        let at_a2 = stages(&c, &progress, &passes, &touched, 1);
        assert_eq!(at_a2[0].1, StageState::Complete);
        assert_eq!(at_a2[1].1, StageState::InProgress);
        assert_eq!(at_a2[2].1, StageState::Available);
        assert_eq!(current(&at_a2), vec!["c"]);

        // A never-touched lesson below the level is skipped.
        let fresh = stages(&c, &HashMap::new(), &HashMap::new(), &HashMap::from([("c".to_string(), (1, activity::Mode::Study))]), 1);
        assert_eq!((fresh[0].1, fresh[1].1), (StageState::Skipped, StageState::Skipped));
        assert_eq!(current(&fresh), vec!["c"]);

        // Everything from A2 on complete: the begun A1 lesson is current again.
        let passes = HashMap::from([("a".into(), passed("a")), ("b".into(), seen), ("c".into(), passed("c")), ("d".into(), passed("d"))]);
        assert_eq!(current(&stages(&c, &progress, &passes, &touched, 1)), vec!["b"]);
    }
}
