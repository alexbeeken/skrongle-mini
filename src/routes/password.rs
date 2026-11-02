//! `/password/change`: the gate an account passes after signing in with a
//! temporary password the owner set (#635, `owner.rs`). The `CurrentUser`
//! extractor sends it here (`AppError::PasswordChangeRequired`) until it
//! chooses a new password; the old one is not asked for: it was just typed.

use askama::Template;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Redirect, Response};
use axum::Form;
use serde::Deserialize;
use std::sync::Arc;

use super::Page;
use crate::auth::{self, CurrentUser, MaybeUser, SessionToken};
use crate::error::AppError;
use crate::AppState;

#[derive(Template)]
#[template(path = "password_change.html")]
pub struct ChangeTemplate {
    pub user: Option<CurrentUser>,
    pub next: String,
    pub error: Option<String>,
}

#[derive(Deserialize)]
pub struct NextQuery {
    pub next: Option<String>,
}

/// Only app-relative paths, and never this page itself.
fn safe_next(next: Option<&str>) -> String {
    match next {
        Some(n) if super::terms::is_app_path(n) && !n.starts_with("/password/change") => n.to_string(),
        _ => "/".to_string(),
    }
}

fn form(user: CurrentUser, next: String, error: Option<String>) -> Response {
    Page(ChangeTemplate { user: Some(user), next, error }).into_response()
}

/// GET /password/change?next=/somewhere
pub async fn change_form(MaybeUser(user): MaybeUser, Query(q): Query<NextQuery>) -> Result<Response, AppError> {
    let next = safe_next(q.next.as_deref());
    let Some(user) = user else { return Err(AppError::RedirectToLogin) };
    if !user.must_change_password {
        return Ok(Redirect::to(&crate::url(&next)).into_response());
    }
    Ok(form(user, next, None))
}

#[derive(Deserialize)]
pub struct ChangeForm {
    #[serde(default)]
    pub new: String,
    #[serde(default)]
    pub confirm: String,
    #[serde(default)]
    pub next: String,
}

/// POST /password/change (new, confirm, next)
pub async fn change(
    State(state): State<Arc<AppState>>,
    MaybeUser(user): MaybeUser,
    SessionToken(token): SessionToken,
    Form(f): Form<ChangeForm>,
) -> Result<Response, AppError> {
    let next = safe_next(Some(&f.next));
    let Some(user) = user else { return Err(AppError::RedirectToLogin) };
    if !user.must_change_password {
        return Ok(Redirect::to(&crate::url(&next)).into_response());
    }
    if f.new != f.confirm {
        return Ok(form(user, next, Some(crate::t("profile.password.mismatch"))));
    }
    if let Err(e) = auth::validate_password(&f.new) {
        return Ok(form(user, next, Some(e)));
    }
    let new = f.new;
    let hash = auth::hash_password_async(new).await?;
    let uid = user.id;
    let keep = token.unwrap_or_default();
    state
        .db
        .call(move |c| {
            crate::profile::set_password_hash(c, uid, &hash)?;
            auth::delete_other_sessions(c, uid, &keep)
        })
        .await?;
    tracing::info!("user {uid} replaced a temporary password");
    Ok(Redirect::to(&crate::url(&next)).into_response())
}

#[cfg(test)]
mod tests {
    use super::safe_next;

    #[test]
    fn next_stays_in_the_app() {
        assert_eq!(safe_next(Some("/profile")), "/profile");
        assert_eq!(safe_next(Some("//evil.example")), "/");
        assert_eq!(safe_next(Some("/password/change?next=/x")), "/");
        assert_eq!(safe_next(None), "/");
    }
}
