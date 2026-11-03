//! `/manage`: the page for whoever runs the server (owners only,
//! `users.owner`). Every account, newest first, all on one page (the
//! server holds at most `caps::MAX_ACCOUNTS`), with
//! the actions on one: a temporary password, two-factor off and make
//! owner (#635, #16, `owner.rs`); and with
//! `SKRONGLE_SIGNUP=invite`, the invitation links (#634, `signup.rs`); and the AI section
//! (#25): the server's AI API keys, which every account's AI features use
//! (`secrets.rs`), and the switch that hides those features for everyone
//! (`settings::AI_OFF`).
//! Anyone else gets a 404, as if the page did not exist; the account menu
//! links here only for owners.

use askama::Template;
use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::{Form, Json};
use serde::Deserialize;
use std::sync::Arc;

use super::Page;
use crate::auth::{self, CurrentUser};
use crate::error::AppError;
use crate::profile;
use crate::secrets::{self, KeyStatus, Provider};
use crate::AppState;

#[derive(Template)]
#[template(path = "manage.html")]
pub struct ManageTemplate {
    pub user: Option<CurrentUser>,
    /// What the last action did, or why it could not.
    pub notice: Option<String>,
    pub error: Option<String>,
    /// `SKRONGLE_SIGNUP`: `open`, `invite` or `closed`.
    pub mode: &'static str,
    /// An invitation link just made: shown this once.
    pub new_link: Option<String>,
    /// A temporary password just set: the account's address and the
    /// password, shown this once.
    pub temp_password: Option<(String, String)>,
    /// Open invitations: id, made, expires.
    pub invites: Vec<(i64, String, String)>,
    /// Every account, newest first.
    pub list: Vec<AccountRow>,
    /// How many accounts there are: the length of `list`.
    pub total: i64,
    /// The most accounts this server holds (`caps::MAX_ACCOUNTS`).
    pub max_accounts: usize,
    /// The server holds them all: no invitation form.
    pub full: bool,
    /// The server's AI API keys, one entry per provider; never the key itself.
    pub api_keys: Vec<KeyStatus>,
    /// The AI features are hidden for every account (`settings::AI_OFF`).
    pub ai_off: bool,
    /// The model the Ollama-compatible server runs (`settings::OLLAMA_MODEL`), '' for the default.
    pub ollama_model: String,
}

pub struct AccountRow {
    pub email: String,
    pub created: String,
    /// '' when no session is on record.
    pub last_sign_in: String,
    pub owner: bool,
}

/// The page, for an owner.
async fn load(state: &AppState, user: CurrentUser) -> Result<ManageTemplate, AppError> {
    let now = crate::db::now();
    let vault = state.vault.clone();
    let (invites, accounts, (api_keys, ai_off, ollama_model)) = state
        .db
        .call(move |c| {
            let ai = (secrets::statuses(c, &vault)?, crate::settings::ai_off(c)?, crate::settings::get(c, crate::settings::OLLAMA_MODEL)?.unwrap_or_default());
            Ok((crate::signup::open_invites(c, now)?, crate::signup::accounts(c)?, ai))
        })
        .await?;
    let total = accounts.len() as i64;
    Ok(ManageTemplate {
        user: Some(user),
        notice: None,
        error: None,
        mode: state.signup.as_str(),
        new_link: None,
        temp_password: None,
        invites: invites.into_iter().map(|i| (i.id, profile::long_date(i.created_at), profile::long_date(i.expires_at))).collect(),
        list: accounts
            .into_iter()
            .map(|a| AccountRow {
                email: a.email,
                created: profile::long_date(a.created_at),
                last_sign_in: a.last_sign_in.map(profile::long_date).unwrap_or_default(),
                owner: a.owner,
            })
            .collect(),
        total,
        max_accounts: crate::caps::MAX_ACCOUNTS,
        full: crate::caps::accounts_full(total, state.dev),
        api_keys,
        ai_off,
        ollama_model,
    })
}

impl ManageTemplate {
    /// The model the Ollama-compatible server runs, as the page shows it.
    pub fn ollama_model_shown(&self) -> &str {
        if self.ollama_model.is_empty() { crate::ai::OLLAMA_DEFAULT_MODEL } else { &self.ollama_model }
    }
}

