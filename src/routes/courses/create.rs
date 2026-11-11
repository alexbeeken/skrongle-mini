//! Making a course: a new one, a copy, and a copy translated by AI.

use super::*;
use crate::perms::Perm;

// ---------- create a course ----------

#[derive(Deserialize)]
pub struct NewForm {
    /// Optional: the course is `courses::untitled()` ("Untitled course") until renamed in the editor.
    #[serde(default)]
    pub title: String,
}

/// GET /courses/new: the old title page. Creating is a POST from /courses.
pub async fn new_form(_user: CurrentUser) -> Response {
    Redirect::to(&crate::url("/courses")).into_response()
}

/// POST /courses/new: "Create a course". A starter course straight away,
/// then the editor with the "Generate the course" dialog open. Like Copy
/// and Copy and translate below, it needs the Create courses permission
/// (#143): without it the route answers 403.
pub async fn create(State(state): State<Arc<AppState>>, user: CurrentUser, Form(form): Form<NewForm>) -> Result<Response, AppError> {
    user.require(Perm::CreateCourses)?;
    let title = match form.title.trim() {
        "" => courses::untitled(),
        t if t.chars().count() > 80 => return Ok(Redirect::to(&crate::url("/courses?notice=title-too-long")).into_response()),
        t => t.to_string(),
    };
    // Notices are keys (#385); the counts in them are digits.
    let courses_notice = |key: &str| Redirect::to(&crate::url(&format!("/courses?notice={key}"))).into_response();
    if too_many_writes(&state, &user).is_some() {
        return Ok(courses_notice("too-many-writes"));
    }
    let id = courses::new_course_id(&title);
    let files = courses::starter_files(&id, &title);
    let uid = user.id;
    let (cid, t) = (id.clone(), title.clone());
    let registry = Arc::clone(&state.courses);
    let limits = state.courses.limits();
    let refused = state
        .db
        .call(move |c| {
            if let Some(p) = courses::room_for_course(c, uid, limits)? {
                return Ok(Some(p));
            }
            courses::create_course(c, uid, &cid, &t, &files)?;
            courses::revalidate(c, &registry, &cid)?;
            Ok(None)
        })
        .await?;
    if refused.is_some() {
        return Ok(courses_notice(&format!("course-count-{}", state.courses.limits().max_courses)));
    }
    tracing::info!("user {} created course {id}", user.id);
    Ok(Redirect::to(&crate::url(&format!("/courses/{id}?generate=1"))).into_response())
}

// ---------- copy a course ----------

/// What a copy of course `id` is made from (#629): its files and title, or
/// the course list's notice key saying why it cannot be copied now. A
/// built-in course or one of the user's own; anything else is a 404.
async fn copy_source(state: &AppState, user: &CurrentUser, id: &str) -> Result<Result<(Vec<CourseFile>, String), &'static str>, AppError> {
    if let Some(title) = state.courses.builtin_by_id(id).map(|c| c.title().to_string()) {
        let (registry, cid) = (Arc::clone(&state.courses), id.to_string());
        let read = tokio::task::spawn_blocking(move || registry.builtin_files(&cid)).await.map_err(|e| AppError::Internal(e.to_string()))?;
        let files = read.ok_or(AppError::NotFound)?.map_err(|e| AppError::Internal(e.to_string()))?;
        return Ok(Ok((files, title)));
    }
    let row = owned(state, user, id).await?;
    // Half written, or stopped half way: nothing whole to copy yet.
    if matches!(row.state, courses::CourseState::Generating | courses::CourseState::Failed) {
        return Ok(Err("copy-not-ready"));
    }
    let cid = id.to_string();
    let files = state.db.call(move |c| courses::load_files(c, &cid)).await?;
    Ok(Ok((files, row.title)))
}

/// Store `files` as a new private course of `user`'s called `title`, within
/// the course limits (`Limits`): the new course's id, or the course list's
/// notice key of the limit it would pass. Nothing of the learner's progress
/// goes with it (#347).
pub(in crate::routes) async fn store_copy(state: &AppState, user: &CurrentUser, files: &[CourseFile], title: &str) -> Result<Result<String, String>, AppError> {
    store_copy_as(state, user, files, title, None).await
}

