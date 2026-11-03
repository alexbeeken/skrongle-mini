//! Quiz page + answer API.

use askama::Template;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;

use super::{CourseView, Page};
use crate::activity;
use crate::auth::{CurrentUser, Project};
use crate::db::now;
use crate::error::AppError;
use crate::mastery;
use crate::content::{ContentStore, Lesson, QuizItem, QuizType};
use crate::quiz::{build_quiz, check_answer, check_len, next_unread, Rng, DEFAULT_QUIZ_LEN};
use crate::vocab;
use crate::AppState;

pub struct QuestionView {
    pub question_id: String,
    pub lesson_slug: String,
    pub lesson_title: String,
    pub unit: String,
    pub prompt: String,
    pub kind: &'static str,
    pub options: Vec<String>,
    pub hints: Vec<String>,
    /// The answer box: its placeholder, a unit shown after it, and whether it
    /// takes a number or formula (no accent bar).
    pub placeholder: String,
    pub answer_unit: String,
    pub math: bool,
    /// Music drawn as a staff above the prompt (ABC notation).
    pub abc: String,
    /// Music the speaker button plays (ABC), and each option as a whole tune
    /// when the options are drawn as staves (`option_kind: "abc"`).
    pub play: String,
    pub staves: Vec<String>,
    /// A fill-bar question: its bar (`abc` with the gap, drawn by
    /// static/fill-bar.js instead of above the prompt) and its tiles, shuffled.
    pub bar: String,
    pub tiles: Vec<String>,
    /// A chord-spell question: its tiles, shuffled and named, and its slots (static/chord-spell.js).
    pub chord: Vec<crate::notation::chord_spell::Tile>,
    pub slots: usize,
    /// A staff-note question: its clef, and the note names as JSON (static/staff-note.js).
    pub clef: String,
    pub note_names: String,
    /// A keyboard question: how many octaves of keys, and whether only the exact key counts.
    pub octaves: u8,
    pub octave: bool,
    /// An `algebra-tiles` question's pile, drawn as a board.
    pub board: Option<crate::algebra_tiles::Tiles>,
    /// Picked from `options` (a multiple choice, a clock to read).
    pub choice: bool,
    /// A clock to set (its minute hand's step) or to read (its hands); see `QuizItem::clock_step`.
    pub clock_step: u32,
    pub clock_hands: String,
    /// A `measure` question's drawing.
    pub measure: Option<crate::measure::MeasureView>,
}

impl QuestionView {
    /// A question as the templates show it, with its choices shuffled.
    pub fn new(content: &ContentStore, lesson: &Lesson, item: &QuizItem, rng: &mut Rng) -> QuestionView {
        let mut options = item.options.clone();
        rng.shuffle(&mut options);
        let mut tiles = item.tiles.clone();
        rng.shuffle(&mut tiles);
        let bar = if item.kind == QuizType::FillBar { item.abc.clone() } else { String::new() };
        let (clef, note_names) = crate::notation::staff_note::view(content, item);
        QuestionView {
            question_id: item.question_id.clone(),
            lesson_slug: lesson.slug.clone(),
            lesson_title: lesson.title.clone(),
            unit: content.unit(lesson.unit_index).title.clone(),
            prompt: item.prompt.clone(),
            kind: item.kind.as_str(),
            hints: item.hints.clone(),
            placeholder: item.kind.placeholder(),
            answer_unit: item.unit.clone(),
            math: item.kind.is_math(),
            abc: if bar.is_empty() { item.abc.clone() } else { String::new() },
            bar,
            tiles,
            play: item.play.clone(),
            staves: item.option_staves(&options),
            options,
            chord: crate::notation::chord_spell::tiles(content, item, rng),
            slots: crate::notation::chord_spell::slots(item),
            clef,
            note_names,
            octaves: crate::notation::keyboard::octaves(item),
            octave: item.octave,
            board: crate::algebra_tiles::of(item),
            choice: item.is_choice(),
            clock_step: item.clock_step(),
            clock_hands: item.clock_hands(),
            measure: item.measure_view(),
        }
    }
}

