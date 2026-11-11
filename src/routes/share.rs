//! Sharing a course with other learners (#451, #16, `src/share.rs`): the
//! Share dialog on the owner's course page (make an invitation link, named
//! or not, for the owner to pass on; take one away), and that link.

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect, Response};
use axum::Form;
use serde::Deserialize;
use std::sync::Arc;

use crate::auth::{CurrentUser, MaybeUser};
use crate::db::now;
use crate::error::AppError;
use crate::perms::Perm;
use crate::urlenc::urlencode;
use crate::share;
use crate::AppState;

#[derive(Deserialize)]
pub struct InviteForm {
    /// A name for the link (who it is for), or nothing.
    #[serde(default)]
    pub label: String,
}

#[derive(Deserialize)]
pub struct RemoveForm {
    /// The link's row (`course-share-row`'s `data-id`).
    pub id: i64,
}

/// Back to the course page with the Share dialog open and a word in it: a
/// notice key the editor turns into text (`editor_notice`, #385).
fn back(id: &str, key: &str) -> Response {
    Redirect::to(&crate::url(&format!("/courses/{id}?share=1&notice={key}"))).into_response()
}

/// GET /courses/{id}/share: the address the links page was drawn
/// at, opened again (from the address bar, say). Back to the Share dialog (#667).
pub async fn to_course(Path(id): Path<String>) -> Response {
    Redirect::to(&crate::url(&format!("/courses/{id}?share=1"))).into_response()
}

/// POST /courses/{id}/share (label): one new invitation link, shown once.
/// Needs the Share courses permission (#144); removing a link and opening
/// one do not.
pub async fn invite(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    headers: HeaderMap,
    Path(id): Path<String>,
    Form(form): Form<InviteForm>,
) -> Result<Response, AppError> {
    user.require(Perm::ShareCourses)?;
    super::courses::owned(&state, &user, &id).await?;
    let label = share::clean_label(&form.label);
    let cid = id.clone();
    let existing = state.db.call(move |c| share::shares_of(c, &cid)).await?.len();
    if existing >= share::MAX_PER_COURSE {
        return Ok(back(&id, "share-full"));
    }
    let base = crate::routes::public_base(&state, &headers);
    let (cid, l) = (id.clone(), label.clone());
    let token = state.db.call(move |c| share::invite(c, &cid, &l, now())).await?;
    tracing::info!("user {} made an invitation link for course {id}", user.id);
    // The owner passes the link on: nothing is mailed. It is stored only
    // hashed, so the page shows it this once.
    let links = vec![super::courses::ShareLink { label, link: format!("{base}/courses/shared/{token}") }];
    super::courses::share_links_page(&state, user, &id, "share-made", links).await
}

/// POST /courses/{id}/share/remove (id): the link stops working, and the
/// course leaves the list of whoever opened it.
pub async fn remove(State(state): State<Arc<AppState>>, user: CurrentUser, Path(id): Path<String>, Form(form): Form<RemoveForm>) -> Result<Response, AppError> {
    super::courses::owned(&state, &user, &id).await?;
    let cid = id.clone();
    let removed = state.db.call(move |c| share::remove(c, &cid, form.id)).await?;
    if removed {
        tracing::info!("user {} removed an invitation link of course {id}", user.id);
    }
    Ok(back(&id, if removed { "share-removed" } else { "share-not-on-list" }))
}

/// GET /courses/shared/{token}: the link in the invitation. Signed in, the
/// course joins the learner's list and opens; signed out, the login page
/// says what the link is for and comes back here afterwards. A link that no
/// longer works (the owner removed it) says so on the login page and on
/// the course list (#1082).
pub async fn open_link(State(state): State<Arc<AppState>>, MaybeUser(user): MaybeUser, Path(token): Path<String>) -> Result<Response, AppError> {
    let token = token.trim().to_string();
    let t = token.clone();
    let share = state.db.call(move |c| share::by_token(c, &t)).await?;
    let Some(user) = user else {
        return Ok(Redirect::to(&crate::url(&format!("/login?share={}", urlencode(&token)))).into_response());
    };
    let Some(share) = share else {
        return Ok(Redirect::to(&crate::url("/courses?notice=share-gone")).into_response());
    };
    let uid = user.id;
    let s = share.clone();
    match state.db.call(move |c| share::claim(c, &s, uid, now())).await? {
        share::Claim::Ok => {}
        share::Claim::Taken => {
            return Ok(Redirect::to(&crate::url("/courses?notice=share-taken")).into_response());
        }
        // The owner's own link: their course page, with the Share dialog open.
        share::Claim::Owner => return Ok(Redirect::to(&crate::url(&format!("/courses/{}?share=1", share.course_id))).into_response()),
    }
    tracing::info!("user {} opened a share link for course {}", user.id, share.course_id);
    let broken = "/courses?notice=share-not-ready".to_string();
    super::courses::open_for(&state, &user, &share.course_id, &broken, "/").await
}

#[cfg(test)]
mod tests;
