//! Sprint mode: the phone-first path and session player.
//!
//! `/sprint` is the home screen (streak, the daily goal's ring, the path of lessons);
//! `/sprint/play` builds a session and hands it to `static/sprint.js` as JSON.
//! `/api/sprint/check` grades Sprint mode-only exercises and `/api/sprint/finish` records a
//! finished session (Guide stage completion for lesson sessions, and a
//! `game_activity` row for every session).

use askama::Template;
use axum::extract::{Query, State};
use axum::http::{header, HeaderMap};
use axum::response::{Html, IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::guide::{stages, StageState};
use super::{CourseView, Page};
use crate::auth::Project;
use crate::db::now;
use crate::activity::{self, Today};
use crate::sprint::{self, Effect, Session};
use crate::error::AppError;
use crate::mastery;
use crate::quiz::Rng;
use crate::{games, guide, progress, vocab, AppState};

// ---------- home ----------

pub struct Node {
    pub number: usize,
    pub slug: String,
    pub title: String,
    pub icon: String,
    pub state: &'static str,
    pub stars: i64,
    pub mastery: u32,
    pub chilis: u8,
    pub is_current: bool,
    pub group: String,
    pub summary: String,
    /// The lesson's pass status in words.
    pub pass: String,
}

impl Node {
    /// The button's accessible name: the title and its state in words, since
    /// the path shows the state only as an icon (🔒, ✓).
    pub fn label(&self) -> String {
        let state = match self.state {
            "locked" => crate::t("sprint.state.locked"),
            "complete" => crate::t("sprint.state.complete"),
            "progress" => crate::t("sprint.state.in_progress"),
            "skipped" => crate::t("sprint.state.skipped"),
            _ => crate::t("sprint.state.not_started"),
        };
        if self.is_current {
            crate::t2("sprint.node_label_current", "title", &self.title, "state", state)
        } else {
            crate::t2("sprint.node_label", "title", &self.title, "state", state)
        }
    }
}

pub struct UnitGroup {
    pub unit: String,
    pub description: String,
    pub done: usize,
    pub total: usize,
    pub nodes: Vec<Node>,
}

#[derive(Template)]
#[template(path = "sprint.html")]
pub struct HomeTemplate {
    pub course: CourseView,
    pub streak: i64,
    pub practised_today: bool,
    pub today: Today,
    pub words: i64,
    pub completed: usize,
    pub total: usize,
    pub current_slug: Option<String>,
    pub current_title: Option<String>,
    pub current_icon: String,
    pub units: Vec<UnitGroup>,
    /// Whether any lesson with questions is read: the review mix draws from
    /// those, else it is only a taste of what's next.
    pub review_read: bool,
}

/// Minutes east of UTC, from the `tz` cookie the player sets (default UTC).
pub(crate) fn tz_offset_secs(headers: &HeaderMap) -> i64 {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|raw| raw.split(';'))
        .find_map(|pair| {
            let (k, v) = pair.trim().split_once('=')?;
            if k.trim() != "tz" {
                return None;
            }
            let minutes: i64 = v.trim().parse().ok()?;
            (-14 * 60..=14 * 60).contains(&minutes).then_some(minutes * 60)
        })
        .unwrap_or(0)
}

