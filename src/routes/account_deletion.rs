//! `/account/deletion`: the one page an account scheduled for deletion can
//! use (#970, `account_deletion.rs`). The `CurrentUser` extractor sends
//! every other page here; it says when the account goes, with *Cancel
//! deletion* and *Log out*.

use askama::Template;
use axum::extract::State;
use axum::response::{IntoResponse, Redirect, Response};
use std::sync::Arc;

use super::Page;
use crate::auth::{CurrentUser, MaybeUser};
use crate::error::AppError;
use crate::AppState;

#[derive(Template)]
#[template(path = "account_deletion.html")]
pub struct DeletionTemplate {
    pub user: Option<CurrentUser>,
    /// The day it is deleted, in words.
    pub date: String,
}

/// GET /account/deletion
pub async fn page(MaybeUser(user): MaybeUser) -> Result<Response, AppError> {
    let Some(user) = user else { return Err(AppError::RedirectToLogin) };
    let Some(at) = user.delete_at else { return Ok(Redirect::to(&crate::url("/")).into_response()) };
    Ok(Page(DeletionTemplate { user: Some(user), date: crate::profile::long_date(at) }).into_response())
}

/// POST /account/deletion/cancel: keep the account. Only a session started
/// after the deletion was asked for gets here (asking ended every session),
/// so it took the full sign-in: password or Google, and the second step.
pub async fn cancel(State(state): State<Arc<AppState>>, MaybeUser(user): MaybeUser) -> Result<Response, AppError> {
    let Some(user) = user else { return Err(AppError::RedirectToLogin) };
    let uid = user.id;
    if state.db.call(move |c| crate::account_deletion::cancel(c, uid)).await? {
        tracing::info!("user {uid} cancelled the deletion of their account");
    }
    Ok(Redirect::to(&crate::url("/")).into_response())
}
