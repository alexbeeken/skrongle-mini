//! "Understand a document" (#232): the course list's second way to start.
//! The learner pastes a document and picks the language to explain it in;
//! a new private course holds it in its `course.json` (`document`) and an
//! AI job of the kind `SubjectKind::Document` (`ai/document.rs`) writes the
//! lessons, like Generate with AI: the same slot, limits and progress modal.
//! Such a course is never shared, copied or translated (`is_document`).

use super::*;
use crate::perms::Perm;

#[derive(Deserialize)]
pub struct DocumentForm {
    #[serde(default)]
    pub text: String,
    /// The app language to explain it in (`i18n/<code>.yaml`).
    #[serde(default)]
    pub lang: String,
    /// A provider offered on the dialog (one with a saved key).
    #[serde(default)]
    pub provider: String,
}

/// Does this `course.json` hold a document? Such a course stays its owner's
/// alone: no share link, no copy, no translated copy.
pub fn is_document(manifest: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(manifest)
        .ok()
        .and_then(|m| m.get("document").and_then(|d| d.as_str()).map(|d| !d.trim().is_empty()))
        .unwrap_or(false)
}

/// Is course `id` (one made in the app) a document course? Read from its
/// stored `course.json`, so a course that does not load is told apart too.
pub(crate) async fn is_document_course(state: &AppState, id: &str) -> Result<bool, AppError> {
    let cid = id.to_string();
    let manifest: Option<String> = state
        .db
        .call(move |c| {
            rusqlite::OptionalExtension::optional(c.query_row(
                "SELECT content FROM course_files WHERE course_id = ?1 AND path = ?2",
                rusqlite::params![cid, MANIFEST_FILE],
                |r| r.get(0),
            ))
        })
        .await?;
    Ok(manifest.as_deref().is_some_and(is_document))
}

/// The first `course.json` of a document course: a placeholder title the
/// job replaces, one unit, the explaining language on both sides and the
/// document itself.
fn first_manifest(id: &str, title: &str, document: &str, lang: &str) -> String {
    let manifest = serde_json::json!({
        "id": id,
        "title": title,
        "icon": "📄",
        "terms": { "label": "Term", "lang": lang },
        "meanings": { "label": "Meaning", "lang": lang },
        "language_features": false,
        "units": [{ "id": "1", "title": title }],
        "groups": [{ "id": "core", "title": title }],
        "document": document,
    });
    serde_json::to_string_pretty(&manifest).unwrap_or_default() + "\n"
}

/// Why the pasted `text` (trimmed, line ends as `\n`) is refused, if it is.
fn text_problem(text: &str) -> Option<String> {
    let n = text.chars().count();
    if n < ai::MIN_PROMPT {
        return Some(crate::t1("courses.document.too_short", "min", ai::MIN_PROMPT));
    }
    (n > crate::content::MAX_DOCUMENT_CHARS).then(|| crate::t1("courses.document.too_long", "max", ai::grouped(crate::content::MAX_DOCUMENT_CHARS)))
}

