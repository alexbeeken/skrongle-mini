//! A course's files: the file page, upload, export, publish and delete, and their JSON API.

use super::*;

#[derive(Template)]
#[template(path = "course_file.html")]
pub struct FileTemplate {
    pub user: Option<CurrentUser>,
    pub id: String,
    pub course_title: String,
    pub path: String,
    pub content: String,
    pub is_new: bool,
    pub error: Option<String>,
}

#[derive(Deserialize)]
pub struct FileParams {
    pub path: Option<String>,
}

/// GET /courses/{id}/file?path=lessons/x.json   (no path: a new lesson file)
pub async fn file_form(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<String>,
    Query(q): Query<FileParams>,
) -> Result<Response, AppError> {
    let row = owned(&state, &user, &id).await?;
    if let Some(to) = stopped_redirect(&row, false) {
        return Ok(to);
    }
    let (path, content, is_new) = match q.path.filter(|p| !p.is_empty()) {
        Some(path) => {
            let (cid, p) = (id.clone(), path.clone());
            let content = state.db.call(move |c| courses::read_file(c, &cid, &p)).await?.ok_or(AppError::NotFound)?;
            (path, content, false)
        }
        None => (
            "lessons/new-lesson.json".to_string(),
            courses::starter_files(&id, &row.title)[1].content.clone(),
            true,
        ),
    };
    Ok(Page(FileTemplate { user: Some(user), id, course_title: row.title, path, content, is_new, error: None }).into_response())
}

#[derive(Deserialize)]
pub struct SaveForm {
    pub path: String,
    pub content: String,
}

/// POST /courses/{id}/file
pub async fn file_save(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<String>,
    Form(form): Form<SaveForm>,
) -> Result<Response, AppError> {
    let row = owned(&state, &user, &id).await?;
    if let Err(r) = not_while_generating(&row) {
        return Ok(r);
    }
    let path = form.path.trim().to_string();
    let refuse = |user: CurrentUser, id: String, title: String, path: String, content: String, e: String| {
        Page(FileTemplate { user: Some(user), id, course_title: title, path, content, is_new: false, error: Some(e) }).into_response()
    };
    if let Err(e) = check_file(&path, &form.content) {
        return Ok(refuse(user, id, row.title, path, form.content, e));
    }
    if let Some(p) = too_many_writes(&state, &user) {
        return Ok(refuse(user, id, row.title, path, form.content, p.text));
    }
    let (cid, p, content) = (id.clone(), path.clone(), form.content.clone());
    let registry = Arc::clone(&state.courses);
    let limits = state.courses.limits();
    let outcome = state
        .db
        .call(move |c| {
            if courses::file_count(c, &cid)? >= courses::MAX_FILES && courses::read_file(c, &cid, &p)?.is_none() {
                let p = crate::problems::Problem::refused(crate::t1("courses.editor.too_many_files", "max", courses::MAX_FILES)).file(&p);
                return Ok(Err(p.text));
            }
            let write = [CourseFile { path: p, content }];
            if let Some(problem) = courses::room_for(c, &cid, courses::size_after(c, &cid, &write)?, limits)? {
                return Ok(Err(problem.file(&write[0].path).text)); // panic-ok: write is a one-element array
            }
            courses::write_file(c, &cid, &write[0].path, &write[0].content)?; // panic-ok: write is a one-element array
            registry.invalidate(&cid);
            courses::load_files(c, &cid).map(Ok)
        })
        .await?;
    let error = match outcome {
        Ok(files) => built(&state, &id, files).await?,
        Err(e) => return Ok(refuse(user, id, row.title, path, form.content, e)),
    };
    Ok(to_editor(&id, if error.is_none() { "saved" } else { "saved-broken" }))
}

#[derive(Deserialize)]
pub struct DeleteFileForm {
    pub path: String,
}

/// POST /courses/{id}/file/delete
pub async fn file_delete(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<String>,
    Form(form): Form<DeleteFileForm>,
) -> Result<Response, AppError> {
    let row = owned(&state, &user, &id).await?;
    if let Err(r) = not_while_generating(&row) {
        return Ok(r);
    }
    if form.path == MANIFEST_FILE {
        return Err(AppError::BadRequest(crate::t("courses.editor.manifest_undeletable")));
    }
    if too_many_writes(&state, &user).is_some() {
        return Ok(to_editor(&id, "too-many-writes"));
    }
    let (cid, p) = (id.clone(), form.path.clone());
    let registry = Arc::clone(&state.courses);
    let files = state
        .db
        .call(move |c| {
            courses::delete_file(c, &cid, &p)?;
            registry.invalidate(&cid);
            courses::load_files(c, &cid)
        })
        .await?;
    let error = built(&state, &id, files).await?;
    Ok(to_editor(&id, if error.is_none() { "file-deleted" } else { "file-deleted-broken" }))
}

