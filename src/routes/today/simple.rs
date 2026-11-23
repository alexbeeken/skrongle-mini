//! A Review or Play stop until #224 builds them: the stop's kind and
//! lessons and a single button that finishes it, so a day can be walked
//! end to end. A review is marked reviewed (`daily::mark_reviewed`); a play
//! that asks for rounds counts one per click (`daily::add_play`) and says
//! how many times are left. #224 replaces this file's two arms in
//! `today::show_stop` and `today::act_on_stop`.

use askama::Template;
use axum::response::{IntoResponse, Redirect, Response};

use super::{kind_label, lesson_titles, StopCtx};
use crate::auth::{CurrentUser, Project};
use crate::daily::{self, Kind};
use crate::error::AppError;
use crate::routes::Page;
use crate::AppState;

#[derive(Template)]
#[template(path = "student_stop.html")]
pub struct StopTemplate {
    pub user: Option<CurrentUser>,
    pub subject: String,
    pub kind: String,
    /// `review` or `play` (`data-kind`).
    pub kind_key: &'static str,
    pub lessons: String,
    /// A play that asks for rounds: how many times are left (else 0).
    pub left: i64,
    pub action: String,
    pub back: String,
}

/// The stop's page: what it is and its one button.
pub fn show(p: Project, ctx: StopCtx) -> Result<Response, AppError> {
    let a = &ctx.stop.activity;
    let left = if a.kind == Kind::Play && daily::play_limited(&ctx.plan, a) { (a.rounds - ctx.stop.plays).max(0) } else { 0 };
    Ok(Page(StopTemplate {
        subject: p.course.title().to_string(),
        kind: kind_label(a.kind),
        kind_key: a.kind.as_str(),
        lessons: lesson_titles(&p.course, &a.lessons),
        left,
        action: crate::url(&super::stop_url(a.id)),
        back: crate::url(&ctx.list_url()),
        user: Some(p.user),
    })
    .into_response())
}

/// The button: a review is marked and finished; a play counts one time
/// through and is finished once it may be (`daily::may_finish`), else its
/// page again with one time fewer to go.
pub async fn post(state: &AppState, p: Project, ctx: StopCtx) -> Result<Response, AppError> {
    let (uid, id) = (p.user.id, ctx.id());
    match ctx.stop.activity.kind {
        Kind::Review => {
            state.db.call(move |c| daily::mark_reviewed(c, uid, id)).await?;
        }
        Kind::Play => {
            let plays = state.db.call(move |c| daily::add_play(c, uid, id)).await?;
            let mut stop = ctx.stop.clone();
            stop.plays = plays;
            if !daily::may_finish(&ctx.plan, &stop) {
                return Ok(Redirect::to(&crate::url(&super::stop_url(id))).into_response());
            }
        }
        Kind::Study => {}
    }
    super::finish(state, &p, &ctx).await
}
