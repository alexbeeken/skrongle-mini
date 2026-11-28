//! The course home (`docs/features-learner-path.md`, "The course home"): Up
//! next, Then, Done today, the Progress panel and the hidden activities,
//! from `path::list`. `pages::root` sends a signed-in learner here.

use std::collections::HashMap;
use std::sync::Arc;

use askama::Template;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};

use super::{CourseView, Page};
use crate::activity::{self, Today};
use crate::auth::{CurrentUser, Project};
use crate::content::ContentStore;
use crate::db::now;
use crate::error::AppError;
use crate::guide::StageProgress;
use crate::mastery::{self, Status};
use crate::path::{self, Item, Kind, SETTINGS};
use crate::progress::{self, LessonPass};
use crate::quiz::current_unit;
use crate::AppState;

/// How many activities "Then" lists after Up next.
const THEN: usize = 3;

/// One activity as the page draws it.
pub struct ItemView {
    pub lesson_id: String,
    /// `study`, `review` or `play`: the form value, the tile's class.
    pub kind: &'static str,
    /// The kind's name and the course's icon for it.
    pub name: String,
    pub icon: String,
    pub title: String,
    /// The line under the title; '' for none.
    pub line: String,
    /// Where starting it goes.
    pub url: String,
}

#[derive(Template)]
#[template(path = "home.html")]
pub struct HomeTemplate {
    pub user: Option<CurrentUser>,
    pub course: Arc<CourseView>,
    /// The worn motivator's id and its Progress panel, drawn by `Hero::render`
    /// (`templates/motivators/<id>.html`) from the shared measures.
    pub motivator: &'static str,
    pub panel: String,
    pub next: Option<ItemView>,
    pub then: Vec<ItemView>,
    pub done_today: Vec<ItemView>,
    pub hidden: Vec<ItemView>,
    /// Every lesson is passed: the empty list says so.
    pub all_passed: bool,
    /// Words due today (#1360), and how many one round reviews
    /// (`vocab::DUE_CAP` at most). No line when nothing is due.
    pub due_count: i64,
    pub due_round: i64,
    /// The course's first visit (#574): its settings as a wizard over the
    /// page, until the learner gets started or closes it.
    pub settings: Option<super::courses::CourseSettings>,
}

/// The kind's name, and the course icon of the mode it opens.
fn name_and_icon(course: &CourseView, kind: Kind) -> (String, String) {
    let icon = match kind {
        Kind::Study => &course.icons.mode_guide,
        Kind::Review => &course.icons.mode_sprint,
        Kind::Play => &course.icons.mode_play,
    };
    (crate::t(&format!("path.kind.{}", kind.as_str())), icon.clone())
}

/// Whole days in a gap, for the page: a part day counts as one, as the score does.
fn days(gap: f64) -> i64 {
    gap.ceil() as i64
}

/// The line under an activity: a Study's steps, a returning Review's clean
/// streak in whole clean reviews, a Play's game.
fn line(content: &ContentStore, item: &Item, stages: &HashMap<String, StageProgress>) -> String {
    match item.kind {
        Kind::Study => {
            let total = content.lesson_by_id(&item.lesson_id).map_or(0, |l| l.guide_steps.len());
            match stages.get(&item.lesson_id).map(|s| s.step) {
                Some(step) if step > 0 && (step as usize) < total => crate::t2("path.step_of", "n", step + 1, "total", total),
                _ => crate::tn("path.steps", total),
            }
        }
        Kind::Review if !item.tried => String::new(),
        Kind::Review if path::mastered(item.streak, &SETTINGS) => crate::tn("path.mastered_check", days(item.gap)),
        Kind::Review => {
            crate::t2("path.clean_reviews", "n", path::clean_attempts(item.streak, &SETTINGS), "total", SETTINGS.clean_to_master)
        }
        Kind::Play => crate::t("play.matching.name"),
    }
}

/// Where starting an activity goes: a Study opens the lesson's steps, a
/// Review a session on the lesson, a Play Word Match on the lesson's terms.
fn url(content: &ContentStore, item: &Item) -> String {
    let slug = content.lesson_by_id(&item.lesson_id).map_or("", |l| l.slug.as_str());
    match item.kind {
        Kind::Study => activity::Mode::Guide.continue_url(slug),
        Kind::Review => activity::Mode::Sprint.continue_url(slug),
        Kind::Play => crate::url(&format!("/play/matching/play?lesson={slug}")),
    }
}

