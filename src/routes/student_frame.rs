//! The student frame (#219): the light layout a student account's pages
//! are served in (`templates/student_base.html`, `static/student.css`),
//! and its first page of its own, **Change my password** (`/password`).
//!
//! A page in the frame has no script, one stylesheet, no font and no
//! picture but the course's own, so it works with JavaScript off on a
//! classroom's old computer. `FRAME_PAGES` lists every route served in it;
//! the test walks the list and checks each page for those rules
//! (`tests::assert_light`). A page moved into the frame adds its path there.

use askama::Template;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use axum::Form;
use serde::Deserialize;
use std::sync::Arc;

use super::Page;
use crate::auth::{self, CurrentUser, MaybeUser, SessionToken};
use crate::error::AppError;
use crate::AppState;

/// Every route served in the student frame, by app path. The test signs
/// in a student and opens each (`/login` signed out), and fails when one
/// has a script, a second stylesheet, a font or a picture. A later page in
/// the frame adds its path here.
#[cfg_attr(not(test), allow(dead_code))]
pub const FRAME_PAGES: &[&str] = &["/login", "/password"];

#[derive(Template)]
#[template(path = "student_password.html")]
pub struct PasswordTemplate {
    pub user: Option<CurrentUser>,
    pub error: Option<String>,
    /// The password was just changed (`?saved=1`).
    pub saved: bool,
}

#[derive(Deserialize)]
pub struct SavedQuery {
    pub saved: Option<String>,
}

#[derive(Deserialize)]
pub struct PasswordForm {
    #[serde(default)]
    pub current: String,
    #[serde(default)]
    pub new: String,
    #[serde(default)]
    pub confirm: String,
}

/// Where a normal account changes its password: Profile settings.
fn to_profile() -> Response {
    Redirect::to(&crate::url("/profile#account")).into_response()
}

/// GET /password: **Change my password** for a student; a normal account
/// goes to its Profile settings, where its password is.
pub async fn password_form(MaybeUser(user): MaybeUser, Query(q): Query<SavedQuery>) -> Result<Response, AppError> {
    let Some(user) = user else { return Err(AppError::RedirectToLogin) };
    if !user.student {
        return Ok(to_profile());
    }
    let saved = q.saved.is_some();
    Ok(Page(PasswordTemplate { user: Some(user), error: None, saved }).into_response())
}

/// POST /password (current, new, confirm): a student changes its own
/// password. The current one is checked as on Profile settings, on the
/// sign-in's per-account counter; every other session of the student ends.
/// Done, it comes back to the page with a notice; refused, to the form
/// with the reason.
pub async fn change_password(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    SessionToken(token): SessionToken,
    Form(form): Form<PasswordForm>,
) -> Result<Response, AppError> {
    if !user.student {
        return Ok(to_profile());
    }
    let fail = |user: CurrentUser, status: StatusCode, msg: String| (status, Page(PasswordTemplate { user: Some(user), error: Some(msg), saved: false })).into_response();
    if form.new != form.confirm {
        return Ok(fail(user, StatusCode::OK, crate::t("profile.password.mismatch")));
    }
    if let Err(e) = auth::validate_password(&form.new) {
        return Ok(fail(user, StatusCode::OK, e));
    }
    if let Some((status, why)) = super::profile::prove_owner(&state, &user, &form.current, "").await? {
        // A wrong password is the form again (200); only the limit is a 429.
        let (status, msg) = if status == StatusCode::FORBIDDEN { (StatusCode::OK, crate::t("profile.password.current_wrong")) } else { (status, why) };
        return Ok(fail(user, status, msg));
    }
    let uid = user.id;
    let hash = auth::hash_password_async(form.new).await?;
    let keep = token.unwrap_or_default();
    let ended = state
        .db
        .call(move |c| {
            crate::profile::set_password_hash(c, uid, &hash)?;
            auth::delete_other_sessions(c, uid, &keep)
        })
        .await?;
    tracing::info!("student {uid} changed their password ({ended} other sessions signed out)");
    Ok(Redirect::to(&crate::url("/password?saved=1")).into_response())
}

#[cfg(test)]
pub(crate) mod tests;
