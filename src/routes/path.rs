//! `POST /path/hide` and `POST /path/unhide` {lesson, kind}: the signed-in
//! learner hides one activity of their active course from their list, or
//! brings it back (`path::hide`, `path::unhide`). Both go to `/`.

use std::sync::Arc;

use axum::extract::State;
use axum::response::Redirect;
use axum::Form;
use serde::Deserialize;

use crate::auth::Project;
use crate::error::AppError;
use crate::path::Kind;
use crate::AppState;

#[derive(Deserialize)]
pub struct Activity {
    /// The lesson's id.
    #[serde(default)]
    pub lesson: String,
    /// `study`, `review` or `play`.
    #[serde(default)]
    pub kind: String,
}

/// The kind, for a lesson of the course; 400 for an unknown kind, 404 for a lesson the learner can't see.
fn check(p: &Project, form: &Activity) -> Result<Kind, AppError> {
    let kind = Kind::parse(&form.kind).ok_or_else(|| AppError::BadRequest("kind must be study, review or play".into()))?;
    p.course.lesson_by_id(&form.lesson).ok_or(AppError::NotFound)?;
    Ok(kind)
}

pub async fn hide(State(state): State<Arc<AppState>>, p: Project, Form(form): Form<Activity>) -> Result<Redirect, AppError> {
    let kind = check(&p, &form)?;
    let (uid, cid) = (p.user.id, p.course_id().to_string());
    state.db.call(move |c| crate::path::hide(c, uid, &cid, &form.lesson, kind, crate::db::now())).await?;
    Ok(Redirect::to(&crate::url("/")))
}

pub async fn unhide(State(state): State<Arc<AppState>>, p: Project, Form(form): Form<Activity>) -> Result<Redirect, AppError> {
    let kind = check(&p, &form)?;
    let (uid, start, course) = (p.user.id, p.start, p.course.clone());
    state.db.call(move |c| crate::path::unhide(c, uid, &course, start, &form.lesson, kind, crate::db::now())).await?;
    Ok(Redirect::to(&crate::url("/")))
}