/// The lesson to read next, offered when recommended practice has nothing to draw from.
pub struct NextLesson {
    pub slug: String,
    pub title: String,
}

#[derive(Template)]
#[template(path = "quiz.html")]
pub struct QuizTemplate {
    pub user: Option<CurrentUser>,
    pub course: CourseView,
    pub heading: String,
    pub subheading: String,
    pub lesson_slug: Option<String>,
    pub questions: Vec<QuestionView>,
    /// Some question has tiles: the page loads the board's script.
    pub has_tiles: bool,
    /// Set on an empty recommended quiz: where to go to unlock some practice.
    pub next_lesson: Option<NextLesson>,
}

impl QuizTemplate {
    /// Is there music to draw (load the notation scripts)?
    pub fn notation(&self) -> bool {
        self.questions.iter().any(|q| !q.abc.is_empty() || !q.play.is_empty() || !q.staves.is_empty() || !q.bar.is_empty() || q.slots > 0)
    }

    /// Is there music to play (load the sound engine)?
    pub fn sound(&self) -> bool {
        self.questions.iter().any(|q| !q.play.is_empty()) || self.keyboard()
    }

    /// Is there a keyboard question (load static/keyboard.js)?
    pub fn keyboard(&self) -> bool {
        self.questions.iter().any(|q| q.kind == "keyboard")
    }

    /// Is there a fill-bar question (load static/fill-bar.js)?
    pub fn fill_bar(&self) -> bool {
        self.questions.iter().any(|q| !q.bar.is_empty())
    }

    /// Is there a chord-spell question (load static/chord-spell.js)?
    pub fn chord_spell(&self) -> bool {
        self.questions.iter().any(|q| q.slots > 0)
    }
    /// Is there a staff-note question (load static/staff-note.js)?
    pub fn staff_note(&self) -> bool {
        self.questions.iter().any(|q| !q.clef.is_empty())
    }
}

#[derive(Deserialize)]
pub struct PracticeParams {
    pub lesson: Option<String>,
    pub n: Option<usize>,
}

/// GET /practice            -> recommended mixed quiz, biased to weak topics
/// GET /practice?lesson=slug -> quiz on one lesson
pub async fn practice(
    State(state): State<Arc<AppState>>,
    p: Project,
    Query(params): Query<PracticeParams>,
) -> Result<Response, AppError> {
    let content = &p.course;
    let only = match params.lesson.as_deref() {
        Some(slug) => Some(content.lesson_by_slug(slug).ok_or(AppError::NotFound)?),
        None => None,
    };
    let at = now();
    let uid = p.user.id;
    let cid = content.id().to_string();
    let touched = only.map(|l| l.id.clone());
    let (progress, recent) = state
        .db
        .call(move |c| {
            if let Some(id) = &touched {
                activity::touch(c, uid, &cid, id, activity::Mode::Study)?;
            }
            let p = mastery::load_progress(c, uid, &cid)?;
            let r = mastery::recently_correct(c, uid, &cid, 24 * 3600)?;
            Ok((p, r))
        })
        .await?;
    let avoid: HashSet<String> = recent.into_iter().collect();

    let mut rng = Rng::from_entropy();
    // Recommended practice is for retaining what you have read: only lessons
    // whose reading check was passed take part. A single lesson can always be
    // practised on request.
    let read_only = only.is_none();
    let picked = build_quiz(content, &progress, only, read_only, p.start, &avoid, params.n.unwrap_or(DEFAULT_QUIZ_LEN), at, true, &mut rng);

    let questions: Vec<QuestionView> =
        picked.into_iter().map(|pk| QuestionView::new(content, pk.lesson, pk.item, &mut rng)).collect();

    let (heading, subheading) = match only {
        Some(l) => (l.title.clone(), format!("{} · {}", content.unit(l.unit_index).title, content.group_title(&l.group))),
        None => (
            crate::t("quiz.page.recommended_title"),
            crate::t("quiz.page.recommended_subtitle"),
        ),
    };
    let next_lesson = if read_only && questions.is_empty() {
        next_unread(content, &progress, p.start, at).map(|l| NextLesson { slug: l.slug.clone(), title: l.title.clone() })
    } else {
        None
    };

    Ok(Page(QuizTemplate {
        course: CourseView::of(content),
        user: Some(p.user),
        heading,
        subheading,
        lesson_slug: only.map(|l| l.slug.clone()),
        has_tiles: questions.iter().any(|q| q.board.is_some()),
        questions,
        next_lesson,
    })
    .into_response())
}