#[derive(Deserialize)]
pub struct ShowQuery {
    /// Back from an action: `unknown`, `revoked`,
    /// `two-factor-off` or `owner`, about `email`;
    /// or `key-removed`, about `provider`; or `ai-off` / `ai-on`.
    pub did: Option<String>,
    pub email: Option<String>,
    pub provider: Option<String>,
}

/// GET /manage
pub async fn show(State(state): State<Arc<AppState>>, user: CurrentUser, Query(q): Query<ShowQuery>) -> Result<Response, AppError> {
    if !user.owner {
        return Err(AppError::NotFound);
    }
    let mut t = load(&state, user).await?;
    let email = q.email.as_deref().unwrap_or_default();
    match q.did.as_deref() {
        Some("unknown") => t.error = Some(crate::t1("manage.unknown", "email", format!("{email:?}"))),
        Some("revoked") => t.notice = Some(crate::t("manage.invite_revoked")),
        Some("two-factor-off") => t.notice = Some(crate::t1("manage.two_factor_off_done", "email", email)),
        Some("owner") => t.notice = Some(crate::t1("manage.make_owner_done", "email", email)),
        Some("key-removed") => {
            if let Some(p) = q.provider.as_deref().and_then(Provider::parse) {
                t.notice = Some(crate::t1("manage.api_keys.removed", "provider", p.label()));
            }
        }
        Some("ai-off") => t.notice = Some(crate::t("manage.ai.hidden_notice")),
        Some("ai-on") => t.notice = Some(crate::t("manage.ai.shown_notice")),
        _ => {}
    }
    Ok(Page(t).into_response())
}

/// Back to the page after an action: what it did, about which address.
fn done(did: &str, email: &str) -> Response {
    let to = format!("/manage?did={did}&email={}", crate::urlenc::urlencode(email));
    Redirect::to(&crate::url(&to)).into_response()
}

#[derive(Deserialize)]
pub struct AccountForm {
    #[serde(default)]
    pub email: String,
}

/// POST /manage/invites: a new invitation link (`SKRONGLE_SIGNUP=invite`),
/// shown on the page this once; only its hash is kept. Owners only, and
/// only in invite mode: a 404 otherwise.
pub async fn make_invite(State(state): State<Arc<AppState>>, user: CurrentUser, headers: HeaderMap) -> Result<Response, AppError> {
    if !user.owner || state.signup != crate::signup::Mode::Invite {
        return Err(AppError::NotFound);
    }
    let (uid, dev) = (user.id, state.dev);
    let token = state
        .db
        .call(move |c| Ok(if crate::signup::is_full(c, dev)? { None } else { Some(crate::signup::make(c, uid, crate::db::now())?) }))
        .await?;
    let Some(token) = token else {
        let mut t = load(&state, user).await?;
        t.error = Some(crate::t("auth.signup.full"));
        return Ok(Page(t).into_response());
    };
    tracing::info!("owner {uid} made an invitation link");
    let site = crate::routes::public_base(&state, &headers);
    let link = format!("{site}/register?invite={token}");
    let mut t = load(&state, user).await?;
    t.new_link = Some(link);
    // The link is a key to the server: not to be cached anywhere.
    Ok(([(header::CACHE_CONTROL, "no-store")], Page(t)).into_response())
}

#[derive(Deserialize)]
pub struct RevokeInviteForm {
    #[serde(default)]
    pub id: i64,
}

/// POST /manage/invites/revoke (id=…): an open invitation stops working.
/// Owners only: a 404 otherwise.
pub async fn revoke_invite(State(state): State<Arc<AppState>>, user: CurrentUser, Form(form): Form<RevokeInviteForm>) -> Result<Response, AppError> {
    if !user.owner {
        return Err(AppError::NotFound);
    }
    let id = form.id;
    if state.db.call(move |c| crate::signup::revoke(c, id)).await? {
        tracing::info!("owner {} revoked invitation {id}", user.id);
    }
    Ok(Redirect::to(&crate::url("/manage?did=revoked")).into_response())
}

