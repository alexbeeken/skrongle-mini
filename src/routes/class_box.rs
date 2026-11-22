//! `/class-box` (#227, `docs/features.md` "The Class box page"): one page
//! about the box itself for an account that may hold students
//! (`Perm::HoldStudents`, or an owner). A student gets a 404 through
//! `students::refused`; any other account here. Its cards, each a `<section
//! class="card">` in `class_box.html` filled from `BoxTemplate`:
//!
//! - **How students join**: the address the teacher's browser is using, and
//!   three steps. `GET /class-box/poster` is the same card alone, to print.
//! - **Right now**: counted when the page opens (`counts`), no polling.
//! - **Backup**, for an owner only: `POST /class-box/backup` (the owner's
//!   password) answers with the archive itself (`backup.rs`). Its last time
//!   is `backup::LAST_BACKUP` in `settings`.
//! - **Courses by file**: a link to Courses.

use askama::Template;
use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Form;
use rusqlite::Connection;
use std::sync::Arc;

use super::Page;
use crate::auth::CurrentUser;
use crate::backup;
use crate::error::AppError;
use crate::perms::Perm;
use crate::AppState;

/// "Signed in or studied in the last" this many seconds.
pub const ON_NOW_SECS: i64 = 10 * 60;

/// The page is a teacher's: anyone who may not hold students gets a 404.
fn teacher(user: &CurrentUser) -> Result<(), AppError> {
    if user.can(Perm::HoldStudents) { Ok(()) } else { Err(AppError::NotFound) }
}

/// The Right now card's numbers.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Counts {
    /// The students counted: the teacher's own; for an owner, every student on the box.
    pub students: i64,
    /// Of them, signed in (a new session) or studied (an answer, a lesson
    /// opened) since `since`.
    pub on_now: i64,
    /// Of them, studied (an answer, a lesson opened) since `today`.
    pub studied_today: i64,
    /// User-made courses on the box (the built-in ones are added by the caller).
    pub made_courses: i64,
}

/// Count the students of `teacher_id` (0: every student) for the Right
/// now card. Nothing records when a session was last used, and adding a
/// write to every request is not worth one number, so "on now" is the
/// latest sign-in, answer or lesson opened (`sessions.created_at`,
/// `quiz_history.answered_at`, `lesson_touches.at`), each read through an
/// index on the student.
pub fn counts(conn: &Connection, teacher_id: i64, since: i64, today: i64) -> rusqlite::Result<Counts> {
    let (students, on_now, studied_today) = conn.query_row(
        "SELECT COUNT(*),
                COALESCE(SUM(EXISTS(SELECT 1 FROM sessions s WHERE s.user_id = u.id AND s.created_at >= ?2)
                          OR EXISTS(SELECT 1 FROM quiz_history q WHERE q.user_id = u.id AND q.answered_at >= ?2)
                          OR EXISTS(SELECT 1 FROM lesson_touches t WHERE t.user_id = u.id AND t.at >= ?2)), 0),
                COALESCE(SUM(EXISTS(SELECT 1 FROM quiz_history q WHERE q.user_id = u.id AND q.answered_at >= ?3)
                          OR EXISTS(SELECT 1 FROM lesson_touches t WHERE t.user_id = u.id AND t.at >= ?3)), 0)
         FROM users u WHERE u.teacher_id IS NOT NULL AND (?1 = 0 OR u.teacher_id = ?1)",
        rusqlite::params![teacher_id, since, today],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let made_courses = conn.query_row("SELECT COUNT(*) FROM courses", [], |r| r.get(0))?;
    Ok(Counts { students, on_now, studied_today, made_courses })
}

/// Bytes as people read a disk: "11.2 GB", "850 MB".
pub fn size_text(bytes: u64) -> String {
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let gb = bytes as f64 / GB;
    if gb >= 1.0 {
        format!("{gb:.1} GB")
    } else {
        format!("{} MB", bytes >> 20)
    }
}

/// "today", "yesterday", "N days ago", in the viewer's local days.
fn days_ago(at: i64, tz: i64, now: i64) -> String {
    match (crate::earnings::day_of(tz, now) - crate::earnings::day_of(tz, at)).max(0) {
        0 => crate::t("class_box.backup_today"),
        1 => crate::t("class_box.backup_yesterday"),
        n => crate::tn("class_box.backup_days_ago", n),
    }
}

// ---------- the page ----------

/// The Backup card, for an owner.
pub struct BackupCard {
    /// "Last backup: …", or that none was made.
    pub last: String,
    /// None was made yet, or the last is a week old or more: shown in red.
    pub stale: bool,
    /// What the form asks for (`profile::ProfileView::proof_kind`):
    /// `password`, `code` (an account without a password but with two-factor
    /// sign-in) or `none`.
    pub proof: &'static str,
    /// The secret key is `SKRONGLE_SECRET_KEY`, not a file: the archive
    /// cannot hold it.
    pub key_outside: bool,
    pub error: Option<String>,
}

#[derive(Template)]
#[template(path = "class_box.html")]
pub struct BoxTemplate {
    pub user: Option<CurrentUser>,
    /// The address the teacher's browser is using (`routes::request_base`).
    pub address: String,
    pub counts: Counts,
    /// Courses on the box: built-in and made.
    pub courses: i64,
    /// Space left on the disk that holds the database; `None` when unknown.
    pub space_left: Option<String>,
    /// Counting every student (an owner) or the teacher's own.
    pub all_students: bool,
    pub backup: Option<BackupCard>,
}

#[derive(Template)]
#[template(path = "class_box_poster.html")]
pub struct PosterTemplate {
    pub user: Option<CurrentUser>,
    pub address: String,
}

async fn page(state: &AppState, user: CurrentUser, headers: &HeaderMap, backup_error: Option<String>) -> Result<BoxTemplate, AppError> {
    let now = crate::db::now();
    let today = now - (now + user.tz).rem_euclid(86_400);
    let (uid, owner, tz) = (user.id, user.owner, user.tz);
    let scope = if owner { 0 } else { uid };
    let (counts, last, proof) = state
        .db
        .call(move |c| {
            let counts = counts(c, scope, now - ON_NOW_SECS, today)?;
            if !owner {
                return Ok((counts, None, "none"));
            }
            let has_password = !crate::profile::password_hash(c, uid)?.unwrap_or_default().is_empty();
            let proof = if has_password {
                "password"
            } else if crate::two_factor::settings(c, uid)?.enabled {
                "code"
            } else {
                "none"
            };
            Ok((counts, backup::last_backup(c)?, proof))
        })
        .await?;
    let dir = match state.db_path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => std::path::PathBuf::from("."),
    };
    let space_left = crate::doctor::disk_space(&dir).ok().map(|(free, _)| size_text(free));
    let backup = owner.then(|| BackupCard {
        last: match last {
            Some(at) => crate::t1("class_box.backup_last", "when", days_ago(at, tz, now)),
            None => crate::t("class_box.backup_never"),
        },
        stale: last.is_none_or(|at| now - at >= 7 * 86_400),
        proof,
        key_outside: !crate::secrets::key_file_path(&state.db_path).is_file(),
        error: backup_error,
    });
    Ok(BoxTemplate {
        address: super::request_base(state, headers),
        courses: state.courses.builtin().len() as i64 + counts.made_courses,
        counts,
        space_left,
        all_students: owner,
        backup,
        user: Some(user),
    })
}

