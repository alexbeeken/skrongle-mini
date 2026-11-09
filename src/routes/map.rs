//! Learning map: every lesson of a unit in order, with your mastery, and how
//! far you are from mastering the unit.

use askama::Template;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use std::sync::Arc;

use super::{CourseView, Page};
use crate::auth::{CurrentUser, Project};
use crate::db::now;
use crate::error::AppError;
use crate::mastery::{self, Status};
use crate::AppState;

pub struct MapNode {
    pub number: usize,
    pub slug: String,
    pub title: String,
    pub group: String,
    pub score: u32,
    pub status_label: String,
    pub status_css: &'static str,
    /// Passed in any mode, and the pass status in words.
    pub passed: bool,
    pub pass: String,
    pub is_here: bool,
}

pub struct UnitTrack {
    pub id: String,
    pub unit: String,
    pub description: String,
    pub total: usize,
    pub mastered: usize,
    pub in_progress: usize,
    pub weak: usize,
    pub untested: usize,
    /// Below the level the learner started at, never studied.
    pub skipped: usize,
    pub percent: u32,
    pub achieved: bool,
    /// The number of the course's "you are here" lesson, when it is in this
    /// unit: the first lesson on the path that is neither mastered nor
    /// skipped. Progress is course-wide, so only one unit has it.
    pub here: Option<usize>,
    pub nodes: Vec<MapNode>,
}

#[derive(Template)]
#[template(path = "map.html")]
pub struct MapTemplate {
    pub user: Option<CurrentUser>,
    pub course: std::sync::Arc<CourseView>,
    pub tracks: Vec<UnitTrack>,
    pub current_unit: String,
}

/// GET /map
pub async fn map(State(state): State<Arc<AppState>>, p: Project) -> Result<Response, AppError> {
    let at = now();
    let uid = p.user.id;
    let content = &p.course;
    let cid = content.id().to_string();
    let (progress, passes, touched) = state
        .db
        .call(move |c| {
            Ok((mastery::load_progress(c, uid, &cid)?, crate::progress::load_all(c, uid, &cid)?, crate::activity::last_touched(c, uid, &cid)?))
        })
        .await?;
    // "You are here" is the lesson every mode resumes at (`activity::resume`).
    let resume = crate::activity::resume(content, &passes, &touched, p.start).map(|r| r.lesson);
    let current = resume.map_or_else(|| crate::quiz::current_unit(content, &progress, p.start, at), |l| l.unit_index);

    let mut tracks = Vec::new();
    for (ui, unit) in content.units().iter().enumerate() {
        let lessons: Vec<_> = content.lessons_in(ui).collect();
        if lessons.is_empty() {
            continue;
        }
        let mut nodes = Vec::new();
        let (mut mastered, mut in_progress, mut weak, mut untested, mut skipped) = (0, 0, 0, 0, 0);
        let mut here = None;
        for (i, l) in lessons.iter().enumerate() {
            let pr = progress.get(&l.id);
            let status = Status::of_lesson(pr, at, l, p.start);
            match status {
                Status::Mastered => mastered += 1,
                Status::InProgress => in_progress += 1,
                Status::NeedsPractice => weak += 1,
                Status::Untested => untested += 1,
                Status::Skipped => skipped += 1,
            }
            let is_here = resume.is_some_and(|r| r.id == l.id);
            if is_here {
                here = Some(i + 1);
            }
            nodes.push(MapNode {
                number: i + 1,
                slug: l.slug.clone(),
                title: l.title.clone(),
                group: content.group_title(&l.group).to_string(),
                score: pr.map(|p| p.effective(at).round() as u32).unwrap_or(0),
                status_label: status.label(),
                status_css: status.css(),
                passed: passes.get(&l.id).is_some_and(|p| p.passed()),
                pass: passes.get(&l.id).map(|p| p.summary()).unwrap_or_default(),
                is_here,
            });
        }
        let total = lessons.len();
        tracks.push(UnitTrack {
            id: unit.id.to_lowercase(),
            unit: unit.title.clone(),
            description: unit.description.clone(),
            total,
            mastered,
            in_progress,
            weak,
            untested,
            skipped,
            percent: ((mastered as f64 / total as f64) * 100.0).round() as u32,
            achieved: mastered == total,
            here,
            nodes,
        });
    }
    Ok(Page(MapTemplate {
        course: content.view(),
        user: Some(p.user),
        tracks,
        current_unit: content.unit(current).title.clone(),
    })
    .into_response())
}
