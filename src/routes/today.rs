//! Today's work for a student (#223, docs/features.md "Daily work"): the
//! student's `/` in a subject is the day's assigned plan as a numbered list
//! with one button, in the student frame. Each activity is a stop
//! (`/today/stop/{id}`); earlier assigned days with work left are under
//! `/today/earlier` and `/today/day/{day}`.
//!
//! Every rule (which stop is next, when one may be started or finished, what
//! is left) is in `crate::daily`; these handlers ask it and draw the answer.
//!
//! The stops: [`show_stop`] and [`act_on_stop`] are the one place that picks
//! a stop's page by its kind. A Study stop is `today/study.rs`; a Review or
//! Play stop is, until #224 builds them, a single button (`today/simple.rs`).

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Redirect, Response};
use axum::Form;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;

use super::Page;
use crate::auth::{CurrentUser, Project};
use crate::content::ContentStore;
use crate::daily::{self, Kind, Plan, Stop, StopState};
use crate::error::AppError;
use crate::AppState;

mod simple;
mod study;

/// One row of the day's list.
pub struct Row {
    /// 1, 2, 3 …
    pub num: usize,
    /// Study, Review or Play.
    pub kind: String,
    /// The lessons' titles, joined with " · ".
    pub lessons: String,
    /// For a play: its games and how many times through, else empty.
    pub extra: String,
    /// Done, Next, or "After Review".
    pub word: String,
    /// `done`, `next` or `locked` (the row's class and `data-state`).
    pub state: &'static str,
}

/// The day's list as the page draws it ([`day_view`]). Built from a
/// student's stops only, so a teacher's preview (#225) can draw it for any
/// student, or for an empty day.
pub struct DayView {
    /// "Today's work", or "Work for Thursday 19 November 2026".
    pub heading: String,
    /// The date (today only), how many are done and days in a row.
    pub line: String,
    pub rows: Vec<Row>,
    /// "Done: Study, Lesson a." after finishing that stop.
    pub notice: Option<String>,
    /// The one main button: its label and where it goes. None when nothing
    /// is assigned or everything is done.
    pub button: Option<(String, String)>,
    /// Nothing assigned for the day.
    pub nothing: bool,
    /// Every activity done.
    pub all_done: bool,
    /// "Earlier days · 3 left" and its link, when earlier days have work left.
    pub earlier: Option<(String, String)>,
    /// The page is an earlier day: it links back to today.
    pub past: bool,
}

/// The date of a class-clock day: "Thursday 19 November 2026".
pub fn date_of(day: i64) -> String {
    let (_, _, _, weekday) = daily::civil(day);
    let key = ["sunday", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday"][weekday as usize];
    crate::i18n::fill(
        &crate::t("student.today.date"),
        &[("weekday", &crate::t(&format!("student.today.weekday.{key}"))), ("date", &crate::profile::long_date(day * 86_400))],
    )
}

/// A kind's name on the page.
pub fn kind_label(kind: Kind) -> String {
    crate::t(match kind {
        Kind::Study => "student.today.kind.study",
        Kind::Review => "student.today.kind.review",
        Kind::Play => "student.today.kind.play",
    })
}

/// The lessons' titles, in the activity's order, joined with " · " (a
/// lesson no longer in the course is left out).
pub fn lesson_titles(course: &ContentStore, lessons: &[String]) -> String {
    lessons.iter().filter_map(|id| course.lesson_by_id(id)).map(|l| l.title.as_str()).collect::<Vec<_>>().join(" · ")
}

/// A play's games and times through: "Word Match, Flashcards · 2 times through".
fn play_extra(a: &daily::Activity) -> String {
    let games: Vec<String> = a
        .games
        .iter()
        .map(|g| if g == "matching" { crate::t("student.today.game.matching") } else { crate::t("student.today.game.flashcards") })
        .collect();
    let mut parts = Vec::new();
    if !games.is_empty() {
        parts.push(games.join(", "));
    }
    if a.rounds > 0 {
        parts.push(crate::tn("student.today.times", a.rounds));
    }
    parts.join(" · ")
}

/// Where a stop's page is.
pub fn stop_url(activity_id: i64) -> String {
    format!("/today/stop/{activity_id}")
}

/// The list of `day` for a student whose stops on it are `stops` (empty:
/// nothing assigned). `today` is the class clock's day, `streak` the days
/// in a row (shown on today only), `done` the activity just finished, and
/// `earlier_left` what is left on earlier days.
pub fn day_view(course: &ContentStore, stops: &[Stop], day: i64, today: i64, streak: i64, done: Option<i64>, earlier_left: i64) -> DayView {
    let past = day < today;
    let rows: Vec<Row> = stops
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let (word, state) = match s.state {
                StopState::Done => (crate::t("student.today.word.done"), "done"),
                StopState::Next => (crate::t("student.today.word.next"), "next"),
                StopState::Locked => {
                    let before = i.checked_sub(1).map(|j| kind_label(stops[j].activity.kind)).unwrap_or_default();
                    (crate::t1("student.today.word.after", "kind", before), "locked")
                }
            };
            let extra = if s.activity.kind == Kind::Play { play_extra(&s.activity) } else { String::new() };
            Row { num: i + 1, kind: kind_label(s.activity.kind), lessons: lesson_titles(course, &s.activity.lessons), extra, word, state }
        })
        .collect();
    let finished = stops.iter().filter(|s| s.state == StopState::Done).count();
    let next = stops.iter().find(|s| s.state == StopState::Next);
    let button = next.map(|s| {
        let label = if s.started {
            "student.today.keep_going"
        } else if finished > 0 {
            "student.today.next_activity"
        } else {
            "student.today.get_started"
        };
        (crate::t(label), stop_url(s.activity.id))
    });
    let mut line = Vec::new();
    if !past {
        line.push(date_of(day));
    }
    if !stops.is_empty() {
        line.push(crate::t2("student.today.done_of", "done", finished, "total", stops.len()));
    }
    if !past && streak > 0 {
        line.push(crate::tn("student.today.streak", streak));
    }
    let notice = done.and_then(|id| stops.iter().find(|s| s.activity.id == id && s.state == StopState::Done)).map(|s| {
        crate::t2("student.today.done_notice", "kind", kind_label(s.activity.kind), "lessons", lesson_titles(course, &s.activity.lessons))
    });
    DayView {
        heading: if past { crate::t1("student.today.day_title", "date", date_of(day)) } else { crate::t("student.today.title") },
        line: line.join(" · "),
        notice,
        button,
        nothing: stops.is_empty(),
        all_done: !stops.is_empty() && next.is_none(),
        earlier: (!past && earlier_left > 0).then(|| (crate::tn("student.today.earlier_link", earlier_left), "/today/earlier".to_string())),
        past,
        rows,
    }
}