/// POST /courses/{id}/publish  -> validate again (after uploads) and report
pub async fn publish(State(state): State<Arc<AppState>>, user: CurrentUser, Path(id): Path<String>) -> Result<Response, AppError> {
    let row = owned(&state, &user, &id).await?;
    if let Err(r) = not_while_generating(&row) {
        return Ok(r);
    }
    if too_many_writes(&state, &user).is_some() {
        return Ok(to_editor(&id, "too-many-writes"));
    }
    let cid = id.clone();
    let files = state.db.call(move |c| courses::load_files(c, &cid)).await?;
    let error = built(&state, &id, files).await?;
    Ok(to_editor(&id, if error.is_none() { "loads" } else { "broken" }))
}

/// POST /courses/{id}/delete
pub async fn delete(State(state): State<Arc<AppState>>, user: CurrentUser, Path(id): Path<String>) -> Result<Response, AppError> {
    let row = owned(&state, &user, &id).await?;
    if let Err(r) = not_while_generating(&row) {
        return Ok(r);
    }
    let cid = id.clone();
    state.db.call(move |c| courses::delete_course(c, &cid)).await?;
    state.courses.invalidate(&id);
    tracing::info!("user {} deleted course {id}", user.id);
    Ok(Redirect::to(&crate::url("/courses?notice=course-deleted")).into_response())
}

/// The download's format version; the files sit beside it, by path.
const EXPORT_VERSION: &str = "1.0";

/// GET /courses/{id}/export.json  -> {"version": "1.0", path: content, …}
pub async fn export(State(state): State<Arc<AppState>>, user: CurrentUser, Path(id): Path<String>) -> Result<Response, AppError> {
    owned(&state, &user, &id).await?;
    let cid = id.clone();
    let files = state.db.call(move |c| courses::load_files(c, &cid)).await?;
    let mut map = serde_json::Map::new();
    for f in files {
        let v = serde_json::from_str(&f.content).unwrap_or(serde_json::Value::String(f.content));
        map.insert(f.path, v);
    }
    #[derive(serde::Serialize)]
    struct Export {
        version: &'static str,
        #[serde(flatten)]
        files: serde_json::Map<String, serde_json::Value>,
    }
    let body = serde_json::to_string_pretty(&Export { version: EXPORT_VERSION, files: map }).map_err(|e| AppError::Internal(e.to_string()))?;
    Ok((
        [
            (header::CONTENT_TYPE, HeaderValue::from_static("application/json; charset=utf-8")),
            (
                header::CONTENT_DISPOSITION,
                HeaderValue::from_str(&format!("attachment; filename=\"{id}.json\"")).expect("header"),
            ),
        ],
        body,
    )
        .into_response())
}

#[derive(Deserialize)]
pub struct UploadFile {
    pub path: String,
    pub content: String,
}

#[derive(Deserialize)]
pub struct UploadRequest {
    pub files: Vec<UploadFile>,
}

