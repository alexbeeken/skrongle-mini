//! Content reports and copyright notices (#968, `content_reports.rs`): the
//! "Report this course" form on a shared course's card, the public copyright
//! form at `/legal/copyright`, and the Superuser page's Forward to owner and
//! Resolve. Every one of them needs mail: without it (Skrongle mini) the
//! report form is a 404 and the copyright page shows the contact address.

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::Form;
use rusqlite::OptionalExtension;
use serde::Deserialize;
use std::sync::Arc;

use super::Page;
use crate::auth::{CurrentUser, MaybeUser};
use crate::content_reports::{self as reports, Report};
use crate::db::now;
use crate::error::AppError;
use crate::security::{self, ClientIp};
use crate::AppState;

/// Mail the operator a new report (every address of
/// [`reports::operator_addresses`]). A message that cannot be queued is
/// logged: the report is stored either way and the Superuser page lists it.
async fn tell_operator(state: &AppState, headers: &HeaderMap, report: &Report) -> Result<(), AppError> {
    let to = state.db.call(|c| reports::operator_addresses(c)).await?;
    if to.is_empty() {
        tracing::warn!("content report {}: no SKRONGLE_LEGAL_EMAIL and no superuser with an address to tell", report.id);
    }
    let link = format!("{}/superuser#reports", crate::verify::public_base(state, headers));
    for addr in to {
        match reports::operator_message(&addr, report, &link) {
            // Over a mail cap `try_queue` logs it; the report is stored either way.
            Ok(m) => {
                let _ = state.mail.try_queue(&state.db, m).await?;
            }
            Err(e) => tracing::warn!("content report {}: cannot mail {addr:?}: {e}", report.id),
        }
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct ReportForm {
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub details: String,
}

/// POST /courses/{id}/report: a learner reports a course shared with them.
/// Built-in courses, their own and courses they cannot open are a 404, as
/// is any course on a server without mail.
pub async fn report_course(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    headers: HeaderMap,
    Path(id): Path<String>,
    Form(form): Form<ReportForm>,
) -> Result<Response, AppError> {
    if !state.caps().mail || state.courses.is_builtin(&id) {
        return Err(AppError::NotFound);
    }
    let (uid, cid) = (user.id, id.clone());
    let course = state
        .db
        .call(move |c| {
            let row: Option<(i64, String)> = c.query_row("SELECT owner_id, title FROM courses WHERE id = ?1", [&cid], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
            Ok(match row {
                Some((owner, title)) if owner != uid && crate::share::can_open(c, uid, &cid)? => Some(title),
                _ => None,
            })
        })
        .await?;
    let Some(title) = course else { return Err(AppError::NotFound) };
    if !reports::REASONS.contains(&form.reason.as_str()) {
        return Err(AppError::BadRequest(crate::t("reports.course.pick_reason")));
    }
    let email = crate::profile::validate_email(&form.email).map_err(AppError::BadRequest)?;
    let details = reports::clip(&form.details, reports::MAX_TEXT);
    if details.is_empty() {
        return Err(AppError::BadRequest(crate::t("reports.course.details_empty")));
    }
    if !state.limits.hit(security::REPORTS_PER_USER, &uid.to_string(), now()) {
        tracing::warn!("content report rate limit hit by user {uid}");
        return Ok(Redirect::to(&crate::url("/courses?notice=report-limit")).into_response());
    }
    let mut report = Report { user_id: Some(uid), course_id: Some(id), course_title: title, reason: form.reason, email, details, created_at: now(), ..Report::default() };
    let r = report.clone();
    report.id = state.db.call(move |c| reports::insert(c, &r)).await?;
    tracing::info!("user {uid} reported course {:?} ({}): report {}", report.course_id, report.reason, report.id);
    tell_operator(&state, &headers, &report).await?;
    Ok(Redirect::to(&crate::url("/courses?notice=reported")).into_response())
}

#[derive(Template)]
#[template(path = "copyright.html")]
pub struct CopyrightTemplate {
    pub user: Option<CurrentUser>,
    /// The server sends mail: the form. Without (Skrongle mini): the contact address only.
    pub form: bool,
    pub contact: String,
    pub sent: bool,
    pub error: Option<String>,
    /// What was typed, kept when the form comes back with an error.
    pub v: CopyrightForm,
}

#[derive(Deserialize, Default, Clone)]
pub struct CopyrightForm {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub work: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub details: String,
    #[serde(default)]
    pub good_faith: Option<String>,
}

#[derive(Deserialize)]
pub struct CopyrightQuery {
    #[serde(default)]
    pub sent: Option<String>,
}

fn copyright_page(state: &AppState, user: Option<CurrentUser>, status: StatusCode, sent: bool, error: Option<String>, v: CopyrightForm) -> Response {
    let t = CopyrightTemplate { user, form: state.caps().mail, contact: crate::terms::legal_email(), sent, error, v };
    (status, Page(t)).into_response()
}

/// GET /legal/copyright: the copyright notice form (public).
pub async fn copyright_form(State(state): State<Arc<AppState>>, MaybeUser(user): MaybeUser, Query(q): Query<CopyrightQuery>) -> Response {
    let mut v = CopyrightForm::default();
    if let Some(u) = &user {
        v.email = u.email.clone();
        v.name = format!("{} {}", u.first_name, u.last_name).trim().to_string();
    }
    copyright_page(&state, user, StatusCode::OK, q.sent.is_some(), None, v)
}

/// POST /legal/copyright: store the notice and mail the operator. Five a
/// minute from one address (counted in dev mode too); past that the form
/// comes back with a 429.
pub async fn copyright_submit(
    State(state): State<Arc<AppState>>,
    MaybeUser(user): MaybeUser,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Form(form): Form<CopyrightForm>,
) -> Result<Response, AppError> {
    if !state.caps().mail {
        return Err(AppError::NotFound);
    }
    if !state.limits.hit_always(security::COPYRIGHT_PER_IP, &ip, now()) {
        tracing::warn!("copyright notice rate limit hit from {ip}");
        return Ok(copyright_page(&state, user, StatusCode::TOO_MANY_REQUESTS, false, Some(crate::t("reports.copyright.too_many")), form));
    }
    let line = |s: &str| reports::clip(s, reports::MAX_LINE);
    let (name, work, location, details) = (line(&form.name), line(&form.work), line(&form.location), reports::clip(&form.details, reports::MAX_TEXT));
    let email = match crate::profile::validate_email(&form.email) {
        Ok(e) => e,
        Err(e) => return Ok(copyright_page(&state, user, StatusCode::OK, false, Some(e), form)),
    };
    if name.is_empty() || work.is_empty() || location.is_empty() || details.is_empty() {
        return Ok(copyright_page(&state, user, StatusCode::OK, false, Some(crate::t("reports.copyright.missing")), form));
    }
    if form.good_faith.is_none() {
        return Ok(copyright_page(&state, user, StatusCode::OK, false, Some(crate::t("reports.copyright.tick_good_faith")), form));
    }
    let loc = location.clone();
    let course = state.db.call(move |c| reports::find_course(c, &loc)).await?;
    let (course_id, course_title) = course.map_or((None, String::new()), |(id, title)| (Some(id), title));
    let mut report = Report {
        user_id: user.as_ref().map(|u| u.id),
        course_id,
        course_title,
        location,
        reason: "copyright".into(),
        email,
        name,
        work,
        details,
        good_faith: true,
        created_at: now(),
        ..Report::default()
    };
    let r = report.clone();
    report.id = state.db.call(move |c| reports::insert(c, &r)).await?;
    tracing::info!("copyright notice {} about course {:?}", report.id, report.course_id);
    tell_operator(&state, &headers, &report).await?;
    Ok(Redirect::to(&crate::url("/legal/copyright?sent=1")).into_response())
}

#[derive(Deserialize)]
pub struct ReportIdForm {
    #[serde(default)]
    pub id: i64,
}

/// POST /superuser/reports/forward (id=…): the course's owner gets a copy
/// of the report (`reports::forward_message`). Superusers only: a 404
/// otherwise, and on a server without mail.
pub async fn forward(State(state): State<Arc<AppState>>, user: CurrentUser, Form(form): Form<ReportIdForm>) -> Result<Response, AppError> {
    if !user.superuser || !state.caps().mail {
        return Err(AppError::NotFound);
    }
    let id = form.id;
    let found = state
        .db
        .call(move |c| {
            let Some(r) = reports::get(c, id)? else { return Ok(None) };
            let owner = match r.course_id.as_deref() {
                Some(cid) => reports::owner(c, cid)?,
                None => None,
            };
            Ok(Some((r, owner)))
        })
        .await?;
    let did = match found {
        Some((r, Some((owner_id, to)))) => match reports::forward_message(&to, owner_id, &r) {
            Ok(m) => {
                state.mail.queue(&state.db, m).await?;
                state.db.call(move |c| reports::mark_forwarded(c, id, now())).await?;
                tracing::info!("superuser {} forwarded content report {id} to the owner (user {owner_id})", user.id);
                "report-forwarded"
            }
            Err(e) => {
                tracing::warn!("content report {id}: cannot mail the owner: {e}");
                "report-no-owner"
            }
        },
        Some(_) => "report-no-owner",
        None => "report-unknown",
    };
    Ok(Redirect::to(&crate::url(&format!("/superuser?did={did}#reports"))).into_response())
}

/// POST /superuser/reports/resolve (id=…): off the open list. Superusers only.
pub async fn resolve(State(state): State<Arc<AppState>>, user: CurrentUser, Form(form): Form<ReportIdForm>) -> Result<Response, AppError> {
    if !user.superuser {
        return Err(AppError::NotFound);
    }
    let id = form.id;
    let did = if state.db.call(move |c| reports::resolve(c, id, now())).await? {
        tracing::info!("superuser {} resolved content report {id}", user.id);
        "report-resolved"
    } else {
        "report-unknown"
    };
    Ok(Redirect::to(&crate::url(&format!("/superuser?did={did}#reports"))).into_response())
}
