//! `/dev/jobs` (#872): the newest rows of the background job table, and
//! Run now / Queue a failing job. Split out of `routes/dev.rs` to keep it
//! under the file-size limit.

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Response};
use axum::{Form, Json};
use serde::Deserialize;
use std::sync::Arc;

use super::super::Page;
use super::{done, wants_html, FormatParams};
use crate::auth::{CurrentUser, MaybeUser};
use crate::error::AppError;
use crate::AppState;

/// One row of `/dev/jobs`.
pub struct JobView {
    pub row: crate::jobs::Row,
    /// `run_at` as "in 5 minutes" or "2 minutes ago".
    pub run_at: String,
    /// The lease's end, for a running row.
    pub lease: String,
    /// Can be made due now (queued or running).
    pub pending: bool,
}

#[derive(Template)]
#[template(path = "dev_jobs.html")]
pub struct JobsTemplate {
    pub user: Option<CurrentUser>,
    pub jobs: Vec<JobView>,
}

/// "in 5 minutes", "within a minute" or the past as [`super::super::time_ago`] says it.
pub(super) fn when(at: i64, now: i64) -> String {
    match at - now {
        d if d <= 0 => super::super::time_ago(at, now),
        d if d < 60 => "within a minute".to_string(),
        d if d < 120 => "in 1 minute".to_string(),
        d => format!("in {} minutes", d / 60),
    }
}

/// GET /dev/jobs: the newest 100 rows of the `jobs` table, newest first.
/// JSON, unless a browser asks for a page (`Accept: text/html`, or
/// `?format=html`); `?format=json` forces JSON.
pub async fn jobs(
    State(state): State<Arc<AppState>>,
    MaybeUser(user): MaybeUser,
    headers: axum::http::HeaderMap,
    Query(q): Query<FormatParams>,
) -> Result<Response, AppError> {
    let rows = state.db.call(|c| crate::jobs::recent(c, 100)).await?;
    if !wants_html(&headers, q.format.as_deref()) {
        return Ok(Json(serde_json::json!({ "jobs": rows })).into_response());
    }
    let now = crate::db::now();
    let jobs = rows
        .into_iter()
        .map(|row| JobView {
            run_at: when(row.run_at, now),
            lease: match (row.state.as_str(), row.locked_until) {
                ("running", Some(t)) => when(t, now),
                _ => String::new(),
            },
            pending: matches!(row.state.as_str(), "queued" | "running"),
            row,
        })
        .collect();
    Ok(Page(JobsTemplate { user, jobs }).into_response())
}

/// POST /dev/jobs/fail: queue a job that always fails, due now, to watch
/// its retries; then back to /dev/jobs.
pub async fn jobs_fail(State(state): State<Arc<AppState>>, headers: axum::http::HeaderMap) -> Result<Response, AppError> {
    let now = crate::db::now();
    let id = state.db.call(move |c| crate::jobs::enqueue(c, crate::jobs::DEV_FAIL.name, &serde_json::json!({}), now, now)).await?;
    tracing::info!("dev: queued failing job #{}", id.unwrap_or_default());
    state.queue.notify();
    Ok(done(&headers, "/dev/jobs"))
}

#[derive(Deserialize)]
pub struct JobAct {
    #[serde(rename = "do")]
    pub action: String,
}

/// POST /dev/jobs/{id} (`do=run_now`): a queued row is due now, a running
/// row's lease ends now (as if its server had stopped); the runner takes it
/// within a moment.
pub async fn jobs_act(State(state): State<Arc<AppState>>, headers: axum::http::HeaderMap, Path(id): Path<i64>, Form(f): Form<JobAct>) -> Result<Response, AppError> {
    if f.action != "run_now" {
        return Err(AppError::BadRequest(format!("do {:?}: run_now", f.action)));
    }
    let now = crate::db::now();
    if state.db.call(move |c| crate::jobs::dev_run_now(c, id, now)).await? == 0 {
        return Err(AppError::NotFound);
    }
    state.queue.notify();
    Ok(done(&headers, "/dev/jobs"))
}
