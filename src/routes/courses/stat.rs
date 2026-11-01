//! The body of a course card's stat modal (#900): fetched when a chip's
//! modal opens, so `/courses` itself stays as cheap as two counts per card.
//! It says what is next, not a full report: for "passed" the next lesson to
//! pass and each unit's count, for "mastered" the lessons to practise next.

use super::*;
use crate::mastery::{self, MASTERED_AT};

/// How many lessons the "practise next" list names at most.
const PRACTISE_MAX: usize = 3;

#[derive(Template)]
#[template(path = "course_stat.html")]
pub struct StatTemplate {
    pub course_id: String,
    pub metric: &'static str,
    /// "passed": the first lesson in course order not passed yet.
    pub next: Option<StatLesson>,
    /// "passed": the last lesson passed, and how long ago.
    pub last: Option<(StatLesson, String)>,
    /// "passed": each unit with lessons, as (title, passed, total).
    pub units: Vec<(String, usize, usize)>,
    /// "mastered": lessons practised but not mastered now, fading ones first.
    pub practise: Vec<StatLesson>,
}

pub struct StatLesson {
    pub slug: String,
    pub title: String,
    /// Effective mastery now, 0-100.
    pub score: u32,
    /// Mastered once (stored score at 80 or more) and below it now.
    pub fading: bool,
}

/// GET /courses/{id}/stat/{metric}: the detail of one chip's modal, as an
/// HTML fragment. 404 for a course the learner has no project in.
pub async fn stat_detail(State(state): State<Arc<AppState>>, user: CurrentUser, Path((id, metric)): Path<(String, String)>) -> Result<Response, AppError> {
    let metric = match metric.as_str() {
        "passed" => "passed",
        "mastered" => "mastered",
        _ => return Err(AppError::NotFound),
    };
    if !accessible(&state, &user, &id).await? {
        return Err(AppError::NotFound);
    }
    let Some(content) = state.courses.get(&state.db, &id).await? else { return Err(AppError::NotFound) };
    let (uid, cid) = (user.id, id.clone());
    let Some((project, passes, progress)) = state
        .db
        .call(move |c| {
            let Some(p) = courses::project(c, uid, &cid)? else { return Ok(None) };
            Ok(Some((p, crate::progress::load_all(c, uid, &cid)?, mastery::load_progress(c, uid, &cid)?)))
        })
        .await?
    else {
        return Err(AppError::NotFound);
    };
    let at = now();
    let start = content.start_index(&project.start_unit);
    let lesson = |l: &crate::content::Lesson| {
        let p = progress.get(&l.id);
        StatLesson {
            slug: l.slug.clone(),
            title: l.title.clone(),
            score: p.map_or(0, |p| p.effective(at).round() as u32),
            fading: p.is_some_and(|p| p.mastery_score >= MASTERED_AT && p.effective(at) < MASTERED_AT),
        }
    };
    let passed = |l: &crate::content::Lesson| passes.get(&l.id).is_some_and(|p| p.passed());
    let mut t = StatTemplate {
        course_id: id.clone(),
        metric,
        next: None,
        last: None,
        units: Vec::new(),
        practise: Vec::new(),
    };
    if metric == "passed" {
        t.next = content
            .lessons()
            .iter()
            // Below the level they started at and never touched: not theirs to pass.
            .find(|l| !passed(l) && !(crate::quiz::is_skipped(progress.get(&l.id), l, start) && !passes.contains_key(&l.id)))
            .map(lesson);
        t.last = content
            .lessons()
            .iter()
            .filter_map(|l| passes.get(&l.id).and_then(|p| p.passed_at).map(|when| (when, l)))
            .max_by_key(|(when, _)| *when)
            .map(|(when, l)| (lesson(l), time_ago(when, at)));
        t.units = (0..content.units().len())
            .filter_map(|u| {
                let total = content.lessons_in(u).count();
                (total > 0).then(|| (content.unit(u).title.clone(), content.lessons_in(u).filter(|l| passed(l)).count(), total))
            })
            .collect();
    } else {
        let mut open: Vec<StatLesson> = content
            .lessons()
            .iter()
            .filter(|l| progress.get(&l.id).is_some_and(|p| p.times_tested > 0 && p.effective(at) < MASTERED_AT))
            .map(lesson)
            .collect();
        // Fading ones first, then the closest to mastered.
        open.sort_by(|a, b| b.fading.cmp(&a.fading).then(b.score.cmp(&a.score)));
        open.truncate(PRACTISE_MAX);
        t.practise = open;
    }
    Ok(axum::response::Html(t.render()?).into_response())
}
