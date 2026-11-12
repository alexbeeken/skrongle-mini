//! The course tools behind `/mcp` (#168). Each is a thin layer over what
//! the course editor calls (`routes::courses`: `own_course`, `check_file`,
//! `course_write_refused`, `may_create_courses`, `store_files`,
//! `store_copy`; `courses::check` and `courses::record`), so there is one
//! way to write a course and one set of checks.
//!
//! Own courses only: a built-in course, a course shared with the account,
//! another account's course, one outside the grant's course list and an id
//! nobody has all answer with the same `not_found`. No tool starts an AI
//! job, shares, deletes, or reads progress.

use rusqlite::Connection;
use serde_json::{json, Value};
use std::sync::Arc;

use super::{format, Reply};
use crate::auth::api::ApiUser;
use crate::content::{CourseFile, MANIFEST_FILE};
use crate::courses::{self, Checked, CourseRow};
use crate::error::AppError;
use crate::routes::courses::{check_file, course_write_refused, may_create_courses, own_course, store_copy, store_files};
use crate::AppState;

/// What a tool call acts with: the server and the grant's account.
pub struct Ctx<'a> {
    pub state: &'a AppState,
    pub user: &'a ApiUser,
}

/// The id the manifest of a course not yet made is checked under.
const DRAFT_ID: &str = "new-course";

/// The page that opens course `id` for its owner (the editor): the public
/// address and the base path when `SKRONGLE_PUBLIC_URL` is set, else the
/// path alone. Never from the request's `Host`.
pub fn link(state: &AppState, id: &str) -> String {
    let path = crate::url(&format!("/courses/{}", crate::urlenc::urlencode(id)));
    match state.public_url.as_deref() {
        Some(base) => format!("{}{path}", base.trim_end_matches('/')),
        None => path,
    }
}

/// The one answer for any course this grant may not reach, so an id says
/// nothing about whose it is or whether it exists.
pub fn not_found() -> Reply {
    Reply::refused("No course of yours has that id. list_courses names the courses you may use.")
}

fn str_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

/// The course row of `id` when this grant may reach it: inside the grant's
/// course list, not built in, the account's own (`own_course`, the
/// editor's check).
fn reachable(c: &Connection, user: &ApiUser, builtin: bool, id: &str) -> rusqlite::Result<Option<CourseRow>> {
    if builtin || !user.allows_course(id) {
        return Ok(None);
    }
    own_course(c, user.id, id)
}

/// The `files` argument as course files, exactly at the paths sent (no
/// guessing: a bare name is refused by `check_file`), or the refusal for a
/// malformed one.
fn files_arg(args: &Value) -> Result<Vec<CourseFile>, Reply> {
    let Some(list) = args.get("files").and_then(Value::as_array) else {
        return Err(Reply::refused("Give \"files\": a list of {\"path\", \"content\"}."));
    };
    if list.is_empty() || list.len() > courses::MAX_FILES {
        return Err(Reply::refused(format!("Send 1 to {} files.", courses::MAX_FILES)));
    }
    let mut out: Vec<CourseFile> = Vec::with_capacity(list.len());
    for (i, f) in list.iter().enumerate() {
        let path = f.get("path").and_then(Value::as_str);
        // An agent may send the JSON itself rather than a string of it.
        let content = match f.get("content") {
            Some(Value::String(s)) => Some(s.clone()),
            Some(v @ (Value::Object(_) | Value::Array(_))) => serde_json::to_string_pretty(v).ok().map(|s| s + "\n"),
            _ => None,
        };
        match (path, content) {
            (Some(p), Some(c)) => {
                if out.iter().any(|o| o.path == p) {
                    return Err(Reply::refused(format!("File {} repeats a path sent before it.", i + 1)));
                }
                out.push(CourseFile { path: p.to_string(), content: c });
            }
            _ => return Err(Reply::refused(format!("File {} needs a \"path\" and a \"content\".", i + 1))),
        }
    }
    Ok(out)
}

/// The editor's check of each file (path, size, JSON or SVG), worded as the
/// editor words it: the refusal when any fails.
fn file_errors(files: &[CourseFile], nothing: &str) -> Option<Reply> {
    let errors: Vec<String> = files.iter().filter_map(|f| check_file(&f.path, &f.content).err()).collect();
    (!errors.is_empty()).then(|| Reply::refused_with(format!("Some files are not valid. {nothing}"), json!({ "errors": errors })))
}