#[derive(Template)]
#[template(path = "student_today.html")]
pub struct TodayTemplate {
    pub user: Option<CurrentUser>,
    /// The top bar's title: the subject's.
    pub subject: String,
    pub view: DayView,
}

#[derive(Deserialize, Default)]
pub struct DoneQuery {
    /// The activity just finished.
    pub done: Option<i64>,
}

/// What a day's list needs from the database: the class clock's today,
/// the student's stops on `day` (today when `None`), days in a row and
/// what is left on earlier days.
async fn load_day(state: &AppState, p: &Project, day: Option<i64>) -> Result<(i64, i64, Vec<Stop>, i64, i64), AppError> {
    let (uid, cid, tz) = (p.user.id, p.course_id().to_string(), p.user.tz);
    let out = state
        .db
        .call(move |c| {
            let now = crate::db::now();
            let today = daily::today(c, now)?;
            let day = day.unwrap_or(today);
            let stops = daily::stops(c, uid, &cid, day)?;
            let (streak, _) = crate::sprint::day_streak(c, uid, &cid, tz, now)?;
            let earlier = daily::unfinished_before(c, uid, &cid, today)?;
            Ok((today, day, stops, streak, earlier))
        })
        .await?;
    Ok(out)
}

/// `/` for a student in a subject (from `pages::root`): today's work.
pub async fn home(state: &AppState, p: Project, q: DoneQuery) -> Result<Response, AppError> {
    let (today, day, stops, streak, earlier) = load_day(state, &p, None).await?;
    let view = day_view(&p.course, &stops, day, today, streak, q.done, earlier);
    Ok(Page(TodayTemplate { subject: p.course.title().to_string(), user: Some(p.user), view }).into_response())
}

fn student_only(p: &Project) -> Result<(), AppError> {
    if p.user.student {
        Ok(())
    } else {
        Err(AppError::NotFound)
    }
}

/// GET /today/day/{day}: an earlier assigned day's list, the same as
/// today's. Today, a later day or a day with nothing assigned goes to `/`.
pub async fn day(State(state): State<Arc<AppState>>, p: Project, Path(day): Path<i64>, Query(q): Query<DoneQuery>) -> Result<Response, AppError> {
    student_only(&p)?;
    let (today, day, stops, streak, earlier) = load_day(&state, &p, Some(day)).await?;
    if day >= today || stops.is_empty() {
        return Ok(Redirect::to(&crate::url("/")).into_response());
    }
    let view = day_view(&p.course, &stops, day, today, streak, q.done, earlier);
    Ok(Page(TodayTemplate { subject: p.course.title().to_string(), user: Some(p.user), view }).into_response())
}

/// One earlier assigned day as `/today/earlier` lists it.
pub struct EarlierRow {
    pub day: i64,
    pub date: String,
    /// "3 left", or "Done".
    pub left: String,
    pub done: bool,
}

#[derive(Template)]
#[template(path = "student_earlier.html")]
pub struct EarlierTemplate {
    pub user: Option<CurrentUser>,
    pub subject: String,
    pub days: Vec<EarlierRow>,
}