/// POST /courses/document: the dialog's "Make the course". Needs the Create
/// courses permission and AI courses, like Create a course and Generate.
pub async fn document_in(State(state): State<Arc<AppState>>, user: CurrentUser, Form(form): Form<DocumentForm>) -> Result<Response, AppError> {
    user.require(Perm::CreateCourses)?;
    user.require_ai_courses()?;
    let courses_notice = |key: &str| Redirect::to(&crate::url(&format!("/courses?notice={key}"))).into_response();
    // Browsers send textareas with CRLF line ends.
    let text = form.text.replace("\r\n", "\n").trim().to_string();
    if let Some(e) = text_problem(&text) {
        return picker_page(&state, user, Some(e), None).await;
    }
    let Some(lang) = crate::i18n::language(&form.lang).map(|l| l.code.clone()) else {
        return picker_page(&state, user, Some(crate::t("courses.document.pick_language")), None).await;
    };
    let mut provider = match ai_provider(&state, &user, &form.provider, "", "", "").await? {
        Ok(p) => p,
        Err(e) => return picker_page(&state, user, Some(e), None).await,
    };
    if let Some(e) = ai::check_capacity(&state.jobs, user.id).err().or_else(|| ai_quota_problem(&state, user.id)) {
        return picker_page(&state, user, Some(e), None).await;
    }
    if too_many_writes(&state, &user).is_some() {
        return Ok(courses_notice("too-many-writes"));
    }
    provider.effort = Some(effort_of(&state, user.id).await?);

    let title = crate::t("courses.document.provisional_title");
    let id = courses::new_course_id(&title);
    let files = vec![CourseFile { path: MANIFEST_FILE.into(), content: first_manifest(&id, &title, &text, &lang) }];
    let size = courses::size_of(&files);
    let job = ai::new_job_id();
    let limits = state.courses.limits();
    let (uid, cid, t, j) = (user.id, id.clone(), title.clone(), job.clone());
    let stored = state
        .db
        .call(move |c| {
            if courses::room_for_course(c, uid, limits)?.is_some() {
                return Ok(Err(format!("course-count-{}", limits.max_courses)));
            }
            if courses::room_for_copy(c, uid, size, limits)?.is_some() {
                return Ok(Err(format!("copy-no-room-{}", limits.account_bytes)));
            }
            courses::create_course(c, uid, &cid, &t, &files)?;
            // An AI course from the start: greyed out in the list until its job ends.
            c.execute("UPDATE courses SET source = 'ai', brief = ?2 WHERE id = ?1", rusqlite::params![cid, t])?;
            courses::set_generating(c, &cid, &j)?;
            Ok(Ok(()))
        })
        .await?;
    if let Err(key) = stored {
        return Ok(courses_notice(&key));
    }
    let req = ai::Request {
        user_id: user.id,
        provider,
        course_id: id.clone(),
        wish: title,
        lesson_count: ai::MAX_DOCUMENT_LESSONS,
        depth: ai::Depth::Full,
        notes: String::new(),
        kind: ai::SubjectKind::Document,
        instructions: String::new(),
        resume_outline: None,
        revise: None,
        add_lesson: None,
        edit: None,
        translate: None,
        named: courses::Draft::default(),
        i18n: crate::i18n::snapshot(),
    };
    match ai::submit(&state, req, job, true).await {
        Ok(job) => {
            count_ai_job(&state, user.id);
            tracing::info!("user {} started a document course {job} as {id}", user.id);
            Ok(Redirect::to(&crate::url(&format!("/courses/{id}?job={job}"))).into_response())
        }
        Err(e) => {
            let (cid, msg) = (id.clone(), e.clone());
            state.db.call(move |c| courses::finish_generation(c, &cid, Some(&msg))).await?;
            picker_page(&state, user, Some(e), None).await
        }
    }
}

#[derive(Template)]
#[template(path = "document.html")]
pub struct DocumentTemplate {
    pub user: Option<CurrentUser>,
    pub course: std::sync::Arc<crate::routes::CourseView>,
    pub title: String,
    pub document: String,
}

/// GET /document: the whole document of the course the learner is in, one
/// click from every lesson of a document course. 404 for any other course.
pub async fn document_page(p: crate::auth::Project) -> Result<Response, AppError> {
    let document = p.course.manifest().document.trim().to_string();
    if document.is_empty() {
        return Err(AppError::NotFound);
    }
    Ok(Page(DocumentTemplate { course: p.course.view(), title: p.course.title().to_string(), user: Some(p.user), document }).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_document_course_is_told_by_its_manifest() {
        assert!(is_document(&first_manifest("d", "Doc", "Take one tablet a day.", "en")));
        assert!(!is_document(r#"{"id": "d", "document": "  "}"#));
        assert!(!is_document(r#"{"id": "d"}"#));
        assert!(!is_document("not json"));
        let m: crate::content::Manifest = serde_json::from_str(&first_manifest("d", "Doc", "Take one tablet a day.", "fr")).unwrap();
        assert_eq!((m.meanings.lang.as_str(), m.is_language()), ("fr", false));
    }

    #[test]
    fn the_pasted_text_has_a_floor_and_a_cap() {
        assert!(text_problem("too short").is_some());
        assert!(text_problem(&"a".repeat(ai::MIN_PROMPT)).is_none());
        assert!(text_problem(&"a".repeat(crate::content::MAX_DOCUMENT_CHARS)).is_none());
        assert!(text_problem(&"a".repeat(crate::content::MAX_DOCUMENT_CHARS + 1)).unwrap().contains("30,000"));
    }
}