/// A new course's title (its manifest's, else "Untitled course") and its
/// files with the manifest's id set to `id`, as Copy stores them
/// (`courses::copied_files`).
fn as_new(files: &[CourseFile], id: &str) -> Result<(String, Vec<CourseFile>), String> {
    let title = files
        .iter()
        .find(|f| f.path == MANIFEST_FILE)
        .and_then(|f| serde_json::from_str::<Value>(&f.content).ok())
        .and_then(|m| m.get("title").and_then(Value::as_str).map(courses::short_title))
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(courses::untitled);
    let files = courses::copied_files(files, id, &title)?;
    Ok((title, files))
}

/// `files` laid over a course's stored files (each replaces the file at its
/// path, or is added), sorted by path as `load_files` gives them.
fn merged(mut stored: Vec<CourseFile>, files: &[CourseFile]) -> Vec<CourseFile> {
    for f in files {
        match stored.iter_mut().find(|s| s.path == f.path) {
            Some(s) => s.content.clone_from(&f.content),
            None => stored.push(f.clone()),
        }
    }
    stored.sort_by(|a, b| a.path.cmp(&b.path));
    stored
}

fn not_valid(e: &str, nothing: &str) -> Reply {
    Reply::refused_with(format!("The course would not load. {nothing}"), json!({ "valid": false, "errors": [e] }))
}

fn busy() -> Reply {
    Reply::refused("An AI job is writing this course. Try again once it has finished.")
}

fn no_permission() -> Reply {
    Reply::refused("This account may not create or change courses.")
}

/// Run tool `name`; `None` when there is no such tool. The grant's scope is
/// asked here too (the HTTP gate asked it first, with a 403).
pub async fn call(ctx: &Ctx<'_>, name: &str, args: &Value) -> Result<Option<Reply>, AppError> {
    let Some(tool) = super::tool(name) else { return Ok(None) };
    if ctx.user.require(tool.scope).is_err() {
        return Ok(Some(Reply::refused(format!("insufficient_scope: this connection lacks {}.", tool.scope.as_str()))));
    }
    Ok(Some(match name {
        "get_course_format" => match format::answer(str_arg(args, "section"), str_arg(args, "file")) {
            Ok(v) => Reply::ok(v),
            Err(e) => Reply::refused(e),
        },
        "list_courses" => list_courses(ctx).await?,
        "get_course_files" => get_course_files(ctx, args).await?,
        "validate_course" => validate_course(ctx, args).await?,
        "create_course" => create_course(ctx, args).await?,
        "put_course_files" => put_course_files(ctx, args).await?,
        _ => return Ok(None),
    }))
}

async fn list_courses(ctx: &Ctx<'_>) -> Result<Reply, AppError> {
    let user = ctx.user.clone();
    let registry = Arc::clone(&ctx.state.courses);
    let rows = ctx
        .state
        .db
        .call(move |c| {
            let mut out = Vec::new();
            for row in courses::courses_owned_by(c, user.id)? {
                if registry.is_builtin(&row.id) || !user.allows_course(&row.id) {
                    continue;
                }
                let lessons = courses::list_files(c, &row.id)?.iter().filter(|(p, _, _)| crate::arrange::is_lesson_path(p)).count();
                let manifest = courses::read_file(c, &row.id, MANIFEST_FILE)?.and_then(|m| serde_json::from_str::<Value>(&m).ok());
                out.push((row, lessons, manifest));
            }
            Ok(out)
        })
        .await?;
    let list: Vec<Value> = rows
        .into_iter()
        .map(|(row, lessons, manifest)| {
            let m = manifest.unwrap_or(Value::Null);
            let terms = m.get("terms");
            json!({
                "id": row.id,
                "title": row.title,
                "language": terms.and_then(|t| t.get("lang")).and_then(Value::as_str),
                "language_label": terms.and_then(|t| t.get("label")).and_then(Value::as_str),
                "lessons": lessons,
                "state": row.state.as_str(),
                "loads": row.error.is_none(),
                "error": row.error,
                "link": link(ctx.state, &row.id),
            })
        })
        .collect();
    Ok(Reply::ok(json!({ "courses": list })))
}