/// GET /class-box
pub async fn show(State(state): State<Arc<AppState>>, user: CurrentUser, headers: HeaderMap) -> Result<Response, AppError> {
    teacher(&user)?;
    Ok(Page(page(&state, user, &headers, None).await?).into_response())
}

/// GET /class-box/poster: How students join alone, in large type, to print.
pub async fn poster(State(state): State<Arc<AppState>>, user: CurrentUser, headers: HeaderMap) -> Result<Response, AppError> {
    teacher(&user)?;
    Ok(Page(PosterTemplate { address: super::request_base(&state, &headers), user: Some(user) }).into_response())
}

// ---------- the backup ----------

#[derive(serde::Deserialize)]
pub struct BackupForm {
    #[serde(default)]
    password: String,
    #[serde(default)]
    code: String,
}

/// The page again, with why the backup was not sent.
async fn refuse(state: &AppState, user: CurrentUser, headers: &HeaderMap, status: StatusCode, why: String) -> Result<Response, AppError> {
    let mut res = Page(page(state, user, headers, Some(why)).await?).into_response();
    *res.status_mut() = status;
    Ok(res)
}

/// POST /class-box/backup (password): an owner's. The owner's password
/// (`profile::prove_owner`: a wrong one counts against sign-in; an owner
/// with two-factor sign-in is asked only for the password, as for every
/// account change), then the archive as the answer, streamed
/// (`backup.rs`).
pub async fn backup(State(state): State<Arc<AppState>>, user: CurrentUser, headers: HeaderMap, Form(form): Form<BackupForm>) -> Result<Response, AppError> {
    if !user.owner {
        return Err(AppError::NotFound);
    }
    if let Some((status, why)) = super::profile::prove_owner(&state, &user, &form.password, &form.code).await? {
        return refuse(&state, user, &headers, status, why).await;
    }
    let Some(turn) = backup::Turn::take() else {
        return refuse(&state, user, &headers, StatusCode::CONFLICT, crate::t("class_box.backup_busy")).await;
    };
    let db_path = state.db_path.clone();
    let snapshot = backup::temp_path(&db_path);
    let to = snapshot.clone();
    if let Err(e) = state.db.call(move |c| backup::snapshot(c, &to)).await {
        let _ = std::fs::remove_file(&snapshot);
        tracing::warn!("backup: the database copy failed: {e}");
        return refuse(&state, user, &headers, StatusCode::INTERNAL_SERVER_ERROR, crate::t("class_box.backup_failed")).await;
    }
    let (now, uid) = (crate::db::now(), user.id);
    let entries = backup::entries(&db_path, &snapshot);
    let db = state.db.clone();
    // On the backup's own thread when it ends: no request is waiting for it.
    let done = move |sent: bool| {
        if sent {
            if let Err(e) = db.with(|c| backup::set_last_backup(c, crate::db::now())) {
                tracing::warn!("backup: its time was not kept: {e}");
            }
            tracing::info!("user {uid} downloaded a backup");
        }
    };
    let body = match backup::spawn_writer(entries, snapshot, now, turn, done) {
        Ok(body) => body,
        Err(e) => {
            tracing::warn!("backup: no thread: {e}");
            return refuse(&state, user, &headers, StatusCode::INTERNAL_SERVER_ERROR, crate::t("class_box.backup_failed")).await;
        }
    };
    let name = backup::file_name(now, user.tz);
    let mut res = axum::body::Body::from_stream(body).into_response();
    let h = res.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/gzip"));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if let Ok(v) = HeaderValue::from_str(&format!("attachment; filename=\"{name}\"")) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    Ok(res)
}

#[cfg(test)]
mod tests;
