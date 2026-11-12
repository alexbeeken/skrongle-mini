//! The profile's Connected apps section (#167, `auth::api`): each grant the
//! account gave an app, with Revoke, and Make a token. Shown only while the
//! owner lets apps connect and the account has Create courses
//! (`CurrentUser::connects_apps`); otherwise both routes answer 404.

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::Form;
use std::sync::Arc;

use super::{profile_with, ProfileTemplate};
use crate::auth::api::{self, Lifetime, NewGrant, Scope};
use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::AppState;

/// One connected app, as the section lists it.
pub struct GrantRow {
    pub id: i64,
    pub name: String,
    /// The scopes in plain words.
    pub scopes: Vec<String>,
    /// The courses it may reach, by title; "all your courses" without a list.
    pub courses: String,
    pub created: String,
    pub expires: String,
    /// When it was last used, or "never used".
    pub last_used: String,
    pub used: bool,
}

/// A choice in Make a token's lists.
pub struct Choice {
    pub id: String,
    pub label: String,
    pub checked: bool,
}

/// What the section shows.
pub struct Apps {
    pub grants: Vec<GrantRow>,
    /// The account has a password to confirm with: Make a token is offered.
    pub can_make: bool,
    /// The account's own courses, for the optional course list.
    pub courses: Vec<Choice>,
    pub lifetimes: Vec<Choice>,
    pub scopes: Vec<Choice>,
    /// A token just made: shown this once.
    pub new_token: Option<String>,
    /// Why Make a token refused; its dialog opens again with it.
    pub error: Option<String>,
    /// `caps::MAX_GRANTS_PER_ACCOUNT`, for the note under the list.
    pub max: usize,
}

/// The section for `user`, or `None` when it is not shown.
pub async fn load(state: &AppState, user: &CurrentUser) -> Result<Option<Apps>, AppError> {
    if !user.connects_apps() {
        return Ok(None);
    }
    let uid = user.id;
    let now = crate::db::now();
    let (grants, owned, has_password) = state
        .db
        .call(move |c| {
            let grants = api::grants(c, uid, now)?;
            let owned = crate::courses::courses_owned_by(c, uid)?;
            let has_password = !crate::profile::password_hash(c, uid)?.unwrap_or_default().is_empty();
            Ok((grants, owned, has_password))
        })
        .await?;
    let title = |id: &str| owned.iter().find(|c| c.id == id).map(|c| c.title.clone());
    let grants = grants
        .into_iter()
        .map(|g| GrantRow {
            id: g.id,
            scopes: g.scopes.iter().map(|s| s.label()).collect(),
            courses: if g.courses.is_empty() {
                crate::t("apps.all_courses")
            } else {
                g.courses.iter().filter_map(|id| title(id)).collect::<Vec<_>>().join(", ")
            },
            created: crate::profile::long_date(g.created_at),
            expires: crate::profile::long_date(g.expires_at),
            last_used: g.last_used_at.map_or_else(|| crate::t("apps.never_used"), |t| crate::routes::time_ago(t, now)),
            used: g.last_used_at.is_some(),
            name: g.client_name,
        })
        .collect();
    Ok(Some(Apps {
        grants,
        can_make: has_password,
        courses: owned.iter().map(|c| Choice { id: c.id.clone(), label: c.title.clone(), checked: false }).collect(),
        lifetimes: Lifetime::ALL.into_iter().map(|l| Choice { id: l.id().into(), label: l.label(), checked: l == Lifetime::DEFAULT }).collect(),
        scopes: Scope::ALL.into_iter().map(|s| Choice { id: s.as_str().into(), label: s.label(), checked: true }).collect(),
        new_token: None,
        error: None,
        max: crate::caps::MAX_GRANTS_PER_ACCOUNT,
    }))
}

/// A token's name as typed: trimmed, 1 to `NAME_MAX` characters, no
/// control, direction or invisible characters (`api::is_hidden_char`: the
/// owner reads this name on Manage Users).
pub fn validate_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(crate::t("apps.name_missing"));
    }
    if name.chars().count() > api::NAME_MAX {
        return Err(crate::t1("apps.name_too_long", "max", api::NAME_MAX));
    }
    if name.chars().any(api::is_hidden_char) {
        return Err(crate::t("apps.name_hidden"));
    }
    Ok(name.to_string())
}