async fn get_course_files(ctx: &Ctx<'_>, args: &Value) -> Result<Reply, AppError> {
    let Some(id) = str_arg(args, "course_id").map(str::to_string) else {
        return Ok(Reply::refused("Give the \"course_id\" (list_courses names yours)."));
    };
    let wanted: Option<Vec<String>> = args.get("paths").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect());
    let (user, cid, builtin) = (ctx.user.clone(), id.clone(), ctx.state.courses.is_builtin(&id));
    let found = ctx
        .state
        .db
        .call(move |c| match reachable(c, &user, builtin, &cid)? {
            Some(row) => Ok(Some((row, courses::load_files(c, &cid)?))),
            None => Ok(None),
        })
        .await?;
    let Some((row, files)) = found else { return Ok(not_found()) };
    let missing: Vec<&String> = wanted.iter().flatten().filter(|p| !files.iter().any(|f| &f.path == *p)).collect();
    let files: Vec<Value> = files
        .iter()
        .filter(|f| wanted.as_ref().is_none_or(|w| w.contains(&f.path)))
        .map(|f| json!({ "path": f.path, "content": f.content }))
        .collect();
    Ok(Reply::ok(json!({
        "course_id": id,
        "title": row.title,
        "state": row.state.as_str(),
        "loads": row.error.is_none(),
        "error": row.error,
        "files": files,
        "missing": missing,
        "link": link(ctx.state, &id),
    })))
}

async fn validate_course(ctx: &Ctx<'_>, args: &Value) -> Result<Reply, AppError> {
    const NOTHING: &str = "Nothing was stored.";
    let id = str_arg(args, "course_id").map(str::to_string);
    let files = match (args.get("files"), &id) {
        (Some(_), _) => match files_arg(args) {
            Ok(f) => f,
            Err(r) => return Ok(r),
        },
        (None, Some(_)) => Vec::new(),
        (None, None) => return Ok(Reply::refused("Give \"files\", a \"course_id\", or both.")),
    };
    if let Some(r) = file_errors(&files, NOTHING) {
        return Ok(r);
    }
    let (check_id, whole) = match id {
        Some(cid) => {
            let (user, c_id, builtin) = (ctx.user.clone(), cid.clone(), ctx.state.courses.is_builtin(&cid));
            let stored = ctx
                .state
                .db
                .call(move |c| match reachable(c, &user, builtin, &c_id)? {
                    Some(_) => courses::load_files(c, &c_id).map(Some),
                    None => Ok(None),
                })
                .await?;
            match stored {
                Some(stored) => (cid, merged(stored, &files)),
                None => return Ok(not_found()),
            }
        }
        None => match as_new(&files, DRAFT_ID) {
            Ok((_, files)) => (DRAFT_ID.to_string(), files),
            Err(e) => return Ok(not_valid(&e, NOTHING)),
        },
    };
    let n = whole.len();
    Ok(match courses::check_off_lock(check_id, whole).await {
        Checked::Built(_) => Reply::ok(json!({ "valid": true, "files": n })),
        Checked::Failed(e) => not_valid(&e, NOTHING),
    })
}

async fn create_course(ctx: &Ctx<'_>, args: &Value) -> Result<Reply, AppError> {
    const NOTHING: &str = "Nothing was stored.";
    if !ctx.user.courses.is_empty() {
        return Ok(Reply::refused("This connection was given a list of courses to work on, so it cannot make a new one."));
    }
    let files = match files_arg(args) {
        Ok(f) => f,
        Err(r) => return Ok(r),
    };
    if let Some(r) = file_errors(&files, NOTHING) {
        return Ok(r);
    }
    // Checked whole, in memory, before anything is stored; Copy's
    // `store_copy` then gives it its id and stores it.
    let (title, draft) = match as_new(&files, DRAFT_ID) {
        Ok(x) => x,
        Err(e) => return Ok(not_valid(&e, NOTHING)),
    };
    if let Checked::Failed(e) = courses::check_off_lock(DRAFT_ID.to_string(), draft).await {
        return Ok(not_valid(&e, NOTHING));
    }
    let uid = ctx.user.id;
    if !ctx.state.db.call(move |c| may_create_courses(c, uid)).await? {
        return Ok(no_permission());
    }
    if let Some(p) = course_write_refused(ctx.state, uid) {
        return Ok(Reply::refused(p.text));
    }
    match store_copy(ctx.state, uid, &files, &title).await? {
        Ok(id) => {
            tracing::info!("mcp: account {uid} grant {} create_course course {id}", ctx.user.grant_id);
            Ok(Reply::ok(json!({ "course_id": id, "title": title, "link": link(ctx.state, &id) })))
        }
        Err(no) => Ok(Reply::refused(no.text)),
    }
}

