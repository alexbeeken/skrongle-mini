//! The waitlist page (#934, `waitlist.rs`): while the waitlist is on, the
//! home page for anyone signed out. One e-mail field; joining sends a link
//! to confirm the address, and the page says "check your e-mail" whether or
//! not the address was on the list already.

use askama::Template;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::Form;
use serde::Deserialize;
use std::sync::Arc;

use super::Page;
use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::security::{self, ClientIp};
use crate::AppState;

#[derive(Template)]
#[template(path = "waitlist.html")]
pub struct WaitlistTemplate {
    pub user: Option<CurrentUser>,
    /// What the page says instead of the form: joined, confirmed, or a bad link.
    pub done: Option<Done>,
    pub error: Option<String>,
    pub email: String,
    /// The built-in themes, each drawn in its own colours, font and pictures (#1071).
    pub themes: Vec<ThemeShow>,
    /// The Skrongle Cash shop's badges, plain art.
    pub badges: Vec<crate::motivators::shop::BadgeView>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Done {
    Joined,
    Confirmed,
    BadLink,
}

/// One theme as the waitlist page shows it off.
pub struct ThemeShow {
    pub id: String,
    pub title: String,
    pub description: String,
    /// The theme's colour tokens and font on its band (`.wl-theme[data-wl-theme=…]`).
    pub css: String,
    pub banner: String,
    /// Its profile pictures: name, picture, tile colour.
    pub avatars: Vec<(String, String, String)>,
    /// Its own words for a right answer.
    pub praise: String,
    /// The badges it draws its own way.
    pub badges: Vec<crate::motivators::shop::BadgeView>,
}

impl ThemeShow {
    fn of(t: &crate::content::Theme) -> ThemeShow {
        let art = |path: &str| super::art_url(&t.id, path);
        let catalogue = &crate::motivators::shop::catalogue().badges;
        ThemeShow {
            id: t.id.clone(),
            title: t.title.clone(),
            description: t.description.clone(),
            css: t.css_for(&format!(".wl-theme[data-wl-theme=\"{}\"]", t.id)), // panic-ok: an attribute selector in a string, not indexing
            banner: art(&t.art.banner),
            avatars: t.avatars.iter().map(|a| (a.label.clone(), art(&a.image), a.color.clone())).collect(),
            praise: t.strings.praise.first().cloned().unwrap_or_default(),
            badges: catalogue
                .iter()
                .map(|b| crate::motivators::shop::BadgeView::of(b, Some(t), false))
                .filter(|b| b.variant != "plain")
                .collect(),
        }
    }
}

impl WaitlistTemplate {
    pub fn new(state: &AppState) -> WaitlistTemplate {
        WaitlistTemplate {
            user: None,
            done: None,
            error: None,
            email: String::new(),
            themes: state.themes.all().iter().filter(|p| p.owner.is_none()).map(|p| ThemeShow::of(&p.theme)).collect(),
            badges: crate::motivators::shop::catalogue().badges.iter().map(|b| crate::motivators::shop::BadgeView::of(b, None, false)).collect(),
        }
    }
}

#[derive(Deserialize)]
pub struct JoinForm {
    #[serde(default)]
    pub email: String,
}

/// POST /waitlist (email=…): on the list, and a confirmation link mailed.
/// With the waitlist off: back to the home page.
pub async fn join(State(state): State<Arc<AppState>>, ClientIp(ip): ClientIp, headers: HeaderMap, Form(form): Form<JoinForm>) -> Result<Response, AppError> {
    if !crate::waitlist::on() {
        return Ok(Redirect::to(&crate::url("/")).into_response());
    }
    let fail = |status: StatusCode, msg: String| (status, Page(WaitlistTemplate { error: Some(msg), email: form.email.trim().to_string(), ..WaitlistTemplate::new(&state) })).into_response();
    if !state.limits.hit(security::WAITLIST_PER_IP, &ip, crate::db::now()) {
        return Ok(fail(StatusCode::TOO_MANY_REQUESTS, crate::t("waitlist.too_many")));
    }
    let email = match crate::profile::validate_email(&form.email) {
        Ok(e) => e,
        Err(e) => return Ok(fail(StatusCode::OK, e)),
    };
    let e = email.clone();
    let joined = state.db.call(move |c| crate::waitlist::join(c, &e, crate::db::now())).await?;
    if let crate::waitlist::Joined::Confirm(token) = joined {
        let link = format!("{}/waitlist/confirm?token={token}", crate::verify::public_base(&state, &headers));
        let mail = crate::waitlist::confirm_mail(&email, &link).map_err(AppError::Internal)?;
        // A cap that says no sends nothing; the page says the same either way.
        if let Ok(id) = state.mail.try_queue(&state.db, mail).await? {
            if state.dev {
                tracing::info!("dev: waitlist confirmation link (mail #{id}): {link}");
            }
        }
    }
    Ok(Page(WaitlistTemplate { done: Some(Done::Joined), ..WaitlistTemplate::new(&state) }).into_response())
}

#[derive(Deserialize)]
pub struct ConfirmQuery {
    pub token: Option<String>,
}

/// GET /waitlist/confirm?token=…: the address is confirmed and can be invited.
pub async fn confirm(State(state): State<Arc<AppState>>, Query(q): Query<ConfirmQuery>) -> Result<Response, AppError> {
    let token = q.token.unwrap_or_default();
    let ok = state.db.call(move |c| crate::waitlist::confirm(c, &token, crate::db::now())).await?;
    let done = if ok { Done::Confirmed } else { Done::BadLink };
    Ok(Page(WaitlistTemplate { done: Some(done), ..WaitlistTemplate::new(&state) }).into_response())
}
