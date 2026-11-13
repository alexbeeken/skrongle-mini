//! The lesson table's save: rearrange, remove, add.

use super::*;

// ---------- lessons: rearrange, remove, add ----------

#[derive(Deserialize)]
pub struct SaveLessons {
    /// Every lesson row of the table, in its order.
    pub lessons: Vec<crate::arrange::Edit>,
    /// The course settings (`course.json`), when changed in the table.
    #[serde(default)]
    pub manifest: Option<String>,
    /// The placement test (`placement.json`), when changed in the table; ""
    /// when the gear's Placement test switch took it out.
    #[serde(default)]
    pub placement: Option<String>,
    /// The course title, renamed by clicking it at the top of the editor.
    #[serde(default)]
    pub title: Option<String>,
    /// Autosave: the editor page that sends it (made up by the page). Its
    /// saves share one Undo step for up to ten minutes.
    #[serde(default)]
    pub session: Option<String>,
    /// Autosave: the version of the files the page shows (`courses::version_of`).
    /// A course that changed since (another tab, an AI job) is refused.
    #[serde(default)]
    pub base: Option<String>,
}

/// One row of the table as saved, in the order the rows were sent.
#[derive(Serialize)]
pub struct SavedRow {
    pub id: String,
    pub path: String,
}

#[derive(Serialize)]
pub struct SavedLessons {
    pub notice: String,
    /// The lesson files in course order, so the page can open a new one in Edit mode.
    pub paths: Vec<String>,
    /// False when there was nothing to save (autosave only; otherwise refused).
    pub saved: bool,
    /// What the save was, for Undo ("" when nothing was saved).
    pub label: String,
    /// The rows sent, as saved: each one's lesson id and file.
    pub rows: Vec<SavedRow>,
    /// The files this save changed, with their new content (the server fills
    /// in a new lesson's unit and group, every moved lesson's order, …), so
    /// the page's copies stay the files.
    pub changed: std::collections::BTreeMap<String, String>,
    /// The files' version after the save, for the next one's `base`.
    pub version: String,
    /// Whether the course loads (with the reason when it does not).
    pub loads: bool,
    pub warning: Option<String>,
    /// The Undo line as it now stands (undo_revision.html).
    pub undo_html: Option<String>,
    /// Whether the course is still a draft that Generate can write.
    pub draft: bool,
}

/// Longest lesson name the lesson table takes (its rename box's maxlength).
const LESSON_TITLE_MAX: usize = 200;

#[derive(Template)]
#[template(source = "{% include \"undo_revision.html\" %}", ext = "html")]
struct UndoFragment {
    u: UndoView,
}

/// What a save of the table came to inside the database call.
enum Saved {
    Done { label: String, rows: Vec<SavedRow>, paths: Vec<String>, changed: std::collections::BTreeMap<String, String>, version: String, files: Vec<crate::content::CourseFile> },
    Unchanged { version: String },
    /// The page is behind: the course changed since it loaded it.
    Stale,
    Refused(crate::problems::Problem),
}