#[derive(Deserialize)]
pub struct CheckAnswer {
    pub question_id: String,
    pub answer: String,
}

#[derive(Deserialize)]
pub struct LessonCheckRequest {
    /// The lesson's slug.
    pub lesson: String,
    pub answers: Vec<CheckAnswer>,
}

#[derive(Serialize, Debug)]
pub struct LessonCheckResponse {
    /// One verdict per answer, in the order they were sent.
    pub results: Vec<AnswerResponse>,
    /// Every answer was right: the lesson now counts as read.
    pub passed: bool,
    pub read: bool,
    /// The worn motivator's line for the check, as HTML (#568).
    pub reward: Option<String>,
}

/// POST /api/lesson-check  { "lesson": "slug", "answers": [{question_id, answer}, ...] }
///
/// The reading check at the bottom of a lesson: two of its questions (one if
/// the lesson has only one), answered together. Each answer is graded and
/// recorded exactly like a practice answer, so mastery and history move; if
/// all are right the lesson is marked read and joins recommended practice.
pub async fn lesson_check(
    State(state): State<Arc<AppState>>,
    p: Project,
    headers: axum::http::HeaderMap,
    Json(req): Json<LessonCheckRequest>,
) -> Result<Json<LessonCheckResponse>, AppError> {
    let since = crate::db::now();
    let tz = super::sprint::tz_offset_secs(&headers);
    let content = &p.course;
    let lesson = content.lesson_by_slug(&req.lesson).ok_or_else(|| AppError::ApiBadRequest(crate::t("quiz.api.unknown_lesson")))?;
    let needed = check_len(lesson);
    if needed == 0 {
        return Err(AppError::ApiBadRequest(crate::t("quiz.api.no_questions")));
    }
    if req.answers.len() != needed {
        return Err(AppError::ApiBadRequest(crate::t1("quiz.api.check_needs", "n", needed)));
    }
    let ids: HashSet<&str> = req.answers.iter().map(|a| a.question_id.as_str()).collect();
    if ids.len() != needed || !ids.iter().all(|id| lesson.quiz.iter().any(|q| q.question_id == *id)) {
        return Err(AppError::ApiBadRequest(crate::t("quiz.api.distinct_questions")));
    }

    let mut results = Vec::with_capacity(needed);
    for a in &req.answers {
        results.push(grade_and_record(&state, &p, &a.question_id, &a.answer, activity::Kind::Answer).await?);
    }
    let passed = results.iter().all(|r| r.correct);
    let read = if passed {
        let uid = p.user.id;
        let cid = content.id().to_string();
        let lid = lesson.id.clone();
        let course = Arc::clone(&p.course);
        // Passing the check is Study mode's "seen" (progress.rs), which also sets read_at.
        state
            .db
            .call(move |c| {
                crate::progress::mark_seen(c, uid, &cid, course.lesson_by_id(&lid).expect("lesson exists"), crate::progress::VIA_STUDY)?;
                activity::record(c, uid, &cid, activity::Kind::Reading, 1, tz)?;
                mastery::load_one(c, uid, &cid, &lid)
            })
            .await?
            .is_some_and(|p| p.is_read())
    } else {
        state
            .db
            .call({
                let uid = p.user.id;
                let cid = content.id().to_string();
                let lid = lesson.id.clone();
                move |c| mastery::load_one(c, uid, &cid, &lid)
            })
            .await?
            .map(|p| p.is_read())
            .unwrap_or(false)
    };
    let reward = state
        .db
        .call({
            let (uid, course, lid) = (p.user.id, Arc::clone(&p.course), lesson.id.clone());
            move |c| crate::motivators::reward(c, uid, &course, tz, since, Some(&lid))
        })
        .await?;
    Ok(Json(LessonCheckResponse { results, passed, read, reward: crate::motivators::reward_html(reward) }))
}