/// POST /manage/temp-password (email=…): the account gets a generated
/// temporary password (`owner.rs`, #635), shown on the page this once,
/// which it must change at its next sign-in; its sessions end (an owner
/// resetting their own keeps this one). Owners only: a 404 otherwise.
/// The log line never holds the password.
pub async fn set_temp_password(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    auth::SessionToken(token): auth::SessionToken,
    Form(form): Form<AccountForm>,
) -> Result<Response, AppError> {
    if !user.owner {
        return Err(AppError::NotFound);
    }
    let email = form.email.trim().to_lowercase();
    let e = email.clone();
    let Some(target) = state.db.call(move |c| crate::owner::find(c, &e)).await? else {
        return Ok(done("unknown", &email));
    };
    let password = crate::owner::temp_password();
    let hash = auth::hash_password_async(password.clone()).await?;
    let keep = if target == user.id { token } else { None };
    let ended = state.db.call(move |c| crate::owner::set_temporary_password(c, target, &hash, keep.as_deref())).await?;
    tracing::info!("owner {} set a temporary password for user {target} ({ended} sessions signed out)", user.id);
    let mut t = load(&state, user).await?;
    t.temp_password = Some((email, password));
    // The password is a key to the account: not to be cached anywhere.
    Ok(([(header::CACHE_CONTROL, "no-store")], Page(t)).into_response())
}

/// POST /manage/make-owner (email=…): the account becomes an owner too
/// (`owner::make_owner`): it sees this page and its actions. Owners only: a
/// 404 otherwise.
pub async fn make_owner(State(state): State<Arc<AppState>>, user: CurrentUser, Form(form): Form<AccountForm>) -> Result<Response, AppError> {
    if !user.owner {
        return Err(AppError::NotFound);
    }
    let email = form.email.trim().to_lowercase();
    let e = email.clone();
    let Some(target) = state.db.call(move |c| crate::owner::find(c, &e)).await? else {
        return Ok(done("unknown", &email));
    };
    state.db.call(move |c| crate::owner::make_owner(c, target)).await?;
    tracing::info!("owner {} made user {target} an owner", user.id);
    Ok(done("owner", &email))
}

/// POST /manage/two-factor-off (email=…): the account's two-factor
/// sign-in is turned off (`owner::disable_two_factor`). Owners only: a
/// 404 otherwise.
pub async fn turn_off_two_factor(State(state): State<Arc<AppState>>, user: CurrentUser, Form(form): Form<AccountForm>) -> Result<Response, AppError> {
    if !user.owner {
        return Err(AppError::NotFound);
    }
    let email = form.email.trim().to_lowercase();
    let e = email.clone();
    let Some(target) = state.db.call(move |c| crate::owner::find(c, &e)).await? else {
        return Ok(done("unknown", &email));
    };
    state.db.call(move |c| crate::owner::disable_two_factor(c, target)).await?;
    tracing::info!("owner {} turned off two-factor sign-in for user {target}", user.id);
    Ok(done("two-factor-off", &email))
}

/// What the keys form autosaves (`static/manage.js`): `key_<provider>`
/// (`key_anthropic`, …; `key_ollama` is a base URL), any of them, and
/// `ollama_model`.
#[derive(Deserialize)]
pub struct KeysForm {
    #[serde(flatten)]
    pub keys: std::collections::HashMap<String, serde_json::Value>,
}

