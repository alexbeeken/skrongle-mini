//! The Skrongle Cash shop (#569, `motivators/shop.rs`): `/shop` lists the
//! catalogue with prices and the learner's shelf; `POST /api/shop/buy` buys
//! one badge with the course's SKC. Reached only from the SKC hero, and only
//! while the course wears Skrongle Cash.

use std::sync::Arc;

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::Form;

use super::{CourseView, Page};
use crate::auth::{CurrentUser, Project};
use crate::error::AppError;
use crate::motivators::shop::{self, BadgeView, Refusal};
use crate::AppState;

#[derive(serde::Deserialize, Default)]
pub struct ShopQuery {
    /// Why the last buy was refused (`Refusal::code`), and of which badge.
    #[serde(default)]
    refused: String,
    #[serde(default)]
    item: String,
    #[serde(default)]
    short: i64,
}

#[derive(Template)]
#[template(path = "shop.html")]
pub struct ShopTemplate {
    pub user: Option<CurrentUser>,
    pub course: CourseView,
    pub balance: i64,
    pub badges: Vec<BadgeView>,
    pub shelf: Vec<BadgeView>,
    /// The refusal to show in a modal as the page opens, in the app's words.
    pub refusal: Option<String>,
}

/// GET /shop: the catalogue and the shelf. Home when the course does not wear SKC.
pub async fn page(State(state): State<Arc<AppState>>, p: Project, headers: HeaderMap, Query(q): Query<ShopQuery>) -> Result<Response, AppError> {
    let uid = p.user.id;
    let store = p.course.clone();
    let tz = super::sprint::tz_offset_secs(&headers);
    let found = state
        .db
        .call(move |c| {
            if crate::motivators::worn(c, uid, store.id())?.id() != "skc" {
                return Ok(None);
            }
            Ok(Some((crate::motivators::skc::balance(c, uid, &store, tz)?, shop::owned(c, uid, store.id())?)))
        })
        .await?;
    let Some((balance, owned)) = found else { return Ok(Redirect::to(&crate::url("/")).into_response()) };
    let badges: Vec<BadgeView> = shop::catalogue().badges.iter().map(|b| BadgeView::of(b, owned.contains(&b.id))).collect();
    let shelf = badges.iter().filter(|b| b.owned).cloned().collect();
    let title = shop::catalogue().get(&q.item).map(|b| b.title()).unwrap_or_default();
    let refusal = match q.refused.as_str() {
        "owned" => Some(crate::t1("motivator.skc.shop.refused_owned", "title", &title)),
        "short" => Some(crate::t2("motivator.skc.shop.refused_short", "n", q.short, "title", &title)),
        "" => None,
        _ => Some(crate::t("motivator.skc.shop.refused_unknown")),
    };
    Ok(Page(ShopTemplate { course: CourseView::of(&p.course), user: Some(p.user), balance, badges, shelf, refusal }).into_response())
}

#[derive(serde::Deserialize)]
pub struct BuyForm {
    item: String,
}

/// POST /api/shop/buy (item): checks the price and the balance, then back
/// to the shop, which shows a refusal in a modal. 400 when the course does
/// not wear Skrongle Cash.
pub async fn buy(State(state): State<Arc<AppState>>, p: Project, headers: HeaderMap, Form(form): Form<BuyForm>) -> Result<Response, AppError> {
    let uid = p.user.id;
    let store = p.course.clone();
    let tz = super::sprint::tz_offset_secs(&headers);
    let item = form.item.clone();
    let done = state.db.call(move |c| shop::buy(c, uid, &store, tz, &item)).await?;
    let to = match done {
        Ok(_) => "/shop".to_string(),
        Err(Refusal::NotWorn) => return Err(AppError::ApiBadRequest("this course does not wear Skrongle Cash".into())),
        Err(Refusal::Short(n)) => format!("/shop?refused=short&short={n}&item={}", form.item),
        Err(r) => format!("/shop?refused={}&item={}", r.code(), if shop::catalogue().get(&form.item).is_some() { form.item.as_str() } else { "" }),
    };
    Ok(Redirect::to(&crate::url(&to)).into_response())
}

/// GET /badges/<id>.svg: a badge's plain art.
pub async fn art(Path(file): Path<String>, headers: HeaderMap) -> Response {
    let Some(body) = file.strip_suffix(".svg").and_then(|id| shop::catalogue().art(id)) else { return StatusCode::NOT_FOUND.into_response() };
    super::svg_response(&headers, body, "public, no-cache")
}

#[cfg(test)]
mod tests;