/// GET /sprint
pub async fn home(State(state): State<Arc<AppState>>, p: Project, headers: HeaderMap) -> Result<Response, AppError> {
    let at = now();
    let uid = p.user.id;
    let content = &p.course;
    let cid = content.id().to_string();
    let tz = tz_offset_secs(&headers);
    let day_start = sprint::local_day_start(tz, at);
    let (progress, gprogress, passes, touched, streak, today, words) = state
        .db
        .call(move |c| {
            let p = mastery::load_progress(c, uid, &cid)?;
            let j = guide::load_all(c, uid, &cid)?;
            let passes = progress::load_all(c, uid, &cid)?;
            let touched = activity::last_touched(c, uid, &cid)?;
            let streak = sprint::day_streak(c, uid, &cid, tz, at)?;
            let today = Today::load(c, uid, &cid, day_start)?;
            let words = vocab::count(c, uid, &cid)?;
            Ok((p, j, passes, touched, streak, today, words))
        })
        .await?;
    let review_read = content.lessons().iter().any(|l| !l.quiz.is_empty() && crate::quiz::is_read(&progress, l));
    let all = stages(content, &gprogress, &passes, &touched, p.start);
    let completed = all.iter().filter(|(_, s)| *s == StageState::Complete).count();
    let total = all.iter().filter(|(_, s)| *s != StageState::Skipped).count();
    let current = all.iter().find(|(c, _)| c.is_current).map(|(c, _)| (c.slug.clone(), c.title.clone()));
    let current_icon = current
        .as_ref()
        .and_then(|(slug, _)| content.lesson_by_slug(slug))
        .map(|l| sprint::lesson_icon(l, content.lessons().iter().position(|x| x.id == l.id).unwrap_or(0)))
        .unwrap_or_else(|| crate::content::Icons::default().start);

    let mut units: Vec<UnitGroup> = Vec::new();
    for (i, ((card, st), lesson)) in all.iter().zip(content.lessons()).enumerate() {
        let m = progress.get(&lesson.id).map(|p| p.effective(at)).unwrap_or(0.0);
        let node = Node {
            number: card.number,
            slug: card.slug.clone(),
            title: card.title.clone(),
            icon: sprint::lesson_icon(lesson, i),
            state: card.state,
            stars: card.stars,
            mastery: m.round() as u32,
            chilis: sprint::chilis(m),
            is_current: card.is_current,
            group: card.group.clone(),
            summary: card.summary.clone(),
            pass: card.pass.clone(),
        };
        let unit = content.unit(lesson.unit_index);
        match units.last_mut() {
            Some(g) if g.unit == unit.title => {
                g.total += usize::from(*st != StageState::Skipped);
                if *st == StageState::Complete {
                    g.done += 1;
                }
                g.nodes.push(node);
            }
            _ => units.push(UnitGroup {
                unit: unit.title.clone(),
                description: unit.description.clone(),
                done: usize::from(*st == StageState::Complete),
                total: usize::from(*st != StageState::Skipped),
                nodes: vec![node],
            }),
        }
    }

    Ok(Page(HomeTemplate {
        course: CourseView::of(content),
        streak: streak.0,
        practised_today: streak.1,
        today,
        words,
        completed,
        total,
        current_slug: current.as_ref().map(|c| c.0.clone()),
        current_title: current.map(|c| c.1),
        current_icon,
        units,
        review_read,
    })
    .into_response())
}

// ---------- play ----------

#[derive(Template)]
#[template(path = "sprint_play.html")]
pub struct PlayTemplate {
    pub title: String,
    pub course_title: String,
    pub course: CourseView,
    pub session_json: String,
    /// A quiz item or a spot or a scale draws music: load the notation scripts.
    pub notation: bool,
    /// An exercise plays music (`play`, or a keyboard's keys): load the sound engine.
    pub sound: bool,
    /// A staff-note question: load static/staff-note.js.
    pub staff_note: bool,
    /// A keyboard question (#1231): load static/keyboard.js.
    pub keyboard: bool,
}

#[derive(Deserialize)]
pub struct PlayParams {
    pub lesson: Option<String>,
    pub mode: Option<String>,
}

/// What a Sprint session is built from.
struct SessionInputs {
    progress: Arc<HashMap<String, mastery::Progress>>,
    avoid: HashSet<String>,
    learned: vocab::Learned,
}

/// Read what the session needs (#627). A lesson session uses only the
/// questions answered right today; the review and lightning mixes (`mix`)
/// also weigh the course's progress and draw from every learned word, which
/// they read as bare keys, with the ones due today (#1360) dealt first.
fn session_inputs(c: &rusqlite::Connection, uid: i64, cid: &str, mix: bool, tz: i64) -> rusqlite::Result<SessionInputs> {
    let avoid = mastery::recently_correct(c, uid, cid, 24 * 3600)?.into_iter().collect();
    let (progress, learned) = if mix { (mastery::load_progress(c, uid, cid)?, vocab::learned_for_mix(c, uid, cid, tz, now())?) } else { Default::default() };
    Ok(SessionInputs { progress, avoid, learned })
}