/// What `put_course_files` found in a database call.
enum Put {
    Ready(String, Vec<CourseFile>),
    Done,
    Refused(Reply),
}

async fn put_course_files(ctx: &Ctx<'_>, args: &Value) -> Result<Reply, AppError> {
    const NOTHING: &str = "Nothing was changed.";
    let Some(id) = str_arg(args, "course_id").map(str::to_string) else {
        return Ok(Reply::refused("Give the \"course_id\" (list_courses names yours)."));
    };
    let files = match files_arg(args) {
        Ok(f) => f,
        Err(r) => return Ok(r),
    };
    if let Some(r) = file_errors(&files, NOTHING) {
        return Ok(r);
    }
    let builtin = ctx.state.courses.is_builtin(&id);
    // Whose it is, and the whole course with the new files, read together.
    let (user, cid, sent) = (ctx.user.clone(), id.clone(), files.clone());
    let first = ctx
        .state
        .db
        .call(move |c| {
            let Some(row) = reachable(c, &user, builtin, &cid)? else { return Ok(Put::Refused(not_found())) };
            if !may_create_courses(c, user.id)? {
                return Ok(Put::Refused(no_permission()));
            }
            if row.is_busy() {
                return Ok(Put::Refused(busy()));
            }
            let stored = courses::load_files(c, &cid)?;
            Ok(Put::Ready(courses::version_of(&stored), merged(stored, &sent)))
        })
        .await?;
    let (before, whole) = match first {
        Put::Ready(v, whole) => (v, whole),
        Put::Refused(r) => return Ok(r),
        Put::Done => return Err(AppError::Internal("put_course_files: no course read".into())),
    };
    // Built off the database lock, as the editor's autosave builds.
    let after = courses::version_of(&whole);
    let checked = courses::check_off_lock(id.clone(), whole).await;
    if let Checked::Failed(e) = &checked {
        return Ok(not_valid(e, NOTHING));
    }
    if let Some(p) = course_write_refused(ctx.state, ctx.user.id) {
        return Ok(Reply::refused(p.text));
    }
    let saved: Vec<String> = files.iter().map(|f| f.path.clone()).collect();
    let (user, cid, limits, registry) = (ctx.user.clone(), id.clone(), ctx.state.courses.limits(), Arc::clone(&ctx.state.courses));
    let outcome = ctx
        .state
        .db
        .call(move |c| {
            let Some(row) = reachable(c, &user, builtin, &cid)? else { return Ok(Put::Refused(not_found())) };
            if row.is_busy() {
                return Ok(Put::Refused(busy()));
            }
            if courses::version_of(&courses::load_files(c, &cid)?) != before {
                return Ok(Put::Refused(Reply::refused(format!("The course changed while it was being checked. {NOTHING} Try again."))));
            }
            if let Err(refusal) = store_files(c, &cid, &files, limits)? {
                return Ok(Put::Refused(Reply::refused(format!("{refusal} {NOTHING}"))));
            }
            courses::record(c, &registry, &cid, &after, checked)?;
            Ok(Put::Done)
        })
        .await?;
    match outcome {
        Put::Done => {
            tracing::info!("mcp: account {} grant {} put_course_files course {id}", ctx.user.id, ctx.user.grant_id);
            Ok(Reply::ok(json!({ "course_id": id, "saved": saved, "link": link(ctx.state, &id) })))
        }
        Put::Refused(r) => Ok(r),
        Put::Ready(..) => Err(AppError::Internal("put_course_files: no write".into())),
    }
}