/// `store_copy`, handed straight to the translation job `job` with its
/// plan (#630) when `translation` says so: then it is never seen ready
/// in the original language.
async fn store_copy_as(
    state: &AppState,
    user: &CurrentUser,
    files: &[CourseFile],
    title: &str,
    translation: Option<(String, String)>,
) -> Result<Result<String, String>, AppError> {
    let id = courses::new_course_id(title);
    let files = courses::copied_files(files, &id, title).map_err(AppError::Internal)?;
    let size = courses::size_of(&files);
    let limits = state.courses.limits();
    let (uid, cid, t) = (user.id, id.clone(), title.to_string());
    let registry = Arc::clone(&state.courses);
    let refused = state
        .db
        .call(move |c| {
            if courses::room_for_course(c, uid, limits)?.is_some() {
                return Ok(Some(format!("course-count-{}", limits.max_courses)));
            }
            if courses::room_for_copy(c, uid, size, limits)?.is_some() {
                return Ok(Some(if size > limits.course_bytes {
                    format!("copy-too-big-{}", limits.course_bytes)
                } else {
                    format!("copy-no-room-{}", limits.account_bytes)
                }));
            }
            courses::create_course(c, uid, &cid, &t, &files)?;
            match &translation {
                Some((job, plan)) => courses::start_translation(c, &cid, job, plan)?,
                None => {
                    courses::revalidate(c, &registry, &cid)?;
                }
            }
            Ok(None)
        })
        .await?;
    Ok(match refused {
        Some(key) => Err(key),
        None => Ok(id),
    })
}

/// POST /courses/{id}/copy: "Copy" on a course card (#629). A new private
/// course of the user's own with every file of the original, called "Copy
/// of <title>", opened in the editor. The original is not touched.
pub async fn copy(State(state): State<Arc<AppState>>, user: CurrentUser, Path(id): Path<String>) -> Result<Response, AppError> {
    user.require(Perm::CreateCourses)?;
    let courses_notice = |key: &str| Redirect::to(&crate::url(&format!("/courses?notice={key}"))).into_response();
    let (files, title) = match copy_source(&state, &user, &id).await? {
        Ok(source) => source,
        Err(key) => return Ok(courses_notice(key)),
    };
    if too_many_writes(&state, &user).is_some() {
        return Ok(courses_notice("too-many-writes"));
    }
    let title = courses::short_title(&crate::t1("courses.picker.copy_title", "title", title.as_str()));
    match store_copy(&state, &user, &files, &title).await? {
        Ok(new_id) => {
            tracing::info!("user {} copied course {id} as {new_id}", user.id);
            Ok(Redirect::to(&crate::url(&format!("/courses/{new_id}"))).into_response())
        }
        Err(key) => Ok(courses_notice(&key)),
    }
}

// ---------- copy and translate a course ----------

#[derive(Deserialize)]
pub struct TranslateForm {
    /// The app language to translate into (`i18n/<code>.yaml`); a resume
    /// keeps its own.
    #[serde(default)]
    pub lang: String,
    /// A provider offered on the dialog (`translate_providers`).
    #[serde(default)]
    pub provider: String,
}