/// GET /sprint/play?lesson=slug | ?mode=review | ?mode=lightning
pub async fn play(
    State(state): State<Arc<AppState>>,
    p: Project,
    Query(params): Query<PlayParams>,
) -> Result<Response, AppError> {
    let at = now();
    let uid = p.user.id;
    let tz = p.user.tz;
    let content = &p.course;
    let cid = content.id().to_string();
    let only = match params.lesson.as_deref() {
        Some(slug) => Some(content.lesson_by_slug(slug).ok_or(AppError::NotFound)?),
        None => None,
    };
    let touched = only.map(|l| l.id.clone());
    let SessionInputs { progress, avoid, learned } = state
        .db
        .call(move |c| {
            if let Some(id) = &touched {
                activity::touch(c, uid, &cid, id, activity::Mode::Sprint)?;
            }
            session_inputs(c, uid, &cid, touched.is_none(), tz)
        })
        .await?;
    let mut rng = Rng::from_entropy();

    let session: Session = match (only, params.mode.as_deref()) {
        (Some(lesson), _) => sprint::lesson_session(content, lesson, &avoid, &mut rng),
        (None, Some("lightning")) => sprint::lightning_session(content, &progress, &avoid, &learned, p.start, at, &mut rng),
        _ => sprint::review_session(content, &progress, &avoid, &learned, p.start, at, &mut rng),
    };
    let session_json = super::script_json(&session)?;
    use sprint::Exercise::{Choice, Keyboard, Scale, Sort, Tiles};
    let keyboard = session.exercises.iter().any(|e| matches!(e, Keyboard { .. }));
    let sound = keyboard || session.exercises.iter().any(|e| match e {
        Choice { play, .. } | Tiles { play, .. } => !play.is_empty(),
        Sort { items, .. } => items.iter().any(|i| !i.play.is_empty()),
        Scale { .. } => true,
        _ => false,
    });
    // A played tune is drawn after the answer, staves may be the options, and a scale draws its notes.
    let notation = session.exercises.iter().any(|e| match e {
        Choice { abc, play, .. } | Tiles { abc, play, .. } | Keyboard { abc, play, .. } => !abc.is_empty() || !play.is_empty(),
        sprint::Exercise::FillBar { abc, .. } | sprint::Exercise::Spot { abc, .. } => !abc.is_empty(),
        sprint::Exercise::ChordSpell { .. } | Scale { .. } => true,
        _ => false,
    });
    let staff_note = session.exercises.iter().any(|e| matches!(e, sprint::Exercise::StaffNote { .. }));
    let tpl = PlayTemplate { title: session.title.clone(), course_title: content.title().to_string(), course: CourseView::of(content), session_json, notation, sound, staff_note, keyboard };
    Ok(Html(tpl.render()?).into_response())
}

// ---------- api ----------

#[derive(Deserialize)]
pub struct CheckRequest {
    pub id: String,
    pub answer: serde_json::Value,
    #[serde(default)]
    pub mistakes: u32,
}

#[derive(Serialize)]
pub struct CheckResponse {
    pub correct: bool,
    pub correct_answer: String,
    pub explanation: String,
    pub note: Option<String>,
    pub mastery: Option<u32>,
    pub new_words: Vec<NewWord>,
}

#[derive(Serialize)]
pub struct NewWord {
    pub term: String,
    pub meaning: String,
}