#[derive(Serialize)]
pub struct UploadResponse {
    pub saved: Vec<String>,
    pub rejected: Vec<String>,
    /// The course's validation error after the upload, if any.
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct FileEntry {
    pub path: String,
    pub bytes: usize,
}

#[derive(Serialize)]
pub struct FilesResponse {
    pub files: Vec<FileEntry>,
    /// False while an AI job is still writing the course.
    pub editable: bool,
    /// The course's validation error, if any.
    pub error: Option<String>,
}

/// GET /api/courses/{id}/files -> the course's file paths and sizes (owner only).
pub async fn files_api(State(state): State<Arc<AppState>>, user: CurrentUser, Path(id): Path<String>) -> Result<Json<FilesResponse>, AppError> {
    let row = owned(&state, &user, &id).await.map_err(|_| AppError::ApiBadRequest("not your course".into()))?;
    let files = state.db.call(move |c| courses::list_files(c, &id)).await?;
    Ok(Json(FilesResponse {
        files: files.into_iter().map(|(path, bytes, _)| FileEntry { path, bytes }).collect(),
        editable: !row.is_busy(),
        error: row.error,
    }))
}

#[derive(Serialize)]
pub struct FileContent {
    pub path: String,
    pub content: String,
}

#[derive(Deserialize)]
pub struct PreviewAnswer {
    /// The question as the editor holds it right now (saved or not).
    pub question: crate::content::QuizItem,
    pub answer: String,
}

#[derive(Serialize)]
pub struct PreviewVerdict {
    pub correct: bool,
    pub note: Option<String>,
    pub correct_answer: String,
    pub explanation: String,
}

/// POST /api/courses/{id}/preview-answer -> the lesson editor's "Try it":
/// grades `answer` against a question as it is being edited, with the
/// same checker every mode uses (`quiz::check_answer`), and records
/// nothing. Owner only; the question need not be saved.
pub async fn preview_answer(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<String>,
    Json(body): Json<PreviewAnswer>,
) -> Result<Json<PreviewVerdict>, AppError> {
    owned(&state, &user, &id).await.map_err(|_| AppError::ApiBadRequest("not your course".into()))?;
    if body.answer.chars().count() > 200 {
        return Err(AppError::ApiBadRequest(crate::t("quiz.api.answer_too_long")));
    }
    let verdict = crate::quiz::check_answer(&body.question, &body.answer);
    let mut body = body;
    crate::notation::fill_bar::answer_missing(std::slice::from_mut(&mut body.question));
    Ok(Json(PreviewVerdict {
        correct: verdict.correct,
        note: verdict.note,
        correct_answer: body.question.correct_answer,
        explanation: body.question.explanation,
    }))
}

#[derive(Deserialize)]
pub struct AnswerKey {
    pub question: crate::content::QuizItem,
}

#[derive(Serialize)]
pub struct AnswerKeyVerdict {
    /// The first answer the checker cannot read; `None` when all can be.
    pub answer: Option<String>,
}

/// POST /api/courses/{id}/answer-key -> whether the checker can read every
/// answer of a numeric or expression question as the lesson editor holds
/// it, the course rule a save would otherwise refuse. Owner only.
pub async fn answer_key(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<String>,
    Json(body): Json<AnswerKey>,
) -> Result<Json<AnswerKeyVerdict>, AppError> {
    owned(&state, &user, &id).await.map_err(|_| AppError::ApiBadRequest("not your course".into()))?;
    Ok(Json(AnswerKeyVerdict { answer: crate::mathcheck::validate_answers(&body.question).err().map(|(a, _)| a) }))
}

/// GET /api/courses/{id}/file?path=lessons/x.json -> one file's content (owner only).
pub async fn file_api(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<String>,
    Query(q): Query<FileParams>,
) -> Result<Json<FileContent>, AppError> {
    owned(&state, &user, &id).await.map_err(|_| AppError::ApiBadRequest("not your course".into()))?;
    let path = q.path.filter(|p| !p.is_empty()).ok_or_else(|| AppError::ApiBadRequest("give a path".into()))?;
    let p = path.clone();
    let content = state
        .db
        .call(move |c| courses::read_file(c, &id, &p))
        .await?
        .ok_or_else(|| AppError::ApiBadRequest(format!("no file {path}")))?;
    Ok(Json(FileContent { path, content }))
}

/// POST /api/courses/{id}/files { files: [{path, content}] }
/// The browser reads the chosen files and sends them here; each is checked
/// to be JSON at an allowed path, then the whole course is validated.
pub async fn upload(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<String>,
    Json(req): Json<UploadRequest>,
) -> Result<Json<UploadResponse>, AppError> {
    let row = owned(&state, &user, &id).await.map_err(|_| AppError::ApiBadRequest("not your course".into()))?;
    if row.is_busy() {
        return Err(AppError::ApiBadRequest("an AI job is writing the course; try again once it has finished".into()));
    }
    if req.files.is_empty() || req.files.len() > 100 {
        return Err(AppError::ApiBadRequest("send 1 to 100 files".into()));
    }
    let mut good: Vec<CourseFile> = Vec::new();
    let mut rejected = Vec::new();
    for f in req.files {
        let path = f.path.trim().trim_start_matches("./").to_string();
        // Accept a bare file name as a lesson (or a picture, for .svg), and
        // anything named course.json as the manifest.
        let path = if path.ends_with(MANIFEST_FILE) {
            MANIFEST_FILE.to_string()
        } else if path.contains('/') {
            path
        } else if path.ends_with(crate::content::PICTURE_EXT) {
            format!("{}{path}", crate::content::PICTURES_DIR)
        } else {
            format!("lessons/{path}")
        };
        match check_file(&path, &f.content) {
            Ok(()) => good.push(CourseFile { path, content: f.content }),
            Err(e) => rejected.push(e),
        }
    }
    if let Some(p) = too_many_writes(&state, &user) {
        return Err(AppError::ApiBadRequest(p.text));
    }
    let cid = id.clone();
    let registry = Arc::clone(&state.courses);
    let paths: Vec<String> = good.iter().map(|f| f.path.clone()).collect();
    let limits = state.courses.limits();
    // Over a limit nothing is stored: the refusal joins `rejected` and the
    // course keeps its state (#366).
    let outcome = state
        .db
        .call(move |c| match store_files(c, &cid, &good, limits)? {
            Ok(()) => {
                registry.invalidate(&cid);
                courses::load_files(c, &cid).map(Ok)
            }
            Err(refusal) => Ok(Err(refusal)),
        })
        .await?;
    match outcome {
        Ok(files) => {
            let error = built(&state, &id, files).await?;
            Ok(Json(UploadResponse { saved: paths, rejected, error }))
        }
        Err(refusal) => {
            rejected.push(refusal);
            Ok(Json(UploadResponse { saved: Vec::new(), rejected, error: row.error }))
        }
    }
}
