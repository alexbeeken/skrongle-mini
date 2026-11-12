//! Manage Users' Connected apps card (#167): the owner's switch **Let apps
//! connect** (`settings::APPS_ON`) and every account's grants, each with
//! Revoke. Owners only; a superuser does not see it, and its routes answer
//! 404 to anyone else.

use axum::extract::State;
use axum::response::{IntoResponse, Redirect, Response};
use axum::Form;
use serde::Deserialize;
use std::sync::Arc;

use crate::auth::api;
use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::AppState;

/// One grant, as the owner's list shows it.
pub struct OwnerGrantRow {
    pub id: i64,
    pub name: String,
    /// The account's address.
    pub email: String,
    pub scopes: Vec<String>,
    /// When it was last used, or "never used".
    pub last_used: String,
    pub expires: String,
}

/// Every account's grants in force, for the owner.
pub fn rows(grants: Vec<api::Grant>, now: i64) -> Vec<OwnerGrantRow> {
    grants
        .into_iter()
        .map(|g| OwnerGrantRow {
            id: g.id,
            scopes: g.scopes.iter().map(|s| s.label()).collect(),
            last_used: g.last_used_at.map_or_else(|| crate::t("apps.never_used"), |t| crate::routes::time_ago(t, now)),
            expires: crate::profile::long_date(g.expires_at),
            email: g.email,
            name: g.client_name,
        })
        .collect()
}

#[derive(Deserialize)]
pub struct AppsForm {
    /// `1` lets apps connect, `0` stops them.
    #[serde(default)]
    pub on: String,
}

/// POST /manage/apps (on=1|0): let apps connect, or not. Off, every API
/// route answers 404 and every token stops working at its next request;
/// the grants stay, so turning it on again brings them back.
pub async fn set_apps(State(state): State<Arc<AppState>>, user: CurrentUser, Form(form): Form<AppsForm>) -> Result<Response, AppError> {
    if !user.owner {
        return Err(AppError::NotFound);
    }
    let on = match form.on.as_str() {
        "1" => true,
        "0" => false,
        _ => return Err(AppError::BadRequest("on must be 1 or 0".into())),
    };
    state.db.call(move |c| crate::settings::set_apps_on(c, on)).await?;
    tracing::info!("owner {} {} apps to connect", user.id, if on { "allowed" } else { "stopped" });
    let did = if on { "apps-on" } else { "apps-off" };
    Ok(Redirect::to(&crate::url(&format!("/manage?did={did}#apps"))).into_response())
}

#[derive(Deserialize)]
pub struct RevokeForm {
    #[serde(default)]
    pub id: i64,
}

/// POST /manage/apps/revoke (id): any account's grant goes, with its
/// tokens. Owners only.
pub async fn revoke(State(state): State<Arc<AppState>>, user: CurrentUser, Form(form): Form<RevokeForm>) -> Result<Response, AppError> {
    if !user.owner {
        return Err(AppError::NotFound);
    }
    let id = form.id;
    if state.db.call(move |c| api::revoke_any(c, id)).await? {
        tracing::info!("owner {} revoked connected app {id}", user.id);
    }
    Ok(Redirect::to(&crate::url("/manage?did=app-revoked#apps")).into_response())
}