/// GET /today/earlier: every earlier assigned day, newest first, with what
/// is left of it; each opens its list.
pub async fn earlier(State(state): State<Arc<AppState>>, p: Project) -> Result<Response, AppError> {
    student_only(&p)?;
    let (uid, cid) = (p.user.id, p.course_id().to_string());
    let list = state
        .db
        .call(move |c| {
            let today = daily::today(c, crate::db::now())?;
            daily::earlier_days(c, uid, &cid, today)
        })
        .await?;
    let days = list
        .into_iter()
        .map(|d| EarlierRow {
            day: d.day,
            date: date_of(d.day),
            left: if d.left > 0 { crate::tn("student.today.left", d.left) } else { crate::t("student.today.word.done") },
            done: d.left == 0,
        })
        .collect();
    Ok(Page(EarlierTemplate { subject: p.course.title().to_string(), user: Some(p.user), days }).into_response())
}

/// A stop the student may open now, as every stop page receives it: the
/// plan it is in, the student's stop (started), and the class clock's day.
pub struct StopCtx {
    pub plan: Plan,
    pub stop: Stop,
    pub today: i64,
}

impl StopCtx {
    /// The activity's id.
    pub fn id(&self) -> i64 {
        self.stop.activity.id
    }

    /// The list the stop belongs to: `/` for today, else its day's page.
    pub fn list_url(&self) -> String {
        if self.plan.day >= self.today {
            "/".to_string()
        } else {
            format!("/today/day/{}", self.plan.day)
        }
    }

    /// Back to the list after finishing, which then says this stop is done.
    pub fn done_url(&self) -> String {
        format!("{}?done={}", self.list_url(), self.id())
    }
}

/// The stop `id`, when the student may open it now: in an assigned plan of
/// the active subject for today or earlier, and the next one of that day
/// (a started stop not finished is always the next one). Starts it.
async fn open_stop(state: &AppState, p: &Project, id: i64) -> Result<Option<StopCtx>, AppError> {
    let (uid, cid) = (p.user.id, p.course_id().to_string());
    let ctx = state
        .db
        .call(move |c| {
            let now = crate::db::now();
            let today = daily::today(c, now)?;
            let Some(plan) = daily::plan_of_activity(c, id)? else { return Ok(None) };
            if plan.course_id != cid || !plan.assigned || plan.day > today {
                return Ok(None);
            }
            let Some(mut stop) = daily::stops(c, uid, &cid, plan.day)?.into_iter().find(|s| s.activity.id == id && s.state == StopState::Next) else {
                return Ok(None);
            };
            if !daily::start(c, uid, &cid, plan.day, id, now)? {
                return Ok(None);
            }
            stop.started = true;
            Ok(Some(StopCtx { plan, stop, today }))
        })
        .await?;
    Ok(ctx)
}

/// Finishes the stop (`daily::finish`, which checks its kind's work is
/// recorded) and goes back to the list, which says it is done; refused, to
/// the list as it is.
pub(crate) async fn finish(state: &AppState, p: &Project, ctx: &StopCtx) -> Result<Response, AppError> {
    let (uid, cid, day, id) = (p.user.id, p.course_id().to_string(), ctx.plan.day, ctx.id());
    let done = state.db.call(move |c| daily::finish(c, uid, &cid, day, id, crate::db::now())).await?;
    let to = if done { ctx.done_url() } else { ctx.list_url() };
    Ok(Redirect::to(&crate::url(&to)).into_response())
}

/// The kind dispatch for a stop's page (#224 swaps the Review and Play arms).
async fn show_stop(state: &AppState, p: Project, ctx: StopCtx) -> Result<Response, AppError> {
    match ctx.stop.activity.kind {
        Kind::Study => study::show(state, p, ctx, Vec::new()).await,
        Kind::Review | Kind::Play => simple::show(p, ctx),
    }
}

/// The kind dispatch for a stop's form post, with the posted fields.
async fn act_on_stop(state: &AppState, p: Project, ctx: StopCtx, form: HashMap<String, String>) -> Result<Response, AppError> {
    match ctx.stop.activity.kind {
        Kind::Study => study::post(state, p, ctx, form).await,
        Kind::Review | Kind::Play => simple::post(state, p, ctx).await,
    }
}

/// GET /today/stop/{id}: the stop's page, when the student may open it
/// ([`open_stop`]); anything else goes back to the list.
pub async fn stop(State(state): State<Arc<AppState>>, p: Project, Path(id): Path<i64>) -> Result<Response, AppError> {
    student_only(&p)?;
    match open_stop(&state, &p, id).await? {
        Some(ctx) => show_stop(&state, p, ctx).await,
        None => Ok(Redirect::to(&crate::url("/")).into_response()),
    }
}

/// POST /today/stop/{id}: the stop's form (answers, "I've read it", the
/// one button of a Review or Play stop).
pub async fn stop_post(
    State(state): State<Arc<AppState>>,
    p: Project,
    Path(id): Path<i64>,
    Form(form): Form<HashMap<String, String>>,
) -> Result<Response, AppError> {
    student_only(&p)?;
    match open_stop(&state, &p, id).await? {
        Some(ctx) => act_on_stop(&state, p, ctx, form).await,
        None => Ok(Redirect::to(&crate::url("/")).into_response()),
    }
}

#[cfg(test)]
mod tests;
