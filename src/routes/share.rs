//! Sharing a course with other learners (#451, `src/share.rs`): the Share
//! dialog on the owner's course page (invite by e-mail address, stop
//! sharing), and the link the invitation carries.

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect, Response};
use axum::Form;
use serde::Deserialize;
use std::sync::Arc;

use crate::auth::{CurrentUser, MaybeUser};
use crate::db::now;
use crate::error::AppError;
use crate::oauth::urlencode;
use crate::share;
use crate::AppState;

#[derive(Deserialize)]
pub struct InviteForm {
    /// Addresses, separated by commas, spaces or new lines.
    #[serde(default)]
    pub emails: String,
}

#[derive(Deserialize)]
pub struct RemoveForm {
    pub email: String,
}

/// Back to the course page with the Share dialog open and a word in it: a
/// notice key the editor turns into text (`editor_notice`, #385).
fn back(id: &str, key: &str) -> Response {
    Redirect::to(&crate::url(&format!("/courses/{id}?share=1&notice={key}"))).into_response()
}

/// GET /courses/{id}/share: the address the no-mail links page was drawn
/// at, opened again (from the address bar, say). Back to the Share dialog (#667).
pub async fn to_course(Path(id): Path<String>) -> Response {
    Redirect::to(&crate::url(&format!("/courses/{id}?share=1"))).into_response()
}

/// POST /courses/{id}/share (emails): one invitation per new address.
pub async fn invite(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    headers: HeaderMap,
    Path(id): Path<String>,
    Form(form): Form<InviteForm>,
) -> Result<Response, AppError> {
    let row = super::courses::owned(&state, &user, &id).await?;
    // The AI content check (#1036) scans what changed before the course
    // reaches anyone else; a few seconds, nothing when nothing changed. It
    // comes first so a fixed course loses its AI flag (#1037). A part that
    // scored high goes to the AI review at once, on the jobs thread.
    if let Some(Ok(r)) = crate::content_check::scan_and_wait(state.db.clone(), id.clone()).await {
        if r.needs_review {
            state.queue.notify();
        }
    }
    // A flagged course (#1033) is not shared; the check runs again if the files changed.
    let cid = id.clone();
    let flagged = state
        .db
        .call(move |c| {
            let files = crate::courses::load_files(c, &cid)?;
            Ok(crate::course_flags::check(c, &cid, &files)?.is_some_and(|f| f.blocks()))
        })
        .await?;
    if flagged {
        return Ok(back(&id, "share-flagged"));
    }
    let emails = match share::parse_addresses(&form.emails) {
        Ok(e) => e,
        // The notice names the problem; the typed address stays out of the URL.
        Err(e) if e == crate::t("courses.share.at_least_one") => return Ok(back(&id, "share-none")),
        Err(e) if e == crate::t1("courses.share.too_many_request", "max", share::MAX_PER_REQUEST) => return Ok(back(&id, "share-many")),
        Err(_) => return Ok(back(&id, "share-bad")),
    };
    if !user.email.is_empty() && emails.iter().any(|e| e.eq_ignore_ascii_case(&user.email)) {
        return Ok(back(&id, "share-own"));
    }
    let cid = id.clone();
    let (existing, owner) = state.db.call(move |c| Ok((share::shares_of(c, &cid)?.len(), share::owner_of(c, &cid)?))).await?;
    if existing + emails.len() > share::MAX_PER_COURSE {
        return Ok(back(&id, "share-full"));
    }
    let owner = owner.unwrap_or_default();
    let base = crate::verify::public_base(&state, &headers);
    let can_mail = state.caps().mail;
    let (mut sent, mut already) = (0, 0);
    let mut links = Vec::new();
    for email in emails {
        let (cid, e) = (id.clone(), email.clone());
        let Some(token) = state.db.call(move |c| share::invite(c, &cid, &e, now())).await? else {
            already += 1;
            continue;
        };
        let link = format!("{base}/courses/shared/{token}");
        // No mail on this server: the owner passes the link on (#633).
        if !can_mail {
            links.push(super::courses::ShareLink { email, link });
            sent += 1;
            continue;
        }
        let mail = share::message(&email, &owner, &row.title, &link, user.id).map_err(AppError::Internal)?;
        let mid = state.mail.queue(&state.db, mail).await?;
        if state.dev {
            tracing::info!("dev: share link for course {id} (mail #{mid}): {link}");
        }
        sent += 1;
    }
    tracing::info!("user {} shared course {id} with {sent} address(es)", user.id);
    if !can_mail {
        // The links are stored only hashed: the page shows them this once.
        return super::courses::share_links_page(&state, user, &id, &format!("share-made-{sent}-{already}"), links).await;
    }
    Ok(back(&id, &format!("share-sent-{sent}-{already}")))
}

/// POST /courses/{id}/share/remove (email): the course leaves their list.
pub async fn remove(State(state): State<Arc<AppState>>, user: CurrentUser, Path(id): Path<String>, Form(form): Form<RemoveForm>) -> Result<Response, AppError> {
    super::courses::owned(&state, &user, &id).await?;
    let email = form.email.trim().to_lowercase();
    let (cid, e) = (id.clone(), email.clone());
    let removed = state.db.call(move |c| share::remove(c, &cid, &e)).await?;
    if removed {
        tracing::info!("user {} stopped sharing course {id} with one address", user.id);
    }
    Ok(back(&id, if removed { "share-removed" } else { "share-not-on-list" }))
}

/// GET /courses/shared/{token}: the link in the invitation. Signed in, the
/// course joins the learner's list and opens; signed out, the login page
/// says what the link is for and comes back here afterwards. A link that no
/// longer works (the owner removed the address, or invited it again with a
/// new link) says so on the login page and on the course list (#1082).
pub async fn open_link(State(state): State<Arc<AppState>>, MaybeUser(user): MaybeUser, Path(token): Path<String>) -> Result<Response, AppError> {
    let token = token.trim().to_string();
    let t = token.clone();
    let share = state.db.call(move |c| share::by_token(c, &t)).await?;
    let Some(user) = user else {
        return Ok(Redirect::to(&crate::url(&format!("/login?share={}", urlencode(&token)))).into_response());
    };
    // Waiting to be deleted (#970): nothing but the page that cancels it.
    if user.delete_at.is_some() {
        return Err(AppError::DeletionScheduled);
    }
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