/// POST /courses/{id}/translate: "Copy and translate to…" (#630). A copy of
/// the course (as Copy makes it, #629) called "<title> (<language>)", which
/// an AI job translates file by file into the chosen language, like a
/// generation: the editor follows it in its progress modal, the course list
/// greys the card out, Cancel stops it. On a stopped translation of the
/// user's own it resumes with the files not done yet. The original is
/// never changed. It runs on the learner's own key, like generation.
pub async fn translate(State(state): State<Arc<AppState>>, user: CurrentUser, Path(id): Path<String>, Form(form): Form<TranslateForm>) -> Result<Response, AppError> {
    user.require(Perm::CreateCourses)?;
    user.require_ai()?;
    let courses_notice = |key: &str| Redirect::to(&crate::url(&format!("/courses?notice={key}"))).into_response();
    // A stopped translation of their own resumes; any other course is copied.
    let resume = if state.courses.is_builtin(&id) {
        None
    } else {
        let row = owned(&state, &user, &id).await?;
        (row.source == "translate" && row.state == courses::CourseState::Failed).then_some(row)
    };
    let mut copy = None;
    let plan = match &resume {
        Some(row) => {
            let cid = row.id.clone();
            let notes = state.db.call(move |c| courses::notes_of(c, &cid)).await?;
            match crate::translate::Plan::parse(&notes) {
                Some(p) => p,
                None => return Ok(courses_notice("translate-no-plan")),
            }
        }
        None => {
            let Some(target) = crate::i18n::language(&form.lang) else { return Err(AppError::BadRequest(crate::t("courses.translate.pick_language"))) };
            let (files, title) = match copy_source(&state, &user, &id).await? {
                Ok(source) => source,
                Err(key) => return Ok(courses_notice(key)),
            };
            let manifest = files
                .iter()
                .find(|f| f.path == MANIFEST_FILE)
                .and_then(|f| serde_json::from_str::<crate::content::Manifest>(&f.content).ok())
                .ok_or_else(|| AppError::Internal(format!("course {id} has no manifest to translate")))?;
            if manifest.meanings.lang.trim().eq_ignore_ascii_case(&target.code) {
                return Ok(courses_notice("translate-same"));
            }
            let title = courses::short_title(&crate::t2("courses.translate.course_title", "title", title.as_str(), "language", target.name.as_str()));
            let plan = crate::translate::Plan {
                from: id.clone(),
                lang: target.code.clone(),
                language_course: manifest.is_language(),
                taught: manifest.terms.label.clone(),
                taught_lang: manifest.terms.lang.clone(),
                learner: manifest.meanings.label.clone(),
                learner_lang: manifest.meanings.lang.clone(),
                done: Vec::new(),
            };
            copy = Some((files, title));
            plan
        }
    };
    // The same checks as Generate, before anything is made.
    let provider = match ai_provider(&state, &user, &form.provider, "", "", "").await? {
        Ok(p) => p,
        Err(e) => return picker_page(&state, user, Some(e), None).await,
    };
    if let Some(e) = ai::check_capacity(&state.jobs, user.id).err().or_else(|| ai_quota_problem(&state, user.id)) {
        return picker_page(&state, user, Some(e), None).await;
    }
    if too_many_writes(&state, &user).is_some() {
        return Ok(courses_notice("too-many-writes"));
    }
    // One request per file still to translate: the manifest and the lessons.
    let files_left = match (&copy, &resume) {
        (Some((files, _)), _) => files.iter().filter(|f| f.path == MANIFEST_FILE || crate::arrange::is_lesson_path(&f.path)).count(),
        (None, Some(row)) => {
            let cid = row.id.clone();
            let paths = state.db.call(move |c| courses::list_files(c, &cid)).await?;
            paths.iter().filter(|(p, _, _)| (p == MANIFEST_FILE || crate::arrange::is_lesson_path(p)) && !plan.done.contains(p)).count()
        }
        (None, None) => 0,
    };
    let job = ai::new_job_id();
    let course_id = match (copy, &resume) {
        (Some((files, title)), _) => match store_copy_as(&state, &user, &files, &title, Some((job.clone(), plan.to_json()))).await? {
            Ok(new_id) => new_id,
            Err(key) => return Ok(courses_notice(&key)),
        },
        (None, Some(row)) => {
            let (cid, j) = (row.id.clone(), job.clone());
            state.db.call(move |c| courses::set_generating(c, &cid, &j)).await?;
            row.id.clone()
        }
        (None, None) => return Err(AppError::NotFound),
    };
    state.courses.invalidate(&course_id);
    let req = ai::Request {
        user_id: user.id,
        provider,
        course_id: course_id.clone(),
        wish: String::new(),
        lesson_count: files_left,
        depth: ai::Depth::Full,
        notes: String::new(),
        kind: ai::SubjectKind::Auto,
        instructions: String::new(),
        resume_outline: None,
        revise: None,
        add_lesson: None,
        edit: None,
        translate: Some(ai::Translation { plan }),
        named: courses::Draft::default(),
        i18n: crate::i18n::snapshot(),
    };
    match ai::submit(&state, req, job.clone(), false).await {
        Ok(job) => {
            count_ai_job(&state, user.id);
            tracing::info!("user {} started translation {job} of {id} as {course_id}", user.id);
            Ok(Redirect::to(&crate::url(&format!("/courses/{course_id}?job={job}"))).into_response())
        }
        Err(e) => {
            let (cid, msg) = (course_id.clone(), e.clone());
            state.db.call(move |c| courses::finish_generation(c, &cid, Some(&msg))).await?;
            picker_page(&state, user, Some(e), None).await
        }
    }
}
