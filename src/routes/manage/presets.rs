//! Saved presets on Manage Users (#140): a Custom set saved under a name
//! from the Permissions dialog (`save_as`, the Custom branch of
//! `POST /manage/permissions`), the list of saved presets under the accounts
//! and `POST /manage/presets/delete`. `perms.rs` decides the names and the
//! limit (`save_preset`, `MAX_SAVED_PRESETS`). A saved preset is not edited:
//! save a new one and delete the old.

use axum::extract::State;
use axum::response::{IntoResponse, Redirect, Response};
use axum::Form;
use serde::Deserialize;
use std::sync::Arc;

use super::{done, ManageTemplate};
use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::perms::{self, Perm, Perms, Preset, PresetError};
use crate::signup::Account;
use crate::AppState;

/// A saved preset in the list under the accounts.
pub struct SavedPreset {
    pub id: i64,
    pub name: String,
    /// The names of the permissions it has, in the dialog's order.
    pub perms: Vec<String>,
    /// How many accounts have it (owners show as Owner, so not them).
    pub accounts: usize,
}

/// The saved presets (not Student and Teacher) for the page, from what
/// `load` has read already.
pub fn saved(presets: &[Preset], accounts: &[Account]) -> Vec<SavedPreset> {
    presets
        .iter()
        .filter(|p| !p.builtin)
        .map(|p| SavedPreset {
            id: p.id,
            name: p.name.clone(),
            perms: Perm::ALL.into_iter().filter(|x| p.perms.has(*x)).map(Perm::label).collect(),
            accounts: accounts.iter().filter(|a| !a.owner && a.preset_id == Some(p.id)).count(),
        })
        .collect()
}

/// What `GET /manage?did=preset-…` says, back from one of the actions here.
pub fn notice(t: &mut ManageTemplate, did: &str) {
    match did {
        "preset-name-taken" => t.error = Some(crate::t("manage.preset_name_taken")),
        "preset-too-many" => t.error = Some(crate::t1("manage.preset_too_many", "max", perms::MAX_SAVED_PRESETS)),
        "preset-bad-name" => t.error = Some(crate::t1("manage.preset_bad_name", "max", perms::PRESET_NAME_MAX)),
        "preset-deleted" => t.notice = Some(crate::t("manage.preset_deleted")),
        _ => {}
    }
}

/// The Custom branch of `POST /manage/permissions` with a `save_as` name:
/// the set becomes a preset and the account is given it, both or neither.
/// Refused (the name, the limit), nothing is saved and the page says why.
pub async fn save_as(state: &AppState, user: &CurrentUser, target: i64, email: &str, name: String, set: Perms) -> Result<Response, AppError> {
    let saved = state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            let id = match perms::save_preset(&tx, &name, set)? {
                Ok(id) => id,
                Err(e) => return Ok(Err(e)),
            };
            perms::apply_preset(&tx, target, id)?;
            tx.commit()?;
            Ok(Ok(id))
        })
        .await?;
    Ok(match saved {
        Ok(id) => {
            tracing::info!("user {} saved preset {id} ({}) and gave it to user {target}", user.id, set.bits());
            done("permissions", email)
        }
        Err(PresetError::NameTaken) => done("preset-name-taken", email),
        Err(PresetError::TooMany) => done("preset-too-many", email),
        Err(PresetError::BadName) => done("preset-bad-name", email),
    })
}

#[derive(Deserialize)]
pub struct DeleteForm {
    #[serde(default)]
    pub id: i64,
}

/// POST /manage/presets/delete (id=…): a saved preset is deleted. The
/// accounts that had it keep their permissions and show as Custom. A
/// built-in preset, or an id no preset has, is a 400. An owner or a
/// superuser; a 404 otherwise.
pub async fn delete(State(state): State<Arc<AppState>>, user: CurrentUser, Form(form): Form<DeleteForm>) -> Result<Response, AppError> {
    if !user.manages_users() {
        return Err(AppError::NotFound);
    }
    let id = form.id;
    let deleted = state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            let deleted = perms::delete_preset(&tx, id)?;
            tx.commit()?;
            Ok(deleted)
        })
        .await?;
    if !deleted {
        return Err(AppError::BadRequest("no such saved preset".into()));
    }
    tracing::info!("user {} deleted preset {id}", user.id);
    Ok(done("preset-deleted", ""))
}

#[derive(Deserialize)]
pub struct DefaultPresetForm {
    #[serde(default)]
    pub preset: String,
}

/// POST /manage/default-preset (preset=<id>): the preset new accounts start
/// as (`settings::DEFAULT_PRESET`, #142). An owner or a superuser; a 404
/// for anyone else. A preset with Superuser is refused: an invitation link
/// must not hand out control of the user list.
pub async fn set_default(State(state): State<Arc<AppState>>, user: CurrentUser, Form(form): Form<DefaultPresetForm>) -> Result<Response, AppError> {
    if !user.manages_users() {
        return Err(AppError::NotFound);
    }
    let Ok(id) = form.preset.trim().parse::<i64>() else {
        return Err(AppError::BadRequest("preset must be a preset's id".into()));
    };
    let chosen = state
        .db
        .call(move |c| {
            let Some(p) = perms::preset(c, id)? else {
                return Ok(None);
            };
            if p.perms.has(Perm::Superuser) {
                return Ok(Some(false));
            }
            crate::settings::set_default_preset(c, id)?;
            Ok(Some(true))
        })
        .await?;
    match chosen {
        None => Err(AppError::BadRequest("no such preset".into())),
        Some(false) => Ok(Redirect::to(&crate::url("/manage?did=preset-default-superuser")).into_response()),
        Some(true) => {
            tracing::info!("user {} set new accounts to start as preset {id}", user.id);
            Ok(Redirect::to(&crate::url(&format!("/manage?did=preset-default&preset={id}"))).into_response())
        }
    }
}