fn view(course: &CourseView, item: &Item, line: String, url: String) -> ItemView {
    let (name, icon) = name_and_icon(course, item.kind);
    ItemView { lesson_id: item.lesson_id.clone(), kind: item.kind.as_str(), name, icon, title: item.title.clone(), line, url }
}

/// GET `/` for a signed-in learner in a course.
pub async fn home(State(state): State<Arc<AppState>>, p: Project, headers: HeaderMap) -> Result<Response, AppError> {
    let at = now();
    let content = &p.course;
    let uid = p.user.id;
    let cid = content.id().to_string();
    let tz = super::sprint::tz_offset_secs(&headers);
    let day_start = crate::sprint::local_day_start(tz, at);
    let (store, start) = (p.course.clone(), p.start);
    let (progress, (streak, practised_today), today, words_learned, stages, passes, touched, (motivator, hero), project, list) = state
        .db
        .call(move |c| {
            let motivator = crate::motivators::worn(c, uid, &cid)?;
            let hero = motivator.load(c, uid, &store, tz)?;
            let project = crate::courses::project(c, uid, &cid)?;
            let progress = mastery::load_progress(c, uid, &cid)?;
            let streak = crate::sprint::day_streak(c, uid, &cid, tz, at)?;
            let today = Today::load(c, uid, &cid, day_start)?;
            let words = crate::vocab::count(c, uid, &cid)?;
            let stages = crate::guide::load_all(c, uid, &cid)?;
            let passes = progress::load_all(c, uid, &cid)?;
            let touched = activity::last_touched(c, uid, &cid)?;
            let list = path::list(c, uid, &store, start, at, tz)?;
            Ok((progress, streak, today, words, stages, passes, touched, (motivator.id(), hero), project, list))
        })
        .await?;
    let resume = activity::resume(content, &passes, &touched, p.start);
    let mut skipped_count = 0;
    // The unit shown is the one of the lesson to resume at; with every lesson
    // passed, the one mastery says the learner is working on.
    let current = resume.as_ref().map_or_else(|| current_unit(content, &progress, p.start, at), |r| r.lesson.unit_index);
    let (mut unit_passed, mut unit_total, mut passed_count) = (0, 0, 0);
    for lesson in content.lessons() {
        let skipped = matches!(Status::of_lesson(progress.get(&lesson.id), at, lesson, p.start), Status::Skipped);
        skipped_count += usize::from(skipped);
        let passed = passes.get(&lesson.id).is_some_and(LessonPass::passed);
        passed_count += usize::from(passed);
        if lesson.unit_index == current && !skipped {
            unit_total += 1;
            unit_passed += usize::from(passed);
        }
    }
    let total_lessons = content.lessons().len() - skipped_count;
    let measures = crate::motivators::Progress {
        today,
        practised_today,
        streak,
        effort_table: activity::table(),
        passed_count,
        total_lessons,
        skipped_count,
        words_learned,
        current_unit: content.unit(current).title.clone(),
        unit_number: current + 1,
        unit_count: content.units().len(),
        unit_passed,
        unit_total,
        unit_percent: (unit_passed * 100).checked_div(unit_total).unwrap_or(0),
    };
    let (due_count, _) = super::vocab::due_cards(&state, &p, 0).await?;
    let course = content.view();
    let mut next = list.next.iter().map(|i| view(&course, i, line(content, i, &stages), url(content, i)));
    let first = next.next();
    let then = next.take(THEN).collect();
    let done_today = list
        .done_today
        .iter()
        .map(|i| {
            let back = if i.kind.repeats() { crate::tn("path.back_in", days(i.gap)) } else { String::new() };
            view(&course, i, back, url(content, i))
        })
        .collect();
    let hidden = list.hidden.iter().map(|i| view(&course, i, String::new(), url(content, i))).collect();
    let settings = project.filter(|pr| !pr.set_up).map(|pr| {
        let mut st = super::courses::settings_view(&pr, content, true);
        st.for_account(&p.user);
        st
    });
    Ok(Page(HomeTemplate {
        settings,
        motivator,
        panel: hero.render(&course, &measures)?,
        course,
        user: Some(p.user),
        next: first,
        then,
        done_today,
        hidden,
        all_passed: total_lessons > 0 && passed_count >= total_lessons,
        due_count,
        due_round: due_count.min(crate::vocab::DUE_CAP as i64),
    })
    .into_response())
}