/// POST /api/sprint/check { id, answer, mistakes? }
pub async fn check(
    State(state): State<Arc<AppState>>,
    p: Project,
    Json(req): Json<CheckRequest>,
) -> Result<Json<CheckResponse>, AppError> {
    if req.id.len() > 200 {
        return Err(AppError::ApiBadRequest("bad id".into()));
    }
    // Typed answers, picked options and arranged tiles all fit easily.
    if req.answer.to_string().len() > 4000 {
        return Err(AppError::ApiBadRequest("answer too long".into()));
    }
    let content = &p.course;
    let graded = sprint::check(content, &req.id, &req.answer, req.mistakes.min(100)).map_err(AppError::ApiBadRequest)?;
    let uid = p.user.id;
    let tz = p.user.tz;
    let cid = content.id().to_string();
    let id = req.id.clone();
    let at = now();
    let correct = graded.correct;
    let effect = graded.effect;
    let (mastery, new_words) = state
        .db
        .call(move |c| {
            let mut new_words: Vec<(String, String)> = Vec::new();
            let mut mastery = None;
            let mut apply_vocab = |c: &rusqlite::Connection, lesson_id: &str, known: &[String], missed: &[String]| -> rusqlite::Result<()> {
                let all: Vec<&str> = known.iter().chain(missed.iter()).map(String::as_str).collect();
                for w in vocab::record_learned(c, uid, &cid, lesson_id, &all, tz)? {
                    new_words.push((lesson_id.to_string(), w));
                }
                for w in known {
                    vocab::review(c, uid, &cid, lesson_id, w, true, tz)?;
                }
                for w in missed {
                    vocab::review(c, uid, &cid, lesson_id, w, false, tz)?;
                }
                Ok(())
            };
            // A quiz question counts as an answer, any other exercise a little less.
            let kind = if matches!(effect, Effect::Mastery { .. }) { activity::Kind::Answer } else { activity::Kind::Exercise };
            activity::record(c, uid, &cid, kind, 1, tz)?;
            match effect {
                Effect::Mastery { lesson_id, selected } => {
                    let p = mastery::record_answer(c, uid, &cid, &lesson_id, &id, &selected, correct)?;
                    mastery = Some(p.effective(at).round() as u32);
                }
                Effect::Vocab { lesson_id, known, missed } => apply_vocab(c, &lesson_id, &known, &missed)?,
                Effect::VocabMany(list) => {
                    for (lesson_id, known, missed) in list {
                        apply_vocab(c, &lesson_id, &known, &missed)?;
                    }
                }
                Effect::None => {}
            }
            Ok((mastery, new_words))
        })
        .await?;
    let new_words = new_words
        .into_iter()
        .filter_map(|(lesson_id, term)| {
            let l = content.lesson_by_id(&lesson_id)?;
            l.vocabulary.iter().find(|v| v.term == term).map(|v| NewWord { term: v.term.clone(), meaning: v.meaning.clone() })
        })
        .collect();
    Ok(Json(CheckResponse {
        correct,
        correct_answer: graded.correct_answer,
        explanation: graded.explanation,
        note: graded.note,
        mastery,
        new_words,
    }))
}

#[derive(Deserialize)]
pub struct FinishRequest {
    pub mode: String,
    pub lesson_id: Option<String>,
    pub answered: i64,
    pub correct: i64,
    pub duration_ms: i64,
}

#[derive(Serialize)]
pub struct FinishResponse {
    pub stars: i64,
    pub completed: Option<i64>,
    pub streak: i64,
    /// Today's effort points and the daily goal (`activity.rs`).
    pub today: i64,
    pub goal: i64,
    /// Lesson sessions: what the session did for the lesson's pass.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pass: Option<progress::Outcome>,
    /// The worn motivator's line for the session, as HTML (#568); none if
    /// it earned nothing.
    pub reward: Option<String>,
}