/// POST /api/manage/keys: save the server's AI API keys sent. All or
/// nothing: a key that does not validate comes back as
/// `422 {"errors": {field: message}}` and nothing is stored. Saved, the
/// answer has each saved key's last four characters (`{"keys": {provider:
/// hint}}`). Owners only: a 404 otherwise. The log line names the provider,
/// never the key.
pub async fn save_keys(State(state): State<Arc<AppState>>, user: CurrentUser, Json(form): Json<KeysForm>) -> Result<Response, AppError> {
    if !user.owner {
        return Err(AppError::NotFound);
    }
    let mut errors = serde_json::Map::new();
    let mut keys = Vec::new();
    let mut model = None;
    for (field, value) in &form.keys {
        let provider = field.strip_prefix("key_").and_then(Provider::parse);
        let r = match (provider, value.as_str()) {
            (Some(p), Some(v)) => secrets::validate_key(p, v).map(|k| keys.push((p, k))),
            (None, Some(v)) if field == "ollama_model" => secrets::validate_model(v).map(|m| model = Some(m)),
            _ => Err(crate::t("profile.save.not_a_field")),
        };
        if let Err(e) = r {
            errors.insert(field.clone(), e.into());
        }
    }
    if !errors.is_empty() {
        return Ok((StatusCode::UNPROCESSABLE_ENTITY, Json(serde_json::json!({ "errors": errors }))).into_response());
    }
    let vault = state.vault.clone();
    let saved: Vec<Provider> = keys.iter().map(|(p, _)| *p).collect();
    let saved_model = model.clone();
    let statuses = state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            for (provider, key) in &keys {
                secrets::save(&tx, &vault, *provider, key)?;
            }
            if let Some(m) = &model {
                crate::settings::set(&tx, crate::settings::OLLAMA_MODEL, m)?;
            }
            let statuses = secrets::statuses(&tx, &vault)?;
            tx.commit()?;
            Ok(statuses)
        })
        .await?;
    for p in &saved {
        tracing::info!("owner {} saved the server's {p} API key", user.id);
    }
    let hints: serde_json::Map<String, serde_json::Value> =
        statuses.iter().filter(|k| saved.contains(&k.provider)).map(|k| (k.provider.id().to_string(), k.hint.clone().unwrap_or_default().into())).collect();
    let mut body = serde_json::json!({ "keys": hints });
    if let Some(m) = saved_model {
        tracing::info!("owner {} set the Ollama model to {m}", user.id);
        body["ollama_model"] = m.into();
    }
    Ok(Json(body).into_response())
}

#[derive(Deserialize)]
pub struct ApiKeyForm {
    #[serde(default)]
    pub provider: String,
}

/// POST /manage/api-keys/remove (provider=…): the server's key for that
/// provider is deleted; the AI features stop offering it. Owners only: a
/// 404 otherwise.
pub async fn remove_key(State(state): State<Arc<AppState>>, user: CurrentUser, Form(form): Form<ApiKeyForm>) -> Result<Response, AppError> {
    if !user.owner {
        return Err(AppError::NotFound);
    }
    let Some(provider) = Provider::parse(&form.provider) else {
        return Err(AppError::BadRequest("unknown provider".into()));
    };
    state.db.call(move |c| secrets::remove(c, provider)).await?;
    tracing::info!("owner {} removed the server's {provider} API key", user.id);
    Ok(Redirect::to(&crate::url(&format!("/manage?did=key-removed&provider={provider}#ai"))).into_response())
}

#[derive(Deserialize)]
pub struct AiForm {
    /// `1` hides the AI features for everyone, `0` shows them again.
    #[serde(default)]
    pub off: String,
}

/// POST /manage/ai (off=1|0): hide or show the AI features for every
/// account (`settings::AI_OFF`). Hidden, every page leaves out the tutor,
/// course generation, Edit with AI and translation, their routes refuse
/// (`CurrentUser::require_ai`), and tutor chats under way end. The keys
/// stay, for when they are shown again. Owners only: a 404 otherwise.
pub async fn set_ai(State(state): State<Arc<AppState>>, user: CurrentUser, Form(form): Form<AiForm>) -> Result<Response, AppError> {
    if !user.owner {
        return Err(AppError::NotFound);
    }
    let off = match form.off.as_str() {
        "1" => true,
        "0" => false,
        _ => return Err(AppError::BadRequest("off must be 1 or 0".into())),
    };
    state.db.call(move |c| crate::settings::set_ai_off(c, off)).await?;
    if off {
        // A tutor conversation under way would still be calling the provider.
        state.tutors.drop_all();
    }
    tracing::info!("owner {} {} the AI features for every account", user.id, if off { "hid" } else { "showed" });
    let did = if off { "ai-off" } else { "ai-on" };
    Ok(Redirect::to(&crate::url(&format!("/manage?did={did}#ai"))).into_response())
}