#[derive(Deserialize)]
pub struct AnswerRequest {
    pub question_id: String,
    pub answer: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct AnswerResponse {
    pub correct: bool,
    pub note: Option<String>,
    pub correct_answer: String,
    /// A keyboard question's right key (an ABC note), for the page to light up.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub key: String,
    pub explanation: String,
    pub lesson_id: String,
    pub lesson_slug: String,
    pub lesson_title: String,
    /// Effective mastery (0-100) for the lesson before and after this answer.
    pub mastery_before: u32,
    pub mastery: u32,
    pub streak: i64,
    /// Vocabulary collected for the first time with this (correct) answer.
    pub new_words: Vec<NewWord>,
}

#[derive(Serialize, Clone, Debug)]
pub struct NewWord {
    pub term: String,
    pub meaning: String,
}

/// POST /api/answer  { "question_id": "...", "answer": "..." }
pub async fn answer(
    State(state): State<Arc<AppState>>,
    p: Project,
    Json(req): Json<AnswerRequest>,
) -> Result<Json<AnswerResponse>, AppError> {
    super::limit_learning_write(&state, p.user.id)?;
    Ok(Json(grade_and_record(&state, &p, &req.question_id, &req.answer, activity::Kind::Answer).await?))
}

/// Grade one answer and record it: mastery, history, collected vocabulary
/// and the daily goal (`kind`: a tutor's question weighs more). Every mode
/// that asks quiz questions (Practice, the Guide, the AI tutor, agents) goes
/// through here, so progress counts the same.
pub async fn grade_and_record(state: &AppState, p: &Project, question_id: &str, answer: &str, kind: activity::Kind) -> Result<AnswerResponse, AppError> {
    let content = &p.course;
    let (lesson, item) = content.question(question_id).ok_or_else(|| AppError::ApiBadRequest(crate::t("quiz.api.unknown_question")))?;
    if answer.chars().count() > 200 {
        return Err(AppError::ApiBadRequest(crate::t("quiz.api.answer_too_long")));
    }

    let verdict = check_answer(item, answer);
    let uid = p.user.id;
    let tz = p.user.tz;
    let cid = content.id().to_string();
    let lesson_id = lesson.id.clone();
    let qid = item.question_id.clone();
    let selected = answer.trim().to_string();
    let correct = verdict.correct;
    let at = now();
    let taught: Vec<String> = if correct {
        content.words_taught(lesson, item).into_iter().map(|w| w.term.clone()).collect()
    } else {
        Vec::new()
    };
    let (before, progress, new_words) = state
        .db
        .call(move |c| {
            let before = mastery::load_one(c, uid, &cid, &lesson_id)?.map(|p| p.effective(at)).unwrap_or(0.0);
            let (after, new_words) = mastery::record_answer_with(c, uid, &cid, &lesson_id, &qid, &selected, correct, |tx| {
                activity::record(tx, uid, &cid, kind, 1, tz)?;
                let words: Vec<&str> = taught.iter().map(String::as_str).collect();
                vocab::record_learned(tx, uid, &cid, &lesson_id, &words, tz)
            })?;
            Ok((before, after, new_words))
        })
        .await?;
    let new_words = new_words
        .into_iter()
        .filter_map(|term| {
            lesson.vocabulary.iter().find(|v| v.term == term).map(|v| NewWord { term: v.term.clone(), meaning: v.meaning.clone() })
        })
        .collect();

    Ok(AnswerResponse {
        correct,
        note: verdict.note,
        correct_answer: item.correct_answer.clone(),
        key: crate::notation::keyboard::key(item),
        explanation: item.explanation.clone(),
        lesson_id: lesson.id.clone(),
        lesson_slug: lesson.slug.clone(),
        lesson_title: lesson.title.clone(),
        mastery_before: before.round() as u32,
        mastery: progress.effective(at).round() as u32,
        streak: progress.streak,
        new_words,
    })
}