/// The profile again with Make a token's dialog open and why it refused.
async fn refuse(state: &AppState, user: CurrentUser, status: StatusCode, why: String) -> Result<Response, AppError> {
    profile_with(state, user, status, move |t: &mut ProfileTemplate| {
        if let Some(a) = t.apps.as_mut() {
            a.error = Some(why);
        }
    })
    .await
}

/// POST /profile/apps/token (name, scope=…, course=…, lifetime, password):
/// Make a token. The account's password in the same form (`prove_owner`,
/// counted by `PASSWORD_PER_USER`); an account without one is not offered
/// this. Made, the profile shows the token this once (`no-store`).
pub async fn make_token(State(state): State<Arc<AppState>>, user: CurrentUser, Form(form): Form<Vec<(String, String)>>) -> Result<Response, AppError> {
    if !user.connects_apps() {
        return Err(AppError::NotFound);
    }
    let field = |name: &str| form.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str()).unwrap_or_default();
    let uid = user.id;
    let has_password = !state.db.call(move |c| crate::profile::password_hash(c, uid)).await?.unwrap_or_default().is_empty();
    if !has_password {
        return Err(AppError::NotFound);
    }
    let name = match validate_name(field("name")) {
        Ok(n) => n,
        Err(e) => return refuse(&state, user, StatusCode::UNPROCESSABLE_ENTITY, e).await,
    };
    let scopes: Vec<Scope> = Scope::ALL.into_iter().filter(|s| form.iter().any(|(k, v)| k == "scope" && v == s.as_str())).collect();
    if scopes.is_empty() {
        return refuse(&state, user, StatusCode::UNPROCESSABLE_ENTITY, api::Refused::NoScope.message()).await;
    }
    let Some(lifetime) = Lifetime::parse(field("lifetime")) else {
        return Err(AppError::BadRequest("lifetime is 30, 90 or 365".into()));
    };
    if let Some((status, _)) = super::prove_owner(&state, &user, field("password"), "").await? {
        let why = if status == StatusCode::FORBIDDEN { crate::t("profile.two_factor.password_wrong") } else { crate::t("profile.two_factor.too_many_attempts") };
        return refuse(&state, user, status, why).await;
    }
    let asked: Vec<String> = form.iter().filter(|(k, _)| k == "course").map(|(_, v)| v.clone()).collect();
    let now = crate::db::now();
    let made = state
        .db
        .call(move |c| {
            // Only the account's own courses can be on the list.
            let owned = crate::courses::courses_owned_by(c, uid)?;
            let courses = match api::own_courses(asked, &owned) {
                Ok(c) => c,
                Err(e) => return Ok(Err(e)),
            };
            let tx = c.transaction()?;
            let g = NewGrant { client_id: api::PERSONAL_CLIENT_ID, client_name: &name, scopes: &scopes, courses: &courses, lifetime };
            let made = api::make_personal(&tx, uid, &g, now)?;
            if made.is_ok() {
                tx.commit()?;
            }
            Ok(made)
        })
        .await?;
    let token = match made {
        Ok(t) => t,
        Err(e) => return refuse(&state, user, StatusCode::CONFLICT, e.message()).await,
    };
    // The log names the account, never the token.
    tracing::info!("user {uid} made a personal API token");
    state.issued.count(now);
    let page = profile_with(&state, user, StatusCode::OK, move |t: &mut ProfileTemplate| {
        if let Some(a) = t.apps.as_mut() {
            a.new_token = Some(token);
        }
    })
    .await?;
    // The token is a key to the account: not to be cached anywhere.
    Ok(([(header::CACHE_CONTROL, "no-store")], page).into_response())
}

#[derive(serde::Deserialize)]
pub struct RevokeForm {
    #[serde(default)]
    pub id: i64,
}

/// POST /profile/apps/revoke (id): the grant and its tokens go; the app's
/// next request gets a 401. Only the account's own grant.
pub async fn revoke(State(state): State<Arc<AppState>>, user: CurrentUser, Form(form): Form<RevokeForm>) -> Result<Response, AppError> {
    if !user.connects_apps() {
        return Err(AppError::NotFound);
    }
    let (uid, id) = (user.id, form.id);
    if state.db.call(move |c| api::revoke(c, uid, id)).await? {
        tracing::info!("user {uid} revoked connected app {id}");
    }
    Ok(Redirect::to(&crate::url("/profile?saved=app-revoked#apps")).into_response())
}