/// POST /api/sprint/finish { mode, lesson_id?, answered, correct, duration_ms }
pub async fn finish(
    State(state): State<Arc<AppState>>,
    p: Project,
    headers: HeaderMap,
    Json(req): Json<FinishRequest>,
) -> Result<Json<FinishResponse>, AppError> {
    super::limit_learning_write(&state, p.user.id)?;
    if req.answered < 0 || req.answered > 200 || req.correct < 0 || req.correct > req.answered || req.duration_ms < 0 {
        return Err(AppError::ApiBadRequest("implausible result".into()));
    }
    let content = &p.course;
    let (board, label, lesson_id) = match req.mode.as_str() {
        "lesson" => {
            let lesson = req
                .lesson_id
                .as_deref()
                .and_then(|id| content.lesson_by_id(id))
                .ok_or_else(|| AppError::ApiBadRequest("unknown lesson".into()))?;
            (lesson.id.clone(), lesson.title.clone(), Some(lesson.id.clone()))
        }
        "lightning" => (sprint::BOARD_LIGHTNING.to_string(), "Lightning round".to_string(), None),
        "review" => (sprint::BOARD_REVIEW.to_string(), "Review mix".to_string(), None),
        _ => return Err(AppError::ApiBadRequest("unknown mode".into())),
    };
    let stars = guide::stars_for(req.correct, req.answered);
    let uid = p.user.id;
    let cid = content.id().to_string();
    let tz = tz_offset_secs(&headers);
    let at = now();
    let day_start = sprint::local_day_start(tz, at);
    let (answered, correct, duration) = (req.answered, req.correct, req.duration_ms.min(6 * 3600 * 1000));
    let course = std::sync::Arc::clone(&p.course);
    // The session's answers were logged as they came: it began `duration` ago.
    let since = at - duration / 1000 - 1;
    let (stage, pass, streak, today, reward) = state
        .db
        .call(move |c| {
            let stage = match &lesson_id {
                Some(id) => Some(guide::record_run(c, uid, &cid, id, correct, answered)?),
                None => None,
            };
            // A lesson session opens with the lesson's bites (seen) and is one run of it.
            let pass = match lesson_id.as_deref().and_then(|id| course.lesson_by_id(id)) {
                Some(l) => Some(progress::record_taught_run(c, uid, &cid, l, progress::VIA_SPRINT, correct, answered)?),
                None => None,
            };
            games::record_activity(c, uid, &cid, sprint::GAME, &board, &label, duration, answered, correct, None)?;
            let (streak, _) = sprint::day_streak(c, uid, &cid, tz, at)?;
            let today = Today::load(c, uid, &cid, day_start)?;
            let reward = crate::motivators::reward(c, uid, &course, tz, since, lesson_id.as_deref())?;
            Ok((stage, pass, streak, today, reward))
        })
        .await?;
    Ok(Json(FinishResponse {
        stars,
        completed: stage.as_ref().map(|s| s.completed),
        streak,
        today: today.points,
        goal: today.goal,
        pass,
        reward: crate::motivators::reward_html(reward),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    /// #627: the narrower reads build the same sessions as the old full reads.
    #[test]
    fn session_inputs_build_the_same_sessions() {
        let store = crate::content::ContentStore::load_dir(std::path::Path::new("data/courses/spanish")).unwrap();
        let cid = store.id().to_string();
        let db = crate::db::Db::open_in_memory().unwrap();
        let lessons: Vec<&crate::content::Lesson> = store.lessons().iter().filter(|l| !l.vocabulary.is_empty()).take(4).collect();
        let (old, lesson_new, mix_new) = db
            .with(|c| {
                let uid = crate::auth::create_user(c, "ann@example.org", "h")?;
                for l in &lessons {
                    let words: Vec<&str> = l.vocabulary.iter().map(|v| v.term.as_str()).collect();
                    vocab::record_learned(c, uid, &cid, &l.id, &words, 0)?;
                    if let Some(q) = l.quiz.first() {
                        mastery::record_answer(c, uid, &cid, &l.id, &q.question_id, "x", true)?;
                    }
                }
                // What `play` read before: everything, for every kind of session.
                let old = SessionInputs {
                    progress: mastery::load_progress(c, uid, &cid)?,
                    avoid: mastery::recently_correct(c, uid, &cid, 24 * 3600)?.into_iter().collect(),
                    learned: vocab::Learned { keys: vocab::load_all(c, uid, &cid)?.into_keys().collect(), due: Default::default() },
                };
                Ok((old, session_inputs(c, uid, &cid, false, 0)?, session_inputs(c, uid, &cid, true, 0)?))
            })
            .unwrap();
        assert!(!old.learned.is_empty() && !old.progress.is_empty() && !old.avoid.is_empty());
        assert!(lesson_new.learned.is_empty() && lesson_new.progress.is_empty());
        let json = |s: &Session| serde_json::to_value(s).unwrap();
        let at = now();
        for l in &lessons {
            let a = sprint::lesson_session(&store, l, &old.avoid, &mut Rng::seeded(11));
            let b = sprint::lesson_session(&store, l, &lesson_new.avoid, &mut Rng::seeded(11));
            assert_eq!(json(&a), json(&b), "lesson {}", l.id);
        }
        for seed in 1..5 {
            let a = sprint::review_session(&store, &old.progress, &old.avoid, &old.learned, 0, at, &mut Rng::seeded(seed));
            let b = sprint::review_session(&store, &mix_new.progress, &mix_new.avoid, &mix_new.learned, 0, at, &mut Rng::seeded(seed));
            assert_eq!(json(&a), json(&b));
            let a = sprint::lightning_session(&store, &old.progress, &old.avoid, &old.learned, 0, at, &mut Rng::seeded(seed));
            let b = sprint::lightning_session(&store, &mix_new.progress, &mix_new.avoid, &mix_new.learned, 0, at, &mut Rng::seeded(seed));
            assert_eq!(json(&a), json(&b));
        }
    }

    #[test]
    fn tz_cookie_parses() {
        let mut h = HeaderMap::new();
        assert_eq!(tz_offset_secs(&h), 0);
        h.insert(header::COOKIE, HeaderValue::from_static("sid=abc; tz=-300"));
        assert_eq!(tz_offset_secs(&h), -300 * 60);
        h.insert(header::COOKIE, HeaderValue::from_static("tz=99999"));
        assert_eq!(tz_offset_secs(&h), 0);
    }
}