/// POST /courses/{id}/lessons and POST /api/courses/{id}/lessons
/// {lessons: [{key, unit, title, content?}], manifest?, placement?, title?, session?, base?}:
/// the lesson table as it stands. The course title, settings, order, units,
/// names, deletions, new lessons and edited JSON go in as one revision, so
/// one Undo takes it all back.
///
/// The page saves as it is edited (`session` set, static/courses.js): its
/// saves within ten minutes join one Undo step; a page that is behind
/// (`base` is not the current version) or a course an AI job holds gets
/// `409 {"error"}`; a table the course rules refuse gets
/// `422 {"errors": {"table": text}, "problems": [Problem]}` (`problems.rs`:
/// kind, file, row, lesson, field, line, message, hint) and nothing is stored; a table that
/// changes nothing is `200` with `saved: false`. Without `session` (an older
/// page, a script) every refusal is `400 {"error"}`, as before.
pub async fn save_lessons(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<String>,
    Json(body): Json<SaveLessons>,
) -> Result<Response, AppError> {
    let autosave = body.session.as_deref().is_some_and(|s| !s.trim().is_empty());
    let conflict = |e: String| (axum::http::StatusCode::CONFLICT, Json(serde_json::json!({ "error": e }))).into_response();
    let row = match owned(&state, &user, &id).await {
        Ok(row) => row,
        // Deleted in another tab, say: retrying would not help.
        Err(_) if autosave => return Ok(conflict(crate::t("courses.editor.gone"))),
        Err(_) => return Err(AppError::ApiBadRequest("not your course".into())),
    };
    if row.is_busy() {
        let busy = if row.is_generating() { crate::t("courses.editor.busy_generating") } else { crate::t("courses.editor.busy_revising") };
        let e = crate::t1("courses.editor.busy_lessons", "busy", busy);
        return if autosave { Ok(conflict(crate::t1("courses.editor.busy_reload", "error", e))) } else { Err(AppError::ApiBadRequest(e)) };
    }
    // The table's boxes have the same limits (maxlength); a request built by
    // hand is refused before anything is parsed: 422 for the table when the
    // page autosaves, as the course rules' refusals are.
    let kb = courses::MAX_FILE_BYTES / 1024;
    let too_big = if body.lessons.len() > courses::MAX_FILES {
        Some(crate::t1("courses.editor.too_many_files", "max", courses::MAX_FILES))
    } else if let Some(r) = body.lessons.iter().find(|r| r.title.chars().count() > LESSON_TITLE_MAX || r.unit.len() > LESSON_TITLE_MAX) {
        Some(crate::t2("courses.editor.lesson_name_too_long", "max", LESSON_TITLE_MAX, "title", r.title.chars().take(40).collect::<String>()))
    } else if body.lessons.iter().filter_map(|r| r.content.as_ref()).chain(body.manifest.as_ref()).chain(body.placement.as_ref()).any(|c| c.len() > courses::MAX_FILE_BYTES) {
        Some(crate::t1("courses.editor.json_too_big", "kb", kb))
    } else {
        too_many_writes(&state, &user).map(|p| p.text)
    };
    if let Some(e) = too_big {
        return if autosave {
            Ok((axum::http::StatusCode::UNPROCESSABLE_ENTITY, Json(serde_json::json!({ "errors": { "table": e } }))).into_response())
        } else {
            Err(AppError::ApiBadRequest(e))
        };
    }
    let registry = Arc::clone(&state.courses);
    let limits = state.courses.limits();
    let course_row = row.clone();
    let outcome = state
        .db
        .call(move |c| {
            let row = course_row;
            let files = courses::load_files(c, &row.id)?;
            if body.base.as_deref().is_some_and(|b| b != courses::version_of(&files)) {
                return Ok(Saved::Stale);
            }
            // A new title is one more change to the settings.
            let retitled = match body.title.as_deref() {
                Some(t) => {
                    let current = files.iter().find(|f| f.path == MANIFEST_FILE).map(|f| f.content.as_str()).unwrap_or_default();
                    match crate::arrange::retitle(body.manifest.as_deref().unwrap_or(current), t) {
                        Ok(m) => m,
                        Err(e) => return Ok(Saved::Refused(*e)),
                    }
                }
                None => None,
            };
            let manifest = retitled.as_deref().or(body.manifest.as_deref());
            let (new_files, mut label, row_paths) = match crate::arrange::edit_rows(&files, &body.lessons, manifest, body.placement.as_deref()) {
                Ok(x) => x,
                Err(e) if e.message == crate::arrange::NOTHING_TO_SAVE && body.session.is_some() => return Ok(Saved::Unchanged { version: courses::version_of(&files) }),
                Err(e) => return Ok(Saved::Refused(*e)),
            };
            if let Some(p) = courses::room_for(c, &row.id, courses::size_of(&new_files), limits)? {
                return Ok(Saved::Refused(p));
            }
            if retitled.is_some() && body.manifest.is_none() {
                let title = body.title.as_deref().unwrap_or_default().trim();
                label = if label == "Saved the course settings" { courses::quoted(&crate::t("courses.editor.renamed_course_to"), title) } else { crate::t1("courses.editor.label_renamed_course", "label", label.replace("(settings edited, ", "(")) };
            }
            let paths = crate::arrange::lessons_of(&new_files).map(|(_, l)| l.into_iter().map(|l| l.path).collect()).unwrap_or_default();
            let outline = crate::arrange::outline_for(row.outline.as_deref(), &new_files);
            let session = body.session.as_deref().filter(|s| !s.trim().is_empty());
            let joined = match session {
                Some(s) => courses::amend_session_step(c, &row.id, s, &new_files, outline.as_deref(), |before| crate::arrange::summary(before, &new_files))?,
                None => false,
            };
            if !joined {
                courses::commit_revision_in(c, &row.id, &new_files, &row.title, outline.as_deref(), &label, session)?;
            }
            let rows = row_paths
                .into_iter()
                .map(|path| {
                    let id = new_files
                        .iter()
                        .find(|f| f.path == path)
                        .and_then(|f| serde_json::from_str::<serde_json::Value>(&f.content).ok())
                        .and_then(|v| v.get("id").and_then(|i| i.as_str()).map(str::to_string))
                        .unwrap_or_default();
                    SavedRow { id, path }
                })
                .collect();
            let changed = new_files
                .iter()
                .filter(|f| !files.iter().any(|g| g.path == f.path && g.content == f.content))
                .map(|f| (f.path.clone(), f.content.clone()))
                .collect();
            // The build is slow: it happens after this call lets the database go (below).
            registry.invalidate(&row.id);
            Ok(Saved::Done { label, rows, paths, changed, version: courses::version_of(&new_files), files: new_files })
        })
        .await?;
    let (label, rows, paths, changed, version, warning) = match outcome {
        Saved::Done { label, rows, paths, changed, version, files } => {
            // Built off the database lock; a save that lands meanwhile records its own result.
            let warning = built(&state, &id, files).await?;
            (label, rows, paths, changed, version, warning)
        }
        Saved::Unchanged { version } => {
            (String::new(), Vec::new(), Vec::new(), Default::default(), version, None)
        }
        Saved::Stale => {
            return Ok(conflict(crate::t("courses.editor.stale")))
        }
        // `errors.table` is what the line under the title says (static/autosave.js);
        // `problems` place it: the row, file, field and line (static/courses.js).
        Saved::Refused(p) if autosave => {
            let body = serde_json::json!({ "errors": { "table": p.text }, "problems": [p] });
            return Ok((axum::http::StatusCode::UNPROCESSABLE_ENTITY, Json(body)).into_response());
        }
        Saved::Refused(p) => return Err(AppError::ApiBadRequest(p.text)),
    };
    let saved = !label.is_empty();
    let notice = match (&warning, saved) {
        (_, false) => String::new(),
        (None, true) => crate::t1("courses.editor.saved_undo", "label", &label),
        (Some(e), true) => crate::t2("courses.editor.saved_broken", "label", &label, "error", e),
    };
    // The page stays, so it gets what a reload would have shown.
    let row = owned(&state, &user, &id).await?;
    let undo_html = match undo_view(&state, &row, false).await? {
        Some(u) => Some(UndoFragment { u }.render()?),
        None => None,
    };
    let draft = gen_mode(&state, &row).await?.is_some();
    Ok(Json(SavedLessons { notice, paths, saved, label, rows, changed, version, loads: warning.is_none() && row.error.is_none(), warning, undo_html, draft }).into_response())
}

