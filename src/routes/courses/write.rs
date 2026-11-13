//! The course writes the editor and the MCP tools (#168, `src/mcp/tools.rs`)
//! share, so there is one way to write a course and one set of checks:
//! whose course it is, a file's name and text, the write limit, the
//! course limits and the write itself. The editor's handlers call these;
//! so do the tools. Nothing here knows how it was asked.

use rusqlite::Connection;

use crate::content::CourseFile;
use crate::courses::{self, CourseRow, Limits};
use crate::db::now;
use crate::problems::Problem;
use crate::AppState;

/// Course `id` when it is `user_id`'s own: a row of theirs in `courses`.
/// A built-in course has no row, a course shared with them is someone
/// else's: both are `None`, as is an id nobody has.
pub(crate) fn own_course(conn: &Connection, user_id: i64, id: &str) -> rusqlite::Result<Option<CourseRow>> {
    Ok(courses::course(conn, id)?.filter(|row| row.owner_id == user_id))
}

/// Whether account `user_id` holds Create courses now (`auth::allowed`, the
/// rule `CurrentUser::require(Perm::CreateCourses)` asks), read from its row.
/// `false` for an account that is gone.
pub(crate) fn may_create_courses(conn: &Connection, user_id: i64) -> rusqlite::Result<bool> {
    use rusqlite::OptionalExtension;
    let row = conn
        .query_row("SELECT owner, perms FROM users WHERE id = ?1", [user_id], |r| Ok((r.get::<_, bool>(0)?, r.get::<_, i64>(1)?)))
        .optional()?;
    Ok(row.is_some_and(|(owner, perms)| crate::auth::allowed(owner, crate::perms::Perms::from_bits(perms), crate::perms::Perm::CreateCourses)))
}

/// Count one write to course files by account `user_id`
/// (`security::COURSE_WRITES_PER_USER`); the refusal when that goes over
/// the limit (#366).
pub(crate) fn course_write_refused(state: &AppState, user_id: i64) -> Option<Problem> {
    let limit = crate::security::COURSE_WRITES_PER_USER;
    if state.limits.hit(limit, &user_id.to_string(), now()) {
        return None;
    }
    Some(
        Problem::refused(format!("You have changed courses {} times in {} minutes, the most an account may", limit.max, limit.window / 60))
            .hint("wait a few minutes, then save again"),
    )
}

/// A file as the file page, the upload and the MCP tools take it, before the
/// course rules (which `courses::check` asks of the whole course): an
/// allowed path (`courses::valid_path`: no `..`, no leading slash, only
/// `course.json`, `placement.json`, `lessons/…json` and `pictures/…svg`),
/// within `MAX_FILE_BYTES`, JSON (or SVG for a picture). Refusals are worded
/// as in the editor (`problems.rs`).
pub(crate) fn check_file(path: &str, content: &str) -> Result<(), String> {
    if !courses::valid_path(path) {
        let p = Problem::refused(crate::t1("courses.file.bad_path", "path", path)).hint(crate::t("courses.file.bad_path_hint"));
        return Err(p.text);
    }
    if content.len() > courses::MAX_FILE_BYTES {
        let p = Problem::refused(crate::t1("courses.file.too_large", "kb", courses::MAX_FILE_BYTES / 1024)).file(path).hint(crate::t("courses.file.too_large_hint"));
        return Err(p.text);
    }
    if crate::content::is_picture_path(path) {
        let head = content.trim_start();
        if !(head.starts_with("<svg") || (head.starts_with("<?xml") && head.contains("<svg"))) {
            return Err(Problem::refused(crate::t("courses.file.not_svg")).file(path).hint(crate::t("courses.file.not_svg_hint")).text);
        }
        return Ok(());
    }
    serde_json::from_str::<serde_json::Value>(content).map_err(|e| Problem::from_serde(&e, Some(content)).file(path).text)?;
    Ok(())
}

/// Store `files` in course `course_id` (each replacing the file at its path,
/// or added), within `MAX_FILES` and the course limits (`room_for`), all in
/// one transaction: `Err(refusal)` when a limit says no, and then nothing
/// is written. Then build the course with `built` (or `courses::check` and
/// `courses::record`), after the `Db::call` has let the database go.
pub(crate) fn store_files(conn: &mut Connection, course_id: &str, files: &[CourseFile], limits: Limits) -> rusqlite::Result<Result<(), String>> {
    if courses::file_count(conn, course_id)? + files.len() > courses::MAX_FILES {
        return Ok(Err(Problem::refused(crate::t1("courses.editor.too_many_files", "max", courses::MAX_FILES)).text));
    }
    if let Some(p) = courses::room_for(conn, course_id, courses::size_after(conn, course_id, files)?, limits)? {
        return Ok(Err(p.text));
    }
    let tx = conn.transaction()?;
    for f in files {
        courses::write_file(&tx, course_id, &f.path, &f.content)?;
    }
    tx.commit()?;
    Ok(Ok(()))
}

/// Build course `id` from `files`, what a handler's write left in it (read in
/// the same `Db::call` as the write), on a thread of its own while the
/// database is free, then record the outcome in a second short call (#84).
/// The course's error, worded in the request's language, or `None` when it
/// loads. A save that lands in between records its own result; this one's
/// is still returned, for the reply.
pub(crate) async fn built(state: &AppState, id: &str, files: Vec<CourseFile>) -> Result<Option<String>, crate::error::AppError> {
    let version = courses::version_of(&files);
    let checked = courses::check_off_lock(id.to_string(), files).await;
    let (registry, cid) = (std::sync::Arc::clone(&state.courses), id.to_string());
    state.db.call(move |c| courses::record(c, &registry, &cid, &version, checked)).await
}