// ---------- paste a word list (#453) ----------

#[derive(Deserialize)]
pub struct PasteWords {
    pub text: String,
    /// The new lesson's name; without one only the preview comes back.
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Serialize)]
pub struct PastedWords {
    pub rows: Vec<crate::word_list::Pair>,
    pub skipped: Vec<crate::word_list::Skipped>,
    /// The new lesson's JSON for the lesson table to save, when a name was
    /// given and some pairs were read.
    pub lesson: Option<String>,
}

/// POST /api/courses/{id}/words {text, title?} -> the pairs read from a
/// pasted word list, the lines that were not, and (with a title) the new
/// lesson's JSON. Stores nothing: the page adds the lesson as a new row of
/// the lesson table and saves the table as usual. Owner only.
pub async fn paste_words(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<String>,
    Json(body): Json<PasteWords>,
) -> Result<Json<PastedWords>, AppError> {
    owned(&state, &user, &id).await.map_err(|_| AppError::ApiBadRequest("not your course".into()))?;
    if body.text.len() > crate::word_list::MAX_TEXT_BYTES {
        return Err(AppError::ApiBadRequest(crate::t1("courses.editor.words_too_long", "kb", crate::word_list::MAX_TEXT_BYTES / 1024)));
    }
    let title = body.title.as_deref().map(str::trim).filter(|t| !t.is_empty());
    if title.is_some_and(|t| t.chars().count() > LESSON_TITLE_MAX) {
        return Err(AppError::ApiBadRequest(crate::t2("courses.editor.lesson_name_too_long", "max", LESSON_TITLE_MAX, "title", title.unwrap_or_default().chars().take(40).collect::<String>())));
    }
    let cid = id.clone();
    let manifest = state.db.call(move |c| courses::read_file(c, &cid, MANIFEST_FILE)).await?.unwrap_or_default();
    let manifest: crate::content::Manifest =
        serde_json::from_str(&manifest).map_err(|_| AppError::ApiBadRequest(crate::t("courses.editor.words_no_settings")))?;
    let parsed = crate::word_list::parse(&body.text, &manifest);
    let lesson = title.filter(|_| !parsed.rows.is_empty()).map(|t| crate::word_list::lesson(t, &parsed.rows, &manifest));
    Ok(Json(PastedWords { rows: parsed.rows, skipped: parsed.skipped, lesson }))
}
